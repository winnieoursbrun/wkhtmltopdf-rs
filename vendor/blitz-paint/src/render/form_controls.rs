use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use anyrender::PaintScene;
use blitz_dom::local_name;
use kurbo::{Affine, BezPath, Cap, Circle, Join, Stroke, Vec2};
use peniko::Fill;
use style::dom::TElement as _;

impl ElementCx<'_, '_> {
    pub(super) fn draw_input(&self, scene: &mut impl PaintScene) {
        if self.node.local_name() != "input" {
            return;
        }
        let Some(checked) = self.element.checkbox_input_checked() else {
            return;
        };

        let type_attr = self.node.attr(local_name!("type"));
        let disabled = self.node.attr(local_name!("disabled")).is_some();

        // TODO this should be coming from css accent-color, but I couldn't find how to retrieve it
        let accent_color = if disabled {
            Color::from_rgba8(209, 209, 209, 255)
        } else {
            self.style.clone_color().as_srgb_color()
        };

        // wkhtmltopdf-rs: Qt's default style as QtWebKit 2.2 prints it
        // (tests/parts/forms): a white box or circle (12 px across) with a
        // 0.7 px black outline centred on its edge; the check mark and the
        // radio dot use the element's colour.
        let rect = self.frame.border_box;
        let outline = Stroke::new(0.7);
        let black = Color::from_rgba8(0, 0, 0, 255);
        match type_attr {
            Some("checkbox") => {
                let r = rect.to_rounded_rect(0.0);
                scene.fill(Fill::NonZero, self.transform, Color::WHITE, None, &r);
                scene.stroke(&outline, self.transform, black, None, &r);
                if checked {
                    let s = rect.width().min(rect.height()) / 13.0;
                    let mut path = BezPath::new();
                    path.move_to((3.0, 6.5));
                    path.line_to((5.5, 9.0));
                    path.line_to((10.0, 3.5));
                    path.apply_affine(Affine::translate(Vec2 { x: rect.x0, y: rect.y0 }) * Affine::scale(s));
                    let style = Stroke::new(2.0 * s).with_caps(Cap::Butt).with_join(Join::Miter);
                    scene.stroke(&style, self.transform, accent_color, None, &path);
                }
            }
            Some("radio") => {
                let center = rect.center();
                let radius = (rect.width().min(rect.height()) - 1.0).max(0.0) / 2.0;
                let ring = Circle::new(center, radius);
                scene.fill(Fill::NonZero, self.transform, Color::WHITE, None, &ring);
                scene.stroke(&outline, self.transform, black, None, &ring);
                if checked {
                    let dot = Circle::new(center, radius / 2.0);
                    scene.fill(Fill::NonZero, self.transform, accent_color, None, &dot);
                }
            }
            _ => {}
        }
    }
}
