//! Document outline. Ports upstream `src/lib/outline.cc` (`Outline`,
//! `OutlinePrivate`, `OutlineItem`): the heading tree of every document,
//! the `__WKANCHOR_` anchors (with the reuse bug of TOC rebuilds), the PDF
//! bookmarks (`printOutline`), `--dump-outline`, the named anchors of each
//! document (`fillAnchors`) and the `[section]`-style header/footer values
//! (`fillHeaderFooterParms`, spec §3.4–§3.5).

use std::fmt::Write as _;

/// One heading (or document root).
#[derive(Debug, Clone, Default)]
pub struct OutlineItem {
    pub value: String,
    /// 1-based page within its document for headings, 0 for roots.
    pub page: usize,
    /// Index of the document (`OutlineItem::document`).
    pub document: usize,
    pub anchor: String,
    pub toc_anchor: String,
    pub forward_links: bool,
    pub back_links: bool,
    /// Location of the heading element on its page (pt, top-left origin).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub children: Vec<OutlineItem>,
}

/// A located heading, input of [`Outline::replace_web_page`].
#[derive(Debug, Clone)]
pub struct LocatedHeading {
    pub level: u32,
    pub text: String,
    /// 1-based page within the document.
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A back link of `--enable-toc-back-links`: the heading's box links to
/// `target` (its TOC entry).
#[derive(Debug, Clone)]
pub struct BackLink {
    pub page: usize,
    pub rect: (f32, f32, f32, f32),
    pub target: String,
}

#[derive(Debug, Default)]
pub struct Outline {
    pub documents: Vec<OutlineItem>,
    /// Page count of each document as used for numbering (excluded
    /// documents count as one page).
    pub document_pages: Vec<usize>,
    /// Sum of `document_pages` (`[topage]`).
    pub page_count: usize,
    anchor_counter: u64,
    /// `hfCache`: per level (section, subsection, subsubsection), the value
    /// for each page number.
    hf_cache: Option<[Vec<Option<String>>; 3]>,
}

/// `QString::number(n, 36)`.
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

impl OutlineItem {
    /// `OutlineItem::fillAnchors`: reuse the anchors of `other` when it has
    /// the same shape (and then, like upstream, `tocAnchor = other->anchor`).
    fn fill_anchors(&mut self, other: Option<&OutlineItem>, counter: &mut u64, back: &mut Vec<BackLink>) {
        let other = other.filter(|o| {
            o.children.len() == self.children.len() && o.document == self.document && o.value == self.value
        });
        if let Some(o) = other {
            self.anchor = o.anchor.clone();
            self.toc_anchor = o.anchor.clone();
        } else {
            self.anchor = format!("__WKANCHOR_{}", base36(*counter));
            *counter += 1;
            self.toc_anchor = format!("__WKANCHOR_{}", base36(*counter));
            *counter += 1;
        }
        if self.back_links {
            back.push(BackLink {
                page: self.page,
                rect: (self.x, self.y, self.x + self.w, self.y + self.h),
                target: self.toc_anchor.clone(),
            });
        }
        for (i, c) in self.children.iter_mut().enumerate() {
            c.fill_anchors(other.and_then(|o| o.children.get(i)), counter, back);
        }
    }

    /// `OutlineItem::differentFrom`.
    fn different_from(&self, other: &OutlineItem) -> bool {
        other.children.len() != self.children.len()
            || other.page != self.page
            || other.document != self.document
            || other.value != self.value
            || self.children.iter().zip(&other.children).any(|(a, b)| a.different_from(b))
    }
}

impl Outline {
    /// `addEmptyWebPage`: an excluded document (cover, `--exclude-from-outline`,
    /// TOC before it is laid out). Its root has no anchors.
    pub fn add_empty_web_page(&mut self) {
        let document = self.document_pages.len();
        self.documents.push(OutlineItem { document, ..Default::default() });
        self.page_count += 1;
        self.document_pages.push(1);
    }

    /// `addWebPage`. Returns the back links.
    pub fn add_web_page(
        &mut self,
        name: &str,
        page_count: usize,
        headings: &[LocatedHeading],
        forward_links: bool,
        back_links: bool,
    ) -> Vec<BackLink> {
        self.add_empty_web_page();
        let d = self.documents.len() - 1;
        self.replace_web_page(d, name, page_count, headings, forward_links, back_links).1
    }

