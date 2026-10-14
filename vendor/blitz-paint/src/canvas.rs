//! wkhtmltopdf-rs addition: `<canvas>` content recorded by script.
//!
//! The JavaScript engine records canvas 2D calls as an `anyrender` scene in
//! canvas pixel space (`crates/wk-js/src/canvas.rs`). Before painting, the
//! embedder registers the pictures here with [`set_pictures`]; the painter
//! replays each one inside its canvas's content box, scaled from the canvas
//! pixel size to the box, and publishes the source text of its glyph runs
//! (see [`crate::run_text`]) so canvas text stays real text in a PDF.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use anyrender::recording::RenderCommand;
use anyrender::{PaintScene, Scene};
use blitz_dom::NodeId;
use kurbo::{Affine, Rect};
use peniko::Fill;

use crate::run_text::RunText;

/// A recorded canvas.
#[derive(Clone)]
pub struct CanvasPicture {
    pub scene: Scene,
    /// Source text of glyph-run commands, by command index.
    pub texts: HashMap<usize, RunText>,
    /// Canvas pixel size.
    pub width: f64,
    pub height: f64,
}

thread_local! {
    static PICTURES: RefCell<HashMap<NodeId, Rc<CanvasPicture>>> = RefCell::new(HashMap::new());
}

/// Register the pictures painted by the next `paint_scene` calls on this
/// thread (replaces any previous set).
pub fn set_pictures(pictures: HashMap<NodeId, Rc<CanvasPicture>>) {
    PICTURES.with(|p| *p.borrow_mut() = pictures);
}

/// Forget the registered pictures.
pub fn clear_pictures() {
    PICTURES.with(|p| p.borrow_mut().clear());
}

pub(crate) fn picture(node: NodeId) -> Option<Rc<CanvasPicture>> {
    PICTURES.with(|p| p.borrow().get(&node).cloned())
}

/// Replay `pic` into `scene`, mapping canvas pixels onto `content` (in the
/// coordinates of `transform`), clipped to the content box.
pub(crate) fn replay(scene: &mut impl PaintScene, pic: &CanvasPicture, transform: Affine, content: Rect) {
    if pic.width <= 0.0 || pic.height <= 0.0 || content.width() <= 0.0 || content.height() <= 0.0 {
        return;
    }
    let place = transform
        * Affine::translate((content.x0, content.y0))
        * Affine::scale_non_uniform(content.width() / pic.width, content.height() / pic.height);
    scene.push_clip_layer(Fill::NonZero, transform, &content);
    for (i, cmd) in pic.scene.commands.iter().enumerate() {
        if let (RenderCommand::GlyphRun(_), Some(t)) = (cmd, pic.texts.get(&i)) {
            crate::run_text::set(t.clone());
        }
        scene.append_scene(Scene { tolerance: pic.scene.tolerance, commands: vec![cmd.clone()] }, place);
    }
    scene.pop_layer();
}
