//! PDF output. Stands in for `QPrinter` + the patched `QPdfEngine` as used
//! by `PdfConverter` (`pdfconverter.cc`): one PDF page per output page with
//! its bands (content slice, HTML header/footer), header/footer texts and
//! lines, named destinations (`addAnchor`), link annotations (`addLink`,
//! `addHyperlink`), the outline, `QPrinter::GrayScale`, and the document
//! information dictionary (`/Title`, `/Creator wkhtmltopdf 0.12.6`,
//! `/Producer Qt 4.8.7`, `/CreationDate` in UTC, like the 0.12.6 binary).
//!
//! The display list recorded by the engine is replayed into krilla: real,
//! subset text (with the source text of every glyph run), images (JPEG
//! passed through), linear/radial/sweep gradients, clips, opacity.
//! Not yet (TODO P2b/P5): box shadows, filters, blend modes,
//! `--lowquality`, `--image-dpi`/`--image-quality`, `--no-pdf-compression`.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use anyrender::Paint;
use anyrender::recording::RenderCommand;
use krilla::color::rgb;
use krilla::configure::{ConfigurationBuilder, PdfVersion};
use krilla::destination::XyzDestination;
use krilla::geom::{Path, PathBuilder, Point, Rect, Size, Transform};
use krilla::metadata::{DateTime, Metadata};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{
    Fill, FillRule, LinearGradient, RadialGradient, SpreadMethod, Stop, Stroke, SweepGradient,
};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::{Document, SerializeSettings};
use kurbo::{Affine, BezPath, PathEl, Shape};
use peniko::Color;
use skrifa::MetadataProvider;
use wk_webkit::{Command, DisplayList, RenderedPage, RunText};

use crate::paginate::PageSlice;

/// Paper and margins, in points.
#[derive(Debug, Clone, Copy)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
}

impl PageSetup {
    pub fn printable_width(&self) -> f32 {
        (self.width - self.margin_left - self.margin_right).max(1.0)
    }

    pub fn printable_height(&self) -> f32 {
        (self.height - self.margin_top - self.margin_bottom).max(1.0)
    }
}

/// A band of a rendered document drawn on a page: the main content slice,
/// or the first page of an HTML header/footer.
pub struct Band<'a> {
    pub page: &'a RenderedPage,
    pub slice: &'a PageSlice,
    /// Top-left corner of the band on the paper, in points (top-left origin).
    pub origin: (f32, f32),
}

/// A font face for text items (header/footer text).
#[derive(Clone)]
pub struct FontFace {
    pub data: Arc<Vec<u8>>,
    pub index: u32,
}

impl FontFace {
    /// Ascent and descent (both positive) in points at `size` points.
    pub fn ascent_descent(&self, size: f32) -> (f32, f32) {
        let Ok(f) = skrifa::FontRef::from_index(&self.data, self.index) else {
            return (size * 0.93, size * 0.24);
        };
        let m = f.metrics(skrifa::instance::Size::new(size), skrifa::instance::LocationRef::default());
        (m.ascent, -m.descent)
    }

    /// Advance width of `text` (no kerning) in points at `size` points.
    pub fn text_width(&self, size: f32, text: &str) -> f32 {
        let Ok(f) = skrifa::FontRef::from_index(&self.data, self.index) else { return 0.0 };
        let cmap = f.charmap();
        let gm = f.glyph_metrics(skrifa::instance::Size::new(size), skrifa::instance::LocationRef::default());
        text.chars().map(|c| cmap.map(c).and_then(|g| gm.advance_width(g)).unwrap_or(0.0)).sum()
    }
}

/// A single-line text drawn at a baseline (`QPainter::drawText`).
#[derive(Clone)]
pub struct TextItem {
    pub face: FontFace,
    /// Font size in points.
    pub size: f32,
    pub x: f32,
    pub baseline: f32,
    pub text: String,
}

/// A horizontal line (`QPainter::drawLine` with the default pen), in
/// points. Qt's default pen is cosmetic (width 0): QPdfEngine writes it as
/// a filled outline `width` thick (0.1 device pixel), extended by half that
/// width at both ends, which viewers draw as a faint hairline.
#[derive(Debug, Clone, Copy)]
pub struct LineItem {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub width: f32,
}

/// Where a link goes.
#[derive(Debug, Clone)]
pub enum LinkTarget {
    Uri(String),
    /// A named destination (`QPainter::addLink`), defined by an [`Anchor`]
    /// somewhere in the document.
    Named(String),
}

/// A link annotation; `rect` is in points, top-left origin.
#[derive(Debug, Clone)]
pub struct PdfLink {
    pub rect: (f32, f32, f32, f32),
    pub target: LinkTarget,
}

/// A named destination (`QPainter::addAnchor`), in points (top-left origin).
#[derive(Debug, Clone)]
pub struct Anchor {
    pub name: String,
    pub x: f32,
    pub y: f32,
}

/// An AcroForm field (`QPainter::addTextField` / `addCheckBox` of the
/// patched Qt). Rectangles are in points, top-left origin.
#[derive(Debug, Clone)]
pub enum FormField {
    Text {
        rect: (f32, f32, f32, f32),
        value: String,
        name: String,
        multiline: bool,
        password: bool,
        readonly: bool,
        /// `-1` when unset.
        maxlen: i32,
    },
    CheckBox {
        rect: (f32, f32, f32, f32),
        checked: bool,
        name: String,
        readonly: bool,
    },
}

