//! Spike paginator: "lay out once on a tall page, then slice", like QtWebKit printing.
//!
//! It collects, in document coordinates (CSS px), the box edges where a break is allowed,
//! the atomic intervals a break must not cut (line boxes, table rows, images,
//! `page-break-inside: avoid`), forced breaks, and table header groups to repeat.
use blitz_dom::{BaseDocument, Node};
use style::Atom;

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub top: f32,
    pub bottom: f32,
    /// Header group rectangle (top, bottom) in document px, when the table has a `<thead>`.
    pub thead: Option<(f32, f32)>,
}

#[derive(Default, Debug)]
pub struct Geometry {
    pub doc_height: f32,
    pub candidates: Vec<f32>,
    pub atomic: Vec<(f32, f32)>,
    pub forced: Vec<f32>,
    pub tables: Vec<TableInfo>,
}

#[derive(Debug, Clone)]
pub struct PageSlice {
    pub y0: f32,
    pub y1: f32,
    /// Header group repeated at the top of this page (document px range).
    pub thead: Option<(f32, f32)>,
}

fn custom_prop(node: &Node, name: &str) -> Option<String> {
    let styles = node.primary_styles()?;
    let v = styles.custom_properties().get_for_cssom(&Atom::from(name))?;
    Some(v.to_variable_value().css.trim().to_string())
}

/// True when `name` is set on this element itself (custom properties inherit, so compare
/// with the parent's value).
fn own_prop(doc: &BaseDocument, node: &Node, name: &str, want: &[&str]) -> bool {
    let Some(v) = custom_prop(node, name) else { return false };
    if !want.contains(&v.as_str()) {
        return false;
    }
    let parent = node.parent.and_then(|p| doc.get_node(p));
    parent.and_then(|p| custom_prop(p, name)).as_deref() != Some(v.as_str())
}

pub fn collect_geometry(doc: &BaseDocument) -> Geometry {
    let mut g = Geometry::default();
    let root = doc.root_element();
    walk(doc, root, &mut g);
    g.doc_height = g.doc_height.max(root.final_layout().size.height);
    g.candidates.push(g.doc_height);
    g.candidates.sort_by(|a, b| a.partial_cmp(b).unwrap());
    g.candidates.dedup();
    g.forced.sort_by(|a, b| a.partial_cmp(b).unwrap());
    g.forced.dedup();
    g
}

/// Vertical extent of a box. Blitz lays tables out as a grid of cells: `thead`/`tbody`/`tr`
/// get no box of their own, so fall back to the union of their descendants' cells.
fn extent(doc: &BaseDocument, node: &Node) -> Option<(f32, f32)> {
    let h = node.final_layout().size.height;
    if h > 0.0 {
        let y = node.absolute_position(0.0, 0.0).y;
        return Some((y, y + h));
    }
    let tag = node.element_data().map(|e| e.name.local.to_string()).unwrap_or_default();
    if !matches!(tag.as_str(), "thead" | "tbody" | "tfoot" | "tr") {
        return None;
    }
    node.children.iter().filter_map(|&c| doc.get_node(c)).filter_map(|c| extent(doc, c)).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

fn walk(doc: &BaseDocument, node: &Node, g: &mut Geometry) {
    let layout = node.final_layout();
    let (top, bottom) = extent(doc, node).unwrap_or_else(|| {
        let y = node.absolute_position(0.0, 0.0).y;
        (y, y)
    });
    g.doc_height = g.doc_height.max(bottom);
    let tag = node.element_data().map(|e| e.name.local.to_string()).unwrap_or_default();

    if node.is_element() {
        if own_prop(doc, node, "wk-break-before", &["always", "page", "left", "right"]) && top > 0.5 {
            g.forced.push(top);
        }
        if own_prop(doc, node, "wk-break-after", &["always", "page", "left", "right"]) {
            g.forced.push(bottom);
        }
        g.candidates.push(top);
        g.candidates.push(bottom);
    }

    let atomic = matches!(tag.as_str(), "tr" | "td" | "th" | "img" | "svg" | "canvas" | "video" | "iframe" | "input" | "textarea" | "select" | "button")
        || (node.is_element() && own_prop(doc, node, "wk-break-inside", &["avoid"]));
    if atomic && bottom > top {
        g.atomic.push((top, bottom));
        return;
    }

    if tag == "table" {
        let mut thead = None;
        for &c in node.children.iter() {
            let child = doc.get_node(c).unwrap();
            if child.element_data().is_some_and(|e| &*e.name.local == "thead") {
                thead = extent(doc, child);
            }
        }
        g.tables.push(TableInfo { top, bottom, thead });
    }

    if let Some(text) = node.element_data().and_then(|e| e.inline_layout_data.as_ref()) {
        let y0 = top + layout.border.top + layout.padding.top + text.block_offset;
        for line in text.layout.lines() {
            let m = line.metrics();
            let (a, b) = (y0 + m.block_min_coord, y0 + m.block_max_coord);
            g.atomic.push((a, b));
            g.candidates.push(a);
            g.candidates.push(b);
        }
    }

    let children = node.layout_children.borrow();
    let ids: Vec<_> = match children.as_ref() {
        Some(c) => c.iter().copied().collect(),
        None => node.children.iter().copied().collect(),
    };
    drop(children);
    for id in ids {
        if let Some(child) = doc.get_node(id) {
            walk(doc, child, g);
        }
    }
}

fn is_safe(g: &Geometry, y: f32) -> bool {
    !g.atomic.iter().any(|&(a, b)| a + 0.01 < y && y < b - 0.01)
}

pub fn paginate(g: &Geometry, page_h: f32) -> Vec<PageSlice> {
    let mut pages = vec![];
    let mut y = 0.0f32;
    let mut thead: Option<(f32, f32)> = None;
    while y < g.doc_height - 0.5 && pages.len() < 10_000 {
        let avail = page_h - thead.map(|(a, b)| b - a).unwrap_or(0.0);
        let limit = y + avail;
        let forced = g.forced.iter().copied().find(|&f| f > y + 0.5 && f <= limit);
        if let Some(f) = forced {
            // Adjacent forced breaks (e.g. `after` on one box, `before` on the next) separated
            // only by margins must not produce an empty page.
            if !g.atomic.iter().any(|&(a, b)| a < f && b > y) && !pages.is_empty() {
                y = f;
                continue;
            }
        }
        let end = if let Some(f) = forced {
            f
        } else if limit >= g.doc_height {
            g.doc_height
        } else {
            g.candidates
                .iter()
                .copied()
                .filter(|&c| c > y + 1.0 && c <= limit && is_safe(g, c))
                .fold(None, |acc: Option<f32>, c| Some(acc.map_or(c, |a| a.max(c))))
                .unwrap_or(limit)
        };
        pages.push(PageSlice { y0: y, y1: end, thead });
        // Repeat the header group when the break falls inside a table body.
        thead = g
            .tables
            .iter()
            .filter(|t| t.top < end && end < t.bottom - 0.5)
            .filter_map(|t| t.thead.filter(|&(_, hb)| hb <= end + 0.5))
            .last();
        y = end;
    }
    pages
}
