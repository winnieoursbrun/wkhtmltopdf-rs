//! Replays a recorded anyrender scene (from blitz-paint) into krilla, one page slice at a time.
//!
//! Spike limitations, on purpose:
//! - gradients are painted with their first stop colour, box shadows and filters are dropped;
//! - anyrender glyph runs carry no source text, so the text layer is rebuilt by reverse cmap
//!   lookup (ligatures and multi-codepoint clusters lose their text).
use crate::args::Args;
use crate::paginate::{Geometry, PageSlice};
use crate::route_blitz::PT_PER_PX;
use anyrender::recording::RenderCommand;
use anyrender::{Paint, Scene};
use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Point, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, Stroke};
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::Document;
use kurbo::{Affine, BezPath, PathEl, Shape};
use peniko::Color;
use skrifa::MetadataProvider;
use std::collections::HashMap;
use std::sync::Arc;

struct FontEntry {
    font: Font,
    rev_cmap: HashMap<u32, char>,
    hmtx: Vec<f32>, // advance / upem per glyph id
}

#[derive(Default)]
struct Caches {
    fonts: HashMap<(u64, u32), Option<FontEntry>>,
    images: HashMap<u64, krilla::image::Image>,
}

fn tf(a: Affine) -> Transform {
    let c = a.as_coeffs();
    Transform::from_row(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32)
}

fn to_path(p: &BezPath) -> Option<Path> {
    let mut b = PathBuilder::new();
    for el in p.elements() {
        match *el {
            PathEl::MoveTo(a) => b.move_to(a.x as f32, a.y as f32),
            PathEl::LineTo(a) => b.line_to(a.x as f32, a.y as f32),
            PathEl::QuadTo(a, c) => b.quad_to(a.x as f32, a.y as f32, c.x as f32, c.y as f32),
            PathEl::CurveTo(a, c, d) => b.cubic_to(a.x as f32, a.y as f32, c.x as f32, c.y as f32, d.x as f32, d.y as f32),
            PathEl::ClosePath => b.close(),
        }
    }
    b.finish()
}

fn rect_path(x0: f32, y0: f32, x1: f32, y1: f32) -> Path {
    to_path(&kurbo::Rect::new(x0 as f64, y0 as f64, x1 as f64, y1 as f64).to_path(0.1)).unwrap()
}

fn color_of(p: &Paint) -> Option<Color> {
    match p {
        Paint::Solid(c) => Some(*c),
        Paint::Gradient(g) => g.stops.first().map(|s| s.color.to_alpha_color()),
        _ => None,
    }
}

fn krilla_fill(c: Color, alpha: f32, rule: peniko::Fill) -> Fill {
    let rgba = c.to_rgba8();
    Fill {
        paint: rgb::Color::new(rgba.r, rgba.g, rgba.b).into(),
        opacity: NormalizedF32::new((rgba.a as f32 / 255.0 * alpha).clamp(0.0, 1.0)).unwrap(),
        rule: if rule == peniko::Fill::EvenOdd { FillRule::EvenOdd } else { FillRule::NonZero },
    }
}

fn font_entry<'a>(caches: &'a mut Caches, fd: &peniko::FontData) -> Option<&'a FontEntry> {
    caches
        .fonts
        .entry((fd.data.id(), fd.index))
        .or_insert_with(|| {
            let bytes: Vec<u8> = fd.data.data().to_vec();
            let fref = skrifa::FontRef::from_index(&bytes, fd.index).ok()?;
            let mut rev_cmap = HashMap::new();
            for (cp, gid) in fref.charmap().mappings() {
                if let Some(ch) = char::from_u32(cp) {
                    let e = rev_cmap.entry(gid.to_u32()).or_insert(ch);
                    // Prefer the plain (lowest) code point for glyphs mapped several times.
                    if (ch as u32) < (*e as u32) {
                        *e = ch;
                    }
                }
            }
            let fm = fref.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
            let upem = fm.units_per_em as f32;
            let gm = fref.glyph_metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
            let n = fm.glyph_count as u32;
            let hmtx = (0..n).map(|g| gm.advance_width(skrifa::GlyphId::new(g)).unwrap_or(0.0) / upem).collect();
            let font = Font::new(Arc::new(bytes).into(), fd.index)?;
            Some(FontEntry { font, rev_cmap, hmtx })
        })
        .as_ref()
}

/// Bounding box of a command in scene coordinates, for page culling.
fn bbox(cmd: &RenderCommand) -> Option<kurbo::Rect> {
    match cmd {
        RenderCommand::Fill(c) => Some(c.transform.transform_rect_bbox(c.shape.bounding_box())),
        RenderCommand::Stroke(c) => Some(c.transform.transform_rect_bbox(c.shape.bounding_box()).inflate(c.style.width, c.style.width)),
        RenderCommand::GlyphRun(c) => {
            // Cull text by baseline: a line belongs to exactly one page, otherwise clipped
            // glyphs would still be extractable text on the neighbouring page.
            let g = c.glyphs.first()?;
            let p = c.transform * kurbo::Point::new(g.x as f64, g.y as f64);
            Some(kurbo::Rect::new(p.x, p.y, p.x, p.y))
        }
        RenderCommand::BoxShadow(c) => Some(c.transform.transform_rect_bbox(c.rect)),
        _ => None,
    }
}

