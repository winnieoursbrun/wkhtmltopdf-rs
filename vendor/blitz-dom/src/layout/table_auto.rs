//! wkhtmltopdf-rs: column widths of WebKit's automatic table layout, as in
//! QtWebKit 2.2 (`WebCore/rendering/AutoTableLayout.cpp`).
//!
//! Blitz lays tables out as a CSS grid. Grid sizing of `auto` tracks spreads
//! the free width equally between columns, whereas WebKit gives every column
//! its minimum width and then shares the whole remaining width in proportion
//! to the columns' maximum (preferred) widths. This module computes the
//! column widths with WebKit's integer algorithm and hands the grid fixed
//! tracks, so only the row heights and the placement stay with the grid.
//!
//! In the collapsing border model the grid lays borders out as gutters. WebKit
//! instead gives each cell half of each collapsed border (the extra pixel on
//! the start side); both add up to the same table width, so the algorithm
//! runs on WebKit's cell widths (gutter included) and the gutter is taken
//! back from each track.

use std::sync::Arc;

use style::Atom;
use style::computed_values::border_collapse::T as BorderCollapse;
use taffy::{
    AvailableSpace, CompactLength, LayoutInput, LayoutPartialTree as _, Line, RequestedAxis,
    ResolveOrZero, RunMode, Size, SizingMode, TrackSizingFunction, style_helpers,
};

use super::resolve_calc_value;
use super::table::{CellWidth, TableTreeWrapper};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Auto,
    Fixed,
    Percent,
}

#[derive(Clone, Copy, Debug)]
struct Col {
    min: i32,
    max: i32,
    kind: Kind,
    /// Fixed width (border box, px) or percentage (0..100)
    width: f32,
    empty: bool,
}

impl Default for Col {
    fn default() -> Self {
        Col { min: 0, max: 0, kind: Kind::Auto, width: 0.0, empty: true }
    }
}

fn measure(w: &mut TableTreeWrapper, index: usize, space: AvailableSpace) -> f32 {
    let input = LayoutInput {
        run_mode: RunMode::ComputeSize,
        sizing_mode: SizingMode::ContentSize,
        axis: RequestedAxis::Horizontal,
        known_dimensions: Size::NONE,
        known_dimensions_are_definite: Size { width: true, height: true },
        parent_size: Size::NONE,
        available_space: Size { width: space, height: AvailableSpace::MaxContent },
        vertical_margins_are_collapsible: Line::FALSE,
    };
    w.compute_child_layout(taffy::NodeId::from(index), input).size.width
}