/// Everything drawn on one output page, in painting order: bands, then
/// lines and texts.
#[derive(Default)]
pub struct OutPage<'a> {
    pub bands: Vec<Band<'a>>,
    pub texts: Vec<TextItem>,
    pub lines: Vec<LineItem>,
    pub links: Vec<PdfLink>,
    pub anchors: Vec<Anchor>,
    pub fields: Vec<FormField>,
}

/// Output options of the printer.
#[derive(Debug, Clone, Copy, Default)]
pub struct PdfOptions {
    /// `QPrinter::GrayScale`: every colour becomes a grey RGB triple.
    pub grayscale: bool,
    /// `--image-dpi`: raster images (other than JPEG, embedded as is) drawn at a
    /// higher resolution are scaled down to it, like Qt's PDF engine (0: never).
    pub image_dpi: u32,
}

/// A bookmark.
#[derive(Debug, Clone)]
pub struct OutlineNode {
    pub title: String,
    /// Named destination of the bookmark (`beginSectionOutline(title,
    /// anchor)`); when it is defined, it wins over `page`/`x`/`y`.
    pub anchor: String,
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub children: Vec<OutlineNode>,
}

/// Document information.
#[derive(Debug, Clone, Default)]
pub struct DocInfo {
    pub title: Option<String>,
    pub creator: String,
    pub producer: String,
    /// Seconds since the Unix epoch (UTC).
    pub creation_time: Option<u64>,
}

/// A face of the display list, shared by the threads drawing pages.
struct FontEntry {
    font: Font,
    /// Smallest code point mapped to each glyph id (`u32::MAX`: none), built on first use:
    /// only runs without source text need it.
    rev_cmap: OnceLock<Vec<u32>>,
    /// Bits of the advance width of each glyph id in em (those of NaN: not read yet), read on
    /// first use: only the last glyph of a run needs it. Two threads reading the same glyph
    /// store the same value.
    advances: Vec<AtomicU32>,
}

impl FontEntry {
    fn advance(&self, fd: &peniko::FontData, gid: u32) -> f32 {
        let Some(slot) = self.advances.get(gid as usize) else { return 0.0 };
        let a = f32::from_bits(slot.load(Ordering::Relaxed));
        if !a.is_nan() {
            return a;
        }
        let a = skrifa::FontRef::from_index(fd.data.data(), fd.index)
            .map(|fref| {
                let size = skrifa::instance::Size::unscaled();
                let upem =
                    f32::from(fref.metrics(size, skrifa::instance::LocationRef::default()).units_per_em);
                let gm = fref.glyph_metrics(size, skrifa::instance::LocationRef::default());
                gm.advance_width(skrifa::GlyphId::new(gid)).unwrap_or(0.0) / upem
            })
            .unwrap_or(0.0);
        slot.store(a.to_bits(), Ordering::Relaxed);
        a
    }

    fn char_of(&self, fd: &peniko::FontData, gid: u32) -> Option<char> {
        let map = self.rev_cmap.get_or_init(|| {
            let mut map = vec![u32::MAX; self.advances.len()];
            let Ok(fref) = skrifa::FontRef::from_index(fd.data.data(), fd.index) else {
                return map;
            };
            for (cp, gid) in fref.charmap().mappings() {
                if char::from_u32(cp).is_none() {
                    continue;
                }
                let g = gid.to_u32() as usize;
                if g >= map.len() {
                    map.resize(g + 1, u32::MAX);
                }
                map[g] = map[g].min(cp);
            }
            map
        });
        map.get(gid as usize).and_then(|&cp| char::from_u32(cp))
    }
}

/// The faces of the display lists, by blob id and index.
type FontMap = HashMap<(u64, u32), Option<Arc<FontEntry>>>;

/// What the threads drawing pages share: the faces, and the culling data of each display
/// list (keyed by its address), computed by the first thread that needs it.
#[derive(Default)]
struct Shared {
    fonts: Mutex<FontMap>,
    cullings: Mutex<HashMap<usize, Arc<OnceLock<Culling>>>>,
    /// Faces of the header/footer texts, keyed by the address of their data.
    text_fonts: Mutex<HashMap<(usize, u32), Option<Font>>>,
}

/// One thread's view of [`Shared`], and its images.
struct Caches<'s> {
    shared: &'s Shared,
    fonts: FontMap,
    images: HashMap<(u64, u32, u32), Option<krilla::image::Image>>,
    /// Culling box ([`bbox`]) of every command of a display list, keyed by
    /// its address: computed once, not once per page and band.
    bboxes: HashMap<usize, Arc<OnceLock<Culling>>>,
}

impl<'s> Caches<'s> {
    fn new(shared: &'s Shared) -> Self {
        Caches { shared, fonts: HashMap::new(), images: HashMap::new(), bboxes: HashMap::new() }
    }

    fn culling(&mut self, commands: &DisplayList) -> Arc<OnceLock<Culling>> {
        let key = commands as *const DisplayList as usize;
        let shared = self.shared;
        let cell = self
            .bboxes
            .entry(key)
            .or_insert_with(|| shared.cullings.lock().unwrap().entry(key).or_default().clone())
            .clone();
        cell.get_or_init(|| Culling::new(commands));
        cell
    }
}

/// Culling data of a display list: the [`bbox`] of every command, and the commands
/// bucketed by vertical extent, so a band only visits the commands that may touch it
/// instead of the whole document's.
struct Culling {
    bboxes: Vec<Option<kurbo::Rect>>,
    /// Commands drawn in every band (no box, or a box with a NaN edge).
    always: Vec<u32>,
    /// Commands whose box overlaps each `BUCKET_H` px slice of the document; boxes
    /// above the first or below the last slice are counted in it.
    buckets: Vec<Vec<u32>>,
}

