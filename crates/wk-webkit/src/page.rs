//! A loaded web page. Stands in for `QWebPage`/`QWebFrame` as driven by
//! wkhtmltopdf's `ResourceObject` (load) and `QWebPrinter` (print layout):
//! html5ever + stylo + taffy/parley through blitz-dom, painted by
//! blitz-paint.
//!
//! WebKit-2012 profile (QtWebKit 2.2 as patched for wkhtmltopdf 0.12.6):
//! - printing lays the page out `1.25×` wider than the printable width
//!   (WebKit's minimum printing shrink factor), so 1 CSS px = 0.6 pt; with
//!   smart shrinking (default) a wider document is laid out up to `2×` the
//!   printable width and scaled down; `--disable-smart-shrinking` uses `1×`;
//! - `--zoom` scales CSS px; `--dpi` does not change the layout (WebKit
//!   compensates it with a zoom of `dpi/96`);
//! - `line-height: normal` rounds the font ascent and descent up
//!   (FreeType), see `vendor/parley`;
//! - `@media screen` unless `--print-media-type`;
//! - page backgrounds are transparent unless the CSS sets one.
//!
//! - author CSS goes through the WebKit-2012 profile (`profile.rs`,
//!   `docs/webkit2012-profile.md`): what QtWebKit 2.2 ignored is dropped,
//!   `-webkit-` properties it honoured are mapped to standard ones; media
//!   queries see `--viewport-size` (0×0 by default); `--minimum-font-size`
//!   raises computed font sizes.
//!
//! JavaScript (P4) runs in `scripting.rs` when the document has scripts;
//! documents without any take the static path below (no realm is created).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use blitz_dom::{BaseDocument, DocumentConfig, MediaType, Node, NodeId, local_name};
use blitz_html::HtmlDocument;
use blitz_traits::shell::{ColorScheme, Viewport};
use parley::FontContext;
use style::values::computed::{BreakBetween, BreakWithin};
use url::Url;

use crate::net::{Fetcher, QueueNet, Queued, deliver};
use crate::paint::{Recorder, glyph_run_bbox};

/// Engine settings of one page object (`settings::Web` + the print-related
/// global settings).
#[derive(Debug, Clone)]
pub struct WebSettings {
    pub print_media_type: bool,
    pub background: bool,
    pub load_images: bool,
    pub enable_javascript: bool,
    pub minimum_font_size: i32,
    pub default_encoding: String,
    /// Contents of `--user-style-sheet`, already loaded.
    pub user_style_sheet: Option<String>,
    /// `--zoom`.
    pub zoom: f32,
    /// `--dpi` (the printer resolution): QtWebKit zooms the frame by `dpi / 96` on top of
    /// `--zoom` and lays out in those device pixels; the page scale does not change, the
    /// rounding of lengths, font sizes and borders does.
    pub dpi: i32,
    /// Smart shrinking (`--enable/disable-smart-shrinking`).
    pub smart_shrinking: bool,
    /// `--viewport-size`: what media queries see (0×0 when unset).
    pub viewport: Option<(u32, u32)>,
    /// `--javascript-delay` (ms, virtual clock).
    pub javascript_delay: u32,
    /// `--window-status` (main pages only).
    pub window_status: String,
    /// `--run-script` (main pages only).
    pub run_script: Vec<String>,
    /// `--debug-javascript`.
    pub debug_javascript: bool,
    /// `--stop-slow-scripts` / `--no-stop-slow-scripts`.
    pub stop_slow_scripts: bool,
    /// `navigator.userAgent` (the loader's `User-Agent`).
    pub user_agent: String,
    /// `--enable-forms`: inputs and text areas are painted white (their
    /// values become form fields), like `beginPrintObject` does.
    pub produce_forms: bool,
    /// Printable area (points) the page will be rendered on, when known at
    /// load time: the load-time layout (resource discovery) is then done at
    /// the print viewport, which `render` reuses instead of laying the
    /// document out a second time.
    pub print_area: Option<(f32, f32)>,
}

impl Default for WebSettings {
    fn default() -> Self {
        WebSettings {
            print_media_type: false,
            background: true,
            load_images: true,
            enable_javascript: true,
            minimum_font_size: -1,
            default_encoding: String::new(),
            user_style_sheet: None,
            zoom: 1.0,
            dpi: 96,
            smart_shrinking: true,
            viewport: None,
            javascript_delay: 200,
            window_status: String::new(),
            run_script: Vec::new(),
            debug_javascript: false,
            stop_slow_scripts: true,
            user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/534.34 (KHTML, like Gecko) wkhtmltopdf Safari/534.34"
                .into(),
            produce_forms: false,
            print_area: None,
        }
    }
}

/// QtWebKit's printing shrink factors (`setPrintingMinimumShrinkFactor`,
/// `setPrintingMaximumShrinkFactor`).
pub const MIN_SHRINK: f32 = 1.25;
pub const MAX_SHRINK: f32 = 2.0;

/// A table and its repeated groups, in document px.
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub top: f32,
    pub bottom: f32,
    pub thead: Option<(f32, f32)>,
    pub tfoot: Option<(f32, f32)>,
}

/// The background and side borders of a box, in document px: what QtWebKit shows of a box
/// below content its paginated layout pushed to the next page (the box grows by the pagination
/// strut, so it runs down to the page bottom).
#[derive(Debug, Clone)]
pub struct BoxFill {
    pub top: f32,
    pub bottom: f32,
    /// Border box left and right edges.
    pub x0: f32,
    pub x1: f32,
    pub background: Option<peniko::Color>,
    /// Solid left and right borders: width and colour.
    pub left: Option<(f32, peniko::Color)>,
    pub right: Option<(f32, peniko::Color)>,
}

/// Where the document may, must and must not be cut (document px).
#[derive(Debug, Default, Clone)]
pub struct Geometry {
    pub doc_height: f32,
    /// Box edges where a break is allowed.
    pub candidates: Vec<f32>,
    /// Intervals a break must not cut (line boxes, rows, replaced elements,
    /// `break-inside: avoid`).
    pub atomic: Vec<(f32, f32)>,
    /// Forced breaks (`break-before/after: page`).
    pub forced: Vec<f32>,
    pub tables: Vec<TableInfo>,
    /// Where boxes start (element boxes other than the root, line boxes):
    /// a page starts at the first of them, so the margins at the top of a
    /// page are truncated like WebKit's paginated layout does.
    pub content_starts: Vec<f32>,
    /// Line boxes of each inline formatting context, top to bottom (for
    /// `orphans`/`widows`, which the 0.12.6 binary does not apply by
    /// default).
    pub paragraphs: Vec<Vec<(f32, f32)>>,
    /// Painted content: line boxes and the unbreakable boxes (rows, replaced
    /// elements). A page that starts in a gap between them starts at the next one.
    pub content_boxes: Vec<(f32, f32)>,
    /// What moves an unforced break up when it straddles the page bottom: the
    /// content area of each line, the unbreakable boxes.
    pub straddlers: Vec<(f32, f32)>,
    /// In-flow boxes with a background or solid side borders, outermost first.
    pub fills: Vec<BoxFill>,
}

/// An `<a href>` hit area, in document px.
#[derive(Debug, Clone)]
pub struct LinkBox {
    pub rect: kurbo::Rect,
    /// The `href` attribute as written.
    pub href: String,
    /// `href` resolved against the document URL.
    pub resolved: Option<Url>,
}

