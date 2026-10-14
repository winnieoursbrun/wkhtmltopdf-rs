//! The PDF conversion pipeline. Ports upstream `src/lib/pdfconverter.cc`
//! (`PdfConverterPrivate`: phases, page counting, link resolution,
//! printing), `src/lib/converter.cc` (progress plumbing) and
//! `src/lib/utilities.cc` (`handleError`), with the observable behaviour of
//! the 0.12.6 binary (spec §3.1, §4).
//!
//! Phases (patched Qt): `Loading pages`, `Counting pages`, `Loading TOC`
//! (only with a TOC object), `Resolving links`, `Loading headers and
//! footers`, `Printing pages`, `Done`.
//!
//! Not yet (P3): header/footer HTML and text, TOC generation (a TOC object
//! prints one blank page so that page order and numbering stay right), the
//! header/footer measuring load. JavaScript (P4) runs inside
//! `WebPage::load` with the object's `--javascript-delay`,
//! `--window-status`, `--run-script`, `--debug-javascript` and slow-script
//! settings.

pub mod outline;
pub mod utilities;

use std::collections::HashMap;
use std::io::{Read, Write};

use url::Url;
use wk_cli::progressfeedback::ProgressFeedback;
use wk_loader::{Feedback, MultiPageLoader, PendingRequest};
use wk_print::{DocInfo, DocPart, LinkTarget, PageSetup, PageSlice, PdfLink};
use wk_settings::{LogLevel, PdfGlobal, PdfObject};
use wk_webkit::{Fetcher, RenderedPage, WebPage, WebSettings};

use crate::outline::{LocatedHeading, Outline};
use crate::utilities::{handle_error, paper_size, to_points};

/// Phase descriptions in patched mode.
const PHASES: [&str; 7] = [
    "Loading pages",
    "Counting pages",
    "Loading TOC",
    "Resolving links",
    "Loading headers and footers",
    "Printing pages",
    "Done",
];

/// `Creator` of the PDF (`QPrinter::setCreator`).
pub const CREATOR: &str = "wkhtmltopdf 0.12.6";
/// `Producer` written by Qt's PDF engine.
pub const PRODUCER: &str = "Qt 4.8.7";

/// Process streams of a conversion.
pub struct ConvertIo<'a> {
    pub stdin: &'a mut dyn Read,
    pub stdout: &'a mut Vec<u8>,
    pub stderr: &'a mut Vec<u8>,
    /// Executable name, used in the `User-Agent`.
    pub app_name: String,
}

type Pf<'a> = ProgressFeedback<&'a mut Vec<u8>>;

/// Adapter: loader messages and progress go to `ProgressFeedback`.
struct LoadFeedback<'a, 'b> {
    pf: &'a mut Pf<'b>,
}

impl Feedback for LoadFeedback<'_, '_> {
    fn warning(&mut self, message: &str) {
        self.pf.warning(message);
    }
    fn error(&mut self, message: &str) {
        self.pf.error(message);
    }
    fn progress(&mut self, percent: i32) {
        self.pf.progress_changed(percent, &format!("{percent}%"));
    }
}

/// The engine's fetcher for page `index` of `loader`.
struct PageFetcher<'a, 'b, 'c> {
    loader: &'a mut MultiPageLoader,
    index: usize,
    pf: &'a mut Pf<'b>,
    pending: Vec<Option<PendingRequest>>,
    _marker: std::marker::PhantomData<&'c ()>,
}

impl Fetcher for PageFetcher<'_, '_, '_> {
    fn create(&mut self, url: &Url) -> usize {
        let mut fb = LoadFeedback { pf: self.pf };
        let p = self.loader.create_request(self.index, url, &mut fb);
        self.pending.push(Some(p));
        self.pending.len() - 1
    }

    fn finish(&mut self, id: usize) -> Option<Vec<u8>> {
        let p = self.pending.get_mut(id)?.take()?;
        let mut fb = LoadFeedback { pf: self.pf };
        let reply = self.loader.finish_request(self.index, &p, &mut fb);
        if reply.is_success() { Some(reply.body) } else { None }
    }

    fn warning(&mut self, message: &str) {
        self.pf.warning(message);
    }
}

