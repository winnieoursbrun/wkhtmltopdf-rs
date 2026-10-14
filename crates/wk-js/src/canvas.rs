//! Canvas 2D as a vector recorder. Stands in for WebCore's
//! `CanvasRenderingContext2D` over a `QPainter` image: instead of
//! rasterising, every drawing call is recorded into an `anyrender` scene in
//! canvas pixel space, which the painter replays inside the `<canvas>` box
//! (`vendor/blitz-paint/src/canvas.rs`), so charts stay vector (and their
//! text real text) in the PDF.
//!
//! The JavaScript half (`prelude.js`) keeps the drawing state (transform,
//! styles, path in device space, save/restore); the natives here turn paths,
//! paints and text into scene commands.
//!
//! Not implemented: pixel access (`getImageData`/`putImageData` return blank
//! data, `toDataURL` an empty PNG), compositing modes other than
//! `source-over`, shadows, patterns (painted with their fallback colour),
//! partial `clearRect` (only a clear of the whole canvas erases).

use std::borrow::Cow;
use std::collections::HashMap;

use anyrender::{PaintScene, Scene};
use blitz_dom::NodeId;
use blitz_paint::run_text::RunText;
use boa_engine::object::builtins::JsArray;
use boa_engine::{Context, JsArgs, JsObject, JsResult, JsString, JsValue};
use kurbo::{Affine, BezPath, Cap, Join, Point, Rect, Stroke};
use parley::layout::PositionedLayoutItem;
use parley::style::{FontFamily, FontStyle, FontWeight, StyleProperty};
use parley::{Layout, LayoutContext};
use peniko::color::{AlphaColor, DynamicColor, Srgb};
use peniko::{ColorStop, Fill, Gradient, ImageAlphaType, ImageBrush, ImageData, ImageFormat};

use crate::dom::{arg_str, get_attr, id_of, local, opt_id};
use crate::host::with_host;

/// QtWebKit rasterises canvases, so canvas text is not text in the
/// reference PDFs. Canvas glyphs are painted as outlines (no text layer),
/// which keeps text extraction (L2) identical; the source text is still
/// recorded and can be published by flipping this.
const CANVAS_TEXT_EXTRACTABLE: bool = false;

/// Recorded content of one `<canvas>`.
#[derive(Default)]
pub struct CanvasState {
    pub scene: Scene,
    /// Source text of glyph-run commands, by command index.
    pub texts: HashMap<usize, RunText>,
    /// Active clips (device space), re-applied after a full clear.
    clips: Vec<BezPath>,
}

impl CanvasState {
    fn clear(&mut self) {
        self.scene.reset();
        self.texts.clear();
        for c in &self.clips {
            self.scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, c);
        }
    }

    /// The scene with every clip layer closed.
    pub(crate) fn finish(mut self) -> (Scene, HashMap<usize, RunText>) {
        for _ in 0..self.clips.len() {
            self.scene.pop_layer();
        }
        (self.scene, self.texts)
    }
}

fn canvas_of(v: &JsValue) -> JsResult<NodeId> {
    id_of(v)
}

