use blitz_traits::node_id::NodeId;
use std::{ops::Range, sync::Arc};
use style::logical_geometry::WritingMode;
use style::properties::ComputedValues;

use atomic_refcell::AtomicRefCell;
use markup5ever::local_name;
use style::properties::style_structs::Border;
use style::servo_arc::Arc as ServoArc;
use style::typed_om::NumericBaseType;
use style::values::computed::calc::CalcPercentageLeaf;
use style::values::computed::length_percentage::{
    CalcLengthPercentage, CalcNode, ComputedLeaf, Unpacked as UnpackedLengthPercentage,
};
use style::values::computed::{Length, LengthPercentage};
use style::values::generics::Optional;
use style::values::specified::box_::{DisplayInside, DisplayOutside};
use style::{
    Atom, computed_values::border_collapse::T as BorderCollapse,
    computed_values::table_layout::T as TableLayout, values::computed::BorderStyle,
};
use style_traits::values::specified::AllowedNumericType;
use taffy::{
    DetailedGridInfo, LayoutPartialTree as _, ResolveOrZero, TrackSizingFunction, style_helpers,
};

use crate::BaseDocument;
use crate::layout::LayoutPassState;

use super::damage::{CONSTRUCT_BOX, CONSTRUCT_DESCENDENT, CONSTRUCT_FC};
use super::resolve_calc_value;

pub struct TableTreeWrapper<'a, 'doc> {
    pub(crate) doc: &'a mut LayoutPassState<'doc>,
    pub(crate) ctx: Arc<TableContext>,
    /// wkhtmltopdf-rs: the table style with the column widths of WebKit's auto
    /// table layout (`table_auto.rs`)
    pub(crate) style_override: Option<taffy::Style<Atom>>,
}

// Deliberately not `Clone`: `style` may hold raw pointers into `calc_values`.
#[derive(Debug)]
pub struct TableContext {
    pub style: taffy::Style<Atom>,
    pub cells: Vec<TableCell>,
    /// wkhtmltopdf-rs: the distinct grid item styles of the cells (`TableCell::style`
    /// indexes it). Cells of a column usually share one style; storing a full
    /// `taffy::Style` per cell cost ~1.2 KB a cell.
    cell_styles: Vec<taffy::Style<Atom>>,
    /// Absolutely positioned descendants of the table's row groups and rows (and of the
    /// table itself). They take no part in the table grid.
    pub oof_children: Vec<(NodeId, taffy::Position)>,
    pub rows: Vec<TableRow>,
    pub columns: Vec<TableColumn>,
    pub computed_grid_info: AtomicRefCell<Option<DetailedGridInfo<Atom>>>,
    pub border_style: Option<ServoArc<Border>>,
    pub border_collapse: BorderCollapse,
    /// Backing storage for `calc()` track sizes synthesised by the table layout code.
    /// Taffy stores calc values as raw pointers, so these must outlive the `style`.
    #[allow(dead_code)]
    calc_values: Vec<LengthPercentage>,
    /// wkhtmltopdf-rs: whether the fixed table layout algorithm applies
    pub is_fixed: bool,
}

// #[derive(Debug, Clone, Eq, PartialEq)]
// pub enum TableItemKind {
//     Row,
//     Cell,
// }

#[derive(Debug, Clone)]
pub struct TableCell {
    // kind: TableItemKind,
    pub node_id: NodeId,
    /// Index (into `TableContext::rows`) of the row the cell originates in
    pub row: u16,
    /// Index of the column the cell originates in
    pub column: u16,
    /// wkhtmltopdf-rs: number of columns the cell spans
    pub colspan: u16,
    /// wkhtmltopdf-rs: the cell's specified width (border box), if any
    pub spec_width: Option<CellWidth>,
    /// wkhtmltopdf-rs: number of rows the cell spans
    pub rowspan: u16,
    /// wkhtmltopdf-rs: the resolved collapsed borders of the cell's edges (top,
    /// right, bottom, left), in the collapsing border model
    pub collapsed: Option<[CollapsedEdge; 4]>,
    /// wkhtmltopdf-rs: index of the cell's grid item style in `TableContext::cell_styles`
    /// (the style's `grid_row` is left out: see `grid_row_start`)
    style: u32,
    /// wkhtmltopdf-rs: the grid line the cell's row starts at (`grid-row: <line> / span
    /// <rowspan>`)
    grid_row_start: i16,
}

/// wkhtmltopdf-rs: interns the grid item styles of a table's cells. A style is compared
/// with the last style of the cell's column and the last style added (O(1) per cell),
/// which catches the repeated styles of a table's rows.
#[derive(Default)]
struct CellStyleInterner {
    styles: Vec<taffy::Style<Atom>>,
    last_by_column: Vec<u32>,
}

impl CellStyleInterner {
    fn intern(&mut self, style: taffy::Style<Atom>, column: u16) -> u32 {
        let column = column as usize;
        if column >= self.last_by_column.len() {
            self.last_by_column.resize(column + 1, u32::MAX);
        }
        let candidates = [self.last_by_column[column], (self.styles.len() as u32).wrapping_sub(1)];
        let index = candidates
            .into_iter()
            .find(|&i| self.styles.get(i as usize) == Some(&style))
            .unwrap_or_else(|| {
                self.styles.push(style);
                self.styles.len() as u32 - 1
            });
        self.last_by_column[column] = index;
        index
    }

    fn finish(mut self) -> Vec<taffy::Style<Atom>> {
        self.styles.shrink_to_fit();
        self.styles
    }
}

