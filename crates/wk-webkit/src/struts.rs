//! Pagination struts of table cells laid out at stale coordinates, as in the first print
//! layout pass of wkhtmltopdf's patched QtWebKit (`RenderTable::layout`,
//! `RenderTableRow::layout` and `RenderTableSection::layoutRows`, qt branch `wk_4.8.7`).
//!
//! Printing relays the page out at the print width with a page height
//! (`FrameView::forceLayoutForPagination`). A table lays its sections out before
//! positioning them, and a row lays its cells out where they still are: at the coordinates
//! of the previous layout, the screen layout of the page at the default viewport (0×0, where
//! a table shrinks to its minimum width and its rows are tall), each cell content still
//! offset by the vertical-align padding of that layout. A line of a cell that crosses a page
//! boundary at those coordinates is pushed to the next page (`adjustLinePositionForPagination`):
//! the cell grows by that strut, and so does its row (`calcRowLogicalHeight`). `layoutRows`
//! then positions the rows and aligns each cell in its row from the cell's height *with* the
//! strut, before laying it out again where it really is, without the strut, keeping the
//! row height. So a row whose stale cells straddled a page boundary is taller than its
//! content, the cell with the largest strut at the top of the row, the others centred as if
//! they were taller (verified on the 0.12.6 binary: `tests/fidelity/cases/doc-invoice.html`,
//! row SKU-1444, 13 px taller; the screen layout read back through `--debug-javascript`).
//!
//! Here: the cells' content origins are recorded from a layout at the screen viewport
//! ([`stale_origins`]), then, on the print layout, each cell's lines are moved to the table's
//! print position plus that stale origin and paginated like QtWebKit does
//! ([`cell_struts`]); a cell's strut is laid out as extra bottom padding (its row grows by it
//! and its content is aligned as in `layoutRows`).

use std::collections::HashMap;

use blitz_dom::{BaseDocument, Node, NodeId};
use style::values::computed::BreakWithin;

fn tag(node: &Node) -> &str {
    node.element_data().map(|e| &*e.name.local).unwrap_or("")
}

fn is_cell(node: &Node) -> bool {
    matches!(tag(node), "td" | "th")
}

/// The layout children of `node` (its DOM children when it has none).
fn children(node: &Node) -> Vec<NodeId> {
    match node.layout_children.borrow().as_ref() {
        Some(c) => c.to_vec(),
        None => node.children.to_vec(),
    }
}

/// Whether `node` is out of the flow of its parent: floated, absolutely positioned or not
/// displayed.
fn out_of_flow(node: &Node) -> bool {
    use style::computed_values::{float::T as Float, position::T as Position};
    node.primary_styles().is_none_or(|s| {
        let b = s.get_box();
        b.display.is_none()
            || b.float != Float::None
            || matches!(b.position, Position::Absolute | Position::Fixed)
    })
}

/// Where the content of a cell starts (absolute y): below its top border and padding and
/// its vertical-align offset, at its first line or the margin edge of its first in-flow box.
fn content_origin(doc: &BaseDocument, cell: &Node) -> Option<f32> {
    let y = cell.absolute_position(0.0, 0.0).y;
    let l = cell.final_layout();
    if let Some(text) = cell.element_data().and_then(|e| e.inline_layout_data.as_ref()) {
        text.layout.lines().next()?;
        return Some(y + l.border.top + l.padding.top + text.block_offset);
    }
    children(cell)
        .into_iter()
        .filter_map(|id| doc.get_node(id))
        .find(|c| c.is_element() && !out_of_flow(c))
        .map(|c| {
            let cl = c.final_layout();
            y + cl.location.y - cl.margin.top
        })
}

/// The content areas (ascent to descent: QtWebKit's `lineTop`/`lineBottom`) of the lines of
/// `node` (unless `skip_own`) and of its in-flow descendants, nested tables aside, in
/// absolute y.
fn lines(doc: &BaseDocument, node: &Node, skip_own: bool, out: &mut Vec<(f32, f32)>) {
    if let Some(text) = node.element_data().and_then(|e| e.inline_layout_data.as_ref()).filter(|_| !skip_own)
    {
        let l = node.final_layout();
        let y0 = node.absolute_position(0.0, 0.0).y + l.border.top + l.padding.top + text.block_offset;
        for line in text.layout.lines() {
            let m = line.metrics();
            out.push((
                y0 + m.content_block_min_coord,
                y0 + m.content_block_max_coord.max(m.content_block_min_coord),
            ));
        }
    }
    for id in children(node) {
        let Some(child) = doc.get_node(id) else { continue };
        if child.is_element() && !out_of_flow(child) && tag(child) != "table" {
            lines(doc, child, false, out);
        }
    }
}

