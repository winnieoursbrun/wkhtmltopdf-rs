//! `--read-args-from-stdin` line reader and shell-like tokenizer. Ports
//! `parseString` and `fgets_large` from upstream `src/pdf/wkhtmltopdf.cc`
//! (spec §1.7).

use std::io::BufRead;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Skip,
    Tok,
    Q1,
    Q2,
    Q1Esc,
    Q2Esc,
    TokEsc,
}

fn is_sep(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\r' | b'\n')
}

/// `parseString`: split a line into arguments. Separators are space, tab,
/// CR and LF; `'…'` and `"…"` quote, and `\` escapes the next character
/// everywhere (also inside single quotes). Pieces concatenate; an empty
/// token (`""`) is dropped. The line ends at the first NUL byte, like the C
/// string it is upstream.
pub fn parse_string(line: &[u8]) -> Vec<Vec<u8>> {
    let end = line.iter().position(|&c| c == 0).unwrap_or(line.len());
    let buff = &line[..end];
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut state = State::Skip;
    let mut read = 0;
    while read < buff.len() {
        let c = buff[read];
        match state {
            State::Skip => {
                if !is_sep(c) {
                    // Re-read this character in the token state.
                    state = State::Tok;
                    continue;
                }
            }
            State::Tok => {
                if c == b'\'' {
                    state = State::Q1;
                } else if c == b'"' {
                    state = State::Q2;
                } else if c == b'\\' {
                    state = State::TokEsc;
                } else if is_sep(c) {
                    state = State::Skip;
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(c);
                }
            }
            State::Q1 => {
                if c == b'\'' {
                    state = State::Tok;
                } else if c == b'\\' {
                    state = State::Q1Esc;
                } else {
                    cur.push(c);
                }
            }
            State::Q2 => {
                if c == b'"' {
                    state = State::Tok;
                } else if c == b'\\' {
                    state = State::Q2Esc;
                } else {
                    cur.push(c);
                }
            }
            State::TokEsc => {
                state = State::Tok;
                cur.push(c);
            }
            State::Q1Esc => {
                state = State::Q1;
                cur.push(c);
            }
            State::Q2Esc => {
                state = State::Q2;
                cur.push(c);
            }
        }
        read += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `fgets_large`: the next line including its `\n` (a final line without
/// `\n` is returned too), or `None` at end of input.
pub fn fgets_large(r: &mut dyn BufRead) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    match r.read_until(b'\n', &mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(buf),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        parse_string(s.as_bytes()).into_iter().map(|v| String::from_utf8(v).unwrap()).collect()
    }

    #[test]
    fn read_args_tokenizer_splits_on_whitespace() {
        assert_eq!(p("  a\tb\r\nc  \n"), ["a", "b", "c"]);
        assert!(p("   \n").is_empty());
        assert!(p("").is_empty());
    }

    #[test]
    fn read_args_tokenizer_quotes_concatenate() {
        assert_eq!(p("a\"b c\"d"), ["ab cd"]);
        assert_eq!(p("'x y' \"z\""), ["x y", "z"]);
    }

    #[test]
    fn read_args_tokenizer_backslash_escapes_even_in_single_quotes() {
        assert_eq!(p(r"a\ b"), ["a b"]);
        assert_eq!(p(r"'it\'s'"), ["it's"]);
        assert_eq!(p(r#""say \"hi\"""#), [r#"say "hi""#]);
    }

    #[test]
    fn read_args_tokenizer_drops_empty_quotes() {
        assert_eq!(p("a \"\" b"), ["a", "b"]);
        assert_eq!(p("''"), Vec::<String>::new());
    }

    #[test]
    fn read_args_tokenizer_unterminated_quote_keeps_token() {
        assert_eq!(p("'abc"), ["abc"]);
    }

    #[test]
    fn fgets_large_returns_last_line_without_newline() {
        let mut r: &[u8] = b"one\ntwo";
        assert_eq!(fgets_large(&mut r).unwrap(), b"one\n");
        assert_eq!(fgets_large(&mut r).unwrap(), b"two");
        assert_eq!(fgets_large(&mut r), None);
    }

    #[test]
    fn fgets_large_has_no_length_limit() {
        let long = vec![b'x'; 5000];
        let mut data = long.clone();
        data.push(b'\n');
        let mut r: &[u8] = &data;
        assert_eq!(fgets_large(&mut r).unwrap().len(), 5001);
    }
}
