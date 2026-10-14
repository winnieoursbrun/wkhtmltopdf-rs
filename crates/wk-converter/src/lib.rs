//! The PDF conversion pipeline. Ports upstream `src/lib/pdfconverter.cc`
//! (`PdfConverterPrivate`: phases, header/footer measuring, page counting,
//! TOC loading, link resolution, header/footer loading, printing),
//! `src/lib/converter.cc` (progress plumbing) and `src/lib/utilities.cc`
//! (`handleError`), with the observable behaviour of the 0.12.6 binary
//! (spec §3.1, §3.4–§3.6, §4).
//!
//! Phases (patched Qt): `Loading pages`, `Counting pages`, `Loading TOC`
//! (only with a TOC object), `Resolving links`, `Loading headers and
//! footers`, `Printing pages`, `Done`.
//!
//! JavaScript (P4) runs inside `WebPage::load` for every loaded page
//! (documents, TOC, measuring and per-page header/footer copies, whose
//! `subst()` script reads `location.search`), with the object's
//! `--javascript-delay`, `--debug-javascript` and slow-script settings;
//! `--run-script` and `--window-status` apply to documents only (the main
//! loader), see [`scripts::PageKind`].

pub mod headerfooter;
pub mod outline;
pub mod scripts;
pub mod toc;
pub mod tocstylesheet;
pub mod utilities;
pub mod xml;
pub mod xslt;

use std::collections::HashMap;
use std::io::{Read, Write};

use url::Url;
use wk_cli::progressfeedback::ProgressFeedback;
use wk_loader::{Feedback, MultiPageLoader, PendingRequest};
use wk_print::{
    Anchor, Band, DocInfo, FontFace, FormField, LineItem, LinkTarget, OutPage, PageSlice, PdfLink,
    PdfOptions, TextItem,
};
use wk_settings::{ColorMode, LogLevel, PdfGlobal, PdfObject};
use wk_webkit::{Fetcher, RenderedPage, WebPage, WebSettings};

use crate::headerfooter::{Parms, hfreplace};
use crate::outline::{BackLink, LocatedHeading, Outline};
use crate::scripts::PageKind;
use crate::utilities::{handle_error, paper_size, points_per_unit};

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
/// `PdfConverter::millimeterToPointMultiplier` (points per mm).
/// `calculateHeaderHeight` divides a height in device pixels by it, so a
/// measured header reserves 4/3 of its printed height at 96 dpi.
const MM_TO_POINT_MULTIPLIER: f64 = 2.83464567;

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
struct PageFetcher<'a, 'b> {
    loader: &'a mut MultiPageLoader,
    index: usize,
    pf: &'a mut Pf<'b>,
    pending: Vec<Option<PendingRequest>>,
}

impl Fetcher for PageFetcher<'_, '_> {
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

/// A located link of a document: output-page index within the object,
/// rectangle in points (top-left origin), target.
#[derive(Debug, Clone)]
struct ObjLink {
    page: usize,
    rect: (f32, f32, f32, f32),
    target: LinkTarget,
}

/// One object of the conversion (`PageObject`).
struct PageObject<'s> {
    settings: &'s PdfObject,
    /// Index in the page loader (documents) or in the TOC loader (TOC).
    loader_index: Option<usize>,
    /// The loader marked the object as skipped (`--load-error-handling skip`).
    skip: bool,
    /// The loaded page, dropped once rendered: the DOM, styles and layout
    /// are not needed to print, and freeing them lowers the peak memory.
    page: Option<WebPage>,
    /// `url_key` of the loaded page's URL.
    url: Option<String>,
    rendered: Option<RenderedPage>,
    slices: Vec<PageSlice>,
    /// Counted pages (`pageCount`; 0 with `pagesCount == false`).
    page_count: usize,
    title: String,
    /// Index of the object's document in the outline.
    outline_doc: Option<usize>,
    header_reserve: f64,
    footer_reserve: f64,
    measure_header: Option<usize>,
    measure_footer: Option<usize>,
    local_links: Vec<ObjLink>,
    external_links: Vec<ObjLink>,
    /// Named anchors (`anchors`): name, page index, position.
    anchors: Vec<(String, usize, f32, f32)>,
    headers: Vec<RenderedPage>,
    footers: Vec<RenderedPage>,
}

impl PageObject<'_> {
    /// `!loaderObject || loaderObject->skip`.
    fn is_skipped(&self) -> bool {
        self.loader_index.is_none() || self.skip
    }
}

/// `looksLikeHtmlAndNotAUrl`.
fn looks_like_html_and_not_a_url(s: &str) -> bool {
    let before_query = s.split('?').next().unwrap_or("");
    before_query.contains('<') || s.to_ascii_lowercase().starts_with("data:")
}

