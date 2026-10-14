//! HTML serialisation for `innerHTML`/`outerHTML` (HTML spec "serializing
//! HTML fragments"). blitz-dom's own `outer_html` writes empty elements
//! XML-style (`<tbody />`), which re-parses differently as HTML.

use blitz_dom::{NodeData, NodeId, ns};

use crate::dom::{children, local};
use crate::host::Host;

const VOID: &[&str] = &[
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input", "keygen",
    "link", "meta", "param", "source", "track", "wbr",
];
const RAW_TEXT: &[&str] =
    &["script", "style", "xmp", "iframe", "noembed", "noframes", "plaintext", "noscript"];

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

pub(crate) fn serialize_node(h: &Host, id: NodeId, out: &mut String) {
    let Some(node) = h.doc.get_node(id) else { return };
    if h.fragments.contains(&id) {
        serialize_children(h, id, out);
        return;
    }
    match &node.data {
        NodeData::Element(e) | NodeData::AnonymousBlock(e) => {
            let name = e.name.local.to_string();
            out.push('<');
            out.push_str(&name);
            for a in e.attrs.iter() {
                out.push(' ');
                if a.name.ns == ns!(xlink) {
                    out.push_str("xlink:");
                }
                out.push_str(&a.name.local);
                out.push_str("=\"");
                escape(&a.value, true, out);
                out.push('"');
            }
            out.push('>');
            if e.name.ns == ns!(html) && VOID.contains(&name.as_str()) {
                return;
            }
            if let Some(t) = e.template_contents {
                serialize_children(h, t, out);
            } else {
                serialize_children(h, id, out);
            }
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

pub(crate) fn serialize_children(h: &Host, id: NodeId, out: &mut String) {
    for c in children(h, id) {
        serialize_node(h, c, out);
    }
}
