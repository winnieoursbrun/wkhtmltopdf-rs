//! Native DOM bindings: Boa objects wrapping blitz-dom `NodeId`s.
//!
//! Wrapper model: every JS node object is an ordinary Boa object whose native data is
//! `NodeW { id }`, created with the prototype matching the node kind. A `NodeId -> JsObject`
//! cache keeps wrapper identity stable (`a.firstChild === a.firstChild`). The cache holds strong
//! (rooted) handles, so a wrapper - and therefore its node - lives as long as the document. Nodes
//! are only ever *detached* by JS-visible operations, never dropped, so an id held by a wrapper can
//! never be recycled by blitz's slab for another node.
use crate::host::{with_host, Host};
use blitz_dom::{local_name, ns, LocalName, NodeData, NodeId, QualName};

pub fn root() -> NodeId {
    with_host(|h| h.doc.root_node().id)
}
use boa_engine::object::builtins::JsArray;
use boa_engine::object::FunctionObjectBuilder;
use boa_engine::property::PropertyDescriptor;
use boa_engine::{
    Context, Finalize, JsArgs, JsData, JsNativeError, JsObject, JsResult, JsString, JsValue, NativeFunction, Trace,
};

type Fp = fn(&JsValue, &[JsValue], &mut Context) -> JsResult<JsValue>;

#[derive(Debug, Trace, Finalize, JsData)]
pub struct NodeW {
    #[unsafe_ignore_trace]
    pub id: NodeId,
}

pub struct Protos {
    pub event_target: JsObject,
    pub node: JsObject,
    pub character_data: JsObject,
    pub element: JsObject,
    pub text: JsObject,
    pub comment: JsObject,
    pub document: JsObject,
    pub fragment: JsObject,
}

// ---------- helpers ----------

pub fn s(v: &str) -> JsValue {
    JsValue::from(JsString::from(v))
}

fn type_err(msg: &str) -> boa_engine::JsError {
    JsNativeError::typ().with_message(msg.to_string()).into()
}

pub fn id_of(v: &JsValue) -> JsResult<NodeId> {
    v.as_object()
        .and_then(|o| o.downcast_ref::<NodeW>().map(|n| n.id))
        .ok_or_else(|| type_err("value is not a Node"))
}

fn opt_id(v: &JsValue) -> Option<NodeId> {
    v.as_object().and_then(|o| o.downcast_ref::<NodeW>().map(|n| n.id))
}

pub fn arg_str(args: &[JsValue], i: usize, ctx: &mut Context) -> JsResult<String> {
    Ok(args.get_or_undefined(i).to_string(ctx)?.to_std_string_escaped())
}

fn attr_qn(name: &str) -> QualName {
    QualName::new(None, ns!(), LocalName::from(name))
}

/// Return (creating on first use) the wrapper object for a node.
pub fn wrap(id: NodeId) -> JsValue {
    with_host(|h| wrap_in(h, id))
}

pub fn wrap_in(h: &mut Host, id: NodeId) -> JsValue {
    if let Some(o) = h.cache.get(&id) {
        return o.clone().into();
    }
    let p = h.protos.as_ref().expect("protos");
    let node = h.doc.get_node(id).expect("live node");
    let proto = if h.fragments.contains(&id) {
        &p.fragment
    } else {
        match &node.data {
            NodeData::Document(_) => &p.document,
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => &p.element,
            NodeData::Text(_) => &p.text,
            NodeData::Comment { .. } => &p.comment,
        }
    };
    let o = JsObject::from_proto_and_data(proto.clone(), NodeW { id });
    h.cache.insert(id, o.clone());
    h.stats_wrappers += 1;
    o.into()
}

fn wrap_opt(id: Option<NodeId>) -> JsValue {
    id.map(wrap).unwrap_or(JsValue::null())
}

fn array_of(ids: Vec<NodeId>, ctx: &mut Context) -> JsValue {
    let vals: Vec<JsValue> = with_host(|h| ids.into_iter().map(|i| wrap_in(h, i)).collect());
    JsArray::from_iter(vals, ctx).into()
}

fn method(ctx: &mut Context, obj: &JsObject, name: &str, len: usize, f: Fp) {
    let func = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(f))
        .name(JsString::from(name))
        .length(len)
        .build();
    obj.define_property_or_throw(
        JsString::from(name),
        PropertyDescriptor::builder().value(func).writable(true).enumerable(false).configurable(true),
        ctx,
    )
    .unwrap();
}