/// wkhtmltopdf-rs: a cell's grid item style: its interned style plus its own row placement.
pub struct CellStyle<'a> {
    base: &'a taffy::Style<Atom>,
    grid_row_start: i16,
    rowspan: u16,
}

impl taffy::CoreStyle for CellStyle<'_> {
    type CustomIdent = Atom;

    #[inline(always)]
    fn box_generation_mode(&self) -> taffy::BoxGenerationMode {
        self.base.box_generation_mode()
    }
    #[inline(always)]
    fn is_block(&self) -> bool {
        self.base.is_block()
    }
    #[inline(always)]
    fn is_compressible_replaced(&self) -> bool {
        self.base.is_compressible_replaced()
    }
    #[inline(always)]
    fn is_replaced(&self) -> bool {
        self.base.is_replaced()
    }
    #[inline(always)]
    fn box_sizing(&self) -> taffy::BoxSizing {
        self.base.box_sizing()
    }
    #[inline(always)]
    fn direction(&self) -> taffy::Direction {
        self.base.direction()
    }
    #[inline(always)]
    fn overflow(&self) -> taffy::Point<taffy::Overflow> {
        self.base.overflow()
    }
    #[inline(always)]
    fn scrollbar_width(&self) -> f32 {
        self.base.scrollbar_width()
    }
    #[inline(always)]
    fn position(&self) -> taffy::Position {
        self.base.position()
    }
    #[inline(always)]
    fn inset(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        self.base.inset()
    }
    #[inline(always)]
    fn is_containing_block(&self) -> taffy::ContainingBlockClaims {
        self.base.is_containing_block()
    }
    #[inline(always)]
    fn size(&self) -> taffy::Size<taffy::Dimension> {
        self.base.size()
    }
    #[inline(always)]
    fn min_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        self.base.min_size()
    }
    #[inline(always)]
    fn max_size(&self) -> taffy::Size<taffy::LengthPercentageAuto> {
        self.base.max_size()
    }
    #[inline(always)]
    fn aspect_ratio(&self) -> Option<f32> {
        self.base.aspect_ratio()
    }
    #[inline(always)]
    fn margin(&self) -> taffy::Rect<taffy::LengthPercentageAuto> {
        self.base.margin()
    }
    #[inline(always)]
    fn padding(&self) -> taffy::Rect<taffy::LengthPercentage> {
        self.base.padding()
    }
    #[inline(always)]
    fn border(&self) -> taffy::Rect<taffy::LengthPercentage> {
        self.base.border()
    }
    #[inline(always)]
    fn contain(&self) -> taffy::Contain {
        self.base.contain()
    }
}

impl taffy::GridItemStyle for CellStyle<'_> {
    #[inline(always)]
    fn grid_row(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        taffy::Line {
            start: style_helpers::line(self.grid_row_start),
            end: style_helpers::span(self.rowspan),
        }
    }
    #[inline(always)]
    fn grid_column(&self) -> taffy::Line<taffy::GridPlacement<Atom>> {
        taffy::GridItemStyle::grid_column(self.base)
    }
    #[inline(always)]
    fn align_self(&self) -> Option<taffy::AlignSelf> {
        taffy::GridItemStyle::align_self(self.base)
    }
    #[inline(always)]
    fn justify_self(&self) -> Option<taffy::AlignSelf> {
        taffy::GridItemStyle::justify_self(self.base)
    }
}

/// wkhtmltopdf-rs: one resolved collapsed border (CSS 2.1 §17.6.2.1).
#[derive(Debug, Clone, Copy)]
pub struct CollapsedEdge {
    pub width: f32,
    pub style: BorderStyle,
    pub color: style::color::AbsoluteColor,
}

impl CollapsedEdge {
    /// Whether the edge paints anything.
    pub fn is_painted(&self) -> bool {
        self.width > 0.0 && !self.style.none_or_hidden()
    }

    /// The part of the border outside the cell above/left of the boundary
    /// (WebKit gives the extra pixel to the cell below/right).
    pub fn outer_half(&self) -> f64 {
        (self.width as f64 / 2.0).floor()
    }

    /// The part of the border inside the cell below/right of the boundary.
    pub fn inner_half(&self) -> f64 {
        (self.width as f64 / 2.0).ceil()
    }

    const NONE: CollapsedEdge = CollapsedEdge {
        width: 0.0,
        style: BorderStyle::None,
        color: style::color::AbsoluteColor::TRANSPARENT_BLACK,
    };

    fn priority(style: BorderStyle) -> u8 {
        match style {
            BorderStyle::Double => 8,
            BorderStyle::Solid => 7,
            BorderStyle::Dashed => 6,
            BorderStyle::Dotted => 5,
            BorderStyle::Ridge => 4,
            BorderStyle::Outset => 3,
            BorderStyle::Groove => 2,
            BorderStyle::Inset => 1,
            _ => 0,
        }
    }

    /// Whether `other` wins over `self`: wider, then by style.
    fn loses_to(&self, other: &CollapsedEdge) -> bool {
        other.width > self.width
            || (other.width == self.width
                && Self::priority(other.style) > Self::priority(self.style))
    }
}

/// wkhtmltopdf-rs: a cell's specified width, as WebKit's `AutoTableLayout`
/// sees it.
#[derive(Debug, Clone, Copy)]
pub enum CellWidth {
    /// A length, as a border-box width in px
    Fixed(f32),
    /// A percentage of the table width (0..1)
    Percent(f32),
}

