// Copyright 2024 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::BreakReason;
use crate::data::LayoutData;
use crate::style::Brush;

/// Alignment of a layout.
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Alignment {
    /// This is [`Alignment::Left`] for LTR text and [`Alignment::Right`] for RTL text.
    #[default]
    Start,
    /// This is [`Alignment::Right`] for LTR text and [`Alignment::Left`] for RTL text.
    End,
    /// Align content to the left edge.
    ///
    /// For alignment that should be aware of text direction, use [`Alignment::Start`] or
    /// [`Alignment::End`] instead.
    Left,
    /// Align each line centered within the container.
    Center,
    /// Align content to the right edge.
    ///
    /// For alignment that should be aware of text direction, use [`Alignment::Start`] or
    /// [`Alignment::End`] instead.
    Right,
    /// Justify each line by spacing out content.
    ///
    /// The last line of a paragraph is start-aligned instead, unless
    /// [`AlignmentOptions::last_line_alignment`] is set.
    Justify,
}

/// Additional options to fine tune alignment
#[derive(Debug, Clone, Copy)]
pub struct AlignmentOptions {
    /// If set to `true`, "end" and "center" alignment will apply even if the line contents are
    /// wider than the alignment width. If it is set to `false`, all overflowing lines will be
    /// [`Alignment::Start`] aligned.
    pub align_when_overflowing: bool,
    /// The alignment of the last line of each paragraph: the last line of the layout and every
    /// line that ends in an explicit line break. This corresponds to the CSS `text-align-last`
    /// property.
    ///
    /// If set to `None`, those lines use the alignment passed to [`Layout::align`], except that
    /// [`Alignment::Justify`] falls back to [`Alignment::Start`]. This is the behavior of
    /// `text-align-last: auto`.
    ///
    /// [`Layout::align`]: crate::Layout::align
    pub last_line_alignment: Option<Alignment>,
}

#[expect(
    clippy::derivable_impls,
    reason = "Make default values explicit rather than relying on the implicit default value of bool"
)]
impl Default for AlignmentOptions {
    fn default() -> Self {
        Self {
            align_when_overflowing: false,
            last_line_alignment: None,
        }
    }
}

/// Align the layout.
pub(crate) fn align<B: Brush>(
    layout: &mut LayoutData<B>,
    alignment: Alignment,
    options: AlignmentOptions,
) {
    layout.alignment = Some(alignment);

    let is_rtl = layout.base_level.is_rtl();

    // wkhtmltopdf-rs: QtWebKit 2.2 aligns and justifies a line with the widths of its text
    // boxes, not with the rounded word segments its line breaker measured (see
    // `ShapedCluster::paint_delta`). A box holding a whole text node is as wide as the node's
    // preferred width (`RenderText::width` returns `maxLogicalWidth()`: each word and each space
    // rounded on its own); a box holding part of one is the rounded sum of its unrounded
    // advances (`QFontMetrics::width`). The difference per line, laid-out minus box widths:
    let box_corrections: alloc::vec::Vec<f32> =
        layout.lines.iter().map(|line| box_correction(layout, line)).collect();

    // Apply alignment to line items
    for (line, box_correction) in layout.lines.iter_mut().zip(box_corrections) {
        line.justification.amount_per_opportunity = 0.;

        let indent = line.metrics.indent;

        if is_rtl {
            // In RTL text, trailing whitespace is on the left. As we hang that whitespace, offset
            // the line to the left. Note: indent is not subtracted here because `free_space` below
            // already accounts for it.
            line.metrics.offset = -line.metrics.hanging_advance;
        } else {
            line.metrics.offset = indent;
        }

        // Compute free space.
        let line_width = line.metrics.inline_max_coord - line.metrics.inline_min_coord;
        let free_space = line_width - indent - line.metrics.advance + line.metrics.hanging_advance;
        let free_space = if free_space > 0.0 { free_space + box_correction } else { free_space };

        if !options.align_when_overflowing && free_space <= 0.0 {
            if is_rtl {
                // In RTL text, right-align on overflow.
                line.metrics.offset += free_space;
            }
            continue;
        }

        let is_last_line = matches!(line.break_reason, BreakReason::None | BreakReason::Explicit);
        let line_alignment = if !is_last_line {
            alignment
        } else if let Some(last_line_alignment) = options.last_line_alignment {
            last_line_alignment
        } else if alignment == Alignment::Justify {
            Alignment::Start
        } else {
            alignment
        };

        match (line_alignment, is_rtl) {
            (Alignment::Left, _) | (Alignment::Start, false) | (Alignment::End, true) => {
                // Do nothing
            }
            (Alignment::Right, _) | (Alignment::Start, true) | (Alignment::End, false) => {
                line.metrics.offset += free_space;
            }
            (Alignment::Center, _) => {
                line.metrics.offset += free_space * 0.5;
            }
            (Alignment::Justify, _) => {
                // Justified alignment doesn't have any effect if free_space is negative or zero
                if free_space <= 0.0 {
                    continue;
                }

                // Justified alignment doesn't apply if there are no whitespace gaps to adjust. In
                // that case, start-align, i.e., left-align for LTR text and right-align for RTL
                // text.
                if line.num_justification_opportunities == 0 {
                    if is_rtl {
                        line.metrics.offset += free_space;
                    }
                    continue;
                }

                line.justification.amount_per_opportunity =
                    free_space / line.num_justification_opportunities as f32;
            }
        }
    }
}

