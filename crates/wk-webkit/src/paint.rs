//! Display list recording. Stands in for WebCore's `GraphicsContext` on a
//! `QPainter` during `QWebFrame::print`: the whole document is painted once
//! into an `anyrender` scene (blitz-paint), and every glyph run keeps the
//! source text published by the vendored blitz-paint
//! (`vendor/blitz-paint/src/run_text.rs`), so the PDF gets real text.

use std::collections::HashMap;
use std::sync::Arc;

use anyrender::recording::RenderCommand;
use anyrender::{Glyph, NormalizedCoord, PaintRef, PaintScene, RenderContext, Scene};
use blitz_paint::run_text::RunText;
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

/// A painted document.
#[derive(Default)]
pub struct Recorder {
    pub scene: Scene,
    /// Source text of glyph-run commands, by command index.
    pub texts: HashMap<usize, RunText>,
}

impl RenderContext for Recorder {}

impl PaintScene for Recorder {
    fn reset(&mut self) {
        self.scene.reset();
        self.texts.clear();
    }

    fn push_layer(
        &mut self,
        fill: Fill,
        blend: impl Into<BlendMode>,
        alpha: f32,
        transform: Affine,
        clip: &impl Shape,
        filter: Option<Arc<anyrender::Filter>>,
        backdrop_filter: Option<Arc<anyrender::Filter>>,
    ) {
        self.scene.push_layer(fill, blend, alpha, transform, clip, filter, backdrop_filter);
    }

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape) {
        self.scene.push_clip_layer(fill, transform, clip);
    }

    fn pop_layer(&mut self) {
        self.scene.pop_layer();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.scene.stroke(style, transform, brush, brush_transform, shape);
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.scene.fill(style, transform, brush, brush_transform, shape);
    }

    fn draw_glyphs<'a, 's: 'a>(
        &'s mut self,
        font: &'a FontData,
        font_size: f32,
        hint: bool,
        normalized_coords: &'a [NormalizedCoord],
        embolden: kurbo::Vec2,
        style: impl Into<StyleRef<'a>>,
        brush: impl Into<PaintRef<'a>>,
        brush_alpha: f32,
        transform: Affine,
        glyph_transform: Option<Affine>,
        glyphs: impl Iterator<Item = Glyph> + Clone,
    ) {
        let index = self.scene.commands.len();
        if let Some(t) = blitz_paint::run_text::take() {
            self.texts.insert(index, t);
        }
        self.scene.draw_glyphs(
            font,
            font_size,
            hint,
            normalized_coords,
            embolden,
            style,
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs,
        );
    }

    fn draw_box_shadow(&mut self, transform: Affine, rect: Rect, brush: Color, radius: f64, std_dev: f64) {
        self.scene.draw_box_shadow(transform, rect, brush, radius, std_dev);
    }
}

/// Bounding box of a glyph run in document coordinates: from the first
/// glyph to the end of the last one, ascent to descent approximated from the
/// font size.
pub fn glyph_run_bbox(cmd: &RenderCommand) -> Option<Rect> {
    let RenderCommand::GlyphRun(c) = cmd else { return None };
    let first = c.glyphs.first()?;
    let last = c.glyphs.last()?;
    let fs = f64::from(c.font_size);
    let x0 = f64::from(first.x);
    let x1 = f64::from(last.x) + fs * 0.6;
    let base = f64::from(first.y);
    let r = Rect::new(x0, base - fs * 0.93, x1, base + fs * 0.24);
    Some(c.transform.transform_rect_bbox(r))
}