/// One object of the conversion (`PageObject`).
struct PageObject<'s> {
    settings: &'s PdfObject,
    /// Index in the loader, for non-TOC objects.
    loader_index: Option<usize>,
    page: Option<WebPage>,
    rendered: Option<RenderedPage>,
    slices: Vec<PageSlice>,
    page_count: usize,
    title: String,
}

/// `looksLikeHtmlAndNotAUrl`.
fn looks_like_html_and_not_a_url(s: &str) -> bool {
    let before_query = s.split('?').next().unwrap_or("");
    before_query.contains('<') || s.to_ascii_lowercase().starts_with("data:")
}

/// The engine settings of an object. `main`: a page of the main loader
/// (`--run-script` and `--window-status` apply to those only).
fn web_settings(o: &PdfObject, user_css: Option<String>, user_agent: &str, main: bool) -> WebSettings {
    WebSettings {
        print_media_type: o.load.print_media_type,
        background: o.web.background,
        load_images: o.web.load_images,
        enable_javascript: o.web.enable_javascript,
        minimum_font_size: o.web.minimum_font_size,
        default_encoding: o.web.default_encoding.clone(),
        user_style_sheet: user_css,
        zoom: o.load.zoom_factor,
        smart_shrinking: o.web.enable_intelligent_shrinking,
        javascript_delay: o.load.jsdelay.max(0) as u32,
        window_status: if main { o.load.window_status.clone() } else { String::new() },
        run_script: if main { o.load.run_script.clone() } else { Vec::new() },
        debug_javascript: o.load.debug_javascript,
        stop_slow_scripts: o.load.stop_slow_scripts,
        user_agent: user_agent.to_string(),
    }
}

/// `--user-style-sheet`: read a local file or a `data:` URL.
fn user_style_sheet(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let guessed = wk_loader::urlguess::guess_url_from_string(s);
    let url = Url::parse(&guessed).ok()?;
    let bytes = match url.scheme() {
        "file" => std::fs::read(url.to_file_path().ok()?).ok()?,
        "data" => data_url::DataUrl::process(url.as_str()).ok()?.decode_to_vec().ok()?.0,
        // TODO(P2b): remote user style sheets.
        _ => return None,
    };
    Some(wk_webkit::encoding::decode_css(&bytes, "UTF-8"))
}

