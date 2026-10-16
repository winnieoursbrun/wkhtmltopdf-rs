//! wkhtmltopdf-rs: CSS generated content that depends on document order — `counter()`,
//! `counters()` and the quotes (`open-quote`, `close-quote`, `no-open-quote`,
//! `no-close-quote`) — resolved by one walk of the document in order, as WebKit's
//! `RenderCounter` and `RenderQuote` do. The text of each `::before`/`::after` is stored in
//! [`BaseDocument::generated_content`] and read by box construction (`pe_content_text`).

use std::collections::HashMap;

use style::properties::ComputedValues;
use style::values::computed::{Content, ContentItem};

use crate::{BaseDocument, node::NodeData};
use blitz_traits::node_id::NodeId;

/// The live counters: for each name, its instances from outermost to innermost, each with the
/// node whose children it is scoped to and its value.
#[derive(Default)]
struct Counters {
    instances: HashMap<String, Vec<(NodeId, i32)>>,
    quote_depth: i32,
}

impl Counters {
    /// `counter-reset` then `counter-increment` of an element (or pseudo-element) whose parent
    /// is `scope`.
    fn apply(&mut self, style: &ComputedValues, scope: NodeId) {
        let counters = style.get_counters();
        for pair in counters.counter_reset.iter() {
            let list = self.instances.entry(pair.name.0.to_string()).or_default();
            // A reset by a sibling replaces the counter its earlier sibling created.
            if list.last().is_some_and(|(owner, _)| *owner == scope) {
                list.pop();
            }
            list.push((scope, pair.value));
        }
        for pair in counters.counter_increment.iter() {
            let list = self.instances.entry(pair.name.0.to_string()).or_default();
            match list.last_mut() {
                Some((_, value)) => *value = value.saturating_add(pair.value),
                None => list.push((scope, pair.value)),
            }
        }
    }

    /// Drop the counters scoped to the children of `scope`.
    fn leave(&mut self, scope: NodeId) {
        for list in self.instances.values_mut() {
            while list.last().is_some_and(|(owner, _)| *owner == scope) {
                list.pop();
            }
        }
    }

    /// The text of a `content` value; `attr()` reads the attributes of `element`.
    fn text(&mut self, style: &ComputedValues, element: &crate::Node) -> Option<String> {
        let Content::Items(items) = &style.get_counters().content else { return None };
        let quotes = &style.get_list().quotes;
        let mut text = String::new();
        for item in &items.items[0..items.alt_start] {
            match item {
                ContentItem::String(s) => text.push_str(s),
                ContentItem::Counter(name, counter_style) => {
                    let value = self.value(&name.0);
                    text.push_str(&crate::layout::list::counter_text(counter_style, value));
                }
                ContentItem::Counters(name, separator, counter_style) => {
                    let values: Vec<i32> = self
                        .instances
                        .get(&*name.0)
                        .map(|l| l.iter().map(|(_, v)| *v).collect())
                        .unwrap_or_default();
                    let values = if values.is_empty() { vec![0] } else { values };
                    let parts: Vec<String> = values
                        .iter()
                        .map(|v| crate::layout::list::counter_text(counter_style, *v))
                        .collect();
                    text.push_str(&parts.join(separator));
                }
                ContentItem::OpenQuote => {
                    text.push_str(quote(quotes, self.quote_depth, true));
                    self.quote_depth += 1;
                }
                ContentItem::CloseQuote => {
                    self.quote_depth = (self.quote_depth - 1).max(0);
                    text.push_str(quote(quotes, self.quote_depth, false));
                }
                ContentItem::NoOpenQuote => self.quote_depth += 1,
                ContentItem::NoCloseQuote => self.quote_depth = (self.quote_depth - 1).max(0),
                // A missing attribute generates the empty string.
                ContentItem::Attr(name) => {
                    if let NodeData::Element(e) = &element.data {
                        if let Some(a) = e.attrs().iter().find(|a| *a.name.local == **name) {
                            text.push_str(&a.value);
                        }
                    }
                }
                _ => {}
            }
        }
        Some(text)
    }

    fn value(&self, name: &str) -> i32 {
        self.instances.get(name).and_then(|l| l.last()).map_or(0, |(_, v)| *v)
    }
}

/// The quote mark at nesting `depth`. QtWebKit 2.2's default quotes are `"` then `'`.
fn quote(quotes: &style::values::computed::Quotes, depth: i32, open: bool) -> &str {
    use style::values::computed::Quotes;
    let depth = depth.max(0) as usize;
    match quotes {
        Quotes::QuoteList(list) => {
            let pairs = &list.0;
            match pairs.get(depth.min(pairs.len().saturating_sub(1))) {
                Some(pair) if open => &pair.opening,
                Some(pair) => &pair.closing,
                None => "",
            }
        }
        Quotes::Auto => {
            if depth == 0 {
                "\""
            } else {
                "'"
            }
        }
    }
}

fn pseudo_styles(doc: &BaseDocument, id: NodeId) -> [Option<style::servo_arc::Arc<ComputedValues>>; 2] {
    let data = doc.nodes[id].try_stylo_element_data().and_then(|s| s.get());
    // Index 1 is `::before`, index 0 `::after` (see `flush_pseudo_elements`).
    let get = |i: usize| data.as_ref().and_then(|d| d.styles.pseudos.as_array()[i].clone());
    [get(0), get(1)]
}

impl BaseDocument {
    /// Resolve the generated content of every `::before` and `::after` in document order.
    pub(crate) fn resolve_generated_content(&mut self) {
        let mut out = HashMap::new();
        let mut counters = Counters::default();
        let root = self.root_node().id;
        visit(self, root, &mut counters, &mut out);
        self.generated_content = out;

        fn visit(
            doc: &BaseDocument,
            id: NodeId,
            counters: &mut Counters,
            out: &mut HashMap<(NodeId, usize), String>,
        ) {
            let children = doc.nodes[id].children.clone();
            for child in children {
                let node = &doc.nodes[child];
                if !matches!(node.data, NodeData::Element(_)) {
                    continue;
                }
                let Some(style) = node.primary_styles().map(|s| (*s).clone()) else { continue };
                if style.get_box().display.is_none() {
                    continue;
                }
                counters.apply(&style, id);
                let [after, before] = pseudo_styles(doc, child);
                if let Some(before) = before {
                    counters.apply(&before, child);
                    if let Some(text) = counters.text(&before, &doc.nodes[child]) {
                        out.insert((child, 1), text);
                    }
                }
                visit(doc, child, counters, out);
                if let Some(after) = after {
                    counters.apply(&after, child);
                    if let Some(text) = counters.text(&after, &doc.nodes[child]) {
                        out.insert((child, 0), text);
                    }
                }
                counters.leave(child);
            }
        }
    }
}
