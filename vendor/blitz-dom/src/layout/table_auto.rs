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
    /// Effective minimum and maximum widths (`effectiveMin/MaxLogicalWidth`)
    min: i32,
    max: i32,
    /// The column's own width (`logicalWidth`): fixed width (border box, px)
    /// or percentage (0..100)
    kind: Kind,
    width: f32,
    /// The width spanning cells leave it (`effectiveLogicalWidth`)
    eff_kind: Kind,
    eff_width: f32,
    empty: bool,
}

impl Default for Col {
    fn default() -> Self {
        Col { min: 0, max: 0, kind: Kind::Auto, width: 0.0, eff_kind: Kind::Auto, eff_width: 0.0, empty: true }
    }
}

/// A cell spanning several columns, for `calc_effective_widths`.
struct SpanCell {
    first: usize,
    span: usize,
    /// The row the cell starts in.
    row: u16,
    min: i32,
    max: i32,
    width: Option<CellWidth>,
}

/// `AutoTableLayout::calcEffectiveLogicalWidth`: widen the columns under each
/// spanning cell (narrowest spans first) so that they hold the cell's minimum
/// and maximum widths, and give the columns under a percentage cell their
/// share of the percentage. Returns the table maximum width percentage cells
/// require.
fn calc_effective_widths(cols: &mut [Col], spans: &mut [SpanCell], spacing: i32) -> i32 {
    for c in cols.iter_mut() {
        c.eff_kind = c.kind;
        c.eff_width = c.width;
    }
    let n = cols.len();
    let mut max_width = 0i32;
    // `insertSpanCell` keeps them sorted by span, each new cell going before those of the same
    // span: `fullRecalc` inserts them column by column, top to bottom, so among equal spans the
    // last column comes first, and in a column the bottom cell.
    spans.sort_by_key(|s| (s.span, std::cmp::Reverse(s.first), std::cmp::Reverse(s.row)));
    for cell in spans.iter() {
        let mut cell_width = cell.width;
        let first = cell.first;
        let mut last = first;
        let mut span = cell.span;
        let mut cell_min = cell.min + spacing;
        let mut cell_max = cell.max + spacing;
        let mut total_percent = 0.0f32;
        let mut span_min = 0i32;
        let mut span_max = 0i32;
        let mut all_percent = true;
        let mut all_fixed = true;
        let mut have_auto = false;
        let mut span_empty = true;
        let mut fixed_width = 0i32;
        while last < n && span > 0 {
            let c = &mut cols[last];
            match c.kind {
                Kind::Percent => {
                    total_percent += c.width;
                    all_fixed = false;
                }
                Kind::Fixed if c.width > 0.0 => {
                    fixed_width += c.width as i32;
                    all_percent = false;
                }
                _ => {
                    have_auto = true;
                    if c.eff_kind != Kind::Percent {
                        c.eff_kind = Kind::Auto;
                        c.eff_width = 0.0;
                        all_percent = false;
                    } else {
                        total_percent += c.eff_width;
                    }
                    all_fixed = false;
                }
            }
            if !c.empty {
                span_empty = false;
            }
            span -= 1;
            span_min += c.min;
            span_max += c.max;
            last += 1;
            cell_min -= spacing;
            cell_max -= spacing;
        }

        if let Some(CellWidth::Percent(p)) = cell_width {
            let p = p * 100.0;
            if total_percent > p || all_percent {
                cell_width = None;
            } else {
                max_width = max_width.max((span_max.max(cell_max) as f32 * 100.0 / p) as i32);
                let mut missing = p - total_percent;
                let mut total: i32 =
                    cols[first..last].iter().filter(|c| c.eff_kind != Kind::Percent).map(|c| c.max).sum();
                for c in cols[first..last].iter_mut() {
                    if total <= 0 {
                        break;
                    }
                    if c.eff_kind != Kind::Percent {
                        let percent = missing * c.max as f32 / total as f32;
                        total -= c.max;
                        missing -= percent;
                        if percent > 0.0 {
                            c.eff_kind = Kind::Percent;
                            c.eff_width = percent;
                        } else {
                            c.eff_kind = Kind::Auto;
                            c.eff_width = 0.0;
                        }
                    }
                }
            }
        }

        // Honour the spanning cell's minimum width.
        if cell_min > span_min {
            if all_fixed {
                for c in cols[first..last].iter_mut() {
                    if fixed_width <= 0 {
                        break;
                    }
                    let w = c.min.max((cell_min as f32 * c.width / fixed_width as f32) as i32);
                    fixed_width -= c.width as i32;
                    cell_min -= w;
                    c.min = w;
                }
            } else {
                let mut remaining_max = span_max;
                let mut remaining_min = span_min;
                // Give the minimum to the fixed columns first…
                for c in cols[first..last].iter_mut() {
                    if remaining_max < 0 {
                        break;
                    }
                    if c.kind == Kind::Fixed && have_auto && fixed_width <= cell_min {
                        let w = c.min.max(c.width as i32);
                        fixed_width -= c.width as i32;
                        remaining_min -= c.min;
                        remaining_max -= c.max;
                        cell_min -= w;
                        c.min = w;
                    }
                }
                // …then to the others, in proportion to their maximum widths.
                for c in cols[first..last].iter_mut() {
                    if remaining_max < 0 || remaining_min >= cell_min {
                        break;
                    }
                    if !(c.kind == Kind::Fixed && have_auto && fixed_width <= cell_min) {
                        let share = if remaining_max != 0 {
                            (cell_min as f32 * c.max as f32 / remaining_max as f32) as i32
                        } else {
                            cell_min
                        };
                        let w = c.min.max(share).min(c.min + (cell_min - remaining_min));
                        remaining_max -= c.max;
                        remaining_min -= c.min;
                        cell_min -= w;
                        c.min = w;
                    }
                }
            }
        }
        if !matches!(cell_width, Some(CellWidth::Percent(_))) {
            if cell_max > span_max {
                let mut span_max = span_max;
                for c in cols[first..last].iter_mut() {
                    if span_max < 0 {
                        break;
                    }
                    let share = if span_max != 0 {
                        (cell_max as f32 * c.max as f32 / span_max as f32) as i32
                    } else {
                        cell_max
                    };
                    let w = c.max.max(share);
                    span_max -= c.max;
                    cell_max -= w;
                    c.max = w;
                }
            }
        } else {
            for c in cols[first..last].iter_mut() {
                c.max = c.max.max(c.min);
            }
        }
        // A span of columns holding only empty cells is treated as having content.
        if span_empty {
            for c in cols[first..last].iter_mut() {
                c.empty = false;
            }
        }
    }
    max_width
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
    if ctx.is_fixed {
        return fixed_layout(&ctx, inputs);
    }
    if ctx.cells.is_empty() {
        return None;
    }
    let n = ctx.style.grid_template_columns.len();
    if n == 0 {
        return None;
    }
    let gap = ctx.style.gap.width.into_raw().value();
    let share = if ctx.border_collapse == BorderCollapse::Collapse { gap } else { 0.0 };
    // QtWebKit runs the algorithm in whole device pixels (`--zoom` × `--dpi` / 96): every
    // width below is in device px, the column widths are converted back at the end.
    let z = stylo_taffy::convert::device_zoom();
    let share_d = (share * z).round();

    // Column minimum/maximum widths from the cells (AutoTableLayout::recalcColumn).
    let mut cols = vec![Col::default(); n];
    // `<col>` widths come first (`fullRecalc`).
    for (col, width) in cols.iter_mut().zip(&ctx.fixed_widths) {
        match *width {
            Some(CellWidth::Fixed(v)) if v > 0.0 => {
                col.kind = Kind::Fixed;
                col.width = v * z;
                col.max = (v * z) as i32;
            }
            Some(CellWidth::Percent(p)) if p > 0.0 => {
                col.kind = Kind::Percent;
                col.width = p * 100.0;
            }
            _ => {}
        }
    }
    let mut spanning = Vec::new();
    for (index, cell) in ctx.cells.iter().enumerate() {
        let min = (measure(w, index, AvailableSpace::MinContent) * z).ceil();
        let max = (measure(w, index, AvailableSpace::MaxContent) * z).ceil().max(min);
        let (min, mut max) = (min as i32 + share_d as i32, max as i32 + share_d as i32);
        // A fixed width is the cell's maximum width, unless its content is wider
        // (`RenderBlock::computePreferredLogicalWidths` for table cells).
        if let Some(CellWidth::Fixed(v)) = cell.spec_width {
            max = min.max((v * z + share_d) as i32);
        }
        // `cellHasContent`: a child box, a border or a padding.
        let has_content = !w.doc.nodes[cell.node_id].children.is_empty() || {
            let style = ctx.cell_style(index);
            let p = style.padding.resolve_or_zero(None, resolve_calc_value);
            let b = style.border.resolve_or_zero(None, resolve_calc_value);
            p.left + p.right + p.top + p.bottom + b.left + b.right + b.top + b.bottom > 0.0
        };
        let first = cell.column as usize;
        if first >= n {
            continue;
        }
        let col = &mut cols[first];
        col.empty &= !has_content;
        col.min = col.min.max(if has_content { 1 } else { 0 });
        col.max = col.max.max(1);
        if cell.colspan > 1 {
            spanning.push(SpanCell {
                first,
                span: (cell.colspan as usize).min(n - first),
                row: cell.row,
                min,
                max,
                width: cell.spec_width,
            });
            continue;
        }
        col.min = col.min.max(min);
        let max_contributor = max > col.max;
        col.max = col.max.max(max);
        match cell.spec_width {
            Some(CellWidth::Fixed(v)) if col.kind != Kind::Percent => {
                let v = v * z + share_d;
                if col.kind != Kind::Fixed || v > col.width || (v == col.width && max_contributor) {
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
        col.max = col.max.max(col.min);
    }
    let spacing = if ctx.border_collapse == BorderCollapse::Collapse { 0 } else { (gap * z).round() as i32 };
    let span_max = calc_effective_widths(&mut cols, &mut spanning, spacing);
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
    let total_percent: f32 =
        cols.iter().filter(|c| c.eff_kind == Kind::Percent).map(|c| c.eff_width).sum();
    if total_percent > 0.0 {
        // A table must be wide enough for its percentage columns to get their share.
        let non_percent: i32 = cols.iter().filter(|c| c.eff_kind != Kind::Percent).map(|c| c.max).sum();
        let mut max_percent = 0.0f32;
        for c in cols.iter().filter(|c| c.eff_kind == Kind::Percent) {
            max_percent = max_percent.max(c.max as f32 * 100.0 / c.eff_width.max(0.01));
        }
        let rest = 100.0 - total_percent.min(100.0);
        let from_rest = if rest > 0.0 { non_percent as f32 * 100.0 / rest } else { 0.0 };
        sum_max = sum_max.max(max_percent as i32).max(from_rest as i32);
    }
    sum_max = sum_max.max(span_max);
    // The table is at least as wide as its caption's minimum width.
    let pref_min = (sum_min as f32 / z + outside).max(super::table::caption_min_width(w.doc, &ctx));
    let pref_max = sum_max as f32 / z + outside;
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
    let table_w = ((width - outside) * z + 1e-3).floor() as i32;
    let mut available = table_w;
    let mut widths: Vec<i32> = cols.iter().map(|c| c.min).collect();
    available -= sum_min;
    let (mut num_auto, mut total_auto, mut alloc_auto) = (0, 0i64, 0);
    let (mut num_fixed, mut total_fixed) = (0, 0.0f32);
    let mut num_auto_empty = 0;
    for c in &cols {
        match c.eff_kind {
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
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.eff_kind == Kind::Percent) {
            let cell = c.min.max((c.eff_width * table_w as f32 / 100.0) as i32);
            available += widths[i] - cell;
            widths[i] = cell;
        }
        if total_percent > 100.0 {
            // Remove the excess from the last percentage columns.
            let mut excess = (table_w as f32 * (total_percent - 100.0) / 100.0) as i32;
            for (i, c) in cols.iter().enumerate().rev() {
                if c.eff_kind == Kind::Percent {
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
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.eff_kind == Kind::Fixed) {
            let v = c.eff_width as i32;
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
            if c.eff_kind == Kind::Auto && !c.empty && total_auto > 0 {
                // `available * float(max) / totalAuto`, in that order: 97 * 64 / 97 is 64, while
                // 97 * (64 / 97) truncates to 63.
                let cell = widths[i].max((available as f32 * c.max as f32 / total_auto as f32) as i32);
                available -= cell;
                total_auto -= i64::from(c.max);
                widths[i] = cell;
            }
        }
    }
    // Then over the fixed columns.
    if available > 0 && num_fixed > 0 {
        let mut total = total_fixed;
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.eff_kind == Kind::Fixed) {
            let cell = (available as f32 * c.max as f32 / total.max(1.0)) as i32;
            available -= cell;
            total -= c.max as f32;
            widths[i] += cell;
        }
    }
    // Then over the percentage columns.
    if available > 0 && total_percent > 0.0 && total_percent < 100.0 {
        let mut total = total_percent;
        for (i, c) in cols.iter().enumerate().filter(|(_, c)| c.eff_kind == Kind::Percent) {
            let cell = (available as f32 * c.eff_width / total) as i32;
            available -= cell;
            total -= c.eff_width;
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
            if cols[i].eff_kind == Kind::Auto && cols[i].empty {
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
        let mut beyond: i32 = (0..n).filter(|&i| cols[i].eff_kind == kind).map(|i| widths[i] - cols[i].min).sum();
        for i in (0..n).rev() {
            if cols[i].eff_kind != kind || beyond <= 0 {
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

    let widths: Vec<f32> = widths.iter().map(|&c| c as f32 / z).collect();
    let mut style = ctx.style.clone();
    style.grid_template_columns = widths
        .iter()
        .map(|&c| {
            let track: TrackSizingFunction = style_helpers::length((c - share).max(0.0));
            track.into()
        })
        .collect();
    if let Some(rows) = rowspan_row_heights(w, &ctx, &widths, gap, share) {
        style.grid_template_rows = rows
            .into_iter()
            .map(|h| {
                let track: TrackSizingFunction = style_helpers::length(h);
                track.into()
            })
            .collect();
    }
    Some((style, width))
}

/// WebKit 534's fixed table layout (`FixedTableLayout`): the specified column
/// widths, as specified (without the cells' padding and border), the rest of
/// the table width shared equally by the other columns, the last one taking
/// the remainder. The table is at least as wide as its fixed columns.
fn fixed_layout(ctx: &super::table::TableContext, inputs: &LayoutInput) -> Option<(taffy::Style<Atom>, f32)> {
    let n = ctx.style.grid_template_columns.len();
    if n == 0 {
        return None;
    }
    let gap = ctx.style.gap.width.into_raw().value();
    let parent_w = inputs.parent_size.width;
    let padding = ctx.style.padding.resolve_or_zero(parent_w, resolve_calc_value);
    let border = ctx.style.border.resolve_or_zero(parent_w, resolve_calc_value);
    let outside = padding.left + padding.right + border.left + border.right + gap * (n as f32 - 1.0);
    let widths = |c: usize| ctx.fixed_widths.get(c).copied().flatten();
    // computePreferredLogicalWidths: the fixed widths (percentages count as 0)
    let fixed_sum: i32 = (0..n)
        .map(|c| match widths(c) {
            Some(CellWidth::Fixed(v)) => v as i32,
            _ => 0,
        })
        .sum();
    let pref = fixed_sum as f32 + outside;
    let spec = ctx.style.size.width.into_raw();
    let width = if let Some(known) = inputs.known_dimensions.width {
        known
    } else if spec.tag() == CompactLength::LENGTH_TAG {
        spec.value().max(pref)
    } else if spec.tag() == CompactLength::PERCENT_TAG && parent_w.is_some() {
        (spec.value() * parent_w.unwrap()).floor().max(pref)
    } else {
        return None;
    };
    let table_w = (width - outside).floor() as i32;

    let mut calc = vec![0i32; n];
    let (mut num_auto, mut total_fixed, mut total_percent_w, mut total_percent) = (0, 0i32, 0i32, 0.0f32);
    for (c, w) in calc.iter_mut().enumerate() {
        match widths(c) {
            Some(CellWidth::Fixed(v)) => {
                *w = v as i32;
                total_fixed += *w;
            }
            Some(CellWidth::Percent(p)) => {
                *w = (p * table_w as f32) as i32;
                total_percent_w += *w;
                total_percent += p;
            }
            None => num_auto += 1,
        }
    }
    let mut total = total_fixed + total_percent_w;
    if num_auto == 0 || total > table_w {
        if total != table_w {
            // Fixed widths only scale up.
            if total_fixed > 0 && total < table_w {
                total_fixed = 0;
                for (c, w) in calc.iter_mut().enumerate() {
                    if matches!(widths(c), Some(CellWidth::Fixed(_))) {
                        *w = (*w as i64 * table_w as i64 / total as i64) as i32;
                        total_fixed += *w;
                    }
                }
            }
            if total_percent > 0.0 {
                total_percent_w = 0;
                for (c, w) in calc.iter_mut().enumerate() {
                    if let Some(CellWidth::Percent(p)) = widths(c) {
                        *w = (p * (table_w - total_fixed) as f32 / total_percent) as i32;
                        total_percent_w += *w;
                    }
                }
            }
            total = total_fixed + total_percent_w;
        }
    } else {
        let mut remaining = table_w - total_fixed - total_percent_w;
        let mut left = num_auto;
        let mut last_auto = 0;
        for (c, w) in calc.iter_mut().enumerate() {
            if widths(c).is_none() {
                let share = remaining / left;
                *w = share;
                remaining -= share;
                if remaining == 0 {
                    break;
                }
                last_auto = c;
                left -= 1;
            }
        }
        if remaining != 0 {
            calc[last_auto] += remaining;
        }
        total = table_w;
    }
    if total < table_w {
        // Spread the extra width over the columns, from the last one.
        let mut remaining = table_w - total;
        let mut left = n as i32;
        while left > 0 {
            let share = remaining / left;
            remaining -= share;
            left -= 1;
            calc[left as usize] += share;
        }
        calc[n - 1] += remaining;
    }

    let mut style = ctx.style.clone();
    style.grid_template_columns = calc
        .iter()
        .map(|&c| {
            let track: TrackSizingFunction = style_helpers::length(c.max(0) as f32);
            track.into()
        })
        .collect();
    Some((style, width))
}

/// The row heights of a table with cells spanning rows, as
/// `RenderTableSection::calcRowLogicalHeight` computes them: rows are sized
/// top to bottom, and a cell spanning rows only grows the last row it spans.
/// (The grid would share the extra height between the spanned rows.) `None`
/// when no cell spans rows or the table has a specified height.
fn rowspan_row_heights(
    w: &mut TableTreeWrapper,
    ctx: &super::table::TableContext,
    widths: &[f32],
    gap: f32,
    share: f32,
) -> Option<Vec<f32>> {
    let nr = ctx.rows.len();
    if nr == 0 || !ctx.cells.iter().any(|c| c.rowspan > 1) || !ctx.style.size.height.is_auto() {
        return None;
    }
    let spacing = if ctx.border_collapse == BorderCollapse::Collapse { 0.0 } else { ctx.style.gap.height.into_raw().value() };
    let mut ends: Vec<Vec<(usize, f32)>> = vec![Vec::new(); nr];
    for (index, cell) in ctx.cells.iter().enumerate() {
        let first = cell.column as usize;
        let last = (first + cell.colspan as usize).min(widths.len());
        if first >= last {
            continue;
        }
        let cell_w: f32 =
            widths[first..last].iter().map(|&c| c - share).sum::<f32>() + gap * (last - first - 1) as f32;
        let input = LayoutInput {
            run_mode: RunMode::ComputeSize,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Vertical,
            known_dimensions: Size { width: Some(cell_w), height: None },
            known_dimensions_are_definite: Size { width: true, height: true },
            parent_size: Size { width: Some(cell_w), height: None },
            available_space: Size { width: AvailableSpace::Definite(cell_w), height: AvailableSpace::MaxContent },
            vertical_margins_are_collapsible: Line::FALSE,
        };
        let h = w.compute_child_layout(taffy::NodeId::from(index), input).size.height;
        let row = cell.row as usize;
        let end = (row + cell.rowspan as usize).min(nr) - 1;
        ends[end].push((row, h));
    }
    let mut heights = vec![0.0f32; nr];
    // pos[r]: the top of row r, spacing between rows included.
    let mut pos = vec![0.0f32; nr + 1];
    for r in 0..nr {
        let mut h = ctx.rows[r].height.unwrap_or(0.0);
        for &(first, cell_h) in &ends[r] {
            h = h.max(cell_h - (pos[r] - pos[first]));
        }
        heights[r] = h.max(0.0);
        pos[r + 1] = pos[r] + heights[r] + spacing;
    }
    Some(heights)
}