impl Culling {
    const BUCKET_H: f64 = 256.0;
    const MAX_BUCKETS: usize = 1 << 14;

    fn new(commands: &DisplayList) -> Culling {
        let mut bboxes = Vec::with_capacity(commands.len());
        bboxes.extend(commands.iter().map(|c| match c.as_rect_fill() {
            // The bounding box of the rectangle's path, which is the rectangle.
            Some((_, transform, _, rect)) => Some(transform.transform_rect_bbox(rect.abs())),
            None => bbox(&c.get()),
        }));
        let bottom = bboxes.iter().flatten().map(|b| b.y1).filter(|y| y.is_finite()).fold(0.0f64, f64::max);
        let n = ((bottom / Self::BUCKET_H) as usize + 1).min(Self::MAX_BUCKETS);
        let mut c = Culling { bboxes: Vec::new(), always: Vec::new(), buckets: vec![Vec::new(); n] };
        for (i, b) in bboxes.iter().enumerate() {
            match b {
                Some(b) if !b.y0.is_nan() && !b.y1.is_nan() => {
                    let (lo, hi) = c.bucket_range(b.y0, b.y1);
                    for bucket in &mut c.buckets[lo..=hi] {
                        bucket.push(i as u32);
                    }
                }
                _ => c.always.push(i as u32),
            }
        }
        for bucket in &mut c.buckets {
            bucket.shrink_to_fit();
        }
        c.bboxes = bboxes;
        c
    }

    /// Buckets covering `[y0, y1]` (clamped to the existing ones, so that two
    /// overlapping ranges always share a bucket).
    fn bucket_range(&self, y0: f64, y1: f64) -> (usize, usize) {
        let last = self.buckets.len() - 1;
        let at = |y: f64| ((y / Self::BUCKET_H).floor().max(0.0) as usize).min(last);
        let (a, b) = (at(y0), at(y1));
        (a.min(b), a.max(b))
    }