/// Tracks the current column position while walking the table's cells, so that
/// cells are assigned the same columns Taffy's dense auto-placement will give them
/// (skipping columns still occupied by rowspan cells from earlier rows).
#[derive(Debug, Default)]
struct ColumnCursor {
    /// The column the next cell in the current row will be placed in
    col: u16,
    /// The total number of columns seen so far
    num_columns: u16,
    /// For each column, the number of further rows it is occupied by a rowspan cell
    rowspans: Vec<u16>,
}

impl ColumnCursor {
    fn start_row(&mut self) {
        self.col = 0;
        for remaining in self.rowspans.iter_mut() {
            *remaining = remaining.saturating_sub(1);
        }
    }

    /// Advance past occupied columns, returning the column of the next cell
    fn next_free(&mut self) -> u16 {
        while self.rowspans.get(self.col as usize).is_some_and(|r| *r > 0) {
            self.col += 1;
        }
        self.col
    }

    fn place(&mut self, colspan: u16, rowspan: u16) {
        let end = self.col + colspan;
        if self.rowspans.len() < end as usize {
            self.rowspans.resize(end as usize, 0);
        }
        for remaining in &mut self.rowspans[self.col as usize..end as usize] {
            *remaining = rowspan - 1;
        }
        self.col = end;
        self.num_columns = self.num_columns.max(end);
    }
}

