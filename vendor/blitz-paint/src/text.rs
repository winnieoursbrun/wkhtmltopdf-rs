use anyrender::PaintScene;
use blitz_dom::{BaseDocument, NodeId, node::TextBrush, util::ToColorColor};
use kurbo::{Affine, BezPath, Cap, Circle, Rect, Stroke};
use parley::{Affinity, Cursor, Layout, Line, PositionedLayoutItem, Selection};
use peniko::Fill;
use std::collections::HashMap;
use style::properties::generated::longhands::text_decoration_style::computed_value::T as TextDecorationStyle;
use style::values::computed::{
    Length, LengthPercentage, TextDecorationLength, TextDecorationLine, TextUnderlinePosition,
};
use style::values::generics::text::{GenericTextDecorationInset, GenericTextDecorationLength};

use crate::color::{Color, ToColorColor as _};
use crate::{FONT_EMBOLDEN_ENABLED, SELECTION_COLOR};

/// wkhtmltopdf-rs: the `position: relative` offset (`top`/`left`, else `-bottom`/`-right`) of
/// the inline elements between a text run's node and its inline root, in scaled px. QtWebKit 2.2
/// moves a relatively positioned inline box with its background and text.
pub(crate) fn relative_inline_offset(doc: &BaseDocument, node_id: NodeId, inline_root_id: NodeId, scale: f64) -> kurbo::Vec2 {
    use style::values::computed::position::Inset;
    let px = |v: &Inset| match v {
        Inset::LengthPercentage(lp) => lp.to_length().map(|l| l.px() as f64),
        _ => None,
    };
    let mut offset = kurbo::Vec2::ZERO;
    let mut id = Some(node_id);
    while let Some(nid) = id {
        if nid == inline_root_id {
            break;
        }
        let Some(node) = doc.get_node(nid) else { break };
        if let Some(styles) = node.primary_styles() {
            if styles.get_box().position == style::computed_values::position::T::Relative {
                let pos = styles.get_position();
                offset.x += px(&pos.left).or_else(|| px(&pos.right).map(|r| -r)).unwrap_or(0.0);
                offset.y += px(&pos.top).or_else(|| px(&pos.bottom).map(|b| -b)).unwrap_or(0.0);
            }
        }
        id = node.parent;
    }
    offset * scale
}

/// The edge boxes wk-webkit inserts as the first and last children of an inline element
/// with horizontal margins, borders or padding (`<wkedge data-edge="start|end">`, an
/// inline-block as wide as the edge, holding its place in the line).
const EDGE_TAG: &str = "wkedge";

fn style_brush_id(glyph_run: &parley::GlyphRun<'_, TextBrush>) -> NodeId {
    glyph_run.style().brush.id
}

/// One line's fragment of an inline element.
struct Fragment {
    id: NodeId,
    depth: usize,
    x0: f64,
    x1: f64,
    /// The border box starts (ends) on this line: its left (right) edge is drawn.
    starts: bool,
    ends: bool,
    /// The element's own text: baseline, ascent and descent (rounded up, like QtWebKit's
    /// FreeType metrics), else a descendant's.
    metrics: Option<(f64, f64, f64)>,
    own_metrics: bool,
}

