//! Route (a): delegate everything to fulgur 0.40.
//!
//! wkhtmltopdf prints 1 CSS px as 0.6 pt (layout 1.25x wider than the printable area), fulgur
//! as 0.75 pt. To compare like for like, fulgur renders onto a page 1.25x larger and the PDF is
//! then scaled back down by 0.8 with lopdf (MediaBox + a `cm` prepended to each page).
use crate::args::Args;
use fulgur::{AssetBundle, Engine, Margin, PageSize};

const K: f32 = 1.25;

pub fn render(a: &Args, html: &str) -> Vec<u8> {
    let mut assets = AssetBundle::new();
    if let Ok(dir) = std::env::var("SPIKE_FONT_DIR") {
        let mut fonts: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
        fonts.sort();
        for f in fonts.iter().filter(|p| p.extension().is_some_and(|e| e == "ttf")) {
            assets.add_font_file(f).unwrap();
        }
    }
    let engine = Engine::builder()
        .page_size(PageSize { width: a.page_w * K, height: a.page_h * K })
        .margin(Margin { top: a.margin[0] * K, right: a.margin[1] * K, bottom: a.margin[2] * K, left: a.margin[3] * K })
        .base_path(a.base_dir())
        .system_fonts(std::env::var("SPIKE_FONT_DIR").is_err())
        .assets(assets)
        .build();
    let pdf = engine.render(html).expect("fulgur render failed");
    rescale(&pdf, a.page_w, a.page_h)
}

fn rescale(pdf: &[u8], w: f32, h: f32) -> Vec<u8> {
    use lopdf::{Document, Object};
    let mut doc = Document::load_mem(pdf).unwrap();
    let s = 1.0 / K;
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    for id in pages {
        let mut content = format!("q {s} 0 0 {s} 0 0 cm\n").into_bytes();
        content.extend(doc.get_page_content(id).unwrap());
        content.extend(b"\nQ\n");
        doc.change_page_content(id, content).unwrap();
        let page = doc.get_object_mut(id).unwrap().as_dict_mut().unwrap();
        page.set("MediaBox", vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]);
        page.remove(b"CropBox");
    }
    let mut out = vec![];
    doc.save_to(&mut out).unwrap();
    out
}
