//! Host side of the realm: the document and the per-page JavaScript state,
//! reachable from native functions through a thread-local (Boa natives are
//! plain function pointers). Ports the parts of WebCore's `ScriptController`,
//! `DOMTimer` and `Console` that wkhtmltopdf observes, plus `MyQWebPage`'s
//! JavaScript hooks (`src/lib/multipageloader.cc`).
//!
//! Rules:
//! - [`with_host`] borrows the host exclusively: never call back into
//!   JavaScript inside it.
//! - [`with_services`] lends the loader/feedback of the caller for the
//!   duration of one call into JavaScript.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use blitz_dom::{BaseDocument, NodeId};
use boa_engine::context::time::{Clock, JsInstant};
use boa_engine::error::{EngineError, RuntimeLimitError};
use boa_engine::{Context, JsError, JsNativeErrorKind, JsObject, JsString, JsValue, Source};
use parley::FontContext;
use url::Url;

use crate::canvas::CanvasState;
use crate::fnlines::FunctionLines;
use crate::JsServices;

/// A pending `setTimeout`/`setInterval`.
pub(crate) struct Timer {
    pub id: u32,
    pub due: f64,
    pub seq: u64,
    pub interval: Option<f64>,
    pub callback: JsValue,
    pub args: Vec<JsValue>,
}

/// One evaluated source: where its lines land in the document.
pub(crate) struct ScriptInfo {
    /// `sourceURL` as QtWebKit reports it.
    pub url: String,
    /// Document line of the first line of the source.
    pub first_line: u32,
    /// Start lines of the functions of the source (computed on first use).
    pub functions: Option<FunctionLines>,
    pub code: Rc<str>,
}

/// Virtual clock shared by the timers and `Date` (DESIGN.md "Determinism").
pub(crate) struct VirtualClock {
    /// Wall-clock time at navigation start, ms since the epoch.
    pub start_wall_ms: i64,
    /// Virtual ms since navigation start.
    pub now: Cell<f64>,
}

impl Clock for VirtualClock {
    fn now(&self) -> JsInstant {
        let ms = self.now.get().max(0.0);
        JsInstant::new((ms / 1000.0) as u64, ((ms % 1000.0) * 1_000_000.0) as u32)
    }

    fn system_time_millis(&self) -> i64 {
        self.start_wall_ms + self.now.get() as i64
    }
}

pub(crate) struct Host {
    pub doc: BaseDocument,
    pub url: Url,
    pub debug_js: bool,
    pub stop_slow_scripts: bool,
    pub media_print: bool,
    pub font_ctx: FontContext,
    /// Wrapper identity: one JS object per node.
    pub cache: HashMap<NodeId, JsObject>,
    pub protos: Option<crate::dom::Protos>,
    /// Carrier elements standing for `DocumentFragment`s.
    pub fragments: HashSet<NodeId>,
    /// Scripts that already ran (or must never run: innerHTML).
    pub executed_scripts: HashSet<NodeId>,
    /// `<script>` elements being executed (innermost last).
    pub current_script: Vec<NodeId>,
    /// Scripts written by `document.write` outside the parser, to run after
    /// the current one.
    pub pending_scripts: Vec<NodeId>,
    /// External scripts inserted by script: they load asynchronously and
    /// run once the current task is over (WebCore's async script queue).
    pub async_scripts: Vec<NodeId>,
    /// Completions of asynchronous requests (XMLHttpRequest), run like
    /// network events: before the async scripts, outside of timers.
    pub network_tasks: Vec<JsValue>,
    /// Whether the parser is running (`document.write` feeds it).
    pub parsing: bool,
    /// Markup written by `document.write` during the current parser pause.
    pub write_buffer: String,
    /// `document.write` after parsing: insertion point per script.
    pub write_anchor: HashMap<NodeId, Option<NodeId>>,
    pub timers: Vec<Timer>,
    pub next_timer: u32,
    pub seq: u64,
    pub clock: Rc<VirtualClock>,
    pub layout_dirty: bool,
    pub print_called: bool,
    /// Sources by Boa path key (`wkjs:<n>`).
    pub scripts: Vec<ScriptInfo>,
    pub canvases: HashMap<NodeId, CanvasState>,
    /// Lines of `<script>` start tags and elements with inline handlers.
    pub element_lines: HashMap<NodeId, u32>,
}