/// Draw the backgrounds and borders of inline elements (e.g. `<span style="background:
/// ...; border: ...">`), line by line, behind the text, as QtWebKit's InlineFlowBox does:
/// a fragment covers the element's text and inline boxes on the line (its descendants'
/// too) from the font's ascent to its descent, extended by the vertical padding and borders,
/// and by the horizontal ones where the element starts and ends (the edge boxes of
/// [`EDGE_TAG`]).
///
/// The inline root's own background is painted separately (as a normal block box), so
/// the root is skipped.
pub(crate) fn draw_inline_backgrounds<'a>(
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    doc: &BaseDocument,
    transform: Affine,
    inline_root_id: NodeId,
    scale: f64,
) {
    let mut fragments: Vec<Fragment> = Vec::new();
    for line in lines {
        fragments.clear();
        // Every element from `id` up to the root (exclusive), innermost first.
        let extend = |fragments: &mut Vec<Fragment>,
                          id: NodeId,
                          x0: f64,
                          x1: f64,
                          metrics: Option<(f64, f64, f64)>| {
            let mut chain = Vec::new();
            let mut cur = Some(id);
            // Up to the inline root, or to the first box that is not inline (the root of an
            // anonymous block is not the parent of its inline content).
            while let Some(c) = cur {
                if c == inline_root_id {
                    break;
                }
                let Some(node) = doc.get_node(c) else { break };
                if node.is_element() {
                    let inline = node.primary_styles().is_some_and(|s| {
                        let d = s.get_box().display;
                        d.outside() == style::values::specified::box_::DisplayOutside::Inline
                            && d.inside() == style::values::specified::box_::DisplayInside::Flow
                    });
                    if !inline {
                        break;
                    }
                    chain.push(c);
                }
                cur = node.parent;
            }
            let depth = chain.len();
            for (k, &c) in chain.iter().enumerate() {
                let own = k == 0 && c == id;
                match fragments.iter_mut().find(|f| f.id == c) {
                    Some(f) => {
                        f.x0 = f.x0.min(x0);
                        f.x1 = f.x1.max(x1);
                        if metrics.is_some() && (f.metrics.is_none() || (own && !f.own_metrics)) {
                            f.metrics = metrics;
                            f.own_metrics = own;
                        }
                    }
                    None => fragments.push(Fragment {
                        id: c,
                        depth: depth - k,
                        x0,
                        x1,
                        starts: false,
                        ends: false,
                        metrics,
                        own_metrics: own && metrics.is_some(),
                    }),
                }
            }
        };
        for item in line.items() {
            match item {
                PositionedLayoutItem::GlyphRun(glyph_run) => {
                    let node_id = glyph_run.style().brush.id;
                    if node_id == inline_root_id {
                        continue;
                    }
                    let metrics = glyph_run.run().font_metrics();
                    let x = glyph_run.offset() as f64;
                    let m = (
                        glyph_run.baseline() as f64,
                        (metrics.ascent as f64).ceil(),
                        (metrics.descent as f64).ceil(),
                    );
                    extend(&mut fragments, node_id, x, x + glyph_run.advance() as f64, Some(m));
                }
                PositionedLayoutItem::InlineBox(ibox) => {
                    let id = NodeId::from_u64(ibox.id);
                    let Some(node) = doc.get_node(id) else { continue };
                    let Some(parent) = node.parent else { continue };
                    let (x0, x1) = (ibox.x as f64, (ibox.x + ibox.width) as f64);
                    if node.element_data().is_some_and(|e| &*e.name.local == EDGE_TAG) {
                        let start = node.element_data().and_then(|e| e.attrs.iter().find(|a| &*a.name.local == "data-edge")).map(|a| a.value.as_str()) != Some("end");
                        // The edge box spans the margin, border and padding: the border box
                        // starts after the margin (ends before it).
                        let margin = doc
                            .get_node(parent)
                            .and_then(|p| p.primary_styles())
                            .map(|s| {
                                let m = s.get_margin();
                                let side = if start { &m.margin_left } else { &m.margin_right };
                                match side {
                                    style::values::generics::length::GenericMargin::LengthPercentage(lp) => {
                                        lp.to_length().map_or(0.0, |l| l.px() as f64)
                                    }
                                    _ => 0.0,
                                }
                            })
                            .unwrap_or(0.0);
                        let (b0, b1) = if start { (x0 + margin, x1) } else { (x0, x1 - margin) };
                        extend(&mut fragments, parent, b0, b1, None);
                        if let Some(f) = fragments.iter_mut().find(|f| f.id == parent) {
                            if start {
                                f.starts = true;
                                f.x0 = b0;
                            } else {
                                f.ends = true;
                                f.x1 = b1;
                            }
                        }
                    } else if ibox.kind == parley::InlineBoxKind::InFlow {
                        extend(&mut fragments, parent, x0, x1, None);
                    }
                }
            }
        }
        if fragments.is_empty() {
            continue;
        }
        let line_baseline = line.metrics().baseline as f64;
        // Outer elements first.
        fragments.sort_by_key(|f| f.depth);
        for f in &fragments {
            let Some(node) = doc.get_node(f.id) else { continue };
            let Some(styles) = node.primary_styles() else { continue };
            let current_color = styles.clone_color();
            let bg_color = styles
                .get_background()
                .background_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            let border = styles.get_border();
            let side = |w: &style::values::computed::BorderSideWidth,
                        st: style::values::specified::border::BorderStyle| {
                if st.none_or_hidden() { 0.0 } else { f64::from(blitz_dom::webkit_border_width(w.0.to_f32_px())) }
            };
            let (bt, bb) = (
                side(&border.border_top_width, border.border_top_style),
                side(&border.border_bottom_width, border.border_bottom_style),
            );
            let bl = if f.starts { side(&border.border_left_width, border.border_left_style) } else { 0.0 };
            let br = if f.ends { side(&border.border_right_width, border.border_right_style) } else { 0.0 };
            let has_border = bt > 0.0 || bb > 0.0 || bl > 0.0 || br > 0.0;
            if bg_color == Color::TRANSPARENT && !has_border {
                continue;
            }
            // Vertical padding does not move the line; it widens the fragment.
            let pad = styles.get_padding();
            let len = |v: &style::values::computed::LengthPercentage| v.to_length().map_or(0.0, |l| l.px() as f64);
            let (pt, pb) = (len(&pad.padding_top.0), len(&pad.padding_bottom.0));
            let (baseline, ascent, descent) = f.metrics.unwrap_or((line_baseline, 0.0, 0.0));
            let rect = Rect::new(f.x0, baseline - ascent - pt - bt, f.x1, baseline + descent + pb + bb);
            if rect.width() <= 0.0 {
                continue;
            }
            // A relatively positioned inline box moves with its background and border.
            let transform = transform.pre_translate(relative_inline_offset(doc, f.id, inline_root_id, scale));
            if bg_color != Color::TRANSPARENT {
                scene.fill(Fill::NonZero, transform, bg_color, None, &rect);
            }
            if !has_border {
                continue;
            }
            let color = |c: &style::values::computed::Color| c.resolve_to_absolute(&current_color).as_srgb_color();
            let edges = [
                (Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + bt), color(&border.border_top_color), border.border_top_style, bt, true),
                (Rect::new(rect.x0, rect.y1 - bb, rect.x1, rect.y1), color(&border.border_bottom_color), border.border_bottom_style, bb, true),
                (Rect::new(rect.x0, rect.y0, rect.x0 + bl, rect.y1), color(&border.border_left_color), border.border_left_style, bl, false),
                (Rect::new(rect.x1 - br, rect.y0, rect.x1, rect.y1), color(&border.border_right_color), border.border_right_style, br, false),
            ];
            for (r, c, st, w, horizontal) in edges {
                if w <= 0.0 {
                    continue;
                }
                fill_border_side(scene, transform, r, c, st, w, horizontal);
            }
        }
    }
}