#[derive(Debug, Clone)]
pub struct TableColumn {
    pub node_id: NodeId,
    /// The column group (if any) that contains the column
    pub group: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub struct TableRow {
    // kind: TableItemKind,
    pub node_id: NodeId,
    /// The row group (if any) that contains the row
    pub group: Option<NodeId>,
    /// The row's specified `height`, if it is a length
    pub height: Option<f32>,
    /// Whether the row or any cell originating in it (and spanning only this row) has a
    /// specified length `height`. Such rows do not receive excess table height when there
    /// are unconstrained rows to receive it instead.
    pub is_constrained: bool,
}

/// The used width of one border side: border widths are not adjusted for
/// border-style in computed styles, so a border with `none`/`hidden` style
/// must be treated as zero-width.
/// The writing mode the table's grid runs in (the table's own)
#[cfg_attr(not(feature = "writing-mode"), allow(unused_variables))]
fn table_wm(doc: &BaseDocument, table_root_node_id: NodeId) -> WritingMode {
    #[cfg(feature = "writing-mode")]
    {
        if doc.try_root_element().map(|root| root.id) == Some(table_root_node_id) {
            doc.root_layout_wm(table_root_node_id)
        } else {
            doc.nodes[table_root_node_id].writing_mode()
        }
    }
    #[cfg(not(feature = "writing-mode"))]
    {
        WritingMode::empty()
    }
}

/// Convert a table part's style into the table's writing mode
fn table_taffy_style(style: &ComputedValues, table_wm: WritingMode) -> taffy::Style<Atom> {
    stylo_taffy::to_taffy_style_in(style, table_wm)
}

fn side_width(width: app_units::Au, style: BorderStyle) -> f32 {
    if style.none_or_hidden() {
        0.0
    } else {
        width.to_f32_px()
    }
}

/// Build a `calc(<percent> + <length>)` track sizing function. The calc value is
/// boxed and stored in `calc_values` so that the raw pointer Taffy holds stays valid.
fn percent_plus_length(
    calc_values: &mut Vec<LengthPercentage>,
    percent: f32,
    length: f32,
) -> TrackSizingFunction {
    let node = CalcNode::Sum(
        vec![
            CalcNode::Leaf(ComputedLeaf::Percentage(CalcPercentageLeaf::new(
                percent,
                Optional::Some(NumericBaseType::Length),
            ))),
            CalcNode::Leaf(ComputedLeaf::Length(Length::new(length))),
        ]
        .into(),
    );
    let value = LengthPercentage::new_calc(node, AllowedNumericType::NonNegative);
    let dim = match value.unpack() {
        UnpackedLengthPercentage::Calc(calc) => {
            let ptr = calc as *const CalcLengthPercentage as *const ();
            // SAFETY: the pointer targets the heap allocation owned by `value`, which
            // is kept alive by `calc_values` for as long as the TableContext exists.
            unsafe { taffy::Dimension::from_raw(taffy::CompactLength::calc(ptr)) }
        }
        UnpackedLengthPercentage::Length(len) => style_helpers::length(len.px()),
        UnpackedLengthPercentage::Percentage(p) => style_helpers::percent(p.0),
    };
    calc_values.push(value);
    dim.into()
}

pub(crate) fn build_table_context(
    doc: &mut BaseDocument,
    table_root_node_id: NodeId,
) -> (TableContext, Vec<NodeId>) {
    let mut cells: Vec<TableCell> = Vec::new();
    let mut cell_styles = CellStyleInterner::default();
    let mut oof_children: Vec<(NodeId, taffy::Position)> = Vec::new();
    let mut rows: Vec<TableRow> = Vec::new();
    let mut row = 0u16;
    let mut cursor = ColumnCursor::default();

    let wm = table_wm(doc, table_root_node_id);
    let root_node = &mut doc.nodes[table_root_node_id];

    let children = std::mem::take(&mut root_node.children);

    let Some(stylo_styles) = root_node.primary_styles() else {
        panic!("Ignoring table because it has no styles");
    };

    let mut style = table_taffy_style(&stylo_styles, wm);
    style.item_is_table = true;
    // Use `dense` row-flow so that each cell scans the row from its
    // leftmost column for the first free track. Without `dense`,
    // `place_definite_secondary_axis_item` keeps a per-item secondary
    // cursor across rows, which means cells in later rows do not
    // backfill columns freed up by rowspan cells from earlier rows.
    style.grid_auto_flow = taffy::GridAutoFlow::RowDense;
    style.grid_auto_columns = Vec::new();
    style.grid_auto_rows = Vec::new();

    // The fixed table layout algorithm only applies when the table has a non-auto width
    let is_fixed = match stylo_styles.clone_table_layout() {
        TableLayout::Fixed => !style.size.width.is_auto(),
        TableLayout::Auto => false,
    };

    let border_collapse = stylo_styles.clone_border_collapse();
    let border_spacing = stylo_styles.clone_border_spacing().0;

    drop(stylo_styles);

    let mut columns: Vec<TableColumn> = Vec::new();
    let mut column_sizes: Vec<taffy::TrackSizingFunction> = Vec::new();
    for child_id in children.iter().copied() {
        collect_columns(doc, child_id, wm, &mut columns, &mut column_sizes);
    }
    // Percentage column widths only take effect in the fixed table layout algorithm
    if !is_fixed {
        for column in column_sizes.iter_mut() {
            if column.max.into_raw().tag() == taffy::CompactLength::PERCENT_TAG {
                *column = style_helpers::auto();
            }
        }
    }
    let mut first_cell_border: Option<ServoArc<Border>> = None;
    // Percentage widths set on first-row cells: (column index, percentage, padding + border)
    let mut percent_columns: Vec<(u16, f32, f32)> = Vec::new();
    let mut calc_values: Vec<LengthPercentage> = Vec::new();
    // Header row groups are laid out before other rows, and footer row groups after
    let row_group_order = |doc: &BaseDocument, child_id: NodeId| -> u8 {
        let display = doc.nodes[child_id]
            .primary_styles()
            .map(|s| s.clone_display());
        match display.map(|d| d.inside()) {
            Some(DisplayInside::TableHeaderGroup) => 0,
            Some(DisplayInside::TableFooterGroup) => 2,
            _ => 1,
        }
    };
    for order in 0..3 {
        for child_id in children.iter().copied() {
            if row_group_order(doc, child_id) != order {
                continue;
            }
            collect_table_cells(
                doc,
                child_id,
                wm,
                is_fixed,
                border_collapse,
                &mut row,
                &mut cursor,
                &mut cells,
                &mut oof_children,
                &mut rows,
                &mut column_sizes,
                &mut first_cell_border,
                &mut percent_columns,
                &mut cell_styles,
            );
        }
    }
    let remaining_column = if is_fixed {
        style_helpers::minmax(style_helpers::length(0.0), style_helpers::fr(1.0))
    } else {
        style_helpers::auto()
    };
    // Trailing `<col>` columns which contain no cells and have no definite width
    // (i.e. would be zero-width) are dropped, so they don't add border-spacing.
    while column_sizes.len() > cursor.num_columns as usize
        && column_sizes
            .last()
            .is_some_and(|c| c.min.into_raw().tag() == taffy::CompactLength::AUTO_TAG)
    {
        column_sizes.pop();
        columns.pop();
    }
    let num_columns = cursor.num_columns.max(column_sizes.len() as u16);
    column_sizes.resize(num_columns as usize, remaining_column);
    if is_fixed {
        // In the fixed table layout algorithm, percentage column widths resolve against
        // the table's content width minus the horizontal border-spacing between columns,
        // whereas Taffy resolves percentage tracks against the container's inner width
        // (from which only the outer border-spacing has been removed via padding). Fold
        // the inner border-spacing into the percentage using `calc(N% - N * spacing)`.
        let spacing_x = match border_collapse {
            BorderCollapse::Separate => border_spacing.width.px(),
            BorderCollapse::Collapse => 0.0,
        };
        let inner_spacing = spacing_x * num_columns.saturating_sub(1) as f32;
        let mut percent_track = |percent: f32, extra: f32| -> TrackSizingFunction {
            let extra = extra - percent * inner_spacing;
            if extra == 0.0 {
                style_helpers::percent(percent)
            } else {
                percent_plus_length(&mut calc_values, percent, extra)
            }
        };

        for column in column_sizes.iter_mut() {
            if column.max.is_auto() {
                *column = remaining_column;
            } else if column.max.into_raw().tag() == taffy::CompactLength::PERCENT_TAG {
                *column = percent_track(column.max.into_raw().value(), 0.0);
            }
        }

        // Percentage widths on cells apply to the cell's content box, so the
        // cell's horizontal padding and border must be added to the column width.
        for (col_idx, percent, extra) in percent_columns {
            if let Some(column) = column_sizes.get_mut(col_idx as usize) {
                *column = percent_track(percent, extra);
            }
        }
    }

    style.grid_template_columns = column_sizes.into_iter().map(|dim| dim.into()).collect();
    // A row's `height` is a minimum: the row grows to fit the content of its cells.
    // Excess table height is distributed to unconstrained rows (via the stretching of
    // `auto` tracks) if there are any, and to all rows otherwise. So constrained rows
    // use a `max-content` (rather than `auto`) maximum when there are unconstrained rows.
    let has_unconstrained_rows = rows.iter().any(|row| !row.is_constrained);
    style.grid_template_rows = rows
        .iter()
        .map(|row| {
            let min = match row.height {
                Some(height) => style_helpers::length(height),
                None => style_helpers::auto(),
            };
            let max = if row.is_constrained && has_unconstrained_rows {
                style_helpers::max_content()
            } else {
                style_helpers::auto()
            };
            style_helpers::minmax(min, max)
        })
        .collect();

    style.gap = match border_collapse {
        BorderCollapse::Separate => {
            // In the separated borders model, `border-spacing` also applies between
            // the table border and the outermost cells, in addition to between cells.
            let spacing_x = border_spacing.width.px();
            let spacing_y = border_spacing.height.px();
            let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
            style.padding = taffy::Rect {
                left: style_helpers::length(padding.left + spacing_x),
                right: style_helpers::length(padding.right + spacing_x),
                top: style_helpers::length(padding.top + spacing_y),
                bottom: style_helpers::length(padding.bottom + spacing_y),
            };
            taffy::Size {
                width: style_helpers::length(spacing_x),
                height: style_helpers::length(spacing_y),
            }
        }
        // wkhtmltopdf-rs: WebKit's collapsed borders are not gutters: each cell
        // takes half of each of its collapsed borders, the table the outer halves
        // (`collapse_borders`).
        BorderCollapse::Collapse => taffy::Size::ZERO.map(style_helpers::length),
    };

    if border_collapse == BorderCollapse::Collapse {
        let [top, right, bottom, left] =
            collapse_borders(doc, table_root_node_id, &mut cells, &mut cell_styles, &rows, num_columns);
        style.border = taffy::Rect {
            left: style_helpers::length(left),
            right: style_helpers::length(right),
            top: style_helpers::length(top),
            bottom: style_helpers::length(bottom),
        };
    }

    let layout_children = cells
        .iter()
        .map(|cell| cell.node_id)
        .chain(oof_children.iter().map(|(node_id, _)| *node_id))
        .collect();
    let root_node = &mut doc.nodes[table_root_node_id];
    root_node.children = children;
    cells.shrink_to_fit();

    (
        TableContext {
            style,
            cells,
            cell_styles: cell_styles.finish(),
            oof_children,
            rows,
            columns,
            computed_grid_info: AtomicRefCell::new(None),
            border_collapse,
            border_style: first_cell_border,
            calc_values,
            is_fixed,
        },
        layout_children,
    )
}

/// wkhtmltopdf-rs: resolve the collapsed borders of every cell edge from the
/// cell, its row, the adjacent cells and rows and the table (WebKit's
/// `RenderTableCell::collapsed*Border`), give each cell its halves
/// (`RenderTableCell::borderHalf*`: the extra pixel goes to the top and left)
/// and return the table's outer halves (top, right, bottom, left).
fn collapse_borders(
    doc: &mut BaseDocument,
    table_id: NodeId,
    cells: &mut [TableCell],
    cell_styles: &mut CellStyleInterner,
    rows: &[TableRow],
    num_columns: u16,
) -> [f32; 4] {
    let (nr, nc) = (rows.len(), num_columns as usize);
    if nr == 0 || nc == 0 {
        return [0.0; 4];
    }
    // side: 0 top, 1 right, 2 bottom, 3 left
    let edge_of = |doc: &BaseDocument, id: NodeId, side: usize| -> CollapsedEdge {
        let Some(style) = doc.nodes[id].primary_styles() else {
            return CollapsedEdge::NONE;
        };
        let b = style.get_border();
        let current = style.clone_color();
        let (w, st, c) = match side {
            0 => (b.border_top_width.0, b.border_top_style, &b.border_top_color),
            1 => (b.border_right_width.0, b.border_right_style, &b.border_right_color),
            2 => (b.border_bottom_width.0, b.border_bottom_style, &b.border_bottom_color),
            _ => (b.border_left_width.0, b.border_left_style, &b.border_left_color),
        };
        CollapsedEdge {
            width: side_width(w, st),
            style: st,
            color: c.resolve_to_absolute(&current),
        }
    };
    let mut grid: Vec<Option<usize>> = vec![None; nr * nc];
    for (i, cell) in cells.iter().enumerate() {
        for r in cell.row as usize..(cell.row as usize + cell.rowspan as usize).min(nr) {
            for c in cell.column as usize..(cell.column as usize + cell.colspan as usize).min(nc) {
                grid[r * nc + c] = Some(i);
            }
        }
    }
    let resolve = |doc: &BaseDocument, candidates: &[(NodeId, usize)]| -> CollapsedEdge {
        let mut best = CollapsedEdge::NONE;
        for &(id, side) in candidates {
            let e = edge_of(doc, id, side);
            if e.style == BorderStyle::Hidden {
                return CollapsedEdge::NONE;
            }
            if best.loses_to(&e) {
                best = e;
            }
        }
        best
    };
    let mut outer = [0.0f32; 4];
    let mut results = Vec::with_capacity(cells.len());
    for cell in cells.iter() {
        let (r0, c0) = (cell.row as usize, cell.column as usize);
        let r1 = (r0 + cell.rowspan as usize).min(nr);
        let c1 = (c0 + cell.colspan as usize).min(nc);
        let id = cell.node_id;
        let row_id = |r: usize| rows[r].node_id;
        let mut sides = [CollapsedEdge::NONE; 4];
        // top
        let mut cand = vec![(id, 0), (row_id(r0), 0)];
        if r0 == 0 {
            cand.push((table_id, 0));
        } else {
            cand.push((row_id(r0 - 1), 2));
            for c in c0..c1 {
                if let Some(o) = grid[(r0 - 1) * nc + c] {
                    cand.push((cells[o].node_id, 2));
                }
            }
        }
        sides[0] = resolve(doc, &cand);
        // bottom
        let mut cand = vec![(id, 2), (row_id(r1 - 1), 2)];
        if r1 >= nr {
            cand.push((table_id, 2));
        } else {
            cand.push((row_id(r1), 0));
            for c in c0..c1 {
                if let Some(o) = grid[r1 * nc + c] {
                    cand.push((cells[o].node_id, 0));
                }
            }
        }
        sides[2] = resolve(doc, &cand);
        // left
        let mut cand = vec![(id, 3)];
        if c0 == 0 {
            cand.push((table_id, 3));
            cand.extend((r0..r1).map(|r| (row_id(r), 3)));
        } else {
            for r in r0..r1 {
                if let Some(o) = grid[r * nc + c0 - 1] {
                    cand.push((cells[o].node_id, 1));
                }
            }
        }
        sides[3] = resolve(doc, &cand);
        // right
        let mut cand = vec![(id, 1)];
        if c1 >= nc {
            cand.push((table_id, 1));
            cand.extend((r0..r1).map(|r| (row_id(r), 1)));
        } else {
            for r in r0..r1 {
                if let Some(o) = grid[r * nc + c1] {
                    cand.push((cells[o].node_id, 3));
                }
            }
        }
        sides[1] = resolve(doc, &cand);
        let (t, r, b, l) = (sides[0].width, sides[1].width, sides[2].width, sides[3].width);
        let halves = [(t / 2.0).ceil(), (r / 2.0).floor(), (b / 2.0).floor(), (l / 2.0).ceil()];
        if r0 == 0 {
            outer[0] = outer[0].max((t / 2.0).floor());
        }
        if r1 >= nr {
            outer[2] = outer[2].max((b / 2.0).ceil());
        }
        // The start and end borders of the table come from the first row only
        // (`RenderTable::calcBorderStart/End`); later rows' wider borders overflow.
        if c0 == 0 && r0 == 0 {
            outer[3] = outer[3].max((l / 2.0).floor());
        }
        if c1 >= nc && r0 == 0 {
            outer[1] = outer[1].max((r / 2.0).ceil());
        }
        results.push((sides, halves));
    }
    // The cells' styles get their border halves: intern them again.
    let old_styles = std::mem::take(cell_styles).styles;
    for (cell, (sides, halves)) in cells.iter_mut().zip(results) {
        cell.collapsed = Some(sides);
        let mut style = old_styles[cell.style as usize].clone();
        style.border = taffy::Rect {
            top: style_helpers::length(halves[0]),
            right: style_helpers::length(halves[1]),
            bottom: style_helpers::length(halves[2]),
            left: style_helpers::length(halves[3]),
        };
        cell.style = cell_styles.intern(style, cell.column);
        doc.nodes[cell.node_id].collapsed_border.set(Some(halves));
    }
    outer
}

/// Collect `<col>` elements (and `display: table-column` elements) along with their
/// widths, which take precedence over cell widths in the fixed table layout algorithm.
/// Columns without a specified width are recorded as `auto`.
fn collect_columns(
    doc: &mut BaseDocument,
    node_id: NodeId,
    wm: WritingMode,
    columns: &mut Vec<TableColumn>,
    column_sizes: &mut Vec<TrackSizingFunction>,
) {
    let node = &doc.nodes[node_id];
    if !node.is_element() {
        return;
    }
    let Some(display) = node.display_style() else {
        return;
    };

    match display.inside() {
        DisplayInside::TableColumnGroup => {
            let first_column = columns.len();
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_columns(doc, child_id, wm, columns, column_sizes);
            }
            doc.nodes[node_id].children = children;
            for column in &mut columns[first_column..] {
                column.group.get_or_insert(node_id);
            }
        }
        DisplayInside::TableColumn => {
            let style = table_taffy_style(&node.primary_styles().unwrap(), wm);
            let span: u16 = node
                .attr(local_name!("span"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|v| v.max(1))
                .unwrap_or(1);
            let column: TrackSizingFunction = match style.size.width.tag() {
                taffy::CompactLength::LENGTH_TAG => {
                    // A definite `max-width` clamps the column width; a zero width is treated as auto
                    let mut width = style.size.width.value();
                    if style.max_size.width.into_raw().tag() == taffy::CompactLength::LENGTH_TAG {
                        width = width.min(style.max_size.width.into_raw().value());
                    }
                    if width > 0.0 {
                        style_helpers::length(width)
                    } else {
                        style_helpers::auto()
                    }
                }
                taffy::CompactLength::PERCENT_TAG => {
                    style_helpers::percent(style.size.width.value())
                }
                // Browsers treat calc() widths on columns as auto
                _ => style_helpers::auto(),
            };
            // A definite `min-width` on a column acts as a floor on its width
            let column = match style.min_size.width.into_raw().tag() {
                taffy::CompactLength::LENGTH_TAG if column.max.is_auto() => style_helpers::minmax(
                    style_helpers::length(style.min_size.width.into_raw().value()),
                    style_helpers::auto(),
                ),
                _ => column,
            };
            for _ in 0..span {
                columns.push(TableColumn {
                    node_id,
                    group: None,
                });
                column_sizes.push(column);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn collect_table_cells(
    doc: &mut BaseDocument,
    node_id: NodeId,
    wm: WritingMode,
    is_fixed: bool,
    border_collapse: BorderCollapse,
    row: &mut u16,
    cursor: &mut ColumnCursor,
    cells: &mut Vec<TableCell>,
    oof_children: &mut Vec<(NodeId, taffy::Position)>,
    rows: &mut Vec<TableRow>,
    columns: &mut Vec<TrackSizingFunction>,
    first_cell_border: &mut Option<ServoArc<Border>>,
    percent_columns: &mut Vec<(u16, f32, f32)>,
    cell_styles: &mut CellStyleInterner,
) {
    let node = &mut doc.nodes[node_id];

    if !node.is_element() {
        return;
    }

    let Some(display) = node.display_style() else {
        #[cfg(feature = "tracing")]
        tracing::info!("Ignoring table descendent because it has no styles");
        return;
    };

    if display.outside() == DisplayOutside::None {
        node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
        return;
    }

    // Absolutely positioned boxes are out-of-flow: they are laid out by their containing
    // block rather than as part of the table grid.
    let position = node.primary_styles().unwrap().clone_position();
    if position.is_absolutely_positioned() {
        oof_children.push((node_id, stylo_taffy::convert::position(position)));
        return;
    }

    match display.inside() {
        DisplayInside::TableRowGroup
        | DisplayInside::TableHeaderGroup
        | DisplayInside::TableFooterGroup
        | DisplayInside::Contents => {
            let first_row = rows.len();
            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                doc.nodes[child_id]
                    .remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
                collect_table_cells(
                    doc,
                    child_id,
                    wm,
                    is_fixed,
                    border_collapse,
                    row,
                    cursor,
                    cells,
                    oof_children,
                    rows,
                    columns,
                    first_cell_border,
                    percent_columns,
                    cell_styles,
                );
            }
            doc.nodes[node_id].children = children;
            if display.inside() != DisplayInside::Contents {
                for row in &mut rows[first_row..] {
                    row.group.get_or_insert(node_id);
                }
            }
        }
        DisplayInside::TableRow => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            *row += 1;
            cursor.start_row();

            let height = node.primary_styles().and_then(|style| {
                let height = stylo_taffy::convert::dimension(&style.clone_height());
                (height.tag() == taffy::CompactLength::LENGTH_TAG).then(|| height.value())
            });
            rows.push(TableRow {
                node_id,
                group: None,
                height,
                is_constrained: height.is_some(),
            });

            let children = std::mem::take(&mut doc.nodes[node_id].children);
            for child_id in children.iter().copied() {
                collect_table_cells(
                    doc,
                    child_id,
                    wm,
                    is_fixed,
                    border_collapse,
                    row,
                    cursor,
                    cells,
                    oof_children,
                    rows,
                    columns,
                    first_cell_border,
                    percent_columns,
                    cell_styles,
                );
            }
            doc.nodes[node_id].children = children;
        }
        DisplayInside::TableCell => {
            // node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // wkhtmltopdf-rs: set again by `collapse_borders` for collapsed tables.
            node.collapsed_border.set(None);
            let stylo_style = &node.primary_styles().unwrap();
            let colspan: u16 = node
                .attr(local_name!("colspan"))
                .and_then(|val| val.parse().ok())
                .unwrap_or(1);
            let rowspan: u16 = node
                .attr(local_name!("rowspan"))
                .and_then(|val| val.parse::<u16>().ok())
                .map(|v| v.clamp(1, 65534))
                .unwrap_or(1);
            let mut style = table_taffy_style(stylo_style, wm);
            let col = cursor.next_free();

            // In the collapsed borders model the borders are laid out as gutters between
            // the cells (see the table's `gap`) rather than as part of the cells
            if border_collapse == BorderCollapse::Collapse {
                style.border = taffy::Rect::ZERO.map(style_helpers::length);
            }

            if first_cell_border.is_none() {
                *first_cell_border = Some(stylo_style.clone_border());
            }

            // In the fixed table layout algorithm the widths of columns are not
            // affected by cell contents, so cells must not impose a min-content
            // floor on their tracks.
            if is_fixed {
                style.min_size.width = style_helpers::length(0.0);
            }

            // Column widths from `<col>` elements take precedence over cell widths
            let col_needs_width =
                (col as usize) >= columns.len() || columns[col as usize].max.is_auto();
            if *row == 1 && col_needs_width {
                let column: TrackSizingFunction = match style.size.width.tag() {
                    taffy::CompactLength::LENGTH_TAG => {
                        let len = style.size.width.value();
                        let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                        let border = style.border.resolve_or_zero(None, resolve_calc_value);
                        match style.box_sizing {
                            taffy::BoxSizing::ContentBox => style_helpers::length(
                                len + padding.left + padding.right + border.left + border.right,
                            ),
                            taffy::BoxSizing::BorderBox => style_helpers::length(len),
                        }
                    }
                    taffy::CompactLength::PERCENT_TAG => {
                        if is_fixed {
                            let extra = match style.box_sizing {
                                taffy::BoxSizing::ContentBox => {
                                    let padding =
                                        style.padding.resolve_or_zero(None, resolve_calc_value);
                                    let border =
                                        style.border.resolve_or_zero(None, resolve_calc_value);
                                    padding.left + padding.right + border.left + border.right
                                }
                                taffy::BoxSizing::BorderBox => 0.0,
                            };
                            // Resolved in `build_table_context` once the column count is known
                            percent_columns.push((col, style.size.width.value(), extra));
                            style_helpers::percent(style.size.width.value())
                        } else {
                            style_helpers::auto()
                        }
                    }
                    taffy::CompactLength::AUTO_TAG => style_helpers::auto(),
                    // Dimension values are always length, percentage, auto or calc(),
                    // so any other tag is a calc() value. Pass it through so that
                    // Taffy resolves it against the table's inner width.
                    _ => style.size.width.into(),
                };
                if (col as usize) < columns.len() {
                    if !column.max.is_auto() {
                        columns[col as usize] = column;
                    }
                } else {
                    columns.resize(col as usize, style_helpers::auto());
                    columns.push(column);
                }
            }

            // The margin properties do not apply to table-internal elements
            style.margin = taffy::Rect::ZERO.map(style_helpers::length);

            // Let Taffy auto-place the column. Combined with
            // `grid_auto_flow: RowDense` set on the table root, each cell
            // scans from the first track in its row for a free position,
            // which makes cells automatically skip columns occupied by
            // rowspan cells from earlier rows.
            style.grid_column = taffy::Line {
                start: style_helpers::auto(),
                end: style_helpers::span(colspan),
            };
            // The row placement is `CellStyle::grid_row`, from `grid_row_start` and
            // `rowspan`: it differs on every row, which would defeat the interning.
            let grid_row_start = *row as i16;
            if rowspan == 1 && style.size.height.tag() == taffy::CompactLength::LENGTH_TAG {
                if let Some(row) = rows.last_mut() {
                    row.is_constrained = true;
                }
            }

            // wkhtmltopdf-rs: the specified width, for WebKit's auto table layout.
            let spec_width = match style.size.width.tag() {
                taffy::CompactLength::LENGTH_TAG => {
                    let len = style.size.width.value();
                    let padding = style.padding.resolve_or_zero(None, resolve_calc_value);
                    let border = style.border.resolve_or_zero(None, resolve_calc_value);
                    let extra = match style.box_sizing {
                        taffy::BoxSizing::ContentBox => {
                            padding.left + padding.right + border.left + border.right
                        }
                        taffy::BoxSizing::BorderBox => 0.0,
                    };
                    (len > 0.0).then_some(CellWidth::Fixed(len + extra))
                }
                taffy::CompactLength::PERCENT_TAG => {
                    Some(CellWidth::Percent(style.size.width.value()))
                }
                _ => None,
            };
            style.size.width = style_helpers::auto();
            cells.push(TableCell {
                node_id,
                row: row.saturating_sub(1),
                column: col,
                colspan: colspan.max(1),
                rowspan,
                collapsed: None,
                spec_width,
                style: cell_styles.intern(style, col),
                grid_row_start,
            });

            cursor.place(colspan, rowspan);
        }
        DisplayInside::Flow
        | DisplayInside::FlowRoot
        | DisplayInside::Flex
        | DisplayInside::Grid => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Probably a table caption: ignore
            // println!(
            //     "Warning: ignoring non-table typed descendent of table ({:?})",
            //     display.inside()
            // );
        }
        DisplayInside::TableColumnGroup | DisplayInside::TableColumn | DisplayInside::Table => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            //Ignore
        }
        DisplayInside::None => {
            node.remove_damage(CONSTRUCT_DESCENDENT | CONSTRUCT_FC | CONSTRUCT_BOX);
            // Ignore
        }
    }
}

pub struct RangeIter(Range<usize>);

impl Iterator for RangeIter {
    type Item = taffy::NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(taffy::NodeId::from)
    }
}

