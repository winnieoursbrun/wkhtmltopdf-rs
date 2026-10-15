//! Display list recording. Stands in for WebCore's `GraphicsContext` on a
//! `QPainter` during `QWebFrame::print`: the whole document is painted once
//! into a display list of `anyrender` commands (blitz-paint), and every glyph
//! run keeps the source text published by the vendored blitz-paint
//! (`vendor/blitz-paint/src/run_text.rs`), so the PDF gets real text.
//!
//! Memory: an `anyrender::recording::RenderCommand` is 464 bytes, plus a
//! heap path for its shape. Table-heavy documents paint tens of thousands of
//! solid rectangles (cell backgrounds, collapsed borders), so those are kept
//! as [`RectFill`] (no path, 104 bytes) and expanded on demand into the very
//! command `anyrender::Scene::fill` would have recorded; every other command
//! is recorded by an `anyrender::Scene` and boxed.

use std::borrow::Cow;
use std::sync::Arc;

use anyrender::recording::{FillCommand, GlyphRunCommand, RenderCommand};
use anyrender::{Glyph, NormalizedCoord, Paint, PaintRef, PaintScene, RenderContext, Scene};
use blitz_paint::run_text::RunText;
use kurbo::{Affine, Rect, Shape, Stroke};
use peniko::{BlendMode, Color, Fill, FontData, StyleRef};

/// A solid rectangle fill (`Scene::fill` with a `Rect`, a solid colour and
/// no brush transform).
#[derive(Debug, Clone)]
pub struct RectFill {
    fill: Fill,
    transform: Affine,
    color: Color,
    rect: Rect,
}

/// One recorded command.
#[derive(Debug, Clone)]
pub enum Command {
    RectFill(RectFill),
    PopLayer,
    Other(Box<RenderCommand>),
}

impl Command {
    /// A solid rectangle fill as its fill rule, transform, colour and rectangle: what
    /// [`get`](Self::get) expands into a `Fill` command with the rectangle's path, read
    /// without building that path.
    pub fn as_rect_fill(&self) -> Option<(Fill, Affine, Color, Rect)> {
        match self {
            Command::RectFill(r) => Some((r.fill, r.transform, r.color, r.rect)),
            _ => None,
        }
    }

    /// The command as `anyrender::Scene` records it.
    pub fn get(&self) -> Cow<'_, RenderCommand> {
        match self {
            Command::Other(c) => Cow::Borrowed(c),
            Command::PopLayer => Cow::Owned(RenderCommand::PopLayer),
            Command::RectFill(r) => Cow::Owned(RenderCommand::Fill(FillCommand {
                fill: r.fill,
                transform: r.transform,
                brush: Paint::Solid(r.color),
                brush_transform: None,
                shape: r.rect.into_path(Scene::default().tolerance),
            })),
        }
    }
}

/// Commands in fixed-size chunks: a long document records tens of thousands
/// of them, and a single `Vec` would hold up to twice its size (three times
/// while it grows).
#[derive(Default)]
pub struct DisplayList {
    chunks: Vec<Vec<Command>>,
    len: usize,
}

impl DisplayList {
    const CHUNK: usize = 4096;

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, i: usize) -> Option<&Command> {
        self.chunks.get(i / Self::CHUNK)?.get(i % Self::CHUNK)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Command> {
        self.chunks.iter().flatten()
    }

    /// The commands in chunks of at most 4096, in order.
    pub fn chunks(&self) -> &[Vec<Command>] {
        &self.chunks
    }

    fn push(&mut self, c: Command) {
        match self.chunks.last_mut() {
            Some(last) if last.len() < Self::CHUNK => last.push(c),
            _ => {
                let mut chunk = Vec::with_capacity(Self::CHUNK);
                chunk.push(c);
                self.chunks.push(chunk);
            }
        }
        self.len += 1;
    }

    fn clear(&mut self) {
        self.chunks.clear();
        self.len = 0;
    }
}

/// Source text of glyph-run commands, by command index: recorded in painting order, so a
/// vector sorted by index (a hash map rehashed its ~9,000 entries as it grew on large.html).
#[derive(Default)]
pub struct RunTexts {
    entries: Vec<(usize, RunText)>,
}

impl RunTexts {
    /// The text of the command at `index`.
    pub fn get(&self, index: &usize) -> Option<&RunText> {
        self.entries.binary_search_by_key(index, |(i, _)| *i).ok().map(|k| &self.entries[k].1)
    }

    /// Set the text of the command at `index`.
    pub fn insert(&mut self, index: usize, text: RunText) {
        match self.entries.last() {
            Some((last, _)) if *last >= index => match self.entries.binary_search_by_key(&index, |(i, _)| *i)
            {
                Ok(k) => self.entries[k].1 = text,
                Err(k) => self.entries.insert(k, (index, text)),
            },
            _ => self.entries.push((index, text)),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The texts with their command index, by index.
    pub fn iter(&self) -> impl Iterator<Item = (&usize, &RunText)> {
        self.entries.iter().map(|(i, t)| (i, t))
    }
}

impl<'a> IntoIterator for &'a RunTexts {
    type Item = (&'a usize, &'a RunText);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (usize, RunText)>,
        fn(&'a (usize, RunText)) -> (&'a usize, &'a RunText),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(i, t)| (i, t))
    }
}

/// A painted document.
#[derive(Default)]
pub struct Recorder {
    /// The display list, in painting order.
    pub commands: DisplayList,
    /// Source text of glyph-run commands, by command index.
    pub texts: RunTexts,
    /// Records one command at a time, moved to `commands`.
    scratch: Scene,
}

impl Recorder {
    fn take_scratch(&mut self) {
        for c in self.scratch.commands.drain(..) {
            self.commands.push(match c {
                RenderCommand::PopLayer => Command::PopLayer,
                c => Command::Other(Box::new(c)),
            });
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
        if let (Paint::Solid(color), Some(rect), None) = (&brush, shape.as_rect(), brush_transform) {
            self.commands.push(Command::RectFill(RectFill { fill: style, transform, color: *color, rect }));
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