impl Host {
    /// Synchronous restyle + relayout, as forced by layout-dependent getters.
    pub fn ensure_layout(&mut self) {
        if self.layout_dirty {
            self.doc.resolve(0.0);
            self.layout_dirty = false;
        }
    }

    pub fn now(&self) -> f64 {
        self.clock.now.get()
    }

    /// Register a source for line mapping; returns its Boa path.
    pub fn add_script(&mut self, url: &str, first_line: u32, code: Rc<str>) -> String {
        self.scripts.push(ScriptInfo { url: url.to_string(), first_line, functions: None, code });
        format!("wkjs:{}", self.scripts.len() - 1)
    }
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static SERVICES: Cell<Option<*mut (dyn JsServices + 'static)>> = const { Cell::new(None) };
}

pub(crate) fn install(h: Host) {
    HOST.with(|c| {
        let mut c = c.borrow_mut();
        assert!(c.is_none(), "one JavaScript session per thread at a time");
        *c = Some(h);
    });
}

pub(crate) fn uninstall() -> Option<Host> {
    HOST.with(|c| c.borrow_mut().take())
}

/// Borrow the host. Never call back into JavaScript inside `f`.
pub(crate) fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> R {
    HOST.with(|c| f(c.borrow_mut().as_mut().expect("JavaScript session installed")))
}

/// Whether a session is installed on this thread.
pub(crate) fn has_host() -> bool {
    HOST.with(|c| c.borrow().is_some())
}

/// Lend `svc` to natives for the duration of `f`.
pub(crate) fn with_services<R>(svc: &mut dyn JsServices, f: impl FnOnce() -> R) -> R {
    let ptr: *mut (dyn JsServices + '_) = svc;
    // SAFETY: the pointer is only dereferenced by `services` while `f` runs,
    // during which `svc` is mutably borrowed by this function and not used by
    // the caller; the guard restores the previous value before returning, so
    // the erased lifetime never outlives the borrow.
    let ptr: *mut (dyn JsServices + 'static) = unsafe { std::mem::transmute(ptr) };
    struct Restore(Option<*mut (dyn JsServices + 'static)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SERVICES.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(SERVICES.with(|c| c.replace(Some(ptr))));
    f()
}

/// Use the lent services, if any. Re-entrant use sees none.
pub(crate) fn services<R>(f: impl FnOnce(&mut dyn JsServices) -> R) -> Option<R> {
    let p = SERVICES.with(|c| c.take())?;
    struct Put(*mut (dyn JsServices + 'static));
    impl Drop for Put {
        fn drop(&mut self) {
            SERVICES.with(|c| c.set(Some(self.0)));
        }
    }
    let put = Put(p);
    // SAFETY: see `with_services`; the slot is emptied while the reference is
    // alive, so no second `&mut` can be created.
    let r = f(unsafe { &mut *put.0 });
    drop(put);
    Some(r)
}

/// `ResourceObject::warning`.
pub(crate) fn warning(msg: &str) {
    services(|s| s.warning(msg));
}

// ---------------------------------------------------------------------------
// Console locations (QtWebKit 2.2 / JSC: the line of the innermost function's
// start, or of the script's start for global code).

/// Source URL marking the prelude (its frames are skipped).
pub(crate) const PRELUDE_URL: &str = "\u{0}prelude";

/// `<sourceURL>:<line>` of the innermost frame that is not prelude code.
pub(crate) fn console_location(ctx: &Context) -> (String, u32) {
    for f in ctx.stack_trace() {
        let loc = f.position();
        let path = format!("{}", loc.path);
        let Some(idx) = path.strip_prefix("wkjs:").and_then(|n| n.parse::<usize>().ok()) else {
            continue;
        };
        let pos = loc.position.map(|p| (p.line_number(), p.column_number())).unwrap_or((1, 1));
        let found = with_host(|h| {
            let info = h.scripts.get_mut(idx)?;
            if info.url == PRELUDE_URL {
                return None;
            }
            let fl = info.functions.get_or_insert_with(|| FunctionLines::scan(&info.code));
            let line = fl.function_start(pos.0, pos.1).unwrap_or(1);
            Some((info.url.clone(), info.first_line + line - 1))
        });
        if let Some(f) = found {
            return f;
        }
    }
    (String::from("undefined"), 0)
}

/// QtWebKit's text for an uncaught exception (JSC wording where it differs).
pub(crate) fn exception_text(err: &JsError, ctx: &mut Context) -> String {
    if let Some(v) = err.as_opaque() {
        // A thrown value: `String(value)`, or `Name: message` for errors.
        if let Some(o) = v.as_object() {
            if o.is::<boa_engine::builtins::error::Error>() {
                let name = o.get(JsString::from("name"), ctx).ok().and_then(|n| n.to_string(ctx).ok());
                let msg = o.get(JsString::from("message"), ctx).ok().and_then(|n| n.to_string(ctx).ok());
                let name = name.map(|n| n.to_std_string_escaped()).unwrap_or_else(|| "Error".into());
                let msg = msg.map(|n| n.to_std_string_escaped()).unwrap_or_default();
                return jsc_message(&name, &msg);
            }
        }
        return v.to_string(ctx).map(|s| s.to_std_string_escaped()).unwrap_or_default();
    }
    if matches!(err.as_engine(), Some(EngineError::RuntimeLimit(_))) {
        return "RangeError: Maximum call stack size exceeded.".to_string();
    }
    match err.try_native(ctx) {
        Ok(n) => {
            let name = match n.kind() {
                JsNativeErrorKind::Aggregate(_) => "AggregateError",
                JsNativeErrorKind::Error => "Error",
                JsNativeErrorKind::Eval => "EvalError",
                JsNativeErrorKind::Range => "RangeError",
                JsNativeErrorKind::Reference => "ReferenceError",
                JsNativeErrorKind::Syntax => "SyntaxError",
                JsNativeErrorKind::Type => "TypeError",
                JsNativeErrorKind::Uri => "URIError",
                _ => "Error",
            };
            jsc_message(name, n.message())
        }
        Err(_) => err.to_string(),
    }
}

fn jsc_message(name: &str, msg: &str) -> String {
    if name == "ReferenceError" {
        if let Some(v) = msg.strip_suffix(" is not defined") {
            return format!("ReferenceError: Can't find variable: {v}");
        }
    }
    if msg.is_empty() { name.to_string() } else { format!("{name}: {msg}") }
}

fn is_slow_script(err: &JsError) -> bool {
    matches!(err.as_engine(), Some(EngineError::RuntimeLimit(RuntimeLimitError::LoopIteration)))
}

/// Report an uncaught exception like `MyQWebPage::javaScriptConsoleMessage`
/// does for 0.12.6 (`undefined:0 <message>`, only with `--debug-javascript`),
/// and a stopped slow script like `shouldInterruptJavaScript`.
pub(crate) fn report_error(err: &JsError, ctx: &mut Context) {
    // Development aid: the full error with Boa's backtrace.
    if std::env::var_os("WKHTMLTOPDF_DEBUG_JS").is_some() {
        eprintln!("[wk-js] uncaught: {err}");
    }
    if is_slow_script(err) {
        // `shouldInterruptJavaScript`, then the interrupt reported as an
        // uncaught exception (verified with the 0.12.6 binary).
        let (stop, debug) = with_host(|h| (h.stop_slow_scripts, h.debug_js));
        if stop {
            warning("A slow script was stopped");
            if debug {
                warning("undefined:0 JavaScript execution exceeded timeout.");
            }
        }
        return;
    }
    if with_host(|h| h.debug_js) {
        let text = exception_text(err, ctx);
        warning(&format!("undefined:0 {text}"));
    }
}

// ---------------------------------------------------------------------------
// Evaluation.

/// Evaluate a source attributed to `url` starting at document line
/// `first_line`, then run the promise jobs.
pub(crate) fn eval(ctx: &mut Context, code: &str, url: &str, first_line: u32) -> Option<JsValue> {
    let code: Rc<str> = Rc::from(code);
    let path = with_host(|h| h.add_script(url, first_line, code.clone()));
    let src = Source::from_bytes(code.as_bytes()).with_path(std::path::Path::new(&path));
    let r = match ctx.eval(src) {
        Ok(v) => Some(v),
        Err(e) => {
            report_error(&e, ctx);
            None
        }
    };
    run_jobs(ctx);
    r
}

pub(crate) fn run_jobs(ctx: &mut Context) {
    if let Err(e) = ctx.run_jobs() {
        report_error(&e, ctx);
    }
}

/// Call a JS function, reporting what it throws.
pub(crate) fn call(ctx: &mut Context, f: &JsValue, this: &JsValue, args: &[JsValue]) -> Option<JsValue> {
    let callable = f.as_callable()?;
    let r = match callable.call(this, args, ctx) {
        Ok(v) => Some(v),
        Err(e) => {
            report_error(&e, ctx);
            None
        }
    };
    run_jobs(ctx);
    r
}

/// Expose elements with an id as globals (`window[id]`), for the ids that
/// appeared since the last call.
pub(crate) fn sync_ids(ctx: &mut Context) {
    let g = ctx.global_object();
    let Ok(wk) = g.get(JsString::from("__wk"), ctx) else { return };
    let Some(wk) = wk.as_object() else { return };
    let Ok(f) = wk.get(JsString::from("syncIds"), ctx) else { return };
    if let Some(f) = f.as_callable() {
        if let Err(e) = f.call(&JsValue::undefined(), &[], ctx) {
            report_error(&e, ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// Timers on the virtual clock.

pub(crate) fn add_timer(callback: JsValue, ms: f64, repeat: bool, args: Vec<JsValue>) -> u32 {
    with_host(|h| {
        let id = h.next_timer;
        h.next_timer += 1;
        h.seq += 1;
        // WebKit clamps timers to >= 1 ms (no 0-ms busy loop on the virtual clock).
        let ms = if ms.is_finite() { ms.max(0.0) } else { 0.0 };
        let ms = if repeat { ms.max(1.0) } else { ms };
        let due = h.now() + ms;
        h.timers.push(Timer { id, due, seq: h.seq, interval: repeat.then_some(ms), callback, args });
        id
    })
}

pub(crate) fn clear_timer(id: u32) {
    with_host(|h| h.timers.retain(|t| t.id != id));
}

/// Run every timer due at or before `until`, in (due, creation) order;
/// stops early when `print()` was called. Returns whether it stopped early.
pub(crate) fn advance(ctx: &mut Context, until: f64) -> bool {
    // A bound on timers fired per call: a 1 ms interval over a long delay is
    // still finite, but a pathological page must not spin forever.
    for _ in 0..1_000_000 {
        if with_host(|h| h.print_called) {
            return true;
        }
        let next = with_host(|h| {
            let idx = h
                .timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.due <= until)
                .min_by(|a, b| a.1.due.total_cmp(&b.1.due).then(a.1.seq.cmp(&b.1.seq)))
                .map(|(i, _)| i)?;
            let t = h.timers.remove(idx);
            if t.due > h.now() {
                h.clock.now.set(t.due);
            }
            let ret = (t.callback.clone(), t.args.clone());
            if let Some(iv) = t.interval {
                h.seq += 1;
                h.timers.push(Timer { due: t.due + iv, seq: h.seq, ..t });
            }
            Some(ret)
        });
        let Some((cb, args)) = next else { break };
        sync_ids(ctx);
        if cb.is_callable() {
            call(ctx, &cb, &JsValue::undefined(), &args);
        } else if let Ok(code) = cb.to_string(ctx) {
            let url = with_host(|h| h.url.to_string());
            eval(ctx, &code.to_std_string_escaped(), &url, 1);
        }
        crate::run_async_scripts(ctx);
    }
    with_host(|h| {
        if until > h.now() {
            h.clock.now.set(until);
        }
    });
    with_host(|h| h.print_called)
}

pub(crate) fn has_timers() -> bool {
    with_host(|h| !h.timers.is_empty())
}