fn num(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<f64> {
    args.get_or_undefined(i).to_number(ctx)
}

fn affine_from(args: &[JsValue], at: usize, ctx: &mut Context) -> JsResult<Affine> {
    let mut m = [0.0; 6];
    for (k, v) in m.iter_mut().enumerate() {
        *v = num(args, at + k, ctx)?;
    }
    Ok(Affine::new(m))
}

/// Path from the flat array the prelude builds: `0 x y` move, `1 x y` line,
/// `2 cx cy x y` quad, `3 c1x c1y c2x c2y x y` cubic, `4` close.
fn path_from(v: &JsValue, ctx: &mut Context) -> JsResult<BezPath> {
    let mut p = BezPath::new();
    let Some(o) = v.as_object() else { return Ok(p) };
    let arr = JsArray::from_object(o.clone())?;
    let n = arr.length(ctx)?;
    let mut vals = Vec::with_capacity(n as usize);
    for i in 0..n {
        vals.push(arr.get(i, ctx)?.to_number(ctx)?);
    }
    let mut i = 0;
    let pt = |a: f64, b: f64| Point::new(a, b);
    while i < vals.len() {
        let op = vals[i] as i32;
        let take = |k: usize| vals.get(i + k).copied().unwrap_or(0.0);
        match op {
            0 => {
                p.move_to(pt(take(1), take(2)));
                i += 3;
            }
            1 => {
                p.line_to(pt(take(1), take(2)));
                i += 3;
            }
            2 => {
                p.quad_to(pt(take(1), take(2)), pt(take(3), take(4)));
                i += 5;
            }
            3 => {
                p.curve_to(pt(take(1), take(2)), pt(take(3), take(4)), pt(take(5), take(6)));
                i += 7;
            }
            4 => {
                p.close_path();
                i += 1;
            }
            _ => break,
        }
    }
    Ok(p)
}

pub(crate) fn parse_color(s: &str) -> Option<AlphaColor<Srgb>> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("transparent") {
        return Some(AlphaColor::TRANSPARENT);
    }
    peniko::color::parse_color(s).ok().map(|c: DynamicColor| c.to_alpha_color::<Srgb>())
}

/// A paint from a `fillStyle`/`strokeStyle` value: a colour string or a
/// gradient descriptor `{kind, x0, y0, r0, x1, y1, r1, stops}`.
fn paint_from(v: &JsValue, alpha: f32, ctx: &mut Context) -> JsResult<Option<anyrender::Paint>> {
    if let Some(o) = v.as_object() {
        let kind = o.get(JsString::from("kind"), ctx)?.to_string(ctx)?.to_std_string_escaped();
        let g =
            |k: &str, ctx: &mut Context| -> JsResult<f64> { o.get(JsString::from(k), ctx)?.to_number(ctx) };
        let mut grad = match kind.as_str() {
            "linear" => Gradient::new_linear(
                Point::new(g("x0", ctx)?, g("y0", ctx)?),
                Point::new(g("x1", ctx)?, g("y1", ctx)?),
            ),
            "radial" => Gradient::new_two_point_radial(
                Point::new(g("x0", ctx)?, g("y0", ctx)?),
                g("r0", ctx)? as f32,
                Point::new(g("x1", ctx)?, g("y1", ctx)?),
                g("r1", ctx)? as f32,
            ),
            _ => {
                // Pattern: its fallback colour.
                let c = o.get(JsString::from("fallback"), ctx)?;
                return paint_from(&c, alpha, ctx);
            }
        };
        let stops = o.get(JsString::from("stops"), ctx)?;
        let mut out: Vec<ColorStop> = Vec::new();
        if let Some(so) = stops.as_object() {
            let arr = JsArray::from_object(so.clone())?;
            let n = arr.length(ctx)?;
            for i in 0..n {
                let st = arr.get(i, ctx)?;
                let Some(sto) = st.as_object() else { continue };
                let off = sto.get(0, ctx)?.to_number(ctx)? as f32;
                let col = sto.get(1, ctx)?.to_string(ctx)?.to_std_string_escaped();
                let c = parse_color(&col).unwrap_or(AlphaColor::BLACK);
                let c = c.multiply_alpha(alpha);
                out.push(ColorStop::from((off, c)));
            }
        }
        if out.is_empty() {
            return Ok(None);
        }
        grad = grad.with_stops(out.as_slice());
        return Ok(Some(anyrender::Paint::Gradient(grad)));
    }
    let st = v.to_string(ctx)?.to_std_string_escaped();
    let Some(c) = parse_color(&st) else { return Ok(None) };
    let c = c.multiply_alpha(alpha);
    if c.components[3] <= 0.0 {
        return Ok(None);
    }
    Ok(Some(anyrender::Paint::Solid(c)))
}