impl taffy::TraversePartialTree for TableTreeWrapper<'_, '_> {
    type ChildIter<'a>
        = RangeIter
    where
        Self: 'a;

    #[inline(always)]
    fn child_ids(&self, _node_id: taffy::NodeId) -> Self::ChildIter<'_> {
        RangeIter(0..self.ctx.cells.len())
    }

    #[inline(always)]
    fn child_count(&self, _node_id: taffy::NodeId) -> usize {
        self.ctx.cells.len()
    }

    #[inline(always)]
    fn get_child_id(&self, _node_id: taffy::NodeId, index: usize) -> taffy::NodeId {
        index.into()
    }
}
impl taffy::TraverseTree for TableTreeWrapper<'_, '_> {}

impl taffy::LayoutPartialTree for TableTreeWrapper<'_, '_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type CustomIdent = Atom;

    fn get_core_container_style(&self, _node_id: taffy::NodeId) -> &taffy::Style<Atom> {
        self.style_override.as_ref().unwrap_or(&self.ctx.style)
    }

    fn resolve_calc_value(&self, calc_ptr: *const (), parent_size: f32) -> f32 {
        resolve_calc_value(calc_ptr, parent_size)
    }

    fn set_unrounded_layout(&mut self, node_id: taffy::NodeId, layout: &taffy::Layout) {
        let node_id = crate::taffy_node_id(self.ctx.cells[usize::from(node_id)].node_id);
        self.doc.set_unrounded_layout(node_id, layout)
    }

    fn compute_child_layout(
        &mut self,
        node_id: taffy::NodeId,
        inputs: taffy::tree::LayoutInput,
    ) -> taffy::LayoutOutput {
        let cell = &self.ctx.cells[usize::from(node_id)];
        let node_id = crate::taffy_node_id(cell.node_id);
        self.doc.compute_child_layout(node_id, inputs)
    }
}

impl taffy::LayoutGridContainer for TableTreeWrapper<'_, '_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style<Atom>
    where
        Self: 'a;

    type GridItemStyle<'a>
        = CellStyle<'a>
    where
        Self: 'a;

    fn get_grid_container_style(&self, node_id: taffy::NodeId) -> Self::GridContainerStyle<'_> {
        self.get_core_container_style(node_id)
    }

    fn get_grid_child_style(&self, child_node_id: taffy::NodeId) -> Self::GridItemStyle<'_> {
        let cell = &self.ctx.cells[usize::from(child_node_id)];
        CellStyle {
            base: &self.ctx.cell_styles[cell.style as usize],
            grid_row_start: cell.grid_row_start,
            rowspan: cell.rowspan,
        }
    }

    fn set_detailed_grid_info(
        &mut self,
        _node_id: taffy::NodeId,
        detailed_grid_info: DetailedGridInfo<Atom>,
    ) {
        *self.ctx.computed_grid_info.borrow_mut() = Some(detailed_grid_info);
    }
}
