//! Display list recording. Stands in for WebCore's `GraphicsContext` on a
//! `QPainter` during `QWebFrame::print`: the whole document is painted once
//! into a display list of `anyrender` commands (blitz-paint), and every glyph
//! run keeps the source text published by the vendored blitz-paint
//! (`vendor/blitz-paint/src/run_text.rs`), so the PDF gets real text.
//!
//! Memory: an `anyrender::recording::RenderCommand` is 464 bytes, plus a
//! heap path for its shape. Table-heavy documents paint tens of thousands of
//! solid rectangles (cell backgrounds, collapsed borders), so those are kept
//! as [`RectFill`] (no path, ~160 bytes) and expanded on demand into the very
//! command `anyrender::Scene::fill` would have recorded; every other command
//! is recorded by an `anyrender::Scene` and boxed.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use anyrender::recording::{FillCommand, GlyphRunCommand, RenderCommand};
use anyrender::{Glyph, NormalizedCoord, Paint, PaintRef, PaintScene, RenderContext, Scene};
use blitz_paint::run_text::RunText;
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

/// A solid rectangle fill (`Scene::fill` with a `Rect` and a solid colour).
#[derive(Debug, Clone)]
pub struct RectFill {
    fill: Fill,
    transform: Affine,
    color: Color,
    brush_transform: Option<Affine>,
    rect: Rect,
}

/// One recorded command.
#[derive(Debug, Clone)]
pub enum Command {
    RectFill(RectFill),
    Other(Box<RenderCommand>),
}

impl Command {
    /// The command as `anyrender::Scene` records it.
    pub fn get(&self) -> Cow<'_, RenderCommand> {
        match self {
            Command::Other(c) => Cow::Borrowed(c),
            Command::RectFill(r) => Cow::Owned(RenderCommand::Fill(FillCommand {
                fill: r.fill,
                transform: r.transform,
                brush: Paint::Solid(r.color),
                brush_transform: r.brush_transform,
                shape: r.rect.into_path(Scene::default().tolerance),
            })),
        }
    }
}

/// A painted document.
#[derive(Default)]
pub struct Recorder {
    /// The display list, in painting order.
    pub commands: Vec<Command>,
    /// Source text of glyph-run commands, by command index.
    pub texts: HashMap<usize, RunText>,
    /// Records one command at a time, moved to `commands`.
    scratch: Scene,
}

impl Recorder {
    fn take_scratch(&mut self) {
        for c in self.scratch.commands.drain(..) {
            self.commands.push(Command::Other(Box::new(c)));
        }
    }
}

impl RenderContext for Recorder {}

impl PaintScene for Recorder {
    fn reset(&mut self) {
        self.scratch.reset();
        self.commands.clear();
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
        self.scratch.push_layer(fill, blend, alpha, transform, clip, filter, backdrop_filter);
        self.take_scratch();
    }

    fn push_clip_layer(&mut self, fill: Fill, transform: Affine, clip: &impl Shape) {
        self.scratch.push_clip_layer(fill, transform, clip);
        self.take_scratch();
    }

    fn pop_layer(&mut self) {
        self.scratch.pop_layer();
        self.take_scratch();
    }

    fn stroke<'a>(
        &mut self,
        style: &Stroke,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        self.scratch.stroke(style, transform, brush, brush_transform, shape);
        self.take_scratch();
    }

    fn fill<'a>(
        &mut self,
        style: Fill,
        transform: Affine,
        brush: impl Into<PaintRef<'a>>,
        brush_transform: Option<Affine>,
        shape: &impl Shape,
    ) {
        let brush = brush.into();
        if let (Paint::Solid(color), Some(rect)) = (&brush, shape.as_rect()) {
            self.commands.push(Command::RectFill(RectFill {
                fill: style,
                transform,
                color: *color,
                brush_transform,
                rect,
            }));
            return;
        }
        self.scratch.fill(style, transform, brush, brush_transform, shape);
        self.take_scratch();
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
        let index = self.commands.len();
        if let Some(t) = blitz_paint::run_text::take() {
            self.texts.insert(index, t);
        }
        // `Scene::draw_glyphs` keeps the scene borrowed for `'a`, so its body
        // is reproduced here.
        let brush = match brush.into() {
            Paint::Solid(c) => Paint::Solid(c),
            Paint::Gradient(g) => Paint::Gradient(g.clone()),
            Paint::Image(i) => Paint::Image(i.to_owned()),
            Paint::Resource(id) => Paint::Resource(id),
            Paint::Custom(_) => Paint::Solid(Color::TRANSPARENT),
        };
        let run = GlyphRunCommand {
            font_data: font.clone(),
            font_size,
            hint,
            normalized_coords: normalized_coords.to_vec(),
            embolden,
            style: style.into().to_owned(),
            brush,
            brush_alpha,
            transform,
            glyph_transform,
            glyphs: glyphs.into_iter().collect(),
        };
        self.commands.push(Command::Other(Box::new(RenderCommand::GlyphRun(run))));
    }

    fn draw_box_shadow(&mut self, transform: Affine, rect: Rect, brush: Color, radius: f64, std_dev: f64) {
        self.scratch.draw_box_shadow(transform, rect, brush, radius, std_dev);
        self.take_scratch();
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