/// One side of an inline fragment's border: solid, or dashes (3 widths long) and dots
/// (squares) with equal gaps, as QtWebKit strokes them; other styles are drawn solid.
fn fill_border_side(
    scene: &mut impl PaintScene,
    transform: Affine,
    r: Rect,
    color: Color,
    style: style::values::specified::border::BorderStyle,
    width: f64,
    horizontal: bool,
) {
    use style::values::specified::border::BorderStyle;
    let dash = match style {
        BorderStyle::Dashed => 3.0 * width,
        BorderStyle::Dotted => width,
        _ => {
            scene.fill(Fill::NonZero, transform, color, None, &r);
            return;
        }
    };
    let len = if horizontal { r.width() } else { r.height() };
    let mut t = 0.0;
    while t < len {
        let e = (t + dash).min(len);
        let seg = if horizontal {
            Rect::new(r.x0 + t, r.y0, r.x0 + e, r.y1)
        } else {
            Rect::new(r.x0, r.y0 + t, r.x1, r.y0 + e)
        };
        scene.fill(Fill::NonZero, transform, color, None, &seg);
        t += 2.0 * dash;
    }
}

/// Per-font-face cache of the OS/2 `usWinAscent / unitsPerEm` ratio, keyed by the font
/// blob's unique id and face index. `None` is cached for fonts without a usable OS/2 or
/// head table so they aren't re-parsed either.
type WinAscentCache = HashMap<(u64, u32), Option<f32>>;

/// The font's OS/2 `usWinAscent`, in the same (device) pixels as `font_size`.
///
/// This is the ascent browsers use as the top of the "em box" when positioning overlines. It
/// is typically taller than the hhea ascent Parley exposes via [`parley::layout::run::RunMetrics`].
/// Returns `None` when the font has no OS/2 table.
#[allow(dead_code)]
fn win_ascent(cache: &mut WinAscentCache, font: &parley::FontData, font_size: f32) -> Option<f32> {
    let ratio = *cache
        .entry((font.data.id(), font.index))
        .or_insert_with(|| win_ascent_ratio(font));
    Some(ratio? * font_size)
}

/// The unitless `usWinAscent / unitsPerEm` ratio for a font face, or `None` when the font
/// has no usable head/OS/2 table.
#[allow(dead_code)]
fn win_ascent_ratio(font: &parley::FontData) -> Option<f32> {
    use skrifa::raw::{FontRef, TableProvider as _};
    let font_ref = FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let units_per_em = font_ref.head().ok()?.units_per_em();
    let win_ascent = font_ref.os2().ok()?.us_win_ascent();
    Some(win_ascent as f32 / units_per_em as f32)
}

/// Mirrors Blink's `SelectBestDashGap`: choose the gap length (as close as possible to
/// `gap_length`) that fits a whole number of `dash_length` dashes across `stroke_length`,
/// so dashes are evenly distributed and a dash lands at each end of the line.
fn select_best_dash_gap(stroke_length: f64, dash_length: f64, gap_length: f64) -> f64 {
    let available_length = stroke_length + gap_length;
    let min_num_dashes = (available_length / (dash_length + gap_length)).floor();
    let max_num_dashes = min_num_dashes + 1.0;
    let min_num_gaps = min_num_dashes - 1.0;
    let max_num_gaps = max_num_dashes - 1.0;
    if min_num_gaps < 1.0 {
        return gap_length;
    }
    let min_gap = (stroke_length - min_num_dashes * dash_length) / min_num_gaps;
    let max_gap = (stroke_length - max_num_dashes * dash_length) / max_num_gaps;
    if max_gap <= 0.0 || (min_gap - gap_length).abs() < (max_gap - gap_length).abs() {
        min_gap
    } else {
        max_gap
    }
}

/// A text decoration resolved from a single "decorating box" (an element that
/// sets `text-decoration-line`), ready to be drawn over a glyph run. Because the
/// `text-decoration-*` properties do not inherit, decorations are propagated from
/// ancestors rather than inherited, so a run may be covered by several of these.
///
/// Only the run-independent (style-derived) values are stored here so an instance
/// can be cached on the ancestor stack and reused across every run within the same
/// decorating box. The `text-decoration-inset` and `text-underline-offset` values
/// depend on the run's advance/font-size and are resolved lazily when drawing.
#[derive(Clone)]
struct ResolvedDecoration {
    line: TextDecorationLine,
    style: TextDecorationStyle,
    color: Color,
    thickness: TextDecorationLength,
    /// `text-underline-offset`; `None` for `auto`. Resolved per run.
    underline_offset: Option<LengthPercentage>,
    /// Whether `text-underline-position: under` is in effect.
    underline_under: bool,
    /// Whether `text-underline-position: from-font` is in effect.
    underline_from_font: bool,
    /// `text-decoration-inset`. Resolved per run.
    inset: GenericTextDecorationInset<LengthPercentage>,
}

/// An element on the current ancestor path (inline root -> run node), caching the
/// values resolved from its computed style so descending into the same node across
/// consecutive runs doesn't re-resolve them.
struct DecorationStackEntry {
    node_id: NodeId,
    /// This node's (inherited) text colour, used for the glyphs of runs whose
    /// innermost node is this one.
    text_color: Color,
    /// The decoration this node introduces as a decorating box, if any.
    decoration: Option<ResolvedDecoration>,
}

