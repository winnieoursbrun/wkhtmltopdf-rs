//! JavaScript for the web engine. Stands in for JavaScriptCore and WebCore's
//! JS bindings inside QtWebKit 2.2, as driven by wkhtmltopdf's
//! `ResourceObject` (`src/lib/multipageloader.cc`): a Boa realm per page with
//! DOM bindings over the blitz-dom document, timers and `Date` on a virtual
//! clock, `window.status`, `window.print()`, dialogs and console output
//! printed like the 0.12.6 binary, the slow-script budget, and a canvas 2D
//! vector recorder.
//!
//! The page lifecycle (parser pauses at `</script>`, sub-resources, events,
//! the wait phase) is driven by `wk-webkit`; this crate owns the document
//! while a [`Session`] is alive and lends it back through
//! [`with_document`].
//!
//! Spike: `spikes/js-dom` (Boa 0.22 ↔ blitz-dom 0.3), ported here.

mod canvas;
mod dom;
mod fnlines;
mod host;
mod natives;
mod serialize;

use std::collections::HashMap;
use std::rc::Rc;

use blitz_dom::{BaseDocument, NodeId};
use boa_engine::{Context, JsObject, JsString, JsValue};
use parley::FontContext;
use url::Url;

use crate::host::{Host, VirtualClock, with_host, with_services};

pub use canvas::CanvasState;

const PRELUDE: &str = include_str!("prelude.js");

/// What the page needs from the loader and the progress feedback.
pub trait JsServices {
    /// `ResourceObject::warning`: dialogs, console messages
    /// (`--debug-javascript`), stopped slow scripts.
    fn warning(&mut self, message: &str);
    /// Fetch a sub-resource synchronously through the loader: scripts
    /// inserted by script, `XMLHttpRequest`. `None` when it failed (the
    /// failure is accounted for like any sub-resource).
    fn fetch(&mut self, url: &Url) -> Option<Vec<u8>>;
}

/// Options of one page's realm (`settings::Web` + `LoadPage`).
pub struct JsOptions {
    /// The page URL (`location`, `document.URL`, inline script source URL).
    pub url: Url,
    /// `--debug-javascript`.
    pub debug_javascript: bool,
    /// `--stop-slow-scripts` (default) / `--no-stop-slow-scripts`.
    pub stop_slow_scripts: bool,
    /// `navigator.userAgent`.
    pub user_agent: String,
    /// Fonts for canvas text.
    pub font_ctx: FontContext,
    /// `--print-media-type` (`matchMedia('print')`).
    pub media_print: bool,
    /// `screen.width`/`height` (QtWebKit's headless screen).
    pub screen: (u32, u32),
}

/// Where a parser-inserted `<script>` gets its code.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptSource {
    /// Not JavaScript (`type="text/template"`, …), or already run.
    Skip,
    /// Inline code, with its document line.
    Inline { code: String, line: u32 },
    /// External script to fetch first.
    External(Url),
}

/// The canvas content recorded by script, ready to paint.
pub use blitz_paint::canvas::CanvasPicture;

/// A page's JavaScript realm, owning the document while alive.
pub struct Session {
    ctx: Context,
    wk: JsObject,
    body_onload_done: bool,
}

/// Loop iterations after which a script counts as slow. QtWebKit asks
/// `shouldInterruptJavaScript` after ~10 s of execution; Boa runs about
/// 20 M simple iterations per second. A deterministic budget (DESIGN.md).
const SLOW_SCRIPT_LOOP_LIMIT: u64 = 200_000_000;

/// Access the document of the running session (the parser's tree sink).
pub fn with_document<R>(f: impl FnOnce(&mut BaseDocument) -> R) -> R {
    with_host(|h| f(&mut h.doc))
}

