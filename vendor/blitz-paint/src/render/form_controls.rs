use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use anyrender::PaintScene;
use blitz_dom::local_name;
use kurbo::{Affine, Arc, BezPath, Circle, Join, Point, Rect, Shape as _, Stroke, Vec2};
use peniko::Fill;
use style::dom::TElement as _;

/// The colours of Qt's Windows style (`QWindowsStyle`, the default style of the patched Qt
/// in wkhtmltopdf 0.12.6), as its bevels print.
const QT_LIGHT: Color = Color::from_rgba8(0xd4, 0xd0, 0xc8, 255);
const QT_MID: Color = Color::from_rgba8(0x80, 0x80, 0x80, 255);
const QT_DARK: Color = Color::from_rgba8(0x40, 0x40, 0x40, 255);
const QT_BLACK: Color = Color::from_rgba8(0, 0, 0, 255);
const QT_SHADOW: Color = Color::from_rgba8(0x6a, 0x68, 0x64, 255);

/// The border colour the user-agent style sheet gives push buttons
/// (`rgba(1, 2, 3, 0)`, wk-webkit's `QT_FORM_CONTROLS`): their 2px border is not drawn by
/// CSS but as Qt's raised bevel ([`ElementCx::draw_qt_button_bevel`]). An author border
/// colour replaces it, and the button is then drawn by CSS as in WebKit.
fn is_qt_bevel_sentinel(c: Color) -> bool {
    let c = c.to_rgba8();
    (c.r, c.g, c.b, c.a) == (1, 2, 3, 0)
}

/// The border colour the user-agent style sheet gives text fields, text areas and list
/// boxes (`rgba(1, 2, 4, 0)`): their 1px border is drawn as Qt's frame
/// ([`ElementCx::draw_qt_button_bevel`]).
fn is_qt_frame_sentinel(c: Color) -> bool {
    let c = c.to_rgba8();
    (c.r, c.g, c.b, c.a) == (1, 2, 4, 0)
}

impl ElementCx<'_, '_> {
    /// Fill `rect` given in units of `unit` css px from `origin`.
    fn fill_units(&self, scene: &mut impl PaintScene, origin: Point, unit: f64, color: Color, r: Rect) {
        let t = self.transform * Affine::translate(origin.to_vec2()) * Affine::scale(unit);
        scene.fill(Fill::NonZero, t, color, None, &r);
    }

    pub(super) fn draw_input(&self, scene: &mut impl PaintScene) {
        if self.node.local_name() != "input" {
            return;
        }
        if self.node.attr(local_name!("type")) == Some("search")
            && self.node.attr(local_name!("value")).is_some_and(|v| !v.is_empty())
        {
            self.draw_search_cancel(scene);
            return;
        }
        let Some(checked) = self.element.checkbox_input_checked() else {
            return;
        };

        let type_attr = self.node.attr(local_name!("type"));
        // The check mark and the radio dot use the element's colour (white with
        // `--enable-forms`, which sets it on every input).
        let mark = self.style.clone_color().as_srgb_color();

        // wkhtmltopdf-rs: QWindowsStyle's indicators as QtWebKit 2.2 prints them (measured on
        // the 0.12.6 binary, tests/fidelity/forms-controls): a 12-unit sunken frame scaled to the
        // 13px box (checkbox), an 11-unit sunken ring scaled to the 12px box (radio). A disabled
        // control prints like an enabled one.
        let rect = self.frame.border_box;
        let origin = rect.origin();
        match type_attr {
            Some("checkbox") => {
                let u = rect.width().min(rect.height()) / 12.0;
                for (color, r) in [
                    (QT_MID, Rect::new(0.0, 0.0, 1.0, 12.0)),
                    (QT_MID, Rect::new(1.0, 0.0, 12.0, 1.0)),
                    (QT_LIGHT, Rect::new(1.0, 11.0, 12.0, 12.0)),
                    (QT_LIGHT, Rect::new(11.0, 1.0, 12.0, 12.0)),
                    (QT_DARK, Rect::new(1.0, 1.0, 2.0, 11.0)),
                    (QT_DARK, Rect::new(2.0, 1.0, 11.0, 2.0)),
                    (Color::WHITE, Rect::new(2.0, 10.0, 11.0, 11.0)),
                    (Color::WHITE, Rect::new(10.0, 2.0, 11.0, 11.0)),
                    (Color::WHITE, Rect::new(2.0, 2.0, 10.0, 10.0)),
                ] {
                    self.fill_units(scene, origin, u, color, r);
                }
                if checked {
                    let mut path = BezPath::new();
                    path.move_to((3.0, 5.5));
                    path.line_to((3.0, 8.0));
                    path.line_to((5.5, 10.5));
                    path.line_to((10.0, 5.5));
                    path.line_to((10.0, 2.5));
                    path.line_to((5.5, 7.5));
                    path.close_path();
                    let t = self.transform * Affine::translate(origin.to_vec2()) * Affine::scale(u);
                    scene.fill(Fill::NonZero, t, mark, None, &path);
                    scene.stroke(&Stroke::new(1.0).with_join(Join::Miter), t, mark, None, &path);
                }
            }
            Some("radio") => {
                let u = rect.width().min(rect.height()) / 11.0;
                let c = Point::new(5.5, 5.5);
                let t = self.transform * Affine::translate(origin.to_vec2()) * Affine::scale(u);
                // A half disc on the top-left (`first`) or bottom-right side of the
                // diagonal from the bottom-left to the top-right.
                let half = |r: f64, first: bool| {
                    let start = -std::f64::consts::FRAC_PI_4;
                    let sweep = if first { -std::f64::consts::PI } else { std::f64::consts::PI };
                    let arc = Arc::new(c, Vec2::new(r, r), start, sweep, 0.0);
                    let mut p = BezPath::new();
                    p.move_to(c);
                    p.line_to(c + Vec2::from_angle(start) * r);
                    for el in arc.append_iter(0.05) {
                        p.push(el);
                    }
                    p.close_path();
                    p
                };
                for (color, r, first) in [
                    (QT_MID, 5.5, true),
                    (Color::WHITE, 5.5, false),
                    (QT_DARK, 4.5, true),
                    (QT_LIGHT, 4.5, false),
                ] {
                    scene.fill(Fill::NonZero, t, color, None, &half(r, first));
                }
                scene.fill(Fill::NonZero, t, Color::WHITE, None, &Circle::new(c, 3.5).to_path(0.05));
                if checked {
                    scene.fill(Fill::NonZero, t, mark, None, &Circle::new(c, 1.5).to_path(0.05));
                }
            }
            _ => {}
        }
    }

