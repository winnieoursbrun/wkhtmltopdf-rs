//! A small XSLT processor for the table of contents. Stands in for
//! `QXmlQuery(QXmlQuery::XSLT20)` as used by upstream
//! `src/lib/pdfconverter.cc` (`loadTocs`): the outline XML (§3.5) is
//! transformed by the default TOC style sheet (`tocstylesheet.rs`) or by a
//! user `--xsl-style-sheet`.
//!
//! Supported subset (enough for the default sheet and simple custom ones,
//! whatever their `version` attribute says):
//! - top level: `xsl:template` (`match`, `name`, `mode`, `priority`),
//!   `xsl:output` (`doctype-public`, `doctype-system`), `xsl:variable`,
//!   `xsl:param`; other declarations are ignored;
//! - instructions: `apply-templates` (`select`, `mode`, `with-param`),
//!   `call-template`, `value-of`, `text`, `if`, `choose`/`when`/`otherwise`,
//!   `for-each`, `attribute`, `element`, `comment`, `copy`, `copy-of`,
//!   `variable`, `param`, `number` (ignored), `sort` (ignored); literal result
//!   elements with attribute value templates;
//! - XPath 1.0 expressions: location paths with the child, attribute,
//!   descendant(-or-self), self, parent, ancestor(-or-self) and sibling
//!   axes, abbreviations, predicates, unions, variables, arithmetic,
//!   comparisons, `and`/`or`, and the core string/number/boolean/node-set
//!   functions;
//! - match patterns made of child (`/`) and descendant (`//`) steps.
//!
//! Not supported (XSLT 2.0 proper): sequences, `xsl:function`, grouping,
//! regular expressions, schema types, `xsl:result-document`, keys, imports
//! and includes. An unsupported instruction is an error, like a static
//! error of QtXmlPatterns: nothing is output.
//!
//! The result is serialised as XML (empty elements as `<x/>`), as
//! QtXmlPatterns does; QtWebKit then reads the file as HTML, so `<ol/>`
//! opens an element that is never closed.

use std::collections::HashMap;

use crate::xml::{Doc, NodeKind, XmlError};

pub const XSL_NS: &str = "http://www.w3.org/1999/XSL/Transform";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";

/// A transformation error, reported like QtXmlPatterns:
/// `Error <code> in <uri>, at line <l>, column <c>: <message>`.
#[derive(Debug, Clone, PartialEq)]
pub struct XsltError {
    pub code: &'static str,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl XsltError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        XsltError { code, line: 0, column: 0, message: message.into() }
    }

    fn from_xml(code: &'static str, e: XmlError) -> Self {
        XsltError { code, line: e.line, column: e.column, message: e.message }
    }

    /// The message QtXmlPatterns prints for a style sheet whose base URI is
    /// `base`.
    pub fn report(&self, base: &str) -> String {
        format!(
            "Error {} in {base}, at line {}, column {}: {}",
            self.code, self.line, self.column, self.message
        )
    }
}

// ---------------------------------------------------------------------------
// XPath.

#[derive(Debug, Clone)]
enum NodeTest {
    Name(String, String),
    Any,
    NsAny(String),
    Text,
    Node,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Axis {
    Child,
    Attribute,
    Descendant,
    DescendantOrSelf,
    Itself,
    Parent,
    Ancestor,
    AncestorOrSelf,
    FollowingSibling,
    PrecedingSibling,
}

#[derive(Debug, Clone)]
struct Step {
    axis: Axis,
    test: NodeTest,
    preds: Vec<Expr>,
}

#[derive(Debug, Clone)]
enum Start {
    Root,
    Context,
    Expr(Box<Expr>),
}

#[derive(Debug, Clone)]
enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    Arith(char, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    Union(Box<Expr>, Box<Expr>),
    Path(Start, Vec<Step>),
    Filter(Box<Expr>, Vec<Expr>),
    Lit(String),
    Num(f64),
    Var(String),
    Func(String, Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Str(String),
    Num(f64),
    Sym(&'static str),
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        let sym2 = ["//", "::", "..", "!=", "<=", ">="].iter().find(|s| **s == two);
        if let Some(s) = sym2 {
            out.push(Tok::Sym(s));
            i += 2;
            continue;
        }
        if ch == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit) || ch.is_ascii_digit() {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                i += 1;
            }
            let n: String = c[st..i].iter().collect();
            out.push(Tok::Num(n.parse().map_err(|_| format!("bad number {n}"))?));
            continue;
        }
        if ch == '"' || ch == '\'' {
            let st = i + 1;
            i += 1;
            while i < c.len() && c[i] != ch {
                i += 1;
            }
            if i >= c.len() {
                return Err("unterminated string literal".into());
            }
            out.push(Tok::Str(c[st..i].iter().collect()));
            i += 1;
            continue;
        }
        if let Some(s) = ["/", "(", ")", "[", "]", ".", "@", ",", "|", "+", "-", "=", "<", ">", "*", "$"]
            .iter()
            .find(|s| s.starts_with(ch))
        {
            out.push(Tok::Sym(s));
            i += 1;
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || matches!(c[i], '_' | '-' | '.')) {
                i += 1;
            }
            // prefix:local or prefix:*
            if i + 1 < c.len() && c[i] == ':' && c[i + 1] != ':' {
                i += 1;
                if c[i] == '*' {
                    i += 1;
                } else {
                    while i < c.len() && (c[i].is_alphanumeric() || matches!(c[i], '_' | '-' | '.')) {
                        i += 1;
                    }
                }
            }
            out.push(Tok::Name(c[st..i].iter().collect()));
            continue;
        }
        return Err(format!("unexpected character '{ch}'"));
    }
    Ok(out)
}

struct XPathParser<'a> {
    toks: Vec<Tok>,
    i: usize,
    /// Namespace prefixes in scope.
    ns: &'a HashMap<String, String>,
}