/// Whether the current insertion is a `<script>` of the session's document
/// marked to never run (used by the parser for `innerHTML`-like contexts).
fn script_is_js(type_attr: Option<&str>, language: Option<&str>) -> bool {
    match type_attr.map(|t| t.trim().to_ascii_lowercase()) {
        Some(t) if !t.is_empty() => {
            let t = t.split(';').next().unwrap_or("").trim().to_string();
            matches!(
                t.as_str(),
                "text/javascript"
                    | "application/javascript"
                    | "application/x-javascript"
                    | "text/ecmascript"
                    | "application/ecmascript"
                    | "text/jscript"
                    | "text/livescript"
                    | "text/javascript1.0"
                    | "text/javascript1.1"
                    | "text/javascript1.2"
                    | "text/javascript1.3"
                    | "text/javascript1.4"
                    | "text/javascript1.5"
                    | "text/x-javascript"
                    | "text/x-ecmascript"
            )
        }
        _ => match language.map(|l| l.trim().to_ascii_lowercase()) {
            Some(l) if !l.is_empty() => l.starts_with("javascript") || l == "jscript" || l == "livescript" || l == "ecmascript",
            _ => true,
        },
    }
}

/// Run a script inserted by script (DOM insertion or `document.write`
/// after parsing): inline code, or an external file fetched through the
/// loader.
pub(crate) fn run_inserted_script(id: NodeId, ctx: &mut Context) {
    let source = with_host(|h| script_source_of(h, id));
    let (code, url, line) = match source {
        ScriptSource::Skip => return,
        ScriptSource::Inline { code, line } => (code, with_host(|h| h.url.to_string()), line),
        ScriptSource::External(url) => {
            let Some(body) = host::services(|s| s.fetch(&url)).flatten() else { return };
            (decode_script(&body), url.to_string(), 1)
        }
    };
    execute_script(ctx, id, &code, &url, line);
}

fn script_source_of(h: &Host, id: NodeId) -> ScriptSource {
    let get = |n: &str| dom::get_attr(h, id, n);
    if !script_is_js(get("type").as_deref(), get("language").as_deref()) {
        return ScriptSource::Skip;
    }
    match get("src") {
        Some(src) if !src.trim().is_empty() => match h.doc.base_url().join(src.trim()) {
            Ok(u) => ScriptSource::External(u),
            Err(_) => ScriptSource::Skip,
        },
        Some(_) => ScriptSource::Skip,
        None => {
            let code = h.doc.get_node(id).map(|n| n.text_content()).unwrap_or_default();
            ScriptSource::Inline { code, line: h.element_lines.get(&id).copied().unwrap_or(1) }
        }
    }
}

/// Decode a script body (UTF-8, Latin-1 fallback like WebKit's default).
pub fn decode_script(body: &[u8]) -> String {
    let body = body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body);
    match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) => body.iter().map(|&b| b as char).collect(),
    }
}

fn execute_script(ctx: &mut Context, id: NodeId, code: &str, url: &str, line: u32) {
    host::sync_ids(ctx);
    with_host(|h| h.current_script.push(id));
    host::eval(ctx, code, url, line);
    let pending = with_host(|h| {
        h.current_script.pop();
        if h.current_script.is_empty() { std::mem::take(&mut h.pending_scripts) } else { Vec::new() }
    });
    // Scripts written by `document.write` from an inserted script.
    for p in pending {
        if with_host(|h| h.executed_scripts.insert(p)) {
            run_inserted_script(p, ctx);
        }
    }
}