/// Calls `f(table, cell)` for the cells of the tables not nested in a cell.
fn for_each_cell(doc: &BaseDocument, mut f: impl FnMut(&Node, &Node)) {
    fn cells<'a>(doc: &'a BaseDocument, node: &'a Node, out: &mut Vec<&'a Node>) {
        for id in children(node) {
            let Some(c) = doc.get_node(id) else { continue };
            if is_cell(c) {
                out.push(c);
            } else if tag(c) != "table" {
                cells(doc, c, out);
            }
        }
    }
    let mut stack = vec![doc.root_element().id];
    while let Some(id) = stack.pop() {
        let Some(n) = doc.get_node(id) else { continue };
        if tag(n) == "table" {
            let mut found = Vec::new();
            cells(doc, n, &mut found);
            for c in found {
                f(n, c);
            }
            continue;
        }
        if is_cell(n) {
            continue;
        }
        stack.extend(children(n));
    }
}

/// Whether the document has a table cell.
pub(crate) fn has_cells(doc: &BaseDocument) -> bool {
    let mut any = false;
    for_each_cell(doc, |_, _| any = true);
    any
}

/// The content origin of every cell relative to its table's top, in the current layout.
pub(crate) fn stale_origins(doc: &BaseDocument) -> HashMap<NodeId, f32> {
    let mut out = HashMap::new();
    for_each_cell(doc, |table, cell| {
        if let Some(o) = content_origin(doc, cell) {
            out.insert(cell.id, o - table.absolute_position(0.0, 0.0).y);
        }
    });
    out
}

/// The strut of every cell (0 for most) when its lines are laid out at the table's position
/// plus the cell's stale origin and paginated every `page_h` CSS px from `first` (the top of
/// the first page: its first content, the margins above being truncated; measured on the
/// 0.12.6 binary with body margins of 0, 8 and 30 px), in whole layout pixels (`zoom` of
/// them a CSS px).
pub(crate) fn cell_struts(
    doc: &BaseDocument,
    stale: &HashMap<NodeId, f32>,
    first: f32,
    page_h: f32,
    zoom: f32,
) -> Vec<(NodeId, f32)> {
    let page = (page_h * zoom).round();
    let mut out = Vec::new();
    if page < 1.0 {
        return out;
    }
    for_each_cell(doc, |table, cell| {
        let (Some(&stale), Some(origin)) = (stale.get(&cell.id), content_origin(doc, cell)) else {
            out.push((cell.id, 0.0));
            return;
        };
        let top = table.absolute_position(0.0, 0.0).y + stale;
        // The lines of a cell whose row is `page-break-inside: avoid` are not paginated
        // (`isTableCellWithPageBreakInsideAvoid`), those of the blocks inside it are.
        let row_avoids = cell.parent.and_then(|p| doc.get_node(p)).is_some_and(|row| {
            tag(row) == "tr"
                && row
                    .primary_styles()
                    .is_some_and(|s| !matches!(s.get_box().break_inside, BreakWithin::Auto))
        });
        let mut ls = Vec::new();
        lines(doc, cell, row_avoids, &mut ls);
        ls.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut delta = 0.0f32;
        for (a, b) in ls {
            // RenderBlock::adjustLinePositionForPagination, in integers.
            let y = ((top + a - origin - first) * zoom).round() + delta;
            let h = ((b - a) * zoom).round();
            if h <= 0.0 || h > page {
                continue;
            }
            let remaining = page - y.rem_euclid(page);
            if remaining < h {
                delta += remaining;
            }
        }
        out.push((cell.id, delta / zoom));
    });
    out
}

/// Clear the struts of a previous print layout.
pub(crate) fn reset(doc: &mut BaseDocument) {
    let mut ids = Vec::new();
    for_each_cell(doc, |_, cell| {
        if cell.pagination_strut.get() != 0.0 {
            ids.push(cell.id);
        }
    });
    for id in ids {
        doc.set_pagination_strut(id, 0.0);
    }
}
