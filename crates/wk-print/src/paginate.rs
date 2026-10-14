//! Page breaking. Stands in for WebKit's printing pagination as driven by
//! `QWebPrinter` (patched QtWebKit used by `pdfconverter.cc`): the document
//! is laid out once on a tall page, then cut into page-high slices.
//!
//! Rules: a cut never falls inside a line box, a table row, a replaced
//! element or a `break-inside: avoid` box; forced breaks (`page-break-*` /
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
        } else {
            g.candidates
                .iter()
                .copied()
                .filter(|&c| c > y + 1.0 && c <= limit && atomic.is_safe(c))
                .fold(None, |acc: Option<f32>, c| Some(acc.map_or(c, |a| a.max(c))))
                .unwrap_or(limit)
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
        y = if forced.is_some() || thead.is_some() { end } else { next_content(g, end) };
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
            g.candidates.extend([a, a + 25.0]);
        }
        let p = paginate(&g, 110.0);
        assert_eq!(p.iter().map(|s| s.y1).collect::<Vec<_>>(), vec![100.0, 200.0, 250.0]);
        g.forced.push(50.0);
        let p = paginate(&g, 110.0);
        assert_eq!(p[0].y1, 50.0);
    }

    #[test]
    fn thead_and_tfoot_repeat() {
        let mut g = Geometry { doc_height: 400.0, ..Default::default() };
        for i in 0..20 {
            let a = i as f32 * 20.0;
            g.atomic.push((a, a + 20.0));
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