    /// Commands that may be drawn in the band `[y0, y1]`, in display-list order.
    fn candidates(&self, y0: f64, y1: f64) -> Vec<u32> {
        let mut out = self.always.clone();
        if !y0.is_nan() && !y1.is_nan() {
            let (lo, hi) = self.bucket_range(y0, y1);
            for bucket in &self.buckets[lo..=hi] {
                out.extend_from_slice(bucket);
            }
        } else {
            // Not reached (bands are finite); keep the exact behaviour anyway.
            out = (0..self.bboxes.len() as u32).collect();
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

fn tf(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

fn to_path(p: &BezPath) -> Option<Path> {
    path_from_elements(p.elements().iter().copied())
}

fn path_from_elements(elements: impl Iterator<Item = PathEl>) -> Option<Path> {
    let mut b = PathBuilder::new();
    for el in elements {
        match el {
            PathEl::MoveTo(a) => b.move_to(a.x as f32, a.y as f32),
            PathEl::LineTo(a) => b.line_to(a.x as f32, a.y as f32),
            PathEl::QuadTo(a, c) => b.quad_to(a.x as f32, a.y as f32, c.x as f32, c.y as f32),
            PathEl::CurveTo(a, c, d) => {
                b.cubic_to(a.x as f32, a.y as f32, c.x as f32, c.y as f32, d.x as f32, d.y as f32)
            }
            PathEl::ClosePath => b.close(),
        }
    }
    b.finish()
}

fn rect_path(x0: f32, y0: f32, x1: f32, y1: f32) -> Option<Path> {
    to_path(&kurbo::Rect::new(f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1)).to_path(0.1))
}

fn norm(v: f32) -> NormalizedF32 {
    NormalizedF32::new(v.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

/// `qGray`: Qt's grey level of an RGB colour.
fn q_gray(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 11 + u32::from(g) * 16 + u32::from(b) * 5) / 32) as u8
}

fn rgb_of(c: Color, gray: bool) -> (rgb::Color, f32) {
    let rgba = c.to_rgba8();
    let a = f32::from(rgba.a) / 255.0;
    if gray {
        let v = q_gray(rgba.r, rgba.g, rgba.b);
        return (rgb::Color::new(v, v, v), a);
    }
    (rgb::Color::new(rgba.r, rgba.g, rgba.b), a)
}

fn stops(g: &peniko::Gradient, gray: bool) -> Vec<Stop> {
    g.stops
        .iter()
        .map(|s| {
            let (c, a) = rgb_of(s.color.to_alpha_color(), gray);
            Stop { offset: norm(s.offset), color: c.into(), opacity: norm(a) }
        })
        .collect()
}

fn spread(e: peniko::Extend) -> SpreadMethod {
    match e {
        peniko::Extend::Pad => SpreadMethod::Pad,
        peniko::Extend::Repeat => SpreadMethod::Repeat,
        peniko::Extend::Reflect => SpreadMethod::Reflect,
    }
}

/// A krilla fill for an anyrender paint, or `None` when it cannot be
/// expressed (images are handled separately).
fn paint_fill(
    p: &Paint,
    brush_transform: Option<Affine>,
    alpha: f32,
    rule: peniko::Fill,
    gray: bool,
) -> Option<Fill> {
    let rule = if rule == peniko::Fill::EvenOdd { FillRule::EvenOdd } else { FillRule::NonZero };
    let transform = tf(brush_transform.unwrap_or(Affine::IDENTITY));
    match p {
        Paint::Solid(c) => {
            let (col, a) = rgb_of(*c, gray);
            Some(Fill { paint: col.into(), opacity: norm(a * alpha), rule })
        }
        Paint::Gradient(g) => {
            let st = stops(g, gray);
            if st.is_empty() {
                return None;
            }
            let paint: krilla::paint::Paint = match &g.kind {
                peniko::GradientKind::Linear(l) => LinearGradient {
                    x1: l.start.x as f32,
                    y1: l.start.y as f32,
                    x2: l.end.x as f32,
                    y2: l.end.y as f32,
                    transform,
                    spread_method: spread(g.extend),
                    stops: st,
                    anti_alias: false,
                }
                .into(),
                peniko::GradientKind::Radial(r) => RadialGradient {
                    fx: r.start_center.x as f32,
                    fy: r.start_center.y as f32,
                    fr: r.start_radius,
                    cx: r.end_center.x as f32,
                    cy: r.end_center.y as f32,
                    cr: r.end_radius,
                    transform,
                    spread_method: spread(g.extend),
                    stops: st,
                    anti_alias: false,
                }
                .into(),
                peniko::GradientKind::Sweep(s) => SweepGradient {
                    cx: s.center.x as f32,
                    cy: s.center.y as f32,
                    start_angle: s.start_angle.to_degrees(),
                    end_angle: s.end_angle.to_degrees(),
                    transform,
                    spread_method: spread(g.extend),
                    stops: st,
                    anti_alias: false,
                }
                .into(),
            };
            Some(Fill { paint, opacity: norm(alpha), rule })
        }
        _ => None,
    }
}

fn font_entry<'a>(caches: &'a mut Caches, fd: &peniko::FontData) -> Option<&'a FontEntry> {
    let shared = caches.shared;
    caches
        .fonts
        .entry((fd.data.id(), fd.index))
        .or_insert_with(|| {
            shared
                .fonts
                .lock()
                .unwrap()
                .entry((fd.data.id(), fd.index))
                .or_insert_with(|| {
                    let fref = skrifa::FontRef::from_index(fd.data.data(), fd.index).ok()?;
                    let fm = fref.metrics(
                        skrifa::instance::Size::unscaled(),
                        skrifa::instance::LocationRef::default(),
                    );
                    let glyph_count = usize::from(fm.glyph_count);
                    // krilla shares the blob's bytes: no copy of the font file per face.
                    let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(fd.data.clone());
                    let font = Font::new(data.into(), fd.index)?;
                    Some(Arc::new(FontEntry {
                        font,
                        rev_cmap: OnceLock::new(),
                        advances: (0..glyph_count).map(|_| AtomicU32::new(f32::NAN.to_bits())).collect(),
                    }))
                })
                .clone()
        })
        .as_deref()
}

/// Glyph outlines as one path (text that must not be extractable, such as
/// list bullets, which QtWebKit draws as shapes).
fn glyph_outlines(fd: &peniko::FontData, size: f32, glyphs: &[anyrender::Glyph]) -> Option<Path> {
    use skrifa::outline::{DrawSettings, OutlinePen};
    struct Pen<'a> {
        b: &'a mut PathBuilder,
        dx: f32,
        dy: f32,
    }
    impl OutlinePen for Pen<'_> {
        fn move_to(&mut self, x: f32, y: f32) {
            self.b.move_to(self.dx + x, self.dy - y);
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.b.line_to(self.dx + x, self.dy - y);
        }
        fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
            self.b.quad_to(self.dx + cx, self.dy - cy, self.dx + x, self.dy - y);
        }
        fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
            self.b.cubic_to(self.dx + a, self.dy - b, self.dx + c, self.dy - d, self.dx + x, self.dy - y);
        }
        fn close(&mut self) {
            self.b.close();
        }
    }
    let data = fd.data.data();
    let fref = skrifa::FontRef::from_index(data, fd.index).ok()?;
    let outlines = fref.outline_glyphs();
    let mut b = PathBuilder::new();
    for g in glyphs {
        if let Some(o) = outlines.get(skrifa::GlyphId::new(g.id)) {
            let settings = DrawSettings::unhinted(
                skrifa::instance::Size::new(size),
                skrifa::instance::LocationRef::default(),
            );
            let mut pen = Pen { b: &mut b, dx: g.x, dy: g.y };
            let _ = o.draw(settings, &mut pen);
        }
    }
    b.finish()
}

/// Bounding box of a command in scene coordinates, for page culling.
fn bbox(cmd: &RenderCommand) -> Option<kurbo::Rect> {
    match cmd {
        RenderCommand::Fill(c) => Some(c.transform.transform_rect_bbox(c.shape.bounding_box())),
        RenderCommand::Stroke(c) => Some(
            c.transform.transform_rect_bbox(c.shape.bounding_box()).inflate(c.style.width, c.style.width),
        ),
        RenderCommand::GlyphRun(c) => {
            // Cull text by baseline: a line belongs to exactly one page,
            // otherwise clipped glyphs would still be extractable text on the
            // neighbouring page.
            let g = c.glyphs.first()?;
            let p = c.transform * kurbo::Point::new(f64::from(g.x), f64::from(g.y));
            Some(kurbo::Rect::new(p.x, p.y, p.x, p.y))
        }
        RenderCommand::BoxShadow(c) => Some(c.transform.transform_rect_bbox(c.rect)),
        _ => None,
    }
}