fn accessor(ctx: &mut Context, obj: &JsObject, name: &str, get: Fp, set: Option<Fp>) {
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
    obj.define_property_or_throw(JsString::from(name), d, ctx).unwrap();
}

// ---------- tree helpers (pure Rust, over the blitz arena) ----------

fn is_element(h: &Host, id: NodeId) -> bool {
    h.doc.get_node(id).is_some_and(|n| n.is_element())
}

fn children(h: &Host, id: NodeId) -> Vec<NodeId> {
    h.doc.get_node(id).map(|n| n.children.to_vec()).unwrap_or_default()
}

fn parent(h: &Host, id: NodeId) -> Option<NodeId> {
    h.doc.get_node(id).and_then(|n| n.parent)
}

fn sibling(h: &Host, id: NodeId, delta: isize, elements_only: bool) -> Option<NodeId> {
    let p = parent(h, id)?;
    let kids = children(h, p);
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

fn descendants(h: &Host, root: NodeId, out: &mut Vec<NodeId>) {
    for c in children(h, root) {
        out.push(c);
        descendants(h, c, out);
    }
}

fn local(h: &Host, id: NodeId) -> Option<String> {
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

/// Detach all children (never drop: JS may hold wrappers to them).
fn detach_children(h: &mut Host, id: NodeId) {
    let kids = children(h, id);
    let mut m = h.doc.mutate();
    for k in kids {
        m.remove_node(k);
    }
}

fn mark_dirty(h: &mut Host) {
    h.layout_dirty = true;
    h.stats_mutations += 1;
}

// ---------- Node ----------

fn node_type(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        if h.fragments.contains(&id) {
            return 11;
        }
        match &h.doc.get_node(id).unwrap().data {
            NodeData::Document(_) => 9,
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => 1,
            NodeData::Text(_) => 3,
            NodeData::Comment { .. } => 8,
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
        match &h.doc.get_node(id).unwrap().data {
            NodeData::Document(_) => "#document".into(),
            NodeData::Element(e) | NodeData::AnonymousBlock(e) => e.name.local.to_ascii_uppercase().to_string(),
            NodeData::Text(_) => "#text".into(),
            NodeData::Comment { .. } => "#comment".into(),
        }
    })))
}

fn local_name_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| local(h, id)).map(|l| s(&l)).unwrap_or(JsValue::null()))
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

fn first_child(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| children(h, id).first().copied())))
}
fn last_child(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(wrap_opt(with_host(|h| children(h, id).last().copied())))
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
    Ok(if id == root() { JsValue::null() } else { wrap(root()) })
}
fn is_connected(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| *ancestors_incl(h, id).last().unwrap() == h.doc.root_node().id).into())
}
fn has_child_nodes(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| !children(h, id).is_empty()).into())
}

fn text_content_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    if id == root() {
        return Ok(JsValue::null());
    }
    Ok(s(&with_host(|h| h.doc.get_node(id).unwrap().text_content())))
}

fn text_content_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let v = args.get_or_undefined(0);
    let text = if v.is_null_or_undefined() { String::new() } else { v.to_string(ctx)?.to_std_string_escaped() };
    with_host(|h| {
        let is_text = matches!(h.doc.get_node(id).unwrap().data, NodeData::Text(_) | NodeData::Comment { .. });
        if is_text {
            set_char_data(h, id, &text);
        } else {
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

fn set_char_data(h: &mut Host, id: NodeId, text: &str) {
    match &mut h.doc.get_node_mut(id).unwrap().data {
        NodeData::Comment { contents } => *contents = text.to_string(),
        _ => h.doc.mutate().set_node_text(id, text),
    }
}

fn data_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| match &h.doc.get_node(id).unwrap().data {
        NodeData::Text(t) => s(&t.content),
        NodeData::Comment { contents } => s(contents),
        _ => JsValue::null(),
    }))
}

