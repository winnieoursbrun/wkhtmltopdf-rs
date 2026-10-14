//! Native DOM bindings: Boa objects wrapping blitz-dom `NodeId`s. Stands in
//! for WebCore's JSC bindings (`JSNode`, `JSElement`, `JSDocument`, …).
//!
//! Wrapper model: every JS node object is an ordinary Boa object whose native
//! data is [`NodeW`], created with the prototype of its interface
//! (`EventTarget → Node → Element → HTMLElement → HTMLCanvasElement`, …). A
//! `NodeId → JsObject` cache keeps identity stable (`a.firstChild ===
//! a.firstChild`, jQuery expandos). The cache holds strong handles for the
//! (one-shot) conversion, and JS-visible removals only *detach* nodes, so an
//! id held by a wrapper is never recycled by blitz's slab.
//!
//! What is cheaper in JavaScript (events, `style`, `classList`, reflected
//! attributes, form state, `location`, …) lives in `prelude.js`.

use std::collections::HashMap;

use blitz_dom::{LocalName, NodeData, NodeId, QualName, local_name, ns};
use boa_engine::object::FunctionObjectBuilder;
use boa_engine::object::builtins::JsArray;
use boa_engine::property::PropertyDescriptor;
use boa_engine::{
    Context, Finalize, JsArgs, JsData, JsNativeError, JsObject, JsResult, JsString, JsValue, NativeFunction, Trace,
};

use crate::host::{Host, with_host};
use crate::serialize::{serialize_children, serialize_node};

pub(crate) type Fp = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

#[derive(Debug, Trace, Finalize, JsData)]
pub(crate) struct NodeW {
    #[unsafe_ignore_trace]
    pub id: NodeId,
}

/// Interface prototypes by name.
pub(crate) struct Protos {
    pub by_name: HashMap<&'static str, JsObject>,
}

impl Protos {
    fn get(&self, name: &str) -> &JsObject {
        self.by_name.get(name).unwrap_or_else(|| &self.by_name["HTMLElement"])
    }
}

/// `(interface, parent)` in creation order.
const INTERFACES: &[(&str, Option<&str>)] = &[
    ("EventTarget", None),
    ("Node", Some("EventTarget")),
    ("CharacterData", Some("Node")),
    ("Text", Some("CharacterData")),
    ("Comment", Some("CharacterData")),
    ("Element", Some("Node")),
    ("HTMLElement", Some("Element")),
    ("SVGElement", Some("Element")),
    ("Document", Some("Node")),
    ("HTMLDocument", Some("Document")),
    ("DocumentFragment", Some("Node")),
    ("HTMLAnchorElement", Some("HTMLElement")),
    ("HTMLBodyElement", Some("HTMLElement")),
    ("HTMLBRElement", Some("HTMLElement")),
    ("HTMLButtonElement", Some("HTMLElement")),
    ("HTMLCanvasElement", Some("HTMLElement")),
    ("HTMLDivElement", Some("HTMLElement")),
    ("HTMLFormElement", Some("HTMLElement")),
    ("HTMLHeadElement", Some("HTMLElement")),
    ("HTMLHeadingElement", Some("HTMLElement")),
    ("HTMLHtmlElement", Some("HTMLElement")),
    ("HTMLIFrameElement", Some("HTMLElement")),
    ("HTMLImageElement", Some("HTMLElement")),
    ("HTMLInputElement", Some("HTMLElement")),
    ("HTMLLIElement", Some("HTMLElement")),
    ("HTMLLinkElement", Some("HTMLElement")),
    ("HTMLMetaElement", Some("HTMLElement")),
    ("HTMLOptionElement", Some("HTMLElement")),
    ("HTMLParagraphElement", Some("HTMLElement")),
    ("HTMLPreElement", Some("HTMLElement")),
    ("HTMLScriptElement", Some("HTMLElement")),
    ("HTMLSelectElement", Some("HTMLElement")),
    ("HTMLSpanElement", Some("HTMLElement")),
    ("HTMLStyleElement", Some("HTMLElement")),
    ("HTMLTableElement", Some("HTMLElement")),
    ("HTMLTableCellElement", Some("HTMLElement")),
    ("HTMLTableRowElement", Some("HTMLElement")),
    ("HTMLTableSectionElement", Some("HTMLElement")),
    ("HTMLTextAreaElement", Some("HTMLElement")),
    ("HTMLTitleElement", Some("HTMLElement")),
    ("HTMLUListElement", Some("HTMLElement")),
    ("HTMLOListElement", Some("HTMLElement")),
    ("HTMLUnknownElement", Some("HTMLElement")),
];

fn interface_of_tag(tag: &str) -> &'static str {
    match tag {
        "a" => "HTMLAnchorElement",
        "body" => "HTMLBodyElement",
        "br" => "HTMLBRElement",
        "button" => "HTMLButtonElement",
        "canvas" => "HTMLCanvasElement",
        "div" => "HTMLDivElement",
        "form" => "HTMLFormElement",
        "head" => "HTMLHeadElement",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "HTMLHeadingElement",
        "html" => "HTMLHtmlElement",
        "iframe" => "HTMLIFrameElement",
        "img" => "HTMLImageElement",
        "input" => "HTMLInputElement",
        "li" => "HTMLLIElement",
        "link" => "HTMLLinkElement",
        "meta" => "HTMLMetaElement",
        "option" => "HTMLOptionElement",
        "p" => "HTMLParagraphElement",
        "pre" => "HTMLPreElement",
        "script" => "HTMLScriptElement",
        "select" => "HTMLSelectElement",
        "span" => "HTMLSpanElement",
        "style" => "HTMLStyleElement",
        "table" => "HTMLTableElement",
        "td" | "th" => "HTMLTableCellElement",
        "tr" => "HTMLTableRowElement",
        "thead" | "tbody" | "tfoot" => "HTMLTableSectionElement",
        "textarea" => "HTMLTextAreaElement",
        "title" => "HTMLTitleElement",
        "ul" => "HTMLUListElement",
        "ol" => "HTMLOListElement",
        _ => "HTMLElement",
    }
}

// ---------- helpers ----------

pub(crate) fn s(v: &str) -> JsValue {
    JsValue::from(JsString::from(v))
}

