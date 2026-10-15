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
}

/// The box standing in for an image that failed to load (`WebPage::broken_images`).
struct AltBox {
    span: NodeId,
    /// The alt text node.
    text: NodeId,
    /// Both dimensions given: the box does not take the size of its text.
    sized: bool,
}

fn tag(node: &Node) -> &str {
    node.element_data().map(|e| &*e.name.local).unwrap_or("")
}

/// Form controls as QtWebKit 2.2 draws them with Qt's default style
/// (measured on the 0.12.6 binary, tests/parts/forms): text fields and
/// textareas inherit the font, have a 1 px black frame on white and a 1 px
/// side inset; checkboxes and radios are 13 px; push buttons use the
/// default sans-serif face on Qt's button grey. Text field and textarea
/// widths come from `size`/`cols` (blitz-dom layout).
const QT_FORM_CONTROLS: &str = "\
input:not([type]), input[type=text], input[type=password], input[type=search], input[type=email], \
input[type=url], input[type=tel], input[type=number], textarea { font: inherit; color: black; \
border: 1px solid black; padding: 0 1px; margin: 2px; background-color: white; line-height: normal; }
textarea { padding: 0; }
input[type=checkbox], input[type=radio] { width: 13px; height: 13px; border: none; padding: 0; \
margin: 3px 3px 3px 4px; color: black; }
input[type=radio] { margin: 3px 3px 0 5px; }
input[type=submit], input[type=reset], input[type=button], button { font: inherit; \
font-family: sans-serif; color: black; background-color: #d4d0c8; border: 1px solid #404040; \
border-top-color: white; border-left-color: white; border-radius: 0; padding: 3px 6px; margin: 2px; }
select { font: inherit; color: black; background-color: white; border: 1px solid black; margin: 2px; }
";

