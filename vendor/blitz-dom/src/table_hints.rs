//! wkhtmltopdf-rs: the presentational attributes of tables as QtWebKit 2.2
//! (WebKit 534.34) maps them (`HTMLTableElement`, `HTMLTablePartElement`,
//! `HTMLTableCellElement`, `HTMLTableCaptionElement`).
//!
//! The table's `border`, `frame`, `rules`, `cellpadding`, `cellspacing` and
//! `bordercolor` attributes style the table itself and, through WebKit's
//! "shared cell declarations", every cell (and, for `rules=groups`, the row
//! groups and column groups). The hints are produced as CSS declaration text,
//! parsed once per distinct text and cached.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;

use style::context::QuirksMode;
use style::properties::PropertyDeclarationBlock;
use style::properties::declaration_block::parse_style_attribute;
use style::servo_arc::Arc as ServoArc;
use style::shared_lock::{Locked, SharedRwLock};
use style::stylesheets::{CssRuleType, UrlExtraData};

use crate::node::{Node, NodeData};

/// The leading integer of an attribute value, as WTF's `String::toInt` reads
/// it for these attributes (leading whitespace and sign allowed).
fn leading_int(value: &str) -> Option<i32> {
    let v = value.trim_start();
    let (neg, digits) = match v.as_bytes().first() {
        Some(b'-') => (true, &v[1..]),
        Some(b'+') => (false, &v[1..]),
        _ => (false, v),
    };
    let end = digits.bytes().take_while(u8::is_ascii_digit).count();
    if end == 0 {
        return None;
    }
    let n: i64 = digits[..end].parse().ok()?;
    let n = n.min(i32::MAX as i64) as i32;
    Some(if neg { -n } else { n })
}

/// A length attribute as WebKit's `addCSSLength` maps it: the leading number,
/// in px unless followed by `%`.
fn css_length(value: &str) -> Option<String> {
    let v = value.trim();
    let end = v.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
    if end == 0 {
        return None;
    }
    let n: f32 = v[..end].parse().ok()?;
    Some(if v[end..].starts_with('%') { format!("{n}%") } else { format!("{n}px") })
}

/// A colour attribute (`addCSSColor`): a CSS colour, else the legacy HTML colour.
fn css_color(value: &str) -> Option<String> {
    let v = value.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("transparent") {
        return None;
    }
    let c = style::servo::attr::parse_legacy_color(v).ok()?;
    let c = c.into_srgb_legacy();
    let [r, g, b] = [c.components.0, c.components.1, c.components.2].map(|x| (x * 255.0).round() as u8);
    Some(format!("rgb({r},{g},{b})"))
}

#[derive(Clone, Copy, PartialEq)]
enum Rules {
    Unset,
    None,
    Groups,
    Rows,
    Cols,
    All,
}

/// What a `<table>`'s attributes say (the members of WebKit's `HTMLTableElement`).
struct TableAttrs {
    border: i32,
    border_color: bool,
    frame: bool,
    rules: Rules,
    padding: i32,
}

fn attr<'a>(node: &'a Node, name: &str) -> Option<&'a str> {
    match &node.data {
        NodeData::Element(e) => e.attrs().iter().find(|a| &*a.name.local == name).map(|a| a.value.as_str()),
        _ => None,
    }
}

fn element_tag(node: &Node) -> Option<&str> {
    match &node.data {
        NodeData::Element(e) if e.name.ns == markup5ever::ns!(html) => Some(&e.name.local),
        _ => None,
    }
}

fn table_attrs(table: &Node) -> TableAttrs {
    let mut t = TableAttrs { border: 0, border_color: false, frame: false, rules: Rules::Unset, padding: 1 };
    if let Some(v) = attr(table, "border") {
        t.border = if v.is_empty() { 1 } else { leading_int(v).unwrap_or(0) };
    }
    if let Some(v) = attr(table, "bordercolor") {
        t.border_color = css_color(v).is_some();
    }
    if let Some(v) = attr(table, "frame") {
        t.frame = frame_sides(v).is_some();
    }
    if let Some(v) = attr(table, "rules") {
        t.rules = match v.to_ascii_lowercase().as_str() {
            "none" => Rules::None,
            "groups" => Rules::Groups,
            "rows" => Rules::Rows,
            "cols" => Rules::Cols,
            "all" => Rules::All,
            _ => Rules::Unset,
        };
    }
    if let Some(v) = attr(table, "cellpadding") {
        t.padding = if v.is_empty() { 1 } else { leading_int(v).unwrap_or(0).max(0) };
    }
    t
}

