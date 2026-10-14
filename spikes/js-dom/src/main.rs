//! jsdom-spike: can boa_engine 0.22 drive a blitz-dom 0.3 document well enough for wkhtmltopdf?
//!
//! Usage: jsdom-spike PAGE.html [--query 'a=1&b=2'] [--javascript-delay MS] [--window-status S]
//!        [--run-script JS]... [--debug-javascript] [--audit] [--canvas-recorder]
//!        [--out FILE] [--expect TEXT]... [--expect-not TEXT]...
mod dom;
mod host;

use blitz_dom::DocumentConfig;
use blitz_traits::shell::{ColorScheme, Viewport};
use boa_engine::{Context, JsString, JsValue};
use host::{fire, set_host, take_host, with_host, Host};
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::FontContext;
use std::sync::Arc;
use std::time::Instant;

const PRELUDE: &str = include_str!("prelude.js");
const VIEWPORT: (u32, u32) = (1000, 1300);

struct Args {
    page: String,
    query: String,
    js_delay: f64,
    window_status: String,
    run_scripts: Vec<String>,
    debug_js: bool,
    audit: bool,
    canvas_recorder: bool,
    loop_limit: Option<u64>,
    out: Option<String>,
    expect: Vec<String>,
    expect_not: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        page: String::new(),
        query: String::new(),
        js_delay: 200.0,
        window_status: String::new(),
        run_scripts: vec![],
        debug_js: false,
        audit: false,
        canvas_recorder: false,
        loop_limit: None,
        out: None,
        expect: vec![],
        expect_not: vec![],
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--query" => a.query = it.next().unwrap(),
            "--javascript-delay" => a.js_delay = it.next().unwrap().parse().unwrap(),
            "--window-status" => a.window_status = it.next().unwrap(),
            "--run-script" => a.run_scripts.push(it.next().unwrap()),
            "--debug-javascript" => a.debug_js = true,
            "--audit" => a.audit = true,
            "--canvas-recorder" => a.canvas_recorder = true,
            "--loop-limit" => a.loop_limit = Some(it.next().unwrap().parse().unwrap()),
            "--out" => a.out = Some(it.next().unwrap()),
            "--expect" => a.expect.push(it.next().unwrap()),
            "--expect-not" => a.expect_not.push(it.next().unwrap()),
            _ => a.page = x,
        }
    }
    a
}