impl Session {
    /// Create the realm for `doc` and install it on this thread.
    pub fn new(doc: BaseDocument, opts: JsOptions) -> Session {
        let start_wall_ms = std::env::var("SOURCE_DATE_EPOCH")
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .map(|s| s * 1000)
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            });
        let clock = Rc::new(VirtualClock { start_wall_ms, now: std::cell::Cell::new(0.0) });
        let mut ctx = Context::builder().clock(clock.clone()).build().expect("Boa context");
        ctx.runtime_limits_mut().set_loop_iteration_limit(if opts.stop_slow_scripts {
            SLOW_SCRIPT_LOOP_LIMIT
        } else {
            u64::MAX
        });
        ctx.runtime_limits_mut().set_recursion_limit(2048);
        let url = opts.url.to_string();
        host::install(Host {
            doc,
            url: opts.url,
            debug_js: opts.debug_javascript,
            stop_slow_scripts: opts.stop_slow_scripts,
            media_print: opts.media_print,
            font_ctx: opts.font_ctx,
            cache: HashMap::new(),
            protos: None,
            fragments: Default::default(),
            executed_scripts: Default::default(),
            current_script: Vec::new(),
            pending_scripts: Vec::new(),
            parsing: false,
            write_buffer: String::new(),
            write_anchor: HashMap::new(),
            timers: Vec::new(),
            next_timer: 1,
            seq: 0,
            clock,
            layout_dirty: true,
            print_called: false,
            scripts: Vec::new(),
            canvases: HashMap::new(),
            element_lines: HashMap::new(),
        });
        let protos = dom::install(&mut ctx);
        let wk = natives::make_wk(&mut ctx, &protos, &url, opts.screen, &opts.user_agent).expect("__wk");
        with_host(|h| h.protos = Some(protos));
        let global = ctx.global_object();
        global.set(JsString::from("__wk"), wk.clone(), false, &mut ctx).expect("__wk global");
        let document = with_host(|h| {
            let r = dom::root(h);
            dom::wrap_in(h, r)
        });
        global.set(JsString::from("document"), document, false, &mut ctx).expect("document global");
        host::eval(&mut ctx, PRELUDE, host::PRELUDE_URL, 1);
        Session { ctx, wk, body_onload_done: false }
    }

    /// Access the document.
    pub fn with_document<R>(&self, f: impl FnOnce(&mut BaseDocument) -> R) -> R {
        with_document(f)
    }

    /// The parser starts or stops (`document.write` feeds the parser while
    /// it runs).
    pub fn set_parsing(&mut self, parsing: bool) {
        with_host(|h| h.parsing = parsing);
    }

    /// Document mode from the parser (`document.compatMode`) and the
    /// encoding the page was decoded with (`document.characterSet`).
    pub fn set_document_info(&mut self, quirks: bool, charset: &str) {
        let _ = self.wk.set(JsString::from("quirks"), quirks, false, &mut self.ctx);
        let _ = self.wk.set(JsString::from("charset"), dom::s(charset), false, &mut self.ctx);
    }

    /// Record the document line of an element's start tag (scripts, inline
    /// handlers).
    pub fn note_line(&mut self, id: NodeId, line: u32) {
        with_host(|h| {
            h.element_lines.insert(id, line);
        });
    }

    /// The parser reached `</script>`: where its code comes from. The script
    /// is marked as started either way (it never runs twice).
    pub fn parser_script(&mut self, id: NodeId) -> ScriptSource {
        with_host(|h| {
            if !h.executed_scripts.insert(id) {
                return ScriptSource::Skip;
            }
            script_source_of(h, id)
        })
    }

    /// Run a parser-inserted script; returns what it wrote with
    /// `document.write`, to be parsed right after it.
    pub fn run_parser_script(&mut self, id: NodeId, code: &str, url: &str, line: u32, svc: &mut dyn JsServices) -> String {
        let ctx = &mut self.ctx;
        with_services(svc, || execute_script(ctx, id, code, url, line));
        with_host(|h| std::mem::take(&mut h.write_buffer))
    }

    /// `<body onload="…">`, once the body start tag was parsed: compiled
    /// into `window.onload` (a later assignment by script wins).
    pub fn install_body_onload(&mut self, svc: &mut dyn JsServices) {
        if self.body_onload_done {
            return;
        }
        let body = with_host(|h| h.doc.find_body_node().map(|b| b.id));
        let Some(body) = body else { return };
        self.body_onload_done = true;
        let Some(code) = with_host(|h| dom::get_attr(h, body, "onload")) else { return };
        let ctx = &mut self.ctx;
        let wk = self.wk.clone();
        with_services(svc, || {
            let f = wk.get(JsString::from("compileHandler"), ctx).unwrap_or_default();
            let el = dom::wrap(body);
            if let Some(handler) = host::call(ctx, &f, &JsValue::undefined(), &[dom::s(&code), el]) {
                let g = ctx.global_object();
                let _ = g.set(JsString::from("onload"), handler, false, ctx);
            }
        });
    }

    fn set_ready_state(&mut self, st: &str) {
        let _ = self.wk.set(JsString::from("readyState"), dom::s(st), false, &mut self.ctx);
    }

    fn fire(&mut self, target: JsValue, ty: &str, bubbles: bool, svc: &mut dyn JsServices) {
        let ctx = &mut self.ctx;
        let wk = self.wk.clone();
        with_services(svc, || {
            host::sync_ids(ctx);
            let f = wk.get(JsString::from("fire"), ctx).unwrap_or_default();
            host::call(ctx, &f, &JsValue::undefined(), &[target, dom::s(ty), bubbles.into()]);
        });
    }

    /// End of parsing: `readyState = interactive`, `DOMContentLoaded`.
    pub fn dom_content_loaded(&mut self, svc: &mut dyn JsServices) {
        self.set_parsing(false);
        self.set_ready_state("interactive");
        let doc = dom::wrap(with_host(|h| dom::root(h)));
        self.fire(doc, "DOMContentLoaded", true, svc);
    }

    /// Sub-resources done: `readyState = complete`, `load` on the window.
    pub fn load_event(&mut self, svc: &mut dyn JsServices) {
        self.install_body_onload(svc);
        self.set_ready_state("complete");
        let g: JsValue = self.ctx.global_object().into();
        self.fire(g, "load", false, svc);
    }

    /// Evaluate code in the page (`--run-script`, `evaluateJavaScript`).
    pub fn eval(&mut self, code: &str, svc: &mut dyn JsServices) {
        let ctx = &mut self.ctx;
        with_services(svc, || {
            host::sync_ids(ctx);
            host::eval(ctx, code, "undefined", 1);
        });
    }

    /// Whether `window.print()` was called.
    pub fn print_requested(&self) -> bool {
        with_host(|h| h.print_called)
    }

    /// `window.status`.
    pub fn window_status(&mut self) -> String {
        let g = self.ctx.global_object();
        g.get(JsString::from("status"), &mut self.ctx)
            .ok()
            .and_then(|v| v.to_string(&mut self.ctx).ok())
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_default()
    }

    /// Advance the virtual clock to `until` ms, running due timers; returns
    /// whether `print()` stopped it early.
    pub fn advance_to(&mut self, until: f64, svc: &mut dyn JsServices) -> bool {
        let ctx = &mut self.ctx;
        with_services(svc, || host::advance(ctx, until))
    }

    /// Virtual ms since navigation start.
    pub fn now(&self) -> f64 {
        with_host(|h| h.now())
    }

    /// Whether a timer is pending.
    pub fn has_timers(&self) -> bool {
        host::has_timers()
    }

    /// The wait after `loadFinished` (`ResourceObject::loadFinished` →
    /// `loadDone`): nothing after `print()` or with a zero delay; with
    /// `window_status`, poll `window.status` every 50 ms until it matches,
    /// then wait `delay` more; else wait `delay`. Timers run on the virtual
    /// clock. Upstream waits forever for a status that never comes; here
    /// the wait ends once no timer can change it any more.
    pub fn wait(&mut self, delay_ms: f64, window_status: Option<&str>, svc: &mut dyn JsServices) {
        if self.print_requested() || delay_ms <= 0.0 {
            return;
        }
        if let Some(want) = window_status.filter(|w| !w.is_empty()) {
            loop {
                if self.window_status() == want {
                    let at = self.now();
                    self.advance_to(at + delay_ms, svc);
                    return;
                }
                if !self.has_timers() {
                    return;
                }
                let next = self.now() + 50.0;
                if self.advance_to(next, svc) {
                    return;
                }
            }
        }
        let until = self.now() + delay_ms;
        self.advance_to(until, svc);
    }

    /// Final document and recorded canvases; uninstalls the realm.
    pub fn finish(mut self) -> (BaseDocument, HashMap<NodeId, CanvasPicture>) {
        let _ = self.ctx.run_jobs();
        let host = host::uninstall().expect("session installed");
        let Host { doc, canvases, .. } = host;
        let mut pics = HashMap::new();
        for (id, state) in canvases {
            let Some(node) = doc.get_node(id) else { continue };
            if node.element_data().is_none_or(|e| &*e.name.local != "canvas") {
                continue;
            }
            let dim = |n: &str, d: f64| node.attr(blitz_dom::LocalName::from(n)).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v >= 0.0).unwrap_or(d);
            let (width, height) = (dim("width", 300.0), dim("height", 150.0));
            let (scene, texts) = state.finish();
            pics.insert(id, CanvasPicture { scene, texts, width, height });
        }
        (doc, pics)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // `finish` already took the host; otherwise free the thread slot.
        if host::has_host() {
            let _ = host::uninstall();
        }
    }
}