/// The sides (top, right, bottom, left) a valid `frame` value draws.
fn frame_sides(value: &str) -> Option<[bool; 4]> {
    Some(match value.to_ascii_lowercase().as_str() {
        "void" => [false; 4],
        "above" => [true, false, false, false],
        "below" => [false, false, true, false],
        "hsides" => [true, false, true, false],
        "lhs" => [false, false, false, true],
        "rhs" => [false, true, false, false],
        "vsides" => [false, true, false, true],
        "box" | "border" => [true; 4],
        _ => return None,
    })
}

/// The `<table>` a table part belongs to: the parent of a row group, column
/// group or caption, the grandparent (or great-grandparent) of a cell.
fn find_table<'a>(node: &'a Node, tag: &str) -> Option<&'a Node> {
    let parent = |n: &'a Node| n.parent.map(|id| n.with(id));
    let is = |n: &Node, t: &str| element_tag(n) == Some(t);
    match tag {
        "td" | "th" => {
            let row = parent(node).filter(|n| is(n, "tr"))?;
            let up = parent(row)?;
            if is(up, "table") {
                return Some(up);
            }
            if matches!(element_tag(up), Some("tbody" | "thead" | "tfoot")) {
                return parent(up).filter(|n| is(n, "table"));
            }
            None
        }
        "tbody" | "thead" | "tfoot" | "colgroup" => parent(node).filter(|n| is(n, "table")),
        _ => None,
    }
}

/// The CSS declarations WebKit maps from the attributes of `node` (an HTML
/// table element or table part), in the order WebKit applies them. Empty when
/// there is nothing to map.
fn hints_css(node: &Node, tag: &str) -> String {
    let mut css = String::new();
    match tag {
        "table" => {
            let t = table_attrs(node);
            if let Some(v) = attr(node, "border") {
                let _ = v;
                let _ = write!(css, "border-width:{}px;", t.border.max(0));
            }
            if let Some(c) = attr(node, "bordercolor").and_then(css_color) {
                let _ = write!(css, "border-color:{c};");
            }
            if let Some(v) = attr(node, "cellspacing").and_then(css_length) {
                if !v.ends_with('%') {
                    let _ = write!(css, "border-spacing:{v};");
                }
            }
            if let Some(v) = attr(node, "align") {
                if v.eq_ignore_ascii_case("center") {
                    css.push_str("margin-left:auto;margin-right:auto;");
                } else if v.eq_ignore_ascii_case("left") {
                    css.push_str("float:left;");
                } else if v.eq_ignore_ascii_case("right") {
                    css.push_str("float:right;");
                }
            }
            if let Some(sides) = attr(node, "frame").and_then(frame_sides) {
                css.push_str("border-width:1px;");
                for (side, on) in ["top", "right", "bottom", "left"].iter().zip(sides) {
                    let _ = write!(css, "border-{side}-style:{};", if on { "solid" } else { "hidden" });
                }
            }
            if t.rules != Rules::Unset {
                css.push_str("border-collapse:collapse;");
            }
            // additionalAttributeStyleDecls
            if (t.border != 0 || t.border_color) && !t.frame {
                let style = if t.border_color { "solid" } else { "outset" };
                let _ = write!(css, "border-style:{style};");
            }
        }
        "td" | "th" => {
            part_hints(node, &mut css);
            if let Some(v) = attr(node, "nowrap") {
                let _ = v;
                css.push_str("white-space:nowrap;");
            }
            if let Some(table) = find_table(node, tag) {
                let t = table_attrs(table);
                let borders = match t.rules {
                    Rules::None | Rules::Groups => "border-width:0;",
                    Rules::All => "border-width:1px;border-style:solid;border-color:inherit;",
                    Rules::Cols => {
                        "border-left-width:1px;border-right-width:1px;border-left-style:solid;\
                         border-right-style:solid;border-color:inherit;"
                    }
                    Rules::Rows => {
                        "border-top-width:1px;border-bottom-width:1px;border-top-style:solid;\
                         border-bottom-style:solid;border-color:inherit;"
                    }
                    Rules::Unset if t.border == 0 => "border-width:0;",
                    Rules::Unset if t.border_color => "border-width:1px;border-style:solid;border-color:inherit;",
                    Rules::Unset => "border-width:1px;border-style:inset;border-color:inherit;",
                };
                css.push_str(borders);
                if t.padding > 0 {
                    let _ = write!(css, "padding:{}px;", t.padding);
                } else {
                    css.push_str("padding:0;");
                }
            }
        }
        "tr" | "tbody" | "thead" | "tfoot" | "col" | "colgroup" => {
            part_hints(node, &mut css);
            if tag != "tr" && tag != "col" {
                if let Some(table) = find_table(node, tag) {
                    if table_attrs(table).rules == Rules::Groups {
                        css.push_str(if tag == "colgroup" {
                            "border-left-width:1px;border-right-width:1px;border-left-style:solid;border-right-style:solid;"
                        } else {
                            "border-top-width:1px;border-bottom-width:1px;border-top-style:solid;border-bottom-style:solid;"
                        });
                    }
                }
            }
        }
        "caption" => {
            if let Some(v) = attr(node, "align") {
                if v.eq_ignore_ascii_case("top") || v.eq_ignore_ascii_case("bottom") {
                    let _ = write!(css, "caption-side:{};", v.to_ascii_lowercase());
                }
            }
        }
        _ => {}
    }
    css
}

