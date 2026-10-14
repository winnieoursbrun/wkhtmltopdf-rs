//! wkhtmltopdf-rs addition: source text of glyph runs.
//!
//! `anyrender::PaintScene::draw_glyphs` carries glyph ids and positions only. A PDF needs the
//! text each glyph stands for (extraction, search, copy). Right before every `draw_glyphs`
//! call, `stroke_text` publishes the run's text here, split per glyph along the parley
//! cluster boundaries, together with the DOM node of the run's style (used for links). A
//! recording `PaintScene` takes it with [`take`] inside its `draw_glyphs`.

use std::cell::RefCell;
use std::ops::Range;

use blitz_dom::{NodeId, node::TextBrush};
use parley::GlyphRun;

/// Text of one glyph run.
#[derive(Debug, Clone, Default)]
pub struct RunText {
    /// The text covered by the run.
    pub text: String,
    /// For each glyph, in drawing order, the byte range of `text` it represents. Glyphs of one
    /// cluster (or ligature) share the same range.
    pub glyph_ranges: Vec<Range<usize>>,
    /// Node id of the innermost element the run belongs to.
    pub node_id: NodeId,
}

thread_local! {
    static CURRENT: RefCell<Option<RunText>> = const { RefCell::new(None) };
}

/// Take the text published for the glyph run being drawn, if any.
pub fn take() -> Option<RunText> {
    CURRENT.with(|c| c.borrow_mut().take())
}

pub(crate) fn publish(source: Option<&str>, glyph_run: &GlyphRun<'_, TextBrush>, node_id: NodeId) {
    let rt = source.map(|src| build(src, glyph_run, node_id));
    CURRENT.with(|c| *c.borrow_mut() = rt);
}

fn clamp(src: &str, r: Range<usize>) -> Range<usize> {
    let mut s = r.start.min(src.len());
    let mut e = r.end.min(src.len()).max(s);
    while s > 0 && !src.is_char_boundary(s) {
        s -= 1;
    }
    while e < src.len() && !src.is_char_boundary(e) {
        e += 1;
    }
    s..e
}

fn build(src: &str, glyph_run: &GlyphRun<'_, TextBrush>, node_id: NodeId) -> RunText {
    let run = glyph_run.run();
    let glyph_count = glyph_run.glyphs().count();
    // Per-glyph absolute ranges, from the clusters of the run in visual order.
    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(glyph_count);
    let mut lo = usize::MAX;
    let mut hi = 0usize;
    let own = glyph_run.shaped_clusters();
    for cluster in run.visual_clusters() {
        let sc = cluster.shaped_clusters_range();
        if sc.start < own.start || sc.end > own.end {
            continue;
        }
        let r = clamp(src, cluster.text_range());
        let n = cluster.glyphs().count();
        if n == 0 {
            // Ligature continuation: its text belongs to the previous glyph(s).
            if let Some(last) = ranges.last().cloned() {
                let merged = last.start.min(r.start)..last.end.max(r.end);
                for g in ranges.iter_mut().rev() {
                    if *g == last {
                        *g = merged.clone();
                    } else {
                        break;
                    }
                }
            }
        } else {
            for _ in 0..n {
                ranges.push(r.clone());
            }
        }
        lo = lo.min(r.start);
        hi = hi.max(r.end);
    }
    // Should not happen: the clusters of the glyph run do not account for its glyphs. Keep the
    // text (one range for all glyphs) rather than lose it.
    if ranges.len() != glyph_count || lo > hi {
        let r = if lo <= hi { clamp(src, lo..hi) } else { clamp(src, run.text_range()) };
        let text = src[r].to_string();
        let len = text.len();
        return RunText { text, glyph_ranges: vec![0..len; glyph_count], node_id };
    }
    let text = src[lo..hi].to_string();
    let glyph_ranges = ranges.into_iter().map(|r| (r.start - lo)..(r.end - lo)).collect();
    RunText { text, glyph_ranges, node_id }
}