fn data_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let text = arg_str(args, 0, ctx)?;
    with_host(|h| {
        set_char_data(h, id, &text);
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

/// Shared insertion: `anchor = None` appends. Fragments are flattened. Inserted, connected
/// `<script>` elements without `src` run immediately (jQuery's DOMEval relies on this).
fn insert(parent_id: NodeId, child: NodeId, anchor: Option<NodeId>, ctx: &mut Context) -> JsResult<()> {
    let (nodes, scripts) = with_host(|h| {
        if ancestors_incl(h, parent_id).contains(&child) {
            return Err(type_err("HierarchyRequestError: the new child is an ancestor of the parent"));
        }
        let nodes = if h.fragments.contains(&child) { children(h, child) } else { vec![child] };
        {
            let mut m = h.doc.mutate();
            match anchor {
                Some(a) => m.insert_nodes_before(a, &nodes),
                None => m.append_children(parent_id, &nodes),
            }
        }
        mark_dirty(h);
        let connected = *ancestors_incl(h, parent_id).last().unwrap() == h.doc.root_node().id;
        let mut scripts = vec![];
        if connected {
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
        Ok((nodes, scripts))
    })?;
    let _ = nodes;
    for sid in scripts {
        crate::host::run_script_element(sid, ctx);
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
            return Err(type_err("NotFoundError: reference node is not a child of this node"));
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
            return Err(type_err("NotFoundError: the node to be removed is not a child of this node"));
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
    insert(id, new, Some(old), ctx)?;
    with_host(|h| h.doc.mutate().remove_node(old));
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
        let mut m = h.doc.mutate();
        let new = m.deep_clone_node(id);
        if !deep {
            for k in m.child_ids(new) {
                m.remove_node(k);
            }
        }
        if h.fragments.contains(&id) {
            h.fragments.insert(new);
        }
        new
    });
    Ok(wrap(new))
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
            return 1 | 32 | if a < b { 4 } else { 2 }; // DISCONNECTED | IMPLEMENTATION_SPECIFIC
        }
        if pb.contains(&a) {
            return 16 | 4; // b is contained by a, follows
        }
        if pa.contains(&b) {
            return 8 | 2; // b contains a, precedes
        }
        let mut i = 0;
        while pa[i] == pb[i] {
            i += 1;
        }
        let kids = children(h, pa[i - 1]);
        let ia = kids.iter().position(|&c| c == pa[i]).unwrap();
        let ib = kids.iter().position(|&c| c == pb[i]).unwrap();
        if ib > ia { 4 } else { 2 }
    });
    Ok(r.into())
}

// ---------- Element ----------

fn get_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    Ok(with_host(|h| h.doc.get_node(id).unwrap().attr(LocalName::from(name)).map(s)).unwrap_or(JsValue::null()))
}

fn has_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    Ok(with_host(|h| h.doc.get_node(id).unwrap().attr(LocalName::from(name)).is_some()).into())
}

pub fn set_attr(h: &mut Host, id: NodeId, name: &str, value: &str) {
    h.doc.mutate().set_attribute(id, attr_qn(name), value);
    mark_dirty(h);
}

fn set_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    let value = arg_str(args, 1, ctx)?;
    with_host(|h| set_attr(h, id, &name, &value));
    Ok(JsValue::undefined())
}

fn remove_attribute(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    with_host(|h| {
        h.doc.mutate().clear_attribute(id, attr_qn(&name));
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

fn get_attribute_names(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let names: Vec<JsValue> = with_host(|h| {
        h.doc.get_node(id).unwrap().attrs().map(|a| a.iter().map(|a| s(&a.name.local)).collect()).unwrap_or_default()
    });
    Ok(JsArray::from_iter(names, ctx).into())
}

fn attr_accessor_get(this: &JsValue, name: &str) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(with_host(|h| h.doc.get_node(id).unwrap().attr(LocalName::from(name)).unwrap_or("").to_string()).as_str()))
}
fn attr_accessor_set(this: &JsValue, args: &[JsValue], ctx: &mut Context, name: &str) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let v = arg_str(args, 0, ctx)?;
    with_host(|h| set_attr(h, id, name, &v));
    Ok(JsValue::undefined())
}
fn id_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    attr_accessor_get(this, "id")
}
fn id_set(this: &JsValue, a: &[JsValue], c: &mut Context) -> JsResult<JsValue> {
    attr_accessor_set(this, a, c, "id")
}
fn class_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    attr_accessor_get(this, "class")
}
fn class_set(this: &JsValue, a: &[JsValue], c: &mut Context) -> JsResult<JsValue> {
    attr_accessor_set(this, a, c, "class")
}

fn inner_html_get(this: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(s(&with_host(|h| {
        let mut out = String::new();
        serialize_children(h, id, &mut out);
        out
    })))
}

