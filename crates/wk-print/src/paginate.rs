//! Page breaking. Stands in for WebKit's printing pagination as driven by
//! `QWebPrinter` (patched QtWebKit used by `pdfconverter.cc`): the document
//! is laid out once on a tall page, then cut into page-high slices.
//!
//! Rules, after QtWebKit 2.2's legacy truncation (`adjustPageHeight`): a page
//! is full height unless something unbreakable straddles its bottom edge — a
//! line's content area (ascent + descent, without the half-leading), a table
//! row, a replaced element, a `break-inside: avoid` box — and then it ends at
//! the top of that thing (of the line box for the first line of a block). The
//! next page starts at the cut when a box is cut through (the rest of it shows
//! at the top), else at the next content (margins in the gap are truncated).
//! Forced breaks (`page-break-*` /
//! `break-*`) start a new page, adjacent ones merge; when a cut falls inside
//! a table, its `thead` is repeated at the top of the next page and its
//! `tfoot` at the bottom of the cut page, as wkhtmltopdf 0.12.6 does.
//! `orphans`/`widows` are not applied: the 0.12.6 binary leaves single-line
//! widows (verified on `tests/corpus/report.html`, last page). Not yet: splitting a single row or line taller than a
//! page (it is cut at the page height), QtWebKit's exact heuristics.

use wk_webkit::Geometry;

/// One output page: the document band `[y0, y1)` (CSS px) plus repeated
/// table groups.
#[derive(Debug, Clone, PartialEq)]
pub struct PageSlice {
    pub y0: f32,
    pub y1: f32,
    /// Header group drawn above the band (document px range).
    pub thead: Option<(f32, f32)>,
    /// Footer group drawn below the band (document px range).
    pub tfoot: Option<(f32, f32)>,
}

impl PageSlice {
    /// Height of the repeated header drawn above the band.
    pub fn thead_height(&self) -> f32 {
        self.thead.map(|(a, b)| b - a).unwrap_or(0.0)
    }

    /// Vertical offset (px from the top of the content area) of document
    /// position `y`, when it lies in this page's band.
    pub fn offset_of(&self, y: f32) -> Option<f32> {
        if y >= self.y0 - 0.5 && y < self.y1 {
            Some(self.thead_height() + (y - self.y0).max(0.0))
        } else {
            None
        }
    }
}

/// The atomic intervals, for "does a cut at `y` fall strictly inside one of
/// them" (`a + 0.01 < y < b - 0.01`) in O(log n) rather than a scan of every
/// line box and row of the document per candidate.
struct Atomic {
    /// `a + 0.01` of each interval, ascending.
    starts: Vec<f32>,
    /// Running maximum of `b - 0.01`, in the order of `starts`.
    max_ends: Vec<f32>,
}

impl Atomic {
    fn new(g: &Geometry) -> Atomic {
        let mut v: Vec<(f32, f32)> = g.atomic.iter().map(|&(a, b)| (a + 0.01, b - 0.01)).collect();
        v.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut max = f32::NEG_INFINITY;
        let max_ends = v
            .iter()
            .map(|&(_, e)| {
                max = max.max(e);
                max
            })
            .collect();
        Atomic { starts: v.into_iter().map(|(s, _)| s).collect(), max_ends }
    }

    // `!(y < end)` on purpose: the same comparison as the linear scan it replaced, NaN included.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    fn is_safe(&self, y: f32) -> bool {
        let n = self.starts.partition_point(|&s| s < y);
        n == 0 || !(y < self.max_ends[n - 1])
    }
}

/// The first content start at or after `y` (or `y` when there is none).
fn next_content(g: &Geometry, y: f32) -> f32 {
    g.content_starts
        .iter()
        .copied()
        .find(|&c| c >= y - 0.01)
        .filter(|&c| c < g.doc_height)
        .unwrap_or(y)
        .max(y)
}

/// QtWebKit's legacy print rect for a page from `y` to `limit`: its bottom edge in whole px.
fn print_bottom(y: f32, limit: f32) -> f32 {
    y.floor() + (limit - y).ceil()
}

/// Whether something that must not be cut straddles the bottom of the page `[y, limit)`:
/// QtWebKit's `adjustPageHeight` paints the page with `truncatedAt = floor(limit)` and moves
/// the cut up to an object that starts above `truncatedAt` but ends below the print rect.
fn crosses(g: &Geometry, y: f32, limit: f32) -> bool {
    let (trunc, bottom) = (limit.floor(), print_bottom(y, limit));
    g.straddlers.iter().any(|&(a, b)| a < trunc && b > bottom && a > y + 1.0)
}

