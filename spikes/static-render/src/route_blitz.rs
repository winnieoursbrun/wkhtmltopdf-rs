//! Route (b) orchestration: parse + style + layout with blitz-dom 0.3, paint the whole document
//! once with blitz-paint into a recorded anyrender scene, cut it into pages with our own
//! paginator, and replay each page slice into krilla.
use crate::args::Args;
use crate::paginate::{self, Geometry};
use crate::pdf_painter;
use blitz_dom::DocumentConfig;
use blitz_html::HtmlDocument;
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use parley::fontique::{Blob, Collection, CollectionOptions, GenericFamily, SourceCache};
use parley::FontContext;
use std::sync::Arc;
use std::time::Instant;

/// wkhtmltopdf/QtWebKit lays the page out 1.25x wider than the printable area (WebKit's minimum
/// printing shrink factor) and prints CSS px at 96 dpi: 1 CSS px = 0.75 / 1.25 = 0.6 pt.
pub const PT_PER_PX: f32 = 0.75 / 1.25;

/// Synchronous provider for `data:` and `file:` URLs (no network in the spike).
struct SyncNet;
impl NetProvider for SyncNet {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.clone();
        let bytes = match url.scheme() {
            "data" => data_url::DataUrl::process(url.as_str())
                .ok()
                .and_then(|d| d.decode_to_vec().ok())
                .map(|(v, _)| v),
            "file" => url.to_file_path().ok().and_then(|p| std::fs::read(p).ok()),
            _ => None,
        };
        match bytes {
            Some(b) => handler.bytes(url.to_string(), Bytes::from(b)),
            None => eprintln!("spike: cannot load {}", &url.as_str()[..url.as_str().len().min(80)]),
        }
    }
}

/// Font set equivalent to the reference container: DejaVu only, generic families mapped to it
/// (fontconfig in `wkref:local` resolves the default family to DejaVu Sans).
fn font_context(font_dir: &std::path::Path) -> FontContext {
    let mut collection = Collection::new(CollectionOptions { system_fonts: false, ..Default::default() });
    let mut entries: Vec<_> = std::fs::read_dir(font_dir).map(|r| r.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if matches!(p.extension().and_then(|s| s.to_str()), Some("ttf" | "otf")) {
            let data = std::fs::read(&p).unwrap();
            collection.register_fonts(Blob::new(Arc::new(data)), None);
        }
    }
    let sans = collection.family_id("DejaVu Sans");
    let mono = collection.family_id("DejaVu Sans Mono");
    if let Some(sans) = sans {
        for g in [GenericFamily::Serif, GenericFamily::SansSerif, GenericFamily::SystemUi, GenericFamily::UiSansSerif] {
            collection.set_generic_families(g, [sans].into_iter());
        }
    }
    if let Some(mono) = mono {
        collection.set_generic_families(GenericFamily::Monospace, [mono].into_iter());
    }
    FontContext { collection, source_cache: SourceCache::new(Default::default()) }
}

/// Paged-media properties are Gecko-only in Stylo (servo mode drops `page-break-*` and
/// `break-*` at parse time). The spike smuggles them through as custom properties.
fn smuggle_break_properties(html: &str) -> String {
    html.replace("page-break-before", "--wk-break-before")
        .replace("page-break-after", "--wk-break-after")
        .replace("page-break-inside", "--wk-break-inside")
}

pub fn render(a: &Args, html: &str) -> Vec<u8> {
    let t0 = Instant::now();
    let content_w_pt = a.page_w - a.margin[1] - a.margin[3];
    let content_h_pt = a.page_h - a.margin[0] - a.margin[2];
    let vw = (content_w_pt / PT_PER_PX).round() as u32;
    let vh = (content_h_pt / PT_PER_PX).round() as u32;

    let font_dir = std::env::var("SPIKE_FONT_DIR").map(Into::into).unwrap_or_else(|_| a.base_dir().join("fonts"));
    let base_url = url::Url::from_directory_path(a.base_dir()).unwrap().to_string();
    let config = DocumentConfig {
        viewport: Some(Viewport::new(vw, vh, 1.0, ColorScheme::Light)),
        base_url: Some(base_url),
        net_provider: Some(Arc::new(SyncNet)),
        font_ctx: Some(font_context(&font_dir)),
        ..Default::default()
    };
    let html = smuggle_break_properties(html);
    let mut doc = HtmlDocument::from_html(&html, config).into_inner();
    // Resources are delivered synchronously into the document channel; resolve until settled.
    for _ in 0..8 {
        doc.resolve(0.0);
        if !doc.has_pending_critical_resources() {
            break;
        }
    }
    doc.resolve(0.0);
    let t_layout = t0.elapsed();

    let geo = paginate::collect_geometry(&doc);
    let pages = paginate::paginate(&geo, vh as f32);
    let doc_h = geo.doc_height.ceil().max(1.0) as u32;

    let mut scene = anyrender::Scene::new();
    blitz_paint::paint_scene(&mut scene, &mut doc, 1.0, vw, doc_h, 0, 0);
    let t_paint = t0.elapsed();

    let title = doc.find_title_node().map(|n| n.text_content());
    let pdf = pdf_painter::write_pdf(a, &scene, &pages, &geo, vw as f32, title);
    if std::env::var("SPIKE_DEBUG").is_ok() {
        dump_debug(&geo, &pages, vw, vh);
        eprintln!(
            "spike: layout {:?} paint {:?} total {:?}; {} scene commands, {} pages",
            t_layout, t_paint, t0.elapsed(), scene.commands.len(), pages.len()
        );
    }
    pdf
}

fn dump_debug(geo: &Geometry, pages: &[paginate::PageSlice], vw: u32, vh: u32) {
    eprintln!("viewport {vw}x{vh}px, doc height {}", geo.doc_height);
    eprintln!("forced breaks: {:?}", geo.forced);
    eprintln!("candidates {} atomic {}: {:?}", geo.candidates.len(), geo.atomic.len(), geo.atomic.iter().filter(|a| a.1 > 1080.0 && a.0 < 1180.0).collect::<Vec<_>>());
    for t in &geo.tables {
        eprintln!("table {:?}", t);
    }
    for (i, p) in pages.iter().enumerate() {
        eprintln!("page {}: {:?}", i + 1, p);
    }
}
