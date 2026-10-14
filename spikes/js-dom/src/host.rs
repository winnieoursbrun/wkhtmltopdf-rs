//! Host side: document + JS context state, `__wk` natives, script execution, virtual-clock timers
//! and the wkhtmltopdf load lifecycle (scripts -> DOMContentLoaded -> load -> --run-script ->
//! --window-status / --javascript-delay wait).
use crate::dom::{self, arg_str, id_of, s, wrap, Protos};
use blitz_dom::{BaseDocument, NodeData, NodeId};
use boa_engine::object::builtins::JsArray;
use boa_engine::{Context, JsArgs, JsObject, JsResult, JsString, JsValue, Source};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Timer {
    id: u32,
    due: f64,
    seq: u64,
    interval: Option<f64>,
    cb: JsValue,
    args: Vec<JsValue>,
}

pub struct Host {
    pub doc: BaseDocument,
    pub cache: HashMap<NodeId, JsObject>,
    pub protos: Option<Protos>,
    pub fragments: HashSet<NodeId>,
    pub executed_scripts: HashSet<NodeId>,
    pub current_script: Vec<NodeId>,
    pub write_anchor: HashMap<NodeId, Option<NodeId>>,
    pub pending_written_scripts: Vec<NodeId>,
    pub timers: Vec<Timer>,
    pub next_timer: u32,
    pub seq: u64,
    /// Virtual clock, ms since navigation start.
    pub now: f64,
    pub layout_dirty: bool,
    pub base_dir: PathBuf,
    pub url: String,
    pub debug_js: bool,
    pub print_called: bool,
    pub missing: BTreeMap<String, u32>,
    pub layouts: u32,
    pub layout_time: Duration,
    pub stats_wrappers: u64,
    pub stats_mutations: u64,
    pub stats_created: u64,
    pub stats_queries: u64,
    pub stats_inner_html: u64,
    pub stats_doc_write: u64,
    pub stats_timers_fired: u64,
}

impl Host {
    pub fn new(doc: BaseDocument, base_dir: PathBuf, url: String, debug_js: bool) -> Self {
        Host {
            doc,
            cache: HashMap::new(),
            protos: None,
            fragments: HashSet::new(),
            executed_scripts: HashSet::new(),
            current_script: vec![],
            write_anchor: HashMap::new(),
            pending_written_scripts: vec![],
            timers: vec![],
            next_timer: 1,
            seq: 0,
            now: 0.0,
            layout_dirty: true,
            base_dir,
            url,
            debug_js,
            print_called: false,
            missing: BTreeMap::new(),
            layouts: 0,
            layout_time: Duration::ZERO,
            stats_wrappers: 0,
            stats_mutations: 0,
            stats_created: 0,
            stats_queries: 0,
            stats_inner_html: 0,
            stats_doc_write: 0,
            stats_timers_fired: 0,
        }
    }