/// The grid style with WebKit's column widths, and the table's border-box
/// width. `None` for the fixed table layout (left to the grid).
pub(crate) fn auto_layout(
    w: &mut TableTreeWrapper,
    inputs: &LayoutInput,
) -> Option<(taffy::Style<Atom>, f32)> {
    let ctx = Arc::clone(&w.ctx);
    if ctx.is_fixed || ctx.cells.is_empty() {
        return None;
    }
    let n = ctx.style.grid_template_columns.len();
    if n == 0 {
        return None;
    }
    let gap = ctx.style.gap.width.into_raw().value();
    let share = if ctx.border_collapse == BorderCollapse::Collapse { gap } else { 0.0 };

    // Column minimum/maximum widths from the cells (AutoTableLayout::recalcColumn).
    let mut cols = vec![Col::default(); n];
    let mut spanning = Vec::new();
    for (index, cell) in ctx.cells.iter().enumerate() {
        let min = measure(w, index, AvailableSpace::MinContent).ceil();
        let max = measure(w, index, AvailableSpace::MaxContent).ceil().max(min);
        let has_content = !w.doc.nodes[cell.node_id].children.is_empty();
        let (min, max) = (min as i32 + share as i32, max as i32 + share as i32);
        let first = cell.column as usize;
        if first >= n {
            continue;
        }
        if cell.colspan > 1 {
            spanning.push((first, (cell.colspan as usize).min(n - first), min, max, cell.spec_width));
            continue;
        }
        let col = &mut cols[first];
        col.empty &= !has_content;
        col.min = col.min.max(if has_content { 1 } else { 0 }).max(min);
        col.max = col.max.max(1).max(max);
        match cell.spec_width {
            Some(CellWidth::Fixed(v)) if col.kind != Kind::Percent => {
                let v = v + share;
                if col.kind != Kind::Fixed || v > col.width {
                    col.kind = Kind::Fixed;
                    col.width = v;
                }
            }
            Some(CellWidth::Percent(p)) if p > 0.0 => {
                if col.kind != Kind::Percent || p * 100.0 > col.width {
                    col.kind = Kind::Percent;
                    col.width = p * 100.0;
                }
            }
            _ => {}
        }
    }
    for col in &mut cols {
        if col.kind == Kind::Fixed {
            col.max = col.min.max(col.width as i32);
        }
        col.max = col.max.max(col.min);
    }
    // Spanning cells (AutoTableLayout::calcEffectiveLogicalWidth, simplified):
    // grow the spanned columns to fit the cell, in proportion to their
    // maximum widths (equally when they have none).
    spanning.sort_by_key(|s| s.1);
    for (first, span, min, max, _) in spanning {
        let range = first..first + span;
        for (target, pick) in [(min, 0usize), (max, 1usize)] {
            let get = |c: &Col| if pick == 0 { c.min } else { c.max };
            let total: i32 = cols[range.clone()].iter().map(get).sum();
            if target <= total {
                continue;
            }
            let mut extra = target - total;
            let mut weight: i32 = cols[range.clone()].iter().map(|c| c.max).sum();
            let mut left = span as i32;
            for col in &mut cols[range.clone()] {
                let add = if weight > 0 {
                    (extra as f32 * col.max as f32 / weight as f32) as i32
                } else {
                    extra / left
                };
                weight -= col.max;
                left -= 1;
                extra -= add;
                if pick == 0 {
                    col.min += add;
                } else {
                    col.max += add;
                }
                col.empty = false;
            }
            if let Some(last) = cols[range.clone()].last_mut() {
                if pick == 0 {
                    last.min += extra;
                } else {
                    last.max += extra;
                }
            }
        }
        for col in &mut cols[range] {
            col.max = col.max.max(col.min);
        }
    }

    // Table width (RenderTable::computeLogicalWidth).
    let parent_w = inputs.parent_size.width;
    let padding = ctx.style.padding.resolve_or_zero(parent_w, resolve_calc_value);
    let border = ctx.style.border.resolve_or_zero(parent_w, resolve_calc_value);
    // Width outside WebKit's cells: table padding and borders, spacing, and
    // the gutters minus the share of them the cells carry.
    let outside = padding.left + padding.right + border.left + border.right
        + gap * (n as f32 - 1.0)
        - share * n as f32;
    let sum_min: i32 = cols.iter().map(|c| c.min).sum();
    let mut sum_max: i32 = cols.iter().map(|c| c.max).sum();
    let total_percent: f32 = cols.iter().filter(|c| c.kind == Kind::Percent).map(|c| c.width).sum();
    if total_percent > 0.0 {
        // A table must be wide enough for its percentage columns to get their share.
        let non_percent: i32 = cols.iter().filter(|c| c.kind != Kind::Percent).map(|c| c.max).sum();
        let mut max_percent = 0.0f32;
        for c in cols.iter().filter(|c| c.kind == Kind::Percent) {
            max_percent = max_percent.max(c.max as f32 * 100.0 / c.width.max(0.01));
        }
        let rest = 100.0 - total_percent.min(100.0);
        let from_rest = if rest > 0.0 { non_percent as f32 * 100.0 / rest } else { 0.0 };
        sum_max = sum_max.max(max_percent as i32).max(from_rest as i32);
    }
    // The table is at least as wide as its caption's minimum width.
    let pref_min = (sum_min as f32 + outside).max(super::table::caption_min_width(w.doc, &ctx));
    let pref_max = sum_max as f32 + outside;
    let spec = ctx.style.size.width.into_raw();
    let width = if let Some(known) = inputs.known_dimensions.width {
        known
    } else if spec.tag() == CompactLength::LENGTH_TAG && spec.value() > 0.0 {
        spec.value().max(pref_min)
    } else if spec.tag() == CompactLength::PERCENT_TAG && parent_w.is_some() {
        (spec.value() * parent_w.unwrap()).floor().max(pref_min)
    } else {
        let avail = match inputs.available_space.width {
            AvailableSpace::Definite(a) => a,
            AvailableSpace::MinContent => pref_min,
            AvailableSpace::MaxContent => pref_max,
        };
        avail.min(pref_max).max(pref_min)
    };

    // AutoTableLayout::layout.
    let table_w = (width - outside).floor() as i32;
    let mut available = table_w;
    let mut widths: Vec<i32> = cols.iter().map(|c| c.min).collect();
    available -= sum_min;
    let (mut num_auto, mut total_auto, mut alloc_auto) = (0, 0i64, 0);
    let (mut num_fixed, mut total_fixed) = (0, 0.0f32);
    let mut num_auto_empty = 0;
    for c in &cols {
        match c.kind {
            Kind::Percent => {}
            Kind::Fixed => {
                num_fixed += 1;
                total_fixed += c.max as f32;
            }
            Kind::Auto => {
                if c.empty {
                    num_auto_empty += 1;
                } else {
                    num_auto += 1;
                    total_auto += i64::from(c.max);
                    alloc_auto += c.min;
                }
            }
        }
    }
    // Percentage columns.
    if available > 0 && total_percent > 0.0 {
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.kind == Kind::Percent) {
            let cell = c.min.max((c.width * table_w as f32 / 100.0) as i32);
            available += widths[i] - cell;
            widths[i] = cell;
        }
        if total_percent > 100.0 {
            // Remove the excess from the last percentage columns.
            let mut excess = (table_w as f32 * (total_percent - 100.0) / 100.0) as i32;
            for (i, c) in cols.iter().enumerate().rev() {
                if c.kind == Kind::Percent {
                    let reduce = excess.min(widths[i] - c.min).max(0);
                    widths[i] -= reduce;
                    available += reduce;
                    excess -= reduce;
                    if excess <= 0 {
                        break;
                    }
                }
            }
        }
    }
    // Fixed columns get their width.
    if available > 0 {
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.kind == Kind::Fixed) {
            let v = c.width as i32;
            if v > widths[i] {
                available += widths[i] - v;
                widths[i] = v;
            }
        }
    }
    // Auto columns share what is left in proportion to their maximum widths.
    if available > 0 && num_auto > 0 {
        available += alloc_auto;
        for (i, c) in cols.iter().enumerate() {
            if c.kind == Kind::Auto && !c.empty && total_auto > 0 {
                let cell = widths[i].max((available as f32 * (c.max as f32 / total_auto as f32)) as i32);
                available -= cell;
                total_auto -= i64::from(c.max);
                widths[i] = cell;
            }
        }
    }
    // Then over the fixed columns.
    if available > 0 && num_fixed > 0 {
        let mut total = total_fixed;
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.kind == Kind::Fixed) {
            let cell = (available as f32 * c.max as f32 / total.max(1.0)) as i32;
            available -= cell;
            total -= c.max as f32;
            widths[i] += cell;
        }
    }
    // Then over the percentage columns.
    if available > 0 && total_percent > 0.0 && total_percent < 100.0 {
        let mut total = total_percent;
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.kind == Kind::Percent) {
            let cell = (available as f32 * c.width / total) as i32;
            available -= cell;
            total -= c.width;
            widths[i] += cell;
            if available <= 0 || total <= 0.0 {
                break;
            }
        }
    }
    // Then equally over the rest, from the last column.
    if available > 0 && n > num_auto_empty {
        let mut total = (n - num_auto_empty) as i32;
        for i in (0..n).rev() {
            if cols[i].kind == Kind::Auto && cols[i].empty {
                continue;
            }
            let cell = available / total;
            available -= cell;
            total -= 1;
            widths[i] += cell;
        }
    }
    // Over-allocated: shrink auto, then fixed, then percentage columns towards
    // their minimum, from the last column.
    for kind in [Kind::Auto, Kind::Fixed, Kind::Percent] {
        if available >= 0 {
            break;
        }
        let mut beyond: i32 = (0..n).filter(|&i| cols[i].kind == kind).map(|i| widths[i] - cols[i].min).sum();
        for i in (0..n).rev() {
            if cols[i].kind != kind || beyond <= 0 {
                continue;
            }
            let diff = widths[i] - cols[i].min;
            let reduce = (available as f32 * diff as f32 / beyond as f32) as i32;
            beyond -= diff;
            widths[i] += reduce;
            available -= reduce;
            if available >= 0 {
                break;
            }
        }
    }

    let mut style = ctx.style.clone();
    style.grid_template_columns = widths
        .iter()
        .map(|&c| {
            let track: TrackSizingFunction = style_helpers::length((c as f32 - share).max(0.0));
            track.into()
        })
        .collect();
    Some((style, width))
}