/// wkhtmltopdf-rs: the laid-out width of `line`'s text minus the width QtWebKit 2.2 gives its
/// text boxes (see `align`). A text box is approximated by a run of clusters of one style.
fn box_correction<B: Brush>(layout: &LayoutData<B>, line: &super::data::LineData) -> f32 {
    use parley_engine::shape::Whitespace;
    let clusters = layout.shaped_text.shaped_clusters();
    let items = &layout.line_items[line.item_range.clone()];
    let last_text = items.iter().rposition(|i| i.kind == super::data::LayoutItemKind::TextRun);
    let mut correction = 0.0;
    for (n, item) in items.iter().enumerate() {
        if item.kind != super::data::LayoutItemKind::TextRun {
            continue;
        }
        let (start, mut end) =
            (item.shaped_cluster_range.start as usize, item.shaped_cluster_range.end as usize);
        let Some(item_clusters) = clusters.get(start..end) else { continue };
        let mut cut_at_end = false;
        if Some(n) == last_text {
            // Trailing whitespace hangs: it is not part of the aligned width.
            let kept = item_clusters
                .iter()
                .rposition(|c| c.whitespace() == Whitespace::None)
                .map_or(0, |i| i + 1);
            cut_at_end = kept < item_clusters.len();
            end = start + kept;
        }
        // An inline box next to the item splits the text into two nodes.
        let is_box = |i: Option<usize>| {
            i.and_then(|i| items.get(i))
                .is_some_and(|i| i.kind == super::data::LayoutItemKind::InlineBox)
        };
        let (box_before, box_after) = (is_box(n.checked_sub(1)), is_box(Some(n + 1)));
        let item_end = item.shaped_cluster_range.end as usize;
        let mut group = start;
        while group < end {
            let style = clusters[group].style_index;
            let group_end = (group..end).find(|&i| clusters[i].style_index != style).unwrap_or(end);
            correction += text_box_correction(
                clusters,
                group..group_end,
                group_end == end && cut_at_end,
                group == start && box_before,
                group_end == item_end && box_after,
            );
            group = group_end;
        }
    }
    correction
}

/// wkhtmltopdf-rs: the laid-out width of the clusters `range` (all of one style, standing for a
/// QtWebKit text box) minus the box's width in WebKit: the text node's preferred width (each
/// word and each space rounded on its own) when the box holds the whole node, otherwise the
/// rounded sum of the unrounded advances. `cut` tells that the box ends where hanging
/// whitespace was left out, so it does not hold the whole node; `node_start`/`node_end` that
/// an inline box (another element) is next to the box on that side.
pub(crate) fn text_box_correction(
    clusters: &[parley_engine::shape::ShapedCluster],
    range: core::ops::Range<usize>,
    cut: bool,
    node_start: bool,
    node_end: bool,
) -> f32 {
    use parley_engine::shape::Whitespace;
    let Some(g) = clusters.get(range.clone()) else { return 0.0 };
    let Some(style) = g.first().map(|c| c.style_index) else { return 0.0 };
    if g.iter().all(|c| c.paint_delta == 0.0) {
        // Nothing was rounded: the laid-out width is the text's.
        return 0.0;
    }
    let whole = (node_start || range.start == 0 || clusters[range.start - 1].style_index != style)
        && !cut
        && (node_end || clusters.get(range.end).is_none_or(|c| c.style_index != style));
    let laid_out: f32 = g.iter().map(|c| c.advance).sum();
    let target = if whole {
        let (mut width, mut word) = (0.0f32, 0.0f32);
        for c in g {
            let unrounded = c.advance - c.paint_delta;
            if c.whitespace() == Whitespace::None {
                // A break opportunity (after `-`) ends a word too.
                if c.is_soft_wrap_opportunity_before() {
                    width += word.round();
                    word = 0.0;
                }
                word += unrounded;
            } else {
                width += word.round() + unrounded.round();
                word = 0.0;
            }
        }
        width + word.round()
    } else {
        g.iter().map(|c| c.advance - c.paint_delta).sum::<f32>().round()
    };
    laid_out - target
}
