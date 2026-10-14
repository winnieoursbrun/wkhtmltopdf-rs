//! A small namespace-aware XML parser and tree, the input side of the TOC
//! transformation. Stands in for the `QXmlQuery` document loading of
//! QtXmlPatterns used by upstream `src/lib/pdfconverter.cc` (`loadTocs`).
//!
//! Supported: elements, attributes, namespaces (prefixes resolved to URIs),
//! character and the five predefined entity references, CDATA, comments;
//! the XML declaration, processing instructions and the DOCTYPE are
//! skipped. Well-formedness errors carry the 1-based line and column of the
//! offending character, and the wording of QtXmlPatterns for the errors the
//! TOC can hit (an unescaped `&` in `--toc-header-text`).

use std::collections::HashMap;

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// A node of the tree; children and attributes are arena indices.
#[derive(Debug, Clone)]
pub enum NodeKind {
    Root,
    Element { ns: String, local: String, prefix: String },
    Attribute { ns: String, local: String, value: String },
    Text(String),
    Comment(String),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub attrs: Vec<usize>,
    /// Namespace declarations in scope on an element (prefix → URI).
    pub ns_scope: HashMap<String, String>,
}

/// A parsed document. Node 0 is the root.
#[derive(Debug, Clone)]
pub struct Doc {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct XmlError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl Doc {
    pub fn node(&self, i: usize) -> &Node {
        &self.nodes[i]
    }

    /// The document element.
    pub fn document_element(&self) -> Option<usize> {
        self.nodes[0]
            .children
            .iter()
            .copied()
            .find(|&c| matches!(self.nodes[c].kind, NodeKind::Element { .. }))
    }

    /// XPath string value.
    pub fn string_value(&self, i: usize) -> String {
        match &self.nodes[i].kind {
            NodeKind::Attribute { value, .. } => value.clone(),
            NodeKind::Text(t) | NodeKind::Comment(t) => t.clone(),
            NodeKind::Root | NodeKind::Element { .. } => {
                let mut s = String::new();
                self.collect_text(i, &mut s);
                s
            }
        }
    }

    fn collect_text(&self, i: usize, out: &mut String) {
        for &c in &self.nodes[i].children {
            match &self.nodes[c].kind {
                NodeKind::Text(t) => out.push_str(t),
                NodeKind::Element { .. } => self.collect_text(c, out),
                _ => {}
            }
        }
    }

    /// Attribute `local` without namespace.
    pub fn attr(&self, el: usize, local: &str) -> Option<&str> {
        self.nodes[el].attrs.iter().find_map(|&a| match &self.nodes[a].kind {
            NodeKind::Attribute { ns, local: l, value } if ns.is_empty() && l == local => {
                Some(value.as_str())
            }
            _ => None,
        })
    }

    pub fn is_element(&self, i: usize, ns: &str, local: &str) -> bool {
        matches!(&self.nodes[i].kind, NodeKind::Element { ns: n, local: l, .. } if n == ns && l == local)
    }

    /// Resolve a QName prefix in the scope of element `el`.
    pub fn resolve_prefix(&self, el: usize, prefix: &str) -> Option<String> {
        if prefix == "xml" {
            return Some(XML_NS.into());
        }
        self.nodes[el].ns_scope.get(prefix).cloned()
    }
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    line: usize,
    col: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn starts(&self, p: &str) -> bool {
        p.chars().enumerate().all(|(k, c)| self.s.get(self.i + k) == Some(&c))
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, XmlError> {
        Err(XmlError { line: self.line, column: self.col, message: message.into() })
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace()) {
            self.bump();
        }
    }

    fn name(&mut self) -> Result<String, XmlError> {
        let mut n = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.') || (c as u32) > 0x7f {
                n.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if n.is_empty() {
            let got = self.peek().map(|c| c.to_string()).unwrap_or_default();
            return self.err(format!("Parse error: Expected a name, but got '{got}'."));
        }
        Ok(n)
    }