fn inner_html_set(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let html = arg_str(args, 0, ctx)?;
    with_host(|h| {
        // Detach first: set_inner_html drops the old children, which would let blitz recycle ids
        // that JS wrappers still hold.
        detach_children(h, id);
        let new: Vec<NodeId> = {
            let mut m = h.doc.mutate();
            m.set_inner_html(id, &html);
            m.child_ids(id).to_vec()
        };
        // Scripts inserted through innerHTML never run (HTML spec).
        for n in new {
            let mut all = vec![n];
            descendants(h, n, &mut all);
            for d in all {
                if local(h, d).as_deref() == Some("script") {
                    h.executed_scripts.insert(d);
                }
            }
        }
        mark_dirty(h);
        h.stats_inner_html += 1;
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

fn get_elements_by_tag_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let name = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    let ids = with_host(|h| {
        let mut all = vec![];
        descendants(h, id, &mut all);
        all.into_iter().filter(|&d| local(h, d).is_some_and(|l| name == "*" || l == name)).collect()
    });
    Ok(array_of(ids, ctx))
}

fn get_elements_by_class_name(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let wanted: Vec<String> = arg_str(args, 0, ctx)?.split_ascii_whitespace().map(String::from).collect();
    let ids = with_host(|h| {
        let mut all = vec![];
        descendants(h, id, &mut all);
        all.into_iter()
            .filter(|&d| {
                let Some(cls) = h.doc.get_node(d).unwrap().attr(local_name!("class")) else { return false };
                !wanted.is_empty() && wanted.iter().all(|w| cls.split_ascii_whitespace().any(|c| c == w))
            })
            .collect()
    });
    Ok(array_of(ids, ctx))
}

fn syntax_err(sel: &str) -> boa_engine::JsError {
    JsNativeError::syntax().with_message(format!("SyntaxError: '{sel}' is not a valid selector")).into()
}

fn query_selector(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    let r = with_host(|h| {
        h.stats_queries += 1;
        h.doc.query_selector_in(id, &sel)
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(wrap_opt(r))
}

fn query_selector_all(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    let r = with_host(|h| {
        h.stats_queries += 1;
        h.doc.query_selector_all_in(id, &sel).map(|v| v.to_vec())
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(array_of(r, ctx))
}

fn matches(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    // No element-level matcher is exposed by blitz-dom: query from the root and test membership.
    let r = with_host(|h| {
        h.stats_queries += 1;
        let root = *ancestors_incl(h, id).last().unwrap();
        h.doc.query_selector_all_in(root, &sel).map(|v| v.contains(&id))
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(r.into())
}

fn closest(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let sel = arg_str(args, 0, ctx)?;
    let r = with_host(|h| {
        let chain = ancestors_incl(h, id);
        let root = *chain.last().unwrap();
        h.doc.query_selector_all_in(root, &sel).map(|v| chain.into_iter().find(|a| v.contains(a)))
    })
    .map_err(|_| syntax_err(&sel))?;
    Ok(wrap_opt(r))
}

// ---- layout-dependent getters: force a synchronous style + layout pass when dirty ----

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

fn get_bounding_client_rect(this: &JsValue, _: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(this)?;
    let r = with_host(|h| {
        h.ensure_layout();
        h.doc.get_client_bounding_rect(id)
    });
    rect_obj(r, ctx)
}

fn offset(this: &JsValue, f: fn(&blitz_dom::BoundingRect) -> f64) -> JsResult<JsValue> {
    let id = id_of(this)?;
    Ok(with_host(|h| {
        h.ensure_layout();
        h.doc.offset_rect(id).map(|r| f(&r)).unwrap_or(0.0)
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
fn client_width(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(t)?;
    Ok(with_host(|h| {
        h.ensure_layout();
        h.doc.get_node(id).unwrap().client_width() as f64
    })
    .into())
}
fn client_height(t: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = id_of(t)?;
    Ok(with_host(|h| {
        h.ensure_layout();
        h.doc.get_node(id).unwrap().client_height() as f64
    })
    .into())
}

// ---------- Document ----------

fn get_element_by_id(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let idv = arg_str(args, 0, ctx)?;
    Ok(wrap_opt(with_host(|h| h.doc.get_element_by_id(&idv))))
}

fn create_element(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let tag = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    let id = with_host(|h| {
        h.stats_created += 1;
        h.doc.mutate().create_element(QualName::new(None, ns!(html), LocalName::from(tag)), vec![])
    });
    Ok(wrap(id))
}

fn create_text_node(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = arg_str(args, 0, ctx)?;
    let id = with_host(|h| {
        h.stats_created += 1;
        h.doc.mutate().create_text_node(&t)
    });
    Ok(wrap(id))
}

fn create_comment(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = arg_str(args, 0, ctx)?;
    Ok(wrap(with_host(|h| h.doc.mutate().create_comment_node(&t))))
}

fn create_document_fragment(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    // blitz-dom has no DocumentFragment node kind: use a detached carrier element, tracked as a
    // fragment so that nodeType/nodeName/insertion semantics follow the DOM.
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
    Ok(wrap_opt(with_host(|h| h.doc.try_root_element().map(|n| n.id))))
}
fn body_get(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(wrap_opt(with_host(|h| h.doc.find_body_node().map(|n| n.id))))
}
fn head_get(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(wrap_opt(with_host(|h| h.doc.find_head_node().map(|n| n.id))))
}
fn title_get(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(s(with_host(|h| h.doc.find_title_node().map(|n| n.text_content())).unwrap_or_default().trim()))
}
fn title_set(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let t = arg_str(args, 0, ctx)?;
    with_host(|h| {
        let title = match h.doc.find_title_node().map(|n| n.id) {
            Some(id) => id,
            None => {
                let head = h.doc.find_head_node().map(|n| n.id).unwrap();
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

/// `document.write` during parsing: the parser is not re-entrant in this spike (blitz-html parses
/// the whole input up front), so the markup is parsed as a fragment and inserted right after the
/// currently executing `<script>` element, which is where a script-pausing parser would put it.
fn document_write(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut html = String::new();
    for i in 0..args.len() {
        html.push_str(&arg_str(args, i, ctx)?);
    }
    with_host(|h| {
        h.stats_doc_write += 1;
        let Some(&script) = h.current_script.last() else {
            eprintln!("spike: document.write after load is not supported (would replace the document)");
            return;
        };
        let parent_id = parent(h, script).unwrap();
        let carrier = {
            let mut m = h.doc.mutate();
            let c = m.create_element(QualName::new(None, ns!(html), local_name!("div")), vec![]);
            m.set_inner_html(c, &html);
            c
        };
        let kids = children(h, carrier);
        // Keep successive writes in order: insert before the node following the last insertion.
        let anchor = h.write_anchor.get(&script).copied().flatten().or_else(|| sibling(h, script, 1, false));
        let mut m = h.doc.mutate();
        match anchor {
            Some(a) => m.insert_nodes_before(a, &kids),
            None => m.append_children(parent_id, &kids),
        }
        drop(m);
        h.write_anchor.insert(script, anchor);
        // Scripts written by document.write do run; queue them after the current one.
        for k in kids {
            let mut all = vec![k];
            descendants(h, k, &mut all);
            for d in all {
                if local(h, d).as_deref() == Some("script") {
                    h.pending_written_scripts.push(d);
                }
            }
        }
        mark_dirty(h);
    });
    Ok(JsValue::undefined())
}

fn document_writeln(t: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let mut a = args.to_vec();
    a.push(s("\n"));
    document_write(t, &a, ctx)
}

// ---------- install ----------

pub fn install(ctx: &mut Context) -> Protos {
    let mk = |ctx: &mut Context, parent: Option<&JsObject>| {
        let o = JsObject::with_object_proto(ctx.intrinsics());
        if let Some(p) = parent {
            o.set_prototype(Some(p.clone()));
        }
        o
    };
    let event_target = mk(ctx, None);
    let node = mk(ctx, Some(&event_target));
    let character_data = mk(ctx, Some(&node));
    let element = mk(ctx, Some(&node));
    let text = mk(ctx, Some(&character_data));
    let comment = mk(ctx, Some(&character_data));
    let document = mk(ctx, Some(&node));
    let fragment = mk(ctx, Some(&node));

    // Node
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
    ] {
        method(ctx, &node, n, l, f);
    }
    // ParentNode mixin (Element, Document, DocumentFragment)
    for o in [&element, &document, &fragment] {
        for (n, g) in [
            ("children", children_get as Fp),
            ("firstElementChild", first_element_child),
            ("lastElementChild", last_element_child),
        ] {
            accessor(ctx, o, n, g, None);
        }
        for (n, l, f) in [
            ("querySelector", 1, query_selector as Fp),
            ("querySelectorAll", 1, query_selector_all),
            ("getElementsByTagName", 1, get_elements_by_tag_name),
            ("getElementsByClassName", 1, get_elements_by_class_name),
        ] {
            method(ctx, o, n, l, f);
        }
    }
    // CharacterData
    for n in ["data", "nodeValue"] {
        accessor(ctx, &character_data, n, data_get, Some(data_set));
    }
    accessor(ctx, &character_data, "length", |t, a, c| {
        let d = data_get(t, a, c)?;
        Ok(d.as_string().map(|s| s.len() as i32).unwrap_or(0).into())
    }, None);
    for o in [&character_data, &element] {
        method(ctx, o, "remove", 0, remove_self);
        accessor(ctx, o, "nextElementSibling", next_element_sibling, None);
        accessor(ctx, o, "previousElementSibling", previous_element_sibling, None);
    }
    // Element
    for (n, g, st) in [
        ("tagName", node_name as Fp, None),
        ("localName", local_name_get, None),
        ("id", id_get, Some(id_set as Fp)),
        ("className", class_get, Some(class_set as Fp)),
        ("innerHTML", inner_html_get, Some(inner_html_set as Fp)),
        ("outerHTML", outer_html_get, None),
        ("offsetWidth", offset_width, None),
        ("offsetHeight", offset_height, None),
        ("offsetTop", offset_top, None),
        ("offsetLeft", offset_left, None),
        ("clientWidth", client_width, None),
        ("clientHeight", client_height, None),
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
    ] {
        method(ctx, &element, n, l, f);
    }
    // Document
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
        ("createElement", 1, create_element),
        ("createTextNode", 1, create_text_node),
        ("createComment", 1, create_comment),
        ("createDocumentFragment", 0, create_document_fragment),
        ("write", 1, document_write),
        ("writeln", 1, document_writeln),
    ] {
        method(ctx, &document, n, l, f);
    }
    method(ctx, &fragment, "getElementById", 1, |this, args, ctx| {
        let id = id_of(this)?;
        let want = arg_str(args, 0, ctx)?;
        Ok(wrap_opt(with_host(|h| {
            let mut all = vec![];
            descendants(h, id, &mut all);
            all.into_iter().find(|&d| h.doc.get_node(d).unwrap().attr(local_name!("id")) == Some(want.as_str()))
        })))
    });

    Protos { event_target, node, character_data, element, text, comment, document, fragment }
}

// ---------- HTML serialisation (HTML spec "serializing HTML fragments") ----------
// blitz-dom's `outer_html` writes empty elements XML-style (`<tbody />`, `<script ... />`), which
// re-parses differently in HTML, so innerHTML/outerHTML need their own serializer.

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr",
];
const RAW_TEXT: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes", "plaintext", "noscript"];

fn escape(s: &str, attr: bool, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '"' if attr => out.push_str("&quot;"),
            '<' if !attr => out.push_str("&lt;"),
            '>' if !attr => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

pub fn serialize_node(h: &Host, id: NodeId, out: &mut String) {
    let node = h.doc.get_node(id).unwrap();
    match &node.data {
        NodeData::Element(e) | NodeData::AnonymousBlock(e) => {
            let name = e.name.local.to_string();
            out.push('<');
            out.push_str(&name);
            for a in e.attrs.iter() {
                out.push(' ');
                out.push_str(&a.name.local);
                out.push_str("=\"");
                escape(&a.value, true, out);
                out.push('"');
            }
            out.push('>');
            if VOID.contains(&name.as_str()) {
                return;
            }
            serialize_children(h, id, out);
            out.push_str("</");
            out.push_str(&name);
            out.push('>');
        }
        NodeData::Text(t) => {
            let raw = node.parent.and_then(|p| local(h, p)).is_some_and(|p| RAW_TEXT.contains(&p.as_str()));
            if raw { out.push_str(&t.content) } else { escape(&t.content, false, out) }
        }
        NodeData::Comment { contents } => {
            out.push_str("<!--");
            out.push_str(contents);
            out.push_str("-->");
        }
        NodeData::Document(_) => serialize_children(h, id, out),
    }
}

pub fn serialize_children(h: &Host, id: NodeId, out: &mut String) {
    for c in children(h, id) {
        serialize_node(h, c, out);
    }
}
