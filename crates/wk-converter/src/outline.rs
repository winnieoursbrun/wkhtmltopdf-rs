//! Document outline. Ports upstream `src/lib/outline.cc` (`Outline`,
//! `OutlineItem`): heading tree per document, `__WKANCHOR_` anchors, the PDF
//! bookmarks (`printOutline`) and `--dump-outline` (spec §3.5).
//!
//! TODO(P3): TOC objects (the TOC document's own entries, anchor reuse on
//! TOC rebuilds), `--enable-toc-back-links`, header/footer section values.

use std::fmt::Write as _;

/// One heading (or document root).
#[derive(Debug, Clone, Default)]
pub struct OutlineItem {
    pub value: String,
    /// 1-based page within its document for headings, 0 for roots.
    pub page: usize,
    /// Heading level (1..=6), 0 for roots.
    pub level: u32,
    pub anchor: String,
    pub toc_anchor: String,
    /// Output page index (0-based) and position (pt, top-left origin).
    pub out_page: usize,
    pub x: f32,
    pub y: f32,
    pub children: Vec<OutlineItem>,
}

/// A located heading, input of [`Outline::add_web_page`].
#[derive(Debug, Clone)]
pub struct LocatedHeading {
    pub level: u32,
    pub text: String,
    pub page: usize,
    pub out_page: usize,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Default)]
pub struct Outline {
    pub documents: Vec<OutlineItem>,
    /// Page count of each document as used for numbering (excluded
    /// documents count as one page).
    pub page_counts: Vec<usize>,
    anchor_counter: u64,
}

fn base36(mut n: u64) -> String {
    const D: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(D[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).expect("ascii")
}

impl Outline {
    fn next_anchor(&mut self) -> String {
        let a = format!("__WKANCHOR_{}", base36(self.anchor_counter));
        self.anchor_counter += 1;
        a
    }

    fn assign(&mut self, item: &mut OutlineItem) {
        item.anchor = self.next_anchor();
        item.toc_anchor = self.next_anchor();
        for c in &mut item.children {
            self.assign(c);
        }
    }

    /// `addWebPage`: the headings must be in (page, y, x) order.
    pub fn add_web_page(
        &mut self,
        title: &str,
        page_count: usize,
        out_first_page: usize,
        headings: &[LocatedHeading],
    ) {
        let mut root =
            OutlineItem { value: title.to_string(), out_page: out_first_page, ..Default::default() };
        // Level stack of paths into the tree.
        let mut stack: Vec<(u32, Vec<usize>)> = Vec::new();
        for h in headings {
            while stack.last().is_some_and(|(l, _)| *l >= h.level) {
                stack.pop();
            }
            let item = OutlineItem {
                value: h.text.clone(),
                page: h.page,
                level: h.level,
                out_page: h.out_page,
                x: h.x,
                y: h.y,
                ..Default::default()
            };
            let parent_path = stack.last().map(|(_, p)| p.clone()).unwrap_or_default();
            let mut parent = &mut root;
            for &i in &parent_path {
                parent = &mut parent.children[i];
            }
            parent.children.push(item);
            let mut path = parent_path;
            path.push(parent.children.len() - 1);
            stack.push((h.level, path));
        }
        self.assign(&mut root);
        self.documents.push(root);
        self.page_counts.push(page_count);
    }

    /// `addEmptyWebPage`: an excluded document (cover, TOC placeholder).
    pub fn add_empty_web_page(&mut self, out_first_page: usize) {
        let mut root = OutlineItem { out_page: out_first_page, ..Default::default() };
        self.assign(&mut root);
        self.documents.push(root);
        self.page_counts.push(1);
    }

    /// Bookmarks for the PDF (`printOutline`): the children of each document
    /// root, down to `depth` levels.
    pub fn pdf_outline(&self, depth: i32) -> Vec<wk_print::OutlineNode> {
        fn conv(i: &OutlineItem, level: i32, depth: i32) -> Option<wk_print::OutlineNode> {
            if level > depth {
                return None;
            }
            Some(wk_print::OutlineNode {
                title: i.value.clone(),
                page: i.out_page,
                x: i.x,
                y: i.y,
                children: i.children.iter().filter_map(|c| conv(c, level + 1, depth)).collect(),
            })
        }
        self.documents.iter().flat_map(|d| d.children.iter().filter_map(|c| conv(c, 1, depth))).collect()
    }

    /// `Outline::dump` (`--dump-outline`).
    pub fn dump(&self, page_offset: i32) -> String {
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        }
        fn item(out: &mut String, i: &OutlineItem, level: usize, prefix: usize, offset: i32) {
            let indent = "  ".repeat(level);
            let page = i.page as i64 + prefix as i64 + i64::from(offset);
            let _ = write!(
                out,
                "{indent}<item title=\"{}\" page=\"{page}\" link=\"{}\" backLink=\"{}\"",
                esc(&i.value),
                i.anchor,
                i.toc_anchor
            );
            if i.children.is_empty() {
                out.push_str("/>\n");
            } else {
                out.push_str(">\n");
                for c in &i.children {
                    item(out, c, level + 1, prefix, offset);
                }
                let _ = writeln!(out, "{indent}</item>");
            }
        }
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<outline xmlns=\"http://wkhtmltopdf.org/outline\">\n",
        );
        let mut prefix = 0usize;
        for (d, count) in self.documents.iter().zip(&self.page_counts) {
            item(&mut out, d, 1, prefix, page_offset);
            prefix += count;
        }
        out.push_str("</outline>\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(level: u32, text: &str, page: usize) -> LocatedHeading {
        LocatedHeading { level, text: text.into(), page, out_page: page - 1, x: 0.0, y: 0.0 }
    }

    #[test]
    fn tree_and_anchors_like_upstream() {
        let mut o = Outline::default();
        o.add_web_page(
            "Doc A",
            1,
            0,
            &[h(1, "Chapter One", 1), h(2, "Sub 1.1", 1), h(1, "Chapter Two & \"Q\"", 1), h(3, "Deep", 1)],
        );
        let xml = o.dump(0);
        assert!(xml.contains(
            "  <item title=\"Doc A\" page=\"0\" link=\"__WKANCHOR_0\" backLink=\"__WKANCHOR_1\">\n    <item title=\"Chapter One\" page=\"1\" link=\"__WKANCHOR_2\" backLink=\"__WKANCHOR_3\">\n      <item title=\"Sub 1.1\" page=\"1\" link=\"__WKANCHOR_4\" backLink=\"__WKANCHOR_5\"/>\n"
        ));
        assert!(xml.contains("title=\"Chapter Two &amp; &quot;Q&quot;\""));
        // An h3 directly under an h1 is its child.
        assert_eq!(o.documents[0].children[1].children[0].value, "Deep");
        assert_eq!(o.pdf_outline(1).len(), 2);
        assert_eq!(o.pdf_outline(1)[0].children.len(), 0);
    }
}