struct Replayer<'a, 's> {
    page: &'a RenderedPage,
    /// Culling boxes of the page's scene.
    culling: &'a Culling,
    caches: &'a mut Caches<'s>,
    image_dpi: u32,
    gray: bool,
    /// Drawing a page draft (`krilla::draft`), which cannot hold images: they are skipped
    /// and `refused` set, the page being drawn again in the document.
    draft: bool,
    refused: bool,
}

impl Replayer<'_, '_> {
    /// The PDF image for `img` drawn with `transform` (image pixels to CSS px).
    /// JPEG sources are embedded as they are; other images drawn above
    /// `--image-dpi` are scaled down to it (smooth), as the 0.12.6 binary does
    /// (a 400×300 PNG shown 84pt wide becomes 116×87 with `--image-dpi 100`).
    fn image(&mut self, img: &peniko::ImageBrush, transform: Affine) -> Option<krilla::image::Image> {
        let id = img.image.data.id();
        let page = self.page;
        let jpeg = page.image_sources.get(&id).filter(|src| src.starts_with(&[0xff, 0xd8])).cloned();
        let (w, h) = (img.image.width, img.image.height);
        let (mut tw, mut th) = (w, h);
        if jpeg.is_none() && self.image_dpi > 0 {
            let pt = f64::from(page.pt_per_px);
            let shown_w = {
                let [a, b, _, _, _, _] = transform.as_coeffs();
                (a * f64::from(w)).hypot(b * f64::from(w))
            } * pt
                / 72.0;
            let shown_h = {
                let [_, _, c, d, _, _] = transform.as_coeffs();
                (c * f64::from(h)).hypot(d * f64::from(h))
            } * pt
                / 72.0;
            let dpi = f64::from(self.image_dpi);
            let (cw, ch) = ((shown_w * dpi).floor() as u32, (shown_h * dpi).floor() as u32);
            if cw < w || ch < h {
                tw = cw.clamp(1, w);
                th = ch.clamp(1, h);
            }
        }
        let gray = self.gray;
        self.caches
            .images
            .entry((id, tw, th))
            .or_insert_with(|| {
                if let Some(src) = jpeg.filter(|_| !gray) {
                    if let Ok(i) = krilla::image::Image::from_jpeg(src.to_vec().into(), false) {
                        return Some(i);
                    }
                }
                let mut data = img.image.data.data().to_vec();
                let (mut iw, mut ih) = (w, h);
                if (tw, th) != (w, h) {
                    let full = image::RgbaImage::from_raw(w, h, data)?;
                    data = image::imageops::resize(&full, tw, th, image::imageops::FilterType::Triangle)
                        .into_raw();
                    (iw, ih) = (tw, th);
                }
                if gray {
                    for px in data.chunks_exact_mut(4) {
                        let v = q_gray(px[0], px[1], px[2]);
                        px[0] = v;
                        px[1] = v;
                        px[2] = v;
                    }
                }
                Some(krilla::image::Image::from_rgba8(data, iw, ih))
            })
            .clone()
    }

    fn glyph_run(
        &mut self,
        surface: &mut Surface,
        c: &anyrender::recording::GlyphRunCommand,
        text: Option<&RunText>,
    ) {
        if c.glyphs.is_empty() {
            return;
        }
        let Some(fill) = paint_fill(&c.brush, None, c.brush_alpha, peniko::Fill::NonZero, self.gray) else {
            return;
        };
        let g0 = c.glyphs[0];
        let size = c.font_size;
        let mut t = c.transform;
        if let Some(gt) = c.glyph_transform {
            // Synthetic oblique: skew around the run origin.
            // The glyph transform is expressed in font space (y up); the run is
            // drawn y down, so mirror it (a forward slant stays forward).
            let flip = Affine::scale_non_uniform(1.0, -1.0);
            let o = kurbo::Vec2::new(f64::from(g0.x), f64::from(g0.y));
            t = t * Affine::translate(o) * flip * gt * flip * Affine::translate(-o);
        }
        let Some(text) = text else {
            // No source text: draw the outlines (list bullets).
            if let Some(p) = glyph_outlines(&c.font_data, size, &c.glyphs) {
                surface.push_transform(&tf(t));
                surface.set_stroke(None);
                surface.set_fill(Some(fill));
                surface.draw_path(&p);
                surface.pop();
            }
            return;
        };
        let Some(fe) = font_entry(self.caches, &c.font_data) else { return };
        let use_ranges = text.glyph_ranges.len() == c.glyphs.len();
        let mut fallback = String::new();
        let mut glyphs = Vec::with_capacity(c.glyphs.len());
        for (i, g) in c.glyphs.iter().enumerate() {
            let range = if use_ranges {
                text.glyph_ranges[i].clone()
            } else {
                let s = fallback.len();
                if let Some(ch) = fe.char_of(&c.font_data, g.id) {
                    fallback.push(ch);
                }
                s..fallback.len()
            };
            let adv = match c.glyphs.get(i + 1) {
                Some(n) => (n.x - g.x) / size,
                None => fe.advance(&c.font_data, g.id),
            };
            glyphs.push(KrillaGlyph::new(
                GlyphId::new(g.id),
                adv,
                0.0,
                (g.y - g0.y) / size,
                0.0,
                range,
                None,
            ));
        }
        let s: &str = if use_ranges { &text.text } else { &fallback };
        surface.set_stroke(None);
        surface.set_fill(Some(fill));
        let c6 = t.as_coeffs();
        if c6[0] == 1.0 && c6[1] == 0.0 && c6[2] == 0.0 && c6[3] == 1.0 {
            // Plain translation: position the run with the text matrix rather
            // than `cm`, as Qt does, so text extractors see real positions.
            let origin = Point::from_xy(g0.x + c6[4] as f32, g0.y + c6[5] as f32);
            surface.draw_glyphs(origin, &glyphs, fe.font.clone(), s, size, false);
        } else {
            surface.push_transform(&tf(t));
            surface.draw_glyphs(Point::from_xy(g0.x, g0.y), &glyphs, fe.font.clone(), s, size, false);
            surface.pop();
        }
    }