fn with_canvas<R>(id: NodeId, f: impl FnOnce(&mut CanvasState) -> R) -> R {
    with_host(|h| f(h.canvases.entry(id).or_default()))
}

/// `__wk.canvasFill(canvas, path, paint, evenodd, alpha, brushTransform…6)`.
pub(crate) fn canvas_fill(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    let path = path_from(args.get_or_undefined(1), ctx)?;
    let alpha = num(args, 4, ctx)? as f32;
    let bt = affine_from(args, 5, ctx)?;
    let Some(paint) = paint_from(args.get_or_undefined(2), alpha, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let rule = if args.get_or_undefined(3).to_boolean() { Fill::EvenOdd } else { Fill::NonZero };
    let brush_t = matches!(paint, anyrender::Paint::Gradient(_)).then_some(bt);
    with_canvas(id, |c| c.scene.fill(rule, Affine::IDENTITY, &paint, brush_t, &path));
    Ok(JsValue::undefined())
}

/// `__wk.canvasStroke(canvas, path, paint, alpha, width, cap, join, miter, dash, dashOffset, ctm…6)`:
/// the path is in device space; it is mapped back to user space and stroked
/// under the transform, so the line width scales like the canvas spec says.
pub(crate) fn canvas_stroke(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    let path = path_from(args.get_or_undefined(1), ctx)?;
    let alpha = num(args, 3, ctx)? as f32;
    let width = num(args, 4, ctx)?;
    let cap = arg_str(args, 5, ctx)?;
    let join = arg_str(args, 6, ctx)?;
    let miter = num(args, 7, ctx)?;
    let mut dashes = Vec::new();
    if let Some(o) = args.get_or_undefined(8).as_object() {
        let arr = JsArray::from_object(o.clone())?;
        for i in 0..arr.length(ctx)? {
            dashes.push(arr.get(i, ctx)?.to_number(ctx)?);
        }
    }
    let dash_offset = num(args, 9, ctx)?;
    let ctm = affine_from(args, 10, ctx)?;
    if ctm.determinant().abs() < 1e-12 || width.is_nan() || width <= 0.0 {
        return Ok(JsValue::undefined());
    }
    let Some(paint) = paint_from(args.get_or_undefined(2), alpha, ctx)? else {
        return Ok(JsValue::undefined());
    };
    let mut user = path;
    user.apply_affine(ctm.inverse());
    let mut stroke = Stroke::new(width)
        .with_caps(match cap.as_str() {
            "round" => Cap::Round,
            "square" => Cap::Square,
            _ => Cap::Butt,
        })
        .with_join(match join.as_str() {
            "round" => Join::Round,
            "bevel" => Join::Bevel,
            _ => Join::Miter,
        })
        .with_miter_limit(if miter > 0.0 { miter } else { 10.0 });
    if !dashes.is_empty()
        && dashes.iter().all(|d| d.is_finite() && *d >= 0.0)
        && dashes.iter().any(|d| *d > 0.0)
    {
        stroke = stroke.with_dashes(dash_offset, dashes);
    }
    let brush_t = matches!(paint, anyrender::Paint::Gradient(_)).then_some(Affine::IDENTITY);
    with_canvas(id, |c| c.scene.stroke(&stroke, ctm, &paint, brush_t, &user));
    Ok(JsValue::undefined())
}

/// `__wk.canvasClip(canvas, path, evenodd)`.
pub(crate) fn canvas_clip(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    let path = path_from(args.get_or_undefined(1), ctx)?;
    let rule = if args.get_or_undefined(2).to_boolean() { Fill::EvenOdd } else { Fill::NonZero };
    with_canvas(id, |c| {
        c.scene.push_clip_layer(rule, Affine::IDENTITY, &path);
        c.clips.push(path);
    });
    Ok(JsValue::undefined())
}

/// `__wk.canvasPopClips(canvas, n)` (restore).
pub(crate) fn canvas_pop_clips(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    let n = num(args, 1, ctx)?.max(0.0) as usize;
    with_canvas(id, |c| {
        for _ in 0..n.min(c.clips.len()) {
            c.clips.pop();
            c.scene.pop_layer();
        }
    });
    Ok(JsValue::undefined())
}

/// `__wk.canvasClear(canvas)`: the whole canvas was cleared (or resized).
pub(crate) fn canvas_clear(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    with_canvas(id, |c| c.clear());
    Ok(JsValue::undefined())
}

/// `__wk.canvasReset(canvas)`: width/height assignment (state and clips reset).
pub(crate) fn canvas_reset(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    with_host(|h| {
        h.canvases.insert(id, CanvasState::default());
    });
    Ok(JsValue::undefined())
}

// ---------- text ----------

/// A parsed CSS `font` shorthand.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CanvasFont {
    pub size: f32,
    pub weight: f32,
    pub italic: bool,
    pub families: String,
}

