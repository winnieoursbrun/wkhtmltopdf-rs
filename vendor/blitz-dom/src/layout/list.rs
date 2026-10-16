use blitz_traits::node_id::NodeId;
use markup5ever::local_name;
use parley::FontFamily;
use style::computed_values::list_style_type::T as ListStyleType;
use style::{
    computed_values::list_style_position::T as ListStylePosition, counter_style::CounterStyle,
};

use crate::{
    BaseDocument,
    node::{ListItemLayout, ListItemLayoutPosition, Marker},
    stylo_to_parley,
};

pub(super) const BULLET_FONT_FAMILY: &str = "Bullet, monospace, sans-serif";

pub(super) fn collect_list_item_children(
    doc: &mut BaseDocument,
    index: &mut usize,
    reversed: bool,
    node_id: NodeId,
) {
    let mut children = doc.nodes[node_id].children.clone();
    if reversed {
        children.reverse();
    }
    for child in children.into_iter() {
        // wkhtmltopdf-rs: `<li value>` renumbers the item and the ones after it.
        if let Some(value) = doc.nodes[child]
            .element_data()
            .filter(|e| e.name.local == local_name!("li"))
            .and_then(|e| e.attr_parsed::<usize>(local_name!("value")))
            .filter(|v| *v >= 1)
        {
            *index = value - 1;
        }
        if let Some(layout) = node_list_item_child(doc, child, *index) {
            let node = &mut doc.nodes[child];
            node.element_data_mut().unwrap().list_item_data = Some(Box::new(layout));
            *index += 1;
            collect_list_item_children(doc, index, reversed, child);
        } else {
            // Unset marker in case it was previously set
            let node = &mut doc.nodes[child];
            if let Some(element_data) = node.element_data_mut() {
                element_data.list_item_data = None;
            }
        }
    }
}

// Return a child node which is of display: list-item
fn node_list_item_child(
    doc: &mut BaseDocument,
    child_id: NodeId,
    index: usize,
) -> Option<ListItemLayout> {
    let node = &doc.nodes[child_id];

    // We only care about elements with display: list-item (li's have this automatically)
    if !node
        .primary_styles()
        .is_some_and(|style| style.get_box().display.is_list_item())
    {
        return None;
    }

    //Break on container elements when already in a list
    if node
        .element_data()
        .map(|element_data| {
            matches!(
                element_data.name.local,
                local_name!("ol") | local_name!("ul"),
            )
        })
        .unwrap_or(false)
    {
        return None;
    };

    let styles = node.primary_styles().unwrap();
    let list_style_type = styles.clone_list_style_type();
    let list_style_position = styles.clone_list_style_position();
    let marker = marker_for_style(list_style_type.clone(), index)?;

    let position = match list_style_position {
        ListStylePosition::Inside => ListItemLayoutPosition::Inside,
        ListStylePosition::Outside => {
            let mut parley_style = stylo_to_parley::style(child_id, &styles);

            // wkhtmltopdf-rs: disc, circle and square are painted as shapes sized from the
            // item's own font (blitz-paint `draw_marker`), so their layout keeps that font.
            if let Some(font_family) = font_for_bullet_style(list_style_type)
                && !matches!(marker, Marker::Char('•' | '◦' | '▪'))
            {
                parley_style.font_family = font_family;
            }

            // Create a parley tree builder
            let mut font_ctx = doc.font_ctx.lock().unwrap();
            let mut builder = doc.layout_ctx.tree_builder(
                &mut font_ctx,
                doc.viewport.scale(),
                true,
                &parley_style,
            );

            // wkhtmltopdf-rs: the marker text, then its suffix, in the item's direction (a
            // Hebrew marker is `א. `, not reordered as right-to-left text).
            builder.set_base_direction(stylo_to_parley::base_direction(
                styles.clone_direction(),
                styles.clone_unicode_bidi(),
            ));
            match &marker {
                Marker::Char(char) => {
                    let mut buf = [0u8; 4];
                    builder.push_text(char.encode_utf8(&mut buf));
                }
                Marker::String(str) => builder.push_text(str),
            };

            let mut layout = builder.build().0;
            // wkhtmltopdf-rs: unbounded, a marker is one line (its max-content width rounds
            // words and spaces on their own, which can be narrower than the laid-out text).
            layout.break_all_lines(None);

            ListItemLayoutPosition::Outside(Box::new(layout))
        }
    };

    Some(ListItemLayout { marker, position })
}

