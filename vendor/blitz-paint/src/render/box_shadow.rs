use super::ElementCx;
use crate::color::{Color, ToColorColor as _};
use anyrender::PaintScene;
use kurbo::{Rect, Vec2};
use peniko::Fill;

impl ElementCx<'_, '_> {
    pub(super) fn draw_outset_box_shadow(&self, scene: &mut impl PaintScene) {
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_outset_shadow = box_shadow.iter().any(|s| !s.inset);
        if !has_outset_shadow {
            return;
        }

        let current_color = self.style.clone_color();
        let opacity = self.style.get_effects().opacity;
        let bg_color = self
            .style
            .get_background()
            .background_color
            .resolve_to_absolute(&current_color)
            .as_srgb_color();
        let bg_is_opaque = bg_color.components[3] >= 1.0;
        let needs_clip = opacity < 1.0 || !bg_is_opaque;

        let max_shadow_rect = box_shadow.iter().fold(Rect::ZERO, |prev, shadow| {
            let x = shadow.base.horizontal.px() as f64 * self.scale;
            let y = shadow.base.vertical.px() as f64 * self.scale;
            let blur = shadow.base.blur.px() as f64 * self.scale;
            let spread = shadow.spread.px() as f64 * self.scale;
            let offset = spread + blur * 2.5;

            let rect = self.frame.border_box.inflate(offset, offset) + Vec2::new(x, y);

            prev.union(rect)
        });

        self.context.layer_manager.maybe_with_layer(
            scene,
            needs_clip,
            1.0,
            self.transform,
            &self.frame.shadow_clip(max_shadow_rect),
            None,
            None,
            |scene| {
                for shadow in box_shadow.iter().filter(|s| !s.inset).rev() {
                    let shadow_color = shadow
                        .base
                        .color
                        .resolve_to_absolute(&current_color)
                        .as_srgb_color();

                    let alpha = shadow_color.components[3];
                    if alpha != 0.0 {
                        let transform = self.transform.then_translate(Vec2 {
                            x: shadow.base.horizontal.px() as f64 * self.scale,
                            y: shadow.base.vertical.px() as f64 * self.scale,
                        });

                        let spread = shadow.spread.px() as f64 * self.scale;
                        let blur = shadow.base.blur.px() as f64;

                        // Without blur or spread the shadow is exactly the element's shape,
                        // so fill the border box path directly (preserving individual radii)
                        if blur == 0.0 && spread == 0.0 {
                            scene.fill(
                                Fill::NonZero,
                                transform,
                                shadow_color,
                                None,
                                &self.frame.border_box_path(),
                            );
                            continue;
                        }

                        // TODO draw shadows with matching individual radii instead of averaging
                        let radius = self.frame.border_radii.average();
                        let rect = self.frame.border_box.inflate(spread, spread);

                        // wkhtmltopdf-rs: a PDF has no blur, and the PDF writer drops
                        // `draw_box_shadow`: approximate the Gaussian with bands.
                        fill_blurred(scene, transform, rect, radius, shadow_color, blur * self.scale, None);
                    }
                }
            },
        )
    }

    pub(super) fn draw_inset_box_shadow(&self, scene: &mut impl PaintScene) {
        let current_color = self.style.clone_color();
        let box_shadow = &self.style.get_effects().box_shadow.0;
        let has_inset_shadow = box_shadow.iter().any(|s| s.inset);
        if !has_inset_shadow {
            return;
        }

        let padding_box = self.frame.padding_box_path();

        for shadow in box_shadow.iter().filter(|s| s.inset) {
            let shadow_color = shadow
                .base
                .color
                .resolve_to_absolute(&current_color)
                .as_srgb_color();
            if shadow_color == Color::TRANSPARENT {
                return;
            }

            // TODO draw shadows with matching individual radii instead of averaging
            let radius = self.frame.border_radii.average();
            let transform = self.transform.then_translate(Vec2 {
                x: shadow.base.horizontal.px() as f64,
                y: shadow.base.vertical.px() as f64,
            });

            // wkhtmltopdf-rs: painted as the padding box minus the (offset,
            // shrunk) hole, without the `DestOut` compositing a PDF cannot express.
            let _ = transform;
            let spread = shadow.spread.px() as f64 * self.scale;
            let offset = Vec2 {
                x: shadow.base.horizontal.px() as f64 * self.scale,
                y: shadow.base.vertical.px() as f64 * self.scale,
            };
            let hole = self.frame.padding_box.inflate(-spread, -spread) + offset;
            scene.push_clip_layer(Fill::NonZero, self.transform, &padding_box);
            fill_blurred(
                scene,
                self.transform,
                hole,
                radius,
                shadow_color,
                shadow.base.blur.px() as f64 * self.scale,
                Some(self.frame.padding_box.inflate(1.0, 1.0)),
            );
            scene.pop_layer();
        }
    }
}

/// wkhtmltopdf-rs: a box shadow without a blur filter. `outer: None` fills
/// `rect` (an outset shadow); `outer: Some(o)` fills `o` minus `rect` (an inset
/// shadow). A blur is approximated by nested rings between the shape grown by
/// -blur and +blur, whose alpha ramps linearly across the blur.
fn fill_blurred(
    scene: &mut impl PaintScene,
    transform: kurbo::Affine,
    rect: Rect,
    radius: f64,
    color: Color,
    blur: f64,
    outer: Option<Rect>,
) {
    use kurbo::Shape as _;
    let rounded = |grow: f64| -> Option<kurbo::BezPath> {
        let r = rect.inflate(grow, grow);
        (r.width() > 0.0 && r.height() > 0.0).then(|| r.to_rounded_rect((radius + grow).max(0.0)).to_path(0.1))
    };
    let alpha = color.components[3];
    if blur <= 0.0 {
        let mut p = outer.map(|o| o.to_path(0.1)).unwrap_or_default();
        if let Some(r) = rounded(0.0) {
            p.extend(r);
        }
        scene.fill(Fill::EvenOdd, transform, color, None, &p);
        return;
    }
    let steps = ((blur * 2.0).ceil() as usize).clamp(2, 16);
    let grow_at = |k: usize| blur - 2.0 * blur * k as f64 / steps as f64;
    // Ring k lies between the shapes grown by grow_at(k) and grow_at(k + 1)
    // (outside to inside); its coverage is the share of the blur behind it.
    for k in 0..steps {
        let t = (k as f64 + 0.5) / steps as f64;
        let coverage = match outer {
            None => t,
            Some(_) => 1.0 - t,
        } as f32;
        let mut p = kurbo::BezPath::new();
        if let Some(a) = rounded(grow_at(k)) {
            p.extend(a);
        }
        if let Some(b) = rounded(grow_at(k + 1)) {
            p.extend(b);
        }
        scene.fill(Fill::EvenOdd, transform, color.with_alpha(alpha * coverage), None, &p);
    }
    // Inside the smallest shape: full shadow (outset) or nothing (inset);
    // outside the largest: nothing (outset) or full shadow (inset).
    match outer {
        None => {
            if let Some(core) = rounded(-blur) {
                scene.fill(Fill::NonZero, transform, color, None, &core);
            }
        }
        Some(o) => {
            let mut p = o.to_path(0.1);
            if let Some(r) = rounded(blur) {
                p.extend(r);
            }
            scene.fill(Fill::EvenOdd, transform, color, None, &p);
        }
    }
}