impl XPathParser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }

    fn is_name(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Name(x)) if x == s)
    }

    /// An operator name (`and`, `or`, `div`, `mod`, `*`) is an operator
    /// when a preceding token exists and is not an operator or `(`, `[`, `,`,
    /// `@`, `::`.
    fn operator_position(&self) -> bool {
        if self.i == 0 {
            return false;
        }
        !matches!(
            &self.toks[self.i - 1],
            Tok::Sym(
                "@" | "::"
                    | "("
                    | "["
                    | ","
                    | "/"
                    | "//"
                    | "|"
                    | "+"
                    | "-"
                    | "="
                    | "!="
                    | "<"
                    | "<="
                    | ">"
                    | ">="
                    | "$"
            )
        ) && !matches!(&self.toks[self.i - 1], Tok::Name(n) if ["and", "or", "div", "mod"].contains(&n.as_str()) && self.i >= 2)
    }

    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.is_sym(s) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected '{s}'"))
        }
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.is_name("or") && self.operator_position() {
            self.i += 1;
            l = Expr::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }

    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.equality()?;
        while self.is_name("and") && self.operator_position() {
            self.i += 1;
            l = Expr::And(Box::new(l), Box::new(self.equality()?));
        }
        Ok(l)
    }

    fn equality(&mut self) -> Result<Expr, String> {
        let mut l = self.relational()?;
        loop {
            let op = if self.is_sym("=") {
                "="
            } else if self.is_sym("!=") {
                "!="
            } else {
                break;
            };
            self.i += 1;
            l = Expr::Cmp(op, Box::new(l), Box::new(self.relational()?));
        }
        Ok(l)
    }

    fn relational(&mut self) -> Result<Expr, String> {
        let mut l = self.additive()?;
        loop {
            let op = ["<=", ">=", "<", ">"].into_iter().find(|s| self.is_sym(s));
            let Some(op) = op else { break };
            self.i += 1;
            l = Expr::Cmp(op, Box::new(l), Box::new(self.additive()?));
        }
        Ok(l)
    }

    fn additive(&mut self) -> Result<Expr, String> {
        let mut l = self.multiplicative()?;
        loop {
            let op = if self.is_sym("+") {
                '+'
            } else if self.is_sym("-") {
                '-'
            } else {
                break;
            };
            self.i += 1;
            l = Expr::Arith(op, Box::new(l), Box::new(self.multiplicative()?));
        }
        Ok(l)
    }

    fn multiplicative(&mut self) -> Result<Expr, String> {
        let mut l = self.unary()?;
        loop {
            let op = if self.is_sym("*") && self.operator_position() {
                '*'
            } else if self.is_name("div") && self.operator_position() {
                '/'
            } else if self.is_name("mod") && self.operator_position() {
                '%'
            } else {
                break;
            };
            self.i += 1;
            l = Expr::Arith(op, Box::new(l), Box::new(self.unary()?));
        }
        Ok(l)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.is_sym("-") {
            self.i += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        let mut l = self.path()?;
        while self.is_sym("|") {
            self.i += 1;
            l = Expr::Union(Box::new(l), Box::new(self.path()?));
        }
        Ok(l)
    }

    fn starts_step(&self) -> bool {
        match self.peek() {
            Some(Tok::Sym(s)) => matches!(*s, "@" | "." | ".." | "*"),
            Some(Tok::Name(_)) => !self.is_function_call() || self.is_node_type(),
            _ => false,
        }
    }

    fn is_function_call(&self) -> bool {
        matches!(self.peek(), Some(Tok::Name(_))) && matches!(self.toks.get(self.i + 1), Some(Tok::Sym("(")))
    }

    fn is_node_type(&self) -> bool {
        matches!(self.peek(), Some(Tok::Name(n)) if ["text", "node", "comment", "processing-instruction"].contains(&n.as_str()))
            && matches!(self.toks.get(self.i + 1), Some(Tok::Sym("(")))
    }

    fn path(&mut self) -> Result<Expr, String> {
        if self.is_sym("/") {
            self.i += 1;
            let steps = if self.starts_step() { self.relative()? } else { Vec::new() };
            return Ok(Expr::Path(Start::Root, steps));
        }
        if self.is_sym("//") {
            self.i += 1;
            let mut steps = vec![Step { axis: Axis::DescendantOrSelf, test: NodeTest::Node, preds: vec![] }];
            steps.extend(self.relative()?);
            return Ok(Expr::Path(Start::Root, steps));
        }
        if self.starts_step() {
            return Ok(Expr::Path(Start::Context, self.relative()?));
        }
        // Filter expression.
        let mut prim = self.primary()?;
        let mut preds = Vec::new();
        while self.is_sym("[") {
            self.i += 1;
            preds.push(self.or()?);
            self.expect("]")?;
        }
        if !preds.is_empty() {
            prim = Expr::Filter(Box::new(prim), preds);
        }
        if self.is_sym("/") || self.is_sym("//") {
            let mut steps = Vec::new();
            if self.is_sym("//") {
                steps.push(Step { axis: Axis::DescendantOrSelf, test: NodeTest::Node, preds: vec![] });
            }
            self.i += 1;
            steps.extend(self.relative()?);
            return Ok(Expr::Path(Start::Expr(Box::new(prim)), steps));
        }
        Ok(prim)
    }

    fn relative(&mut self) -> Result<Vec<Step>, String> {
        let mut steps = vec![self.step()?];
        loop {
            if self.is_sym("/") {
                self.i += 1;
                steps.push(self.step()?);
            } else if self.is_sym("//") {
                self.i += 1;
                steps.push(Step { axis: Axis::DescendantOrSelf, test: NodeTest::Node, preds: vec![] });
                steps.push(self.step()?);
            } else {
                break;
            }
        }
        Ok(steps)
    }

    fn step(&mut self) -> Result<Step, String> {
        if self.is_sym(".") {
            self.i += 1;
            return Ok(Step { axis: Axis::Itself, test: NodeTest::Node, preds: vec![] });
        }
        if self.is_sym("..") {
            self.i += 1;
            return Ok(Step { axis: Axis::Parent, test: NodeTest::Node, preds: vec![] });
        }
        let mut axis = Axis::Child;
        if self.is_sym("@") {
            self.i += 1;
            axis = Axis::Attribute;
        } else if let (Some(Tok::Name(n)), Some(Tok::Sym("::"))) = (self.peek(), self.toks.get(self.i + 1)) {
            axis = match n.as_str() {
                "child" => Axis::Child,
                "attribute" => Axis::Attribute,
                "descendant" => Axis::Descendant,
                "descendant-or-self" => Axis::DescendantOrSelf,
                "self" => Axis::Itself,
                "parent" => Axis::Parent,
                "ancestor" => Axis::Ancestor,
                "ancestor-or-self" => Axis::AncestorOrSelf,
                "following-sibling" => Axis::FollowingSibling,
                "preceding-sibling" => Axis::PrecedingSibling,
                other => return Err(format!("unsupported axis {other}")),
            };
            self.i += 2;
        }
        let test = match self.peek().cloned() {
            Some(Tok::Sym("*")) => {
                self.i += 1;
                NodeTest::Any
            }
            Some(Tok::Name(n)) if self.is_node_type() => {
                self.i += 1;
                self.expect("(")?;
                if let Some(Tok::Str(_)) = self.peek() {
                    self.i += 1;
                }
                self.expect(")")?;
                match n.as_str() {
                    "text" => NodeTest::Text,
                    "comment" => NodeTest::Comment,
                    _ => NodeTest::Node,
                }
            }
            Some(Tok::Name(n)) => {
                self.i += 1;
                match n.split_once(':') {
                    Some((p, "*")) => {
                        NodeTest::NsAny(self.ns.get(p).cloned().ok_or(format!("unknown prefix {p}"))?)
                    }
                    Some((p, l)) => NodeTest::Name(
                        self.ns.get(p).cloned().ok_or(format!("unknown prefix {p}"))?,
                        l.to_string(),
                    ),
                    None => NodeTest::Name(String::new(), n),
                }
            }
            _ => return Err("expected a node test".into()),
        };
        let mut preds = Vec::new();
        while self.is_sym("[") {
            self.i += 1;
            preds.push(self.or()?);
            self.expect("]")?;
        }
        Ok(Step { axis, test, preds })
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().cloned() {
            Some(Tok::Str(s)) => {
                self.i += 1;
                Ok(Expr::Lit(s))
            }
            Some(Tok::Num(n)) => {
                self.i += 1;
                Ok(Expr::Num(n))
            }
            Some(Tok::Sym("$")) => {
                self.i += 1;
                match self.peek().cloned() {
                    Some(Tok::Name(n)) => {
                        self.i += 1;
                        Ok(Expr::Var(n))
                    }
                    _ => Err("expected a variable name".into()),
                }
            }
            Some(Tok::Sym("(")) => {
                self.i += 1;
                let e = self.or()?;
                self.expect(")")?;
                Ok(e)
            }
            Some(Tok::Name(n)) if self.is_function_call() => {
                self.i += 2;
                let mut args = Vec::new();
                if !self.is_sym(")") {
                    args.push(self.or()?);
                    while self.is_sym(",") {
                        self.i += 1;
                        args.push(self.or()?);
                    }
                }
                self.expect(")")?;
                Ok(Expr::Func(n, args))
            }
            _ => Err("unexpected token".into()),
        }
    }
}

