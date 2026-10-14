//! wkhtmltopdf-rs addition: inline layouts handed over to the painter.
//!
//! A document painted for the last time does not need its text layouts once their text is
//! drawn. The embedder moves them out of the document
//! (`BaseDocument::take_inline_layouts`) and registers them here with [`set`] before
//! `paint_scene`; the painter takes each one when it paints the inline root's text and drops
//! it right after, so the memory of the layouts already painted is reused by the display
//! list being recorded instead of adding to it.

use std::cell::RefCell;
use std::collections::HashMap;

use blitz_dom::NodeId;
use blitz_dom::node::TextLayout;

thread_local! {
    static LAYOUTS: RefCell<HashMap<NodeId, Box<TextLayout>>> = RefCell::new(HashMap::new());
}

/// Register the inline layouts of the document painted by the next `paint_scene` call on
/// this thread (replaces any previous set).
pub fn set(layouts: HashMap<NodeId, Box<TextLayout>>) {
    LAYOUTS.with(|l| *l.borrow_mut() = layouts);
}

/// Drop the registered layouts that were not painted.
pub fn clear() {
    let layouts = LAYOUTS.with(|l| std::mem::take(&mut *l.borrow_mut()));
    drop(layouts);
}

/// Take the registered layout of an inline root.
pub(crate) fn take(node: NodeId) -> Option<Box<TextLayout>> {
    LAYOUTS.with(|l| l.borrow_mut().remove(&node))
}

/// Read the registered layout of an inline root, if any.
pub(crate) fn with<R>(node: NodeId, f: impl FnOnce(Option<&TextLayout>) -> R) -> R {
    LAYOUTS.with(|l| f(l.borrow().get(&node).map(|b| &**b)))
}