/// An `h1`…`h6` element (outline source).
#[derive(Debug, Clone)]
pub struct Heading {
    pub level: u32,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Named anchors for internal links, in document px.
#[derive(Debug, Default, Clone)]
pub struct Anchors {
    pub a_name: HashMap<String, (f32, f32)>,
    pub id: HashMap<String, (f32, f32)>,
    pub name: HashMap<String, (f32, f32)>,
}

impl Anchors {
    /// `a[name="f"]`, then `*[id="f"]`, then `*[name="f"]` (spec §3.6).
    pub fn find(&self, f: &str) -> Option<(f32, f32)> {
        self.a_name.get(f).or_else(|| self.id.get(f)).or_else(|| self.name.get(f)).copied()
    }
}

/// The printable output of a page: display list plus everything the
/// paginator and the PDF writer need.
pub struct RenderedPage {
    pub recorder: Recorder,
    pub geometry: Geometry,
    pub links: Vec<LinkBox>,
    pub headings: Vec<Heading>,
    pub anchors: Anchors,
    /// Layout width in CSS px.
    pub layout_width: f32,
    /// PDF points per CSS px on the page.
    pub pt_per_px: f32,
    /// Height of a page in CSS px (the document is cut every `page_height`).
    pub page_height: f32,
    /// Original encoded bytes of raster images, by image blob id (JPEG
    /// passthrough).
    pub image_sources: HashMap<u64, Arc<Vec<u8>>>,
    /// Height of the `<body>` box in CSS px (header/footer measuring).
    pub body_height: f32,
    /// `input` elements, then `textarea` elements, in document order.
    pub form_elements: Vec<FormElement>,
}

/// An `input` or `textarea` element (source of `--enable-forms` fields).
#[derive(Debug, Clone)]
pub struct FormElement {
    /// Border box in document px.
    pub rect: kurbo::Rect,
    pub is_textarea: bool,
    /// The `type` attribute as written (empty when absent).
    pub input_type: String,
    pub name: String,
    pub value: String,
    pub checked: bool,
    pub readonly: bool,
    pub maxlength: Option<i32>,
}

/// A loaded page.
pub struct WebPage {
    doc: BaseDocument,
    net: QueueNet,
    settings: WebSettings,
    /// Bodies of loaded resources by URL (for JPEG passthrough).
    bodies: HashMap<String, Arc<Vec<u8>>>,
    /// Values of password inputs, which are displayed as bullets.
    password_values: HashMap<NodeId, String>,
    pub url: Url,
    /// Canvas content recorded by script.
    canvases: HashMap<NodeId, Rc<blitz_paint::canvas::CanvasPicture>>,
    /// Boxes positioned against the initial containing block and moved by
    /// [`place_viewport_boxes`](Self::place_viewport_boxes): node, shift, resulting `y`.
    viewport_shifts: Vec<(NodeId, f32, f32)>,
}

/// A document moved to a pool thread only to be dropped there ([`WebPage::render_final`]).
struct DroppedDocument(#[allow(dead_code)] BaseDocument);

// SAFETY: the only fields that make `BaseDocument` not `Send` are two trait objects whose
// traits do not require `Send`: its `Arc<dyn HtmlParserProvider>` and, inside Stylo's
// device, its `Box<dyn FontMetricsProvider>`. The implementations used here are
// `blitz_html::HtmlProvider` (a unit struct) and blitz-dom's `BlitzFontMetricsProvider` (an
// `Arc<Mutex<FontContext>>`), both `Send`. Once wrapped, the document is not reachable from
// anywhere else and is only dropped, by one thread.
unsafe impl Send for DroppedDocument {}

/// The box standing in for an image that failed to load (`WebPage::broken_images`).
struct AltBox {
    span: NodeId,
    /// The alt text node.
    text: NodeId,
    /// The width (height) is given: the box does not take the width (height) of its text.
    sized_w: bool,
    sized_h: bool,
}

fn tag(node: &Node) -> &str {
    node.element_data().map(|e| &*e.name.local).unwrap_or("")
}

/// Form controls as QtWebKit 2.2 draws them with Qt's default style
/// (measured on the 0.12.6 binary, tests/parts/forms and tests/fidelity/forms-*): text
/// fields and textareas inherit the font, have a 1 px black frame on white and a 1 px
/// side inset (the frame a 0.7px line on the border box edge, drawn by blitz-paint in place
/// of the `rgba(1, 2, 4, 0)` border), their text #545454 when disabled and a placeholder in #a9a9a9; the input
/// types QtWebKit 2.2 had no widget for (color, date and time) are text fields;
/// checkboxes are 13 px and radios 12 px, both with 3 px margins; push buttons use the default sans-serif face on
/// Qt's button grey, inside a 2px raised bevel (drawn by blitz-paint in place of the
/// `rgba(1, 2, 3, 0)` border), with #808080 text when disabled. Text field and textarea
/// widths come from `size`/`cols` (blitz-dom layout).
const QT_FORM_CONTROLS: &str = "\
input:not([type]), input[type=text], input[type=password], input[type=search], input[type=email], \
input[type=url], input[type=tel], input[type=number], input[type=color], input[type=date], \
input[type=datetime], input[type=datetime-local], input[type=month], input[type=week], \
input[type=time], textarea { font: inherit; color: black; \
border: 1px solid rgba(1, 2, 4, 0); padding: 0 1px; margin: 2px; background-color: white; line-height: normal; }
textarea { padding: 0; }
input[placeholder]:not([placeholder=\"\"]):not([value]), input[placeholder]:not([placeholder=\"\"])[value=\"\"] { color: #a9a9a9; }
input[type=number] { margin-right: 5px; }
input:disabled:not([type=checkbox]):not([type=radio]):not([type=submit]):not([type=reset]):not([type=button]), \
textarea:disabled { color: #545454; }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; border: none; padding: 0; \
margin: 3px; color: black; }
input[type=radio] { width: 12px; height: 12px; margin: 3px; }
input[type=submit], input[type=reset], input[type=button], button { font: inherit; \
font-family: sans-serif; color: black; background-color: #d4d0c8; border: 2px solid rgba(1, 2, 3, 0); \
border-radius: 0; padding: 4px; margin: 2px; }
input:disabled[type=submit], input:disabled[type=reset], input:disabled[type=button], button:disabled { \
color: #808080; }
select { font: inherit; color: black; background-color: white; border: 1px solid black; margin: 2px; }
";

pub(crate) fn ua_css(settings: &WebSettings) -> String {
    let mut css = String::from(QT_FORM_CONTROLS);
    // WebKit's html.css: `<bdo>` overrides the bidi order (blitz's rule for it has a Gecko
    // selector the style system drops).
    css.push_str("bdo, bdo[dir] { unicode-bidi: bidi-override; }\n");
    if settings.enable_javascript {
        css.push_str("noscript { display: none !important; }\n");
    }
    if !settings.background {
        css.push_str("* { background-color: transparent !important; background-image: none !important; }\n");
    }
    if !settings.load_images {
        // `--no-images`: an image without a size is an empty replaced box, but still content
        // that gives its line the strut's height, which parley gives only to a line with
        // a box of some height (an inline-block holding only such an image is a line tall).
        css.push_str("img { min-height: 1px; }\n");
    }
    if settings.produce_forms {
        // `elm.setStyleProperty("color", "white")` on every input and
        // textarea: text fields keep the inner-text colour computed at
        // load, so only the theme-painted marks (check, radio dot) vanish.
        // The controls are not themed either: plain outlined boxes and discs, drawn by
        // blitz-paint for the frame sentinel border colour (no border width).
        css.push_str(
            "input[type=checkbox], input[type=radio] { color: white !important; \
             border-color: rgba(1, 2, 4, 0) !important; }\n",
        );
    }
    css
}

/// The viewport a page is first laid out at for printing on `printable_w_pt` ×
/// `printable_h_pt` points: the printable area in 96 dpi pixels widened by the
/// minimum shrink factor and truncated (`FrameView::forceLayoutForPagination`
/// sets `static_cast<int>(pageLogicalWidth)`: 649 px of A4 lay out 811 px wide,
/// 233 px 291), in CSS px (÷ `--zoom`).
fn print_viewport(settings: &WebSettings, printable_w_pt: f32, printable_h_pt: f32) -> (f32, f32) {
    let zoom = if settings.zoom > 0.0 { settings.zoom } else { 1.0 };
    let min_shrink = if settings.smart_shrinking { MIN_SHRINK } else { 1.0 };
    let w = (printable_w_pt / 0.75 * min_shrink + 1e-3).floor().max(1.0);
    let h = (printable_h_pt / 0.75 * min_shrink + 1e-3).floor().max(1.0);
    (w / zoom, h / zoom)
}

/// Device pixels per CSS px: `--zoom` × `--dpi` / 96 (QtWebKit's frame zoom factor).
fn device_zoom(settings: &WebSettings) -> f32 {
    let zoom = if settings.zoom > 0.0 { settings.zoom } else { 1.0 };
    let dpi = if settings.dpi > 0 { settings.dpi } else { 96 };
    zoom * dpi as f32 / 96.0
}

/// Lay out the next pages in device pixels ([`device_zoom`]): lengths, font sizes and borders
/// (blitz-dom, stylo-taffy) and text measurement (parley).
fn set_device_zoom(settings: &WebSettings) {
    let zoom = device_zoom(settings);
    blitz_dom::set_device_zoom(zoom);
    parley::device_px::set_device_zoom(zoom);
}

/// One resource request found in document order.
struct Unit {
    url: Url,
    /// Requested only to account for its outcome (scripts: not run yet).
    script: bool,
    image: bool,
}

impl WebPage {
    /// Parse `html`, load its sub-resources through `fetcher`, and run its
    /// scripts when JavaScript is enabled (the whole `QWebFrame::load` up to
    /// wkhtmltopdf's `loadDone`, including the `--javascript-delay` wait).
    ///
    /// This is also the entry point for header/footer HTML: load the page
    /// with its query string in `url` (`?page=1&topage=3…`, read by the
    /// substitution script through `location.search`), with
    /// `run_script`/`window_status` left empty, then [`render`](Self::render).
    ///
    /// `font_ctx` `None` is the process's default context (`wk_fonts::default_font_context`),
    /// taken once the page is parsed: a worker may still be scanning the fonts meanwhile.
    pub fn load(
        html: &str,
        url: Url,
        settings: WebSettings,
        font_ctx: Option<FontContext>,
        fetcher: &mut dyn Fetcher,
    ) -> WebPage {
        crate::profile::install();
        set_device_zoom(&settings);
        style::properties::cascade::set_minimum_font_size(settings.minimum_font_size as f32);
        let (vw, vh) = settings.viewport.unwrap_or((0, 0));
        crate::profile::set_media_viewport(vw as f32, vh as f32);
        if settings.enable_javascript && crate::scripting::needs_js(html) {
            let font_ctx = font_ctx.unwrap_or_else(wk_fonts::default_font_context);
            return crate::scripting::load(html, url, settings, font_ctx, fetcher);
        }
        let net = QueueNet::default();
        let media = if settings.print_media_type { MediaType::print() } else { MediaType::screen() };
        let (load_w, load_h) = match settings.print_area {
            Some((w, h)) => {
                let (w, h) = print_viewport(&settings, w, h);
                (w as u32, h as u32)
            }
            None => (800, 600),
        };
        let config = DocumentConfig {
            viewport: Some(Viewport::new(load_w, load_h, 1.0, ColorScheme::Light)),
            base_url: Some(url.to_string()),
            net_provider: Some(Arc::new(net.clone())),
            // An empty context until the page is parsed (see `set_font_context` below).
            font_ctx: Some(FontContext {
                collection: parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                    shared: false,
                    system_fonts: false,
                }),
                source_cache: Default::default(),
            }),
            media_type: Some(media),
            ..Default::default()
        };
        // QtWebKit parses every `text/html` document with its HTML parser,
        // whatever its DOCTYPE or root namespace; blitz-html would switch to
        // its XML parser on an XHTML DOCTYPE, an XML declaration or an
        // `xmlns` on `<html>` (then `<ol/>` would be an empty list, as in no
        // browser). A leading empty comment defeats the sniffing.
        let sniff_proof = format!("<!---->{html}");
        crate::timing("parse start");
        let mut doc = HtmlDocument::from_html(&sniff_proof, config).into_inner();
        doc.set_font_context(font_ctx.unwrap_or_else(wk_fonts::default_font_context));
        crate::timing("parsed");
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
        let mut page = WebPage {
            doc,
            net,
            settings,
            bodies: HashMap::new(),
            password_values: HashMap::new(),
            url,
            canvases: HashMap::new(),
            viewport_shifts: Vec::new(),
        };
        page.mask_passwords();
        page.load_resources(fetcher);
        crate::timing("resources loaded");
        page
    }

    /// A page loaded with scripts (`scripting.rs`): password values are
    /// masked on the final DOM, after the scripts ran.
    pub(crate) fn from_parts(
        doc: BaseDocument,
        net: QueueNet,
        settings: WebSettings,
        bodies: HashMap<String, Arc<Vec<u8>>>,
        url: Url,
        canvases: HashMap<NodeId, Rc<blitz_paint::canvas::CanvasPicture>>,
    ) -> WebPage {
        let mut page = WebPage {
            doc,
            net,
            settings,
            bodies,
            password_values: HashMap::new(),
            url,
            canvases,
            viewport_shifts: Vec::new(),
        };
        page.mask_passwords();
        page
    }

    /// QtWebKit displays the value of `input[type=password]` as bullets.
    fn mask_passwords(&mut self) {
        let mut found = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            if tag(n) == "input"
                && n.attr(local_name!("type")).is_some_and(|t| t.eq_ignore_ascii_case("password"))
            {
                if let Some(v) = n.attr(local_name!("value")).filter(|v| !v.is_empty()) {
                    found.push((id, v.to_string()));
                }
            }
        }
        let mut m = self.doc.mutate();
        for (id, v) in &found {
            let bullets = "\u{2022}".repeat(v.chars().count());
            m.set_attribute(
                *id,
                blitz_dom::QualName::new(None, blitz_dom::ns!(), local_name!("value")),
                &bullets,
            );
        }
        drop(m);
        self.password_values.extend(found);
    }

    /// The document `<title>`.
    pub fn title(&self) -> Option<String> {
        self.doc.find_title_node().map(|n| n.text_content())
    }

    /// Resources in document order, split in batches at each `<script>`
    /// (a parser-blocking script ends what WebCore requests together).
    fn document_units(&self) -> Vec<Vec<Unit>> {
        let mut batches = vec![Vec::new()];
        let base = self.doc.base_url().clone();
        let mut stack = vec![self.doc.root_node().id];
        let mut order = Vec::new();
        while let Some(id) = stack.pop() {
            let Some(node) = self.doc.get_node(id) else { continue };
            order.push(id);
            for &c in node.children.iter().rev() {
                stack.push(c);
            }
        }
        for id in order {
            let node = self.doc.get_node(id).expect("node");
            match tag(node) {
                "link" => {
                    let rel = node.attr(local_name!("rel")).unwrap_or("").to_ascii_lowercase();
                    if rel.split_ascii_whitespace().any(|r| r == "stylesheet") {
                        if let Some(u) = node.attr(local_name!("href")).and_then(|h| base.join(h).ok()) {
                            batches.last_mut().expect("batch").push(Unit {
                                url: u,
                                script: false,
                                image: false,
                            });
                        }
                    }
                }
                "img" => {
                    if let Some(u) = node
                        .attr(local_name!("src"))
                        .filter(|s| !s.is_empty())
                        .and_then(|h| base.join(h).ok())
                    {
                        batches.last_mut().expect("batch").push(Unit { url: u, script: false, image: true });
                    }
                }
                "script" if self.settings.enable_javascript => {
                    if let Some(u) = node
                        .attr(local_name!("src"))
                        .filter(|s| !s.is_empty())
                        .and_then(|h| base.join(h).ok())
                    {
                        batches.last_mut().expect("batch").push(Unit { url: u, script: true, image: false });
                    }
                    batches.push(Vec::new());
                }
                _ => {}
            }
        }
        batches
    }

    fn serve(&mut self, items: Vec<(Option<Queued>, Url, bool)>, fetcher: &mut dyn Fetcher) {
        // createRequest for the whole batch, then the replies.
        let mut handles = Vec::new();
        for (q, url, skip) in items {
            if skip {
                if let Some(q) = q {
                    deliver(q, None);
                }
                continue;
            }
            let id = fetcher.create(&url);
            handles.push((q, url, id));
        }
        for (q, url, id) in handles {
            let body = fetcher.finish(id);
            if let Some(b) = &body {
                self.bodies.insert(url.to_string(), Arc::new(b.clone()));
            }
            if let Some(q) = q {
                deliver(q, body);
            }
        }
    }

    fn load_resources(&mut self, fetcher: &mut dyn Fetcher) {
        let mut queued: Vec<Option<Queued>> = self.net.take().into_iter().map(Some).collect();
        for batch in self.document_units() {
            let mut items = Vec::new();
            for unit in batch {
                let q = queued
                    .iter_mut()
                    .find(|q| q.as_ref().is_some_and(|q| q.url == unit.url))
                    .and_then(Option::take);
                if q.is_none() && !unit.script {
                    // Already requested (same URL earlier in the document).
                    continue;
                }
                let skip = unit.image && !self.settings.load_images;
                items.push((q, unit.url, skip));
            }
            self.serve(items, fetcher);
        }
        // Whatever blitz asked for that is not in the document order
        // (imports, fonts, background images), until the queue is empty.
        let mut rest: Vec<Queued> = queued.into_iter().flatten().collect();
        for _ in 0..32 {
            if rest.is_empty() {
                self.doc.resolve(0.0);
                rest = self.net.take();
                if rest.is_empty() {
                    break;
                }
            }
            let items = rest.drain(..).map(|q| {
                let url = q.url.clone();
                let skip = !self.settings.load_images && is_image_url(&url);
                (Some(q), url, skip)
            });
            let items: Vec<_> = items.collect();
            self.serve(items, fetcher);
        }
    }

    /// Lay out for printing on a printable area of `printable_w_pt` ×
    /// `printable_h_pt` points, paint, and collect pagination data.
    pub fn render(&mut self, printable_w_pt: f32, printable_h_pt: f32) -> RenderedPage {
        self.render_inner(printable_w_pt, printable_h_pt, false)
    }

    /// [`render`](Self::render) for a page printed once and then dropped: the text layouts
    /// are handed over to the painter, which frees each one as soon as its text is painted,
    /// so they do not add up with the display list being recorded (the memory peak of a
    /// conversion).
    ///
    /// The page (DOM, styles, layout) is then dropped on the rayon pool, off the critical
    /// path of the conversion.
    pub fn render_final(mut self, printable_w_pt: f32, printable_h_pt: f32) -> RenderedPage {
        let rendered = self.render_inner(printable_w_pt, printable_h_pt, true);
        let WebPage { doc, net, settings, bodies, password_values, url, canvases, .. } = self;
        // The canvas pictures are reference counted without atomics: released here.
        drop(canvases);
        let rest = (net, settings, bodies, password_values, url);
        let doc = DroppedDocument(doc);
        rayon::spawn(move || {
            // A panic in a destructor would abort the process from a pool thread: ignore it,
            // the page being of no further use.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop((doc, rest))));
        });
        rendered
    }

    fn render_inner(&mut self, printable_w_pt: f32, printable_h_pt: f32, release_text: bool) -> RenderedPage {
        let (mut layout_w, vh) = print_viewport(&self.settings, printable_w_pt, printable_h_pt);
        let zoom = if self.settings.zoom > 0.0 { self.settings.zoom } else { 1.0 };
        let max_shrink = if self.settings.smart_shrinking { MAX_SHRINK } else { 1.0 };
        // The page rect in 96 dpi pixels (whole, see `Paper` in wk-converter).
        let print_w = printable_w_pt / 0.75;
        let print_h = printable_h_pt / 0.75;
        // The page height the document is paginated at, in layout pixels (`pageLogicalHeight`,
        // truncated): the printable height widened like the width, so 979 px of A4 are 1223 px.
        let mut page_h = vh * zoom;
        crate::timing("render start");
        self.undo_viewport_shifts();
        set_device_zoom(&self.settings);
        self.qt_selects();
        self.qt_widgets();
        self.qt_sized_field_margins();
        self.inline_edges();
        let alt_boxes = self.broken_images();
        self.set_viewport(layout_w, vh);
        crate::timing("viewport set");
        if self.settings.smart_shrinking {
            let content_w = self.content_width();
            if content_w > layout_w + 0.5 {
                // A wider document lays out at its width, up to the maximum shrink, keeping the
                // page's ratio (`resizePageRectsKeepingRatio`), both truncated.
                let w = ((content_w * zoom - 1e-3).ceil()).min((print_w * max_shrink + 1e-3).floor());
                page_h = (w * print_h / print_w + 1e-3).floor();
                layout_w = w / zoom;
                self.set_viewport(layout_w, vh);
            }
        }
        let alt_settled = self.settle_alt_boxes(&alt_boxes);
        if self.settle_legends() || alt_settled {
            self.set_viewport(layout_w, vh);
        }
        let pt_per_px = printable_w_pt / layout_w;

        crate::timing("pre-geometry");
        let mut geometry = collect_geometry(&self.doc);
        if self.place_viewport_boxes(&geometry, vh) {
            geometry = collect_geometry(&self.doc);
        }
        crate::timing("geometry");
        let doc_h = geometry.doc_height.ceil().max(1.0) as u32;
        let mut recorder = Recorder::default();
        blitz_paint::canvas::set_pictures(self.canvases.clone());
        // Qt anchors pattern brushes at the first page's top left corner: the first content.
        let first_page_top = geometry
            .content_starts
            .iter()
            .copied()
            .find(|&c| c >= -0.01 && c < geometry.doc_height)
            .unwrap_or(0.0)
            .max(0.0);
        blitz_paint::pattern_origin::set(0.0, f64::from(first_page_top));
        if release_text {
            blitz_paint::inline_layouts::set(self.doc.take_inline_layouts());
        }
        blitz_paint::paint_scene(&mut recorder, &mut self.doc, 1.0, layout_w as u32, doc_h, 0, 0);
        blitz_paint::inline_layouts::clear();
        blitz_paint::canvas::clear_pictures();
        crate::timing("painted");

        if std::env::var_os("WKHTMLTOPDF_DEBUG_LAYOUT").is_some() {
            self.debug_layout();
        }
        if std::env::var_os("WKHTMLTOPDF_DEBUG_SCENE").is_some() {
            for (i, c) in recorder.commands.iter().enumerate() {
                if let anyrender::recording::RenderCommand::GlyphRun(g) = &*c.get() {
                    let t = recorder.texts.get(&i);
                    eprintln!(
                        "glyphs #{i} n={} at {:?} skew={:?} text={:?} node={:?}",
                        g.glyphs.len(),
                        g.transform.translation(),
                        g.glyph_transform.map(|t| t.as_coeffs()[2]),
                        t.map(|t| t.text.as_str()),
                        t.map(|t| t.node_id)
                    );
                }
            }
        }
        let (links, headings, anchors) = self.collect_links(&recorder);
        crate::timing("links");
        let image_sources = self.image_sources();
        let body_height = self.body_height();
        let form_elements = self.form_elements();
        RenderedPage {
            recorder,
            geometry,
            links,
            headings,
            anchors,
            layout_width: layout_w,
            pt_per_px,
            page_height: page_h / zoom,
            image_sources,
            body_height,
            form_elements,
        }
    }

    /// Height of the first `<body>` element's box (`elementLocation(body)`).
    fn body_height(&self) -> f32 {
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            if tag(n) == "body" {
                return n.final_layout().size.height;
            }
            stack.extend(n.children.iter().rev().copied());
        }
        0.0
    }

    /// `findAllElements("input")` then `findAllElements("textarea")`.
    fn form_elements(&self) -> Vec<FormElement> {
        let mut order = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            order.push(id);
            stack.extend(n.children.iter().rev().copied());
        }
        let mut out = Vec::new();
        for want in ["input", "textarea"] {
            for &id in &order {
                let n = self.doc.get_node(id).expect("node");
                if tag(n) != want {
                    continue;
                }
                let pos = n.absolute_position(0.0, 0.0);
                let l = n.final_layout();
                let attr = |a| n.attr(a).map(str::to_string);
                out.push(FormElement {
                    rect: kurbo::Rect::new(
                        f64::from(pos.x),
                        f64::from(pos.y),
                        f64::from(pos.x + l.size.width),
                        f64::from(pos.y + l.size.height),
                    ),
                    is_textarea: want == "textarea",
                    input_type: attr(local_name!("type")).unwrap_or_default(),
                    name: attr(local_name!("name")).unwrap_or_default(),
                    // QWebElement::toPlainText() of a textarea is empty in
                    // QtWebKit 2.2: its content is dropped (verified).
                    value: if want == "textarea" {
                        String::new()
                    } else if let Some(v) = self.password_values.get(&id) {
                        v.clone()
                    } else {
                        attr(local_name!("value")).unwrap_or_default()
                    },
                    checked: n.attr(local_name!("checked")).is_some(),
                    readonly: n.attr(local_name!("readonly")).is_some(),
                    maxlength: n.attr(local_name!("maxlength")).map(|v| v.trim().parse().unwrap_or(0)),
                });
            }
        }
        out
    }

