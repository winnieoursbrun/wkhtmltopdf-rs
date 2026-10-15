//! Page load with JavaScript. Stands in for the part of `QWebFrame::load`
//! that runs scripts, as observed through wkhtmltopdf's `ResourceObject`
//! (`src/lib/multipageloader.cc`): the parser runs scripts in document order
//! (pausing at each `</script>`, external scripts fetched through the
//! loader so they count in its error accounting), then `DOMContentLoaded`,
//! sub-resources, `load` (with `<body onload>`), `--run-script`, and the
//! wait phase (`--javascript-delay`, `--window-status`, `window.print()`) on
//! the virtual clock.
//!
//! Requests are created when their elements enter the document and
//! finished when the parser blocks on an external script or reaches the
//! end, which reproduces the interleaving of load messages and script
//! output of the 0.12.6 binary (`docs/upstream-fixtures/0.12.6/conversion/
//! warn.stderr`, `info_b.stderr`).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use blitz_dom::{BaseDocument, DocumentConfig, MediaType, local_name};
use blitz_traits::shell::{ColorScheme, Viewport};
use parley::FontContext;
use url::Url;
use wk_js::{JsOptions, JsServices, ScriptSource, Session};

use crate::net::{Fetcher, QueueNet, Queued, deliver};
use crate::page::{WebPage, WebSettings, is_image_url, ua_css};
use crate::parser::{Parser, Step};

/// QtWebKit's headless screen in the patched Qt (spec §3.3).
const SCREEN: (u32, u32) = (1366, 768);

/// Whether a document can run script at all: without `<script>` or an
/// inline `on…` handler nothing executes, and the static path is used.
pub(crate) fn needs_js(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    lower.contains("<script") || lower.contains("onload")
}

/// The loader side of a page load: the engine's request queue, requests in
/// flight, and what the scripts need (warnings, synchronous fetches).
struct Io<'f> {
    fetcher: &'f mut dyn Fetcher,
    net: QueueNet,
    /// Created, not yet finished: (blitz handler, url, fetcher handle).
    pending: Vec<(Option<Queued>, Url, usize)>,
    /// Outcome of every finished request, by URL (WebCore's memory cache).
    done: HashMap<String, Option<Vec<u8>>>,
    /// Requested but not finished, by URL (a second request waits for it).
    in_flight: HashSet<String>,
    bodies: HashMap<String, Arc<Vec<u8>>>,
    load_images: bool,
}

impl JsServices for Io<'_> {
    fn warning(&mut self, message: &str) {
        self.fetcher.warning(message);
    }

    fn fetch(&mut self, url: &Url) -> Option<Vec<u8>> {
        let id = self.fetcher.create(url);
        let body = self.fetcher.finish(id);
        if let Some(b) = &body {
            self.bodies.insert(url.to_string(), Arc::new(b.clone()));
        }
        body
    }
}

impl Io<'_> {
    /// `createRequest` for everything the document asked for since the
    /// last call.
    fn create_queued(&mut self, session: &Session) {
        let queued = self.net.take();
        if queued.is_empty() {
            return;
        }
        let images: HashSet<String> =
            if self.load_images { HashSet::new() } else { session.with_document(|d| image_urls(d)) };
        let mut waiting = Vec::new();
        for q in queued {
            let key = q.url.to_string();
            if let Some(body) = self.done.get(&key) {
                deliver(q, body.clone());
                continue;
            }
            if self.in_flight.contains(&key) {
                waiting.push(q);
                continue;
            }
            if !self.load_images && (images.contains(&key) || is_image_url(&q.url)) {
                deliver(q, None);
                continue;
            }
            let id = self.fetcher.create(&q.url);
            self.in_flight.insert(key);
            let u = q.url.clone();
            self.pending.push((Some(q), u, id));
        }
        // Duplicates of a request in flight get its outcome when it finishes.
        for q in waiting {
            let url = q.url.clone();
            self.pending.push((Some(q), url, usize::MAX));
        }
    }

    /// `createRequest` for an external script.
    fn create_url(&mut self, url: &Url) -> usize {
        let id = self.fetcher.create(url);
        self.pending.push((None, url.clone(), id));
        id
    }

    /// Finish every request in flight, in creation order; returns the body
    /// of request `want`.
    fn finish_all(&mut self, want: Option<usize>) -> Option<Vec<u8>> {
        let mut out = None;
        let pending = std::mem::take(&mut self.pending);
        let mut late = Vec::new();
        for (q, url, id) in pending {
            if id == usize::MAX {
                late.push((q, url));
                continue;
            }
            let body = self.fetcher.finish(id);
            let key = url.to_string();
            self.in_flight.remove(&key);
            if let Some(b) = &body {
                self.bodies.insert(key.clone(), Arc::new(b.clone()));
            }
            if want == Some(id) && q.is_none() {
                out = body.clone();
            }
            if let Some(q) = q {
                self.done.insert(key, body.clone());
                deliver(q, body);
            }
        }
        for (q, url) in late {
            if let Some(q) = q {
                let body = self.done.get(url.as_str()).cloned().flatten();
                deliver(q, body);
            }
        }
        out
    }

    /// Resources requested by style resolution (imports, fonts, background
    /// images), until no new request appears.
    fn settle(&mut self, session: &Session) {
        for _ in 0..32 {
            self.create_queued(session);
            if self.pending.is_empty() {
                session.with_document(|d| d.resolve(0.0));
                self.create_queued(session);
                if self.pending.is_empty() {
                    break;
                }
            }
            self.finish_all(None);
        }
    }
}