// Determine the marker to render for a given list style type
fn marker_for_style(list_style_type: ListStyleType, index: usize) -> Option<Marker> {
    Some(match list_style_type.0 {
        CounterStyle::None => return None,
        CounterStyle::Name(name) => match &*name.0 {
            "lower-alpha" | "lower-latin" => {
                let mut marker = String::new();
                build_alpha_marker(index, &mut marker);
                Marker::String(format!("{marker}. "))
            }
            "upper-alpha" | "upper-latin" => {
                let mut marker = String::new();
                build_alpha_marker(index, &mut marker);
                Marker::String(format!("{}. ", marker.to_ascii_uppercase()))
            }
            "decimal" => Marker::String(format!("{}. ", index + 1)),
            // wkhtmltopdf-rs: the other numbering systems of WebKit 2012
            // (`RenderListMarker::listMarkerText`), each followed by `. `.
            name @ ("decimal-leading-zero" | "lower-roman" | "upper-roman" | "lower-greek"
            | "armenian" | "upper-armenian" | "lower-armenian" | "georgian" | "hebrew"
            | "cjk-ideographic") => {
                Marker::String(format!("{}. ", webkit_marker_text(name, index as i64 + 1)))
            }
            "disc" => Marker::Char('•'),
            "circle" => Marker::Char('◦'),
            "square" => Marker::Char('▪'),
            "disclosure-open" => Marker::Char('▾'),
            "disclosure-closed" => Marker::Char('▸'),
            _ => Marker::Char('□'),
        },
        CounterStyle::String(atom_string) => Marker::String(atom_string.as_ref().to_string()),
        CounterStyle::Symbols { symbols, .. } => {
            // Use symbols from counter-style definition
            let syms = &symbols.0;
            let len = syms.len();
            if len == 0 {
                Marker::Char('•')
            } else {
                // Cycle through symbols based on index: 1st, 2nd, 1st, 2nd, etc.
                let idx = if len > 1 { index % len } else { 0 };
                // Symbol is String or Ident variant, extract the string value
                let sym = &syms[idx];
                let marker_str = match sym {
                    style::counter_style::Symbol::String(s) => s.to_string(),
                    style::counter_style::Symbol::Ident(id) => id.0.to_string(),
                };
                Marker::String(marker_str)
            }
        }
    })
}

// Override the font to our specific bullet font when rendering bullets
fn font_for_bullet_style(list_style_type: ListStyleType) -> Option<FontFamily<'static>> {
    if list_style_type.0.is_bullet() {
        return Some(BULLET_FONT_FAMILY.into());
    }

    None
}