    /// Replay `commands` onto `surface`, restricted to the band `[y0, y1)`.
    fn replay(&mut self, surface: &mut Surface, commands: &DisplayList, y0: f32, y1: f32) {
        let (y0d, y1d) = (f64::from(y0), f64::from(y1));
        let mut layers: Vec<usize> = vec![];
        for idx in self.culling.candidates(y0d, y1d) {
            let idx = idx as usize;
            let cmd = commands.get(idx).expect("culled command");
            if let Some(b) = self.culling.bboxes[idx] {
                let is_text = matches!(cmd, Command::Other(c) if matches!(**c, RenderCommand::GlyphRun(_)));
                if b.y1 < y0d || b.y0 > y1d || (is_text && (b.y0 < y0d || b.y0 >= y1d)) {
                    continue;
                }
            }
            if let Some((rule, transform, color, rect)) = cmd.as_rect_fill() {
                // The `Fill` command `cmd.get()` expands to, without building its path.
                let Some(fill) = paint_fill(&Paint::Solid(color), None, 1.0, rule, self.gray) else {
                    continue;
                };
                let Some(p) = path_from_elements(rect.path_elements(0.1)) else { continue };
                surface.push_transform(&tf(transform));
                surface.set_fill(Some(fill));
                surface.set_stroke(None);
                surface.draw_path(&p);
                surface.pop();
                continue;
            }
            match &*cmd.get() {
                RenderCommand::PushLayer(c) => {
                    let mut n = 0;
                    if c.alpha < 1.0 {
                        surface.push_opacity(norm(c.alpha));
                        n += 1;
                    }
                    if let Some(p) = to_path(&c.clip).and_then(|p| p.transform(tf(c.transform))) {
                        surface.push_clip_path(&p, &FillRule::NonZero);
                        n += 1;
                    }
                    layers.push(n);
                }
                RenderCommand::PushClipLayer(c) => {
                    if let Some(p) = to_path(&c.clip).and_then(|p| p.transform(tf(c.transform))) {
                        surface.push_clip_path(&p, &FillRule::NonZero);
                        layers.push(1);
                    } else {
                        layers.push(0);
                    }
                }
                RenderCommand::PopLayer => {
                    for _ in 0..layers.pop().unwrap_or(0) {
                        surface.pop();
                    }
                }
                RenderCommand::Fill(c) => match &c.brush {
                    Paint::Image(_) if self.draft => self.refused = true,
                    Paint::Image(img) => {
                        let to_px = c.transform * c.brush_transform.unwrap_or(Affine::IDENTITY);
                        let Some(kimg) = self.image(img, to_px) else { continue };
                        let Some(clip) = to_path(&c.shape) else { continue };
                        let Some(size) = Size::from_wh(img.image.width as f32, img.image.height as f32)
                        else {
                            continue;
                        };
                        surface.push_transform(&tf(c.transform));
                        surface.push_clip_path(&clip, &FillRule::NonZero);
                        surface.push_transform(&tf(c.brush_transform.unwrap_or(Affine::IDENTITY)));
                        surface.draw_image(kimg, size);
                        surface.pop();
                        surface.pop();
                        surface.pop();
                    }
                    brush => {
                        let Some(fill) = paint_fill(brush, c.brush_transform, 1.0, c.fill, self.gray) else {
                            continue;
                        };
                        let Some(p) = to_path(&c.shape) else { continue };
                        surface.push_transform(&tf(c.transform));
                        surface.set_fill(Some(fill));
                        surface.set_stroke(None);
                        surface.draw_path(&p);
                        surface.pop();
                    }
                },
                RenderCommand::Stroke(c) => {
                    let Some(fill) =
                        paint_fill(&c.brush, c.brush_transform, 1.0, peniko::Fill::NonZero, self.gray)
                    else {
                        continue;
                    };
                    let Some(p) = to_path(&c.shape) else { continue };
                    surface.push_transform(&tf(c.transform));
                    surface.set_fill(None);
                    surface.set_stroke(Some(Stroke {
                        paint: fill.paint,
                        width: c.style.width as f32,
                        opacity: fill.opacity,
                        dash: if c.style.dash_pattern.is_empty() {
                            None
                        } else {
                            Some(krilla::paint::StrokeDash {
                                array: c.style.dash_pattern.iter().map(|d| *d as f32).collect(),
                                offset: c.style.dash_offset as f32,
                            })
                        },
                        ..Default::default()
                    }));
                    surface.draw_path(&p);
                    surface.set_stroke(None);
                    surface.pop();
                }
                RenderCommand::GlyphRun(c) => {
                    let text = self.page.recorder.texts.get(&idx);
                    self.glyph_run(surface, c, text);
                }
                RenderCommand::BoxShadow(_) => {}
            }
        }
        for n in layers.into_iter().rev() {
            for _ in 0..n {
                surface.pop();
            }
        }
    }
}