/// Parse `[style] [variant] [weight] size[/line-height] families`.
pub(crate) fn parse_font(s: &str) -> CanvasFont {
    let mut f = CanvasFont { size: 10.0, weight: 400.0, italic: false, families: "sans-serif".into() };
    let s = s.trim();
    // Split off the family list: it starts after the size token.
    let mut rest = s;
    loop {
        let rest_trim = rest.trim_start();
        let (tok, after) = match rest_trim.find(char::is_whitespace) {
            Some(i) => (&rest_trim[..i], &rest_trim[i..]),
            None => (rest_trim, ""),
        };
        if tok.is_empty() {
            return f;
        }
        let lower = tok.to_ascii_lowercase();
        match lower.as_str() {
            "italic" | "oblique" => f.italic = true,
            "normal" | "small-caps" => {}
            "bold" | "bolder" => f.weight = 700.0,
            "lighter" => f.weight = 300.0,
            w if w.len() == 3 && w.chars().all(|c| c.is_ascii_digit()) => {
                f.weight = w.parse().unwrap_or(400.0)
            }
            _ => {
                // The size token (possibly with /line-height).
                let size_tok = lower.split('/').next().unwrap_or("");
                if let Some(px) = css_length_px(size_tok) {
                    f.size = px;
                    let fam = after.trim();
                    if !fam.is_empty() {
                        f.families = fam.to_string();
                    }
                }
                return f;
            }
        }
        rest = after;
    }
}

fn css_length_px(t: &str) -> Option<f32> {
    let named = match t {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let (num, unit) = t.split_at(t.find(|c: char| c.is_ascii_alphabetic() || c == '%').unwrap_or(t.len()));
    let v: f32 = num.parse().ok()?;
    match unit {
        "px" | "" => Some(v),
        "pt" => Some(v * 4.0 / 3.0),
        "em" | "rem" => Some(v * 16.0),
        "%" => Some(v * 16.0 / 100.0),
        "pc" => Some(v * 16.0),
        "in" => Some(v * 96.0),
        "cm" => Some(v * 96.0 / 2.54),
        "mm" => Some(v * 96.0 / 25.4),
        _ => None,
    }
}

fn layout_text(text: &str, font: &CanvasFont) -> Layout<[u8; 4]> {
    with_host(|h| {
        let mut lcx: LayoutContext<[u8; 4]> = LayoutContext::new();
        let mut b = lcx.ranged_builder(&mut h.font_ctx, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(font.families.clone()))));
        // Measured like the document's text (WebKit-2012 profile): the font
        // at a whole-pixel size, no kerning or ligatures; the advances and
        // word rounding come from the shared shaper (vendor/parley_engine).
        b.push_default(StyleProperty::FontSize(font.size.round()));
        let off = |tag: &[u8; 4]| parley::setting::FontFeature {
            tag: parley::setting::Tag::from_bytes(*tag),
            value: 0,
        };
        b.push_default(StyleProperty::FontFeatures(parley::style::FontFeatures::List(Cow::Owned(vec![
            off(b"clig"),
            off(b"kern"),
            off(b"liga"),
        ]))));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(font.weight)));
        if font.italic {
            b.push_default(StyleProperty::FontStyle(FontStyle::Italic));
        }
        let mut layout = b.build(text);
        layout.break_all_lines(None);
        layout
    })
}