pub(crate) fn dom_err(name: &str, msg: &str) -> boa_engine::JsError {
    JsNativeError::typ().with_message(format!("{name}: {msg}")).into()
}

pub(crate) fn id_of(v: &JsValue) -> JsResult<NodeId> {
    v.as_object()
        .and_then(|o| o.downcast_ref::<NodeW>().map(|n| n.id))
        .ok_or_else(|| JsNativeError::typ().with_message("value is not a Node").into())
}

pub(crate) fn opt_id(v: &JsValue) -> Option<NodeId> {
    v.as_object().and_then(|o| o.downcast_ref::<NodeW>().map(|n| n.id))
}

pub(crate) fn arg_str(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(args.get_or_undefined(i).to_string(ctx)?.to_std_string_escaped())
}

fn attr_qn(name: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(name))
}

pub(crate) fn root(h: &Host) -> NodeId {
    h.doc.root_node().id
}

/// The wrapper of a node (created on first use).
pub(crate) fn wrap(id: NodeId) -> JsValue {
    with_host(|h| wrap_in(h, id))
}

pub(crate) fn wrap_in(h: &mut Host, id: NodeId) -> JsValue {
    if let Some(o) = h.cache.get(&id) {
        return o.clone().into();
    }
    let Some(node) = h.doc.get_node(id) else { return JsValue::null() };
    let iface: &str = if h.fragments.contains(&id) {
        "DocumentFragment"
    } else {
        match &node.data {
            NodeData::Document(_) => "HTMLDocument",
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => {
                if e.name.ns == ns!(svg) {
                    "SVGElement"
                } else {
                    interface_of_tag(&e.name.local)
                }
            }
            NodeData::Text(_) => "Text",
            NodeData::Comment { .. } => "Comment",
        }
    };
    let proto = h.protos.as_ref().expect("protos").get(iface).clone();
    let o = JsObject::from_proto_and_data(proto, NodeW { id });
    h.cache.insert(id, o.clone());
    o.into()
}

fn wrap_opt(id: Option<NodeId>) -> JsValue {
    id.map(wrap).unwrap_or(JsValue::null())
}

pub(crate) fn array_of(ids: Vec<NodeId>, ctx: &mut Context) -> JsValue {
    let vals: Vec<JsValue> = with_host(|h| ids.into_iter().map(|i| wrap_in(h, i)).collect());
    JsArray::from_iter(vals, ctx).into()
}

pub(crate) fn method(ctx: &mut Context, obj: &JsObject, name: &str, len: usize, f: Fp) {
    let func = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(f))
        .name(JsString::from(name))
        .length(len)
        .build();
    obj.define_property_or_throw(
        JsString::from(name),
        PropertyDescriptor::builder().value(func).writable(true).enumerable(false).configurable(true),
        ctx,
    )
    .expect("define method");
}

pub(crate) fn accessor(ctx: &mut Context, obj: &JsObject, name: &str, get: Fp, set: Option<Fp>) {
    let g = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(get))
        .name(JsString::from(format!("get {name}")))
        .build();
    let mut d = PropertyDescriptor::builder().get(g).enumerable(true).configurable(true);
    if let Some(set) = set {
        let sf = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(set))
            .name(JsString::from(format!("set {name}")))
            .length(1)
            .build();
        d = d.set(sf);
    }
    obj.define_property_or_throw(JsString::from(name), d, ctx).expect("define accessor");
}

// ---------- tree helpers (over the blitz arena) ----------

pub(crate) fn is_element(h: &Host, id: NodeId) -> bool {
    h.doc.get_node(id).is_some_and(|n| n.is_element()) && !h.fragments.contains(&id)
}

pub(crate) fn children(h: &Host, id: NodeId) -> Vec<NodeId> {
    h.doc.get_node(id).map(|n| n.children.to_vec()).unwrap_or_default()
}

pub(crate) fn parent(h: &Host, id: NodeId) -> Option<NodeId> {
    h.doc.get_node(id).and_then(|n| n.parent)
}

fn sibling(h: &Host, id: NodeId, delta: isize, elements_only: bool) -> Option<NodeId> {
    let p = parent(h, id)?;
    let node = h.doc.get_node(p)?;
    let kids = &node.children;
    let mut i = kids.iter().position(|&c| c == id)? as isize;
    loop {
        i += delta;
        if i < 0 || i as usize >= kids.len() {
            return None;
        }
        let c = kids[i as usize];
        if !elements_only || is_element(h, c) {
            return Some(c);
        }
    }
}

pub(crate) fn descendants(h: &Host, root: NodeId, out: &mut Vec<NodeId>) {
    if let Some(n) = h.doc.get_node(root) {
        for &c in n.children.iter() {
            out.push(c);
            descendants(h, c, out);
        }
    }
}

pub(crate) fn local(h: &Host, id: NodeId) -> Option<String> {
    if h.fragments.contains(&id) {
        return None;
    }
    h.doc.get_node(id)?.element_data().map(|e| e.name.local.to_string())
}

fn ancestors_incl(h: &Host, id: NodeId) -> Vec<NodeId> {
    let mut v = vec![id];
    let mut cur = id;
    while let Some(p) = parent(h, cur) {
        v.push(p);
        cur = p;
    }
    v
}

pub(crate) fn is_connected_in(h: &Host, id: NodeId) -> bool {
    *ancestors_incl(h, id).last().expect("non-empty") == root(h)
}

/// Detach all children (never drop: JS may hold wrappers to them).
pub(crate) fn detach_children(h: &mut Host, id: NodeId) {
    let kids = children(h, id);
    if kids.is_empty() {
        return;
    }
    let mut m = h.doc.mutate();
    for k in kids {
        m.remove_node(k);
    }
}

pub(crate) fn mark_dirty(h: &mut Host) {
    h.layout_dirty = true;
}

pub(crate) fn get_attr(h: &Host, id: NodeId, name: &str) -> Option<String> {
    let node = h.doc.get_node(id)?;
    node.element_data()?.attrs.iter().find(|a| &*a.name.local == name).map(|a| a.value.clone())
}

pub(crate) fn set_attr(h: &mut Host, id: NodeId, name: &str, value: &str) {
    if !h.doc.get_node(id).is_some_and(|n| n.is_element()) {
        return;
    }
    h.doc.mutate().set_attribute(id, attr_qn(name), value);
    mark_dirty(h);
}