/// URLs of `<img src>` (for `--no-images`).
fn image_urls(doc: &BaseDocument) -> HashSet<String> {
    let base = doc.base_url().clone();
    let mut out = HashSet::new();
    let mut stack = vec![doc.root_node().id];
    while let Some(id) = stack.pop() {
        let Some(n) = doc.get_node(id) else { continue };
        stack.extend(n.children.iter().copied());
        if n.element_data().is_some_and(|e| *e.name.local == *"img") {
            if let Some(u) = n.attr(local_name!("src")).and_then(|s| base.join(s).ok()) {
                out.insert(u.to_string());
            }
        }
    }
    out
}

/// Load `html` with scripts enabled (see the module documentation).
pub(crate) fn load(
    html: &str,
    url: Url,
    settings: WebSettings,
    font_ctx: FontContext,
    fetcher: &mut dyn Fetcher,
) -> WebPage {
    let net = QueueNet::default();
    let media = if settings.print_media_type { MediaType::print() } else { MediaType::screen() };
    let config = DocumentConfig {
        // QtWebKit lays the page out in a 0×0 viewport while loading
        // (verified: `div.offsetWidth` is 0 during load).
        viewport: Some(Viewport::new(0, 0, 1.0, ColorScheme::Light)),
        base_url: Some(url.to_string()),
        net_provider: Some(Arc::new(net.clone())),
        font_ctx: Some(font_ctx.clone()),
        media_type: Some(media),
        html_parser_provider: Some(Arc::new(blitz_html::HtmlProvider)),
        ..Default::default()
    };
    let mut doc = BaseDocument::new(config);
    let extra = ua_css(&settings);
    if !extra.is_empty() {
        doc.add_user_agent_stylesheet(&extra);
    }
    if let Some(css) = &settings.user_style_sheet {
        // A user sheet is parsed by WebKit too: same profile as author CSS.
        doc.add_user_agent_stylesheet(&crate::profile::rewrite(
            css,
            blitz_dom::css_rewrite::CssSource::Sheet,
        ));
    }
    let mut session = Session::new(
        doc,
        JsOptions {
            url: url.clone(),
            debug_javascript: settings.debug_javascript,
            stop_slow_scripts: settings.stop_slow_scripts,
            user_agent: settings.user_agent.clone(),
            font_ctx,
            media_print: settings.print_media_type,
            screen: SCREEN,
        },
    );
    let mut io = Io {
        fetcher,
        net: net.clone(),
        pending: Vec::new(),
        done: HashMap::new(),
        in_flight: HashSet::new(),
        bodies: HashMap::new(),
        load_images: settings.load_images,
    };
    let page_url = url.to_string();

    // Parse, running scripts in document order.
    session.set_parsing(true);
    let mut parser = Parser::new(html);
    loop {
        let step = parser.step();
        for (id, line) in parser.sink().lines.borrow_mut().drain(..) {
            session.note_line(id, line);
        }
        session.set_document_info(parser.sink().quirks.get(), "UTF-8");
        let Step::Script(node) = step else { break };
        if parser.sink().body_seen.get() {
            session.install_body_onload(&mut io);
        }
        io.create_queued(&session);
        match session.parser_script(node) {
            ScriptSource::Skip => {}
            ScriptSource::Inline { code, line } => {
                let written = session.run_parser_script(node, &code, &page_url, line, &mut io);
                parser.write(&written);
            }
            ScriptSource::External(src) => {
                // The parser blocks: every request in flight completes.
                let id = io.create_url(&src);
                if let Some(body) = io.finish_all(Some(id)) {
                    let code = wk_js::decode_script(&body);
                    let written = session.run_parser_script(node, &code, src.as_str(), 1, &mut io);
                    parser.write(&written);
                }
            }
        }
    }
    drop(parser);

    // End of parsing.
    io.create_queued(&session);
    session.install_body_onload(&mut io);
    session.dom_content_loaded(&mut io);
    io.create_queued(&session);
    io.finish_all(None);
    session.run_async_scripts(&mut io);
    io.settle(&session);
    session.load_event(&mut io);
    io.settle(&session);
    if session.print_requested() {
        // `window.print()` before `loadFinished` ended the load early
        // (`printRequested` → `loadDone`); QtWebKit's own progress and
        // finished signals then reach a finished `ResourceObject`
        // (verified with 0.12.6).
        io.fetcher.warning(
            "A finished ResourceObject received a loading progress signal. This might be an indication of an iframe taking too long to load.",
        );
        io.fetcher.warning(
            "A finished ResourceObject received a loading finished signal. This might be an indication of an iframe taking too long to load.",
        );
    }

    // `loadFinished`: --run-script (main pages only), then the wait.
    // After an early `print()`, `loadFinished` reaches a finished object:
    // no --run-script.
    if !session.print_requested() {
        for code in &settings.run_script {
            session.eval(code, &mut io);
        }
    }
    let status = (!settings.window_status.is_empty()).then_some(settings.window_status.as_str());
    session.wait(f64::from(settings.javascript_delay), status, &mut io);
    io.settle(&session);

    let bodies = std::mem::take(&mut io.bodies);
    drop(io);
    let (doc, pictures) = session.finish();
    let canvases = pictures.into_iter().map(|(id, p)| (id, Rc::new(p))).collect();
    WebPage::from_parts(doc, net, settings, bodies, url, canvases)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves nothing; collects warnings.
    #[derive(Default)]
    struct NoNet(Vec<String>);
    impl Fetcher for NoNet {
        fn create(&mut self, _: &Url) -> usize {
            0
        }
        fn finish(&mut self, _: usize) -> Option<Vec<u8>> {
            None
        }
        fn warning(&mut self, m: &str) {
            self.0.push(m.to_string());
        }
    }

    fn load(html: &str, settings: WebSettings) -> (WebPage, Vec<String>) {
        let fonts =
            wk_fonts::font_context(&wk_fonts::FontOptions { system_fonts: false, extra_dirs: Vec::new() });
        let mut net = NoNet::default();
        let url = Url::parse("file:///tmp/page.html?page=3&topage=7").unwrap();
        let page = WebPage::load(html, url, settings, Some(fonts), &mut net);
        (page, net.0)
    }

    fn title(page: &WebPage) -> String {
        page.title().unwrap_or_default().trim().to_string()
    }

    #[test]
    fn head_scripts_do_not_see_the_body() {
        let html = "<html><head><title>t</title><script>document.title = (document.body ? 'body' : 'nobody') + ' ' + document.getElementsByTagName('p').length;</script></head><body><p>x</p></body></html>";
        let (page, _) = load(html, WebSettings::default());
        assert_eq!(title(&page), "nobody 0");
    }

    #[test]
    fn document_write_goes_back_into_the_parser() {
        let html = "<title>t</title><body><p>a</p><script>document.write('<p>b</p><scr' + 'ipt>document.write(\"<p>c</p>\")</scr' + 'ipt>');</script><p>d</p><script>document.title = [].map.call(document.getElementsByTagName('p'), function (p) { return p.textContent; }).join('');</script>";
        let (page, _) = load(html, WebSettings::default());
        assert_eq!(title(&page), "abcd");
    }

    #[test]
    fn query_string_reaches_location_search() {
        let html = "<title>t</title><body onload=\"document.title = location.search\">";
        let (page, _) = load(html, WebSettings::default());
        assert_eq!(title(&page), "?page=3&topage=7");
    }

    #[test]
    fn timers_run_on_the_virtual_clock_until_the_delay() {
        let html = "<title>t</title><script>var s = ''; [50, 150, 250].forEach(function (ms) { setTimeout(function () { s += ms + ' '; document.title = s; }, ms); });</script>";
        let (page, _) = load(html, WebSettings { javascript_delay: 200, ..WebSettings::default() });
        assert_eq!(title(&page), "50 150");
    }

    #[test]
    fn window_status_waits_then_the_delay_runs() {
        let html = "<title>t</title><script>var n = 0; function tick() { n++; document.title = 'n' + n; if (n === 10) window.status = 'go'; else setTimeout(tick, 100); } setTimeout(tick, 100); setTimeout(function () { document.title += ' late'; }, 1150);</script>";
        let settings =
            WebSettings { javascript_delay: 200, window_status: "go".into(), ..WebSettings::default() };
        let (page, _) = load(html, settings);
        // The status is seen at t=1000 (polled every 50 ms), then 200 ms more:
        // the 1150 ms timer runs.
        assert_eq!(title(&page), "n10 late");
    }

    #[test]
    fn wrappers_keep_their_identity_and_expandos() {
        let html = "<title>t</title><body><div id=d></div><script>var d = document.getElementById('d'); d.foo = 42; document.title = (document.body.firstChild === document.body.firstChild) + ' ' + document.getElementById('d').foo + ' ' + (d === window.d);</script>";
        let (page, _) = load(html, WebSettings::default());
        assert_eq!(title(&page), "true 42 true");
    }

    #[test]
    fn dialogs_print_like_0_12_6() {
        let html = "<script>alert('a'); confirm('b'); prompt('c', 'd');</script>";
        let (_, warnings) = load(html, WebSettings::default());
        assert_eq!(
            warnings,
            vec![
                "Javascript alert: a".to_string(),
                "Javascript confirm: b (answered yes)".to_string(),
                "Javascript prompt: c (answered d)".to_string(),
            ]
        );
    }

    #[test]
    fn disabled_javascript_runs_nothing() {
        let html = "<title>t</title><script>document.title = 'ran'; alert('x');</script>";
        let (page, warnings) = load(html, WebSettings { enable_javascript: false, ..WebSettings::default() });
        assert_eq!(title(&page), "t");
        assert!(warnings.is_empty());
    }
}