/// Text metrics: (width, ascent, descent) in px.
fn text_metrics(layout: &Layout<[u8; 4]>) -> (f32, f32, f32) {
    let mut ascent: f32 = 0.0;
    let mut descent: f32 = 0.0;
    if let Some(line) = layout.lines().next() {
        for run in line.runs() {
            let m = run.font_metrics();
            ascent = ascent.max(m.ascent);
            descent = descent.max(m.descent);
        }
    }
    (layout.full_width(), ascent, descent)
}

/// `__wk.measureText(font, text)` → `{width, actualBoundingBoxAscent, …}`.
pub(crate) fn measure_text(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let font = parse_font(&arg_str(args, 0, ctx)?);
    let text = arg_str(args, 1, ctx)?.replace(['\n', '\t', '\r', '\u{c}'], " ");
    let layout = layout_text(&text, &font);
    let (w, a, d) = text_metrics(&layout);
    let o = JsObject::with_object_proto(ctx.intrinsics());
    for (k, v) in [
        ("width", w),
        ("actualBoundingBoxAscent", a),
        ("actualBoundingBoxDescent", d),
        ("fontBoundingBoxAscent", a),
        ("fontBoundingBoxDescent", d),
        ("actualBoundingBoxLeft", 0.0),
        ("actualBoundingBoxRight", w),
    ] {
        o.set(JsString::from(k), f64::from(v), false, ctx)?;
    }
    Ok(o.into())
}