/// Replays `scene` onto `surface`, restricted to the document band [y0, y1).
fn replay(surface: &mut krilla::surface::Surface, scene: &Scene, caches: &mut Caches, y0: f32, y1: f32) {
    // Number of krilla pushes per open anyrender layer.
    let mut layers: Vec<usize> = vec![];
    for cmd in &scene.commands {
        if let Some(b) = bbox(cmd) {
            if b.y1 < y0 as f64 || b.y0 > y1 as f64 || (matches!(cmd, RenderCommand::GlyphRun(_)) && (b.y0 < y0 as f64 || b.y0 >= y1 as f64)) {
                continue;
            }
        }
        match cmd {
            RenderCommand::PushLayer(c) => {
                let mut n = 0;
                if c.alpha < 1.0 {
                    surface.push_opacity(NormalizedF32::new(c.alpha.clamp(0.0, 1.0)).unwrap());
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
                Paint::Image(img) => {
                    let id = img.image.data.id();
                    let kimg = caches
                        .images
                        .entry(id)
                        .or_insert_with(|| {
                            krilla::image::Image::from_rgba8(img.image.data.data().to_vec(), img.image.width, img.image.height)
                        })
                        .clone();
                    let Some(clip) = to_path(&c.shape) else { continue };
                    surface.push_transform(&tf(c.transform));
                    surface.push_clip_path(&clip, &FillRule::NonZero);
                    surface.push_transform(&tf(c.brush_transform.unwrap_or(Affine::IDENTITY)));
                    surface.draw_image(kimg, Size::from_wh(img.image.width as f32, img.image.height as f32).unwrap());
                    surface.pop();
                    surface.pop();
                    surface.pop();
                }
                brush => {
                    let Some(col) = color_of(brush) else { continue };
                    let Some(p) = to_path(&c.shape) else { continue };
                    surface.push_transform(&tf(c.transform));
                    surface.set_fill(Some(krilla_fill(col, 1.0, c.fill)));
                    surface.set_stroke(None);
                    surface.draw_path(&p);
                    surface.pop();
                }
            },
            RenderCommand::Stroke(c) => {
                let Some(col) = color_of(&c.brush) else { continue };
                let Some(p) = to_path(&c.shape) else { continue };
                let rgba = col.to_rgba8();
                surface.push_transform(&tf(c.transform));
                surface.set_fill(None);
                surface.set_stroke(Some(Stroke {
                    paint: rgb::Color::new(rgba.r, rgba.g, rgba.b).into(),
                    width: c.style.width as f32,
                    opacity: NormalizedF32::new(rgba.a as f32 / 255.0).unwrap(),
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
                if c.glyphs.is_empty() {
                    continue;
                }
                let Some(col) = color_of(&c.brush) else { continue };
                let Some(fe) = font_entry(caches, &c.font_data) else { continue };
                let size = c.font_size;
                let mut text = String::new();
                let mut glyphs = Vec::with_capacity(c.glyphs.len());
                let g0 = c.glyphs[0];
                for (i, g) in c.glyphs.iter().enumerate() {
                    let start = text.len();
                    if let Some(ch) = fe.rev_cmap.get(&g.id) {
                        text.push(*ch);
                    }
                    let adv = match c.glyphs.get(i + 1) {
                        Some(n) => (n.x - g.x) / size,
                        None => fe.hmtx.get(g.id as usize).copied().unwrap_or(0.0),
                    };
                    let _ = start;
                    glyphs.push((g.id, adv, (g.y - g0.y) / size, start..text.len()));
                }
                let kg: Vec<KrillaGlyph> = glyphs
                    .into_iter()
                    .map(|(id, adv, dy, r)| KrillaGlyph::new(GlyphId::new(id), adv, 0.0, dy, 0.0, r, None))
                    .collect();
                let mut t = c.transform;
                if let Some(gt) = c.glyph_transform {
                    // Synthetic oblique: skew around the run origin.
                    let o = kurbo::Vec2::new(g0.x as f64, g0.y as f64);
                    t = t * Affine::translate(o) * gt * Affine::translate(-o);
                }
                surface.push_transform(&tf(t));
                surface.set_stroke(None);
                surface.set_fill(Some(krilla_fill(col, c.brush_alpha, peniko::Fill::NonZero)));
                surface.draw_glyphs(Point::from_xy(g0.x, g0.y), &kg, fe.font.clone(), &text, size, false);
                surface.pop();
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

pub fn write_pdf(a: &Args, scene: &Scene, pages: &[PageSlice], _geo: &Geometry, vw: f32, title: Option<String>) -> Vec<u8> {
    let mut doc = Document::new();
    let mut caches = Caches::default();
    let s = PT_PER_PX;
    let (ml, mt) = (a.margin[3], a.margin[0]);
    // Horizontal clip: everything that fits on the paper.
    let (cx0, cx1) = (-ml / s, (a.page_w - ml) / s);
    let _ = vw;
    for p in pages {
        let mut page = doc.start_page_with(PageSettings::from_wh(a.page_w, a.page_h).unwrap());
        let mut surface = page.surface();
        surface.push_transform(&Transform::from_row(s, 0.0, 0.0, s, ml, mt));
        let mut dy = 0.0;
        if let Some((h0, h1)) = p.thead {
            surface.push_transform(&Transform::from_translate(0.0, -h0));
            surface.push_clip_path(&rect_path(cx0, h0, cx1, h1), &FillRule::NonZero);
            replay(&mut surface, scene, &mut caches, h0, h1);
            surface.pop();
            surface.pop();
            dy = h1 - h0;
        }
        surface.push_transform(&Transform::from_translate(0.0, dy - p.y0));
        surface.push_clip_path(&rect_path(cx0, p.y0, cx1, p.y1), &FillRule::NonZero);
        replay(&mut surface, scene, &mut caches, p.y0, p.y1);
        surface.pop();
        surface.pop();
        surface.pop();
        surface.finish();
        page.finish();
    }
    if let Some(t) = title {
        doc.set_metadata(krilla::metadata::Metadata::new().title(t).creator("spike (blitz + krilla)".into()));
    }
    doc.finish().expect("krilla finish")
}
