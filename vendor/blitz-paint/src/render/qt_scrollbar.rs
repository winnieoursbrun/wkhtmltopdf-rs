//! wkhtmltopdf-rs: the scroll bars of a box that scrolls (`overflow: scroll`, or `auto` with
//! overflowing content), as QtWebKit 2.2 prints them: QWindowsStyle's `CC_ScrollBar`
//! (`ScrollbarThemeQt`), 16px thick, inside the padding box (layout reserves the room, see
//! blitz-dom's `layout_with_qt_scrollbars`). Measured on the 0.12.6 binary
//! (tests/fidelity/overflow).

use super::ElementCx;
use crate::color::Color;
use anyrender::PaintScene;
use blitz_dom::node::{QT_SCROLLBAR_EXTENT, QT_SCROLLBAR_HORIZONTAL, QT_SCROLLBAR_VERTICAL};
use kurbo::{Affine, BezPath, Rect, Shape as _, Stroke};
use peniko::Fill;

const BUTTON_FACE: Color = Color::from_rgba8(0xd4, 0xd0, 0xc8, 255);
const BUTTON_SHADOW: Color = Color::from_rgba8(0x6a, 0x68, 0x64, 255);
const BLACK: Color = Color::from_rgba8(0, 0, 0, 255);
const ARROW_DISABLED: Color = Color::from_rgba8(160, 160, 164, 255);

/// QWindowsStyle's `PM_ScrollBarSliderMin` as the 0.12.6 binary prints it.
const MIN_THUMB: i64 = 9;

/// The 8x8 arrow images of the scroll bar buttons (`#`: the arrow, `G`/`W`: the gray arrow
/// and its white shadow of a disabled scroll bar), as the 0.12.6 binary embeds them.
const ARROW_UP: [&str; 8] = [
    "........", "........", "...#....", "..###...", ".#####..", "#######.", "........", "........",
];
const ARROW_DOWN: [&str; 8] = [
    "........", "........", "#######.", ".#####..", "..###...", "...#....", "........", "........",
];
const ARROW_LEFT: [&str; 8] = [
    ".....#..", "....##..", "...###..", "..####..", "...###..", "....##..", "........", "........",
];
const ARROW_RIGHT: [&str; 8] = [
    "..#.....", "..##....", "..###...", "..####..", "..###...", "..##....", "..#.....", "........",
];
const ARROW_UP_DISABLED: [&str; 8] = [
    "........", "........", "...G....", "..GGG...", ".GGGGG..", "GGGGGGG.", ".WWWWWWW", "........",
];
const ARROW_DOWN_DISABLED: [&str; 8] = [
    "........", "........", "GGGGGGG.", ".GGGGGWW", "..GGGWW.", "...GWW..", "....W...", "........",
];
const ARROW_LEFT_DISABLED: [&str; 8] = [
    ".....G..", "....GGW.", "...GGGW.", "..GGGGW.", "...GGGW.", "....GGW.", ".....WW.", "........",
];
const ARROW_RIGHT_DISABLED: [&str; 8] = [
    "..G.....", "..GG....", "..GGG...", "..GGGG..", "..GGGWW.", "..GGWW..", "..GWW...", "...W....",
];

impl ElementCx<'_, '_> {
    /// The box's content clip with scroll bars: its padding box without them
    /// (`RenderBox::overflowClipRect`).
    pub(super) fn qt_scrollport_path(&self) -> BezPath {
        let bars = self.node.qt_scrollbars.get();
        let ext = f64::from(QT_SCROLLBAR_EXTENT) * self.scale;
        let mut r = self.frame.padding_box;
        if bars & QT_SCROLLBAR_VERTICAL != 0 {
            r.x1 = (r.x1 - ext).max(r.x0);
        }
        if bars & QT_SCROLLBAR_HORIZONTAL != 0 {
            r.y1 = (r.y1 - ext).max(r.y0);
        }
        r.to_path(0.1)
    }