/// `__wk.canvasText(canvas, text, x, y, font, align, baseline, maxWidth,
/// paint, alpha, ctm…6, stroke, lineWidth, rtl)`.
pub(crate) fn canvas_text(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    let text = arg_str(args, 1, ctx)?.replace(['\n', '\t', '\r', '\u{c}'], " ");
    let x = num(args, 2, ctx)?;
    let y = num(args, 3, ctx)?;
    let font = parse_font(&arg_str(args, 4, ctx)?);
    let align = arg_str(args, 5, ctx)?;
    let baseline = arg_str(args, 6, ctx)?;
    let max_width = args.get_or_undefined(7);
    let max_width = if max_width.is_undefined() { None } else { Some(max_width.to_number(ctx)?) };
    let alpha = num(args, 9, ctx)? as f32;
    let ctm = affine_from(args, 10, ctx)?;
    let stroke = args.get_or_undefined(16).to_boolean();
    let line_width = num(args, 17, ctx)?;
    let rtl = args.get_or_undefined(18).to_boolean();
    let Some(paint) = paint_from(args.get_or_undefined(8), alpha, ctx)? else {
        return Ok(JsValue::undefined());
    };
    if text.is_empty() || !x.is_finite() || !y.is_finite() {
        return Ok(JsValue::undefined());
    }
    let layout = layout_text(&text, &font);
    let (w, ascent, descent) = text_metrics(&layout);
    let w = f64::from(w);
    let dx = match align.as_str() {
        "center" => -w / 2.0,
        "right" => -w,
        "end" if !rtl => -w,
        "start" if rtl => -w,
        _ => 0.0,
    };
    let (ascent, descent) = (f64::from(ascent), f64::from(descent));
    // Offset from the alphabetic baseline to the requested one.
    let dy = match baseline.as_str() {
        "top" | "hanging" => ascent,
        "middle" => (ascent - descent) / 2.0,
        "bottom" | "ideographic" => -descent,
        _ => 0.0,
    };
    let mut t = ctm * Affine::translate((x, y));
    if let Some(mw) = max_width {
        if mw > 0.0 && w > mw {
            t *= Affine::scale_non_uniform(mw / w, 1.0);
        }
    }
    t *= Affine::translate((dx, dy));
    with_host(|h| {
        let c = h.canvases.entry(id).or_default();
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
                let run = gr.run();
                let baseline_y = gr.baseline();
                let tr = run.text_range();
                let run_text = text.get(tr.clone()).unwrap_or("").to_string();
                // One range per glyph, along the clusters.
                let mut ranges = Vec::new();
                for cl in run.visual_clusters() {
                    let r = cl.text_range();
                    let r = r.start.saturating_sub(tr.start)..r.end.saturating_sub(tr.start);
                    for _ in 0..cl.glyphs().count() {
                        ranges.push(r.clone());
                    }
                }
                let glyphs: Vec<anyrender::Glyph> = gr
                    .positioned_glyphs()
                    .map(|g| anyrender::Glyph { id: g.id as _, x: g.x, y: g.y - baseline_y })
                    .collect();
                if ranges.len() != glyphs.len() {
                    ranges = vec![0..run_text.len(); glyphs.len()];
                }
                let coords: Vec<anyrender::NormalizedCoord> =
                    run.normalized_coords().iter().map(|c| c.to_bits()).collect();
                let index = c.scene.commands.len();
                let style: peniko::Style = if stroke {
                    peniko::Style::Stroke(Stroke::new(line_width))
                } else {
                    peniko::Style::Fill(Fill::NonZero)
                };
                c.scene.draw_glyphs(
                    run.font(),
                    run.font_size(),
                    false,
                    &coords,
                    kurbo::Vec2::ZERO,
                    &style,
                    &paint,
                    1.0,
                    t,
                    run.synthesis().skew().map(|a| Affine::skew(f64::from(a.to_radians().tan()), 0.0)),
                    glyphs.into_iter(),
                );
                if CANVAS_TEXT_EXTRACTABLE {
                    c.texts.insert(index, RunText { text: run_text, glyph_ranges: ranges, node_id: id });
                }
            }
        }
    });
    Ok(JsValue::undefined())
}

// ---------- images ----------

/// `__wk.canvasImage(canvas, source, sx, sy, sw, sh, dx, dy, dw, dh, alpha, ctm…6)`:
/// an `<img>` (its decoded raster) or another canvas (its recording).
pub(crate) fn canvas_image(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = canvas_of(args.get_or_undefined(0))?;
    let Some(src) = opt_id(args.get_or_undefined(1)) else { return Ok(JsValue::undefined()) };
    let mut r = [0.0f64; 8];
    for (k, v) in r.iter_mut().enumerate() {
        *v = num(args, 2 + k, ctx)?;
    }
    let alpha = num(args, 10, ctx)? as f32;
    let ctm = affine_from(args, 11, ctx)?;
    let [sx, sy, sw, sh, dx, dy, dw, dh] = r;
    if sw == 0.0 || sh == 0.0 || dw == 0.0 || dh == 0.0 {
        return Ok(JsValue::undefined());
    }
    // Source space → destination rectangle, under the current transform.
    let place = ctm
        * Affine::translate((dx, dy))
        * Affine::scale_non_uniform(dw / sw, dh / sh)
        * Affine::translate((-sx, -sy));
    let clip = Rect::new(sx, sy, sx + sw, sy + sh);
    let tag = with_host(|h| local(h, src)).unwrap_or_default();
    if tag == "canvas" {
        let sub =
            with_host(|h| h.canvases.get(&src).map(|c| (c.scene.clone(), c.texts.clone(), c.clips.len())));
        let Some((scene, texts, open)) = sub else { return Ok(JsValue::undefined()) };
        with_host(|h| {
            let c = h.canvases.entry(id).or_default();
            c.scene.push_layer(Fill::NonZero, peniko::Mix::Normal, alpha, place, &clip, None, None);
            let base = c.scene.commands.len();
            let mut cmds = scene.commands;
            for _ in 0..open {
                cmds.push(anyrender::recording::RenderCommand::PopLayer);
            }
            for (i, cmd) in cmds.into_iter().enumerate() {
                c.scene.commands.push(apply_transform(cmd, place));
                if let Some(t) = texts.get(&i) {
                    c.texts.insert(base + i, t.clone());
                }
            }
            c.scene.pop_layer();
        });
        return Ok(JsValue::undefined());
    }
    let img = with_host(|h| {
        let node = h.doc.get_node(src)?;
        let raster = node.element_data()?.raster_image_data()?;
        Some((raster.width, raster.height, raster.data.clone()))
    });
    let Some((w, hgt, data)) = img else { return Ok(JsValue::undefined()) };
    let image = ImageData {
        data,
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w,
        height: hgt,
    };
    let brush = ImageBrush::new(image).with_alpha(alpha);
    with_canvas(id, |c| {
        c.scene.fill(Fill::NonZero, place, brush.as_ref(), None, &clip);
    });
    Ok(JsValue::undefined())
}

