//! Script-pausing HTML parser. Stands in for WebCore's `HTMLDocumentParser`
//! with scripting: the html5ever tokenizer stops at every `</script>`, the
//! caller runs the script (fetching it first when external), and markup it
//! wrote with `document.write` is pushed back in front of the remaining
//! input. So a script sees exactly the elements parsed before it, like in a
//! browser.
//!
//! The tree sink is blitz-html's (`DocumentHtmlParser`) adapted to reach the
//! document through [`wk_js::with_document`] for each operation, because
//! the document lives in the JavaScript session while scripts run between
//! two parser steps. It also records the line of `<script>` and `<body>`
//! start tags (console messages are attributed to document lines).

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;

use blitz_dom::node::Attribute;
use blitz_dom::{NodeId, QualName, local_name};
use html5ever::TokenizerResult;
use html5ever::buffer_queue::BufferQueue;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{Tokenizer, TokenizerOpts};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink};
use wk_js::with_document;

fn to_blitz_attr(attr: html5ever::Attribute) -> Attribute {
    Attribute { name: attr.name, value: attr.value.to_string() }
}

/// html5ever tree sink over the session's document.
pub(crate) struct PausingSink {
    /// Element names by node (html5ever borrows them).
    names: RefCell<HashMap<NodeId, QualName>>,
    fallback_name: QualName,
    line: Cell<u64>,
    /// Lines of `<script>` and `<body>` start tags.
    pub lines: RefCell<Vec<(NodeId, u32)>>,
    pub quirks: Cell<bool>,
    pub body_seen: Cell<bool>,
}

impl PausingSink {
    pub(crate) fn new() -> PausingSink {
        PausingSink {
            names: RefCell::new(HashMap::new()),
            fallback_name: QualName::new(None, blitz_dom::ns!(html), local_name!("div")),
            line: Cell::new(1),
            lines: RefCell::new(Vec::new()),
            quirks: Cell::new(false),
            body_seen: Cell::new(false),
        }
    }
}

/// Owned element names handed to html5ever.
#[derive(Debug)]
pub(crate) enum ElemNameRef<'a> {
    Map(Ref<'a, QualName>),
    Fallback(&'a QualName),
}

impl html5ever::interface::ElemName for ElemNameRef<'_> {
    fn ns(&self) -> &html5ever::Namespace {
        match self {
            ElemNameRef::Map(r) => &r.ns,
            ElemNameRef::Fallback(q) => &q.ns,
        }
    }
    fn local_name(&self) -> &html5ever::LocalName {
        match self {
            ElemNameRef::Map(r) => &r.local,
            ElemNameRef::Fallback(q) => &q.local,
        }
    }
}

impl TreeSink for PausingSink {
    type Output = ();
    type Handle = NodeId;
    type ElemName<'a>
        = ElemNameRef<'a>
    where
        Self: 'a;