fn parse_expr(s: &str, ns: &HashMap<String, String>) -> Result<Expr, String> {
    let toks = tokenize(s)?;
    let mut p = XPathParser { toks, i: 0, ns };
    let e = p.or()?;
    if p.i != p.toks.len() {
        return Err(format!("unexpected trailing tokens in '{s}'"));
    }
    Ok(e)
}

#[derive(Debug, Clone)]
enum Val {
    Nodes(Vec<usize>),
    Str(String),
    Num(f64),
    Bool(bool),
}

fn fmt_num(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn str_to_num(s: &str) -> f64 {
    s.trim().parse().unwrap_or(f64::NAN)
}

struct Ctx {
    node: usize,
    pos: usize,
    size: usize,
}

// ---------------------------------------------------------------------------
// Result tree.

#[derive(Debug, Clone)]
enum RNode {
    Elem { name: String, ns: String, attrs: Vec<(String, String)>, children: Vec<RNode> },
    Text(String),
    Comment(String),
}

#[derive(Default)]
struct Out {
    /// Open elements; the bottom one is a synthetic container.
    stack: Vec<RNode>,
}

impl Out {
    fn new() -> Self {
        Out {
            stack: vec![RNode::Elem {
                name: String::new(),
                ns: String::new(),
                attrs: vec![],
                children: vec![],
            }],
        }
    }

    fn push(&mut self, n: RNode) {
        if let Some(RNode::Elem { children, .. }) = self.stack.last_mut() {
            if let (RNode::Text(t), Some(RNode::Text(prev))) = (&n, children.last_mut()) {
                prev.push_str(t);
                return;
            }
            children.push(n);
        }
    }

    fn text(&mut self, t: &str) {
        if !t.is_empty() {
            self.push(RNode::Text(t.to_string()));
        }
    }

    fn open(&mut self, name: String, ns: String) {
        self.stack.push(RNode::Elem { name, ns, attrs: vec![], children: vec![] });
    }

    fn close(&mut self) {
        let n = self.stack.pop().expect("open element");
        self.push(n);
    }

    fn attr(&mut self, name: String, value: String) {
        if self.stack.len() < 2 {
            return;
        }
        if let Some(RNode::Elem { attrs, .. }) = self.stack.last_mut() {
            if let Some(a) = attrs.iter_mut().find(|(k, _)| *k == name) {
                a.1 = value;
            } else {
                attrs.push((name, value));
            }
        }
    }

    fn into_children(mut self) -> Vec<RNode> {
        while self.stack.len() > 1 {
            self.close();
        }
        match self.stack.pop() {
            Some(RNode::Elem { children, .. }) => children,
            _ => vec![],
        }
    }
}

fn esc_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn esc_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn serialize(nodes: &[RNode], out: &mut String, top: bool) {
    for n in nodes {
        match n {
            RNode::Text(t) => esc_text(t, out),
            RNode::Comment(c) => {
                out.push_str("<!--");
                out.push_str(c);
                out.push_str("-->");
            }
            RNode::Elem { name, ns, attrs, children } => {
                out.push('<');
                out.push_str(name);
                if top && !ns.is_empty() && !name.contains(':') {
                    out.push_str(" xmlns=\"");
                    esc_attr(ns, out);
                    out.push('"');
                }
                for (k, v) in attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    esc_attr(v, out);
                    out.push('"');
                }
                if children.is_empty() {
                    out.push_str("/>");
                } else {
                    out.push('>');
                    serialize(children, out, false);
                    out.push_str("</");
                    out.push_str(name);
                    out.push('>');
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Style sheet.

#[derive(Debug, Clone)]
struct PatternStep {
    test: NodeTest,
    attribute: bool,
    preds: Vec<Expr>,
    /// Separator to the previous step: `true` for `//`.
    descendant: bool,
}

#[derive(Debug, Clone)]
struct Pattern {
    /// `None` for `/`.
    steps: Option<Vec<PatternStep>>,
    absolute: bool,
}

struct Template {
    pattern: Option<Pattern>,
    name: Option<String>,
    mode: Option<String>,
    priority: f64,
    node: usize,
}

/// A compiled style sheet.
pub struct Stylesheet {
    doc: Doc,
    templates: Vec<Template>,
    globals: Vec<usize>,
    doctype_public: Option<String>,
    doctype_system: Option<String>,
    exprs: std::cell::RefCell<HashMap<(usize, String), Expr>>,
}

fn default_priority(p: &Pattern) -> f64 {
    match &p.steps {
        None => 0.5,
        Some(s) if s.len() == 1 && s[0].preds.is_empty() => match s[0].test {
            NodeTest::Name(..) => 0.0,
            NodeTest::NsAny(_) => -0.25,
            _ => -0.5,
        },
        _ => 0.5,
    }
}

fn parse_pattern(s: &str, ns: &HashMap<String, String>) -> Result<Vec<Pattern>, String> {
    let e = parse_expr(s, ns)?;
    let mut alts = Vec::new();
    fn collect(e: Expr, alts: &mut Vec<Expr>) {
        match e {
            Expr::Union(a, b) => {
                collect(*a, alts);
                collect(*b, alts);
            }
            e => alts.push(e),
        }
    }
    collect(e, &mut alts);
    let mut out = Vec::new();
    for a in alts {
        let Expr::Path(start, steps) = a else { return Err(format!("unsupported pattern '{s}'")) };
        let absolute = matches!(start, Start::Root);
        if absolute && steps.is_empty() {
            out.push(Pattern { steps: None, absolute: true });
            continue;
        }
        let mut ps: Vec<PatternStep> = Vec::new();
        let mut descendant = false;
        for (i, st) in steps.into_iter().enumerate() {
            if st.axis == Axis::DescendantOrSelf && matches!(st.test, NodeTest::Node) && st.preds.is_empty() {
                if i == 0 && absolute {
                    // `//x`: any x.
                    descendant = false;
                    continue;
                }
                descendant = true;
                continue;
            }
            match st.axis {
                Axis::Child | Axis::Attribute => {}
                _ => return Err(format!("unsupported pattern axis in '{s}'")),
            }
            ps.push(PatternStep {
                test: st.test,
                attribute: st.axis == Axis::Attribute,
                preds: st.preds,
                descendant,
            });
            descendant = false;
        }
        let leading_any = s.trim_start().starts_with("//");
        out.push(Pattern { steps: Some(ps), absolute: absolute && !leading_any });
    }
    Ok(out)
}

impl Stylesheet {
    /// Parse and compile a style sheet. Errors are static errors
    /// (`XTSE0010` for a malformed sheet).
    pub fn parse(src: &str) -> Result<Stylesheet, XsltError> {
        let mut doc = crate::xml::parse(src).map_err(|e| XsltError::from_xml("XTSE0010", e))?;
        strip_whitespace(&mut doc);
        let root = doc.document_element().ok_or_else(|| XsltError::new("XTSE0010", "No document element"))?;
        if !(doc.is_element(root, XSL_NS, "stylesheet") || doc.is_element(root, XSL_NS, "transform")) {
            return Err(XsltError::new("XTSE0010", "The document element is not xsl:stylesheet"));
        }
        let mut sheet = Stylesheet {
            doc,
            templates: vec![],
            globals: vec![],
            doctype_public: None,
            doctype_system: None,
            exprs: Default::default(),
        };
        let children = sheet.doc.node(root).children.clone();
        for c in children {
            let NodeKind::Element { ns, local, .. } = &sheet.doc.node(c).kind else { continue };
            if ns != XSL_NS {
                continue;
            }
            match local.as_str() {
                "template" => {
                    let name = sheet.doc.attr(c, "name").map(str::to_string);
                    let mode = sheet.doc.attr(c, "mode").map(str::to_string);
                    let prio = sheet.doc.attr(c, "priority").and_then(|p| p.trim().parse::<f64>().ok());
                    match sheet.doc.attr(c, "match") {
                        Some(m) => {
                            let pats = parse_pattern(m, &sheet.doc.node(c).ns_scope)
                                .map_err(|e| XsltError::new("XTSE0340", e))?;
                            for p in pats {
                                let priority = prio.unwrap_or_else(|| default_priority(&p));
                                sheet.templates.push(Template {
                                    pattern: Some(p),
                                    name: name.clone(),
                                    mode: mode.clone(),
                                    priority,
                                    node: c,
                                });
                            }
                        }
                        None => sheet.templates.push(Template {
                            pattern: None,
                            name,
                            mode,
                            priority: 0.0,
                            node: c,
                        }),
                    }
                }
                "output" => {
                    sheet.doctype_public = sheet.doc.attr(c, "doctype-public").map(str::to_string);
                    sheet.doctype_system = sheet.doc.attr(c, "doctype-system").map(str::to_string);
                }
                "variable" | "param" => sheet.globals.push(c),
                _ => {}
            }
        }
        Ok(sheet)
    }

    fn expr(&self, at: usize, src: &str) -> Result<Expr, XsltError> {
        if let Some(e) = self.exprs.borrow().get(&(at, src.to_string())) {
            return Ok(e.clone());
        }
        let e = parse_expr(src, &self.doc.node(at).ns_scope)
            .map_err(|m| XsltError::new("XPST0003", format!("{m} in expression '{src}'")))?;
        self.exprs.borrow_mut().insert((at, src.to_string()), e.clone());
        Ok(e)
    }

    /// Transform `input` (an XML document) and serialise the result.
    pub fn transform(&self, input: &str) -> Result<String, XsltError> {
        let src = crate::xml::parse(input).map_err(|e| XsltError::from_xml("FODC0002", e))?;
        let mut run = Run { sheet: self, src: &src, vars: vec![HashMap::new()], depth: 0 };
        for &g in &self.globals {
            run.declare(g, &Ctx { node: 0, pos: 1, size: 1 })?;
        }
        let mut out = Out::new();
        run.apply(&[0], None, &mut out, &HashMap::new())?;
        let nodes = out.into_children();
        // No XML declaration: the TOC file is read as HTML (an XML declaration
        // would make the engine switch to its XML parser).
        let mut s = String::new();
        if let Some(sys) = &self.doctype_system {
            let root_name = nodes
                .iter()
                .find_map(|n| match n {
                    RNode::Elem { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "html".into());
            match &self.doctype_public {
                Some(p) => s.push_str(&format!("<!DOCTYPE {root_name} PUBLIC \"{p}\" \"{sys}\">\n")),
                None => s.push_str(&format!("<!DOCTYPE {root_name} SYSTEM \"{sys}\">\n")),
            }
        }
        serialize(&nodes, &mut s, true);
        s.push('\n');
        Ok(s)
    }
}

/// Drop whitespace-only text nodes of the style sheet, except in `xsl:text`.
fn strip_whitespace(doc: &mut Doc) {
    let n = doc.nodes.len();
    for i in 0..n {
        let keep = doc.is_element(i, XSL_NS, "text");
        if keep {
            continue;
        }
        let kids = doc.nodes[i].children.clone();
        let filtered: Vec<usize> = kids
            .into_iter()
            .filter(|&c| !matches!(&doc.nodes[c].kind, NodeKind::Text(t) if t.trim().is_empty()))
            .collect();
        doc.nodes[i].children = filtered;
    }
}

struct Run<'a> {
    sheet: &'a Stylesheet,
    src: &'a Doc,
    vars: Vec<HashMap<String, Val>>,
    depth: usize,
}

impl<'a> Run<'a> {
    fn sdoc(&self) -> &'a Doc {
        &self.sheet.doc
    }

    // --- XPath evaluation -------------------------------------------------

    fn string(&self, v: &Val) -> String {
        match v {
            Val::Nodes(n) => n.first().map(|&i| self.src.string_value(i)).unwrap_or_default(),
            Val::Str(s) => s.clone(),
            Val::Num(n) => fmt_num(*n),
            Val::Bool(b) => b.to_string(),
        }
    }

    fn number(&self, v: &Val) -> f64 {
        match v {
            Val::Num(n) => *n,
            Val::Bool(b) => f64::from(u8::from(*b)),
            other => str_to_num(&self.string(other)),
        }
    }

    fn boolean(&self, v: &Val) -> bool {
        match v {
            Val::Nodes(n) => !n.is_empty(),
            Val::Str(s) => !s.is_empty(),
            Val::Num(n) => *n != 0.0 && !n.is_nan(),
            Val::Bool(b) => *b,
        }
    }

    fn test(&self, n: usize, test: &NodeTest, axis: Axis) -> bool {
        let kind = &self.src.node(n).kind;
        let principal_attr = axis == Axis::Attribute;
        match test {
            NodeTest::Node => true,
            NodeTest::Text => matches!(kind, NodeKind::Text(_)),
            NodeTest::Comment => matches!(kind, NodeKind::Comment(_)),
            NodeTest::Any => {
                if principal_attr {
                    matches!(kind, NodeKind::Attribute { .. })
                } else {
                    matches!(kind, NodeKind::Element { .. })
                }
            }
            NodeTest::NsAny(u) => match kind {
                NodeKind::Element { ns, .. } if !principal_attr => ns == u,
                NodeKind::Attribute { ns, .. } if principal_attr => ns == u,
                _ => false,
            },
            NodeTest::Name(u, l) => match kind {
                NodeKind::Element { ns, local, .. } if !principal_attr => ns == u && local == l,
                NodeKind::Attribute { ns, local, .. } if principal_attr => ns == u && local == l,
                _ => false,
            },
        }
    }

    fn descendants(&self, n: usize, out: &mut Vec<usize>) {
        for &c in &self.src.node(n).children {
            out.push(c);
            self.descendants(c, out);
        }
    }

    fn axis_nodes(&self, n: usize, axis: Axis) -> Vec<usize> {
        let node = self.src.node(n);
        match axis {
            Axis::Child => node.children.clone(),
            Axis::Attribute => node.attrs.clone(),
            Axis::Itself => vec![n],
            Axis::Parent => node.parent.into_iter().collect(),
            Axis::Descendant => {
                let mut v = vec![];
                self.descendants(n, &mut v);
                v
            }
            Axis::DescendantOrSelf => {
                let mut v = vec![n];
                self.descendants(n, &mut v);
                v
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                let mut v = vec![];
                if axis == Axis::AncestorOrSelf {
                    v.push(n);
                }
                let mut p = node.parent;
                while let Some(x) = p {
                    v.push(x);
                    p = self.src.node(x).parent;
                }
                v
            }
            Axis::FollowingSibling | Axis::PrecedingSibling => {
                let Some(p) = node.parent else { return vec![] };
                if matches!(node.kind, NodeKind::Attribute { .. }) {
                    return vec![];
                }
                let sibs = &self.src.node(p).children;
                let i = sibs.iter().position(|&s| s == n).unwrap_or(0);
                if axis == Axis::FollowingSibling {
                    sibs[i + 1..].to_vec()
                } else {
                    sibs[..i].iter().rev().copied().collect()
                }
            }
        }
    }

    fn filter(&mut self, nodes: Vec<usize>, preds: &[Expr]) -> Result<Vec<usize>, XsltError> {
        let mut cur = nodes;
        for p in preds {
            let size = cur.len();
            let mut next = Vec::new();
            for (i, &n) in cur.iter().enumerate() {
                let v = self.eval(p, &Ctx { node: n, pos: i + 1, size })?;
                let keep = match v {
                    Val::Num(x) => x == (i + 1) as f64,
                    other => self.boolean(&other),
                };
                if keep {
                    next.push(n);
                }
            }
            cur = next;
        }
        Ok(cur)
    }

    fn eval(&mut self, e: &Expr, c: &Ctx) -> Result<Val, XsltError> {
        Ok(match e {
            Expr::Lit(s) => Val::Str(s.clone()),
            Expr::Num(n) => Val::Num(*n),
            Expr::Var(v) => self
                .vars
                .iter()
                .rev()
                .find_map(|s| s.get(v).cloned())
                .ok_or_else(|| XsltError::new("XPST0008", format!("undefined variable ${v}")))?,
            Expr::Or(a, b) => {
                let a = self.eval(a, c)?;
                if self.boolean(&a) {
                    Val::Bool(true)
                } else {
                    let b = self.eval(b, c)?;
                    Val::Bool(self.boolean(&b))
                }
            }
            Expr::And(a, b) => {
                let a = self.eval(a, c)?;
                if !self.boolean(&a) {
                    Val::Bool(false)
                } else {
                    let b = self.eval(b, c)?;
                    Val::Bool(self.boolean(&b))
                }
            }
            Expr::Cmp(op, a, b) => {
                let a = self.eval(a, c)?;
                let b = self.eval(b, c)?;
                Val::Bool(self.compare(op, &a, &b))
            }
            Expr::Arith(op, a, b) => {
                let a = self.eval(a, c)?;
                let b = self.eval(b, c)?;
                let (x, y) = (self.number(&a), self.number(&b));
                Val::Num(match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    '/' => x / y,
                    _ => x % y,
                })
            }
            Expr::Neg(a) => {
                let a = self.eval(a, c)?;
                Val::Num(-self.number(&a))
            }
            Expr::Union(a, b) => {
                let (Val::Nodes(mut x), Val::Nodes(y)) = (self.eval(a, c)?, self.eval(b, c)?) else {
                    return Err(XsltError::new("XPTY0004", "union of non-node-sets"));
                };
                x.extend(y);
                x.sort_unstable();
                x.dedup();
                Val::Nodes(x)
            }
            Expr::Filter(p, preds) => {
                let Val::Nodes(n) = self.eval(p, c)? else {
                    return Err(XsltError::new("XPTY0004", "predicate on a non-node-set"));
                };
                Val::Nodes(self.filter(n, preds)?)
            }
            Expr::Path(start, steps) => {
                let mut cur = match start {
                    Start::Root => vec![0],
                    Start::Context => vec![c.node],
                    Start::Expr(e) => match self.eval(e, c)? {
                        Val::Nodes(n) => n,
                        _ => return Err(XsltError::new("XPTY0004", "path step on a non-node-set")),
                    },
                };
                for st in steps {
                    let mut next = Vec::new();
                    for &n in &cur {
                        let cand: Vec<usize> = self
                            .axis_nodes(n, st.axis)
                            .into_iter()
                            .filter(|&x| self.test(x, &st.test, st.axis))
                            .collect();
                        next.extend(self.filter(cand, &st.preds)?);
                    }
                    next.sort_unstable();
                    next.dedup();
                    cur = next;
                }
                Val::Nodes(cur)
            }
            Expr::Func(name, args) => self.call(name, args, c)?,
        })
    }

    fn compare(&self, op: &str, a: &Val, b: &Val) -> bool {
        let cmp_atoms = |x: &Val, y: &Val| -> bool {
            if matches!(op, "=" | "!=") {
                let eq = match (x, y) {
                    (Val::Bool(_), _) | (_, Val::Bool(_)) => self.boolean(x) == self.boolean(y),
                    (Val::Num(_), _) | (_, Val::Num(_)) => self.number(x) == self.number(y),
                    _ => self.string(x) == self.string(y),
                };
                if op == "=" { eq } else { !eq }
            } else {
                let (p, q) = (self.number(x), self.number(y));
                match op {
                    "<" => p < q,
                    "<=" => p <= q,
                    ">" => p > q,
                    _ => p >= q,
                }
            }
        };
        let atoms = |v: &Val| -> Vec<Val> {
            match v {
                Val::Nodes(n) => n.iter().map(|&i| Val::Str(self.src.string_value(i))).collect(),
                other => vec![other.clone()],
            }
        };
        match (a, b) {
            (Val::Nodes(_), Val::Bool(_)) | (Val::Bool(_), Val::Nodes(_)) => {
                cmp_atoms(&Val::Bool(self.boolean(a)), &Val::Bool(self.boolean(b)))
            }
            (Val::Nodes(_), _) | (_, Val::Nodes(_)) => {
                let (xa, xb) = (atoms(a), atoms(b));
                xa.iter().any(|x| {
                    xb.iter().any(|y| {
                        let y = match (x, y) {
                            (Val::Str(_), Val::Num(_)) => y.clone(),
                            _ => y.clone(),
                        };
                        cmp_atoms(x, &y)
                    })
                })
            }
            _ => cmp_atoms(a, b),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], c: &Ctx) -> Result<Val, XsltError> {
        let mut v = Vec::with_capacity(args.len());
        for a in args {
            v.push(self.eval(a, c)?);
        }
        let s = |i: usize, me: &Self| -> String {
            v.get(i).map(|x| me.string(x)).unwrap_or_else(|| me.src.string_value(c.node))
        };
        let n = |i: usize, me: &Self| -> f64 { v.get(i).map(|x| me.number(x)).unwrap_or(f64::NAN) };
        Ok(match name {
            "position" => Val::Num(c.pos as f64),
            "last" => Val::Num(c.size as f64),
            "count" => match v.first() {
                Some(Val::Nodes(n)) => Val::Num(n.len() as f64),
                _ => return Err(XsltError::new("XPTY0004", "count() of a non-node-set")),
            },
            "string" => Val::Str(s(0, self)),
            "number" => Val::Num(
                v.first()
                    .map(|x| self.number(x))
                    .unwrap_or_else(|| str_to_num(&self.src.string_value(c.node))),
            ),
            "boolean" => Val::Bool(v.first().is_some_and(|x| self.boolean(x))),
            "not" => Val::Bool(!v.first().is_some_and(|x| self.boolean(x))),
            "true" => Val::Bool(true),
            "false" => Val::Bool(false),
            "concat" => Val::Str(v.iter().map(|x| self.string(x)).collect()),
            "string-length" => Val::Num(s(0, self).chars().count() as f64),
            "normalize-space" => Val::Str(s(0, self).split_whitespace().collect::<Vec<_>>().join(" ")),
            "contains" => Val::Bool(s(0, self).contains(&s(1, self))),
            "starts-with" => Val::Bool(s(0, self).starts_with(&s(1, self))),
            "ends-with" => Val::Bool(s(0, self).ends_with(&s(1, self))),
            "substring-before" => {
                let (a, b) = (s(0, self), s(1, self));
                Val::Str(a.find(&b).map(|i| a[..i].to_string()).unwrap_or_default())
            }
            "substring-after" => {
                let (a, b) = (s(0, self), s(1, self));
                Val::Str(a.find(&b).map(|i| a[i + b.len()..].to_string()).unwrap_or_default())
            }
            "substring" => {
                let a: Vec<char> = s(0, self).chars().collect();
                let start = n(1, self).round();
                let end = if v.len() > 2 { start + n(2, self).round() } else { f64::INFINITY };
                Val::Str(
                    a.iter()
                        .enumerate()
                        .filter(|(i, _)| {
                            let p = (*i + 1) as f64;
                            p >= start && p < end
                        })
                        .map(|(_, ch)| *ch)
                        .collect(),
                )
            }
            "translate" => {
                let (a, from, to) = (s(0, self), s(1, self), s(2, self));
                let (from, to): (Vec<char>, Vec<char>) = (from.chars().collect(), to.chars().collect());
                Val::Str(
                    a.chars()
                        .filter_map(|ch| match from.iter().position(|&f| f == ch) {
                            Some(i) => to.get(i).copied(),
                            None => Some(ch),
                        })
                        .collect(),
                )
            }
            "upper-case" => Val::Str(s(0, self).to_uppercase()),
            "lower-case" => Val::Str(s(0, self).to_lowercase()),
            "floor" => Val::Num(n(0, self).floor()),
            "ceiling" => Val::Num(n(0, self).ceil()),
            "round" => Val::Num((n(0, self) + 0.5).floor()),
            "sum" => match v.first() {
                Some(Val::Nodes(ns)) => {
                    Val::Num(ns.iter().map(|&i| str_to_num(&self.src.string_value(i))).sum())
                }
                _ => return Err(XsltError::new("XPTY0004", "sum() of a non-node-set")),
            },
            "name" | "local-name" => {
                let node = match v.first() {
                    Some(Val::Nodes(ns)) => ns.first().copied(),
                    Some(_) => return Err(XsltError::new("XPTY0004", "name() of a non-node-set")),
                    None => Some(c.node),
                };
                Val::Str(match node.map(|i| &self.src.node(i).kind) {
                    Some(NodeKind::Element { local, prefix, .. }) if name == "name" && !prefix.is_empty() => {
                        format!("{prefix}:{local}")
                    }
                    Some(NodeKind::Element { local, .. }) | Some(NodeKind::Attribute { local, .. }) => {
                        local.clone()
                    }
                    _ => String::new(),
                })
            }
            other => return Err(XsltError::new("XPST0017", format!("unknown function {other}()"))),
        })
    }

    // --- Templates --------------------------------------------------------

    fn match_step(&mut self, n: usize, st: &PatternStep) -> Result<bool, XsltError> {
        let axis = if st.attribute { Axis::Attribute } else { Axis::Child };
        let is_attr = matches!(self.src.node(n).kind, NodeKind::Attribute { .. });
        if is_attr != st.attribute || matches!(self.src.node(n).kind, NodeKind::Root) {
            return Ok(false);
        }
        if !self.test(n, &st.test, axis) {
            return Ok(false);
        }
        if st.preds.is_empty() {
            return Ok(true);
        }
        let Some(p) = self.src.node(n).parent else { return Ok(false) };
        let sibs: Vec<usize> =
            self.axis_nodes(p, axis).into_iter().filter(|&x| self.test(x, &st.test, axis)).collect();
        Ok(self.filter(sibs, &st.preds)?.contains(&n))
    }

    fn match_steps(&mut self, n: usize, steps: &[PatternStep], absolute: bool) -> Result<bool, XsltError> {
        let Some((last, rest)) = steps.split_last() else { return Ok(false) };
        if !self.match_step(n, last)? {
            return Ok(false);
        }
        let parent = self.src.node(n).parent;
        if rest.is_empty() {
            return Ok(if absolute { parent == Some(0) } else { true });
        }
        if last.descendant {
            let mut p = parent;
            while let Some(x) = p {
                if self.match_steps(x, rest, absolute)? {
                    return Ok(true);
                }
                p = self.src.node(x).parent;
            }
            Ok(false)
        } else {
            match parent {
                Some(x) => self.match_steps(x, rest, absolute),
                None => Ok(false),
            }
        }
    }

    fn matches(&mut self, n: usize, p: &Pattern) -> Result<bool, XsltError> {
        match &p.steps {
            None => Ok(n == 0),
            Some(steps) => {
                let steps = steps.clone();
                self.match_steps(n, &steps, p.absolute)
            }
        }
    }

    fn find_template(&mut self, n: usize, mode: Option<&str>) -> Result<Option<usize>, XsltError> {
        let mut best: Option<(f64, usize)> = None;
        for (i, t) in self.sheet.templates.iter().enumerate() {
            if t.mode.as_deref() != mode {
                continue;
            }
            let Some(p) = &t.pattern else { continue };
            if !self.matches(n, p)? {
                continue;
            }
            if best.is_none_or(|(bp, _)| t.priority >= bp) {
                best = Some((t.priority, i));
            }
        }
        Ok(best.map(|(_, i)| i))
    }

    fn apply(
        &mut self,
        nodes: &[usize],
        mode: Option<&str>,
        out: &mut Out,
        params: &HashMap<String, Val>,
    ) -> Result<(), XsltError> {
        self.depth += 1;
        if self.depth > 500 {
            return Err(XsltError::new("XTDE0560", "Too deep recursion"));
        }
        let size = nodes.len();
        for (i, &n) in nodes.iter().enumerate() {
            let ctx = Ctx { node: n, pos: i + 1, size };
            match self.find_template(n, mode)? {
                Some(t) => {
                    let body = self.sheet.templates[t].node;
                    self.run_template(body, &ctx, out, params)?;
                }
                None => match &self.src.node(n).kind {
                    NodeKind::Root | NodeKind::Element { .. } => {
                        let kids = self.src.node(n).children.clone();
                        self.apply(&kids, mode, out, &HashMap::new())?;
                    }
                    NodeKind::Text(t) => out.text(t),
                    NodeKind::Attribute { value, .. } => out.text(value),
                    NodeKind::Comment(_) => {}
                },
            }
        }
        self.depth -= 1;
        Ok(())
    }

    fn run_template(
        &mut self,
        body: usize,
        ctx: &Ctx,
        out: &mut Out,
        params: &HashMap<String, Val>,
    ) -> Result<(), XsltError> {
        self.vars.push(HashMap::new());
        let kids = self.sdoc().node(body).children.clone();
        for &k in &kids {
            if self.sdoc().is_element(k, XSL_NS, "param") {
                let name = self.sdoc().attr(k, "name").unwrap_or("").to_string();
                match params.get(&name) {
                    Some(v) => {
                        self.vars.last_mut().expect("scope").insert(name, v.clone());
                    }
                    None => self.declare(k, ctx)?,
                }
            }
        }
        let r = self.seq(&kids, ctx, out);
        self.vars.pop();
        r
    }

    fn declare(&mut self, el: usize, ctx: &Ctx) -> Result<(), XsltError> {
        let name = self.sdoc().attr(el, "name").unwrap_or("").to_string();
        let v = match self.sdoc().attr(el, "select") {
            Some(sel) => {
                let e = self.sheet.expr(el, sel)?;
                self.eval(&e, ctx)?
            }
            None => {
                let mut o = Out::new();
                let kids = self.sdoc().node(el).children.clone();
                self.seq(&kids, ctx, &mut o)?;
                let mut s = String::new();
                fn text_of(n: &[RNode], s: &mut String) {
                    for x in n {
                        match x {
                            RNode::Text(t) => s.push_str(t),
                            RNode::Elem { children, .. } => text_of(children, s),
                            RNode::Comment(_) => {}
                        }
                    }
                }
                text_of(&o.into_children(), &mut s);
                Val::Str(s)
            }
        };
        self.vars.last_mut().expect("scope").insert(name, v);
        Ok(())
    }

    fn avt(&mut self, el: usize, s: &str, ctx: &Ctx) -> Result<String, XsltError> {
        let mut out = String::new();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '{' if chars.get(i + 1) == Some(&'{') => {
                    out.push('{');
                    i += 2;
                }
                '}' if chars.get(i + 1) == Some(&'}') => {
                    out.push('}');
                    i += 2;
                }
                '{' => {
                    let st = i + 1;
                    let mut j = st;
                    while j < chars.len() && chars[j] != '}' {
                        j += 1;
                    }
                    let src: String = chars[st..j].iter().collect();
                    let e = self.sheet.expr(el, &src)?;
                    let v = self.eval(&e, ctx)?;
                    out.push_str(&self.string(&v));
                    i = j + 1;
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        Ok(out)
    }

    fn select(&mut self, el: usize, ctx: &Ctx, default_children: bool) -> Result<Vec<usize>, XsltError> {
        match self.sdoc().attr(el, "select").map(str::to_string) {
            Some(sel) => {
                let e = self.sheet.expr(el, &sel)?;
                match self.eval(&e, ctx)? {
                    Val::Nodes(n) => Ok(n),
                    _ => Err(XsltError::new("XTTE0520", format!("'{sel}' does not select nodes"))),
                }
            }
            None if default_children => Ok(self.src.node(ctx.node).children.clone()),
            None => Ok(vec![]),
        }
    }

    fn with_params(&mut self, el: usize, ctx: &Ctx) -> Result<HashMap<String, Val>, XsltError> {
        let mut params = HashMap::new();
        let kids = self.sdoc().node(el).children.clone();
        for k in kids {
            if self.sdoc().is_element(k, XSL_NS, "with-param") {
                self.vars.push(HashMap::new());
                self.declare(k, ctx)?;
                let scope = self.vars.pop().expect("scope");
                params.extend(scope);
            }
        }
        Ok(params)
    }

    fn seq(&mut self, kids: &[usize], ctx: &Ctx, out: &mut Out) -> Result<(), XsltError> {
        for &k in kids {
            self.instruction(k, ctx, out)?;
        }
        Ok(())
    }

    fn copy_node(&self, n: usize, out: &mut Out) {
        match &self.src.node(n).kind {
            NodeKind::Root => {
                for &c in &self.src.node(n).children {
                    self.copy_node(c, out);
                }
            }
            NodeKind::Element { ns, local, .. } => {
                out.open(local.clone(), ns.clone());
                for &a in &self.src.node(n).attrs {
                    if let NodeKind::Attribute { local, value, .. } = &self.src.node(a).kind {
                        out.attr(local.clone(), value.clone());
                    }
                }
                for &c in &self.src.node(n).children {
                    self.copy_node(c, out);
                }
                out.close();
            }
            NodeKind::Attribute { local, value, .. } => out.attr(local.clone(), value.clone()),
            NodeKind::Text(t) => out.text(t),
            NodeKind::Comment(c) => out.push(RNode::Comment(c.clone())),
        }
    }

    fn instruction(&mut self, k: usize, ctx: &Ctx, out: &mut Out) -> Result<(), XsltError> {
        let node = self.sdoc().node(k).clone();
        match &node.kind {
            NodeKind::Text(t) => {
                out.text(t);
                Ok(())
            }
            NodeKind::Comment(_) | NodeKind::Root | NodeKind::Attribute { .. } => Ok(()),
            NodeKind::Element { ns, local, prefix } if ns != XSL_NS => {
                let name = if ns == XHTML_NS || prefix.is_empty() {
                    local.clone()
                } else {
                    format!("{prefix}:{local}")
                };
                out.open(name, ns.clone());
                for &a in &node.attrs {
                    if let NodeKind::Attribute { ns: ans, local: al, value } = &self.sdoc().node(a).kind {
                        if ans == XSL_NS {
                            continue;
                        }
                        let v = self.avt(k, value, ctx)?;
                        out.attr(al.clone(), v);
                    }
                }
                self.vars.push(HashMap::new());
                let r = self.seq(&node.children, ctx, out);
                self.vars.pop();
                r?;
                out.close();
                Ok(())
            }
            NodeKind::Element { local, .. } => match local.as_str() {
                "apply-templates" => {
                    let nodes = self.select(k, ctx, true)?;
                    let mode = self.sdoc().attr(k, "mode").map(str::to_string);
                    let params = self.with_params(k, ctx)?;
                    self.apply(&nodes, mode.as_deref(), out, &params)
                }
                "call-template" => {
                    let name = self.sdoc().attr(k, "name").unwrap_or("").to_string();
                    let t = self
                        .sheet
                        .templates
                        .iter()
                        .rev()
                        .find(|t| t.name.as_deref() == Some(name.as_str()))
                        .map(|t| t.node)
                        .ok_or_else(|| XsltError::new("XTSE0650", format!("No template named {name}")))?;
                    let params = self.with_params(k, ctx)?;
                    self.run_template(t, ctx, out, &params)
                }
                "value-of" => {
                    let sel = self.sdoc().attr(k, "select").unwrap_or(".").to_string();
                    let e = self.sheet.expr(k, &sel)?;
                    let v = self.eval(&e, ctx)?;
                    let s = self.string(&v);
                    out.text(&s);
                    Ok(())
                }
                "text" => {
                    let t = self.sdoc().string_value(k);
                    out.text(&t);
                    Ok(())
                }
                "if" => {
                    let test = self.sdoc().attr(k, "test").unwrap_or("false()").to_string();
                    let e = self.sheet.expr(k, &test)?;
                    let v = self.eval(&e, ctx)?;
                    if self.boolean(&v) {
                        self.vars.push(HashMap::new());
                        let r = self.seq(&node.children, ctx, out);
                        self.vars.pop();
                        r?;
                    }
                    Ok(())
                }
                "choose" => {
                    for &w in &node.children {
                        let take = if self.sdoc().is_element(w, XSL_NS, "when") {
                            let test = self.sdoc().attr(w, "test").unwrap_or("false()").to_string();
                            let e = self.sheet.expr(w, &test)?;
                            let v = self.eval(&e, ctx)?;
                            self.boolean(&v)
                        } else {
                            self.sdoc().is_element(w, XSL_NS, "otherwise")
                        };
                        if take {
                            let kids = self.sdoc().node(w).children.clone();
                            self.vars.push(HashMap::new());
                            let r = self.seq(&kids, ctx, out);
                            self.vars.pop();
                            return r;
                        }
                    }
                    Ok(())
                }
                "for-each" => {
                    let nodes = self.select(k, ctx, false)?;
                    let size = nodes.len();
                    for (i, n) in nodes.into_iter().enumerate() {
                        let c = Ctx { node: n, pos: i + 1, size };
                        self.vars.push(HashMap::new());
                        let r = self.seq(&node.children, &c, out);
                        self.vars.pop();
                        r?;
                    }
                    Ok(())
                }
                "attribute" => {
                    let name = self.sdoc().attr(k, "name").unwrap_or("").to_string();
                    let name = self.avt(k, &name, ctx)?;
                    let mut o = Out::new();
                    self.seq(&node.children, ctx, &mut o)?;
                    let mut v = String::new();
                    for c in o.into_children() {
                        if let RNode::Text(t) = c {
                            v.push_str(&t);
                        }
                    }
                    out.attr(name, v);
                    Ok(())
                }
                "element" => {
                    let name = self.sdoc().attr(k, "name").unwrap_or("").to_string();
                    let name = self.avt(k, &name, ctx)?;
                    out.open(name, String::new());
                    self.seq(&node.children, ctx, out)?;
                    out.close();
                    Ok(())
                }
                "comment" => {
                    let mut o = Out::new();
                    self.seq(&node.children, ctx, &mut o)?;
                    let mut v = String::new();
                    for c in o.into_children() {
                        if let RNode::Text(t) = c {
                            v.push_str(&t);
                        }
                    }
                    out.push(RNode::Comment(v));
                    Ok(())
                }
                "copy" => {
                    match self.src.node(ctx.node).kind.clone() {
                        NodeKind::Element { ns, local, .. } => {
                            out.open(local, ns);
                            self.seq(&node.children, ctx, out)?;
                            out.close();
                        }
                        NodeKind::Root => self.seq(&node.children, ctx, out)?,
                        _ => self.copy_node(ctx.node, out),
                    }
                    Ok(())
                }
                "copy-of" => {
                    let sel = self.sdoc().attr(k, "select").unwrap_or(".").to_string();
                    let e = self.sheet.expr(k, &sel)?;
                    match self.eval(&e, ctx)? {
                        Val::Nodes(ns) => {
                            for n in ns {
                                self.copy_node(n, out);
                            }
                        }
                        v => {
                            let s = self.string(&v);
                            out.text(&s);
                        }
                    }
                    Ok(())
                }
                "variable" => self.declare(k, ctx),
                "param" | "sort" | "number" | "fallback" | "message" => Ok(()),
                other => Err(XsltError::new("XTSE0010", format!("Unsupported instruction xsl:{other}"))),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<outline xmlns="http://wkhtmltopdf.org/outline">
  <item title="Doc" page="0" link="__WKANCHOR_0" backLink="__WKANCHOR_1">
    <item title="A &amp; B" page="1" link="__WKANCHOR_2" backLink="__WKANCHOR_3">
      <item title="A.1" page="2" link="__WKANCHOR_4" backLink="__WKANCHOR_5"/>
    </item>
    <item title="" page="3" link="__WKANCHOR_6" backLink="__WKANCHOR_7"/>
  </item>
</outline>
"#;

    #[test]
    fn default_style_sheet_shape() {
        let xsl = crate::tocstylesheet::dump_default_toc_style_sheet(&wk_settings::TableOfContent::default());
        let s = Stylesheet::parse(&xsl).unwrap();
        let html = s.transform(XML).unwrap();
        assert!(html.contains("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Strict//EN\""));
        assert!(html.contains("<h1>Table of Contents</h1>"));
        assert!(html.contains(
            "<li><div><a href=\"__WKANCHOR_2\" name=\"__WKANCHOR_3\">A &amp; B</a><span>1</span></div><ul><!--added to prevent self-closing tags in QtXmlPatterns-->"
        ));
        // The empty title has no div, but its list is still there.
        assert!(html.contains("<li><ul><!--added"));
    }

    #[test]
    fn custom_templates_avt_and_empty_elements() {
        let xsl = r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            xmlns:o="http://wkhtmltopdf.org/outline">
          <xsl:template match="o:outline"><ol><xsl:apply-templates select="o:item/o:item"/></ol></xsl:template>
          <xsl:template match="o:item">
            <li><xsl:if test="@title!=''"><a href="{@link}">E: <xsl:value-of select="@title"/></a> p.<xsl:value-of select="@page + 1"/></xsl:if>
            <ol><xsl:apply-templates select="o:item"/></ol></li>
          </xsl:template>
        </xsl:stylesheet>"#;
        let html = Stylesheet::parse(xsl).unwrap().transform(XML).unwrap();
        assert!(html.contains("<li><a href=\"__WKANCHOR_2\">E: A &amp; B</a> p.2<ol><li>"), "{html}");
        assert!(html.contains("<ol/>"), "{html}");
    }

    #[test]
    fn xpath_functions() {
        let xsl = r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
            xmlns:o="http://wkhtmltopdf.org/outline">
          <xsl:template match="/"><r><xsl:value-of select="count(//o:item)"/>|<xsl:value-of
            select="concat(substring('abcdef', 2, 3), '-', string-length(//o:item[2]/@title))"/>|<xsl:for-each
            select="//o:item[@page &gt; 1]"><xsl:value-of select="position()"/>/<xsl:value-of select="last()"/>;</xsl:for-each>|<xsl:choose><xsl:when test="not(//o:item[@title='zz'])">none</xsl:when><xsl:otherwise>some</xsl:otherwise></xsl:choose></r></xsl:template>
        </xsl:stylesheet>"#;
        let html = Stylesheet::parse(xsl).unwrap().transform(XML).unwrap();
        assert!(html.contains("<r>4|bcd-0|1/2;2/2;|none</r>"), "{html}");
    }
}