/// Civil date from days since the Unix epoch (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn date_time(secs: u64) -> DateTime {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil(days);
    DateTime::new(y.clamp(0, 9999) as u16)
        .month(m as u8)
        .day(d as u8)
        .hour((rem / 3600) as u8)
        .minute((rem % 3600 / 60) as u8)
        .second((rem % 60) as u8)
        .utc_offset_hour(0)
        .utc_offset_minute(0)
}

fn outline_node(n: &OutlineNode, named: &BTreeMap<String, XyzDestination>) -> krilla::outline::OutlineNode {
    let dest = named
        .get(&n.anchor)
        .cloned()
        .unwrap_or_else(|| XyzDestination::new(n.page, Point::from_xy(n.x, n.y)));
    let mut k = krilla::outline::OutlineNode::new(n.title.clone(), dest);
    for c in &n.children {
        k.push_child(outline_node(c, named));
    }
    k
}

/// Draw one band: its document range (plus repeated table groups) placed at
/// `band.origin`, clipped horizontally to the paper. Returns `false` when drawing a draft
/// that cannot hold the band (an image).
fn draw_band(
    surface: &mut Surface,
    band: &Band<'_>,
    paper_w: f32,
    caches: &mut Caches,
    opts: PdfOptions,
    draft: bool,
) -> bool {
    let s = band.page.pt_per_px;
    let (ox, oy) = band.origin;
    let (cx0, cx1) = (-ox / s, (paper_w - ox) / s);
    surface.push_transform(&Transform::from_row(s, 0.0, 0.0, s, ox, oy));
    let commands = &band.page.recorder.commands;
    let culling = caches.culling(commands);
    let mut r = Replayer {
        page: band.page,
        culling: culling.get().expect("culling computed"),
        caches,
        gray: opts.grayscale,
        image_dpi: opts.image_dpi,
        draft,
        refused: false,
    };
    let p = band.slice;
    let mut dy = 0.0;
    for b in [p.thead, Some((p.y0, p.y1)), p.tfoot] {
        let Some((b0, b1)) = b else { continue };
        surface.push_transform(&Transform::from_translate(0.0, dy - b0));
        if let Some(clip) = rect_path(cx0, b0, cx1, b1) {
            surface.push_clip_path(&clip, &FillRule::NonZero);
            r.replay(surface, commands, b0, b1);
            surface.pop();
        }
        surface.pop();
        dy += b1 - b0;
    }
    surface.pop();
    !r.refused
}

/// Draw everything of `p` on its page's surface. Returns `false` when drawing a draft that
/// cannot hold the page.
fn draw_page(
    surface: &mut Surface,
    p: &OutPage<'_>,
    width: f32,
    caches: &mut Caches,
    opts: PdfOptions,
    draft: bool,
) -> bool {
    let mut ok = true;
    for band in &p.bands {
        ok &= draw_band(surface, band, width, caches, opts, draft);
    }
    for l in &p.lines {
        let h = l.width / 2.0;
        let mut b = PathBuilder::new();
        if let Some(rect) = Rect::from_xywh(l.x0 - h, l.y - h, l.x1 - l.x0 + l.width, l.width) {
            b.push_rect(rect);
        }
        if let Some(path) = b.finish() {
            surface.set_stroke(None);
            surface.set_fill(Some(Fill { paint: rgb::Color::new(0, 0, 0).into(), ..Default::default() }));
            surface.draw_path(&path);
        }
    }
    for t in &p.texts {
        if t.text.is_empty() {
            continue;
        }
        let key = (Arc::as_ptr(&t.face.data) as usize, t.face.index);
        let font = caches
            .shared
            .text_fonts
            .lock()
            .unwrap()
            .entry(key)
            .or_insert_with(|| Font::new(t.face.data.clone().into(), t.face.index))
            .clone();
        let Some(font) = font else { continue };
        surface.set_stroke(None);
        surface.set_fill(Some(Fill {
            paint: rgb::Color::new(0, 0, 0).into(),
            opacity: NormalizedF32::ONE,
            rule: FillRule::NonZero,
        }));
        surface.draw_text(
            Point::from_xy(t.x, t.baseline),
            font,
            t.size,
            &t.text,
            false,
            krilla::text::TextDirection::Auto,
        );
    }
    ok
}

/// Pages drafted on the rayon pool (`krilla::draft`) while the document takes them in order.
/// At most `window` pages are drafted ahead of the one the document waits for, to bound the
/// memory they hold. A page nobody has started drafting when the document reaches it is
/// drawn in the document directly.
struct Drafts {
    /// Per page: 0 not started, 1 taken by a thread, 2 taken by the document.
    state: Vec<AtomicU8>,
    done: Mutex<Vec<Option<Option<krilla::draft::PageDraft>>>>,
    ready: Condvar,
}

impl Drafts {
    fn new(n: usize) -> Self {
        Drafts {
            state: (0..n).map(|_| AtomicU8::new(0)).collect(),
            done: Mutex::new((0..n).map(|_| None).collect()),
            ready: Condvar::new(),
        }
    }

    /// The draft of page `i` (made by a pool thread), or `None` when the document must draw
    /// it: nobody had started it, or the draft cannot be used.
    fn take(&self, i: usize) -> Option<krilla::draft::PageDraft> {
        if self.state[i].compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            return None;
        }
        let mut done = self.done.lock().unwrap();
        loop {
            if let Some(d) = done[i].take() {
                return d;
            }
            done = self.ready.wait(done).unwrap();
        }
    }
}