/// `QUrl::fromPercentEncoding(url.toString(QUrl::RemoveFragment))`.
fn url_key(u: &Url) -> String {
    let mut u = u.clone();
    u.set_fragment(None);
    let s = u.to_string();
    let bytes: Vec<u8> = {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(b[i]);
            i += 1;
        }
        out
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Seconds since the epoch, honouring `SOURCE_DATE_EPOCH` for reproducible
/// output.
fn now_secs() -> u64 {
    if let Some(v) = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|v| v.parse().ok()) {
        return v;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

enum Output {
    Stdout,
    File(std::fs::File),
}

/// Convert `objects` according to `global`; returns the process exit code
/// (`handleError(converter.convert(), converter.httpErrorCode())`).
pub fn convert(global: &PdfGlobal, objects: &[PdfObject], io: ConvertIo<'_>) -> i32 {
    let ConvertIo { stdin, stdout, stderr, app_name } = io;
    let (success, error_code) =
        convert_raw(global, objects, ConvertIo { stdin, stdout, stderr: &mut *stderr, app_name });
    let (code, line) = handle_error(success, error_code);
    if let Some(l) = line {
        stderr.extend_from_slice(l.as_bytes());
    }
    code
}

/// `PdfConverter::convert()` and `httpErrorCode()`, without `handleError`
/// (the `--read-args-from-stdin` loop only looks at success).
pub fn convert_raw(global: &PdfGlobal, objects: &[PdfObject], io: ConvertIo<'_>) -> (bool, i32) {
    let ConvertIo { stdin, stdout, stderr, app_name } = io;
    let mut pf = ProgressFeedback::new(global.log_level, &mut *stderr);
    let mut error_code = 0;
    let success = run(global, objects, stdin, stdout, &mut pf, &mut error_code, &app_name);
    (success, error_code)
}

fn phase(pf: &mut Pf<'_>, i: usize) {
    pf.phase_changed(PHASES[i], i, PHASES.len());
}

#[allow(clippy::too_many_lines)]
fn run(
    global: &PdfGlobal,
    objects: &[PdfObject],
    stdin: &mut dyn Read,
    stdout: &mut Vec<u8>,
    pf: &mut Pf<'_>,
    error_code: &mut i32,
    app_name: &str,
) -> bool {
    // beginConvert
    let mut g = global.clone();
    let mut loader = MultiPageLoader::new(&global.load, true, app_name);
    let mut objs: Vec<PageObject<'_>> = Vec::new();
    for s in objects {
        for (hf, name) in [(&s.header, "--header-html"), (&s.footer, "--footer-html")] {
            if !hf.html_url.is_empty() && looks_like_html_and_not_a_url(&hf.html_url) {
                pf.error(&format!("{name} should be a URL and not a string containing HTML code."));
                return false;
            }
        }
        let mut o = PageObject {
            settings: s,
            loader_index: None,
            page: None,
            rendered: None,
            slices: Vec::new(),
            page_count: 0,
            title: String::new(),
        };
        if !s.is_table_of_content {
            match loader.add_resource(&s.page, &s.load, stdin) {
                Ok(i) => o.loader_index = Some(i),
                Err(e) => {
                    pf.error(&e);
                    return false;
                }
            }
        }
        objs.push(o);
    }
    phase(pf, 0);
    pf.progress_changed(0, "0%");
    // TODO(P3): header/footer measuring load when a margin is unset.
    if g.margin.top.value == -1.0 {
        g.margin.top.value = 10.0;
    }
    if g.margin.bottom.value == -1.0 {
        g.margin.bottom.value = 10.0;
    }

    // Load all pages: start them, then load each in order.
    let fonts = wk_fonts::font_context(&wk_fonts::FontOptions::default());
    {
        let n = loader.resources.len();
        let mut fb = LoadFeedback { pf: &mut *pf };
        for i in 0..n {
            loader.set_progress(i, 10, &mut fb);
        }
    }
    for o in objs.iter_mut() {
        let Some(i) = o.loader_index else { continue };
        let (ok, reply) = {
            let mut fb = LoadFeedback { pf: &mut *pf };
            loader.load_main(i, &mut fb)
        };
        if !ok {
            let mut fb = LoadFeedback { pf: &mut *pf };
            loader.load_finished(i, false, &mut fb);
            if loader.has_error {
                break;
            }
            // Ignored failure: an empty page is printed.
            let ws = web_settings(o.settings, None, &wk_loader::user_agent(app_name), true);
            let mut f = PageFetcher {
                loader: &mut loader,
                index: i,
                pf: &mut *pf,
                pending: vec![],
                _marker: Default::default(),
            };
            o.page = Some(WebPage::load("", reply.final_url.clone(), ws, fonts.clone(), &mut f));
            continue;
        }
        {
            let mut fb = LoadFeedback { pf: &mut *pf };
            loader.set_progress(i, 50, &mut fb);
        }
        let html = wk_webkit::encoding::decode_html(
            &reply.body,
            reply.content_type.as_deref(),
            &o.settings.web.default_encoding,
        );
        let ws = web_settings(
            o.settings,
            user_style_sheet(&o.settings.web.user_style_sheet),
            &wk_loader::user_agent(app_name),
            true,
        );
        let page = {
            let mut f = PageFetcher {
                loader: &mut loader,
                index: i,
                pf: &mut *pf,
                pending: vec![],
                _marker: Default::default(),
            };
            WebPage::load(&html, reply.final_url.clone(), ws, fonts.clone(), &mut f)
        };
        o.page = Some(page);
        let mut fb = LoadFeedback { pf: &mut *pf };
        loader.set_progress(i, 100, &mut fb);
        loader.load_finished(i, true, &mut fb);
        if loader.has_error {
            break;
        }
    }
    loader.load_done();
    // pagesLoaded
    if *error_code == 0 {
        *error_code = loader.http_error_code();
    }
    if loader.has_error {
        return false;
    }

    let m = &g.margin;
    if m.left.unit != m.right.unit || m.left.unit != m.top.unit || m.left.unit != m.bottom.unit {
        pf.error("Currently all margin units must be the same!");
        return false;
    }
    let (paper_w, paper_h) = paper_size(&g);
    let unit = |v: f64| to_points(wk_settings::UnitReal::new(v, m.left.unit), g.dpi) as f32;
    let setup = PageSetup {
        width: paper_w as f32,
        height: paper_h as f32,
        margin_top: unit(m.top.value),
        margin_right: unit(m.right.value),
        margin_bottom: unit(m.bottom.value),
        margin_left: unit(m.left.value),
    };

    // painter->begin(printer): the output is opened before printing.
    let mut output = if g.out == "-" || g.out.is_empty() {
        Output::Stdout
    } else {
        match std::fs::File::create(&g.out) {
            Ok(f) => Output::File(f),
            Err(_) => {
                pf_raw(pf, "QPainter::begin(): Returned false\n");
                pf.error("Unable to write to destination");
                return false;
            }
        }
    };

    // Title: --title, else the first loaded page's <title>.
    for o in objs.iter_mut() {
        if let Some(p) = &o.page {
            o.title = p.title().map(|t| collapse_ws(&t)).unwrap_or_default();
        }
    }
    let mut title = g.document_title.clone();
    if title.is_empty() {
        for o in &objs {
            let skip = o.loader_index.is_none_or(|i| loader.resources[i].skip);
            if skip || o.settings.is_table_of_content {
                continue;
            }
            if !o.title.is_empty() {
                title = o.title.clone();
                break;
            }
        }
    }

    // Counting pages (phase 1).
    phase(pf, 1);
    let tot = objs.len();
    let mut outline = Outline::default();
    let mut out_page = 0usize;
    let page_offset = g.page_offset;
    for (d, o) in objs.iter_mut().enumerate() {
        if o.settings.is_table_of_content {
            // TODO(P3): TOC generation. One provisional page, like upstream
            // before the TOC is laid out.
            o.page_count = 1;
            outline.add_empty_web_page(out_page);
            out_page += 1;
            continue;
        }
        let skip = o.loader_index.is_none_or(|i| loader.resources[i].skip);
        if skip {
            continue;
        }
        pf.progress_changed(((d + 1) * 100 / tot) as i32, &format!("Object {} of {}", d + 1, tot));
        let Some(page) = o.page.as_mut() else { continue };
        let rendered = page.render(setup.printable_width(), setup.printable_height());
        let page_h_px = setup.printable_height() / rendered.pt_per_px;
        o.slices = wk_print::paginate(&rendered.geometry, page_h_px);
        o.page_count = if o.settings.pages_count { o.slices.len() } else { 0 };
        if o.settings.include_in_outline {
            let s = rendered.pt_per_px;
            let headings: Vec<LocatedHeading> = rendered
                .headings
                .iter()
                .filter_map(|h| {
                    let (pi, off) = locate(&o.slices, h.y)?;
                    Some(LocatedHeading {
                        level: h.level,
                        text: h.text.clone(),
                        page: pi + 1,
                        out_page: out_page + pi,
                        x: setup.margin_left + h.x * s,
                        y: setup.margin_top + off * s,
                    })
                })
                .collect();
            outline.add_web_page(&o.title, o.page_count, out_page, &headings);
        } else {
            outline.add_empty_web_page(out_page);
        }
        out_page += o.slices.len();
        o.rendered = Some(rendered);
    }
    let page_count: usize = objs.iter().map(|o| o.page_count).sum();
    let copies = g.copies.max(1) as usize;
    let actual_pages = page_count * copies;

    if objs.iter().any(|o| o.settings.is_table_of_content) {
        // TODO(P3): load and lay out the TOC.
        phase(pf, 2);
    }

    // Resolving links (phase 3).
    phase(pf, 3);
    let mut url_to_obj: HashMap<String, usize> = HashMap::new();
    for (d, o) in objs.iter().enumerate() {
        let skip = o.loader_index.is_none_or(|i| loader.resources[i].skip);
        if skip || o.settings.is_table_of_content {
            continue;
        }
        if let Some(i) = o.loader_index {
            url_to_obj.insert(url_key(&loader.resources[i].url), d);
        }
    }
    // First output page of each object.
    let mut first_page = Vec::with_capacity(objs.len());
    let mut acc = 0usize;
    for o in &objs {
        first_page.push(acc);
        acc += if o.settings.is_table_of_content { 1 } else { o.slices.len() };
    }
    let mut links: Vec<PdfLink> = Vec::new();
    for (d, o) in objs.iter().enumerate() {
        let skip = o.loader_index.is_none_or(|i| loader.resources[i].skip);
        if skip && !o.settings.is_table_of_content {
            continue;
        }
        pf.progress_changed(((d + 1) * 100 / tot) as i32, &format!("Object {} of {}", d + 1, tot));
        let Some(r) = &o.rendered else { continue };
        let (ulocal, uexternal) = (o.settings.use_local_links, o.settings.use_external_links);
        if !ulocal && !uexternal {
            continue;
        }
        let s = r.pt_per_px;
        for l in &r.links {
            if l.href.is_empty() || l.href.starts_with("__WKANCHOR_") {
                continue;
            }
            let Some(resolved) = &l.resolved else { continue };
            let Some((pi, off)) = locate(&o.slices, l.rect.y0 as f32) else { continue };
            let x0 = setup.margin_left + l.rect.x0 as f32 * s;
            let y0 = setup.margin_top + off * s;
            let rect = (x0, y0, x0 + l.rect.width() as f32 * s, y0 + l.rect.height() as f32 * s);
            let page = first_page[d] + pi;
            if let Some(&target) = url_to_obj.get(&url_key(resolved)) {
                if !ulocal {
                    continue;
                }
                let t = &objs[target];
                let Some(tr) = &t.rendered else { continue };
                let pos = match resolved.fragment() {
                    None => Some((0.0, 0.0)),
                    Some(f) => tr.anchors.find(f),
                };
                let Some((ax, ay)) = pos else { continue };
                let Some((tp, toff)) = locate(&t.slices, ay) else { continue };
                links.push(PdfLink {
                    page,
                    rect,
                    target: LinkTarget::Page {
                        page: first_page[target] + tp,
                        x: setup.margin_left + ax * tr.pt_per_px,
                        y: setup.margin_top + toff * tr.pt_per_px,
                    },
                });
            } else if uexternal {
                let uri = if g.resolve_relative_links { resolved.to_string() } else { l.href.clone() };
                links.push(PdfLink { page, rect, target: LinkTarget::Uri(uri) });
            }
        }
    }

    // Loading headers and footers (phase 4). TODO(P3).
    phase(pf, 4);

    // Printing pages (phase 5).
    phase(pf, 5);
    pf.progress_changed(0, "Preparing");
    let blank = blank_page(&setup, &fonts);
    let mut parts: Vec<DocPart<'_>> = Vec::new();
    let mut part_links: Vec<PdfLink> = Vec::new();
    let collate = g.collate;
    let cc = if collate { copies } else { 1 };
    let per_page_copies = if collate { 1 } else { copies };
    let mut actual_page = 0usize;
    let blank_slice = [PageSlice { y0: 0.0, y1: 1.0, thead: None, tfoot: None }];
    for _ in 0..cc {
        for (d, o) in objs.iter().enumerate() {
            let pages: Vec<(&RenderedPage, &PageSlice)> = if o.settings.is_table_of_content {
                vec![(&blank, &blank_slice[0])]
            } else {
                let skip = o.loader_index.is_none_or(|i| loader.resources[i].skip);
                match (&o.rendered, skip) {
                    (Some(r), false) => o.slices.iter().map(|s| (r, s)).collect(),
                    _ => Vec::new(),
                }
            };
            for (pi, (r, s)) in pages.into_iter().enumerate() {
                for _ in 0..per_page_copies {
                    actual_page += 1;
                    pf.progress_changed(
                        (actual_page * 100 / actual_pages.max(1)) as i32,
                        &format!("Page {actual_page} of {actual_pages}"),
                    );
                    let out_index = parts.len();
                    let src_page = first_page[d] + pi;
                    for l in links.iter().filter(|l| l.page == src_page) {
                        part_links.push(PdfLink { page: out_index, ..l.clone() });
                    }
                    parts.push(DocPart { page: r, slices: std::slice::from_ref(s) });
                }
            }
        }
    }
    let bookmarks = if g.outline { outline.pdf_outline(g.outline_depth) } else { Vec::new() };
    let info = DocInfo {
        title: if title.is_empty() { None } else { Some(title.clone()) },
        creator: CREATOR.into(),
        producer: PRODUCER.into(),
        creation_time: Some(now_secs()),
    };
    let pdf = if parts.is_empty() {
        // Zero pages (e.g. every object skipped): krilla needs one page.
        wk_print::write_pdf(&setup, &[DocPart { page: &blank, slices: &blank_slice }], &[], &[], &info)
    } else {
        wk_print::write_pdf(&setup, &parts, &part_links, &bookmarks, &info)
    };
    let pdf = match pdf {
        Ok(p) => p,
        Err(e) => {
            pf.error(&format!("PDF generation failed: {e}"));
            return false;
        }
    };
    if !g.dump_outline.is_empty() {
        let _ = std::fs::write(&g.dump_outline, outline.dump(page_offset));
    }
    match &mut output {
        Output::Stdout => stdout.extend_from_slice(&pdf),
        Output::File(f) => {
            if f.write_all(&pdf).is_err() {
                pf.error("Unable to write to destination");
                return false;
            }
        }
    }
    loader.clear_resources();
    phase(pf, 6);
    true
}

/// Qt prints its own warnings straight to stderr, whatever the log level.
fn pf_raw(pf: &mut Pf<'_>, s: &str) {
    pf.raw(s);
}

/// `QString::simplified`.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Page (index within the object) and offset (px from the top of the
/// content area) of document position `y`.
fn locate(slices: &[PageSlice], y: f32) -> Option<(usize, f32)> {
    slices.iter().enumerate().find_map(|(i, s)| s.offset_of(y).map(|o| (i, o))).or_else(|| {
        // Positions past the last band belong to the last page.
        let last = slices.len().checked_sub(1)?;
        let s = &slices[last];
        (y >= s.y1).then(|| (last, s.thead_height() + (s.y1 - s.y0)))
    })
}

/// An empty page (TOC placeholder, zero-page documents).
fn blank_page(setup: &PageSetup, fonts: &parley::FontContext) -> RenderedPage {
    struct NoFetch;
    impl Fetcher for NoFetch {
        fn create(&mut self, _: &Url) -> usize {
            0
        }
        fn finish(&mut self, _: usize) -> Option<Vec<u8>> {
            None
        }
    }
    let url = Url::parse("about:blank").expect("valid url");
    let mut p = WebPage::load("", url, WebSettings::default(), fonts.clone(), &mut NoFetch);
    p.render(setup.printable_width(), setup.printable_height())
}

/// Whether progress output is enabled for `level` (used by tests).
pub fn shows_progress(level: LogLevel) -> bool {
    level >= LogLevel::Info
}