    fn finish(self) -> Self::Output {}

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Self::Handle {
        with_document(|d| d.root_node().id)
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        if !self.names.borrow().contains_key(target) {
            let name =
                with_document(|d| d.get_node(*target).and_then(|n| n.element_data()).map(|e| e.name.clone()));
            match name {
                Some(n) => {
                    self.names.borrow_mut().insert(*target, n);
                }
                None => return ElemNameRef::Fallback(&self.fallback_name),
            }
        }
        ElemNameRef::Map(Ref::map(self.names.borrow(), |m| &m[target]))
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<html5ever::Attribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let attrs: Vec<Attribute> = attrs.into_iter().map(to_blitz_attr).collect();
        let local = name.local.clone();
        let id = with_document(|d| d.mutate().create_element(name.clone(), attrs));
        self.names.borrow_mut().insert(id, name);
        if local == local_name!("script") || local == local_name!("body") {
            self.lines.borrow_mut().push((id, self.line.get() as u32));
            if local == local_name!("body") {
                self.body_seen.set(true);
            }
        }
        id
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        with_document(|d| d.mutate().create_comment_node(&text))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Self::Handle {
        with_document(|d| d.mutate().create_comment_node(""))
    }

    fn append(&self, parent_id: &Self::Handle, child: NodeOrText<Self::Handle>) {
        with_document(|d| {
            let mut m = d.mutate();
            match child {
                NodeOrText::AppendNode(id) => m.append_children(*parent_id, &[id]),
                NodeOrText::AppendText(text) => {
                    let appended = m
                        .last_child_id(*parent_id)
                        .is_some_and(|id| m.append_text_to_node(id, &text).is_ok());
                    if !appended {
                        let t = m.create_text_node(&text);
                        m.append_children(*parent_id, &[t]);
                    }
                }
            }
        });
    }

    fn append_before_sibling(&self, sibling_id: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        with_document(|d| {
            let mut m = d.mutate();
            match new_node {
                NodeOrText::AppendNode(id) => m.insert_nodes_before(*sibling_id, &[id]),
                NodeOrText::AppendText(text) => {
                    let appended = m
                        .previous_sibling_id(*sibling_id)
                        .is_some_and(|id| m.append_text_to_node(id, &text).is_ok());
                    if !appended {
                        let t = m.create_text_node(&text);
                        m.insert_nodes_before(*sibling_id, &[t]);
                    }
                }
            }
        });
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        if with_document(|d| d.mutate().node_has_parent(*element)) {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _name: StrTendril, _public_id: StrTendril, _system_id: StrTendril) {}

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        with_document(|d| d.mutate().template_contents(*target))
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks.set(mode == QuirksMode::Quirks);
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<html5ever::Attribute>) {
        let attrs = attrs.into_iter().map(to_blitz_attr).collect();
        with_document(|d| d.mutate().add_attrs_if_missing(*target, attrs));
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        with_document(|d| d.mutate().remove_node(*target));
    }

    fn reparent_children(&self, old_parent_id: &Self::Handle, new_parent_id: &Self::Handle) {
        with_document(|d| d.mutate().reparent_children(*old_parent_id, *new_parent_id));
    }

    fn set_current_line(&self, line: u64) {
        self.line.set(line);
    }
}

/// What the parser stopped for.
pub(crate) enum Step {
    /// A `</script>` was reached.
    Script(NodeId),
    /// The whole input was parsed.
    Done,
}

/// An incremental parse of one document.
pub(crate) struct Parser {
    tok: Tokenizer<TreeBuilder<NodeId, PausingSink>>,
    input: BufferQueue,
}

impl Parser {
    pub(crate) fn new(html: &str) -> Parser {
        let tb = TreeBuilder::new(
            PausingSink::new(),
            TreeBuilderOpts {
                exact_errors: false,
                // Scripts run: <noscript> content is raw text, so nothing
                // in it is requested or run (and the user-agent style sheet
                // hides the element).
                scripting_enabled: true,
                iframe_srcdoc: false,
                drop_doctype: true,
                quirks_mode: QuirksMode::NoQuirks,
            },
        );
        let tok = Tokenizer::new(tb, TokenizerOpts::default());
        let input = BufferQueue::default();
        input.push_back(StrTendril::from_slice(html));
        Parser { tok, input }
    }

    pub(crate) fn sink(&self) -> &PausingSink {
        &self.tok.sink.sink
    }

    /// Parse until the next `</script>` or the end of the input.
    pub(crate) fn step(&mut self) -> Step {
        loop {
            match self.tok.feed(&self.input) {
                TokenizerResult::Script(node) => return Step::Script(node),
                TokenizerResult::EncodingIndicator(_) => continue,
                TokenizerResult::Done => {
                    if self.input.is_empty() {
                        self.tok.end();
                        return Step::Done;
                    }
                }
            }
        }
    }

    /// Insert markup written by `document.write` at the insertion point.
    pub(crate) fn write(&mut self, html: &str) {
        if !html.is_empty() {
            self.input.push_front(StrTendril::from_slice(html));
        }
    }
}