/// wkhtmltopdf-rs: marker text of `value` in the numbering systems WebKit 2012 implements in
/// `RenderListMarker.cpp` (out of range values fall back to decimal, as there).
fn webkit_marker_text(name: &str, value: i64) -> String {
    fn additive(value: i64, digits: &[&[char]]) -> String {
        // digits[k][d - 1] is digit d (1..=9) of the 10^k place.
        let mut out = String::new();
        let mut rest = value;
        let mut places = Vec::new();
        for _ in 0..digits.len() {
            places.push(rest % 10);
            rest /= 10;
        }
        for k in (0..digits.len()).rev() {
            let d = places[k];
            if d > 0 {
                out.push(digits[k][d as usize - 1]);
            }
        }
        out
    }
    match name {
        "decimal-leading-zero" if (-9..=9).contains(&value) => {
            if value < 0 { format!("-0{}", -value) } else { format!("0{value}") }
        }
        "lower-roman" | "upper-roman" if (1..=3999).contains(&value) => {
            const ROMAN: [(i64, &str); 13] = [
                (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
                (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
            ];
            let mut out = String::new();
            let mut rest = value;
            for (n, letters) in ROMAN {
                while rest >= n {
                    out.push_str(letters);
                    rest -= n;
                }
            }
            if name == "upper-roman" { out.to_ascii_uppercase() } else { out }
        }
        "lower-greek" if value >= 1 => {
            const GREEK: [char; 24] = [
                'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π',
                'ρ', 'σ', 'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
            ];
            let mut out = Vec::new();
            let mut n = value;
            while n > 0 {
                n -= 1;
                out.push(GREEK[(n % 24) as usize]);
                n /= 24;
            }
            out.iter().rev().collect()
        }
        "armenian" | "upper-armenian" | "lower-armenian" if (1..=9999).contains(&value) => {
            let base = if name == "lower-armenian" { 0x561 } else { 0x531 };
            let place = |k: u32| -> Vec<char> {
                (0..9).map(|d| char::from_u32(base + k * 9 + d).unwrap_or('?')).collect()
            };
            let (p0, p1, p2, p3) = (place(0), place(1), place(2), place(3));
            additive(value, &[&p0, &p1, &p2, &p3])
        }
        "georgian" if (1..=19999).contains(&value) => {
            const UNITS: [char; 9] = ['ა', 'ბ', 'გ', 'დ', 'ე', 'ვ', 'ზ', 'ჱ', 'თ'];
            const TENS: [char; 9] = ['ი', 'კ', 'ლ', 'მ', 'ნ', 'ჲ', 'ო', 'პ', 'ჟ'];
            const HUNDREDS: [char; 9] = ['რ', 'ს', 'ტ', 'ჳ', 'ფ', 'ქ', 'ღ', 'ყ', 'შ'];
            const THOUSANDS: [char; 9] = ['ჩ', 'ც', 'ძ', 'წ', 'ჭ', 'ხ', 'ჴ', 'ჯ', 'ჰ'];
            let mut out = String::new();
            if value >= 10000 {
                out.push('ჵ');
            }
            out + &additive(value % 10000, &[&UNITS, &TENS, &HUNDREDS, &THOUSANDS])
        }
        "hebrew" if (1..=999).contains(&value) => {
            const UNITS: [char; 9] = ['א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט'];
            const TENS: [char; 9] = ['י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ'];
            let mut out = String::new();
            let mut n = value;
            // Hundreds: ק ר ש ת, then ת repeated for 500 and more.
            while n >= 400 {
                out.push('ת');
                n -= 400;
            }
            if n >= 100 {
                out.push(['ק', 'ר', 'ש'][(n / 100 - 1) as usize]);
                n %= 100;
            }
            if n == 15 || n == 16 {
                out.push('ט');
                out.push(if n == 15 { 'ו' } else { 'ז' });
                return out;
            }
            if n >= 10 {
                out.push(TENS[(n / 10 - 1) as usize]);
                n %= 10;
            }
            if n > 0 {
                out.push(UNITS[(n - 1) as usize]);
            }
            out
        }
        "cjk-ideographic" if value >= 0 => {
            const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
            const UNITS: [char; 4] = ['\0', '十', '百', '千'];
            let s = value.to_string();
            let len = s.len();
            if len > 4 {
                return value.to_string();
            }
            let mut out = String::new();
            let mut zero = false;
            for (i, c) in s.chars().enumerate() {
                let d = c.to_digit(10).unwrap_or(0) as usize;
                let unit = len - 1 - i;
                if d == 0 {
                    zero = true;
                    continue;
                }
                if zero && !out.is_empty() {
                    out.push(DIGITS[0]);
                }
                zero = false;
                if !(d == 1 && unit == 1 && i == 0) {
                    out.push(DIGITS[d]);
                }
                if unit > 0 {
                    out.push(UNITS[unit]);
                }
            }
            if out.is_empty() { DIGITS[0].to_string() } else { out }
        }
        _ => value.to_string(),
    }
}

const ALPHABET: [char; 26] = [
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z',
];

// Construct alphanumeric marker from index, appending characters when index exceeds powers of 26
fn build_alpha_marker(index: usize, str: &mut String) {
    let rem = index % 26;
    let sym = ALPHABET[rem];
    str.insert(0, sym);
    let rest = (index - rem) as i64 / 26 - 1;
    if rest >= 0 {
        build_alpha_marker(rest as usize, str);
    }
}

#[cfg(test)]
mod tests {
    use crate::node::Marker;
    use style::{
        Atom,
        counter_style::CounterStyle,
        values::{CustomIdent, computed::ListStyleType},
    };

    use super::marker_for_style;

    fn list_style(s: &str) -> ListStyleType {
        ListStyleType(CounterStyle::Name(CustomIdent(Atom::from(s))))
    }

    #[test]
    fn test_marker_for_disc() {
        let result = marker_for_style(ListStyleType::disc(), 0);
        assert_eq!(result, Some(Marker::Char('•')));
    }

    #[test]
    fn test_marker_for_decimal() {
        let result_1 = marker_for_style(list_style("decimal"), 0);
        let result_2 = marker_for_style(list_style("decimal"), 1);
        assert_eq!(result_1, Some(Marker::String("1. ".to_string())));
        assert_eq!(result_2, Some(Marker::String("2. ".to_string())));
    }

    #[test]
    fn test_marker_for_lower_alpha() {
        let result_1 = marker_for_style(list_style("lower-alpha"), 0);
        let result_2 = marker_for_style(list_style("lower-alpha"), 1);
        let result_extended_1 = marker_for_style(list_style("lower-alpha"), 26);
        let result_extended_2 = marker_for_style(list_style("lower-alpha"), 27);
        assert_eq!(result_1, Some(Marker::String("a. ".to_string())));
        assert_eq!(result_2, Some(Marker::String("b. ".to_string())));
        assert_eq!(result_extended_1, Some(Marker::String("aa. ".to_string())));
        assert_eq!(result_extended_2, Some(Marker::String("ab. ".to_string())));
    }

    #[test]
    fn test_marker_for_upper_alpha() {
        let result_1 = marker_for_style(list_style("upper-alpha"), 0);
        let result_2 = marker_for_style(list_style("upper-alpha"), 1);
        let result_extended_1 = marker_for_style(list_style("upper-alpha"), 26);
        let result_extended_2 = marker_for_style(list_style("upper-alpha"), 27);
        assert_eq!(result_1, Some(Marker::String("A. ".to_string())));
        assert_eq!(result_2, Some(Marker::String("B. ".to_string())));
        assert_eq!(result_extended_1, Some(Marker::String("AA. ".to_string())));
        assert_eq!(result_extended_2, Some(Marker::String("AB. ".to_string())));
    }
}