/// `--viewport-size WxH`: split on `x`, the first and last parts as integers;
/// applied only when valid and not empty (`QSize::isValid && !isEmpty`).
fn viewport_size(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split('x').collect();
    let w = parts.first()?.trim().parse::<i64>().ok()?;
    let h = parts.last()?.trim().parse::<i64>().ok()?;
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// The engine settings of a page of `kind`; `app_name` gives the
/// `User-Agent` (also `navigator.userAgent`); `viewport` is
/// `--viewport-size`.
fn web_settings(
    o: &PdfObject,
    user_css: Option<String>,
    app_name: &str,
    kind: PageKind,
    viewport: Option<(u32, u32)>,
    print_area: Option<(f32, f32)>,
) -> WebSettings {
    let main = kind.is_main_loader();
    WebSettings {
        viewport,
        print_media_type: o.load.print_media_type,
        background: o.web.background,
        load_images: o.web.load_images,
        enable_javascript: o.web.enable_javascript,
        minimum_font_size: o.web.minimum_font_size,
        default_encoding: o.web.default_encoding.clone(),
        user_style_sheet: user_css,
        zoom: o.load.zoom_factor,
        smart_shrinking: o.web.enable_intelligent_shrinking,
        produce_forms: o.produce_forms,
        javascript_delay: o.load.jsdelay.max(0) as u32,
        window_status: if main { o.load.window_status.clone() } else { String::new() },
        run_script: if main { o.load.run_script.clone() } else { Vec::new() },
        debug_javascript: o.load.debug_javascript,
        stop_slow_scripts: o.load.stop_slow_scripts,
        user_agent: wk_loader::user_agent(app_name),
        print_area,
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
    String::from_utf8_lossy(&out).into_owned()
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
    let mut conv = Converter {
        g: global.clone(),
        pf: &mut pf,
        error_code: &mut error_code,
        app_name: &app_name,
        fonts: wk_fonts::font_context(&wk_fonts::FontOptions::default()),
        temp: toc::TempFiles::default(),
        print_area: None,
    };
    let success = conv.run(objects, stdin, stdout);
    (success, error_code)
}

/// Paper geometry of the conversion, in points.
#[derive(Debug, Clone, Copy)]
struct Paper {
    width: f32,
    height: f32,
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl Paper {
    /// QPrinter's geometry in whole device pixels at `dpi`: the paper rect
    /// is the paper size in points scaled and rounded, the page rect is
    /// the paper rect less each margin rounded (A4 at 96 dpi is 793 × 1123
    /// px; 10 mm margins are 38 px, so the body starts at 28.5 pt and is
    /// 717 px wide, as in the reference PDFs). `margins` are top, right,
    /// bottom, left in points; the result keeps the PDF page size in points.
    fn snapped(width: f64, height: f64, margins: [f64; 4], dpi: i32) -> Paper {
        let k = f64::from(dpi.max(1)) / 72.0;
        let px = |pt: f64| (pt * k).round();
        let (w, h) = (px(width), px(height));
        let [t, r, b, l] = margins.map(px);
        Paper {
            width: width as f32,
            height: height as f32,
            top: (t / k) as f32,
            left: (l / k) as f32,
            right: (width - (w - r) / k) as f32,
            bottom: (height - (h - b) / k) as f32,
        }
    }

    fn printable_width(&self) -> f32 {
        (self.width - self.left - self.right).max(1.0)
    }
    fn printable_height(&self) -> f32 {
        (self.height - self.top - self.bottom).max(1.0)
    }
}

struct Converter<'a, 'b> {
    g: PdfGlobal,
    pf: &'a mut Pf<'b>,
    error_code: &'a mut i32,
    app_name: &'a str,
    fonts: parley::FontContext,
    temp: toc::TempFiles,
    /// Printable area (points) the pages being loaded will be rendered on
    /// (`WebSettings::print_area`).
    print_area: Option<(f32, f32)>,
}

impl Converter<'_, '_> {
    fn phase(&mut self, i: usize) {
        self.pf.phase_changed(PHASES[i], i, PHASES.len());
    }

    /// Load every resource of `loader` in order (`MultiPageLoader::load`).
    /// `specs[i]` gives the settings and kind of resource `i`. Returns the
    /// pages, or `None` when an abort-mode failure happened.
    fn load_all(
        &mut self,
        loader: &mut MultiPageLoader,
        specs: &[(&PdfObject, PageKind)],
    ) -> Option<Vec<Option<WebPage>>> {
        let n = loader.resources.len();
        {
            let mut fb = LoadFeedback { pf: &mut *self.pf };
            for i in 0..n {
                loader.set_progress(i, 10, &mut fb);
            }
        }
        let mut pages: Vec<Option<WebPage>> = (0..n).map(|_| None).collect();
        for i in 0..n {
            let (settings, kind) = specs[i];
            let (ok, reply) = {
                let mut fb = LoadFeedback { pf: &mut *self.pf };
                loader.load_main(i, &mut fb)
            };
            if !ok {
                let mut fb = LoadFeedback { pf: &mut *self.pf };
                loader.load_finished(i, false, &mut fb);
                if loader.has_error {
                    break;
                }
                // Ignored failure: an empty page is printed.
                let ws = web_settings(
                    settings,
                    None,
                    self.app_name,
                    kind,
                    viewport_size(&self.g.viewport_size),
                    self.print_area,
                );
                let mut f =
                    PageFetcher { loader: &mut *loader, index: i, pf: &mut *self.pf, pending: vec![] };
                pages[i] = Some(WebPage::load("", reply.final_url.clone(), ws, self.fonts.clone(), &mut f));
                continue;
            }
            {
                let mut fb = LoadFeedback { pf: &mut *self.pf };
                loader.set_progress(i, 50, &mut fb);
            }
            let html = wk_webkit::encoding::decode_html(
                &reply.body,
                reply.content_type.as_deref(),
                &settings.web.default_encoding,
            );
            let ws = web_settings(
                settings,
                user_style_sheet(&settings.web.user_style_sheet),
                self.app_name,
                kind,
                viewport_size(&self.g.viewport_size),
                self.print_area,
            );
            let page = {
                let mut f =
                    PageFetcher { loader: &mut *loader, index: i, pf: &mut *self.pf, pending: vec![] };
                WebPage::load(&html, reply.final_url.clone(), ws, self.fonts.clone(), &mut f)
            };
            pages[i] = Some(page);
            let mut fb = LoadFeedback { pf: &mut *self.pf };
            loader.set_progress(i, 100, &mut fb);
            loader.load_finished(i, true, &mut fb);
            if loader.has_error {
                break;
            }
        }
        loader.load_done();
        if loader.has_error { None } else { Some(pages) }
    }

    #[allow(clippy::too_many_lines)]
    fn run(&mut self, objects: &[PdfObject], stdin: &mut dyn Read, stdout: &mut Vec<u8>) -> bool {
        // beginConvert
        let mut loader = MultiPageLoader::new(&self.g.load, true, self.app_name);
        let mut measuring = MultiPageLoader::new(&self.g.load, false, self.app_name);
        let mut measuring_specs: Vec<(&PdfObject, PageKind)> = Vec::new();
        let mut header_heights_calc_needed = false;
        let mut objs: Vec<PageObject<'_>> = Vec::new();
        for s in objects {
            let mut o = PageObject {
                settings: s,
                loader_index: None,
                skip: false,
                page: None,
                url: None,
                rendered: None,
                slices: Vec::new(),
                page_count: 0,
                title: String::new(),
                outline_doc: None,
                header_reserve: 0.0,
                footer_reserve: 0.0,
                measure_header: None,
                measure_footer: None,
                local_links: Vec::new(),
                external_links: Vec::new(),
                anchors: Vec::new(),
                headers: Vec::new(),
                footers: Vec::new(),
            };
            for (hf, is_header) in [(&s.header, true), (&s.footer, false)] {
                if hf.html_url.is_empty() {
                    continue;
                }
                if looks_like_html_and_not_a_url(&hf.html_url) {
                    let name = if is_header { "--header-html" } else { "--footer-html" };
                    self.pf.error(&format!("{name} should be a URL and not a string containing HTML code."));
                    return false;
                }
                let margin = if is_header { self.g.margin.top.value } else { self.g.margin.bottom.value };
                if margin == -1.0 {
                    header_heights_calc_needed = true;
                    let i = match measuring.add_resource(&hf.html_url, &s.load, stdin) {
                        Ok(i) => i,
                        Err(e) => {
                            self.pf.error(&e);
                            return false;
                        }
                    };
                    measuring_specs.push((s, PageKind::HeaderFooter));
                    if is_header {
                        o.measure_header = Some(i);
                    } else {
                        o.measure_footer = Some(i);
                    }
                } else if is_header {
                    o.header_reserve = margin + f64::from(hf.spacing);
                } else {
                    o.footer_reserve = margin + f64::from(hf.spacing);
                }
            }
            if !s.is_table_of_content {
                match loader.add_resource(&s.page, &s.load, stdin) {
                    Ok(i) => o.loader_index = Some(i),
                    Err(e) => {
                        self.pf.error(&e);
                        return false;
                    }
                }
            }
            objs.push(o);
        }
        self.phase(0);
        self.pf.progress_changed(0, "0%");

        if header_heights_calc_needed {
            // measuringHeadersLoaded: render each header/footer once and
            // reserve its body height (mm) plus the spacing.
            let (pw, ph) = paper_size(&self.g);
            let unit = |v: f64, u| v * points_per_unit(u, self.g.dpi);
            let right = unit(self.g.margin.right.value, self.g.margin.right.unit);
            let left = unit(self.g.margin.left.value, self.g.margin.left.unit);
            let printable_w = Paper::snapped(pw, ph, [0.0, right, 0.0, left], self.g.dpi).printable_width();
            self.print_area = Some((printable_w, ph as f32));
            let pages = self.load_all(&mut measuring, &measuring_specs);
            self.print_area = None;
            if *self.error_code == 0 {
                *self.error_code = measuring.http_error_code();
            }
            let Some(mut pages) = pages else { return false };
            let dev_per_pt = f64::from(self.g.dpi.max(1)) / 72.0;
            // `calculateHeaderHeight`: the body height printed on a test
            // printer (same shrink as the page), in whole device pixels
            // (QRect, truncated), divided by millimeterToPointMultiplier
            // (points per mm). Verified with the 0.12.6 binary on bodies of
            // 10 to 400 px: a 77 px body prints 61 px tall and reserves
            // 61 / 2.835 mm = 81 px (hf-html-static), a 100 px body 107 px.
            let mut measure = |i: Option<usize>| -> Option<f64> {
                // Each measured page is printed once: render_final frees its text as painted.
                let p = pages.get_mut(i?)?.take()?;
                let r = p.render_final(printable_w, ph as f32);
                let printed = (f64::from(r.body_height * r.pt_per_px) * dev_per_pt + 1e-6).floor();
                Some(printed / MM_TO_POINT_MULTIPLIER)
            };
            for o in objs.iter_mut() {
                if let Some(h) = measure(o.measure_header) {
                    o.header_reserve = h + f64::from(o.settings.header.spacing);
                }
                if let Some(h) = measure(o.measure_footer) {
                    o.footer_reserve = h + f64::from(o.settings.footer.spacing);
                }
            }
            measuring.clear_resources();
        } else {
            if self.g.margin.top.value == -1.0 {
                self.g.margin.top.value = 10.0;
            }
            if self.g.margin.bottom.value == -1.0 {
                self.g.margin.bottom.value = 10.0;
            }
            for o in objs.iter_mut() {
                o.header_reserve = self.g.margin.top.value;
                o.footer_reserve = self.g.margin.bottom.value;
            }
        }

        // Load all pages.
        let specs: Vec<(&PdfObject, PageKind)> = objs
            .iter()
            .filter(|o| o.loader_index.is_some())
            .map(|o| (o.settings, PageKind::Document))
            .collect();
        // The paper is known before the pages load (it depends on the
        // measured headers only): they are laid out at the print viewport.
        let m = self.g.margin;
        let max_header = objs
            .iter()
            .map(|o| o.header_reserve)
            .fold(objs.first().map_or(0.0, |o| o.header_reserve), f64::max);
        let max_footer = objs
            .iter()
            .map(|o| o.footer_reserve)
            .fold(objs.first().map_or(0.0, |o| o.footer_reserve), f64::max);
        let (paper_w, paper_h) = paper_size(&self.g);
        // Margins are in the left margin's unit (measured heights are mm
        // values interpreted in that unit, like upstream).
        let ppu = points_per_unit(m.left.unit, self.g.dpi);
        let paper = Paper::snapped(
            paper_w,
            paper_h,
            [max_header * ppu, m.right.value * ppu, max_footer * ppu, m.left.value * ppu],
            self.g.dpi,
        );

        self.print_area = Some((paper.printable_width(), paper.printable_height()));
        let pages = self.load_all(&mut loader, &specs);
        self.print_area = None;
        // pagesLoaded
        if *self.error_code == 0 {
            *self.error_code = loader.http_error_code();
        }
        let Some(mut pages) = pages else { return false };
        for o in objs.iter_mut() {
            if let Some(i) = o.loader_index {
                o.page = pages[i].take();
                o.url = o.page.as_ref().map(|p| url_key(&p.url));
                o.skip = loader.resources[i].skip;
            }
        }

        let m = self.g.margin;
        if m.left.unit != m.right.unit || m.left.unit != m.top.unit || m.left.unit != m.bottom.unit {
            self.pf.error("Currently all margin units must be the same!");
            return false;
        }
        // painter->begin(printer): the output is opened before printing.
        let mut output = if self.g.out == "-" || self.g.out.is_empty() {
            Output::Stdout
        } else {
            match std::fs::File::create(&self.g.out) {
                Ok(f) => Output::File(f),
                Err(_) => {
                    self.pf.raw("QPainter::begin(): Returned false\n");
                    self.pf.error("Unable to write to destination");
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
        let mut title = self.g.document_title.clone();
        if title.is_empty() {
            for o in &objs {
                if o.is_skipped() || o.settings.is_table_of_content {
                    continue;
                }
                if !o.title.is_empty() {
                    title = o.title.clone();
                    break;
                }
            }
        }

        // Counting pages (phase 1): preprocessPage.
        self.phase(1);
        let tot = objs.len();
        let mut outline = Outline::default();
        let mut page_count = 0usize;
        for (d, o) in objs.iter_mut().enumerate() {
            if o.settings.is_table_of_content {
                o.page_count = 1;
                page_count += 1;
                outline.add_empty_web_page();
                o.outline_doc = Some(outline.documents.len() - 1);
                continue;
            }
            if o.is_skipped() {
                continue;
            }
            self.pf.progress_changed(((d + 1) * 100 / tot) as i32, &format!("Object {} of {}", d + 1, tot));
            let Some(page) = o.page.take() else { continue };
            // The page's DOM (styles, layout, parley layouts) is dropped once rendered:
            // only its URL key (`o.url`) is used from here on.
            let rendered = page.render_final(paper.printable_width(), paper.printable_height());
            o.slices = wk_print::paginate(&rendered.geometry, paper.printable_height() / rendered.pt_per_px);
            o.page_count = if o.settings.pages_count { o.slices.len() } else { 0 };
            page_count += o.page_count;
            let ps = o.settings;
            if ps.include_in_outline {
                let headings = located_headings(&rendered, &o.slices, &paper);
                let back = outline.add_web_page(
                    &o.title,
                    o.slices.len(),
                    &headings,
                    ps.toc.forward_links,
                    ps.toc.back_links,
                );
                o.local_links.extend(back_links(back));
            } else {
                outline.add_empty_web_page();
            }
            o.outline_doc = Some(outline.documents.len() - 1);
            o.rendered = Some(rendered);
        }
        let copies = self.g.copies.max(1) as usize;
        let mut actual_pages = page_count * copies;

        // loadTocs / tocLoaded, until the TOC no longer changes.
        if objs.iter().any(|o| o.settings.is_table_of_content) {
            let mut announced = false;
            loop {
                let mut toc_loader = MultiPageLoader::new(&self.g.load, false, self.app_name);
                let mut toc_specs: Vec<(&PdfObject, PageKind)> = Vec::new();
                for o in objs.iter_mut() {
                    let ps = o.settings;
                    if !ps.is_table_of_content {
                        continue;
                    }
                    o.local_links.clear();
                    o.external_links.clear();
                    o.anchors.clear();
                    let xsl = match toc::style_sheet(ps) {
                        Ok(x) => x,
                        Err(_) => {
                            self.pf.error("Could not read the TOC XSL");
                            return false;
                        }
                    };
                    let xml = outline.dump(self.g.page_offset);
                    let _ = self.temp.write(".xml", xml.as_bytes());
                    let (html, err) = toc::transform(&xsl, &xml);
                    if let Some(e) = err {
                        self.pf.raw(&format!("{e}\n"));
                    }
                    if std::env::var_os("WKHTMLTOPDF_DEBUG_TOC").is_some() {
                        eprintln!("{html}");
                    }
                    let Some(path) = self.temp.write(".html", html.as_bytes()) else {
                        self.pf.error("Could not open the TOC for writing");
                        return false;
                    };
                    match toc_loader.add_resource(&path.display().to_string(), &ps.load, stdin) {
                        Ok(i) => o.loader_index = Some(i),
                        Err(e) => {
                            self.pf.error(&e);
                            return false;
                        }
                    }
                    toc_specs.push((ps, PageKind::Toc));
                }
                if !announced {
                    announced = true;
                    self.phase(2);
                }
                self.print_area = Some((paper.printable_width(), paper.printable_height()));
                let pages = self.load_all(&mut toc_loader, &toc_specs);
                self.print_area = None;
                if *self.error_code == 0 {
                    *self.error_code = toc_loader.http_error_code();
                }
                let Some(mut pages) = pages else { return false };
                // tocLoaded
                let mut toc_changed = false;
                page_count = 0;
                for o in objs.iter_mut() {
                    if !o.settings.is_table_of_content {
                        if !o.is_skipped() {
                            page_count += o.page_count;
                        }
                        continue;
                    }
                    let Some(i) = o.loader_index else { continue };
                    o.skip = toc_loader.resources[i].skip;
                    let Some(page) = pages[i].take() else { continue };
                    if o.skip {
                        o.page = Some(page);
                        continue;
                    }
                    // handleTocPage
                    o.title = page.title().map(|t| collapse_ws(&t)).unwrap_or_default();
                    let rendered = page.render_final(paper.printable_width(), paper.printable_height());
                    o.slices =
                        wk_print::paginate(&rendered.geometry, paper.printable_height() / rendered.pt_per_px);
                    let pc = if o.settings.pages_count { o.slices.len() } else { 0 };
                    if pc != o.page_count {
                        o.page_count = pc;
                        toc_changed = true;
                    }
                    page_count += o.page_count;
                    let headings = located_headings(&rendered, &o.slices, &paper);
                    let ps = o.settings;
                    let doc = o.outline_doc.expect("toc outline document");
                    let (changed, back) = outline.replace_web_page(
                        doc,
                        &ps.toc.caption_text,
                        o.slices.len(),
                        &headings,
                        ps.toc.forward_links,
                        ps.toc.back_links,
                    );
                    toc_changed |= changed;
                    o.local_links.extend(back_links(back));
                    o.rendered = Some(rendered);
                }
                actual_pages = page_count * copies;
                toc_loader.clear_resources();
                if !toc_changed {
                    break;
                }
            }
        }

        // Resolving links (phase 3).
        self.phase(3);
        let mut url_to_obj: HashMap<String, usize> = HashMap::new();
        for (d, o) in objs.iter().enumerate() {
            if o.is_skipped() || o.settings.is_table_of_content {
                continue;
            }
            if let Some(u) = &o.url {
                url_to_obj.insert(u.clone(), d);
            }
        }
        // Targets registered on other objects: (object, name, page, x, y).
        let mut foreign_anchors: Vec<(usize, String, usize, f32, f32)> = Vec::new();
        for d in 0..objs.len() {
            if objs[d].is_skipped() {
                continue;
            }
            self.pf.progress_changed(((d + 1) * 100 / tot) as i32, &format!("Object {} of {}", d + 1, tot));
            let o = &objs[d];
            let (ulocal, uexternal) = (o.settings.use_local_links, o.settings.use_external_links);
            if !ulocal && !uexternal {
                continue;
            }
            let Some(r) = &o.rendered else { continue };
            let (mut local, mut external, mut anchors) = (Vec::new(), Vec::new(), Vec::new());
            find_links(
                r,
                &o.slices,
                &paper,
                (ulocal, uexternal),
                self.g.resolve_relative_links,
                &url_to_obj,
                &objs,
                &mut local,
                &mut external,
                &mut anchors,
                &mut foreign_anchors,
            );
            let o = &mut objs[d];
            o.local_links.extend(local);
            o.external_links.extend(external);
            o.anchors.extend(anchors);
        }
        for (t, name, page, x, y) in foreign_anchors {
            objs[t].anchors.push((name, page, x, y));
        }

        // Loading headers and footers (phase 4): loadHeaders.
        self.phase(4);
        let mut hf_loader = MultiPageLoader::new(&self.g.load, false, self.app_name);
        let mut hf_specs: Vec<(&PdfObject, PageKind)> = Vec::new();
        // (object, is_header) per hf loader resource.
        let mut hf_owner: Vec<(usize, bool)> = Vec::new();
        let (time, date, isodate) = headerfooter::time_and_dates(now_secs());
        let mut page_number = 1usize;
        for (d, o) in objs.iter().enumerate() {
            if o.is_skipped() {
                continue;
            }
            let ps = o.settings;
            for op in 0..o.page_count {
                if !ps.header.html_url.is_empty() || !ps.footer.html_url.is_empty() {
                    let mut parms =
                        fill_parms(&mut outline, page_number, o, &title, &self.g, (&time, &date, &isodate));
                    parms.set("sitepage", (op + 1).to_string());
                    parms.set("sitepages", o.page_count.to_string());
                    for (hf, is_header) in [(&ps.header, true), (&ps.footer, false)] {
                        if hf.html_url.is_empty() {
                            continue;
                        }
                        let base = wk_loader::urlguess::guess_url_from_string(&hf.html_url);
                        let url = Url::parse(&base)
                            .map(|u| headerfooter::header_footer_url(&u, &parms).to_string())
                            .unwrap_or(base);
                        if hf_loader.add_resource(&url, &ps.load, stdin).is_ok() {
                            hf_specs.push((ps, PageKind::HeaderFooter));
                            hf_owner.push((d, is_header));
                        }
                    }
                }
                if ps.pages_count {
                    page_number += 1;
                }
            }
        }
        if !hf_specs.is_empty() {
            // The header page is printed with zero vertical margins.
            self.print_area = Some((paper.printable_width(), paper.height));
            let pages = self.load_all(&mut hf_loader, &hf_specs);
            self.print_area = None;
            if *self.error_code == 0 {
                *self.error_code = hf_loader.http_error_code();
            }
            let Some(pages) = pages else { return false };
            for (p, &(d, is_header)) in pages.into_iter().zip(&hf_owner) {
                let Some(p) = p else { continue };
                // The header page is printed with zero vertical margins.
                let r = p.render_final(paper.printable_width(), paper.height);
                if is_header {
                    objs[d].headers.push(r);
                } else {
                    objs[d].footers.push(r);
                }
            }
        }

        // Printing pages (phase 5): printDocument.
        self.phase(5);
        self.pf.progress_changed(0, "Preparing");
        let hf_slices: HashMap<(usize, bool, usize), PageSlice> = objs
            .iter()
            .enumerate()
            .flat_map(|(d, o)| {
                let h = o
                    .headers
                    .iter()
                    .enumerate()
                    .map(move |(i, r)| ((d, true, i), first_slice(r, paper.height)));
                let f = o
                    .footers
                    .iter()
                    .enumerate()
                    .map(move |(i, r)| ((d, false, i), first_slice(r, paper.height)));
                h.chain(f)
            })
            .collect();
        let mut out_pages: Vec<OutPage<'_>> = Vec::new();
        // First output page of each object in the first copy.
        let mut first_page: HashMap<usize, usize> = HashMap::new();
        let collate = self.g.collate;
        let cc = if collate { copies } else { 1 };
        let per_page_copies = if collate { 1 } else { copies };
        let mut actual_page = 1usize;
        let mut faces: HashMap<String, Option<FontFace>> = HashMap::new();
        for _ in 0..cc {
            let mut page_number = 1usize;
            for (d, o) in objs.iter().enumerate() {
                // beginPrintObject
                if o.is_skipped() {
                    continue;
                }
                let Some(r) = &o.rendered else { continue };
                first_page.entry(d).or_insert(out_pages.len());
                let ps = o.settings;
                let has_text_hf = ps.header.line
                    || ps.footer.line
                    || !ps.header.left.is_empty()
                    || !ps.footer.left.is_empty()
                    || !ps.header.center.is_empty()
                    || !ps.footer.center.is_empty()
                    || !ps.header.right.is_empty()
                    || !ps.footer.right.is_empty();
                let mut anchors: Vec<(String, usize, f32, f32)> = Vec::new();
                if let Some(doc) = o.outline_doc {
                    for (name, page, x, y) in outline.child_anchors(doc) {
                        anchors.push((name, page.saturating_sub(1), x, y));
                    }
                }
                anchors.extend(o.anchors.iter().cloned());
                for (pi, slice) in o.slices.iter().enumerate() {
                    for _ in 0..per_page_copies {
                        // spoolPage
                        self.pf.progress_changed(
                            (actual_page * 100 / actual_pages.max(1)) as i32,
                            &format!("Page {actual_page} of {actual_pages}"),
                        );
                        let mut op = OutPage {
                            bands: vec![Band { page: r, slice, origin: (paper.left, paper.top) }],
                            ..Default::default()
                        };
                        if ps.produce_forms {
                            op.fields = form_fields(r, &o.slices, pi, &paper);
                        }
                        for (name, page, x, y) in &anchors {
                            if *page == pi {
                                op.anchors.push(Anchor { name: name.clone(), x: *x, y: *y });
                            }
                        }
                        for l in o.local_links.iter().chain(&o.external_links) {
                            if l.page == pi {
                                op.links.push(PdfLink { rect: l.rect, target: l.target.clone() });
                            }
                        }
                        // endPage
                        let needs_parms = has_text_hf;
                        let parms = needs_parms.then(|| {
                            let mut p = fill_parms(
                                &mut outline,
                                page_number,
                                o,
                                &title,
                                &self.g,
                                (&time, &date, &isodate),
                            );
                            p.set("sitepage", (pi + 1).to_string());
                            p.set("sitepages", o.page_count.to_string());
                            p
                        });
                        if let Some(parms) = &parms {
                            self.text_header_footer(&mut op, ps, parms, &paper, &mut faces);
                        }
                        if let Some(h) = o.headers.get(pi) {
                            let s = hf_slices.get(&(d, true, pi)).expect("header slice");
                            let spacing = mm_to_pt(ps.header.spacing);
                            // translate(0, -elementLocation(body).height()):
                            // whole device pixels (a 77 px body at 0.8 is 61).
                            let dev = self.g.dpi.max(1) as f32 / 72.0;
                            let body = (h.body_height * h.pt_per_px * dev + 1e-3).floor() / dev;
                            let origin = (paper.left, paper.top - spacing - body);
                            push_hf_band(
                                &mut op,
                                h,
                                s,
                                origin,
                                &url_to_obj,
                                self.g.resolve_relative_links,
                                paper.top,
                            );
                        }
                        if let Some(f) = o.footers.get(pi) {
                            let s = hf_slices.get(&(d, false, pi)).expect("footer slice");
                            let spacing = mm_to_pt(ps.footer.spacing);
                            let origin = (paper.left, paper.top + paper.printable_height() + spacing);
                            push_hf_band(
                                &mut op,
                                f,
                                s,
                                origin,
                                &url_to_obj,
                                self.g.resolve_relative_links,
                                paper.top,
                            );
                        }
                        out_pages.push(op);
                        actual_page += 1;
                    }
                    if ps.pages_count {
                        page_number += 1;
                    }
                }
            }
        }
        let bookmarks = if self.g.outline {
            let doc_obj: HashMap<usize, usize> =
                objs.iter().enumerate().filter_map(|(d, o)| o.outline_doc.map(|doc| (doc, d))).collect();
            outline.pdf_outline(self.g.outline_depth, &|doc| {
                doc_obj.get(&doc).and_then(|d| first_page.get(d)).copied()
            })
        } else {
            Vec::new()
        };
        let info = DocInfo {
            title: if title.is_empty() { None } else { Some(title.clone()) },
            creator: CREATOR.into(),
            producer: PRODUCER.into(),
            creation_time: Some(now_secs()),
        };
        let opts = PdfOptions {
            grayscale: self.g.color_mode == ColorMode::GrayScale,
            image_dpi: self.g.image_dpi.max(0) as u32,
        };
        let pdf = wk_print::write_pdf(
            paper.width,
            paper.height,
            &out_pages,
            &bookmarks,
            self.g.outline,
            &info,
            opts,
        );
        let pdf = match pdf {
            Ok(p) => p,
            Err(e) => {
                self.pf.error(&format!("PDF generation failed: {e}"));
                return false;
            }
        };
        if !self.g.dump_outline.is_empty() {
            let _ = std::fs::write(&self.g.dump_outline, outline.dump(self.g.page_offset));
        }
        match &mut output {
            Output::Stdout => stdout.extend_from_slice(&pdf),
            Output::File(f) => {
                if f.write_all(&pdf).is_err() {
                    self.pf.error("Unable to write to destination");
                    return false;
                }
            }
        }
        loader.clear_resources();
        hf_loader.clear_resources();
        self.phase(6);
        true
    }

    /// The text part of `endPage`: lines and the six texts, in the top and
    /// bottom margins, in device pixels of the printer converted to points.
    fn text_header_footer(
        &mut self,
        op: &mut OutPage<'_>,
        ps: &PdfObject,
        parms: &Parms,
        paper: &Paper,
        faces: &mut HashMap<String, Option<FontFace>>,
    ) {
        let w = paper.printable_width();
        let h = paper.printable_height();
        let dpi = self.g.dpi.max(1) as f32;
        let px = |pt: f32| pt * dpi / 72.0;
        let pt = |px: f32| px * 72.0 / dpi;
        let x_left = paper.left;
        // printer->height(): the printable height in whole device pixels.
        let h_px = px(h).round();
        for (hf, is_header) in [(&ps.header, true), (&ps.footer, false)] {
            let sp = px(mm_to_pt(hf.spacing));
            // drawLine(int, int, int, int): the spacing is truncated.
            let line_y = if is_header { -sp.trunc() } else { (h_px + sp).trunc() };
            if hf.line {
                op.lines.push(LineItem {
                    x0: x_left,
                    x1: x_left + w,
                    y: paper.top + pt(line_y),
                    width: pt(0.1),
                });
            }
            let face = faces
                .entry(hf.font_name.clone())
                .or_insert_with(|| {
                    wk_fonts::regular_face(&mut self.fonts, &hf.font_name)
                        .map(|(data, index)| FontFace { data, index })
                })
                .clone();
            let Some(face) = face else { continue };
            let size = hf.font_size.max(1) as f32;
            let (a, de) = face.ascent_descent(size);
            // FreeType metrics in whole device pixels (ascender ceiled,
            // descender floored), and dy = the "M" bounding rect height =
            // ascent + descent. The QRect coordinates are ints (truncated).
            // Verified on tests/parts: 16 px DejaVu Sans (15 + 4) puts the
            // baseline 4 px above the body, 21.33 px DejaVu Serif with 8 mm
            // spacing (20 + 6) 36 px above it.
            let ceil = |v: f32| (v - 1e-3).ceil();
            let (ascent, descent) = (ceil(px(a)), ceil(px(de)));
            let dy = ascent + descent;
            let baseline_px = if is_header {
                // QRect(0, -dy - spacing, w, h), AlignTop.
                (-dy - sp).trunc() + ascent
            } else {
                // QRect(0, 0, w, h + dy + spacing), AlignBottom.
                (h_px + dy + sp).trunc() - descent
            };
            let baseline = paper.top + pt(baseline_px);
            for (text, align) in [(&hf.left, 0), (&hf.center, 1), (&hf.right, 2)] {
                if text.is_empty() {
                    continue;
                }
                let t = hfreplace(text, parms);
                let tw = face.text_width(size, &t);
                let x = match align {
                    0 => x_left,
                    1 => x_left + (w - tw) / 2.0,
                    _ => x_left + w - tw,
                };
                op.texts.push(TextItem { face: face.clone(), size, x, baseline, text: t });
            }
        }
    }
}

/// `spacing * printer->height() / printer->heightMM()`, in points.
fn mm_to_pt(mm: f32) -> f32 {
    mm * 72.0 / 25.4
}

/// The first page of a header/footer document (`spoolPage(1)`), laid out on
/// the whole paper height.
fn first_slice(r: &RenderedPage, paper_h: f32) -> PageSlice {
    wk_print::paginate(&r.geometry, paper_h / r.pt_per_px).into_iter().next().unwrap_or(PageSlice {
        y0: 0.0,
        y1: 1.0,
        thead: None,
        tfoot: None,
    })
}

/// Draw a header/footer page and its links (`findLinks` on the header
/// frame: both link kinds are always on there). Like upstream, the link
/// rectangles are `link_shift` (the top margin) above the drawn links:
/// `addLink` maps them as if the content area started at the paper's top
/// edge (verified: hf-html-static's header link lies above the page).
fn push_hf_band<'p>(
    op: &mut OutPage<'p>,
    r: &'p RenderedPage,
    slice: &'p PageSlice,
    origin: (f32, f32),
    url_to_obj: &HashMap<String, usize>,
    resolve_relative: bool,
    link_shift: f32,
) {
    let s = r.pt_per_px;
    for l in &r.links {
        if l.href.is_empty() {
            continue;
        }
        if (l.rect.y0 as f32) < slice.y0 || (l.rect.y0 as f32) >= slice.y1 {
            continue;
        }
        let x0 = origin.0 + l.rect.x0 as f32 * s;
        let y0 = origin.1 + (l.rect.y0 as f32 - slice.y0) * s - link_shift;
        let rect = (x0, y0, x0 + l.rect.width() as f32 * s, y0 + l.rect.height() as f32 * s);
        if l.href.starts_with("__WKANCHOR_") {
            op.links.push(PdfLink { rect, target: LinkTarget::Named(l.href.clone()) });
            continue;
        }
        let Some(resolved) = &l.resolved else { continue };
        if url_to_obj.contains_key(&url_key(resolved)) {
            op.links.push(PdfLink { rect, target: LinkTarget::Named(resolved.to_string()) });
        } else {
            let uri = if resolve_relative { resolved.to_string() } else { l.href.clone() };
            op.links.push(PdfLink { rect, target: LinkTarget::Uri(uri) });
        }
    }
    op.bands.push(Band { page: r, slice, origin });
}

/// `fillParms`: the variables of page number `page` of object `o`.
fn fill_parms(
    outline: &mut Outline,
    page: usize,
    o: &PageObject<'_>,
    title: &str,
    g: &PdfGlobal,
    (time, date, isodate): (&str, &str, &str),
) -> Parms {
    let ps = o.settings;
    let off = g.page_offset as i64;
    let mut p = Parms::default();
    for (k, v) in &ps.replacements {
        p.set(k, v.clone());
    }
    let [section, subsection, subsubsection] = outline.sections(page);
    p.set("frompage", (off + 1).to_string());
    p.set("topage", (off + outline.page_count as i64).to_string());
    p.set("page", (page as i64 + off).to_string());
    p.set("webpage", ps.page.clone());
    p.set("section", section);
    p.set("subsection", subsection);
    p.set("subsubsection", subsubsection);
    p.set("doctitle", title);
    p.set("title", o.title.clone());
    p.set("time", time);
    p.set("date", date);
    p.set("isodate", isodate);
    p
}

/// Headings of a laid-out document with their page (1-based) and position
/// on it, in (page, y, x) order (`QMap` key of `replaceWebPage`).
fn located_headings(r: &RenderedPage, slices: &[PageSlice], paper: &Paper) -> Vec<LocatedHeading> {
    let s = r.pt_per_px;
    let mut v: Vec<LocatedHeading> = r
        .headings
        .iter()
        .filter_map(|h| {
            let (pi, off) = locate(slices, h.y)?;
            Some(LocatedHeading {
                level: h.level,
                text: h.text.clone(),
                page: pi + 1,
                x: paper.left + h.x * s,
                y: paper.top + off * s,
                w: h.w * s,
                h: h.h * s,
            })
        })
        .collect();
    v.sort_by(|a, b| a.page.cmp(&b.page).then(a.y.total_cmp(&b.y)).then(a.x.total_cmp(&b.x)));
    v
}

fn back_links(back: Vec<BackLink>) -> Vec<ObjLink> {
    back.into_iter()
        .map(|b| ObjLink {
            page: b.page.saturating_sub(1),
            rect: b.rect,
            target: LinkTarget::Named(b.target),
        })
        .collect()
}

/// `findLinks`: `__WKANCHOR_` anchors and links, internal links to loaded
/// objects (named after their full URL, the target registered on the
/// target object), external links.
#[allow(clippy::too_many_arguments)]
fn find_links(
    r: &RenderedPage,
    slices: &[PageSlice],
    paper: &Paper,
    (ulocal, uexternal): (bool, bool),
    resolve_relative: bool,
    url_to_obj: &HashMap<String, usize>,
    objs: &[PageObject<'_>],
    local: &mut Vec<ObjLink>,
    external: &mut Vec<ObjLink>,
    anchors: &mut Vec<(String, usize, f32, f32)>,
    foreign: &mut Vec<(usize, String, usize, f32, f32)>,
) {
    let s = r.pt_per_px;
    let mut names: Vec<(&String, &(f32, f32))> =
        r.anchors.a_name.iter().filter(|(n, _)| n.starts_with("__WKANCHOR_")).collect();
    names.sort_by(|a, b| a.0.cmp(b.0));
    for (n, (x, y)) in names {
        if let Some((pi, off)) = locate(slices, *y) {
            anchors.push((n.clone(), pi, paper.left + x * s, paper.top + off * s));
        }
    }
    for l in &r.links {
        if l.href.is_empty() {
            continue;
        }
        let Some((pi, off)) = locate(slices, l.rect.y0 as f32) else { continue };
        let x0 = paper.left + l.rect.x0 as f32 * s;
        let y0 = paper.top + off * s;
        let rect = (x0, y0, x0 + l.rect.width() as f32 * s, y0 + l.rect.height() as f32 * s);
        if l.href.starts_with("__WKANCHOR_") {
            local.push(ObjLink { page: pi, rect, target: LinkTarget::Named(l.href.clone()) });
            continue;
        }
        let Some(resolved) = &l.resolved else { continue };
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
            let name = resolved.to_string();
            foreign.push((
                target,
                name.clone(),
                tp,
                paper.left + ax * tr.pt_per_px,
                paper.top + toff * tr.pt_per_px,
            ));
            local.push(ObjLink { page: pi, rect, target: LinkTarget::Named(name) });
        } else if uexternal {
            let uri = if resolve_relative { resolved.to_string() } else { l.href.clone() };
            external.push(ObjLink { page: pi, rect, target: LinkTarget::Uri(uri) });
        }
    }
}

/// `--enable-forms` fields of page `pi`: text areas and `text`/`password`
/// inputs become text fields, checkboxes become checkboxes; untyped inputs,
/// radios, selects and buttons are not converted.
fn form_fields(r: &RenderedPage, slices: &[PageSlice], pi: usize, paper: &Paper) -> Vec<FormField> {
    let s = r.pt_per_px;
    let mut out = Vec::new();
    for e in &r.form_elements {
        let Some((p, off)) = locate(slices, e.rect.y0 as f32) else { continue };
        if p != pi {
            continue;
        }
        let x0 = paper.left + e.rect.x0 as f32 * s;
        let y0 = paper.top + off * s;
        let rect = (x0, y0, x0 + e.rect.width() as f32 * s, y0 + e.rect.height() as f32 * s);
        let t = e.input_type.as_str();
        if e.is_textarea || t == "text" || t == "password" {
            out.push(FormField::Text {
                rect,
                value: e.value.clone(),
                name: e.name.clone(),
                multiline: e.is_textarea,
                password: t == "password",
                readonly: e.readonly,
                maxlen: e.maxlength.unwrap_or(-1),
            });
        } else if t == "checkbox" {
            out.push(FormField::CheckBox {
                rect,
                checked: e.checked,
                name: e.name.clone(),
                readonly: e.readonly,
            });
        }
    }
    out
}

/// `QString::simplified`.
fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Page (index within the object) and offset (px from the top of the
/// content area) of document position `y`.
fn locate(slices: &[PageSlice], y: f32) -> Option<(usize, f32)> {
    // Positions in the truncated top margin belong to the first page.
    if slices.first().is_some_and(|s| y < s.y0) {
        return Some((0, 0.0));
    }
    slices.iter().enumerate().find_map(|(i, s)| s.offset_of(y).map(|o| (i, o))).or_else(|| {
        // Positions past the last band belong to the last page.
        let last = slices.len().checked_sub(1)?;
        let s = &slices[last];
        (y >= s.y1).then(|| (last, s.thead_height() + (s.y1 - s.y0)))
    })
}

/// Whether progress output is enabled for `level` (used by tests).
pub fn shows_progress(level: LogLevel) -> bool {
    level >= LogLevel::Info
}