    /// Paint the box's scroll bars (and the white corner between two of them) over its
    /// padding box, unscrolled.
    pub(super) fn draw_qt_scrollbars(&self, scene: &mut impl PaintScene) {
        let bars = self.node.qt_scrollbars.get();
        if bars == 0 {
            return;
        }
        let layout = self.node.final_layout();
        let s = self.scale;
        let pb = self.frame.padding_box;
        // The padding box in CSS px.
        let (x0, y0) = (pb.x0 / s, pb.y0 / s);
        let (w, h) = (pb.width() / s, pb.height() / s);
        let ext = f64::from(QT_SCROLLBAR_EXTENT);
        let vertical = bars & QT_SCROLLBAR_VERTICAL != 0;
        let horizontal = bars & QT_SCROLLBAR_HORIZONTAL != 0;
        let client_w = (w - if vertical { ext } else { 0.0 }).max(0.0);
        let client_h = (h - if horizontal { ext } else { 0.0 }).max(0.0);
        let overflow = layout.scrollable_overflow_rect;
        let t = self.transform * Affine::scale(s);
        if horizontal {
            let origin = Affine::translate((x0, y0 + h - ext));
            draw_bar(scene, t * origin, client_w, client_w, f64::from(overflow.right), false);
        }
        if vertical {
            let origin = Affine::translate((x0 + w - ext, y0));
            draw_bar(scene, t * origin, client_h, client_h, f64::from(overflow.bottom), true);
        }
        if horizontal && vertical {
            let corner = Rect::new(x0 + client_w, y0 + client_h, x0 + w, y0 + h);
            scene.fill(Fill::NonZero, t, Color::WHITE, None, &corner);
        }
    }
}

/// One scroll bar `len` long, drawn in its own coordinates (`t` maps them to the page; a
/// vertical bar is drawn as a horizontal one with the axes swapped), scrolled to its start.
fn draw_bar(scene: &mut impl PaintScene, t: Affine, len: f64, visible: f64, total: f64, vertical: bool) {
    // Swap the axes of a vertical bar: `(along, across)` coordinates.
    let page = t;
    let t = if vertical { t * Affine::new([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]) } else { t };
    let ext = f64::from(QT_SCROLLBAR_EXTENT);
    scene.fill(Fill::NonZero, t, BUTTON_FACE, None, &Rect::new(0.0, 0.0, len, ext));

    let visible = visible.round() as i64;
    let range = (total.round() as i64 - visible).max(0);
    let enabled = range > 0;
    let button = ext.min((len / 2.0).floor());
    // The buttons, with their arrows.
    let (sub, add) = match (vertical, enabled) {
        (false, true) => (ARROW_LEFT, ARROW_RIGHT),
        (false, false) => (ARROW_LEFT_DISABLED, ARROW_RIGHT_DISABLED),
        (true, true) => (ARROW_UP, ARROW_DOWN),
        (true, false) => (ARROW_UP_DISABLED, ARROW_DOWN_DISABLED),
    };
    for (start, arrow) in [(0.0, sub), (len - button, add)] {
        bevel(scene, t, start, button);
        // The arrow images are drawn in page axes, 4px into the button.
        let arrow_origin = if vertical {
            Affine::translate((4.0, start + 4.0))
        } else {
            Affine::translate((start + 4.0, 4.0))
        };
        draw_arrow(scene, page * arrow_origin, &arrow);
    }

    // The track between the buttons, and the thumb at its start.
    let track0 = button;
    let track1 = len - button;
    if track1 <= track0 {
        return;
    }
    if !enabled {
        dense4(scene, t, page, Rect::new(track0, 0.0, track1, ext), BLACK, vertical);
        return;
    }
    // QCommonStyle's `SC_ScrollBarSlider` with WebKit's page step (`RenderLayer`:
    // 87.5% of the visible size, at most 40px less).
    let maxlen = (track1 - track0) as i64;
    let page_step = ((visible as f64 * 0.875) as i64).max(visible - 40).max(1);
    let thumb = ((page_step * maxlen) / (range + page_step)).max(MIN_THUMB).min(maxlen);
    let thumb1 = track0 + thumb as f64;
    if thumb1 < track1 {
        dense4(scene, t, page, Rect::new(thumb1, 0.0, track1, ext), BUTTON_FACE, vertical);
    }
    bevel(scene, t, track0, thumb as f64);
}