/// Threads for page drafts: the rayon pool, when it has more than one thread.
fn draft_threads(pages: usize) -> usize {
    let threads = rayon::current_num_threads();
    if threads > 1 && pages > 1 { threads } else { 0 }
}

/// Write the PDF: `pages` in order on `width` × `height` pt paper.
/// `outline_mode` adds `/PageMode /UseOutlines` to the catalog, like Qt
/// does whenever the outline is enabled.
///
/// With a rayon pool of several threads, pages are drafted on it while the document takes
/// them in order (`krilla::draft`); the output is the same as drawing every page here.
pub fn write_pdf(
    width: f32,
    height: f32,
    pages: &[OutPage<'_>],
    outline: &[OutlineNode],
    outline_mode: bool,
    info: &DocInfo,
    opts: PdfOptions,
) -> Result<Vec<u8>, String> {
    let configuration =
        ConfigurationBuilder::new().with_version(PdfVersion::Pdf14).finish().map_err(|e| format!("{e:?}"))?;
    let settings =
        SerializeSettings { xmp_metadata: false, enable_tagging: false, configuration, ..Default::default() };
    let mut doc = Document::new_with(settings);
    let shared = Shared::default();
    let page_settings = PageSettings::from_wh(width, height).ok_or("invalid page size")?;
    // Named destinations: like Qt's anchor table, the last definition of a
    // name wins (with copies, the last copy). Ordered, so destinations are
    // registered (and numbered) in the same order on every run.
    let mut named: BTreeMap<String, XyzDestination> = BTreeMap::new();
    for (i, p) in pages.iter().enumerate() {
        for a in &p.anchors {
            named.insert(a.name.clone(), XyzDestination::new(i, Point::from_xy(a.x, a.y)));
        }
    }
    let threads = draft_threads(pages.len());
    let drafts = Drafts::new(if threads > 0 { pages.len() } else { 0 });
    let draft_settings = doc.draft_settings();
    let mut drafted = 0;
    rayon::in_place_scope(|scope| {
        let drafts = &drafts;
        let (shared, draft_settings, page_settings) = (&shared, &draft_settings, &page_settings);
        let spawn = |i: usize| {
            scope.spawn(move |_| {
                if drafts.state[i].compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_err() {
                    return;
                }
                let mut caches = Caches::new(shared);
                let mut usable = true;
                let draft = krilla::draft::draft_page(draft_settings, page_settings, |surface| {
                    usable = draw_page(surface, &pages[i], width, &mut caches, opts, true);
                });
                let draft = (usable && draft.is_usable()).then_some(draft);
                drafts.done.lock().unwrap()[i] = Some(draft);
                drafts.ready.notify_all();
            });
        };
        let window = threads * 2;
        if threads > 0 {
            for i in 0..window.min(pages.len()) {
                spawn(i);
            }
        }
        let mut caches = Caches::new(shared);
        for (i, p) in pages.iter().enumerate() {
            if threads > 0 && i + window < pages.len() {
                spawn(i + window);
            }
            let mut page = doc.start_page_with(page_settings.clone());
            for l in &p.links {
                let Some(rect) = Rect::from_ltrb(l.rect.0, l.rect.1, l.rect.2, l.rect.3) else { continue };
                let target = match &l.target {
                    LinkTarget::Uri(u) => {
                        krilla::annotation::Target::Action(krilla::action::LinkAction::new(u.clone()).into())
                    }
                    LinkTarget::Named(n) => {
                        let Some(xyz) = named.get(n) else { continue };
                        krilla::annotation::Target::Destination(
                            krilla::destination::NamedDestination::new(n.clone(), xyz.clone()).into(),
                        )
                    }
                };
                page.add_annotation(krilla::annotation::Annotation::new_link(
                    krilla::annotation::LinkAnnotation::new(rect, target),
                    None,
                ));
            }
            let applied = match (threads > 0).then(|| drafts.take(i)).flatten() {
                Some(draft) => page.apply_draft(draft).is_ok(),
                None => false,
            };
            drafted += usize::from(applied);
            if !applied {
                let mut surface = page.surface();
                draw_page(&mut surface, p, width, &mut caches, opts, false);
                surface.finish();
            }
            page.finish();
        }
    });
    wk_webkit::timing(&format!("pages drawn from drafts: {drafted} of {}", pages.len()));
    for (name, xyz) in &named {
        let _ = doc.register_named_destination(krilla::destination::NamedDestination::new(
            name.clone(),
            xyz.clone(),
        ));
    }
    if !outline.is_empty() {
        let mut o = krilla::outline::Outline::new();
        for n in outline {
            o.push_child(outline_node(n, &named));
        }
        doc.set_outline(o);
    }
    let mut meta = Metadata::new().creator(info.creator.clone()).producer(info.producer.clone());
    if let Some(t) = &info.title {
        meta = meta.title(t.clone());
    }
    if let Some(secs) = info.creation_time {
        meta = meta.creation_date(date_time(secs));
    }
    doc.set_metadata(meta);
    let pdf = doc.finish().map_err(|e| format!("{e:?}"))?;
    let fields: Vec<&[FormField]> = pages.iter().map(|p| p.fields.as_slice()).collect();
    if outline_mode || fields.iter().any(|f| !f.is_empty()) {
        crate::acroform::update(pdf, height, outline_mode, &fields)
    } else {
        Ok(pdf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_740), (2026, 10, 14));
    }
}
