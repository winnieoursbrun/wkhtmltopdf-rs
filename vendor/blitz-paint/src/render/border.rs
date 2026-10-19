use anyrender::PaintScene;
use blitz_dom::node::SpecialElementData;
use kurbo::{BezPath, Cap, Circle, Insets, Join, PathEl, Point, Rect, Shape as _, Stroke, Vec2};
use peniko::{Color, Fill};
use smallvec::SmallVec;
use style::{
    computed_values::border_collapse::T as BorderCollapse,
    values::computed::{BorderStyle, OutlineStyle},
};

use crate::{
    color::ToColorColor as _,
    kurbo_css::Edge,
    render::ElementCx,
};

/// A darker version of a colour (mirrors WebKit/Blink's `Color::Dark`): the
/// brightest channel is reduced by a fixed amount and the others scaled to match,
/// preserving hue.
fn darken(color: Color) -> Color {
    let [r, g, b, a] = color.components;
    let v = r.max(g).max(b);
    if v == 0.0 {
        // wkhtmltopdf-rs: WebKit's `darkenedBlack` (0x545454).
        return Color::new([0x54 as f32 / 255.0, 0x54 as f32 / 255.0, 0x54 as f32 / 255.0, a]);
    }
    let multiplier = ((v - 0.33) / v).max(0.0);
    Color::new([r * multiplier, g * multiplier, b * multiplier, a])
}

/// The colour of a single edge of an `inset`/`outset` border.
///
/// An `outset` border is raised: the top/left edges keep the colour and the bottom/right
/// edges are darkened (`inset` is the reverse).
/// wkhtmltopdf-rs: as QtWebKit 2.2 (RenderObject::drawLineForBoxSide) shades them, with
/// `Color::dark` only; Chrome's lightening of dark colours came later (measured on the
/// 0.12.6 binary, tests/fidelity/borders-styles).
fn beveled_edge_color(color: Color, edge: Edge, inset: bool) -> Color {
    let top_or_left = matches!(edge, Edge::Top | Edge::Left);
    if top_or_left == inset { darken(color) } else { color }
}

/// The (outer half, inner half) colours of a single edge of a `groove`/`ridge`
/// border.
///
/// A `groove` looks carved into the page: its outer half is shaded like `inset`
/// and its inner half like `outset`. `ridge` is the reverse (raised).
fn grooved_edge_colors(color: Color, edge: Edge, ridge: bool) -> (Color, Color) {
    let outer = beveled_edge_color(color, edge, !ridge);
    let inner = beveled_edge_color(color, edge, ridge);
    (outer, inner)
}

/// The `(dash, gap)` lengths of a dashed border, as multiples of the border
/// thickness.
///
/// Matches Chrome: borders that are at least 3px use a 2:1 dash-to-gap ratio,
/// while thinner borders use longer dashes and gaps (3:2) so they still read as
/// dashes rather than dots or a solid line. `thickness` and `scale` are in device
/// pixels; the 3px threshold is applied in CSS pixels.
fn dashed_ratios(thickness: f64, scale: f64) -> (f64, f64) {
    if thickness >= 3.0 * scale {
        (2.0, 1.0)
    } else {
        (3.0, 2.0)
    }
}