/// QWindowsStyle's raised bevel (`qDrawWinButton`) of a button or thumb from `start`,
/// `len` long along the bar: cosmetic lines, light and black outside, white and dark
/// gray inside.
fn bevel(scene: &mut impl PaintScene, t: Affine, start: f64, len: f64) {
    if len < 4.0 {
        return;
    }
    let (a0, a1) = (start, start + len - 1.0);
    let line = |pts: [(f64, f64); 3]| {
        let mut p = BezPath::new();
        p.move_to(pts[0]);
        p.line_to(pts[1]);
        p.line_to(pts[2]);
        p
    };
    // (along, across) points; the across axis is 0..15.
    for (color, pts) in [
        (BUTTON_FACE, [(a1 - 1.0, 0.0), (a0, 0.0), (a0, 14.0)]),
        (BLACK, [(a1, 0.0), (a1, 15.0), (a0, 15.0)]),
        (Color::WHITE, [(a1 - 2.0, 1.0), (a0 + 1.0, 1.0), (a0 + 1.0, 13.0)]),
        (BUTTON_SHADOW, [(a1 - 1.0, 1.0), (a1 - 1.0, 14.0), (a0 + 1.0, 14.0)]),
    ] {
        scene.stroke(&Stroke::new(0.0), t, color, None, &line(pts));
    }
    let face = Rect::new(a0 + 2.0, 2.0, a1 - 1.0, 14.0);
    scene.fill(Fill::NonZero, t, BUTTON_FACE, None, &face);
}

/// An 8x8 arrow image at `t`.
fn draw_arrow(scene: &mut impl PaintScene, t: Affine, rows: &[&str; 8]) {
    for (color, key) in [(BLACK, b'#'), (ARROW_DISABLED, b'G'), (Color::WHITE, b'W')] {
        let mut path = BezPath::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.bytes().enumerate() {
                if c == key {
                    let r = Rect::new(x as f64, y as f64, x as f64 + 1.0, y as f64 + 1.0);
                    path.extend(r.path_elements(0.1));
                }
            }
        }
        if !path.elements().is_empty() {
            scene.fill(Fill::NonZero, t, color, None, &path);
        }
    }
}

/// A scroll bar groove: `background` under white `Qt::Dense4Pattern`, a checkerboard of
/// 2px squares centred on the even coordinates of the bar, white where the coordinates'
/// halves add up to an even number. The 0.12.6 binary writes it as an 8x8 tiling pattern
/// anchored at the bar's corner; it is drawn here as a repeated 8x8 image (which the PDF
/// writer turns into a tiling pattern of the same size). `rect` is in (along, across)
/// coordinates, `t` maps them to the page, `page` maps the bar's page axes.
pub(super) fn dense4(scene: &mut impl PaintScene, t: Affine, page: Affine, rect: Rect, background: Color, vertical: bool) {
    scene.fill(Fill::NonZero, t, background, None, &rect);
    let page_rect = if vertical { Rect::new(rect.y0, rect.x0, rect.y1, rect.x1) } else { rect };
    scene.fill(Fill::NonZero, page, dense4_tile(), None, &page_rect);
}

/// The 8x8 tile of [`dense4`]: white squares on transparency.
fn dense4_tile() -> peniko::ImageBrushRef<'static> {
    static TILE: std::sync::OnceLock<peniko::ImageBrush> = std::sync::OnceLock::new();
    TILE.get_or_init(|| {
        let cell = |v: usize| (v + 1) / 2;
        let mut data = Vec::with_capacity(8 * 8 * 4);
        for y in 0..8 {
            for x in 0..8 {
                let white = (cell(x) + cell(y)) % 2 == 0;
                data.extend_from_slice(if white { &[255, 255, 255, 255] } else { &[0, 0, 0, 0] });
            }
        }
        peniko::ImageBrush {
            image: peniko::ImageData {
                data: peniko::Blob::new(std::sync::Arc::new(data)),
                format: peniko::ImageFormat::Rgba8,
                width: 8,
                height: 8,
                alpha_type: peniko::ImageAlphaType::Alpha,
            },
            sampler: peniko::ImageSampler {
                x_extend: peniko::Extend::Repeat,
                y_extend: peniko::Extend::Repeat,
                quality: peniko::ImageQuality::Low,
                alpha: 1.0,
            },
        }
    })
    .as_ref()
}