/// Resolve the cached style values for a single node into a [`DecorationStackEntry`].
fn resolve_decoration_entry(doc: &BaseDocument, node_id: NodeId) -> DecorationStackEntry {
    let Some(styles) = doc.get_node(node_id).and_then(|node| node.primary_styles()) else {
        return DecorationStackEntry {
            node_id,
            text_color: Color::BLACK,
            decoration: None,
        };
    };

    let itext = styles.get_inherited_text();
    let text = styles.get_text();
    let text_color = itext.color.as_color_color();

    let drawn_lines = TextDecorationLine::UNDERLINE
        | TextDecorationLine::OVERLINE
        | TextDecorationLine::LINE_THROUGH;
    let line = text.text_decoration_line;
    // Decorations propagate through the box tree, and a `display: contents` element
    // generates no box, so its decorations have no effect on descendants.
    let is_contents = styles.clone_display().is_contents();
    let decoration = (!is_contents && line.intersects(drawn_lines)).then(|| {
        // `text-decoration-color: currentColor` (the initial value) resolves against
        // the decorating box's own colour, not the descendant run's.
        let color = text
            .text_decoration_color
            .as_absolute()
            .map(ToColorColor::as_color_color)
            .unwrap_or(text_color);

        ResolvedDecoration {
            line,
            style: text.text_decoration_style,
            color,
            thickness: text.text_decoration_thickness.clone(),
            underline_offset: itext.text_underline_offset.non_auto(),
            underline_under: itext
                .text_underline_position
                .contains(TextUnderlinePosition::UNDER),
            underline_from_font: itext
                .text_underline_position
                .contains(TextUnderlinePosition::FROM_FONT),
            inset: text.text_decoration_inset.clone(),
        }
    });

    DecorationStackEntry {
        node_id,
        text_color,
        decoration,
    }
}

/// The run-dependent geometry used to position and size a decoration, captured from a
/// single glyph run. Firefox draws a decoration once for the whole decorating box using
/// the box's own font, so we prefer the values from a run whose innermost node *is* the
/// decorating box (its own text), falling back to the first run it covers.
#[derive(Clone)]
#[allow(dead_code)]
struct DecorationRunGeometry {
    /// The baseline the decoration is positioned from (the run's own, `vertical-align`
    /// shifted, baseline).
    baseline: f32,
    ascent: f32,
    descent: f32,
    underline_offset: f32,
    underline_size: f32,
    strikethrough_size: f32,
    font: parley::FontData,
    font_size: f32,
    css_font_size: f64,
}

/// A decoration accumulated across the runs of a single line for one decorating box.
///
/// The `text-decoration-*` properties describe the whole box, so we gather the horizontal
/// extent it covers on the line (`min_x`/`max_x`) and the font geometry to draw with, then
/// paint a single line spanning the box rather than one segment per run.
struct LineDecoration {
    node_id: NodeId,
    deco: ResolvedDecoration,
    min_x: f64,
    max_x: f64,
    /// Geometry from a run whose innermost node is the decorating box itself.
    own: Option<DecorationRunGeometry>,
    /// Geometry from the first run the box covers (fallback when it has no own text on
    /// this line, e.g. it only wraps differently-sized descendants).
    first: Option<DecorationRunGeometry>,
}

/// Reusable scratch storage for drawing text across all inline formatting contexts in a
/// document. The vectors are cleared between uses without releasing their allocations.
#[derive(Default)]
pub(crate) struct DrawTextContext {
    stack: Vec<DecorationStackEntry>,
    path_scratch: Vec<NodeId>,
    deco_boxes: Vec<LineDecoration>,
    win_ascent_ratios: WinAscentCache,
    /// wkhtmltopdf-rs: `text-overflow: ellipsis` on the inline root being
    /// drawn: the width of its content box (layout units).
    pub(crate) ellipsis_limit: Option<f64>,
}

/// wkhtmltopdf-rs: the glyph id and advance of `…` in `font` at `size`.
fn ellipsis_glyph(font: &peniko::FontData, size: f32) -> Option<(u32, f64)> {
    use skrifa::MetadataProvider;
    let f = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let gid = f.charmap().map('\u{2026}')?;
    let adv = f
        .glyph_metrics(skrifa::instance::Size::new(size), skrifa::instance::LocationRef::default())
        .advance_width(gid)?;
    Some((gid.to_u32(), adv as f64))
}

/// Resolve the CSS `text-decoration-thickness` to a device-pixel size.
///
/// - Percentages are resolved against the font size,
/// - `from-font` uses the metrics from Parley,
/// - `auto` uses a thickness of `font-size / 10` (minimum: 1px).
///
/// The result is floored to a whole number of device pixels (minimum 1), matching
/// browsers, which snap decoration thickness to whole pixels.
// wkhtmltopdf-rs: unused, decorations are 1px lines as in QtWebKit 2.2 (kept for upstream).
#[allow(dead_code)]
fn decoration_size(
    thickness: &TextDecorationLength,
    metric_size: f32,
    css_font_size: f64,
    scale: f64,
) -> f32 {
    (match thickness {
        GenericTextDecorationLength::LengthPercentage(lp) => {
            lp.resolve(Length::new(css_font_size as f32)).px() as f64 * scale
        }
        GenericTextDecorationLength::FromFont => metric_size as f64,
        GenericTextDecorationLength::Auto => (css_font_size / 10.0).max(1.0) * scale,
    })
    .floor()
    .max(1.0) as f32
}