    /// `&…;` at the current position (the `&` not yet consumed).
    fn reference(&mut self, out: &mut String) -> Result<(), XmlError> {
        self.bump();
        match self.peek() {
            Some('#') => {
                self.bump();
                let hex = self.peek() == Some('x');
                if hex {
                    self.bump();
                }
                let mut digits = String::new();
                while let Some(c) = self.peek() {
                    if c == ';' {
                        break;
                    }
                    digits.push(c);
                    self.bump();
                }
                if self.bump() != Some(';') {
                    return self.err("Parse error: Expected ';'.");
                }
                let v = if hex { u32::from_str_radix(&digits, 16) } else { digits.parse() };
                match v.ok().and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => return self.err("Parse error: Invalid character reference."),
                }
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' || c == ':' => {
                let n = self.name()?;
                if self.peek() != Some(';') {
                    let got = self.peek().map(|c| c.to_string()).unwrap_or_default();
                    return self.err(format!("Parse error: Expected ';', but got '{got}'."));
                }
                self.bump();
                match n.as_str() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    _ => return self.err(format!("Parse error: Entity '{n}' not declared.")),
                }
            }
            other => {
                let got = other.map(|c| c.to_string()).unwrap_or_default();
                return self.err(format!("Parse error: Expected '#' or '[a-zA-Z]', but got '{got}'."));
            }
        }
        Ok(())
    }
}