fn apply_transform(
    mut cmd: anyrender::recording::RenderCommand,
    t: Affine,
) -> anyrender::recording::RenderCommand {
    use anyrender::recording::RenderCommand as C;
    match &mut cmd {
        C::PushLayer(c) => c.transform = t * c.transform,
        C::PushClipLayer(c) => c.transform = t * c.transform,
        C::PopLayer => {}
        C::Stroke(c) => c.transform = t * c.transform,
        C::Fill(c) => c.transform = t * c.transform,
        C::GlyphRun(c) => c.transform = t * c.transform,
        C::BoxShadow(c) => c.transform = t * c.transform,
    }
    cmd
}

/// `__wk.imageSize(img)` → `[naturalWidth, naturalHeight]` of a decoded image.
pub(crate) fn image_size(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let Some(id) = opt_id(args.get_or_undefined(0)) else { return Ok(JsValue::null()) };
    let wh = with_host(|h| {
        let node = h.doc.get_node(id)?;
        let raster = node.element_data()?.raster_image_data()?;
        Some((raster.width, raster.height))
    });
    let (w, hh) = wh.unwrap_or((0, 0));
    Ok(JsArray::from_iter([JsValue::from(w), JsValue::from(hh)], ctx).into())
}

/// The canvas size from its attributes (defaults 300×150).
pub(crate) fn canvas_size(h: &crate::host::Host, id: NodeId) -> (f64, f64) {
    let dim = |n: &str, d: f64| {
        get_attr(h, id, n).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| *v >= 0.0).unwrap_or(d)
    };
    (dim("width", 300.0), dim("height", 150.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_shorthand() {
        assert_eq!(
            parse_font("bold 12px 'Helvetica Neue', Helvetica, Arial, sans-serif"),
            CanvasFont {
                size: 12.0,
                weight: 700.0,
                italic: false,
                families: "'Helvetica Neue', Helvetica, Arial, sans-serif".into()
            }
        );
        assert_eq!(parse_font("10px sans-serif").size, 10.0);
        assert_eq!(parse_font("italic 600 9pt/2 serif").weight, 600.0);
        assert!(parse_font("italic 600 9pt/2 serif").italic);
    }

    #[test]
    fn colours() {
        assert_eq!(parse_color("#ff0000").map(|c| c.to_rgba8().to_u8_array()), Some([255, 0, 0, 255]));
        assert_eq!(
            parse_color("rgba(0, 0, 255, 0.5)").map(|c| c.to_rgba8().to_u8_array()),
            Some([0, 0, 255, 128])
        );
        assert!(parse_color("nonsense").is_none());
    }
}
