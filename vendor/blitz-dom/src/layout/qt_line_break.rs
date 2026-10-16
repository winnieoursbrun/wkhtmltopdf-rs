//! wkhtmltopdf-rs: QtWebKit 2.2's soft wrap opportunities between punctuation.
//!
//! QtWebKit breaks lines with Qt 4.8's line breaker, an older UAX #14 than ICU's. The two
//! disagree around punctuation: QtWebKit breaks after an ASCII hyphen before digits and before
//! most punctuation (`2026-10-01` wraps as `2026-` / `10-` / `01`, `a-.` as `a-` / `.`), never
//! after `/` (`http://x.y/z` stays whole), and breaks after `?` before letters.
//!
//! The table below is the reference's behaviour, measured pair by pair on the real wkhtmltopdf
//! 0.12.6: every `x y` of the characters in [`CHARS`] rendered as `a x y a` and `1 x y 1` in a
//! 1px-wide block (both contexts gave the same breaks), the line breaks read back with
//! `pdftotext -bbox`. ASCII letters behave as `a`, digits as `1`, the other Latin letters as
//! `é`. Pairs of letters and digits, and characters outside the table, are left to ICU.

use parley::LineBreakContext;

/// The measured characters, in the order of the rows and columns of [`BREAKS`].
const CHARS: &str = "a1-/.,:;?!()[]{}%$+=_&*'\"#@<>|~\\^`—–€é’“”«»…";

/// `BREAKS[row][col]`: `|` when QtWebKit can break between `CHARS[row]` and `CHARS[col]`.
const BREAKS: [&str; 44] = [
    "..................................|.........", // a
    "..................................|.........", // 1
    "|||||||||||||||||||||||||||||||||||..|.....|", // -
    "..................................|........|", // /
    "..........|.|.|............|......|.|......|", // .
    "..........|.|.|............|......|.|......|", // ,
    "..........|.|.|............|......|.|......|", // :
    "..........|.|.|............|......|.|......|", // ;
    "|||.......|.|.|.|||||||..||||||||||.|......|", // ?
    "..........|.|.|............|......|.|......|", // !
    "............................................", // (
    "..........|.|.|............|......|........|", // )
    "............................................", // [
    "..........|.|.|............|......|........|", // ]
    "............................................", // {
    "..........|.|.|............|......|........|", // }
    "..........|.|.|............|......|........|", // %
    "..................................|........|", // $
    "..........|.|.|............|......|........|", // +
    "..........|.|.|............|......|.........", // =
    "..................................|.........", // _
    "..........|.|.|............|......|.........", // &
    "..........|.|.|............|......|.........", // *
    "............................................", // '
    "..........|.|.|............|................", // "
    "..........|.|.|............|......|.........", // #
    "..................................|.........", // @
    "..................................|.........", // <
    "..........|.|.|............|......|.........", // >
    "..........|.|.|............|......|.||.....|", // |
    "..........|.|.|............|......|.........", // ~
    "..........|.|.|............|......|........|", // \
    "..................................|.........", // ^
    "..................................|.........", // `
    "||........|.|.|.|||||||..||||.||||..||.....|", // —
    "||........|.|.|.|||||||..||||.|||||.||.....|", // –
    "..................................|........|", // €
    "..................................|.........", // é
    "............................................", // ’
    "............................................", // “
    "............................................", // ”
    "............................................", // «
    "............................................", // »
    "||........|.|.|.|||||||..||||.|||||.||......", // …
];

/// Index of `c` (or of the character it behaves as) in [`CHARS`].
fn index(c: char) -> Option<usize> {
    let c = if c.is_ascii_alphabetic() {
        'a'
    } else if c.is_ascii_digit() {
        '1'
    } else if ('\u{c0}'..='\u{24f}').contains(&c) && c.is_alphabetic() {
        'é'
    } else {
        c
    };
    CHARS.chars().position(|x| x == c)
}

/// The [`parley::LineBreakOverrideFn`] giving QtWebKit's opportunities.
pub(crate) fn qtwebkit_line_break(cx: LineBreakContext) -> Option<bool> {
    breaks_between(cx.before, cx.after)
}

fn breaks_between(before: char, after: char) -> Option<bool> {
    let (a, b) = (index(before)?, index(after)?);
    // Letters and digits between themselves: ICU already agrees, and `word-break` stays its own.
    let alnum = |i: usize| i <= 1 || i == 37;
    if alnum(a) && alnum(b) {
        return None;
    }
    Some(BREAKS[a].as_bytes()[b] == b'|')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_square() {
        assert_eq!(CHARS.chars().count(), BREAKS.len());
        assert!(BREAKS.iter().all(|r| r.len() == BREAKS.len()));
        assert_eq!(index('é'), Some(37));
    }

    #[test]
    fn qtwebkit_breaks() {
        // `2026-10-01` wraps after each hyphen; a URL does not wrap after its slashes.
        assert_eq!(breaks_between('-', '1'), Some(true));
        assert_eq!(breaks_between('-', '-'), Some(true));
        assert_eq!(breaks_between('/', 'x'), Some(false));
        assert_eq!(breaks_between('?', 'W'), Some(true));
        assert_eq!(breaks_between('-', '€'), Some(false));
        assert_eq!(breaks_between('a', 'b'), None);
        assert_eq!(breaks_between('-', 'ж'), None);
    }
}