/// Draws a single decoration line of the given `text-decoration-style` spanning
/// `[x0, x0 + width]`. `baseline` is the line's baseline and `offset` positions the line
/// relative to it (`y = baseline - offset + size / 2`, growing downward). Geometry is built
/// explicitly (rather than relying on backend dash support) so it renders consistently
/// across renderers.
///
/// `double_dir` points away from the text (`+1` = down, `-1` = up) and is used to place the
/// second line of a `double` decoration.
#[allow(clippy::too_many_arguments)]
fn draw_decoration_line(
    scene: &mut impl PaintScene,
    transform: Affine,
    scale: f64,
    x0: f64,
    width: f64,
    baseline: f32,
    offset: f32,
    size: f32,
    brush: &anyrender::Paint,
    double_dir: f64,
    deco_style: TextDecorationStyle,
    inset_start: f64,
    inset_end: f64,
) {
    // Inset the line from the box's start/end edges (assumes LTR: start is the left edge).
    // Negative insets extend the line past the text.
    let x = x0 + inset_start;
    let w = width - inset_start - inset_end;
    if w <= 0.0 {
        return;
    }
    let y = (baseline - offset + size / 2.0) as f64;
    let size = size as f64;
    let butt_stroke = Stroke::new(size).with_caps(Cap::Butt);

    match deco_style {
        TextDecorationStyle::MozNone => {
            // no line. Equivalent to `text-decoration-line: none`
        }
        // `solid`
        TextDecorationStyle::Solid => {
            let line = kurbo::Line::new((x, y), (x + w, y));
            scene.stroke(&butt_stroke, transform, brush, None, &line);
        }
        // Two lines, each `size` thick, separated by a 1px (CSS) gap. This
        // matches Chrome, where the second line is offset by `thickness + 1px`
        // and sits further from the text (below for underline, above for
        // overline).
        TextDecorationStyle::Double => {
            let one_css_px = scale;
            for cy in [y, y + double_dir * (size + one_css_px)] {
                let line = kurbo::Line::new((x, cy), (x + w, cy));
                scene.stroke(&butt_stroke, transform, brush, None, &line);
            }
        }
        // Round dots (diameter = line thickness) spaced one dot apart.
        TextDecorationStyle::Dotted => {
            let radius = size / 2.0;
            let step = 2.0 * size;
            let mut cx = x + radius;
            while cx <= x + w - radius {
                let dot = Circle::new((cx, y), radius);
                scene.fill(Fill::NonZero, transform, brush, None, &dot);
                cx += step;
            }
        }
        // Dashes as filled rectangles. Dash/gap lengths are relative to the
        // thickness (matching Blink): thinner lines use proportionally longer
        // dashes and gaps so they don't read as dotted or solid.
        TextDecorationStyle::Dashed => {
            let (dash_ratio, gap_ratio) = if size >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) };
            let dash = size * dash_ratio;
            let nominal_gap = size * gap_ratio;
            // Nudge the gap so a whole number of dashes fits the run evenly.
            let gap = if w > 2.0 * dash {
                select_best_dash_gap(w, dash, nominal_gap)
            } else {
                nominal_gap
            };
            let mut dx = x;
            while dx < x + w {
                let end = (dx + dash).min(x + w);
                let rect = Rect::new(dx, y - size / 2.0, end, y + size / 2.0);
                scene.fill(Fill::NonZero, transform, brush, None, &rect);
                dx += dash + gap;
            }
        }
        // A squiggle built from alternating quadratic Béziers.
        TextDecorationStyle::Wavy => {
            let amplitude = size;
            let half_wave = (2.0 * size).max(1.0);
            let mut path = BezPath::new();
            path.move_to((x, y));
            let mut px = x;
            let mut up = true;
            while px < x + w {
                let nx = (px + half_wave).min(x + w);
                // A quad Bézier reaches half the control-point offset at its
                // midpoint, so use `2 * amplitude` to hit the target peak.
                let ctrl_y = if up {
                    y - 2.0 * amplitude
                } else {
                    y + 2.0 * amplitude
                };
                path.quad_to(((px + nx) / 2.0, ctrl_y), (nx, y));
                px = nx;
                up = !up;
            }
            let stroke = Stroke::new(size).with_caps(Cap::Round);
            scene.stroke(&stroke, transform, brush, None, &path);
        }
    }
}