    /// `<select>` as QtWebKit 2.2 paints it with Qt's default (Windows) style, measured on the
    /// 0.12.6 binary: a combo box is a 2px sunken frame (#6a6864 and black above and left,
    /// white and #d4d0c8 below and right) around a white field holding the selected option
    /// (17px high, the widest option with 2px of padding on the left and 22px on the right,
    /// measured with tests/fidelity probes) and a 16px raised button
    /// with a 7px black arrow; a list box (`multiple` or `size` > 1) shows its options one per
    /// line, the selected ones in white on navy, beside a 16px scroll bar, at least 4 rows.
    /// blitz-dom has no select layout: each one is rebuilt here as nested spans (the hidden
    /// options of a combo box only give it its width).
    fn qt_selects(&mut self) {
        struct Select {
            id: NodeId,
            options: Vec<(String, bool)>,
            list_rows: Option<usize>,
            attrs: Vec<blitz_dom::Attribute>,
        }
        let mut found = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            if tag(n) != "select" {
                stack.extend(n.children.iter().copied());
                continue;
            }
            let mut options = Vec::new();
            let mut inner: Vec<NodeId> = n.children.iter().rev().copied().collect();
            while let Some(c) = inner.pop() {
                let Some(o) = self.doc.get_node(c) else { continue };
                match tag(o) {
                    "option" => {
                        let text = o.text_content().split_whitespace().collect::<Vec<_>>().join(" ");
                        options.push((text, o.attr(local_name!("selected")).is_some()));
                    }
                    "optgroup" => inner.extend(o.children.iter().rev().copied()),
                    _ => {}
                }
            }
            let multiple = n.attr(local_name!("multiple")).is_some();
            let size: usize = n.attr(local_name!("size")).and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            let list_rows = (multiple || size > 1).then(|| size.max(4));
            if list_rows.is_none() {
                // A combo box shows the last selected option, else the first one.
                let shown = options.iter().rposition(|o| o.1).unwrap_or(0);
                for (i, o) in options.iter_mut().enumerate() {
                    o.1 = i == shown;
                }
            }
            let attrs = n.element_data().map(|e| e.attrs.to_vec()).unwrap_or_default();
            found.push(Select { id, options, list_rows, attrs });
        }
        if found.is_empty() {
            return;
        }
        let span = blitz_dom::QualName::new(None, blitz_dom::ns!(html), local_name!("span"));
        let style_name = blitz_dom::QualName::new(None, blitz_dom::ns!(), local_name!("style"));
        let mut m = self.doc.mutate();
        let el = |m: &mut blitz_dom::DocumentMutator, style: String, children: &[NodeId]| {
            let attrs = vec![blitz_dom::Attribute { name: style_name.clone(), value: style }];
            let e = m.create_element(span.clone(), attrs);
            m.append_children(e, children);
            e
        };
        const SUNKEN: &str = "border:1px solid;border-color:#6a6864 white white #6a6864;";
        const SUNKEN_IN: &str = "border:1px solid;border-color:black #d4d0c8 #d4d0c8 black;";
        const RAISED: &str =
            "border:1px solid;border-color:#d4d0c8 black black #d4d0c8;background-color:#d4d0c8;";
        const RAISED_IN: &str = "border:1px solid;border-color:white #6a6864 #6a6864 white;";
        let button = |m: &mut blitz_dom::DocumentMutator, pos: &str, down: bool, enabled: bool| {
            // QWindowsStyle's arrow: rows 7, 5, 3 and 1px wide (or the reverse), 3px from the
            // left and 5px from the top of the button's padding box. A disabled scroll bar
            // (every option visible) draws it in Qt's disabled-text grey over a white copy
            // 1px down and right.
            let ink = if enabled { "black" } else { "#a0a0a4" };
            let mut children =
                vec![el(m, format!("position:absolute;left:0;top:0;right:0;bottom:0;{RAISED_IN}"), &[])];
            let layers: &[(&str, i32)] = if enabled { &[(ink, 0)] } else { &[("white", 1), (ink, 0)] };
            for &(color, shift) in layers {
                for row in 0..4 {
                    let inset = if down { row } else { 3 - row };
                    children.push(el(
                        m,
                        format!(
                            "position:absolute;left:{}px;top:{}px;width:{}px;height:1px;background-color:{color};",
                            3 + inset + shift,
                            5 + row + shift,
                            7 - 2 * inset
                        ),
                        &[],
                    ));
                }
            }
            el(m, format!("position:absolute;{pos}width:14px;{RAISED}"), &children)
        };
        for s in found {
            let mut outer_style = format!(
                "display:inline-block;box-sizing:border-box;vertical-align:baseline;white-space:pre;\
                 text-indent:0;text-align:left;line-height:normal;font:inherit;color:black;margin:2px;{SUNKEN}"
            );
            if s.list_rows.is_some() {
                // A list box has a plain 1px black frame. QtWebKit puts its baseline 7px above
                // its bottom margin edge (RenderListBox's baselineAdjustment): the last row's
                // baseline with 13px text. Here the box has no baseline (it sits on it).
                outer_style
                    .push_str("overflow:hidden;vertical-align:-7px;border:1px solid rgba(1, 2, 4, 0);");
            }
            let mut kept = Vec::new();
            for a in s.attrs {
                match &*a.name.local {
                    "style" => outer_style.push_str(&a.value),
                    "id" | "class" | "title" | "lang" | "dir" => kept.push(a),
                    _ => {}
                }
            }
            let field = if let Some(rows) = s.list_rows {
                // The first `rows` options, then empty rows: the list is `rows` lines high.
                let mut lines = Vec::new();
                for i in 0..rows {
                    let (text, selected) = s.options.get(i).map_or(("", false), |o| (o.0.as_str(), o.1));
                    let t = m.create_text_node(if text.is_empty() { "\u{a0}" } else { text });
                    let colors = if selected { "background-color:#000080;color:white;" } else { "" };
                    // Rows are a line plus 1px apart; the last one has no gap.
                    let gap = if i + 1 < rows { "padding-bottom:1px;" } else { "" };
                    lines.push(el(&mut m, format!("display:block;padding:0 2px;{gap}{colors}"), &[t]));
                }
                let list = el(&mut m, "display:block;".into(), &lines);
                let enabled = s.options.len() > rows;
                let up = button(&mut m, "top:0;right:0;height:14px;", false, enabled);
                let down = button(&mut m, "bottom:0;right:0;height:14px;", true, enabled);
                let track = el(
                    &mut m,
                    "position:absolute;top:0;bottom:0;right:0;width:16px;background-color:rgba(1, 2, 5, 0);"
                        .into(),
                    &[up, down],
                );
                let style =
                    "display:block;position:relative;overflow:hidden;background-color:white;padding-right:16px;"
                        .to_string();
                el(&mut m, style, &[list, track])
            } else {
                // The shown option first (the box's baseline), then the hidden ones.
                let shown = s.options.iter().find(|o| o.1).map_or("", |o| o.0.as_str());
                let t = m.create_text_node(if shown.is_empty() { "\u{a0}" } else { shown });
                let mut children = vec![el(&mut m, "display:block;".into(), &[t])];
                for (text, _) in s.options.iter().filter(|o| !o.1) {
                    let t = m.create_text_node(text);
                    children.push(el(
                        &mut m,
                        "display:block;height:0;overflow:hidden;visibility:hidden;".into(),
                        &[t],
                    ));
                }
                children.push(button(&mut m, "top:0;right:0;bottom:0;", true, true));
                let style = format!(
                    "display:block;position:relative;overflow:hidden;background-color:white;\
                     padding:0 22px 0 2px;{SUNKEN_IN}"
                );
                el(&mut m, style, &children)
            };
            kept.push(blitz_dom::Attribute { name: style_name.clone(), value: outer_style });
            let outer = m.create_element(span.clone(), kept);
            m.append_children(outer, &[field]);
            m.replace_node_with(s.id, &[outer]);
        }
    }

    /// The other widgets QtWebKit 2.2 draws with Qt's Windows style, measured on the 0.12.6
    /// binary (tests/fidelity/forms-controls, forms-inputs), rebuilt as spans like
    /// [`qt_selects`](Self::qt_selects): `input[type=range]` (a 129×16 slider: a sunken
    /// groove and an 11px raised thumb), `<progress>` (a 130×13 sunken trough with 7px navy
    /// chunks every 9px), `<meter>` (a 65×13 grey gradient bar, filled in green) and
    /// `input[type=file]` (a "Choose File" push button and "No file selected"). Their
    /// fallback content is not shown.
    fn qt_widgets(&mut self) {
        enum Kind {
            Range(f64),
            Progress(Option<f64>),
            Meter(f64),
            File,
        }
        let num =
            |n: &Node, a| n.attr(a).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
        let mut found = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            let kind = match tag(n) {
                "input" => match n.attr(local_name!("type")).map(str::to_ascii_lowercase).as_deref() {
                    Some("range") => {
                        let (min, max) = (
                            num(n, local_name!("min")).unwrap_or(0.0),
                            num(n, local_name!("max")).unwrap_or(100.0),
                        );
                        let max = if max < min { min } else { max };
                        let v = num(n, local_name!("value")).unwrap_or((min + max) / 2.0).clamp(min, max);
                        Some(Kind::Range(if max > min { (v - min) / (max - min) } else { 0.0 }))
                    }
                    Some("file") => Some(Kind::File),
                    _ => None,
                },
                "progress" => {
                    let max = num(n, local_name!("max")).filter(|m| *m > 0.0).unwrap_or(1.0);
                    Some(Kind::Progress(num(n, local_name!("value")).map(|v| (v / max).clamp(0.0, 1.0))))
                }
                "meter" => {
                    let (min, max) = (
                        num(n, local_name!("min")).unwrap_or(0.0),
                        num(n, local_name!("max")).unwrap_or(1.0),
                    );
                    let max = if max < min { min } else { max };
                    let v = num(n, local_name!("value")).unwrap_or(0.0).clamp(min, max);
                    Some(Kind::Meter(if max > min { (v - min) / (max - min) } else { 0.0 }))
                }
                _ => None,
            };
            match kind {
                Some(k) => {
                    let attrs = n.element_data().map(|e| e.attrs.to_vec()).unwrap_or_default();
                    found.push((id, k, attrs));
                }
                None => stack.extend(n.children.iter().copied()),
            }
        }
        if found.is_empty() {
            return;
        }
        let span = blitz_dom::QualName::new(None, blitz_dom::ns!(html), local_name!("span"));
        let style_name = blitz_dom::QualName::new(None, blitz_dom::ns!(), local_name!("style"));
        let mut m = self.doc.mutate();
        let el = |m: &mut blitz_dom::DocumentMutator, style: String, children: &[NodeId]| {
            let attrs = vec![blitz_dom::Attribute { name: style_name.clone(), value: style }];
            let e = m.create_element(span.clone(), attrs);
            m.append_children(e, children);
            e
        };
        // Whole-pixel rectangles `(x0, y0, x1, y1, colour)`.
        let rects = |m: &mut blitz_dom::DocumentMutator,
                     list: &[(f64, f64, f64, f64, &str)]|
         -> Vec<NodeId> {
            list.iter()
                .filter(|r| r.2 > r.0 && r.3 > r.1)
                .map(|&(x0, y0, x1, y1, c)| {
                    el(
                        m,
                        format!(
                            "position:absolute;left:{x0}px;top:{y0}px;width:{}px;height:{}px;background-color:{c};",
                            x1 - x0,
                            y1 - y0
                        ),
                        &[],
                    )
                })
                .collect()
        };
        const LIGHT: &str = "#d4d0c8";
        const SHADOW: &str = "#6a6864";
        for (id, kind, attrs) in found {
            let (outer_style, children) = match kind {
                Kind::Range(f) => {
                    let (w, tw) = (129.0, 11.0);
                    let t = ((w - tw) * f).round();
                    let list = [
                        (0.0, 6.0, 128.0, 7.0, SHADOW),
                        (0.0, 6.0, 1.0, 9.0, SHADOW),
                        (0.0, 9.0, 129.0, 10.0, "white"),
                        (128.0, 6.0, 129.0, 10.0, "white"),
                        (1.0, 7.0, 127.0, 8.0, "black"),
                        (t, 0.0, t + 1.0, 15.0, "white"),
                        (t, 0.0, t + 10.0, 1.0, "white"),
                        (t, 15.0, t + 11.0, 16.0, "black"),
                        (t + 10.0, 0.0, t + 11.0, 16.0, "black"),
                        (t + 1.0, 1.0, t + 2.0, 14.0, LIGHT),
                        (t + 1.0, 1.0, t + 9.0, 2.0, LIGHT),
                        (t + 1.0, 14.0, t + 10.0, 15.0, SHADOW),
                        (t + 9.0, 1.0, t + 10.0, 15.0, SHADOW),
                        (t + 2.0, 2.0, t + 9.0, 14.0, LIGHT),
                    ];
                    // The slider's baseline is its border box bottom (RenderSlider).
                    let style = "display:inline-block;position:relative;width:129px;height:16px;margin:2px;                                 vertical-align:-2px;"
                        .to_string();
                    (style, rects(&mut m, &list))
                }
                Kind::Progress(f) => {
                    let mut list = vec![
                        (0.0, 0.0, 129.0, 1.0, SHADOW),
                        (0.0, 1.0, 1.0, 12.0, SHADOW),
                        (0.0, 12.0, 130.0, 13.0, "white"),
                        (129.0, 0.0, 130.0, 12.0, "white"),
                        (1.0, 1.0, 129.0, 12.0, LIGHT),
                    ];
                    // QWindowsStyle: chunks of 9px (7px and a gap) over the contents rect.
                    let chunks = f.map_or(0, |f| (((130.0 - 6.0) / 9.0_f64).floor() * f).round() as usize);
                    for k in 0..chunks {
                        let x = 3.0 + 9.0 * k as f64;
                        list.push((x, 3.0, x + 7.0, 10.0, "#000080"));
                    }
                    let style =
                        "display:inline-block;position:relative;width:130px;height:13px;vertical-align:-2px;"
                            .to_string();
                    (style, rects(&mut m, &list))
                }
                Kind::Meter(f) => {
                    // WebKit's meter gradient as Qt rasterises it: nine bands below a clear
                    // top row and left column (the 0.12.6 binary prints it as an image).
                    const BG: [&str; 9] = [
                        "#e6e6e6", "#ededed", "#e0e0e0", "#d3d3d3", "#cccccc", "#cdcdcd", "#d0d0d0",
                        "#d4d4d4", "#d8d8d8",
                    ];
                    const FG: [&str; 9] = [
                        "#bce692", "#cbeda8", "#aad37b", "#89b94d", "#77aa33", "#7bae38", "#85b846",
                        "#90c355", "#9bce64",
                    ];
                    let (w, band) = (65.0, 1.25);
                    let fill = (w * f).round();
                    let mut list = Vec::new();
                    for i in 0..9 {
                        let y0 = band * (i + 1) as f64;
                        list.push((band, y0, w, y0 + band, BG[i]));
                        list.push((band, y0, fill, y0 + band, FG[i]));
                    }
                    let style =
                        "display:inline-block;position:relative;width:65px;height:13px;vertical-align:-2px;"
                            .to_string();
                    (style, rects(&mut m, &list))
                }
                Kind::File => {
                    let label = m.create_text_node("Choose File");
                    let button = el(
                        &mut m,
                        "display:inline-block;font-family:sans-serif;color:black;background-color:#d4d0c8;                         border:2px solid rgba(1, 2, 3, 0);padding:4px;margin:2px;"
                            .into(),
                        &[label],
                    );
                    // RenderFileUploadControl draws the file name 6px below the button's
                    // baseline with 13px text, 2px after the button margin.
                    let text = m.create_text_node("No file selected");
                    let name = el(
                        &mut m,
                        "display:inline-block;vertical-align:-6px;margin-left:2px;".into(),
                        &[text],
                    );
                    let style = "display:inline-block;white-space:pre;text-indent:0;".to_string();
                    (style, vec![button, name])
                }
            };
            let mut style = outer_style;
            let mut kept = Vec::new();
            for a in attrs {
                match &*a.name.local {
                    "style" => style.push_str(&a.value),
                    "id" | "class" | "title" | "lang" | "dir" => kept.push(a),
                    _ => {}
                }
            }
            kept.push(blitz_dom::Attribute { name: style_name.clone(), value: style });
            let outer = m.create_element(span.clone(), kept);
            m.append_children(outer, &children);
            m.replace_node_with(id, &[outer]);
        }
    }

    /// Text fields and text areas lose the 2px theme margins in an axis whose size the author
    /// sets (`RenderThemeQt::computeSizeBasedOnStyle` only sizes, and spaces, an auto axis):
    /// a textarea `width: 100px` has no left and right margins, one `height: 50px` no top
    /// and bottom margins (measured on the 0.12.6 binary).
    fn qt_sized_field_margins(&mut self) {
        let is_field = |n: &Node| match tag(n) {
            "textarea" => true,
            "input" => n.attr(local_name!("type")).map(str::to_ascii_lowercase).is_none_or(|t| {
                matches!(
                    t.as_str(),
                    "text"
                        | "password"
                        | "search"
                        | "email"
                        | "url"
                        | "tel"
                        | "number"
                        | "color"
                        | "date"
                        | "datetime"
                        | "datetime-local"
                        | "month"
                        | "week"
                        | "time"
                )
            }),
            _ => false,
        };
        let mut found = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            if !is_field(n) {
                continue;
            }
            let Some(st) = n.primary_styles() else { continue };
            let (pos, m) = (st.get_position(), st.get_margin());
            let theme = |v: &style::values::generics::length::GenericMargin<
                style::values::computed::LengthPercentage,
            >| {
                matches!(v, style::values::generics::length::GenericMargin::LengthPercentage(lp)
                if lp.to_length().is_some_and(|l| l.px() == 2.0))
            };
            let mut extra = String::new();
            if !pos.width.is_auto() && theme(&m.margin_left) && theme(&m.margin_right) {
                extra.push_str("margin-left:0;margin-right:0;");
            }
            if !pos.height.is_auto() && theme(&m.margin_top) && theme(&m.margin_bottom) {
                extra.push_str("margin-top:0;margin-bottom:0;");
            }
            if !extra.is_empty() {
                let style = n.attr(local_name!("style")).unwrap_or("").to_string();
                let sep =
                    if style.trim_end().is_empty() || style.trim_end().ends_with(';') { "" } else { ";" };
                found.push((id, format!("{style}{sep}{extra}")));
            }
        }
        let mut m = self.doc.mutate();
        for (id, style) in found {
            m.set_attribute(
                id,
                blitz_dom::QualName::new(None, blitz_dom::ns!(), local_name!("style")),
                &style,
            );
        }
    }

    /// Horizontal margins, borders and padding of inline elements, which QtWebKit lays out
    /// at the start of the element's first line fragment and the end of its last one;
    /// parley has no inline edges. Each one becomes an inline-block of that width and no
    /// height (`<wkedge data-edge="start|end">`) as the element's first (last) child: it
    /// takes the room in the line, and blitz-paint finds where the element's border box
    /// starts and ends from it (`draw_inline_backgrounds`).
    fn inline_edges(&mut self) {
        use style::values::computed::LengthPercentage;
        use style::values::computed::length_percentage::Unpacked;
        use style::values::generics::length::GenericMargin;
        use style::values::specified::box_::{DisplayInside, DisplayOutside};
        const EDGE: &str = "wkedge";
        // Lengths in whole device pixels (WebKit's `roundForImpreciseConversion`); percentages
        // of the containing block are left out.
        let zoom = blitz_dom::device_zoom();
        let px = |v: &LengthPercentage| match v.unpack() {
            Unpacked::Length(l) => {
                let x = l.px() * zoom;
                (if (x.ceil() - x) <= 0.01 { x.ceil() } else { x.trunc() }) / zoom
            }
            _ => 0.0,
        };
        let mut found = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            let name = tag(n);
            if name.is_empty()
                || matches!(
                    name,
                    EDGE | "br"
                        | "img"
                        | "input"
                        | "select"
                        | "textarea"
                        | "button"
                        | "svg"
                        | "canvas"
                        | "iframe"
                        | "object"
                        | "embed"
                        | "video"
                        | "audio"
                        | "math"
                        | "html"
                        | "body"
                )
            {
                continue;
            }
            let Some(s) = n.primary_styles() else { continue };
            let display = s.get_box().display;
            if display.outside() != DisplayOutside::Inline || display.inside() != DisplayInside::Flow {
                continue;
            }
            let margin = |m: &GenericMargin<LengthPercentage>| match m {
                GenericMargin::LengthPercentage(lp) => px(lp),
                _ => 0.0,
            };
            let b = s.get_border();
            let border = |w: &style::values::computed::BorderSideWidth,
                          st: style::values::specified::border::BorderStyle| {
                if st.none_or_hidden() { 0.0 } else { blitz_dom::webkit_border_width(w.0.to_f32_px()) }
            };
            let (m, p) = (s.get_margin(), s.get_padding());
            let start = margin(&m.margin_left)
                + border(&b.border_left_width, b.border_left_style)
                + px(&p.padding_left.0);
            let end = margin(&m.margin_right)
                + border(&b.border_right_width, b.border_right_style)
                + px(&p.padding_right.0);
            if start == 0.0 && end == 0.0 {
                continue;
            }
            let has_edges = n.children.iter().filter_map(|&c| self.doc.get_node(c)).any(|c| tag(c) == EDGE);
            if !has_edges {
                found.push((id, start, end));
            }
        }
        if found.is_empty() {
            return;
        }
        let edge_name =
            blitz_dom::QualName::new(None, blitz_dom::ns!(html), blitz_dom::LocalName::from(EDGE));
        let attr = |name: &str, value: String| blitz_dom::Attribute {
            name: blitz_dom::QualName::new(None, blitz_dom::ns!(), blitz_dom::LocalName::from(name)),
            value,
        };
        let mut m = self.doc.mutate();
        for (id, start, end) in found {
            for (side, width) in [("start", start), ("end", end)] {
                if width == 0.0 {
                    continue;
                }
                let style = format!(
                    "display:inline-block !important;width:{width}px !important;height:0 !important;\
                     margin:0 !important;padding:0 !important;border:0 !important;\
                     vertical-align:baseline !important;float:none !important;position:static !important;"
                );
                let e = m.create_element(
                    edge_name.clone(),
                    vec![attr("data-edge", side.to_string()), attr("style", style)],
                );
                if side == "start" {
                    m.prepend_nodes(id, &[e]);
                } else {
                    m.append_children(id, &[e]);
                }
            }
        }
    }

    /// QtWebKit's `RenderImage::paintReplaced` for an image that failed to load: a light
    /// gray (`Color::lightGray`) cosmetic line on the edges of its box (drawn by blitz-paint
    /// for the `rgba(1, 2, 6, 0)` outline colour), and the `alt` text at its top left when it
    /// fits. Not with `--no-images`, where the 0.12.6 binary draws nothing.
    ///
    /// Each such image becomes an inline-block span with its `id`, `class` and `style`, sized
    /// by its `width`/`height` attributes or inline style, holding the alt text. A dimension
    /// given neither way is the alt text's (its width truncated to whole pixels, so that the
    /// text no longer fits and is left out; 4px when there is no text). Sizes from style
    /// sheet rules on `img` are lost.
    /// Returns the boxes for [`settle_alt_boxes`](Self::settle_alt_boxes).
    fn broken_images(&mut self) -> Vec<AltBox> {
        if !self.settings.load_images {
            return Vec::new();
        }
        let mut found = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            if tag(n) != "img" {
                continue;
            }
            let Some(el) = n.element_data() else { continue };
            if matches!(el.image_data(), Some(d) if !matches!(d, blitz_dom::node::ImageData::None)) {
                continue;
            }
            let inline = n.attr(local_name!("style")).unwrap_or("").to_ascii_lowercase();
            let in_style =
                |prop: &str| inline.split(';').any(|d| d.split(':').next().is_some_and(|p| p.trim() == prop));
            let size = |name, prop: &str| {
                if in_style(prop) {
                    return Some(String::new());
                }
                let v = n.attr(name)?.trim();
                let num: f32 = v.trim_end_matches('%').trim().parse().ok().filter(|x: &f32| *x >= 0.0)?;
                Some(if v.ends_with('%') { format!("{prop}:{num}%;") } else { format!("{prop}:{num}px;") })
            };
            let w = size(local_name!("width"), "width");
            let h = size(local_name!("height"), "height");
            let block = n.primary_styles().is_some_and(|s| {
                s.get_box().display.outside() == style::values::specified::box_::DisplayOutside::Block
            });
            let alt = n.attr(local_name!("alt")).unwrap_or("").to_string();
            found.push((id, w, h, block, alt, el.attrs.to_vec()));
        }
        let mut out = Vec::new();
        let mut m = self.doc.mutate();
        for (id, w, h, block, alt, attrs) in found {
            let display = if block { "block" } else { "inline-block" };
            let (sized_w, sized_h) = (w.is_some(), h.is_some());
            // An empty alt text sizes the box like a 4px padding alone.
            let pad_w = if sized_w || !alt.is_empty() { "" } else { "padding-right:4px;" };
            let pad_h = if sized_h || !alt.is_empty() { "" } else { "padding-bottom:4px;" };
            let mut style = format!(
                "display:{display};box-sizing:content-box;{}{}{pad_w}{pad_h}overflow:hidden;\
                 white-space:pre;line-height:normal;text-indent:0;text-align:left;\
                 outline:0 none rgba(1, 2, 6, 0);",
                w.unwrap_or_default(),
                h.unwrap_or_default(),
            );
            let mut kept = Vec::new();
            for a in attrs {
                match &*a.name.local {
                    "style" => style.push_str(&a.value),
                    "id" | "class" | "title" | "lang" | "dir" => kept.push(a),
                    _ => {}
                }
            }
            kept.push(blitz_dom::Attribute {
                name: blitz_dom::QualName::new(None, blitz_dom::ns!(), local_name!("style")),
                value: style,
            });
            let span = m.create_element(
                blitz_dom::QualName::new(None, blitz_dom::ns!(html), local_name!("span")),
                kept,
            );
            let text = m.create_text_node(&alt);
            m.append_children(span, &[text]);
            m.replace_node_with(id, &[span]);
            out.push(AltBox { span, text, sized_w, sized_h });
        }
        drop(m);
        out
    }

    /// After a layout: a box sized by its alt text keeps that size without the text, and a
    /// sized box drops a text that does not fit (QtWebKit draws it only when it fits).
    /// Returns whether the layout must be redone.
    fn settle_alt_boxes(&mut self, boxes: &[AltBox]) -> bool {
        let mut changes = Vec::new();
        for b in boxes {
            let Some(n) = self.doc.get_node(b.span) else { continue };
            let l = n.final_layout();
            let (cw, ch) = (l.content_box_width(), l.content_box_height());
            let text = n.element_data().and_then(|e| e.inline_layout_data.as_ref());
            // A width taken from the text is that text's truncated to whole pixels (QtWebKit
            // lays out in integers), so the text no longer fits and is not drawn.
            let fits = b.sized_w
                && !text.is_some_and(|t| {
                    t.layout.width() > cw + 0.01
                        || t.layout.lines().count() > 1
                        || t.layout.height() > ch + 0.01
                });
            if !(b.sized_w && b.sized_h) || !fits {
                let size = (!(b.sized_w && b.sized_h)).then_some((cw.floor(), ch));
                changes.push((b.span, size, (!fits).then_some(b.text)));
            }
        }
        let mut m = self.doc.mutate();
        for &(span, size, text) in &changes {
            if let Some((w, h)) = size {
                m.set_style_property(span, "width", &format!("{w}px"));
                m.set_style_property(span, "height", &format!("{h}px"));
            }
            if let Some(text) = text {
                m.set_node_text(text, "");
            }
        }
        drop(m);
        !changes.is_empty()
    }

    /// Move the boxes positioned against the initial containing block (`position: absolute`
    /// without a positioned ancestor, `position: fixed`) to where QtWebKit 2.2 prints them.
    /// Its printed document starts at the first in-flow content, the margins above being
    /// truncated (see `paginate`), and the containing block runs from there down to the end
    /// of the document: `top: 0` is the top of the first page, `bottom: 0` the end of the
    /// content on the last page, not the bottom of the viewport the layout used (all verified
    /// on the 0.12.6 binary, `tests/fidelity/cases/position-fixed.html` and `positioning.html`).
    /// Returns whether a box moved.
    fn place_viewport_boxes(&mut self, g: &Geometry, viewport_h: f32) -> bool {
        let root = self.doc.root_element();
        let root_id = root.id;
        let doc_h = root.final_layout().size.height;
        let origin = g
            .content_starts
            .iter()
            .copied()
            .find(|&c| c >= -0.01 && c < g.doc_height)
            .unwrap_or(0.0)
            .max(0.0);
        let mut moves = Vec::new();
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            if !n.is_element() {
                continue;
            }
            let Some(styles) = n.primary_styles() else { continue };
            use style::computed_values::position::T as Position;
            if !matches!(styles.get_box().position, Position::Absolute | Position::Fixed) {
                continue;
            }
            let against_icb = match n.containing_block() {
                None => true,
                Some(cb) => cb == root_id || self.doc.get_node(cb).is_none_or(|c| !c.is_element()),
            };
            if !against_icb {
                continue;
            }
            let pos = styles.get_position();
            let dy = if !pos.top.is_auto() {
                origin
            } else if !pos.bottom.is_auto() {
                doc_h - viewport_h
            } else {
                0.0
            };
            if dy != 0.0 {
                moves.push((id, dy));
            }
        }
        for &(id, dy) in &moves {
            if let Some(n) = self.doc.get_node_mut(id) {
                n.final_layout_mut().location.y += dy;
                let y = n.final_layout().location.y;
                self.viewport_shifts.push((id, dy, y));
            }
        }
        !moves.is_empty()
    }

    /// Put back the boxes [`place_viewport_boxes`](Self::place_viewport_boxes) moved, unless a
    /// new layout already replaced their position.
    fn undo_viewport_shifts(&mut self) {
        for (id, dy, y) in std::mem::take(&mut self.viewport_shifts) {
            if let Some(n) = self.doc.get_node_mut(id) {
                if n.final_layout().location.y == y {
                    n.final_layout_mut().location.y -= dy;
                }
            }
        }
    }

    /// RenderFieldset::layoutLegend: a `<legend>` (the fieldset's first element child)
    /// taller than the top border sits at the top of the fieldset's border box, over its
    /// top border and padding, and the content follows it after the top padding (the border
    /// is drawn through the legend's middle by blitz-paint). Here the legend, laid out in
    /// the content box, is pulled up by the top border and padding and pushes the content
    /// down by the top padding. Returns whether the layout must be redone.
    fn settle_legends(&mut self) -> bool {
        let mut changes = Vec::new();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            if tag(n) != "fieldset" {
                continue;
            }
            let Some(legend) =
                n.children.iter().filter_map(|&c| self.doc.get_node(c)).find(|c| c.is_element())
            else {
                continue;
            };
            if tag(legend) != "legend" {
                continue;
            }
            let l = n.final_layout();
            let (border, padding) = (l.border.top, l.padding.top);
            let ll = legend.final_layout();
            if ll.size.height <= border {
                continue;
            }
            let (top, bottom) = (-(border + padding), padding);
            if (ll.margin.top - top).abs() > 0.01 || (ll.margin.bottom - bottom).abs() > 0.01 {
                changes.push((legend.id, top, bottom));
            }
        }
        let mut m = self.doc.mutate();
        for &(id, top, bottom) in &changes {
            m.set_style_property(id, "margin-top", &format!("{top}px"));
            m.set_style_property(id, "margin-bottom", &format!("{bottom}px"));
        }
        drop(m);
        !changes.is_empty()
    }

    fn set_viewport(&mut self, w: f32, h: f32) {
        self.doc.set_viewport(Viewport::new(w as u32, h as u32, 1.0, ColorScheme::Light));
        for _ in 0..4 {
            self.doc.resolve(0.0);
            if !self.doc.has_pending_critical_resources() {
                break;
            }
        }
    }

    /// Print every element box (`WKHTMLTOPDF_DEBUG_LAYOUT`): tag, position,
    /// size, border and padding, indented by depth.
    fn debug_layout(&self) {
        let mut stack = vec![(self.doc.root_element().id, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            for &c in n.children.iter().rev() {
                stack.push((c, depth + 1));
            }
            if !n.is_element() {
                continue;
            }
            let l = n.final_layout();
            let p = n.absolute_position(0.0, 0.0);
            eprintln!(
                "{:indent$}{} x={:.2} y={:.2} w={:.2} h={:.2} border={:?} padding={:?}",
                "",
                tag(n),
                p.x,
                p.y,
                l.size.width,
                l.size.height,
                (l.border.top, l.border.right, l.border.bottom, l.border.left),
                (l.padding.top, l.padding.right, l.padding.bottom, l.padding.left),
                indent = depth * 2
            );
        }
    }

    /// Widest right edge of any box (the document's scroll width).
    fn content_width(&self) -> f32 {
        let mut w: f32 = 0.0;
        let mut stack = vec![self.doc.root_element().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            if !n.is_element() {
                continue;
            }
            let l = n.final_layout();
            if l.size.width > 0.0 {
                let p = n.absolute_position(0.0, 0.0);
                w = w.max(p.x + l.size.width);
                // Text overflowing its box (`white-space: nowrap`, a long word) widens the
                // document too, unless the box clips it: QtWebKit shrinks to the document's
                // overflow width.
                let clips = n
                    .primary_styles()
                    .is_some_and(|s| s.clone_overflow_x() != style::values::computed::Overflow::Visible);
                if let Some(text) = n.element_data().and_then(|e| e.inline_layout_data.as_ref())
                    && !clips
                {
                    // The text boxes' widths, as QtWebKit's overflow (a whole text node is
                    // its preferred width: each word and each space rounded on its own).
                    let lines = text.layout.lines().map(|line| {
                        let m = line.metrics();
                        m.offset + m.advance - m.hanging_advance - line.box_correction()
                    });
                    let text_w = lines.fold(0.0f32, f32::max) / text.layout.scale();
                    w = w.max(p.x + l.border.left + l.padding.left + text_w);
                }
            }
        }
        w
    }

    fn image_sources(&self) -> HashMap<u64, Arc<Vec<u8>>> {
        let mut out = HashMap::new();
        let base = self.doc.base_url().clone();
        let mut stack = vec![self.doc.root_node().id];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            stack.extend(n.children.iter().copied());
            let Some(el) = n.element_data() else { continue };
            let Some(raster) = el.raster_image_data() else { continue };
            let Some(src) = n.attr(local_name!("src")).and_then(|s| base.join(s).ok()) else { continue };
            let bytes = if src.scheme() == "data" {
                data_url_bytes(&src).map(Arc::new)
            } else {
                self.bodies.get(src.as_str()).cloned()
            };
            if let Some(b) = bytes {
                out.insert(raster.data.id(), b);
            }
        }
        out
    }

    fn collect_links(&self, rec: &Recorder) -> (Vec<LinkBox>, Vec<Heading>, Anchors) {
        let doc = &self.doc;
        let base = doc.base_url().clone();
        let mut anchors = Anchors::default();
        let mut headings = Vec::new();
        let mut links = Vec::new();
        // anchor element of each node inside an <a href>
        let mut link_of: HashMap<NodeId, NodeId> = HashMap::new();
        let mut stack: Vec<(NodeId, Option<NodeId>)> = vec![(doc.root_node().id, None)];
        while let Some((id, inherited)) = stack.pop() {
            let Some(n) = doc.get_node(id) else { continue };
            let mut current = inherited;
            if n.is_element() {
                let t = tag(n);
                let pos = n.absolute_position(0.0, 0.0);
                let xy = (pos.x, pos.y);
                if let Some(v) = n.attr(local_name!("id")) {
                    anchors.id.entry(v.to_string()).or_insert(xy);
                }
                if let Some(v) = n.attr(local_name!("name")) {
                    anchors.name.entry(v.to_string()).or_insert(xy);
                    if t == "a" {
                        anchors.a_name.entry(v.to_string()).or_insert(xy);
                    }
                }
                if t == "a" && n.attr(local_name!("href")).is_some() {
                    current = Some(id);
                    let l = n.final_layout();
                    if l.size.width > 0.0 && l.size.height > 0.0 {
                        let href = n.attr(local_name!("href")).unwrap_or("").to_string();
                        links.push(LinkBox {
                            rect: kurbo::Rect::new(
                                pos.x as f64,
                                pos.y as f64,
                                (pos.x + l.size.width) as f64,
                                (pos.y + l.size.height) as f64,
                            ),
                            resolved: base.join(&href).ok(),
                            href,
                        });
                    }
                }
                if let Some(level) = t.strip_prefix('h').and_then(|d| d.parse::<u32>().ok()) {
                    // `findAllElements("h1,…,h9")`: h7–h9 are taken literally.
                    if (1..=9).contains(&level) && t.len() == 2 {
                        let text = n.text_content().replace(['\n', '\r', '\t'], " ");
                        let text = text.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
                        if !text.is_empty() {
                            let l = n.final_layout();
                            headings.push(Heading {
                                level,
                                text,
                                x: pos.x,
                                y: pos.y,
                                w: l.size.width,
                                h: l.size.height,
                            });
                        }
                    }
                }
            }
            if let Some(a) = current {
                link_of.insert(id, a);
            }
            for &c in n.children.iter().rev() {
                stack.push((c, current));
            }
        }
        // Inline anchors: union of their glyph runs.
        let mut inline: HashMap<(NodeId, i64), kurbo::Rect> = HashMap::new();
        for (idx, rt) in &rec.texts {
            let Some(&a) = link_of.get(&rt.node_id) else { continue };
            let Some(bb) = rec.commands.get(*idx).and_then(|c| glyph_run_bbox(&c.get())) else { continue };
            let key = (a, bb.y1.round() as i64);
            inline.entry(key).and_modify(|r| *r = r.union(bb)).or_insert(bb);
        }
        let mut inline: Vec<_> = inline.into_iter().collect();
        inline.sort_by_key(|((node, line), _)| (*line, *node));
        for ((a, _), rect) in inline {
            let n = doc.get_node(a).expect("anchor");
            // Block anchors already have their box.
            let l = n.final_layout();
            if l.size.width > 0.0 && l.size.height > 0.0 {
                continue;
            }
            let href = n.attr(local_name!("href")).unwrap_or("").to_string();
            links.push(LinkBox { rect, resolved: base.join(&href).ok(), href });
        }
        headings.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        (links, headings, anchors)
    }
}