    /// Synchronous restyle + relayout, as forced by layout-dependent DOM getters.
    pub fn ensure_layout(&mut self) {
        if self.layout_dirty {
            let t = Instant::now();
            self.doc.resolve(0.0);
            self.layout_time += t.elapsed();
            self.layouts += 1;
            self.layout_dirty = false;
        }
    }
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

pub fn set_host(h: Host) {
    HOST.with(|c| *c.borrow_mut() = Some(h));
}
pub fn take_host() -> Host {
    HOST.with(|c| c.borrow_mut().take().unwrap())
}

/// Never call back into JS while inside `f` (the host is exclusively borrowed).
pub fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> R {
    HOST.with(|c| f(c.borrow_mut().as_mut().expect("host installed")))
}

// ---------- console / errors ----------

/// `<sourceURL>:<line>` of the innermost JS frame that is not the prelude.
fn js_location(ctx: &Context) -> (String, u32) {
    for f in ctx.stack_trace() {
        let loc = f.position();
        let path = format!("{}", loc.path);
        if path.contains("prelude.js") {
            continue;
        }
        let line = loc.position.map(|p| p.line_number()).unwrap_or(0);
        return (path, line);
    }
    (String::new(), 0)
}

pub fn report_error(err: &boa_engine::JsError, ctx: &mut Context, src: &str) {
    let msg = match err.try_native(ctx) {
        Ok(n) => format!("{}: {}", n.kind(), n.message()),
        Err(_) => err.to_string(),
    };
    let (debug, url) = with_host(|h| (h.debug_js, h.url.clone()));
    // QtWebKit reports uncaught exceptions through javaScriptConsoleMessage.
    if debug {
        eprintln!("Warning: {}:{} {}", if src.is_empty() { &url } else { src }, 0, msg);
    }
    eprintln!("spike: uncaught JS error in {src}: {msg}");
}

// ---------- __wk natives ----------

fn wk_console(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let msg = arg_str(args, 0, ctx)?;
    let (path, line) = js_location(ctx);
    if with_host(|h| h.debug_js) {
        // 0.12.6 binary: `Warning: <sourceURL>:<line> <message>` (docs/upstream-spec.md §JS hooks).
        eprintln!("Warning: {path}:{line} {msg}");
    }
    Ok(JsValue::undefined())
}

fn wk_dialog(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let kind = arg_str(args, 0, ctx)?;
    let msg = arg_str(args, 1, ctx)?;
    let def = arg_str(args, 2, ctx)?;
    match kind.as_str() {
        "alert" => eprintln!("Warning: Javascript alert: {msg}"),
        "confirm" => eprintln!("Warning: Javascript confirm: {msg} (answered yes)"),
        _ => eprintln!("Warning: Javascript prompt: {msg} (answered {def})"),
    }
    Ok(JsValue::undefined())
}

fn wk_add_timer(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let cb = args.get_or_undefined(0).clone();
    let ms = args.get_or_undefined(1).to_number(ctx)?.max(0.0);
    let repeat = args.get_or_undefined(2).to_boolean();
    let extra: Vec<JsValue> = match args.get_or_undefined(3).as_object() {
        Some(o) => {
            let arr = JsArray::from_object(o.clone())?;
            let n = arr.length(ctx)?;
            let mut v = Vec::with_capacity(n as usize);
            for i in 0..n {
                v.push(arr.get(i, ctx)?);
            }
            v
        }
        None => vec![],
    };
    let id = with_host(|h| {
        let id = h.next_timer;
        h.next_timer += 1;
        h.seq += 1;
        // WebKit clamps repeating timers to >= 1 ms (no 0-ms busy loops on the virtual clock).
        let ms = if repeat { ms.max(1.0) } else { ms };
        h.timers.push(Timer { id, due: h.now + ms, seq: h.seq, interval: repeat.then_some(ms), cb, args: extra });
        id
    });
    Ok(id.into())
}

fn wk_clear_timer(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = args.get_or_undefined(0).to_number(ctx)? as u32;
    with_host(|h| h.timers.retain(|t| t.id != id));
    Ok(JsValue::undefined())
}

fn wk_now(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(with_host(|h| h.now).into())
}

fn wk_print(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_host(|h| h.print_called = true);
    Ok(JsValue::undefined())
}

fn wk_missing(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let name = arg_str(args, 0, ctx)?;
    with_host(|h| *h.missing.entry(name).or_default() += 1);
    Ok(JsValue::undefined())
}

fn wk_report_error(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let e = arg_str(args, 0, ctx)?;
    eprintln!("spike: uncaught error in event listener: {e}");
    Ok(JsValue::undefined())
}

fn style_attr(id: NodeId) -> String {
    with_host(|h| h.doc.get_node(id).and_then(|n| n.attr(blitz_dom::local_name!("style"))).unwrap_or("").to_string())
}

fn wk_style_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let prop = arg_str(args, 1, ctx)?;
    let attr = style_attr(id);
    Ok(s(&with_host(|h| h.doc.style_attr_get_property(&attr, &prop))))
}