// ---------- Node ----------

fn node_type(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        if h.fragments.contains(&id) {
            return 11;
        }
        match h.doc.get_node(id).map(|n| &n.data) {
            Some(NodeData::Document(_)) => 9,
            Some(NodeData::Element(_) | NodeData::AnonymousBlock(_)) => 1,
            Some(NodeData::Text(_)) => 3,
            Some(NodeData::Comment { .. }) => 8,
            None => 0,
        }
    })
    .into())
}

fn node_name(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(&with_host(|h| {
        if h.fragments.contains(&id) {
            return "#document-fragment".to_string();
        }
        match h.doc.get_node(id).map(|n| &n.data) {
            Some(NodeData::Document(_)) => "#document".into(),
            Some(NodeData::Element(e) | NodeData::AnonymousBlock(e)) => {
                if e.name.ns == ns!(html) {
                    e.name.local.to_ascii_uppercase().to_string()
                } else {
                    e.name.local.to_string()
                }
            }
            Some(NodeData::Text(_)) => "#text".into(),
            Some(NodeData::Comment { .. }) => "#comment".into(),
            None => String::new(),
        }
    })))
}

fn local_name_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| local(h, id)).map(|l| s(&l)).unwrap_or(JsValue::null()))
}

fn namespace_uri(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        if h.fragments.contains(&id) {
            return None;
        }
        h.doc.get_node(id)?.element_data().map(|e| e.name.ns.to_string())
    })
    .map(|n| s(&n))
    .unwrap_or(JsValue::null()))
}

fn parent_node(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| parent(h, id))))
}

fn parent_element(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| parent(h, id).filter(|&p| is_element(h, p)))))
}

fn child_nodes(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let ids = with_host(|h| children(h, id));
    Ok(array_of(ids, ctx))
}

fn children_get(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let ids = with_host(|h| children(h, id).into_iter().filter(|&c| is_element(h, c)).collect());
    Ok(array_of(ids, ctx))
}

fn child_element_count(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| children(h, id).into_iter().filter(|&c| is_element(h, c)).count() as i32).into())
}

fn first_child(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| h.doc.get_node(id).and_then(|n| n.children.first().copied()))))
}
fn last_child(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| h.doc.get_node(id).and_then(|n| n.children.last().copied()))))
}
fn first_element_child(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| children(h, id).into_iter().find(|&c| is_element(h, c)))))
}
fn last_element_child(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| children(h, id).into_iter().rev().find(|&c| is_element(h, c)))))
}
fn next_sibling(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| sibling(h, id, 1, false))))
}
fn previous_sibling(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| sibling(h, id, -1, false))))
}
fn next_element_sibling(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| sibling(h, id, 1, true))))
}
fn previous_element_sibling(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| sibling(h, id, -1, true))))
}
fn owner_document(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        let r = root(h);
        if id == r { JsValue::null() } else { wrap_in(h, r) }
    }))
}
fn is_connected(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| is_connected_in(h, id)).into())
}
fn get_root_node(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        let r = *ancestors_incl(h, id).last().expect("non-empty");
        wrap_in(h, r)
    }))
}
fn has_child_nodes(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| !children(h, id).is_empty()).into())
}

fn text_content_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        if id == root(h) {
            return JsValue::null();
        }
        s(&h.doc.get_node(id).map(|n| n.text_content()).unwrap_or_default())
    }))
}

fn text_content_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    // `null` empties; `undefined` becomes the string "undefined", like
    // QtWebKit 2.2 (verified: the footer substitution script writes
    // "undefined" for a missing variable).
    let v = args.get_or_undefined(0);
    let text = if v.is_null() { String::new() } else { v.to_string(ctx)?.to_std_string_escaped() };
    with_host(|h| {
        let is_char = matches!(
            h.doc.get_node(id).map(|n| &n.data),
            Some(NodeData::Text(_) | NodeData::Comment { .. })
        );
        if is_char {
            set_char_data(h, id, &text);
        } else if id != root(h) {
            detach_children(h, id);
            if !text.is_empty() {
                let mut m = h.doc.mutate();
                let t = m.create_text_node(&text);
                m.append_children(id, &[t]);
            }
        }
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

pub(crate) fn set_char_data(h: &mut Host, id: NodeId, text: &str) {
    let Some(node) = h.doc.get_node_mut(id) else { return };
    match &mut node.data {
        NodeData::Comment { contents } => *contents = text.to_string(),
        NodeData::Text(_) => h.doc.mutate().set_node_text(id, text),
        _ => {}
    }
}

fn data_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| match h.doc.get_node(id).map(|n| &n.data) {
        Some(NodeData::Text(t)) => s(&t.content),
        Some(NodeData::Comment { contents }) => s(contents),
        _ => JsValue::null(),
    }))
}

