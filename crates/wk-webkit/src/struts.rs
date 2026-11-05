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
//! Here, only for the tables whose cell lines QtWebKit paginates ([`paginated_tables`]: most
//! documents have none, e.g. when rows are `page-break-inside: avoid`): the cells' lines are
//! recorded from the print layout ([`capture`]), their content origins from a layout at the
//! screen viewport ([`stale_origins`]; at the default 0-wide viewport, of each table on its
//! own), then each cell's lines are moved to the table's print position plus that stale
//! origin and paginated like QtWebKit does ([`struts`]); a cell's strut is laid out as extra
//! bottom padding (its row grows by it and its content is aligned as in `layoutRows`).

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

/// Whether `f` holds for any layout child of `node` (its DOM children when it has none),
/// without copying the child list.
fn any_child(node: &Node, mut f: impl FnMut(NodeId) -> bool) -> bool {
    match node.layout_children.borrow().as_ref() {
        Some(c) => c.iter().any(|&id| f(id)),
        None => node.children.iter().any(|&id| f(id)),
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

/// The tables not nested in a table cell, in document order.
fn tables(doc: &BaseDocument) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![doc.root_element().id];
    while let Some(id) = stack.pop() {
        let Some(n) = doc.get_node(id) else { continue };
        if tag(n) == "table" {
            out.push(id);
            continue;
        }
        if is_cell(n) {
            continue;
        }
        let mut c = children(n);
        c.reverse();
        stack.extend(c);
    }
    out
}

/// The cells of `table` (not those of nested tables).
fn cells(doc: &BaseDocument, table: NodeId) -> Vec<&Node> {
    fn walk<'a>(doc: &'a BaseDocument, node: &'a Node, out: &mut Vec<&'a Node>) {
        any_child(node, |id| {
            if let Some(c) = doc.get_node(id) {
                if is_cell(c) {
                    out.push(c);
                } else if tag(c) != "table" {
                    walk(doc, c, out);
                }
            }
            false
        });
    }
    let mut out = Vec::new();
    if let Some(t) = doc.get_node(table) {
        walk(doc, t, &mut out);
    }
    out
}

/// Whether the row of `cell` is `page-break-inside: avoid`: the lines of the cell itself are
/// then not paginated (`isTableCellWithPageBreakInsideAvoid`), those of the blocks inside
/// it are.
fn row_avoids(doc: &BaseDocument, cell: &Node) -> bool {
    cell.parent.and_then(|p| doc.get_node(p)).is_some_and(|row| {
        tag(row) == "tr"
            && row.primary_styles().is_some_and(|s| !matches!(s.get_box().break_inside, BreakWithin::Auto))
    })
}

/// Whether `node` has a block-level descendant (nested tables aside).
fn has_block(doc: &BaseDocument, node: &Node) -> bool {
    any_child(node, |id| {
        doc.get_node(id).is_some_and(|c| {
            c.is_element()
                && tag(c) != "table"
                && (c.primary_styles().is_some_and(|s| !s.get_box().display.is_inline_flow())
                    || has_block(doc, c))
        })
    })
}

/// The tables some of whose cell lines QtWebKit paginates in its first print layout pass:
/// those with a cell in a row that is not `page-break-inside: avoid`, or with a block inside.
pub(crate) fn paginated_tables(doc: &BaseDocument) -> Vec<NodeId> {
    tables(doc)
        .into_iter()
        .filter(|&t| cells(doc, t).into_iter().any(|c| !row_avoids(doc, c) || has_block(doc, c)))
        .collect()
}

/// Whether the stale cells of `table` may reach a page boundary (pages of `page` CSS px from
/// `first`): an upper bound of the table's height in the screen layout, where each cell's
/// text may break at every character other than a letter or a digit, must cross one. Every
/// row grows at most by its tallest cell's growth. Unknown content (a block or an inline box
/// in a cell) may always cross.
pub(crate) fn may_cross(doc: &BaseDocument, table: NodeId, first: f32, page: f32) -> bool {
    let Some(t) = doc.get_node(table) else { return false };
    let mut growth: HashMap<Option<NodeId>, f32> = HashMap::new();
    for cell in cells(doc, table) {
        let l = cell.final_layout();
        let Some(text) = cell.element_data().and_then(|e| e.inline_layout_data.as_ref()) else {
            if has_block(doc, cell) {
                return true;
            }
            continue;
        };
        if text.layout.inline_boxes().len() > 0 || has_block(doc, cell) {
            return true;
        }
        let (mut n, mut line_h) = (0usize, 0.0f32);
        for line in text.layout.lines() {
            let m = line.metrics();
            n += 1;
            line_h = line_h.max(m.block_max_coord - m.block_min_coord);
        }
        let breaks = text.text.chars().filter(|c| !c.is_alphanumeric() || (*c as u32) >= 0x2e80).count() + 1;
        let pb = l.border.top + l.border.bottom + l.padding.top + l.padding.bottom;
        let bound = pb + breaks.max(n) as f32 * line_h;
        let g = growth.entry(cell.parent).or_insert(0.0);
        *g = g.max(bound - l.size.height);
    }
    let y0 = t.absolute_position(0.0, 0.0).y - first;
    let y1 = y0 + t.final_layout().size.height + growth.values().sum::<f32>() + 1.0;
    page < 1.0 || (y0 / page).floor() != (y1 / page).floor()
}