    /// The cancel button of a non-empty search field (RenderThemeQt's 9px dark cross, 1px
    /// from the right border, centred).
    fn draw_search_cancel(&self, scene: &mut impl PaintScene) {
        let r = self.frame.border_box;
        let o = Point::new(r.x1 - 10.0, r.y0 + ((r.height() - 9.0) / 2.0).round());
        let t = self.transform * Affine::translate(o.to_vec2());
        let mut cross = BezPath::new();
        cross.move_to((1.5, 1.5));
        cross.line_to((7.5, 7.5));
        cross.move_to((7.5, 1.5));
        cross.line_to((1.5, 7.5));
        let ink = Color::from_rgba8(0x1f, 0x1f, 0x1f, 0xe0);
        scene.stroke(&Stroke::new(1.3), t, ink, None, &cross);
    }

    /// For a `<fieldset>` whose first element child is a `<legend>` taller than its top
    /// border: how far down its top border is drawn (centred on the legend), and the
    /// legend's span (in border-box coordinates), where the border is not drawn.
    pub(super) fn fieldset_legend(&self) -> Option<(f64, Rect)> {
        if self.node.local_name() != "fieldset" {
            return None;
        }
        let dom = self.context.dom;
        let legend = self
            .node
            .children
            .iter()
            .filter_map(|&id| dom.get_node(id))
            .find(|n| n.is_element())
            .filter(|n| n.local_name() == "legend")?;
        let l = legend.final_layout();
        let border_top = self.frame.border_width.y0;
        let (y, h) = (f64::from(l.location.y), f64::from(l.size.height));
        if h <= border_top {
            return None;
        }
        let offset = (y + (h - border_top) / 2.0).floor().max(0.0);
        let x = self.frame.border_box.x0 + f64::from(l.location.x);
        let gap = Rect::new(x, self.frame.border_box.y0, x + f64::from(l.size.width), self.frame.border_box.y0 + y + h);
        Some((offset, gap))
    }

    /// Qt's raised push button bevel on a button whose border is the user-agent one: white
    /// and black outside, `#d4d0c8` and `#6a6864` inside (top-left, bottom-right), each
    /// 1px, as QWindowsStyle prints it (measured on the 0.12.6 binary).
    pub(super) fn draw_qt_button_bevel(&self, scene: &mut impl PaintScene) {
        let border = self.style.get_border();
        if self.node.final_layout().border.top < 1.0 {
            return;
        }
        let current = self.style.clone_color();
        let top = border.border_top_color.resolve_to_absolute(&current).as_srgb_color();
        if is_qt_frame_sentinel(top) {
            // A text field's frame as QtWebKit 2.2 prints it: a black line 0.7px wide on the
            // border box's edge (measured on the 0.12.6 binary).
            let r = self.frame.border_box;
            scene.stroke(&Stroke::new(0.7), self.transform, QT_BLACK, None, &r);
            return;
        }
        if !is_qt_bevel_sentinel(top) {
            return;
        }
        let r = self.frame.border_box;
        let (w, h) = (r.width(), r.height());
        if w < 4.0 || h < 4.0 {
            return;
        }
        let o = r.origin();
        for (color, rect) in [
            (Color::WHITE, Rect::new(0.0, 0.0, 1.0, h - 1.0)),
            (Color::WHITE, Rect::new(0.0, 0.0, w - 1.0, 1.0)),
            (QT_BLACK, Rect::new(0.0, h - 1.0, w, h)),
            (QT_BLACK, Rect::new(w - 1.0, 0.0, w, h)),
            (QT_LIGHT, Rect::new(1.0, 1.0, 2.0, h - 2.0)),
            (QT_LIGHT, Rect::new(1.0, 1.0, w - 2.0, 2.0)),
            (QT_SHADOW, Rect::new(1.0, h - 2.0, w - 1.0, h - 1.0)),
            (QT_SHADOW, Rect::new(w - 2.0, 1.0, w - 1.0, h - 1.0)),
        ] {
            self.fill_units(scene, o, 1.0, color, rect);
        }
    }
}