/// Paint the decorations accumulated for one line, one line per decorating box.
fn flush_line_decorations(
    scene: &mut impl PaintScene,
    transform: Affine,
    scale: f64,
    deco_boxes: &[LineDecoration],
    win_ascent_ratios: &mut WinAscentCache,
    inline_root_id: NodeId,
    line_baseline: f32,
) {
    // Draw innermost boxes first so ancestors' decorations paint on top, matching the
    // per-run drawing order this replaced (`stack.iter().rev()`).
    for acc in deco_boxes.iter().rev() {
        let deco = &acc.deco;
        // Prefer the decorating box's own font; fall back to the first run it covers.
        let geom = match (&acc.own, &acc.first) {
            (Some(own), _) => own.clone(),
            (None, Some(first)) => {
                let mut geom = first.clone();
                // A descendant run's baseline may be shifted by `vertical-align`, but the
                // decoration is positioned from the decorating box's own baseline. For the
                // inline root that is the line's baseline.
                if acc.node_id == inline_root_id {
                    geom.baseline = line_baseline;
                }
                geom
            }
            (None, None) => continue,
        };
        let geom = &geom;
        let width = acc.max_x - acc.min_x;
        if width <= 0.0 {
            continue;
        }
        let brush = anyrender::Paint::from(deco.color);
        // wkhtmltopdf-rs: QtWebKit 2.2 (`InlineTextBox::paintDecoration`) draws every
        // decoration as a 1px line from the text box's top, with `a` the font's whole-pixel
        // ascent: the overline at the top, the line-through `2 * a / 3` px below it, the
        // underline one pixel below the baseline (reference: typo-decoration).
        let webkit_px = scale as f32;
        let webkit_ascent = ((geom.ascent / webkit_px).ceil()) * webkit_px;
        let _ = win_ascent_ratios;

        // `text-decoration-inset` shortens (or, when negative, extends) the line from the
        // inline-start/end edges. Percentages resolve against the decoration line length
        // (the box's total advance on this line); `auto` is no inset.
        let (inset_start, inset_end) = match &deco.inset {
            GenericTextDecorationInset::LengthPercentage { start, end } => {
                // The extent is in device pixels; convert to CSS pixels so the resolved
                // result can be scaled back.
                let line_length = Length::new((width / scale) as f32);
                (
                    start.resolve(line_length).px() as f64 * scale,
                    end.resolve(line_length).px() as f64 * scale,
                )
            }
            GenericTextDecorationInset::Auto => (0.0, 0.0),
        };

        if deco.line.contains(TextDecorationLine::UNDERLINE) {
            let _ = (&deco.thickness, geom.underline_size);
            let size = webkit_px;

            // `text-underline-offset` moves the underline away from the text. `auto` keeps
            // the font's suggested position; otherwise it resolves against the font size
            // (percentages relative to 1em).
            let extra_underline_offset = deco
                .underline_offset
                .as_ref()
                .map(|lp| lp.resolve(Length::new(geom.css_font_size as f32)).px() as f64 * scale)
                .unwrap_or(0.0);

            // `text-underline-position: under` places the underline below the glyph box
            // (below descenders) rather than at the font's suggested position near the
            // alphabetic baseline. We anchor the top of the line at the descent so it clears
            // descending glyphs like "gqy".
            // An explicit offset under `auto` is taken from the alphabetic
            // baseline, as CSS and Chrome have it, not from the auto gap.
            let base_offset = if deco.underline_under {
                -geom.descent
            } else if deco.underline_offset.is_some() && !deco.underline_from_font {
                0.0
            } else {
                geom.underline_offset
            };
            let _ = (base_offset, extra_underline_offset);
            let offset = -webkit_px;

            // A `double` underline extends downward, away from the text.
            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if deco.line.contains(TextDecorationLine::OVERLINE) {
            // Fonts don't provide a dedicated overline metric, so reuse the underline
            // thickness. The line sits at the top of the "em box": its lower edge rests on
            // the ascent so it clears the glyphs, and it extends upward from there.
            //
            // Browsers use the OS/2 `usWinAscent` for this edge, which is taller than the
            // hhea-based ascent Parley reports; using the smaller value would draw the
            // overline too low. Fall back to Parley's ascent for fonts without a usable
            // OS/2 table.
            let size = webkit_px;
            let offset = webkit_ascent;

            // A `double` overline extends upward, away from the text.
            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                -1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
        if deco.line.contains(TextDecorationLine::LINE_THROUGH) {
            let _ = geom.strikethrough_size;
            let size = webkit_px;
            let a = (webkit_ascent / webkit_px) as i32;
            let offset = (a - 2 * a / 3) as f32 * webkit_px;

            draw_decoration_line(
                scene,
                transform,
                scale,
                acc.min_x,
                width,
                geom.baseline,
                offset,
                size,
                &brush,
                1.0,
                deco.style,
                inset_start,
                inset_end,
            );
        }
    }
}

pub(crate) fn stroke_text<'a>(
    scene: &mut impl PaintScene,
    lines: impl Iterator<Item = Line<'a, TextBrush>>,
    source_text: Option<&str>,
    doc: &BaseDocument,
    transform: Affine,
    scale: f64,
    inline_root_id: NodeId,
    context: &mut DrawTextContext,
) {
    let DrawTextContext {
        stack,
        path_scratch,
        deco_boxes,
        win_ascent_ratios,
        ellipsis_limit,
    } = context;
    let ellipsis_limit = *ellipsis_limit;
    stack.clear();
    path_scratch.clear();
    deco_boxes.clear();

    // Persistent stack mirroring the ancestor path (inline root -> current run's
    // node) as we walk the runs. The `text-decoration-*` properties are *not*
    // inherited; instead a decoration set on an ancestor is propagated to the
    // descendant text it wraps. Rather than re-resolving styles for the whole
    // ancestor chain on every run, we cache each node's resolved values here and,
    // for each run, only resolve styles for the nodes newly descended into (popping
    // as we ascend). `path_scratch` is a reusable buffer for the run's node path.
    for line in lines {
        // Decorations accumulated for this line, keyed by decorating box, so each box is
        // painted once (spanning all its runs) using its own font — matching Firefox, which
        // draws one decoration per box rather than one stepped segment per differently-sized
        // run. Clearing preserves the allocation for the next line and inline context.
        deco_boxes.clear();

        // wkhtmltopdf-rs: `text-overflow: ellipsis` (WebKit 2012): on a line
        // wider than the box, glyphs past (box width - ellipsis width) are
        // not drawn and `…` follows the last drawn glyph, in the font of the
        // line's first run.
        let mut cut: Option<f64> = None;
        let mut ellipsis: Option<(peniko::FontData, f32, u32, f64, Color, NodeId, f32)> = None;
        let mut ellipsis_x = 0.0f64;
        if let Some(limit) = ellipsis_limit {
            if line.metrics().advance as f64 > limit + 0.01 {
                let first = line.items().find_map(|item| match item {
                    PositionedLayoutItem::GlyphRun(g) => Some(g),
                    _ => None,
                });
                if let Some(g) = first {
                    let run = g.run();
                    if let Some((gid, adv)) = ellipsis_glyph(run.font(), run.font_size()) {
                        cut = Some(limit - adv);
                        let color = doc
                            .get_node(g.style().brush.id)
                            .and_then(|n| n.primary_styles())
                            .map(|s| s.clone_color().as_srgb_color())
                            .unwrap_or(Color::BLACK);
                        ellipsis = Some((
                            run.font().clone(),
                            run.font_size(),
                            gid,
                            adv,
                            color,
                            g.style().brush.id,
                            g.baseline(),
                        ));
                    }
                }
            }
        }
        let keep = |x: f32, advance: f32| cut.is_none_or(|c| (x + advance) as f64 <= c + 0.01);

        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(glyph_run) = item {
                // wkhtmltopdf-rs: text whose first family is an `@font-face` that failed to
                // load is laid out with the fallback font but not painted (QtWebKit 2.2).
                if !doc.failed_font_families.is_empty() && in_failed_font(doc, glyph_run.style().brush.id) {
                    continue;
                }
                let run = glyph_run.run();
                // wkhtmltopdf-rs: the run's source text, per glyph; the glyphs of the bidi
                // formatting characters of inline embeddings and overrides (blitz-dom
                // `bidi_controls`) are neither drawn nor text.
                crate::run_text::publish(source_text, &glyph_run, style_brush_id(&glyph_run));
                let mut run_text = crate::run_text::take();
                let hidden: Vec<bool> = run_text.as_ref().map_or_else(Vec::new, |rt| {
                    rt.glyph_ranges
                        .iter()
                        .map(|r| {
                            rt.text.get(r.clone()).is_some_and(|t| {
                                !t.is_empty()
                                    && t.chars().all(|c| matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
                            })
                        })
                        .collect()
                });
                if let Some(rt) = run_text.as_mut() {
                    let mut i = 0;
                    rt.glyph_ranges.retain(|_| {
                        i += 1;
                        !hidden[i - 1]
                    });
                }
                let shown = |i: usize| !hidden.get(i).copied().unwrap_or(false);
                let font = run.font();
                let font_size = run.font_size();
                let metrics = run.font_metrics();
                let style = glyph_run.style();
                let synthesis = run.synthesis();
                let glyph_xform = synthesis
                    .skew()
                    .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

                let css_font_size = font_size as f64 / scale;

                // Reconcile the stack with this run's ancestor path. Build the path
                // (inline root -> run node), keep the shared prefix already on the stack,
                // pop the rest, and resolve styles only for the newly-descended nodes.
                path_scratch.clear();
                let mut walk_id = Some(style.brush.id);
                while let Some(node_id) = walk_id {
                    path_scratch.push(node_id);
                    if node_id == inline_root_id {
                        break;
                    }
                    walk_id = doc.get_node(node_id).and_then(|node| node.parent);
                }
                path_scratch.reverse();

                let shared = stack
                    .iter()
                    .zip(path_scratch.iter())
                    .take_while(|(entry, node_id)| entry.node_id == **node_id)
                    .count();
                stack.truncate(shared);
                for &node_id in &path_scratch[shared..] {
                    stack.push(resolve_decoration_entry(doc, node_id));
                }

                // The glyph colour comes from the run's own node (the stack top): `color`
                // inherits, so the innermost inline element already carries the right value.
                let text_color = stack.last().map(|e| e.text_color).unwrap_or(Color::BLACK);

                let embolden = if FONT_EMBOLDEN_ENABLED {
                    let fs = font_size as f64 / scale;
                    kurbo::Vec2::new((0.015125 * fs).min(0.3), (0.0121 * fs).min(0.3))
                } else {
                    kurbo::Vec2::default()
                };

                let normalized_coords: &[i16] = bytemuck::cast_slice(run.normalized_coords());
                // wkhtmltopdf-rs: a relatively positioned inline element moves its text.
                let transform = transform.pre_translate(relative_inline_offset(doc, style.brush.id, inline_root_id, scale));
                // wkhtmltopdf-rs: `text-shadow` (QtWebKit 2.2 paints it): the glyphs
                // again under the text, offset, in the shadow colour (no blur).
                if let Some(node_styles) = doc.get_node(style.brush.id).and_then(|n| n.primary_styles()) {
                    let current = node_styles.clone_color();
                    for shadow in node_styles.get_inherited_text().text_shadow.0.iter().rev() {
                        let color = shadow.color.resolve_to_absolute(&current).as_srgb_color();
                        if color.components[3] <= 0.0 {
                            continue;
                        }
                        let offset = transform.then_translate(kurbo::Vec2::new(
                            shadow.horizontal.px() as f64 * scale,
                            shadow.vertical.px() as f64 * scale,
                        ));
                        scene.draw_glyphs(
                            font,
                            font_size,
                            !FONT_EMBOLDEN_ENABLED,
                            normalized_coords,
                            embolden,
                            Fill::NonZero,
                            &anyrender::Paint::from(color),
                            1.0,
                            offset,
                            glyph_xform,
                            glyph_run.positioned_glyphs().enumerate().filter(|(i, g)| shown(*i) && keep(g.x, g.advance)).map(|(_, g)| g).map(|glyph| anyrender::Glyph {
                                id: glyph.id as _,
                                x: glyph.x,
                                y: glyph.y,
                            }),
                        );
                    }
                }
                // wkhtmltopdf-rs: publish the run's source text before the glyphs are drawn.
                if let Some(rt) = run_text {
                    crate::run_text::set(rt);
                }
                if cut.is_some() {
                    let kept = glyph_run.positioned_glyphs().enumerate().filter(|(i, g)| shown(*i) && keep(g.x, g.advance)).map(|(_, g)| g).count();
                    if let Some(end) =
                        glyph_run.positioned_glyphs().enumerate().filter(|(i, g)| shown(*i) && keep(g.x, g.advance)).map(|(_, g)| g).map(|g| g.x + g.advance).last()
                    {
                        ellipsis_x = ellipsis_x.max(end as f64);
                    }
                    if let Some(mut rt) = crate::run_text::take() {
                        rt.glyph_ranges.truncate(kept);
                        crate::run_text::set(rt);
                    }
                }
                scene.draw_glyphs(
                    font,
                    font_size,
                    !FONT_EMBOLDEN_ENABLED, // hint
                    normalized_coords,
                    embolden,
                    Fill::NonZero,
                    &anyrender::Paint::from(text_color),
                    1.0, // alpha
                    transform,
                    glyph_xform,
                    glyph_run.positioned_glyphs().enumerate().filter(|(i, g)| shown(*i) && keep(g.x, g.advance)).map(|(_, g)| g).map(|glyph| anyrender::Glyph {
                        id: glyph.id as _,
                        x: glyph.x,
                        y: glyph.y,
                    }),
                );

                // Accumulate this run's contribution to each decorating box on its ancestor
                // path. The decoration is drawn once per box after the whole line has been
                // walked (see `flush_line_decorations`), so mixed font sizes within a box
                // produce a single straight line rather than one stepped segment per run.
                let geometry = DecorationRunGeometry {
                    baseline: glyph_run.baseline(),
                    ascent: metrics.ascent,
                    descent: metrics.descent,
                    underline_offset: metrics.underline_offset,
                    underline_size: metrics.underline_size,
                    strikethrough_size: metrics.strikethrough_size,
                    font: font.clone(),
                    font_size,
                    css_font_size,
                };
                let run_node_id = style.brush.id;
                let run_x0 = glyph_run.offset() as f64;
                let run_x1 = run_x0 + glyph_run.advance() as f64;

                for entry in stack.iter() {
                    if entry.decoration.is_none() {
                        continue;
                    }
                    let idx = match deco_boxes.iter().position(|d| d.node_id == entry.node_id) {
                        Some(idx) => idx,
                        None => {
                            deco_boxes.push(LineDecoration {
                                node_id: entry.node_id,
                                deco: entry.decoration.clone().unwrap(),
                                min_x: f64::INFINITY,
                                max_x: f64::NEG_INFINITY,
                                own: None,
                                first: None,
                            });
                            deco_boxes.len() - 1
                        }
                    };
                    let acc = &mut deco_boxes[idx];
                    acc.min_x = acc.min_x.min(run_x0);
                    acc.max_x = acc.max_x.max(run_x1);
                    if acc.first.is_none() {
                        acc.first = Some(geometry.clone());
                    }
                    // A run whose innermost node is the box itself is the box's own text, so
                    // its font is the one Firefox positions and sizes the decoration with.
                    if entry.node_id == run_node_id {
                        acc.own = Some(geometry.clone());
                    }
                }
            }
        }

        if let Some((font, size, gid, _adv, color, node_id, baseline)) = ellipsis.take() {
            crate::run_text::set(crate::run_text::RunText {
                text: "\u{2026}".to_string(),
                glyph_ranges: vec![0..3],
                node_id,
            });
            scene.draw_glyphs(
                &font,
                size,
                !FONT_EMBOLDEN_ENABLED,
                &[],
                kurbo::Vec2::default(),
                Fill::NonZero,
                &anyrender::Paint::from(color),
                1.0,
                transform,
                None,
                std::iter::once(anyrender::Glyph { id: gid as _, x: ellipsis_x as f32, y: baseline }),
            );
        }

        flush_line_decorations(
            scene,
            transform,
            scale,
            deco_boxes,
            win_ascent_ratios,
            inline_root_id,
            line.metrics().baseline,
        );
    }
}

/// Draw selection highlight rectangles for the given byte range in a layout.
/// Uses Parley's Selection type for accurate geometry calculation.
pub(crate) fn draw_text_selection(
    scene: &mut impl PaintScene,
    layout: &Layout<TextBrush>,
    transform: Affine,
    selection_start: usize,
    selection_end: usize,
) {
    let anchor = Cursor::from_byte_index(layout, selection_start, Affinity::Downstream);
    let focus = Cursor::from_byte_index(layout, selection_end, Affinity::Downstream);
    let selection = Selection::new(anchor, focus);

    selection.geometry_with(layout, |rect, _line_idx| {
        let rect = kurbo::Rect::new(rect.x0, rect.y0, rect.x1, rect.y1);
        scene.fill(Fill::NonZero, transform, SELECTION_COLOR, None, &rect);
    });
}

/// wkhtmltopdf-rs: whether the first family of `node_id`'s font is an `@font-face` family
/// whose font failed to load (`BaseDocument::failed_font_families`).
fn in_failed_font(doc: &BaseDocument, node_id: NodeId) -> bool {
    let Some(styles) = doc.get_node(node_id).and_then(|n| n.primary_styles()) else {
        return false;
    };
    match styles.get_font().font_family.families.list.first() {
        Some(style::values::computed::font::SingleFontFamily::FamilyName(name)) => doc
            .failed_font_families
            .contains(&name.name.as_str().to_lowercase()),
        _ => false,
    }
}