/// `HTMLTablePartElement`: `bordercolor` and `valign` (`align`, `bgcolor`,
/// `width` and `height` are mapped with the other elements').
fn part_hints(node: &Node, css: &mut String) {
    if let Some(c) = attr(node, "bordercolor").and_then(css_color) {
        let _ = write!(css, "border-color:{c};border-style:solid;");
    }
    if let Some(v) = attr(node, "valign") {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline" | "text-top" | "text-bottom" | "sub" | "super") {
            let _ = write!(css, "vertical-align:{v};");
        }
    }
}

thread_local! {
    /// The parsed hints, per document lock and declaration text. The rule tree
    /// keys its nodes by block, so cells with the same hints share one block
    /// (and their rule node, and the style sharing cache).
    static CACHE: RefCell<HashMap<(usize, String), ServoArc<Locked<PropertyDeclarationBlock>>>> =
        RefCell::new(HashMap::new());
}

/// Whether `align` on this element is mapped here rather than to the generic
/// `text-align` of block elements.
pub(crate) fn handles_align(tag: &str) -> bool {
    matches!(tag, "table" | "caption" | "td" | "th" | "tr" | "tbody" | "thead" | "tfoot" | "col" | "colgroup")
}

/// The declaration block of WebKit's table attribute mapping for `node`,
/// locked with the document's `lock`.
pub(crate) fn table_hints(
    node: &Node,
    lock: &SharedRwLock,
) -> Option<ServoArc<Locked<PropertyDeclarationBlock>>> {
    let tag = element_tag(node)?;
    if !handles_align(tag) {
        return None;
    }
    let css = hints_css(node, tag);
    if css.is_empty() {
        return None;
    }
    let key = (lock.id(), css);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(block) = cache.get(&key) {
            return Some(block.clone());
        }
        let url = UrlExtraData(ServoArc::new(url::Url::parse("about:blank").unwrap()));
        let block = parse_style_attribute(&key.1, &url, None, QuirksMode::NoQuirks, CssRuleType::Style);
        let block = ServoArc::new(lock.wrap(block));
        cache.insert(key, block.clone());
        Some(block)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_values_parse_like_webkit() {
        assert_eq!(leading_int(" 3px"), Some(3));
        assert_eq!(leading_int("x"), None);
        assert_eq!(css_length("5"), Some("5px".into()));
        assert_eq!(css_length("10%"), Some("10%".into()));
        assert_eq!(css_color("red"), Some("rgb(255,0,0)".into()));
        assert_eq!(frame_sides("HSIDES"), Some([true, false, true, false]));
    }
}