/// Parse `src` into a tree.
pub fn parse(src: &str) -> Result<Doc, XmlError> {
    let chars: Vec<char> = src.chars().collect();
    let mut p = Parser { s: &chars, i: 0, line: 1, col: 1 };
    let mut nodes = vec![Node {
        kind: NodeKind::Root,
        parent: None,
        children: vec![],
        attrs: vec![],
        ns_scope: HashMap::new(),
    }];
    let mut stack: Vec<usize> = vec![0];
    let mut text = String::new();
    let flush = |nodes: &mut Vec<Node>, stack: &[usize], text: &mut String| {
        if text.is_empty() {
            return;
        }
        let parent = *stack.last().expect("stack");
        if parent == 0 && text.trim().is_empty() {
            text.clear();
            return;
        }
        let id = nodes.len();
        nodes.push(Node {
            kind: NodeKind::Text(std::mem::take(text)),
            parent: Some(parent),
            children: vec![],
            attrs: vec![],
            ns_scope: HashMap::new(),
        });
        nodes[parent].children.push(id);
    };
    while p.peek().is_some() {
        if p.starts("<?") {
            flush(&mut nodes, &stack, &mut text);
            while p.peek().is_some() && !p.starts("?>") {
                p.bump();
            }
            p.skip(2);
        } else if p.starts("<!--") {
            flush(&mut nodes, &stack, &mut text);
            p.skip(4);
            let mut c = String::new();
            while p.peek().is_some() && !p.starts("-->") {
                c.push(p.bump().expect("char"));
            }
            p.skip(3);
            let parent = *stack.last().expect("stack");
            let id = nodes.len();
            nodes.push(Node {
                kind: NodeKind::Comment(c),
                parent: Some(parent),
                children: vec![],
                attrs: vec![],
                ns_scope: HashMap::new(),
            });
            nodes[parent].children.push(id);
        } else if p.starts("<![CDATA[") {
            p.skip(9);
            while p.peek().is_some() && !p.starts("]]>") {
                text.push(p.bump().expect("char"));
            }
            p.skip(3);
        } else if p.starts("<!") {
            flush(&mut nodes, &stack, &mut text);
            let mut depth = 0;
            while let Some(c) = p.bump() {
                match c {
                    '[' => depth += 1,
                    ']' => depth -= 1,
                    '>' if depth <= 0 => break,
                    _ => {}
                }
            }
        } else if p.starts("</") {
            flush(&mut nodes, &stack, &mut text);
            p.skip(2);
            let n = p.name()?;
            p.ws();
            if p.bump() != Some('>') {
                return p.err("Parse error: Expected '>'.");
            }
            let Some(&top) = stack.last() else { return p.err("Parse error: Unexpected end tag.") };
            let open = match &nodes[top].kind {
                NodeKind::Element { prefix, local, .. } => {
                    if prefix.is_empty() {
                        local.clone()
                    } else {
                        format!("{prefix}:{local}")
                    }
                }
                _ => return p.err(format!("Parse error: Unexpected end tag '{n}'.")),
            };
            if open != n {
                return p.err(format!("Parse error: Opening and ending tag mismatch: '{open}' and '{n}'."));
            }
            stack.pop();
        } else if p.peek() == Some('<') {
            flush(&mut nodes, &stack, &mut text);
            p.bump();
            let qname = p.name()?;
            let mut raw_attrs: Vec<(String, String)> = Vec::new();
            loop {
                p.ws();
                match p.peek() {
                    Some('>') | Some('/') => break,
                    None => return p.err("Parse error: Unexpected end of document."),
                    _ => {}
                }
                let an = p.name()?;
                p.ws();
                if p.bump() != Some('=') {
                    return p.err("Parse error: Expected '='.");
                }
                p.ws();
                let q = p.bump();
                if !matches!(q, Some('"') | Some('\'')) {
                    return p.err("Parse error: Expected a quoted attribute value.");
                }
                let q = q.expect("quote");
                let mut v = String::new();
                loop {
                    match p.peek() {
                        None => return p.err("Parse error: Unexpected end of document."),
                        Some(c) if c == q => {
                            p.bump();
                            break;
                        }
                        Some('&') => p.reference(&mut v)?,
                        Some('<') => return p.err("Parse error: '<' in an attribute value."),
                        Some(c) => {
                            // Attribute value normalisation.
                            v.push(if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c });
                            p.bump();
                        }
                    }
                }
                raw_attrs.push((an, v));
            }
            let empty = p.peek() == Some('/');
            if empty {
                p.bump();
            }
            if p.bump() != Some('>') {
                return p.err("Parse error: Expected '>'.");
            }
            let parent = *stack.last().expect("stack");
            let mut scope = nodes[parent].ns_scope.clone();
            for (k, v) in &raw_attrs {
                if k == "xmlns" {
                    scope.insert(String::new(), v.clone());
                } else if let Some(pfx) = k.strip_prefix("xmlns:") {
                    scope.insert(pfx.to_string(), v.clone());
                }
            }
            let (prefix, local) = match qname.split_once(':') {
                Some((a, b)) => (a.to_string(), b.to_string()),
                None => (String::new(), qname.clone()),
            };
            let ns = if prefix == "xml" {
                XML_NS.to_string()
            } else {
                match scope.get(&prefix) {
                    Some(u) => u.clone(),
                    None if prefix.is_empty() => String::new(),
                    None => return p.err(format!("Parse error: Namespace prefix '{prefix}' not declared.")),
                }
            };
            let id = nodes.len();
            nodes.push(Node {
                kind: NodeKind::Element { ns, local, prefix },
                parent: Some(parent),
                children: vec![],
                attrs: vec![],
                ns_scope: scope.clone(),
            });
            nodes[parent].children.push(id);
            for (k, v) in raw_attrs {
                if k == "xmlns" || k.starts_with("xmlns:") {
                    continue;
                }
                let (ans, alocal) = match k.split_once(':') {
                    Some((pfx, l)) => (
                        if pfx == "xml" {
                            XML_NS.to_string()
                        } else {
                            scope.get(pfx).cloned().unwrap_or_default()
                        },
                        l.to_string(),
                    ),
                    None => (String::new(), k),
                };
                let aid = nodes.len();
                nodes.push(Node {
                    kind: NodeKind::Attribute { ns: ans, local: alocal, value: v },
                    parent: Some(id),
                    children: vec![],
                    attrs: vec![],
                    ns_scope: HashMap::new(),
                });
                nodes[id].attrs.push(aid);
            }
            if !empty {
                stack.push(id);
            }
        } else if p.peek() == Some('&') {
            p.reference(&mut text)?;
        } else {
            let c = p.bump().expect("char");
            if stack.len() == 1 && !c.is_whitespace() {
                return p.err("Parse error: Content outside the document element.");
            }
            text.push(if c == '\r' { '\n' } else { c });
        }
    }
    flush(&mut nodes, &stack, &mut text);
    if stack.len() > 1 {
        return p.err("Parse error: Premature end of document.");
    }
    let doc = Doc { nodes };
    if doc.document_element().is_none() {
        return p.err("Parse error: Premature end of document.");
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_and_entities() {
        let d =
            parse(r#"<?xml version="1.0"?><o:a xmlns:o="urn:x" b="1 &amp; 2"><c>t&lt;</c><!--k--></o:a>"#)
                .unwrap();
        let a = d.document_element().unwrap();
        assert!(d.is_element(a, "urn:x", "a"));
        assert_eq!(d.attr(a, "b"), Some("1 & 2"));
        assert_eq!(d.string_value(a), "t<");
    }

    #[test]
    fn unescaped_ampersand_like_qtxmlpatterns() {
        let e = parse("<a>\n  <t>A & B</t></a>").unwrap_err();
        assert_eq!((e.line, e.column), (2, 9));
        assert_eq!(e.message, "Parse error: Expected '#' or '[a-zA-Z]', but got ' '.");
    }
}