fn wk_style_set(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let prop = arg_str(args, 1, ctx)?;
    let value = arg_str(args, 2, ctx)?;
    let important = args.get_or_undefined(3).to_boolean();
    let attr = style_attr(id);
    with_host(|h| {
        // Invalid declarations are ignored (CSSOM); the style attribute is the single source of
        // truth, so serialisation and restyle both see the change.
        if let Some(new) = h.doc.style_attr_set_property(&attr, &prop, &value, important) {
            dom::set_attr(h, id, "style", &new);
        }
    });
    Ok(JsValue::undefined())
}

fn wk_style_known(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let prop = arg_str(args, 0, ctx)?;
    Ok(blitz_dom::css_property_is_supported(&prop).into())
}

fn wk_computed(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let prop = arg_str(args, 1, ctx)?;
    Ok(s(&with_host(|h| {
        h.ensure_layout();
        h.doc.resolved_style_value(id, &prop)
    })))
}

fn wk_media_matches(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let q = arg_str(args, 0, ctx)?;
    Ok((!q.contains("print")).into())
}

pub fn make_wk(ctx: &mut Context, viewport: (u32, u32), audit: bool, canvas_recorder: bool) -> JsResult<JsObject> {
    let wk = JsObject::with_object_proto(ctx.intrinsics());
    type Fp = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;
    let natives: [(&str, Fp); 14] = [
        ("console", wk_console),
        ("dialog", wk_dialog),
        ("addTimer", wk_add_timer),
        ("clearTimer", wk_clear_timer),
        ("now", wk_now),
        ("print", wk_print),
        ("missing", wk_missing),
        ("reportError", wk_report_error),
        ("styleGet", wk_style_get),
        ("styleSet", wk_style_set),
        ("styleKnown", wk_style_known),
        ("computed", wk_computed),
        ("mediaMatches", wk_media_matches),
        ("forceLayout", |_, _, _| {
            with_host(|h| h.ensure_layout());
            Ok(JsValue::undefined())
        }),
    ];
    for (n, f) in natives {
        let func = boa_engine::object::FunctionObjectBuilder::new(ctx.realm(), boa_engine::NativeFunction::from_fn_ptr(f))
            .name(JsString::from(n))
            .build();
        wk.set(JsString::from(n), func, false, ctx)?;
    }
    let protos = JsObject::with_object_proto(ctx.intrinsics());
    let (url, list) = with_host(|h| {
        let p = h.protos.as_ref().unwrap();
        let list = vec![
            ("eventTarget", p.event_target.clone()),
            ("node", p.node.clone()),
            ("characterData", p.character_data.clone()),
            ("element", p.element.clone()),
            ("text", p.text.clone()),
            ("comment", p.comment.clone()),
            ("document", p.document.clone()),
            ("fragment", p.fragment.clone()),
        ];
        (h.url.clone(), list)
    });
    for (n, o) in list {
        protos.set(JsString::from(n), o, false, ctx)?;
    }
    wk.set(JsString::from("protos"), protos, false, ctx)?;
    wk.set(JsString::from("url"), s(&url), false, ctx)?;
    wk.set(JsString::from("audit"), audit, false, ctx)?;
    wk.set(JsString::from("canvasRecorder"), canvas_recorder, false, ctx)?;
    wk.set(JsString::from("viewportWidth"), viewport.0, false, ctx)?;
    wk.set(JsString::from("viewportHeight"), viewport.1, false, ctx)?;
    wk.set(JsString::from("readyState"), s("loading"), false, ctx)?;
    Ok(wk)
}

// ---------- script execution ----------

pub fn eval(ctx: &mut Context, code: &str, path: &str) {
    let src = Source::from_bytes(code).with_path(std::path::Path::new(path));
    if let Err(e) = ctx.eval(src) {
        report_error(&e, ctx, path);
    }
    if let Err(e) = ctx.run_jobs() {
        report_error(&e, ctx, path);
    }
}