/// Whether a line box or an unbreakable box spans `y`.
fn straddled(g: &Geometry, y: f32) -> bool {
    g.content_boxes.iter().any(|&(a, b)| a < y - 0.01 && b > y + 0.01)
}

/// Cut `g` into pages of `page_h` CSS px.
pub fn paginate(g: &Geometry, page_h: f32) -> Vec<PageSlice> {
    let mut pages = vec![];
    let mut y = 0.0f32;
    let mut thead: Option<(f32, f32)> = None;
    let page_h = page_h.max(1.0);
    let atomic = Atomic::new(g);
    // Truncate the margins at the top of the first page.
    y = next_content(g, y);
    while y < g.doc_height - 0.5 && pages.len() < 100_000 {
        let thead_h = thead.map(|(a, b)| b - a).unwrap_or(0.0);
        let mut avail = (page_h - thead_h).max(page_h * 0.25);
        // A table that goes on past this page repeats its footer here.
        let mut tfoot = None;
        let mut tfoot_top = f32::INFINITY;
        for t in &g.tables {
            if let Some((fa, fb)) = t.tfoot {
                if t.top < y + avail && t.bottom > y + avail + 0.5 && fa > y {
                    tfoot = Some((fa, fb));
                    tfoot_top = fa;
                    avail = (avail - (fb - fa)).max(page_h * 0.25);
                }
            }
        }
        let limit = (y + avail).min(tfoot_top);
        let forced = g.forced.iter().copied().find(|&f| f > y + 0.5 && f <= limit);
        if let Some(f) = forced {
            // Adjacent forced breaks (e.g. `after` on one box, `before` on the
            // next) separated only by margins must not produce an empty page.
            if !g.atomic.iter().any(|&(a, b)| a < f && b > y) && !pages.is_empty() {
                y = f;
                continue;
            }
        }
        let end = if let Some(f) = forced {
            f
        } else if limit >= g.doc_height {
            g.doc_height
        } else if crosses(g, y, limit) {
            // QtWebKit's `bestTruncatedAt`: the page ends at the top of what straddles it.
            g.candidates
                .iter()
                .copied()
                .filter(|&c| c > y + 1.0 && c <= print_bottom(y, limit) && atomic.is_safe(c))
                .fold(None, |acc: Option<f32>, c| Some(acc.map_or(c, |a| a.max(c))))
                .unwrap_or(limit)
        } else {
            // Nothing straddles the cut: the page is full height, and the next one starts
            // inside whatever gap (margins) the cut falls in.
            limit
        };
        // The footer is only repeated when the table really continues.
        let tfoot = tfoot.filter(|_| g.tables.iter().any(|t| t.tfoot == tfoot && t.bottom > end + 0.5));
        pages.push(PageSlice { y0: y, y1: end, thead, tfoot });
        // Repeat the header group when the break falls inside a table body.
        thead = g
            .tables
            .iter()
            .filter(|t| t.top < end && end < t.bottom - 0.5)
            .filter_map(|t| t.thead.filter(|&(_, hb)| hb <= end + 0.5))
            .next_back();
        // A page after an unforced break starts at the next content: the
        // margins adjoining the break are truncated.
        // The next page starts at the cut (QtWebKit's `printedPagesHeight`) when content
        // straddles it, so the rest of a box cut through shows at its top; a cut in a gap
        // between boxes moves to the next content: the adjoining margins are truncated.
        // After a forced break (`page-break-after` puts it at the bottom of its box) the
        // margins down to the next box are truncated too, unless a box edge (an ancestor's
        // bottom padding or border) lies in between.
        y = if thead.is_some() || straddled(g, end) {
            end
        } else if forced.is_some() {
            let next = next_content(g, end);
            // `candidates` is sorted (`collect_geometry`).
            let first = g.candidates.partition_point(|&c| c <= end + 0.5);
            let edge_between = g.candidates.get(first).is_some_and(|&c| c < next - 0.5);
            if edge_between { end } else { next }
        } else {
            next_content(g, end)
        };
        // Skip the real footer of a table that ends right after the cut.
        if let Some((fa, fb)) = tfoot {
            if (y - fa).abs() < 0.5 {
                y = fb;
            }
        }
    }
    if pages.is_empty() {
        pages.push(PageSlice { y0: 0.0, y1: page_h, thead: None, tfoot: None });
    }
    if std::env::var_os("WKHTMLTOPDF_DEBUG_PAGES").is_some() {
        eprintln!("paginate: page_h={page_h} doc_h={}", g.doc_height);
        for (i, p) in pages.iter().enumerate() {
            eprintln!("  page {}: [{}, {}) thead={:?} tfoot={:?}", i + 1, p.y0, p.y1, p.thead, p.tfoot);
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use wk_webkit::TableInfo;

    #[test]
    fn short_document_is_one_page() {
        let g = Geometry { doc_height: 100.0, candidates: vec![0.0, 100.0], ..Default::default() };
        assert_eq!(paginate(&g, 1000.0).len(), 1);
    }

    #[test]
    fn lines_are_not_cut_and_forced_breaks_apply() {
        let mut g = Geometry { doc_height: 250.0, ..Default::default() };
        for i in 0..10 {
            let a = i as f32 * 25.0;
            g.atomic.push((a, a + 25.0));
            g.straddlers.push((a, a + 25.0));
            g.candidates.extend([a, a + 25.0]);
        }
        let p = paginate(&g, 110.0);
        assert_eq!(p.iter().map(|s| s.y1).collect::<Vec<_>>(), vec![100.0, 200.0, 250.0]);
        g.forced.push(50.0);
        let p = paginate(&g, 110.0);
        assert_eq!(p[0].y1, 50.0);
    }

    /// A paragraph of 20px lines with an 18px content area (1px half-leading).
    fn paragraph(g: &mut Geometry, top: f32, lines: usize) {
        for i in 0..lines {
            let a = top + i as f32 * 20.0;
            let (ca, cb) = (a + 1.0, a + 19.0);
            g.atomic.push((if i == 0 { a } else { ca }, cb));
            g.straddlers.push((ca, cb));
            g.content_boxes.push((a, a + 20.0));
            g.content_starts.push(a);
            g.candidates.extend([a, a + 20.0, ca, cb]);
        }
    }

    #[test]
    fn breaks_follow_qtwebkit_truncation() {
        // A line whose content area straddles the page bottom moves the cut to the top of its
        // content area (first line of a block: its line box top).
        let mut g = Geometry { doc_height: 300.0, ..Default::default() };
        paragraph(&mut g, 0.0, 15);
        let p = paginate(&g, 110.0);
        assert_eq!(p[0].y1, 101.0);
        assert_eq!(p[1].y0, 101.0);
        let mut g = Geometry { doc_height: 300.0, ..Default::default() };
        paragraph(&mut g, 0.0, 5);
        paragraph(&mut g, 105.0, 5);
        assert_eq!(paginate(&g, 110.0)[0].y1, 105.0);
        // A line box whose content area ends above the page bottom stays: the page is full
        // height, and the next one starts at the cut, inside that line box.
        let mut g = Geometry { doc_height: 300.0, ..Default::default() };
        paragraph(&mut g, 0.0, 5);
        paragraph(&mut g, 120.0, 5);
        let p = paginate(&g, 159.5);
        assert_eq!((p[0].y1, p[1].y0), (159.5, 159.5));
        // A cut in the gap between two blocks: the next page starts at the next content.
        let p = paginate(&g, 110.0);
        assert_eq!((p[0].y1, p[1].y0), (110.0, 120.0));
    }

    #[test]
    fn margins_after_a_forced_break_are_truncated() {
        // `page-break-after` on a box ending at 40, the next block's content at 60.
        let mut g = Geometry { doc_height: 300.0, ..Default::default() };
        paragraph(&mut g, 0.0, 2);
        paragraph(&mut g, 60.0, 2);
        g.forced.push(40.0);
        let p = paginate(&g, 1000.0);
        assert_eq!((p[0].y1, p[1].y0), (40.0, 60.0));
        // An ancestor's bottom edge in the gap (its padding, border) stays on the next page.
        g.candidates.push(50.0);
        g.candidates.sort_by(f32::total_cmp);
        let p = paginate(&g, 1000.0);
        assert_eq!(p[1].y0, 40.0);
    }

    #[test]
    fn thead_and_tfoot_repeat() {
        let mut g = Geometry { doc_height: 400.0, ..Default::default() };
        for i in 0..20 {
            let a = i as f32 * 20.0;
            g.atomic.push((a, a + 20.0));
            g.straddlers.push((a, a + 20.0));
            g.candidates.extend([a, a + 20.0]);
        }
        g.tables.push(TableInfo {
            top: 0.0,
            bottom: 400.0,
            thead: Some((0.0, 20.0)),
            tfoot: Some((380.0, 400.0)),
        });
        let p = paginate(&g, 200.0);
        assert!(p.len() >= 3);
        assert_eq!(p[0].tfoot, Some((380.0, 400.0)));
        assert_eq!(p[1].thead, Some((0.0, 20.0)));
        // The real footer ends the table; it is not drawn twice.
        assert!(p.last().unwrap().y1 >= 400.0);
    }
}