/// Same font setup as spikes/static-render: DejaVu only, generic families mapped to it.
fn font_context() -> FontContext {
    let dir = std::env::var("SPIKE_FONT_DIR").unwrap_or_else(|_| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/wk-fonts/fonts").to_string()
    });
    let mut collection = Collection::new(CollectionOptions { system_fonts: false, ..Default::default() });
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if matches!(p.extension().and_then(|s| s.to_str()), Some("ttf" | "otf")) {
                collection.register_fonts(Blob::new(Arc::new(std::fs::read(&p).unwrap())), None);
            }
        }
    }
    if let Some(sans) = collection.family_id("DejaVu Sans") {
        for g in [GenericFamily::Serif, GenericFamily::SansSerif, GenericFamily::SystemUi] {
            collection.set_generic_families(g, [sans].into_iter());
        }
    }
    FontContext { collection, source_cache: SourceCache::new(Default::default()) }
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let a = parse_args();
    let t_start = Instant::now();
    let path = std::fs::canonicalize(&a.page).expect("page");
    let html = std::fs::read_to_string(&path).unwrap();
    let mut url = format!("file://{}", path.display());
    if !a.query.is_empty() {
        url = format!("{url}?{}", a.query);
    }

    // 1. Parse (blitz-html parses the whole input; no script-pausing parser in this spike).
    let t = Instant::now();
    let config = DocumentConfig {
        viewport: Some(Viewport::new(VIEWPORT.0, VIEWPORT.1, 1.0, ColorScheme::Light)),
        base_url: Some(url.clone()),
        html_parser_provider: Some(Arc::new(blitz_html::HtmlProvider)),
        font_ctx: Some(font_context()),
        ..Default::default()
    };
    let doc = blitz_html::HtmlDocument::from_html(&html, config).into_inner();
    let t_parse = t.elapsed();
    set_host(Host::new(doc, path.parent().unwrap().to_path_buf(), url, a.debug_js));

    // 2. JS realm: native protos + __wk + prelude.
    let t = Instant::now();
    let mut ctx = Context::default();
    if let Some(n) = a.loop_limit {
        // Closest Boa knob to QtWebKit's slow-script interrupt: a per-loop iteration cap.
        ctx.runtime_limits_mut().set_loop_iteration_limit(n);
    }
    let protos = dom::install(&mut ctx);
    with_host(|h| h.protos = Some(protos));
    let wk = host::make_wk(&mut ctx, VIEWPORT, a.audit, a.canvas_recorder).unwrap();
    let global = ctx.global_object();
    global.set(JsString::from("__wk"), wk.clone(), false, &mut ctx).unwrap();
    global.set(JsString::from("document"), host::document_wrapper(), false, &mut ctx).unwrap();
    host::eval(&mut ctx, PRELUDE, "prelude.js");
    let t_realm = t.elapsed();

    // 3. Scripts in document order, then DOMContentLoaded, load (incl. <body onload>), --run-script.
    let t = Instant::now();
    let scripts = with_host(|h| host::all_scripts(h));
    for id in scripts {
        if with_host(|h| h.executed_scripts.insert(id)) {
            host::run_script_element(id, &mut ctx);
        }
    }
    let set_ready = |ctx: &mut Context, st: &str| {
        wk.set(JsString::from("readyState"), dom::s(st), false, ctx).unwrap();
    };
    set_ready(&mut ctx, "interactive");
    fire(&mut ctx, &wk, host::document_wrapper(), "DOMContentLoaded", true);
    set_ready(&mut ctx, "complete");
    let body_onload = with_host(|h| {
        h.doc.find_body_node().and_then(|b| b.attr(blitz_dom::local_name!("onload")).map(String::from))
    });
    if let Some(code) = body_onload {
        // <body onload> is a window event handler content attribute.
        host::eval(&mut ctx, &format!("window.onload = function (event) {{ {code}\n}};"), "body-onload");
    }
    fire(&mut ctx, &wk, global.clone().into(), "load", false);
    for (i, rs) in a.run_scripts.iter().enumerate() {
        host::eval(&mut ctx, rs, &format!("run-script-{i}"));
    }
    let t_scripts = t.elapsed();

    // 4. Wait phase on the virtual clock (multipageloader.cc loadFinished semantics).
    let t = Instant::now();
    let mut wait_note = String::new();
    let printed = with_host(|h| h.print_called);
    if printed || a.js_delay == 0.0 {
        wait_note = "done immediately (print() or delay 0)".into();
    } else if !a.window_status.is_empty() {
        let want = a.window_status.clone();
        let mut found = false;
        // Poll every 50 virtual ms; give up after 60 s of virtual time (upstream waits forever).
        for _ in 0..1200 {
            if host::window_status(&mut ctx) == want {
                found = true;
                break;
            }
            let until = with_host(|h| h.now) + 50.0;
            host::advance(&mut ctx, until, &mut |_| with_host(|h| h.print_called));
            if with_host(|h| h.print_called) {
                break;
            }
            if with_host(|h| h.timers.is_empty()) && host::window_status(&mut ctx) != want {
                wait_note = format!("window.status never becomes {want:?} and no timer is pending: upstream would hang");
                break;
            }
        }
        if found {
            let found_at = with_host(|h| h.now);
            let until = found_at + a.js_delay;
            host::advance(&mut ctx, until, &mut |_| with_host(|h| h.print_called));
            wait_note = format!("window.status {want:?} found at t={found_at} ms, then waited {} ms", a.js_delay);
        }
    } else {
        host::advance(&mut ctx, a.js_delay, &mut |_| with_host(|h| h.print_called));
        wait_note = format!("waited {} virtual ms", a.js_delay);
    }
    let t_wait = t.elapsed();
    let final_status = host::window_status(&mut ctx);

    // Audit output (read before the realm goes away).
    let audit = |ctx: &mut Context, key: &str| -> Vec<(String, i64)> {
        let o = wk.get(JsString::from(key), ctx).unwrap();
        let Some(o) = o.as_object() else { return vec![] };
        let mut v = vec![];
        for k in o.own_property_keys(ctx).unwrap() {
            let n = o.get(k.clone(), ctx).unwrap().to_number(ctx).unwrap_or(0.0) as i64;
            v.push((k.to_string(), n));
        }
        v.sort();
        v
    };
    let missing_props = audit(&mut ctx, "missingSet");
    let canvas_used = audit(&mut ctx, "canvasUsed");

    // 5. Final style + layout (what the PDF pipeline would do next), then serialise.
    let t = Instant::now();
    with_host(|h| {
        h.layout_dirty = true;
        h.ensure_layout();
    });
    let t_layout = t.elapsed();
    let t = Instant::now();
    let out = with_host(|h| {
        let mut s = String::from("<!DOCTYPE html>");
        dom::serialize_node(h, h.doc.root_element().id, &mut s);
        s
    });
    let t_ser = t.elapsed();
    let total = t_start.elapsed();
    let _ = JsValue::undefined();

    match &a.out {
        Some(p) => std::fs::write(p, &out).unwrap(),
        None => println!("{out}"),
    }

    let h = take_host();
    eprintln!("spike: page={} url_query={:?}", a.page, a.query);
    eprintln!(
        "spike: time ms: parse={:.1} realm+prelude={:.1} scripts+events={:.1} wait(virtual {} ms)={:.1} final-layout={:.1} serialize={:.1} total={:.1}",
        ms(t_parse),
        ms(t_realm),
        ms(t_scripts),
        h.now,
        ms(t_wait),
        ms(t_layout),
        ms(t_ser),
        ms(total)
    );
    eprintln!(
        "spike: forced layouts during JS={} ({:.1} ms) wrappers={} mutations={} created={} queries={} innerHTML-sets={} document.write={} timers-fired={} pending-timers={}",
        h.layouts.saturating_sub(1),
        ms(h.layout_time.saturating_sub(t_layout)),
        h.stats_wrappers,
        h.stats_mutations,
        h.stats_created,
        h.stats_queries,
        h.stats_inner_html,
        h.stats_doc_write,
        h.stats_timers_fired,
        h.timers.len()
    );
    eprintln!("spike: wait: {wait_note}; final window.status={final_status:?}");
    for (k, n) in &h.missing {
        eprintln!("spike: missing API used: {k} x{n}");
    }
    for (k, n) in &missing_props {
        eprintln!("spike: audit: absent property looked up: {k} x{n}");
    }
    for (k, n) in &canvas_used {
        eprintln!("spike: canvas2d member used: {k} x{n}");
    }
    let mut ok = true;
    for e in &a.expect {
        let hit = out.contains(e.as_str());
        ok &= hit;
        eprintln!("spike: expect {:?}: {}", e, if hit { "PASS" } else { "FAIL" });
    }
    for e in &a.expect_not {
        let hit = out.contains(e.as_str());
        ok &= !hit;
        eprintln!("spike: expect-not {:?}: {}", e, if hit { "FAIL" } else { "PASS" });
    }
    std::process::exit(if ok { 0 } else { 1 });
}