/// Execute one `<script>` element (inline or external `src`), then any scripts it inserted
/// with `document.write`.
pub fn run_script_element(id: NodeId, ctx: &mut Context) {
    let (code, path) = with_host(|h| {
        let node = h.doc.get_node(id).unwrap();
        let ty = node.attr(blitz_dom::local_name!("type")).unwrap_or("").to_ascii_lowercase();
        if !(ty.is_empty() || ty.contains("javascript") || ty == "module") {
            return (None, String::new());
        }
        match node.attr(blitz_dom::local_name!("src")) {
            Some(src) => {
                let p = if let Some(rest) = src.strip_prefix("file://") { PathBuf::from(rest) } else { h.base_dir.join(src) };
                match std::fs::read_to_string(&p) {
                    Ok(code) => (Some(code), format!("file://{}", p.canonicalize().unwrap_or(p).display())),
                    Err(e) => {
                        eprintln!("spike: cannot load script {}: {e}", p.display());
                        (None, String::new())
                    }
                }
            }
            None => (Some(node.text_content()), h.url.clone()),
        }
    });
    let Some(code) = code else { return };
    with_host(|h| h.current_script.push(id));
    eval(ctx, &code, &path);
    let written = with_host(|h| {
        h.current_script.pop();
        std::mem::take(&mut h.pending_written_scripts)
    });
    for w in written {
        if with_host(|h| h.executed_scripts.insert(w)) {
            run_script_element(w, ctx);
        }
    }
}

pub fn all_scripts(h: &Host) -> Vec<NodeId> {
    fn walk(h: &Host, id: NodeId, out: &mut Vec<NodeId>) {
        let n = h.doc.get_node(id).unwrap();
        if let NodeData::Element(e) = &n.data {
            if &*e.name.local == "script" {
                out.push(id);
            }
        }
        for &c in n.children.iter() {
            walk(h, c, out);
        }
    }
    let mut v = vec![];
    walk(h, h.doc.root_node().id, &mut v);
    v
}

// ---------- events & timers ----------

pub fn fire(ctx: &mut Context, wk: &JsObject, target: JsValue, ty: &str, bubbles: bool) {
    let f = wk.get(JsString::from("fire"), ctx).unwrap();
    if let Some(f) = f.as_callable() {
        if let Err(e) = f.call(&JsValue::undefined(), &[target, s(ty), bubbles.into()], ctx) {
            report_error(&e, ctx, ty);
        }
    }
    if let Err(e) = ctx.run_jobs() {
        report_error(&e, ctx, ty);
    }
}

/// Run every timer due at or before `until` on the virtual clock, in (due, creation) order.
/// Returns early when `stop` says so (window.status matched, window.print() called).
pub fn advance(ctx: &mut Context, until: f64, stop: &mut dyn FnMut(&mut Context) -> bool) -> bool {
    loop {
        if stop(ctx) {
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
            h.now = h.now.max(t.due);
            h.stats_timers_fired += 1;
            let ret = (t.cb.clone(), t.args.clone());
            if let Some(iv) = t.interval {
                h.seq += 1;
                h.timers.push(Timer { due: t.due + iv, seq: h.seq, ..t });
            }
            Some(ret)
        });
        let Some((cb, args)) = next else { break };
        if let Some(f) = cb.as_callable() {
            if let Err(e) = f.call(&JsValue::undefined(), &args, ctx) {
                report_error(&e, ctx, "timer");
            }
            if let Err(e) = ctx.run_jobs() {
                report_error(&e, ctx, "timer");
            }
        } else if let Ok(code) = cb.to_string(ctx) {
            eval(ctx, &code.to_std_string_escaped(), "timer");
        }
    }
    with_host(|h| h.now = h.now.max(until));
    stop(ctx)
}

pub fn window_status(ctx: &mut Context) -> String {
    let g = ctx.global_object();
    g.get(JsString::from("status"), ctx)
        .ok()
        .and_then(|v| v.to_string(ctx).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default()
}

pub fn document_wrapper() -> JsValue {
    wrap(dom::root())
}