    /// `replaceWebPage`: rebuild the tree of `document` from its headings,
    /// which must be in (page, y, x) order. Returns whether the outline
    /// changed (structure or page count) and the back links.
    pub fn replace_web_page(
        &mut self,
        document: usize,
        name: &str,
        page_count: usize,
        headings: &[LocatedHeading],
        forward_links: bool,
        back_links: bool,
    ) -> (bool, Vec<BackLink>) {
        let mut root = OutlineItem { value: name.to_string(), document, ..Default::default() };
        // Level stack of paths into the tree (`levelStack`).
        let mut stack: Vec<(u32, Vec<usize>)> = Vec::new();
        for h in headings {
            if h.text.is_empty() {
                continue;
            }
            while stack.last().is_some_and(|(l, _)| *l >= h.level) {
                stack.pop();
            }
            let item = OutlineItem {
                value: h.text.clone(),
                page: h.page,
                document,
                forward_links,
                back_links,
                x: h.x,
                y: h.y,
                w: h.w,
                h: h.h,
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
        let mut back = Vec::new();
        let old = std::mem::take(&mut self.documents[document]);
        root.fill_anchors(Some(&old), &mut self.anchor_counter, &mut back);
        let mut changed = old.different_from(&root);
        self.documents[document] = root;
        if self.document_pages[document] != page_count {
            self.page_count -= self.document_pages[document];
            self.document_pages[document] = page_count;
            self.page_count += page_count;
            changed = true;
        }
        (changed, back)
    }

    /// `fillAnchors` / `fillChildAnchors`: the named anchors every heading of
    /// `document` defines, as (name, page, x, y).
    pub fn child_anchors(&self, document: usize) -> Vec<(String, usize, f32, f32)> {
        fn walk(i: &OutlineItem, out: &mut Vec<(String, usize, f32, f32)>) {
            for c in &i.children {
                if c.anchor.is_empty() {
                    continue;
                }
                out.push((c.anchor.clone(), c.page, c.x, c.y));
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        if let Some(d) = self.documents.get(document) {
            walk(d, &mut out);
        }
        out
    }

    /// Bookmarks for the PDF (`printOutline`): the children of each document
    /// root, down to `depth` levels. `first_page(document)` is the output
    /// page index of the document's first page.
    pub fn pdf_outline(
        &self,
        depth: i32,
        first_page: &dyn Fn(usize) -> Option<usize>,
    ) -> Vec<wk_print::OutlineNode> {
        fn conv(i: &OutlineItem, level: i32, depth: i32, first: usize) -> Option<wk_print::OutlineNode> {
            if level > depth {
                return None;
            }
            Some(wk_print::OutlineNode {
                title: i.value.clone(),
                anchor: i.anchor.clone(),
                page: first + i.page.saturating_sub(1),
                x: i.x,
                y: i.y,
                children: i.children.iter().filter_map(|c| conv(c, level + 1, depth, first)).collect(),
            })
        }
        self.documents
            .iter()
            .enumerate()
            .filter_map(|(d, root)| first_page(d).map(|f| (root, f)))
            .flat_map(|(root, f)| root.children.iter().filter_map(move |c| conv(c, 1, depth, f)))
            .collect()
    }

    fn prefix_sum(&self) -> Vec<usize> {
        let mut p = vec![0];
        for x in &self.document_pages {
            p.push(p.last().expect("prefix") + x);
        }
        p
    }

    /// `Outline::dump` (`--dump-outline` and the TOC input).
    pub fn dump(&self, page_offset: i32) -> String {
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        }
        fn item(out: &mut String, i: &OutlineItem, level: usize, prefix: &[usize], offset: i32) {
            let indent = "  ".repeat(level);
            let page = i.page as i64 + prefix[i.document] as i64 + i64::from(offset);
            let _ = write!(
                out,
                "{indent}<item title=\"{}\" page=\"{page}\" link=\"{}\" backLink=\"{}\"",
                esc(&i.value),
                esc(&i.anchor),
                esc(&i.toc_anchor)
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
        let prefix = self.prefix_sum();
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<outline xmlns=\"http://wkhtmltopdf.org/outline\">\n",
        );
        for d in &self.documents {
            item(&mut out, d, 1, &prefix, page_offset);
        }
        out.push_str("</outline>\n");
        out
    }

    /// `buildHFCache`, run once on the first header/footer page.
    fn build_hf_cache(&mut self) {
        let prefix = self.prefix_sum();
        let mut cache: [Vec<Option<String>>; 3] = [vec![None], vec![None], vec![None]];
        fn walk(i: &OutlineItem, level: usize, prefix: &[usize], cache: &mut [Vec<Option<String>>; 3]) {
            if level >= 3 {
                return;
            }
            for j in &i.children {
                let page = j.page + prefix[j.document];
                let c = &mut cache[level];
                while c.len() < page {
                    let last = c.last().cloned().flatten();
                    c.push(last);
                }
                if c.len() == page {
                    c.push(Some(j.value.clone()));
                }
                walk(j, level + 1, prefix, cache);
            }
        }
        for d in &self.documents {
            walk(d, 0, &prefix, &mut cache);
        }
        self.hf_cache = Some(cache);
    }

    /// `[section]`, `[subsection]` and `[subsubsection]` of page number
    /// `page` (1-based, without the page offset).
    pub fn sections(&mut self, page: usize) -> [String; 3] {
        if self.hf_cache.is_none() {
            self.build_hf_cache();
        }
        let cache = self.hf_cache.as_mut().expect("hf cache");
        let mut out: [String; 3] = Default::default();
        for (l, c) in cache.iter_mut().enumerate() {
            while c.len() <= page {
                let last = c.last().cloned().flatten();
                c.push(last);
            }
            out[l] = c[page].clone().unwrap_or_default();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(level: u32, text: &str, page: usize) -> LocatedHeading {
        LocatedHeading { level, text: text.into(), page, x: 0.0, y: 0.0, w: 0.0, h: 0.0 }
    }

    #[test]
    fn tree_and_anchors_like_upstream() {
        let mut o = Outline::default();
        o.add_web_page(
            "Doc A",
            1,
            &[h(1, "Chapter One", 1), h(2, "Sub 1.1", 1), h(1, "Chapter Two & \"Q\"", 1), h(3, "Deep", 1)],
            true,
            false,
        );
        let xml = o.dump(0);
        assert!(xml.contains(
            "  <item title=\"Doc A\" page=\"0\" link=\"__WKANCHOR_0\" backLink=\"__WKANCHOR_1\">\n    <item title=\"Chapter One\" page=\"1\" link=\"__WKANCHOR_2\" backLink=\"__WKANCHOR_3\">\n      <item title=\"Sub 1.1\" page=\"1\" link=\"__WKANCHOR_4\" backLink=\"__WKANCHOR_5\"/>\n"
        ));
        assert!(xml.contains("title=\"Chapter Two &amp; &quot;Q&quot;\""));
        // An h3 directly under an h1 is its child.
        assert_eq!(o.documents[0].children[1].children[0].value, "Deep");
        let first = |_| Some(0);
        assert_eq!(o.pdf_outline(1, &first).len(), 2);
        assert_eq!(o.pdf_outline(1, &first)[0].children.len(), 0);
    }

    /// Spec §3.5: a TOC rebuilt with the same shape keeps its anchors, and
    /// `backLink` then equals `link`.
    #[test]
    fn toc_rebuild_reuses_anchor_as_back_link() {
        let mut o = Outline::default();
        o.add_empty_web_page();
        o.add_web_page("Doc", 1, &[h(1, "A", 1)], true, false);
        let (changed, _) =
            o.replace_web_page(0, "Table of Contents", 1, &[h(1, "Table of Contents", 1)], true, false);
        assert!(changed);
        assert_eq!(o.documents[0].children[0].anchor, "__WKANCHOR_6");
        assert_eq!(o.documents[0].children[0].toc_anchor, "__WKANCHOR_7");
        let (changed, _) =
            o.replace_web_page(0, "Table of Contents", 1, &[h(1, "Table of Contents", 1)], true, false);
        assert!(!changed);
        assert_eq!(o.documents[0].toc_anchor, "__WKANCHOR_4");
        assert_eq!(o.documents[0].children[0].toc_anchor, "__WKANCHOR_6");
        // An excluded document has no anchors.
        let mut e = Outline::default();
        e.add_empty_web_page();
        assert!(e.dump(0).contains("<item title=\"\" page=\"0\" link=\"\" backLink=\"\"/>"));
    }

    /// Spec §3.4: the first heading of a page wins, values carry forward,
    /// and the subsection is never reset by a new section.
    #[test]
    fn section_values_carry_forward_and_leak() {
        let mut o = Outline::default();
        o.add_web_page("D", 3, &[h(1, "One", 1), h(2, "One.1", 1), h(1, "Two", 3)], true, false);
        o.add_web_page("E", 1, &[h(1, "Three", 1)], true, false);
        assert_eq!(o.sections(1), ["One".to_string(), "One.1".into(), String::new()]);
        assert_eq!(o.sections(2)[0], "One");
        assert_eq!(o.sections(3), ["Two".to_string(), "One.1".into(), String::new()]);
        assert_eq!(o.sections(4), ["Three".to_string(), "One.1".into(), String::new()]);
        assert_eq!(o.page_count, 4);
    }
}