fn data_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let v = args.get_or_undefined(0);
    let text = if v.is_null() { String::new() } else { v.to_string(ctx)?.to_std_string_escaped() };
    with_host(|h| {
        set_char_data(h, id, &text);
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

/// Ids a value stands for when inserted: fragments are flattened.
fn insertion_nodes(h: &Host, child: NodeId) -> Vec<NodeId> {
    if h.fragments.contains(&child) { children(h, child) } else { vec![child] }
}

/// Shared insertion: `anchor = None` appends. Fragments are flattened.
/// Inserted, connected `<script>` elements run (jQuery's DOMEval relies on
/// this).
pub(crate) fn insert(parent_id: NodeId, child: NodeId, anchor: Option<NodeId>, ctx: &mut Context) -> JsResult<()> {
    let scripts = with_host(|h| {
        if ancestors_incl(h, parent_id).contains(&child) {
            return Err(dom_err("HierarchyRequestError", "the new child is an ancestor of the parent"));
        }
        if child == root(h) {
            return Err(dom_err("HierarchyRequestError", "a document cannot be inserted"));
        }
        let nodes = insertion_nodes(h, child);
        if nodes.is_empty() {
            return Ok(Vec::new());
        }
        if anchor.is_some_and(|a| nodes.contains(&a)) {
            return Ok(Vec::new());
        }
        {
            let mut m = h.doc.mutate();
            match anchor {
                Some(a) => m.insert_nodes_before(a, &nodes),
                None => m.append_children(parent_id, &nodes),
            }
        }
        mark_dirty(h);
        let mut scripts = vec![];
        if is_connected_in(h, parent_id) {
            for &n in &nodes {
                let mut all = vec![n];
                descendants(h, n, &mut all);
                for d in all {
                    if local(h, d).as_deref() == Some("script") && h.executed_scripts.insert(d) {
                        scripts.push(d);
                    }
                }
            }
        }
        Ok(scripts)
    })?;
    for sid in scripts {
        // External scripts load asynchronously; inline ones run now.
        let external = with_host(|h| get_attr(h, sid, "src").is_some_and(|s| !s.trim().is_empty()));
        if external {
            with_host(|h| h.async_scripts.push(sid));
        } else {
            crate::run_inserted_script(sid, ctx);
        }
    }
    Ok(())
}

fn append_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let c = id_of(args.get_or_undefined(0))?;
    insert(id, c, None, ctx)?;
    Ok(args.get_or_undefined(0).clone())
}

fn insert_before(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let c = id_of(args.get_or_undefined(0))?;
    let anchor = opt_id(args.get_or_undefined(1));
    if let Some(a) = anchor {
        if with_host(|h| parent(h, a)) != Some(id) {
            return Err(dom_err("NotFoundError", "the reference node is not a child of this node"));
        }
    }
    insert(id, c, anchor, ctx)?;
    Ok(args.get_or_undefined(0).clone())
}

fn remove_child(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let c = id_of(args.get_or_undefined(0))?;
    with_host(|h| {
        if parent(h, c) != Some(id) {
            return Err(dom_err("NotFoundError", "the node to be removed is not a child of this node"));
        }
        h.doc.mutate().remove_node(c);
        mark_dirty(h);
        Ok(())
    })?;
    Ok(args.get_or_undefined(0).clone())
}

fn replace_child(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let new = id_of(args.get_or_undefined(0))?;
    let old = id_of(args.get_or_undefined(1))?;
    if with_host(|h| parent(h, old)) != Some(id) {
        return Err(dom_err("NotFoundError", "the node to be replaced is not a child of this node"));
    }
    if new != old {
        insert(id, new, Some(old), ctx)?;
        with_host(|h| {
            h.doc.mutate().remove_node(old);
            mark_dirty(h);
        });
    }
    Ok(args.get_or_undefined(1).clone())
}

fn remove_self(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    with_host(|h| {
        if parent(h, id).is_some() {
            h.doc.mutate().remove_node(id);
            mark_dirty(h);
        }
    });
    Ok(JsValue::undefined())
}

fn clone_node(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let deep = args.get_or_undefined(0).to_boolean();
    let new = with_host(|h| {
        if id == root(h) {
            return None;
        }
        let new = {
            let mut m = h.doc.mutate();
            let new = m.deep_clone_node(id);
            if !deep {
                for k in m.child_ids(new) {
                    m.remove_node(k);
                }
            }
            new
        };
        if h.fragments.contains(&id) {
            h.fragments.insert(new);
        }
        // Clones of executed scripts do not run again.
        let mut all = vec![new];
        descendants(h, new, &mut all);
        for d in all {
            if local(h, d).as_deref() == Some("script") {
                h.executed_scripts.insert(d);
            }
        }
        Some(new)
    });
    Ok(wrap_opt(new))
}

fn contains(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let Some(o) = opt_id(args.get_or_undefined(0)) else { return Ok(false.into()) };
    Ok(with_host(|h| ancestors_incl(h, o).contains(&id)).into())
}

fn compare_document_position(this: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let a = id_of(this)?;
    let b = id_of(args.get_or_undefined(0))?;
    if a == b {
        return Ok(0.into());
    }
    let r = with_host(|h| {
        let pa: Vec<NodeId> = ancestors_incl(h, a).into_iter().rev().collect();
        let pb: Vec<NodeId> = ancestors_incl(h, b).into_iter().rev().collect();
        if pa[0] != pb[0] {
            return 1 | 32 | if a < b { 4 } else { 2 };
        }
        if pb.contains(&a) {
            return 16 | 4;
        }
        if pa.contains(&b) {
            return 8 | 2;
        }
        let mut i = 0;
        while pa[i] == pb[i] {
            i += 1;
        }
        let kids = children(h, pa[i - 1]);
        let ia = kids.iter().position(|&c| c == pa[i]).unwrap_or(0);
        let ib = kids.iter().position(|&c| c == pb[i]).unwrap_or(0);
        if ib > ia { 4 } else { 2 }
    });
    Ok(r.into())
}

fn normalize(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    with_host(|h| {
        let mut all = vec![id];
        descendants(h, id, &mut all);
        for p in all {
            let kids = children(h, p);
            let mut prev_text: Option<NodeId> = None;
            for k in kids {
                let text = match h.doc.get_node(k).map(|n| &n.data) {
                    Some(NodeData::Text(t)) => Some(t.content.clone()),
                    _ => None,
                };
                match (text, prev_text) {
                    (Some(t), _) if t.is_empty() => {
                        h.doc.mutate().remove_node(k);
                    }
                    (Some(t), Some(pt)) => {
                        let mut joined = match h.doc.get_node(pt).map(|n| &n.data) {
                            Some(NodeData::Text(x)) => x.content.clone(),
                            _ => String::new(),
                        };
                        joined.push_str(&t);
                        h.doc.mutate().set_node_text(pt, &joined);
                        h.doc.mutate().remove_node(k);
                    }
                    (Some(_), None) => prev_text = Some(k),
                    (None, _) => prev_text = None,
                }
            }
        }
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

// ---------- ParentNode / ChildNode mixins ----------

/// Nodes for `append(...nodes)`: strings become text nodes.
fn nodes_from_args(args: &[JsValue], ctx: &mut Context) -> JsResult<Vec<NodeId>> {
    let mut out = Vec::new();
    for a in args {
        match opt_id(a) {
            Some(id) => out.push(id),
            None => {
                let t = a.to_string(ctx)?.to_std_string_escaped();
                out.push(with_host(|h| h.doc.mutate().create_text_node(&t)));
            }
        }
    }
    Ok(out)
}

fn append_nodes(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    for n in nodes_from_args(args, ctx)? {
        insert(id, n, None, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn prepend_nodes(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let first = with_host(|h| children(h, id).first().copied());
    for n in nodes_from_args(args, ctx)? {
        insert(id, n, first, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn replace_children(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let nodes = nodes_from_args(args, ctx)?;
    with_host(|h| detach_children(h, id));
    for n in nodes {
        insert(id, n, None, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn before_nodes(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let Some(p) = with_host(|h| parent(h, id)) else { return Ok(JsValue::undefined()) };
    for n in nodes_from_args(args, ctx)? {
        insert(p, n, Some(id), ctx)?;
    }
    Ok(JsValue::undefined())
}

fn after_nodes(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let Some(p) = with_host(|h| parent(h, id)) else { return Ok(JsValue::undefined()) };
    let anchor = with_host(|h| sibling(h, id, 1, false));
    for n in nodes_from_args(args, ctx)? {
        insert(p, n, anchor, ctx)?;
    }
    Ok(JsValue::undefined())
}

fn replace_with(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let Some(p) = with_host(|h| parent(h, id)) else { return Ok(JsValue::undefined()) };
    for n in nodes_from_args(args, ctx)? {
        if n != id {
            insert(p, n, Some(id), ctx)?;
        }
    }
    with_host(|h| {
        if parent(h, id).is_some() {
            h.doc.mutate().remove_node(id);
        }
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

// ---------- Element ----------

fn get_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    Ok(with_host(|h| get_attr(h, id, &name)).map(|v| s(&v)).unwrap_or(JsValue::null()))
}

fn has_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    Ok(with_host(|h| get_attr(h, id, &name).is_some()).into())
}

fn set_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?;
    if name.is_empty() || name.contains(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"') {
        return Err(dom_err("InvalidCharacterError", "invalid attribute name"));
    }
    let name = if with_host(|h| h.doc.get_node(id).and_then(|n| n.element_data()).is_some_and(|e| e.name.ns == ns!(html))) {
        name.to_ascii_lowercase()
    } else {
        name
    };
    let value = arg_str(args, 1, ctx)?;
    with_host(|h| set_attr(h, id, &name, &value));
    Ok(JsValue::undefined())
}

fn remove_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    with_host(|h| {
        if get_attr(h, id, &name).is_some() {
            h.doc.mutate().clear_attribute(id, attr_qn(&name));
            mark_dirty(h);
        }
    });
    Ok(JsValue::undefined())
}

fn get_attribute_names(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let names: Vec<JsValue> = with_host(|h| {
        h.doc
            .get_node(id)
            .and_then(|n| n.attrs())
            .map(|a| a.iter().map(|a| s(&a.name.local)).collect())
            .unwrap_or_default()
    });
    Ok(JsArray::from_iter(names, ctx).into())
}

fn id_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(&with_host(|h| get_attr(h, id, "id")).unwrap_or_default()))
}
fn id_set(this: &JsValue, a: &[JsValue], c: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let v = arg_str(a, 0, c)?;
    with_host(|h| set_attr(h, id, "id", &v));
    Ok(JsValue::undefined())
}
fn class_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(&with_host(|h| get_attr(h, id, "class")).unwrap_or_default()))
}
fn class_set(this: &JsValue, a: &[JsValue], c: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let v = arg_str(a, 0, c)?;
    with_host(|h| set_attr(h, id, "class", &v));
    Ok(JsValue::undefined())
}

fn inner_html_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(&with_host(|h| {
        let mut out = String::new();
        serialize_children(h, id, &mut out);
        out
    })))
}

/// Parse `html` as the children of a detached carrier element and return the
/// new nodes, detached. Scripts parsed this way never run (HTML spec).
pub(crate) fn parse_fragment(h: &mut Host, context_tag: &str, html: &str) -> Vec<NodeId> {
    let carrier = {
        let mut m = h.doc.mutate();
        let tag = if context_tag.is_empty() { "div" } else { context_tag };
        let c = m.create_element(QualName::new(None, ns!(html), LocalName::from(tag)), vec![]);
        m.set_inner_html(c, html);
        c
    };
    let kids = children(h, carrier);
    for &k in &kids {
        let mut all = vec![k];
        descendants(h, k, &mut all);
        for d in all {
            if local(h, d).as_deref() == Some("script") {
                h.executed_scripts.insert(d);
            }
        }
    }
    detach_children(h, carrier);
    kids
}

fn inner_html_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let v = args.get_or_undefined(0);
    let html = if v.is_null() { String::new() } else { v.to_string(ctx)?.to_std_string_escaped() };
    with_host(|h| {
        let tag = local(h, id).unwrap_or_default();
        // Detach first: set_inner_html drops the old children, which would
        // let blitz recycle ids that wrappers still hold.
        detach_children(h, id);
        let kids = parse_fragment(h, &tag, &html);
        if !kids.is_empty() {
            h.doc.mutate().append_children(id, &kids);
        }
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

fn outer_html_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(&with_host(|h| {
        let mut out = String::new();
        serialize_node(h, id, &mut out);
        out
    })))
}

fn outer_html_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let html = arg_str(args, 0, ctx)?;
    with_host(|h| {
        let Some(p) = parent(h, id) else { return };
        let tag = local(h, p).unwrap_or_default();
        let kids = parse_fragment(h, &tag, &html);
        let mut m = h.doc.mutate();
        if !kids.is_empty() {
            m.insert_nodes_before(id, &kids);
        }
        m.remove_node(id);
        drop(m);
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

fn insert_adjacent_html(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let pos = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    let html = arg_str(args, 1, ctx)?;
    with_host(|h| {
        let p = parent(h, id);
        let ctx_tag = match pos.as_str() {
            "beforebegin" | "afterend" => p.and_then(|p| local(h, p)).unwrap_or_default(),
            _ => local(h, id).unwrap_or_default(),
        };
        let kids = parse_fragment(h, &ctx_tag, &html);
        if kids.is_empty() {
            return Ok(());
        }
        match pos.as_str() {
            "beforebegin" => match p {
                Some(_) => h.doc.mutate().insert_nodes_before(id, &kids),
                None => return Err(dom_err("NoModificationAllowedError", "no parent")),
            },
            "afterbegin" => match children(h, id).first().copied() {
                Some(f) => h.doc.mutate().insert_nodes_before(f, &kids),
                None => h.doc.mutate().append_children(id, &kids),
            },
            "beforeend" => h.doc.mutate().append_children(id, &kids),
            "afterend" => match (p, sibling(h, id, 1, false)) {
                (Some(_), Some(n)) => h.doc.mutate().insert_nodes_before(n, &kids),
                (Some(p), None) => h.doc.mutate().append_children(p, &kids),
                (None, _) => return Err(dom_err("NoModificationAllowedError", "no parent")),
            },
            _ => return Err(dom_err("SyntaxError", "invalid position")),
        }
        mark_dirty(h);
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_ids_matching(h: &Host, root_id: NodeId, f: &dyn Fn(&Host, NodeId) -> bool) -> Vec<NodeId> {
    let mut all = vec![];
    descendants(h, root_id, &mut all);
    all.into_iter().filter(|&d| is_element(h, d) && f(h, d)).collect()
}

fn get_elements_by_tag_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    let ids = with_host(|h| {
        element_ids_matching(h, id, &|h, d| name == "*" || local(h, d).is_some_and(|l| l.eq_ignore_ascii_case(&name)))
    });
    Ok(array_of(ids, ctx))
}

fn get_elements_by_tag_name_ns(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut a = args.to_vec();
    if !a.is_empty() {
        a.remove(0);
    }
    get_elements_by_tag_name(this, &a, ctx)
}

fn get_elements_by_class_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let wanted: Vec<String> = arg_str(args, 0, ctx)?.split_ascii_whitespace().map(String::from).collect();
    let ids = with_host(|h| {
        element_ids_matching(h, id, &|h, d| {
            let Some(cls) = get_attr(h, d, "class") else { return false };
            !wanted.is_empty() && wanted.iter().all(|w| cls.split_ascii_whitespace().any(|c| c == w))
        })
    });
    Ok(array_of(ids, ctx))
}

fn get_elements_by_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let want = arg_str(args, 0, ctx)?;
    let ids = with_host(|h| element_ids_matching(h, id, &|h, d| get_attr(h, d, "name").as_deref() == Some(&want)));
    Ok(array_of(ids, ctx))
}

fn syntax_err(sel: &str) -> boa_engine::JsError {
    JsNativeError::syntax().with_message(format!("SyntaxError: '{sel}' is not a valid selector")).into()
}

fn query_selector(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    let r = with_host(|h| h.doc.query_selector_in(id, &sel).map(|r| r.filter(|&n| n != id)))
    .map_err(|_| syntax_err(&sel))?;
    Ok(wrap_opt(r))
}

fn query_selector_all(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    let r = with_host(|h| {
        h.doc.query_selector_all_in(id, &sel).map(|v| v.into_iter().filter(|&n| n != id).collect::<Vec<_>>())
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(array_of(r, ctx))
}

fn matches(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    // No element-level matcher is exposed by blitz-dom: query from the root
    // of the node's tree and test membership.
    let r = with_host(|h| {
        let top = *ancestors_incl(h, id).last().expect("non-empty");
        h.doc.query_selector_all_in(top, &sel).map(|v| v.contains(&id))
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(r.into())
}

fn closest(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    let r = with_host(|h| {
        let chain = ancestors_incl(h, id);
        let top = *chain.last().expect("non-empty");
        h.doc.query_selector_all_in(top, &sel).map(|v| chain.into_iter().find(|a| v.contains(a)))
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(wrap_opt(r))
}

// ---- layout-dependent getters: synchronous style + layout when dirty ----

fn rect_obj(r: Option<blitz_dom::BoundingRect>, ctx: &mut Context) -> JsResult<JsValue> {
    let r = r.unwrap_or(blitz_dom::BoundingRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 });
    let o = JsObject::with_object_proto(ctx.intrinsics());
    for (k, v) in [
        ("x", r.x),
        ("y", r.y),
        ("left", r.x),
        ("top", r.y),
        ("width", r.width),
        ("height", r.height),
        ("right", r.x + r.width),
        ("bottom", r.y + r.height),
    ] {
        o.set(JsString::from(k), v, false, ctx)?;
    }
    Ok(o.into())
}

fn layout_rect(h: &mut Host, id: NodeId) -> Option<blitz_dom::BoundingRect> {
    if !is_element(h, id) || !is_connected_in(h, id) {
        return None;
    }
    h.ensure_layout();
    h.doc.get_client_bounding_rect(id)
}

fn get_bounding_client_rect(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let r = with_host(|h| layout_rect(h, id));
    rect_obj(r, ctx)
}

fn offset(this: &JsValue, f: fn(&blitz_dom::BoundingRect) -> f64) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        if !is_element(h, id) || !is_connected_in(h, id) {
            return 0.0;
        }
        h.ensure_layout();
        h.doc.offset_rect(id).map(|r| f(&r).round()).unwrap_or(0.0)
    })
    .into())
}
fn offset_width(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    offset(t, |r| r.width)
}
fn offset_height(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    offset(t, |r| r.height)
}
fn offset_top(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    offset(t, |r| r.y)
}
fn offset_left(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    offset(t, |r| r.x)
}
fn offset_parent(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(t)?;
    Ok(wrap_opt(with_host(|h| {
        if !is_connected_in(h, id) {
            return None;
        }
        h.doc.find_body_node().map(|b| b.id).filter(|&b| b != id)
    })))
}
fn client_size(t: &JsValue, width: bool) -> JsResult<JsValue> {
    let id = id_of(t)?;
    Ok(with_host(|h| {
        if !is_element(h, id) || !is_connected_in(h, id) {
            return 0.0;
        }
        h.ensure_layout();
        let Some(n) = h.doc.get_node(id) else { return 0.0 };
        (if width { n.client_width() } else { n.client_height() }) as f64
    })
    .into())
}
fn client_width(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    client_size(t, true)
}
fn client_height(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    client_size(t, false)
}
fn zero(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(0.into())
}
fn noop(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

// ---------- Document ----------

fn get_element_by_id(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let scope = id_of(this)?;
    let want = arg_str(args, 0, ctx)?;
    Ok(wrap_opt(with_host(|h| {
        if scope == root(h) {
            if let Some(found) = h.doc.get_element_by_id(&want) {
                if is_connected_in(h, found) {
                    return Some(found);
                }
            }
        }
        let mut all = vec![];
        descendants(h, scope, &mut all);
        all.into_iter().find(|&d| get_attr(h, d, "id").as_deref() == Some(want.as_str()))
    })))
}

fn create_element(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let tag = arg_str(args, 0, ctx)?;
    if tag.is_empty() || tag.contains(|c: char| c.is_whitespace() || c == '<' || c == '>') {
        return Err(dom_err("InvalidCharacterError", "invalid tag name"));
    }
    let tag = tag.to_ascii_lowercase();
    let id = with_host(|h| h.doc.mutate().create_element(QualName::new(None, ns!(html), LocalName::from(tag)), vec![]));
    Ok(wrap(id))
}

fn create_element_ns(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let nsv = args.get_or_undefined(0);
    let nsuri = if nsv.is_null_or_undefined() { String::new() } else { nsv.to_string(ctx)?.to_std_string_escaped() };
    let qname = arg_str(args, 1, ctx)?;
    let local_part = qname.rsplit(':').next().unwrap_or(&qname).to_string();
    let namespace = match nsuri.as_str() {
        "http://www.w3.org/1999/xhtml" => ns!(html),
        "http://www.w3.org/2000/svg" => ns!(svg),
        "http://www.w3.org/1998/Math/MathML" => ns!(mathml),
        "" => ns!(),
        other => blitz_dom::Namespace::from(other),
    };
    let id = with_host(|h| h.doc.mutate().create_element(QualName::new(None, namespace, LocalName::from(local_part)), vec![]));
    Ok(wrap(id))
}

fn create_text_node(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = arg_str(args, 0, ctx)?;
    Ok(wrap(with_host(|h| h.doc.mutate().create_text_node(&t))))
}

fn create_comment(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = arg_str(args, 0, ctx)?;
    Ok(wrap(with_host(|h| h.doc.mutate().create_comment_node(&t))))
}

fn create_document_fragment(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    // blitz-dom has no DocumentFragment node kind: a detached carrier element
    // tracked as a fragment (nodeType 11, flattened on insertion).
    let id = with_host(|h| {
        let id = h
            .doc
            .mutate()
            .create_element(QualName::new(None, ns!(html), LocalName::from("wk-fragment")), vec![]);
        h.fragments.insert(id);
        id
    });
    Ok(wrap(id))
}

fn document_element(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(wrap_opt(with_host(|h| {
        let r = root(h);
        children(h, r).into_iter().find(|&c| is_element(h, c))
    })))
}
fn body_get(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(wrap_opt(with_host(|h| {
        let html = children(h, root(h)).into_iter().find(|&c| is_element(h, c))?;
        children(h, html).into_iter().find(|&c| matches!(local(h, c).as_deref(), Some("body" | "frameset")))
    })))
}
fn head_get(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(wrap_opt(with_host(|h| {
        let html = children(h, root(h)).into_iter().find(|&c| is_element(h, c))?;
        children(h, html).into_iter().find(|&c| local(h, c).as_deref() == Some("head"))
    })))
}
fn title_get(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let t = with_host(|h| {
        let ids = element_ids_matching(h, root(h), &|h, d| local(h, d).as_deref() == Some("title"));
        ids.first().and_then(|&t| h.doc.get_node(t)).map(|n| n.text_content()).unwrap_or_default()
    });
    Ok(s(&t.split_whitespace().collect::<Vec<_>>().join(" ")))
}
fn title_set(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = arg_str(args, 0, ctx)?;
    with_host(|h| {
        let found = element_ids_matching(h, root(h), &|h, d| local(h, d).as_deref() == Some("title")).first().copied();
        let title = match found {
            Some(id) => id,
            None => {
                let Some(head) = h.doc.find_head_node().map(|n| n.id) else { return };
                let mut m = h.doc.mutate();
                let id = m.create_element(QualName::new(None, ns!(html), local_name!("title")), vec![]);
                m.append_children(head, &[id]);
                id
            }
        };
        detach_children(h, title);
        let mut m = h.doc.mutate();
        let tn = m.create_text_node(&t);
        m.append_children(title, &[tn]);
    });
    Ok(JsValue::undefined())
}

/// `document.write`: while parsing, the markup goes back into the parser's
/// input right after the current `<script>` (the driver feeds it). Written
/// from a script inserted later, it is parsed as a fragment inserted after
/// that script. Without a current script (after the load), it is ignored
/// (WebKit would replace the document).
fn document_write(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut html = String::new();
    for i in 0..args.len() {
        html.push_str(&arg_str(args, i, ctx)?);
    }
    with_host(|h| {
        if h.parsing && !h.current_script.is_empty() {
            h.write_buffer.push_str(&html);
            return;
        }
        let Some(&script) = h.current_script.last() else { return };
        let Some(parent_id) = parent(h, script) else { return };
        let tag = local(h, parent_id).unwrap_or_default();
        let kids = parse_fragment(h, &tag, &html);
        // Scripts written this way run (queued after the current one).
        for &k in &kids {
            let mut all = vec![k];
            descendants(h, k, &mut all);
            for d in all {
                if local(h, d).as_deref() == Some("script") {
                    h.executed_scripts.remove(&d);
                    h.pending_scripts.push(d);
                }
            }
        }
        let anchor = h.write_anchor.get(&script).copied().flatten().or_else(|| sibling(h, script, 1, false));
        if !kids.is_empty() {
            let mut m = h.doc.mutate();
            match anchor {
                Some(a) => m.insert_nodes_before(a, &kids),
                None => m.append_children(parent_id, &kids),
            }
        }
        h.write_anchor.insert(script, anchor);
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

fn document_writeln(t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut a = args.to_vec();
    a.push(s("\n"));
    document_write(t, &a, ctx)
}

fn import_node(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let n = args.get_or_undefined(0).clone();
    clone_node(&n, &args[1.min(args.len())..], ctx)
}

fn adopt_node(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    with_host(|h| {
        if parent(h, id).is_some() {
            h.doc.mutate().remove_node(id);
            mark_dirty(h);
        }
    });
    Ok(args.get_or_undefined(0).clone())
}

// ---------- install ----------

pub(crate) fn install(ctx: &mut Context) -> Protos {
    let mut by_name: HashMap<&'static str, JsObject> = HashMap::new();
    for (name, parent_name) in INTERFACES {
        let o = JsObject::with_object_proto(ctx.intrinsics());
        if let Some(p) = parent_name {
            o.set_prototype(Some(by_name[p].clone()));
        }
        by_name.insert(name, o);
    }
    let p = |n: &str| by_name[n].clone();
    let node = p("Node");
    let character_data = p("CharacterData");
    let element = p("Element");
    let document = p("Document");
    let fragment = p("DocumentFragment");

    for (n, g, st) in [
        ("nodeType", node_type as Fp, None),
        ("nodeName", node_name, None),
        ("parentNode", parent_node, None),
        ("parentElement", parent_element, None),
        ("childNodes", child_nodes, None),
        ("firstChild", first_child, None),
        ("lastChild", last_child, None),
        ("nextSibling", next_sibling, None),
        ("previousSibling", previous_sibling, None),
        ("ownerDocument", owner_document, None),
        ("isConnected", is_connected, None),
        ("textContent", text_content_get, Some(text_content_set as Fp)),
    ] {
        accessor(ctx, &node, n, g, st);
    }
    for (n, l, f) in [
        ("appendChild", 1, append_child as Fp),
        ("insertBefore", 2, insert_before),
        ("removeChild", 1, remove_child),
        ("replaceChild", 2, replace_child),
        ("cloneNode", 1, clone_node),
        ("contains", 1, contains),
        ("hasChildNodes", 0, has_child_nodes),
        ("compareDocumentPosition", 1, compare_document_position),
        ("getRootNode", 0, get_root_node),
        ("normalize", 0, normalize),
    ] {
        method(ctx, &node, n, l, f);
    }
    // ParentNode mixin.
    for o in [&element, &document, &fragment] {
        for (n, g) in [
            ("children", children_get as Fp),
            ("firstElementChild", first_element_child),
            ("lastElementChild", last_element_child),
            ("childElementCount", child_element_count),
        ] {
            accessor(ctx, o, n, g, None);
        }
        for (n, l, f) in [
            ("querySelector", 1, query_selector as Fp),
            ("querySelectorAll", 1, query_selector_all),
            ("getElementsByTagName", 1, get_elements_by_tag_name),
            ("getElementsByTagNameNS", 2, get_elements_by_tag_name_ns),
            ("getElementsByClassName", 1, get_elements_by_class_name),
            ("append", 0, append_nodes),
            ("prepend", 0, prepend_nodes),
            ("replaceChildren", 0, replace_children),
        ] {
            method(ctx, o, n, l, f);
        }
    }
    // CharacterData.
    for n in ["data", "nodeValue"] {
        accessor(ctx, &character_data, n, data_get, Some(data_set));
    }
    // ChildNode mixin.
    for o in [&character_data, &element] {
        for (n, l, f) in [
            ("remove", 0, remove_self as Fp),
            ("before", 0, before_nodes),
            ("after", 0, after_nodes),
            ("replaceWith", 0, replace_with),
        ] {
            method(ctx, o, n, l, f);
        }
        accessor(ctx, o, "nextElementSibling", next_element_sibling, None);
        accessor(ctx, o, "previousElementSibling", previous_element_sibling, None);
    }
    // Element.
    for (n, g, st) in [
        ("tagName", node_name as Fp, None),
        ("localName", local_name_get, None),
        ("namespaceURI", namespace_uri, None),
        ("id", id_get, Some(id_set as Fp)),
        ("className", class_get, Some(class_set as Fp)),
        ("innerHTML", inner_html_get, Some(inner_html_set as Fp)),
        ("outerHTML", outer_html_get, Some(outer_html_set as Fp)),
        ("offsetWidth", offset_width, None),
        ("offsetHeight", offset_height, None),
        ("offsetTop", offset_top, None),
        ("offsetLeft", offset_left, None),
        ("offsetParent", offset_parent, None),
        ("clientWidth", client_width, None),
        ("clientHeight", client_height, None),
        ("clientTop", zero, None),
        ("clientLeft", zero, None),
        ("scrollTop", zero, Some(noop as Fp)),
        ("scrollLeft", zero, Some(noop as Fp)),
        ("scrollWidth", client_width, None),
        ("scrollHeight", client_height, None),
    ] {
        accessor(ctx, &element, n, g, st);
    }
    for (n, l, f) in [
        ("getAttribute", 1, get_attribute as Fp),
        ("setAttribute", 2, set_attribute),
        ("removeAttribute", 1, remove_attribute),
        ("hasAttribute", 1, has_attribute),
        ("getAttributeNames", 0, get_attribute_names),
        ("matches", 1, matches),
        ("webkitMatchesSelector", 1, matches),
        ("closest", 1, closest),
        ("getBoundingClientRect", 0, get_bounding_client_rect),
        ("insertAdjacentHTML", 2, insert_adjacent_html),
        ("getElementsByName", 1, get_elements_by_name),
    ] {
        method(ctx, &element, n, l, f);
    }
    // Document.
    for (n, g, st) in [
        ("documentElement", document_element as Fp, None),
        ("body", body_get, None),
        ("head", head_get, None),
        ("title", title_get, Some(title_set as Fp)),
    ] {
        accessor(ctx, &document, n, g, st);
    }
    for (n, l, f) in [
        ("getElementById", 1, get_element_by_id as Fp),
        ("getElementsByName", 1, get_elements_by_name),
        ("createElement", 1, create_element),
        ("createElementNS", 2, create_element_ns),
        ("createTextNode", 1, create_text_node),
        ("createComment", 1, create_comment),
        ("createDocumentFragment", 0, create_document_fragment),
        ("write", 1, document_write),
        ("writeln", 1, document_writeln),
        ("importNode", 2, import_node),
        ("adoptNode", 1, adopt_node),
    ] {
        method(ctx, &document, n, l, f);
    }
    method(ctx, &fragment, "getElementById", 1, get_element_by_id);

    Protos { by_name }
}