pub(crate) fn ua_css(settings: &WebSettings) -> String {
    let mut css = String::from(QT_FORM_CONTROLS);
    if settings.enable_javascript {
        css.push_str("noscript { display: none !important; }\n");
    }
    if !settings.background {
        css.push_str("* { background-color: transparent !important; background-image: none !important; }\n");
    }
    if settings.produce_forms {
        // `elm.setStyleProperty("color", "white")` on every input and
        // textarea: text fields keep the inner-text colour computed at
        // load, so only the theme-painted marks (check, radio dot) vanish.
        css.push_str("input[type=checkbox], input[type=radio] { color: white !important; }\n");
    }
    css
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
    pub fn load(
        html: &str,
        url: Url,
        settings: WebSettings,
        font_ctx: FontContext,
        fetcher: &mut dyn Fetcher,
    ) -> WebPage {
        crate::profile::install();
        style::properties::cascade::set_minimum_font_size(settings.minimum_font_size as f32);
        let (vw, vh) = settings.viewport.unwrap_or((0, 0));
        crate::profile::set_media_viewport(vw as f32, vh as f32);
        if settings.enable_javascript && crate::scripting::needs_js(html) {
            return crate::scripting::load(html, url, settings, font_ctx, fetcher);
        }
        let net = QueueNet::default();
        let media = if settings.print_media_type { MediaType::print() } else { MediaType::screen() };
        let config = DocumentConfig {
            viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
            base_url: Some(url.to_string()),
            net_provider: Some(Arc::new(net.clone())),
            font_ctx: Some(font_ctx),
            media_type: Some(media),
            ..Default::default()
        };
        // QtWebKit parses every `text/html` document with its HTML parser,
        // whatever its DOCTYPE or root namespace; blitz-html would switch to
        // its XML parser on an XHTML DOCTYPE, an XML declaration or an
        // `xmlns` on `<html>` (then `<ol/>` would be an empty list, as in no
        // browser). A leading empty comment defeats the sniffing.
        let sniff_proof = format!("<!---->{html}");
        let mut doc = HtmlDocument::from_html(&sniff_proof, config).into_inner();
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
        };
        page.mask_passwords();
        page.load_resources(fetcher);
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
        let mut page = WebPage { doc, net, settings, bodies, password_values: HashMap::new(), url, canvases };
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
    pub fn render_final(mut self, printable_w_pt: f32, printable_h_pt: f32) -> RenderedPage {
        self.render_inner(printable_w_pt, printable_h_pt, true)
    }

    fn render_inner(&mut self, printable_w_pt: f32, printable_h_pt: f32, release_text: bool) -> RenderedPage {
        let zoom = if self.settings.zoom > 0.0 { self.settings.zoom } else { 1.0 };
        let (min_shrink, max_shrink) =
            if self.settings.smart_shrinking { (MIN_SHRINK, MAX_SHRINK) } else { (1.0, 1.0) };
        // Printable width in CSS px at 96 dpi.
        let base_w = printable_w_pt / 0.75 / zoom;
        let base_h = printable_h_pt / 0.75 / zoom;
        let mut layout_w = (base_w * min_shrink).round().max(1.0);
        let vh = (base_h * min_shrink).round().max(1.0);
        let alt_boxes = self.broken_images();
        self.set_viewport(layout_w, vh);
        if self.settings.smart_shrinking {
            let content_w = self.content_width();
            if content_w > layout_w + 0.5 {
                layout_w = content_w.min((base_w * max_shrink).round()).ceil();
                self.set_viewport(layout_w, vh);
            }
        }
        if self.settle_alt_boxes(&alt_boxes) {
            self.set_viewport(layout_w, vh);
        }
        let pt_per_px = printable_w_pt / layout_w;

        let geometry = collect_geometry(&self.doc);
        let doc_h = geometry.doc_height.ceil().max(1.0) as u32;
        let mut recorder = Recorder::default();
        blitz_paint::canvas::set_pictures(self.canvases.clone());
        if release_text {
            blitz_paint::inline_layouts::set(self.doc.take_inline_layouts());
        }
        blitz_paint::paint_scene(&mut recorder, &mut self.doc, 1.0, layout_w as u32, doc_h, 0, 0);
        blitz_paint::inline_layouts::clear();
        blitz_paint::canvas::clear_pictures();

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

    /// QtWebKit's `RenderImage::paintReplaced` for an image that failed to load: a 1px light
    /// gray (`Color::lightGray`) rectangle on its content box, and the `alt` text at its top
    /// left when it fits. Not with `--no-images`, where the 0.12.6 binary draws nothing.
    ///
    /// Each such image becomes an inline-block span with its `id`, `class` and `style`, sized
    /// by its `width`/`height` attributes or inline style, holding the alt text. A dimension
    /// given neither way is the alt text's (width plus 4px; 4px when there is no text), and
    /// QtWebKit then leaves the text out. Sizes from style sheet rules on `img` are lost.
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
            let pad_w = if sized_w { "" } else { "padding-right:4px;" };
            let pad_h = if sized_h || !alt.is_empty() { "" } else { "padding-bottom:4px;" };
            let mut style = format!(
                "display:{display};box-sizing:content-box;{}{}{pad_w}{pad_h}overflow:hidden;\
                 white-space:pre;line-height:normal;text-indent:0;text-align:left;\
                 outline:1px solid #c0c0c0;outline-offset:-1px;",
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
            out.push(AltBox { span, text, sized: sized_w && sized_h });
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
            if !b.sized {
                changes.push((b.span, Some((cw, ch)), b.text));
            } else if text.is_some_and(|t| {
                t.layout.width() > cw + 0.01 || t.layout.lines().count() > 1 || t.layout.height() > ch + 0.01
            }) {
                changes.push((b.span, None, b.text));
            }
        }
        let mut m = self.doc.mutate();
        for &(span, size, text) in &changes {
            if let Some((w, h)) = size {
                m.set_style_property(span, "width", &format!("{w}px"));
                m.set_style_property(span, "height", &format!("{h}px"));
            }
            m.set_node_text(text, "");
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

/// Collect the pagination data of a laid-out document.
pub fn collect_geometry(doc: &BaseDocument) -> Geometry {
    let mut g = Geometry::default();
    let root = doc.root_element();
    walk(doc, root, &mut g);
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

fn walk(doc: &BaseDocument, node: &Node, g: &mut Geometry) {
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
            // A page starts at the first box edge at or after the cut: the
            // margins there are truncated, empty boxes with a height are not
            // (QtWebKit keeps a 30px empty div at the top of page 1 but
            // drops the body's margin).
            if t != "html" && bottom > top {
                g.content_starts.push(top);
            }
            if b.display.is_none() {
                return;
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
        g.content_starts.push(top);
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
            g.content_starts.push(a);
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
            walk(doc, child, g);
        }
    }
}
