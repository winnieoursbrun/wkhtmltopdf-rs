//! wkhtmltopdf-rs: pages drawn away from their document, on other threads.
//!
//! Most of the cost of a page is turning its drawing into content-stream operators
//! (paths, colours, transforms, number formatting), which does not depend on the rest of the
//! document. [`draft_page`] draws a page on a [`Surface`] of its own, with a private
//! serializer, and keeps aside the two things that do depend on the document:
//!
//! - the graphics states the page uses, registered (numbered and written) in the document
//!   when the draft is applied, in the order the page used them; their names in the page's
//!   resources are given in that order too, so the draft already has the right ones;
//! - the inside of every text object (`BT` … `ET`): which font object a glyph goes to, its
//!   CID in the subset and whether its text needs `/ActualText` depend on every glyph shown
//!   before it in the document, so it is written when the draft is applied, in order.
//!
//! [`Page::apply_draft`](crate::page::Page::apply_draft) then produces exactly the content
//! stream and resources that drawing the page on [`Page::surface`](crate::page::Page::surface)
//! at that point would have produced. A draft that needed the document in any other way
//! (images, opacity groups, masks, gradients and patterns, outlined or colour glyphs, tagged
//! content) is refused by `apply_draft`, and the page is then drawn in the document as usual.

use std::sync::Arc;

use crate::chunk_container::ChunkContainer;
use crate::content::{ContentBuilder, DraftContent, DraftLog};
use crate::geom::Transform;
use crate::page::{page_root_transform, PageSettings};
use crate::serialize::{SerializeContext, SerializeSettings};
use crate::surface::Surface;
use crate::Document;

/// The settings pages of a document are drafted with ([`Document::draft_settings`]).
#[derive(Clone)]
pub struct DraftSettings {
    pub(crate) settings: Arc<SerializeSettings>,
}

/// A page drawn by [`draft_page`], to be applied to a page of the document it was drafted
/// for with [`Page::apply_draft`](crate::page::Page::apply_draft).
pub struct PageDraft {
    pub(crate) settings: Arc<SerializeSettings>,
    pub(crate) root_transform: Transform,
    pub(crate) bbox_important: bool,
    pub(crate) content: DraftContent,
}

impl PageDraft {
    /// Whether the page can be applied (it used nothing a draft cannot express).
    pub fn is_usable(&self) -> bool {
        !self.content.log.failed
    }
}

impl Document {
    /// The settings to draft this document's pages with.
    pub fn draft_settings(&self) -> DraftSettings {
        DraftSettings { settings: self.serializer_context.serialize_settings() }
    }
}

/// Draw a page of `page_settings` with `draw` away from its document (on any thread).
pub fn draft_page(
    settings: &DraftSettings,
    page_settings: &PageSettings,
    draw: impl FnOnce(&mut Surface<'_>),
) -> PageDraft {
    let serialize_settings = (*settings.settings).clone();
    let unsupported = serialize_settings.enable_tagging;
    let mut sc = SerializeContext::new(serialize_settings);
    // Validators, and the settings `SerializeContext::new` turns on for them (tagging, no
    // device colour spaces): a draft only covers the plain case.
    let unsupported = unsupported
        || sc.serialize_settings().enable_tagging
        || sc.serialize_settings().no_device_cs
        || !sc.serialize_settings().validators().is_empty();
    let mut chunk_container = ChunkContainer::new(&sc);
    let root_transform = page_root_transform(page_settings.surface_size().height());
    let bbox_important = page_settings.media_box().is_none();
    let mut root = ContentBuilder::new(root_transform, bbox_important, &sc);
    root.draft = Some(DraftLog { events: Vec::new(), failed: unsupported });
    let mut out = None;
    {
        let mut surface = Surface::new_draft(&mut sc, &mut chunk_container, root, &mut out);
        draw(&mut surface);
    }
    let content = out.expect("the draft surface writes its content when dropped");
    PageDraft { settings: Arc::clone(&settings.settings), root_transform, bbox_important, content }
}