/// Return `count` points spaced `spacing` apart (by arc length) along `path`,
/// starting at its beginning. Used to place dots along a rounded border.
fn sample_points_along_path(path: &BezPath, spacing: f64, count: usize) -> Vec<Point> {
    // Flatten to a polyline so we can walk it by arc length.
    let mut poly: Vec<Point> = Vec::new();
    kurbo::flatten(path.iter(), 0.1, |el| match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => poly.push(p),
        PathEl::ClosePath => {
            if let Some(&first) = poly.first() {
                poly.push(first);
            }
        }
        _ => {}
    });

    let mut points = Vec::with_capacity(count);
    if poly.len() < 2 {
        return points;
    }

    let mut seg = 0;
    let mut seg_start = 0.0;
    let mut seg_len = (poly[1] - poly[0]).hypot();
    for i in 0..count {
        let target = i as f64 * spacing;
        while seg + 2 < poly.len() && target > seg_start + seg_len {
            seg_start += seg_len;
            seg += 1;
            seg_len = (poly[seg + 1] - poly[seg]).hypot();
        }
        let t = if seg_len > 0.0 {
            ((target - seg_start) / seg_len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        points.push(poly[seg].lerp(poly[seg + 1], t));
    }
    points
}

impl ElementCx<'_, '_> {
    /// Draw all borders for a node
    pub(crate) fn draw_border(&self, scene: &mut impl PaintScene) {
        // wkhtmltopdf-rs: a cell of a collapsed table paints no border of its own;
        // the table paints the resolved collapsed borders (`draw_table_borders`).
        if self.node.collapsed_border.get().is_some() {
            return;
        }
        let style = &*self.style;
        let border = style.get_border();
        let current_color = style.clone_color();

        // Fast path: a uniform-width, single-color solid border whose corners are
        // all circular arcs (equal x and y radius — sharp rectangles, ordinary
        // rounded rectangles and circles all qualify) can be drawn as a single
        // stroke along its center-line instead of a fill per edge. Besides being
        // faster, this works around Vello seam artifacts at the quarter-arc joins
        // when a thin circular/elliptical border is drawn as the
        // border-box/padding-box annulus.
        //
        // A full ellipse (`border-radius: 50%` on a non-square box) has the same
        // seam problem but no rounded-rect representation, so it is stroked as an
        // ellipse.
        let all_solid = border.border_top_style == BorderStyle::Solid
            && border.border_right_style == BorderStyle::Solid
            && border.border_bottom_style == BorderStyle::Solid
            && border.border_left_style == BorderStyle::Solid;
        if all_solid {
            let color = border
                .border_top_color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            let same_color = [
                &border.border_right_color,
                &border.border_bottom_color,
                &border.border_left_color,
            ]
            .iter()
            .all(|c| c.resolve_to_absolute(&current_color).as_srgb_color() == color);
            let bw = self.frame.border_width;
            let uniform_width = bw.x0 == bw.y0 && bw.x0 == bw.x1 && bw.x0 == bw.y1 && bw.x0 > 0.0;
            if same_color && uniform_width && color.components[3] > 0.0 {
                let bb = self.frame.border_box;
                let pb = self.frame.padding_box;
                let width = (bb.width() - pb.width()) / 2.0;
                // Miter join keeps sharp corners square. It has no effect on rounded
                // corners, whose arcs meet the straight edges tangentially.
                let stroke = Stroke::new(width).with_join(Join::Miter);

                if self.frame.is_uniform_corner_border() {
                    let radii = self.frame.border_radii;
                    // The stroke matches the CSS border shape only when each corner is
                    // sharp or its radius is at least the border width (outer radius
                    // `r`, inner radius `r - width`, concentric). For 0 < r < width CSS
                    // draws a rounded outer edge against a sharp inner corner, which a
                    // stroke can't represent — leave those to the per-edge fill.
                    let is_exact = |r: Vec2| r.x == 0.0 || r.x >= width;
                    let stroke_is_exact = is_exact(radii.top_left)
                        && is_exact(radii.top_right)
                        && is_exact(radii.bottom_right)
                        && is_exact(radii.bottom_left);
                    if stroke_is_exact {
                        let half_width = width / 2.0;
                        // Center-line rect, and corner radii shrunk to match.
                        let centerline_box = bb - Insets::uniform(half_width);
                        let centerline_radius = |r: Vec2| (r.x - half_width).max(0.0);
                        let radii = kurbo::RoundedRectRadii::new(
                            centerline_radius(radii.top_left),
                            centerline_radius(radii.top_right),
                            centerline_radius(radii.bottom_right),
                            centerline_radius(radii.bottom_left),
                        );
                        let rr = kurbo::RoundedRect::from_rect(centerline_box, radii);
                        scene.stroke(&stroke, self.transform, color, None, &rr);
                        return;
                    }
                } else if self.frame.is_elliptical_border() {
                    // Center-line radii, straight from the boxes.
                    let rx = (bb.width() + pb.width()) / 4.0;
                    let ry = (bb.height() + pb.height()) / 4.0;
                    let e = kurbo::Ellipse::new(bb.center(), (rx, ry), 0.0);
                    scene.stroke(&stroke, self.transform, color, None, &e);
                    return;
                }
            }
        }

        // (colour, path) pairs to be filled. Several entries may share a colour;
        // they are grouped before filling so that adjacent same-coloured regions
        // are drawn together, avoiding anti-aliasing seams between them.
        //
        // At most 8 entries: 4 edges, each of which can contribute 2 (the outer
        // and inner halves of a `groove`/`ridge`).
        let mut borders: SmallVec<[(Color, BezPath); 8]> = SmallVec::new();

        for &edge in &[Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
            let (color, edge_style) = match edge {
                Edge::Top => (&border.border_top_color, border.border_top_style),
                Edge::Right => (&border.border_right_color, border.border_right_style),
                Edge::Bottom => (&border.border_bottom_color, border.border_bottom_style),
                Edge::Left => (&border.border_left_color, border.border_left_style),
            };
            let color = color.resolve_to_absolute(&current_color).as_srgb_color();

            if color.components[3] <= 0.0 {
                continue;
            }

            match edge_style {
                // `none`/`hidden` produce a zero-width border during layout, but
                // guard against drawing them anyway.
                BorderStyle::None | BorderStyle::Hidden => {}

                // Dashed and dotted edges are drawn immediately as their own
                // (clipped) shapes rather than being batched with the solid edges.
                // wkhtmltopdf-rs: QtWebKit 2.2 strokes a dotted border on a box
                // with rounded corners as a solid line (dashed stays dashed).
                BorderStyle::Dotted if self.frame.border_radii.average() > 0.0 => {
                    borders.push((color, self.frame.border_edge_shape(edge)));
                }
                BorderStyle::Dotted | BorderStyle::Dashed if !self.frame.has_border_radius() => {
                    self.draw_qt_dashed_edge(scene, edge, color, edge_style == BorderStyle::Dotted)
                }
                BorderStyle::Dotted => self.draw_dotted_border_edge(scene, edge, color),
                BorderStyle::Dashed => self.draw_dashed_border_edge(scene, edge, color),

                // A double border is two solid lines separated by a gap, splitting
                // the border width into three equal parts (outer line / gap / inner
                // line). Both lines share a color, so both rings are placed into a
                // single path.
                BorderStyle::Double => {
                    // Needs at least 3px (one device pixel per line and per gap) to
                    // render as two lines; thinner borders fall back to a solid
                    // fill, matching browser behaviour.
                    let path = if self.edge_width(edge) < 3.0 * self.scale {
                        self.frame.border_edge_shape(edge)
                    } else {
                        // wkhtmltopdf-rs: WebKit's lines are `(width + 1) / 3` whole pixels.
                        let w = (self.edge_width(edge) / self.scale).round();
                        let third = ((w + 1.0) / 3.0).floor() / w;
                        let mut path = self
                            .frame
                            .border_slice(0.0, third)
                            .border_edge_shape(edge);
                        path.extend(
                            &self
                                .frame
                                .border_slice(1.0 - third, 1.0)
                                .border_edge_shape(edge),
                        );
                        path
                    };
                    borders.push((color, path));
                }

                // `inset`/`outset` are solid, but each edge is shaded lighter or
                // darker to give a bevelled (3D) appearance.
                BorderStyle::Inset | BorderStyle::Outset => {
                    let inset = edge_style == BorderStyle::Inset;
                    let shade = beveled_edge_color(color, edge, inset);
                    borders.push((shade, self.frame.border_edge_shape(edge)));
                }

                // `groove`/`ridge` split each edge into an outer and an inner half,
                // each shaded as if it were `inset`/`outset`, producing a carved or
                // raised ridge.
                BorderStyle::Groove | BorderStyle::Ridge => {
                    let ridge = edge_style == BorderStyle::Ridge;
                    let (outer, inner) = grooved_edge_colors(color, edge, ridge);
                    // wkhtmltopdf-rs: WebKit splits the edge at `(a + b + 1) / 2` in whole
                    // pixels: the top or left half gets the odd pixel (the outer half of the
                    // top and left edges, the inner half of the bottom and right ones).
                    let w = (self.edge_width(edge) / self.scale).round();
                    let split = if w > 0.0 {
                        let first = (w / 2.0).ceil();
                        if matches!(edge, Edge::Top | Edge::Left) { first / w } else { (w - first) / w }
                    } else {
                        0.5
                    };
                    borders.push((
                        outer,
                        self.frame.border_slice(0.0, split).border_edge_shape(edge),
                    ));
                    borders.push((
                        inner,
                        self.frame.border_slice(split, 1.0).border_edge_shape(edge),
                    ));
                }

                // Solid fills the whole edge region with the border colour.
                BorderStyle::Solid => {
                    borders.push((color, self.frame.border_edge_shape(edge)));
                }
            }
        }

        if borders.is_empty() {
            return;
        }

        // Group together identical colors by sorting, then fill each group as a
        // single path.
        borders.sort_unstable_by(|a, b| {
            a.0.components
                .partial_cmp(&b.0.components)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut start = 0;
        while start < borders.len() {
            let color = borders[start].0;
            let mut path = std::mem::take(&mut borders[start].1);
            let mut next = start + 1;
            while next < borders.len() && borders[next].0 == color {
                path.extend(&borders[next].1);
                next += 1;
            }
            scene.fill(Fill::NonZero, self.transform, color, None, &path);
            start = next;
        }
    }

    /// The border width (in device pixels) of a single edge.
    fn edge_width(&self, edge: Edge) -> f64 {
        match edge {
            Edge::Top => self.frame.border_width.y0,
            Edge::Bottom => self.frame.border_width.y1,
            Edge::Left => self.frame.border_width.x0,
            Edge::Right => self.frame.border_width.x1,
        }
    }

    /// A `dashed` or `dotted` edge of a box with square corners as QtWebKit 2.2 draws it
    /// (GraphicsContext::drawLine on a QPainter): a `w`×`w` square at each end, and between
    /// them a line `w` wide dashed by Qt's pattern from its start, `3w` dashes and gaps
    /// (dotted: `w` and `w`), with flat ends (measured on the 0.12.6 binary,
    /// tests/fidelity/borders-styles).
    fn draw_qt_dashed_edge(&self, scene: &mut impl PaintScene, edge: Edge, color: Color, dotted: bool) {
        qt_dashed_side(scene, self.transform, self.frame.border_box, self.edge_width(edge), edge, color, dotted);
    }
}

/// One `dashed`/`dotted` side `w` wide of the box `bb` as QtWebKit draws it
/// ([`ElementCx::draw_qt_dashed_edge`]).
fn qt_dashed_side(
    scene: &mut impl PaintScene,
    transform: kurbo::Affine,
    bb: Rect,
    w: f64,
    edge: Edge,
    color: Color,
    dotted: bool,
) {
    {
        if w <= 0.0 {
            return;
        }
        let h = w / 2.0;
        let (a, b, start, end) = match edge {
            Edge::Top => (
                Rect::new(bb.x0, bb.y0, bb.x0 + w, bb.y0 + w),
                Rect::new(bb.x1 - w, bb.y0, bb.x1, bb.y0 + w),
                Point::new(bb.x0 + w, bb.y0 + h),
                Point::new(bb.x1 - w, bb.y0 + h),
            ),
            Edge::Bottom => (
                Rect::new(bb.x0, bb.y1 - w, bb.x0 + w, bb.y1),
                Rect::new(bb.x1 - w, bb.y1 - w, bb.x1, bb.y1),
                Point::new(bb.x0 + w, bb.y1 - h),
                Point::new(bb.x1 - w, bb.y1 - h),
            ),
            Edge::Left => (
                Rect::new(bb.x0, bb.y0, bb.x0 + w, bb.y0 + w),
                Rect::new(bb.x0, bb.y1 - w, bb.x0 + w, bb.y1),
                Point::new(bb.x0 + h, bb.y0 + w),
                Point::new(bb.x0 + h, bb.y1 - w),
            ),
            Edge::Right => (
                Rect::new(bb.x1 - w, bb.y0, bb.x1, bb.y0 + w),
                Rect::new(bb.x1 - w, bb.y1 - w, bb.x1, bb.y1),
                Point::new(bb.x1 - h, bb.y0 + w),
                Point::new(bb.x1 - h, bb.y1 - w),
            ),
        };
        scene.fill(Fill::NonZero, transform, color, None, &a);
        scene.fill(Fill::NonZero, transform, color, None, &b);
        if (end - start).hypot() <= 0.0 {
            return;
        }
        // The dashes, as rectangles (one path).
        let dash = if dotted { w } else { 3.0 * w };
        let horizontal = start.y == end.y;
        let len = if horizontal { end.x - start.x } else { end.y - start.y };
        let mut dashes = BezPath::new();
        let mut t = 0.0;
        while t < len {
            let e = (t + dash).min(len);
            let r = if horizontal {
                Rect::new(start.x + t, start.y - h, start.x + e, start.y + h)
            } else {
                Rect::new(start.x - h, start.y + t, start.x + h, start.y + e)
            };
            dashes.extend(kurbo::Shape::path_elements(&r, 0.1));
            t += 2.0 * dash;
        }
        scene.fill(Fill::NonZero, transform, color, None, &dashes);
    }
}

impl ElementCx<'_, '_> {

    /// Draw a single `dashed` border edge.
    ///
    /// The dashes are produced by stroking the centre line of the border with a
    /// dash pattern (butt caps give them flat, square ends). Everything is clipped
    /// to the edge's region (the same trapezoid used by the solid path) so corners
    /// are mitred, adjacent edges of different colors don't overlap, and any
    /// border-radius is respected.
    fn draw_dashed_border_edge(&self, scene: &mut impl PaintScene, edge: Edge, color: Color) {
        let thickness = self.edge_width(edge);
        if thickness <= 0.0 {
            return;
        }

        let (dash_ratio, gap_ratio) = dashed_ratios(thickness, self.scale);

        // Work out the centre line to stroke and the dash/gap lengths along it.
        let (centerline, dash, gap) = if self.frame.has_border_radius() {
            // Rounded corners: stroke the rounded centre line running through the
            // whole perimeter. Every edge uses the same centre line and pattern, so
            // dashes stay continuous and aligned as they wrap around each corner.
            // Dash and gap keep their ratio, sized so a whole number of periods fit
            // exactly around the perimeter (kurbo merges the dash across the seam).
            let mut centerline = self.frame.border_slice(0.0, 0.5).padding_box_path();
            centerline.close_path();
            let perimeter = centerline.perimeter(0.1);
            if perimeter <= 0.0 {
                return;
            }
            let period0 = (dash_ratio + gap_ratio) * thickness;
            let count = (perimeter / period0).round().max(1.0);
            let period = perimeter / count;
            let dash = period * dash_ratio / (dash_ratio + gap_ratio);
            (centerline, dash, period - dash)
        } else {
            // Square corners: stroke a straight line through the middle of the edge,
            // corner to corner. Dash and gap keep their ratio but are scaled so the
            // edge both starts and ends with a dash (covering the corners).
            let bb = self.frame.border_box;
            let half = thickness / 2.0;
            let (start, end, length) = match edge {
                Edge::Top => (
                    Point::new(bb.x0, bb.y0 + half),
                    Point::new(bb.x1, bb.y0 + half),
                    bb.width(),
                ),
                Edge::Bottom => (
                    Point::new(bb.x0, bb.y1 - half),
                    Point::new(bb.x1, bb.y1 - half),
                    bb.width(),
                ),
                Edge::Left => (
                    Point::new(bb.x0 + half, bb.y0),
                    Point::new(bb.x0 + half, bb.y1),
                    bb.height(),
                ),
                Edge::Right => (
                    Point::new(bb.x1 - half, bb.y0),
                    Point::new(bb.x1 - half, bb.y1),
                    bb.height(),
                ),
            };
            if length <= 0.0 {
                return;
            }
            let dash0 = dash_ratio * thickness;
            let gap0 = gap_ratio * thickness;
            // `count` dashes with `count - 1` gaps between them.
            let count = ((length + gap0) / (dash0 + gap0)).round().max(1.0);
            let r = dash_ratio / gap_ratio;
            let gap = length / (count * r + count - 1.0);

            let mut line = BezPath::new();
            line.move_to(start);
            line.line_to(end);
            (line, r * gap, gap)
        };

        let stroke = Stroke::new(thickness)
            .with_caps(Cap::Butt)
            .with_dashes(0.0, [dash, gap]);
        let clip = self.frame.border_edge_shape(edge);
        scene.push_clip_layer(Fill::NonZero, self.transform, &clip);
        scene.stroke(&stroke, self.transform, color, None, &centerline);
        scene.pop_layer();
    }

    /// Draw a single `dotted` border edge.
    ///
    /// Dots are filled circles (diameter == border thickness). Unlike dashes they
    /// can't be produced by stroking, because kurbo doesn't emit zero-length dashes
    /// (so a round-capped dash pattern would render nothing); drawing them
    /// explicitly also lets us anchor a dot in each square corner. Everything is
    /// clipped to the edge's region, as for the other styles.
    fn draw_dotted_border_edge(&self, scene: &mut impl PaintScene, edge: Edge, color: Color) {
        let thickness = self.edge_width(edge);
        if thickness <= 0.0 {
            return;
        }
        let radius = thickness / 2.0;

        let mut path = BezPath::new();
        if self.frame.has_border_radius() {
            // Rounded corners: dots spaced evenly around the rounded centre line so
            // the ring wraps seamlessly around the corners.
            let mut centerline = self.frame.border_slice(0.0, 0.5).padding_box_path();
            centerline.close_path();
            let perimeter = centerline.perimeter(0.1);
            if perimeter <= 0.0 {
                return;
            }
            let count = (perimeter / (2.0 * thickness)).round().max(1.0);
            let spacing = perimeter / count;
            for center in sample_points_along_path(&centerline, spacing, count as usize) {
                path.extend(Circle::new(center, radius).path_elements(0.1));
            }
        } else {
            // Square corners: a dot is anchored in each corner (both ends of the
            // edge, inset by the radius so it sits snugly in the corner) and the
            // rest are spread evenly between them.
            let bb = self.frame.border_box;
            let length = match edge {
                Edge::Top | Edge::Bottom => bb.width(),
                Edge::Left | Edge::Right => bb.height(),
            };
            if length <= 0.0 {
                return;
            }
            let dot_center = |along: f64| -> Point {
                match edge {
                    Edge::Top => Point::new(bb.x0 + along, bb.y0 + radius),
                    Edge::Bottom => Point::new(bb.x0 + along, bb.y1 - radius),
                    Edge::Left => Point::new(bb.x0 + radius, bb.y0 + along),
                    Edge::Right => Point::new(bb.x1 - radius, bb.y0 + along),
                }
            };

            let span = length - thickness;
            if span <= 0.0 {
                // Edge too short to fit two dots; place a single centred dot.
                let center = dot_center(length / 2.0);
                path.extend(Circle::new(center, radius).path_elements(0.1));
            } else {
                // Aim for a centre-to-centre spacing of about two dot diameters,
                // then adjust it so the first and last dots land in the corners.
                let gaps = (span / (2.0 * thickness)).round().max(1.0) as usize;
                let spacing = span / gaps as f64;
                for i in 0..=gaps {
                    let center = dot_center(radius + i as f64 * spacing);
                    path.extend(Circle::new(center, radius).path_elements(0.1));
                }
            }
        }

        let clip = self.frame.border_edge_shape(edge);
        scene.push_clip_layer(Fill::NonZero, self.transform, &clip);
        scene.fill(Fill::NonZero, self.transform, color, None, &path);
        scene.pop_layer();
    }

    /// wkhtmltopdf-rs: the collapsing border model as QtWebKit 2.2 paints it:
    /// every cell edge carries its resolved collapsed border (`collapse_borders`
    /// in blitz-dom's `layout/table.rs`), straddling the cell boundary with the
    /// extra pixel inside the cell below/right of it.
    pub(crate) fn draw_table_borders(&self, scene: &mut impl PaintScene) {
        let SpecialElementData::TableRoot(table) = &self.element.special_data else {
            return;
        };
        if table.border_collapse != BorderCollapse::Collapse {
            return;
        }
        let scale = self.scale;
        let transform = self.transform;
        let fill_rect = |scene: &mut _, x0: f64, y0: f64, x1: f64, y1: f64, color: Color| {
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            let shape = Rect::new(x0, y0, x1, y1).scale_from_origin(scale);
            PaintScene::fill(scene, Fill::NonZero, transform, color, None, &shape);
        };
        // An edge in its style, as WebKit's Qt graphics context strokes collapsed borders
        // (`drawLine`): dots and dashes as on a box with square corners (one border width
        // square at each end of the cell's edge, then dashes of one width for dots and three
        // for dashes, gaps as long; measured on the 0.12.6 binary), two thirds for double.
        let fill = |scene: &mut _, x0: f64, y0: f64, x1: f64, y1: f64, style: BorderStyle, color: Color| {
            let horizontal = x1 - x0 >= y1 - y0;
            let width = if horizontal { y1 - y0 } else { x1 - x0 };
            if matches!(style, BorderStyle::Dotted | BorderStyle::Dashed) && width > 0.0 {
                let bb = Rect::new(x0, y0, x1, y1).scale_from_origin(scale);
                let edge = if horizontal { Edge::Top } else { Edge::Left };
                qt_dashed_side(scene, transform, bb, width * scale, edge, color, style == BorderStyle::Dotted);
            } else if style == BorderStyle::Double && width >= 3.0 {
                let third = (width / 3.0).round();
                if horizontal {
                    fill_rect(scene, x0, y0, x1, y0 + third, color);
                    fill_rect(scene, x0, y1 - third, x1, y1, color);
                } else {
                    fill_rect(scene, x0, y0, x0 + third, y1, color);
                    fill_rect(scene, x1 - third, y0, x1, y1, color);
                }
            } else {
                fill_rect(scene, x0, y0, x1, y1, color);
            }
        };
        for cell in &table.cells {
            let Some(edges) = &cell.collapsed else { continue };
            let Some(node) = self.context.dom.get_node(cell.node_id) else { continue };
            let l = node.final_layout();
            let (x, y) = (l.location.x as f64, l.location.y as f64);
            let (w, h) = (l.size.width as f64, l.size.height as f64);
            let [top, right, bottom, left] = edges;
            let color = |c: style::color::AbsoluteColor| c.as_srgb_color();
            // Each edge reaches into the neighbour by its lower half.
            let (lx0, rx1) = (x - left.outer_half(), x + w + right.inner_half());
            if top.is_painted() {
                fill(scene, lx0, y - top.outer_half(), rx1, y + top.inner_half(), top.style, color(top.color));
            }
            if bottom.is_painted() {
                fill(scene, lx0, y + h - bottom.outer_half(), rx1, y + h + bottom.inner_half(), bottom.style, color(bottom.color));
            }
            if left.is_painted() {
                fill(scene, x - left.outer_half(), y, x + left.inner_half(), y + h, left.style, color(left.color));
            }
            if right.is_painted() {
                fill(scene, x + w - right.outer_half(), y, x + w + right.inner_half(), y + h, right.style, color(right.color));
            }
        }
    }

    /// ❌ dotted - Defines a dotted border
    /// ❌ dashed - Defines a dashed border
    /// ✅ solid - Defines a solid border
    /// ❌ double - Defines a double border
    /// ❌ groove - Defines a 3D grooved border. The effect depends on the border-color value
    /// ❌ ridge - Defines a 3D ridged border. The effect depends on the border-color value
    /// ❌ inset - Defines a 3D inset border. The effect depends on the border-color value
    /// ❌ outset - Defines a 3D outset border. The effect depends on the border-color value
    /// ✅ none - Defines no border
    /// ✅ hidden - Defines a hidden border
    pub(crate) fn draw_outline(&self, scene: &mut impl PaintScene) {
        let outline = self.style.get_outline();

        let current_color = self.style.clone_color();
        let color = outline
            .outline_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();

        let style = match outline.outline_style {
            OutlineStyle::Auto => return,
            OutlineStyle::BorderStyle(style) => style,
        };

        if matches!(style, BorderStyle::None | BorderStyle::Hidden) {
            return;
        }
        // wkhtmltopdf-rs: QtWebKit 2.2 draws outlines as plain rectangles (no
        // border-radius) outside the border box, `outline-offset` away from it.
        let width = self.frame.outline_width;
        if width <= 0.0 {
            return;
        }
        let offset = outline.outline_offset.to_f64_px() * self.scale;
        let inner = self.frame.border_box.inflate(offset, offset);
        let outer = inner.inflate(width, width);
        // Each side as QtWebKit draws a border side of that style (drawLineForBoxSide).
        match style {
            BorderStyle::Dashed | BorderStyle::Dotted => {
                for edge in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
                    qt_dashed_side(scene, self.transform, outer, width, edge, color, style == BorderStyle::Dotted);
                }
            }
            BorderStyle::Double if width >= 3.0 * self.scale => {
                let w = (width / self.scale).round();
                let third = ((w + 1.0) / 3.0).floor() * self.scale;
                for (o, i) in [(outer, outer.inflate(-third, -third)), (inner.inflate(third, third), inner)] {
                    let mut path = o.to_path(0.1);
                    path.extend(i.to_path(0.1));
                    scene.fill(Fill::EvenOdd, self.transform, color, None, &path);
                }
            }
            _ => {
                let mut path = outer.to_path(0.1);
                path.extend(inner.to_path(0.1));
                scene.fill(Fill::EvenOdd, self.transform, color, None, &path);
            }
        }
    }
}