pub(crate) fn is_image_url(url: &Url) -> bool {
    let p = url.path().to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg", ".ico"].iter().any(|e| p.ends_with(e))
}

fn data_url_bytes(url: &Url) -> Option<Vec<u8>> {
    let d = data_url::DataUrl::process(url.as_str()).ok()?;
    d.decode_to_vec().ok().map(|(v, _)| v)
}

// ---------------------------------------------------------------------------
// Pagination geometry (ported from spikes/static-render/src/paginate.rs).

fn breaks_page(b: BreakBetween) -> bool {
    matches!(b, BreakBetween::Page | BreakBetween::Always | BreakBetween::Left | BreakBetween::Right)
}

/// Vertical extent of a box. Blitz lays tables out as a grid of cells:
/// `thead`/`tbody`/`tr` get no box of their own, so fall back to the union of
/// their descendants' cells.
fn extent(doc: &BaseDocument, node: &Node) -> Option<(f32, f32)> {
    if !node.is_element() {
        return None;
    }
    let h = node.final_layout().size.height;
    if h > 0.0 {
        let y = node.absolute_position(0.0, 0.0).y;
        return Some((y, y + h));
    }
    if !matches!(tag(node), "thead" | "tbody" | "tfoot" | "tr") {
        return None;
    }
    node.children
        .iter()
        .filter_map(|&c| doc.get_node(c))
        .filter_map(|c| extent(doc, c))
        .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// Vertical extents of the cells of a table (not of nested tables).
fn collect_cell_edges(doc: &BaseDocument, node: &Node, out: &mut Vec<(f32, f32)>) {
    for &c in node.children.iter() {
        let Some(child) = doc.get_node(c) else { continue };
        match tag(child) {
            "td" | "th" => {
                if let Some(e) = extent(doc, child) {
                    out.push(e);
                }
            }
            "table" => {}
            _ => collect_cell_edges(doc, child, out),
        }
    }
}

/// The [`BoxFill`] of `node`, when it has a background colour or solid side borders. A table
/// with collapsing borders paints no border of its own.
fn box_fill(
    node: &Node,
    styles: &style::properties::ComputedValues,
    top: f32,
    bottom: f32,
) -> Option<BoxFill> {
    use style::values::specified::BorderStyle;
    let current = styles.clone_color();
    let srgb = |c: &style::color::AbsoluteColor| {
        peniko::Color::new(*c.to_color_space(style::color::ColorSpace::Srgb).raw_components())
    };
    let bg = srgb(&styles.get_background().background_color.resolve_to_absolute(&current));
    let background = (bg.components[3] > 0.0).then_some(bg);
    let collapsed = tag(node) == "table"
        && styles.get_inherited_table().border_collapse
            == style::computed_values::border_collapse::T::Collapse;
    let border = styles.get_border();
    let side = |w: f32, st: BorderStyle, c: &style::values::computed::Color| {
        (!collapsed && st == BorderStyle::Solid && w > 0.0)
            .then(|| (w, srgb(&c.resolve_to_absolute(&current))))
    };
    let left =
        side(border.border_left_width.0.to_f32_px(), border.border_left_style, &border.border_left_color);
    let right =
        side(border.border_right_width.0.to_f32_px(), border.border_right_style, &border.border_right_color);
    if background.is_none() && left.is_none() && right.is_none() {
        return None;
    }
    let x0 = node.absolute_position(0.0, 0.0).x;
    Some(BoxFill { top, bottom, x0, x1: x0 + node.final_layout().size.width, background, left, right })
}

/// Collect the pagination data of a laid-out document.
pub fn collect_geometry(doc: &BaseDocument) -> Geometry {
    let mut g = Geometry::default();
    let root = doc.root_element();
    walk(doc, root, &mut g, false);
    g.doc_height = g.doc_height.max(root.final_layout().size.height);
    g.candidates.push(g.doc_height);
    g.candidates.sort_by(f32::total_cmp);
    g.candidates.dedup();
    g.forced.sort_by(f32::total_cmp);
    g.forced.dedup();
    g.content_starts.sort_by(f32::total_cmp);
    g.content_starts.dedup();
    g
}

/// Whether `node` has in-flow content of its own: text, or a child box that is neither floated,
/// out of flow nor `display: none`.
fn has_flow_content(doc: &BaseDocument, node: &Node) -> bool {
    if node.element_data().is_some_and(|e| e.inline_layout_data.is_some()) {
        return true;
    }
    let children = node.layout_children.borrow();
    let ids: Vec<NodeId> = match children.as_ref() {
        Some(c) => c.iter().copied().collect(),
        None => node.children.iter().copied().collect(),
    };
    drop(children);
    ids.iter().filter_map(|&id| doc.get_node(id)).any(|c| {
        if !c.is_element() {
            return false;
        }
        c.primary_styles().is_some_and(|s| {
            let b = s.get_box();
            // A float's margin is not truncated: a container of floats keeps its own top.
            !b.display.is_none()
                && b.float == style::computed_values::float::T::None
                && !matches!(
                    b.position,
                    style::computed_values::position::T::Absolute
                        | style::computed_values::position::T::Fixed
                )
        })
    })
}

/// `oof`: inside an absolutely positioned or fixed box, whose content does not start a page.
fn walk(doc: &BaseDocument, node: &Node, g: &mut Geometry, mut oof: bool) {
    let layout = node.final_layout();
    let (top, bottom) = extent(doc, node).unwrap_or_else(|| {
        let y = node.absolute_position(0.0, 0.0).y;
        (y, y)
    });
    g.doc_height = g.doc_height.max(bottom);
    let t = tag(node).to_string();
    let mut avoid_inside = false;

    if node.is_element() {
        if let Some(styles) = node.primary_styles() {
            let b = styles.get_box();
            if breaks_page(b.break_before) && top > 0.5 {
                g.forced.push(top);
            }
            if breaks_page(b.break_after) {
                g.forced.push(bottom);
            }
            avoid_inside = !matches!(b.break_inside, BreakWithin::Auto);
            oof |= matches!(
                b.position,
                style::computed_values::position::T::Absolute | style::computed_values::position::T::Fixed
            );
            // A page starts at the first box edge at or after the cut: the
            // margins there are truncated, empty boxes with a height are not
            // (QtWebKit keeps a 30px empty div at the top of page 1 but
            // drops the body's margin). A container whose top edge has no
            // border or padding starts where its content does: the margin of
            // a first child that does not collapse through it (Bootstrap's
            // `.container::before` table) is truncated too (verified on the
            // 0.12.6 binary, tests/fidelity/cases/doc-bootstrap.html), while
            // a top border or padding is kept with what lies below it.
            let solid_top =
                layout.border.top + layout.padding.top > 0.0 || t == "table" || !has_flow_content(doc, node);
            if t != "html" && bottom > top && !oof && solid_top {
                g.content_starts.push(top);
            }
            // A float's margin is not truncated: its margin box starts the content.
            if b.float != style::computed_values::float::T::None && bottom > top && !oof {
                g.content_starts.push(top - layout.margin.top.max(0.0));
            }
            if b.display.is_none() {
                return;
            }
            if !oof && bottom > top && !matches!(t.as_str(), "td" | "th" | "tr" | "thead" | "tbody" | "tfoot")
            {
                if let Some(fill) = box_fill(node, &styles, top, bottom) {
                    g.fills.push(fill);
                }
            }
        }
        g.candidates.push(top);
        g.candidates.push(bottom);
    }

    let atomic = matches!(
        t.as_str(),
        "tr" | "td"
            | "th"
            | "img"
            | "svg"
            | "canvas"
            | "video"
            | "iframe"
            | "input"
            | "textarea"
            | "select"
            | "button"
    ) || avoid_inside;
    if atomic && bottom > top {
        if !oof {
            g.content_starts.push(top);
        }
        g.atomic.push((top, bottom));
        g.straddlers.push((top, bottom));
        g.content_boxes.push((top, bottom));
        return;
    }

    if t == "table" {
        let mut thead = None;
        let mut tfoot = None;
        for &c in node.children.iter() {
            let Some(child) = doc.get_node(c) else { continue };
            match tag(child) {
                "thead" if thead.is_none() => thead = extent(doc, child),
                "tfoot" if tfoot.is_none() => tfoot = extent(doc, child),
                _ => {}
            }
        }
        // The repeated groups take the border gutters and spacing around their
        // cells with them: the header from the table's top edge down to the
        // next row's cells, the footer from the previous row's cells down to
        // the table's bottom edge (WebKit repeats the painted group).
        let mut cell_edges = Vec::new();
        collect_cell_edges(doc, node, &mut cell_edges);
        let thead = thead.map(|(a, b)| {
            let a = if a - top < 8.0 { top } else { a };
            let next =
                cell_edges.iter().map(|e| e.0).filter(|&t| t >= b - 0.01).fold(f32::INFINITY, f32::min);
            (a, if next.is_finite() { next } else { b })
        });
        let tfoot = tfoot.map(|(a, b)| {
            let prev =
                cell_edges.iter().map(|e| e.1).filter(|&t| t <= a + 0.01).fold(f32::NEG_INFINITY, f32::max);
            let b = if bottom - b < 8.0 { bottom } else { b };
            (if prev.is_finite() { prev } else { a }, b)
        });
        g.tables.push(TableInfo { top, bottom, thead, tfoot });
    }

    if let Some(text) = node.element_data().and_then(|e| e.inline_layout_data.as_ref()) {
        let y0 = top + layout.border.top + layout.padding.top + text.block_offset;
        let lines: Vec<(f32, f32)> = text
            .layout
            .lines()
            .map(|l| (y0 + l.metrics().block_min_coord, y0 + l.metrics().block_max_coord))
            .collect();
        if lines.len() > 1 {
            g.paragraphs.push(lines);
        }
        for (i, line) in text.layout.lines().enumerate() {
            let m = line.metrics();
            let (a, b) = (y0 + m.block_min_coord, y0 + m.block_max_coord);
            // QtWebKit 2.2 paginates on the root inline box's lineTop/lineBottom, which leave
            // out the half-leading: a line straddles the cut when its content area (ascent and
            // descent) does, and the page then ends at the top of that content area, or at the
            // line box top for the first line of a block (both verified on the 0.12.6 binary).
            let (ca, cb) = (y0 + m.content_block_min_coord, y0 + m.content_block_max_coord);
            let cb = cb.max(ca);
            g.straddlers.push((ca, cb));
            g.atomic.push((if i == 0 { ca.min(a) } else { ca }, cb));
            g.content_boxes.push((a, b));
            if !oof {
                g.content_starts.push(a);
            }
            g.candidates.extend([a, b, ca, cb]);
        }
    }

    let children = node.layout_children.borrow();
    let ids: Vec<_> = match children.as_ref() {
        Some(c) => c.iter().copied().collect(),
        None => node.children.iter().copied().collect(),
    };
    drop(children);
    for id in ids {
        if let Some(child) = doc.get_node(id) {
            walk(doc, child, g, oof);
        }
    }
}