/// Whether `table` gets a 0 width from a 0-wide viewport: its ancestors are in-flow blocks
/// without a specified width.
pub(crate) fn isolable(doc: &BaseDocument, table: NodeId) -> bool {
    use style::values::specified::box_::{DisplayInside, DisplayOutside};
    let mut id = doc.get_node(table).and_then(|t| t.parent);
    while let Some(i) = id {
        let Some(n) = doc.get_node(i) else { return false };
        if !n.is_element() {
            break;
        }
        let Some(s) = n.primary_styles() else { return false };
        let d = s.get_box().display;
        let pos = s.get_position();
        if out_of_flow(n)
            || d.outside() != DisplayOutside::Block
            || !matches!(d.inside(), DisplayInside::Flow | DisplayInside::FlowRoot)
            || !pos.width.is_auto()
            || !pos.min_width.is_auto()
        {
            return false;
        }
        id = n.parent;
    }
    true
}

/// The content origin of every cell of `tables` relative to its table's top, in the current
/// layout.
pub(crate) fn stale_origins(doc: &BaseDocument, tables: &[NodeId]) -> HashMap<NodeId, f32> {
    let mut out = HashMap::new();
    for &t in tables {
        let Some(table) = doc.get_node(t) else { continue };
        let top = table.absolute_position(0.0, 0.0).y;
        for cell in cells(doc, t) {
            if let Some(o) = content_origin(doc, cell) {
                out.insert(cell.id, o - top);
            }
        }
    }
    out
}

/// The print layout of the cells of some tables, as their struts need it: each cell with
/// its table's top and the content areas of its paginated lines, relative to the cell's
/// content origin.
pub(crate) struct PrintCells(Vec<PrintCell>);

/// A cell, its table's top and its line content areas (see [`PrintCells`]).
type PrintCell = (NodeId, f32, Vec<(f32, f32)>);

/// The [`PrintCells`] of `tables` in the current layout.
pub(crate) fn capture(doc: &BaseDocument, tables: &[NodeId]) -> PrintCells {
    let mut out = Vec::new();
    for &t in tables {
        let Some(table) = doc.get_node(t) else { continue };
        let table_top = table.absolute_position(0.0, 0.0).y;
        for cell in cells(doc, t) {
            let mut ls = Vec::new();
            if let Some(origin) = content_origin(doc, cell) {
                lines(doc, cell, row_avoids(doc, cell), &mut ls);
                ls.sort_by(|a, b| a.0.total_cmp(&b.0));
                for l in &mut ls {
                    *l = (l.0 - origin, l.1 - origin);
                }
            }
            out.push((cell.id, table_top, ls));
        }
    }
    PrintCells(out)
}

/// The strut of every cell (0 for most) when its lines are laid out at its table's print
/// position plus its stale origin and paginated every `page_h` CSS px from `first` (the top
/// of the first page: its first content, the margins above being truncated; measured on the
/// 0.12.6 binary with body margins of 0, 8 and 30 px), in whole layout pixels (`zoom` of
/// them a CSS px).
pub(crate) fn struts(
    print: &PrintCells,
    stale: &HashMap<NodeId, f32>,
    first: f32,
    page_h: f32,
    zoom: f32,
) -> Vec<(NodeId, f32)> {
    let page = (page_h * zoom).round();
    let mut out = Vec::with_capacity(print.0.len());
    for (cell, table_top, ls) in &print.0 {
        let Some(&stale) = stale.get(cell).filter(|_| page >= 1.0) else {
            out.push((*cell, 0.0));
            continue;
        };
        let top = table_top + stale;
        let mut delta = 0.0f32;
        for &(a, b) in ls {
            // RenderBlock::adjustLinePositionForPagination, in integers.
            let y = ((top + a - first) * zoom).round() + delta;
            let h = ((b - a) * zoom).round();
            if h <= 0.0 || h > page {
                continue;
            }
            let remaining = page - y.rem_euclid(page);
            if remaining < h {
                delta += remaining;
            }
        }
        out.push((*cell, delta / zoom));
    }
    out
}

/// Clear the struts a previous print layout gave the cells of `tables`.
pub(crate) fn reset(doc: &mut BaseDocument, tables: &[NodeId]) {
    let ids: Vec<NodeId> = tables
        .iter()
        .copied()
        .flat_map(|t| cells(doc, t))
        .filter(|c| c.pagination_strut.get() != 0.0)
        .map(|c| c.id)
        .collect();
    for id in ids {
        doc.set_pagination_strut(id, 0.0);
    }
}
