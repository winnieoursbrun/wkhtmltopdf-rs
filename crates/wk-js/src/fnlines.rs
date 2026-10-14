//! Function start lines of a script, for console message locations.
//!
//! QtWebKit 2.2 (JSC without a debugger attached) reports a console message
//! at the line where the *innermost executing function starts*, or where the
//! script starts for global code (verified with the 0.12.6 binary: a
//! `console.log` on line 7 inside `function f() {` declared on line 6 is
//! reported at line 6). Boa only gives the position of the current
//! statement, so the function boundaries are recovered with a lexical scan:
//! comments, strings, template literals and regular expressions are skipped,
//! `function … {` and `=> {` open a function body, the matching `}` closes it.

/// (line, column), 1-based.
type Pos = (u32, u32);

/// Function bodies of a source, as (start line, open position, close position).
#[derive(Debug, Default)]
pub(crate) struct FunctionLines {
    functions: Vec<(u32, Pos, Pos)>,
}

const REGEX_KEYWORDS: &[&str] =
    &["return", "typeof", "case", "in", "of", "new", "delete", "void", "throw", "do", "else", "instanceof"];

impl FunctionLines {
    pub(crate) fn scan(code: &str) -> FunctionLines {
        let chars: Vec<char> = code.chars().collect();
        let mut out = Vec::new();
        // Open functions: (start line, open pos, brace depth of the body).
        let mut stack: Vec<(u32, (u32, u32), i32)> = Vec::new();
        // `function` seen, waiting for its body: (start line, paren depth).
        let mut pending: Option<(u32, i32)> = None;
        let (mut line, mut col) = (1u32, 1u32);
        let mut braces = 0i32;
        let mut parens = 0i32;
        // Whether a `/` here starts a regular expression.
        let mut regex_ok = true;
        let mut i = 0usize;
        let n = chars.len();
        macro_rules! bump {
            () => {{
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                } else {
                    col += 1;
                }
                i += 1;
            }};
        }
        while i < n {
            let c = chars[i];
            match c {
                '/' if i + 1 < n && chars[i + 1] == '/' => {
                    while i < n && chars[i] != '\n' {
                        bump!();
                    }
                }
                '/' if i + 1 < n && chars[i + 1] == '*' => {
                    bump!();
                    bump!();
                    while i < n && !(chars[i] == '*' && i + 1 < n && chars[i + 1] == '/') {
                        bump!();
                    }
                    if i < n {
                        bump!();
                        bump!();
                    }
                }
                '\'' | '"' | '`' => {
                    let q = c;
                    bump!();
                    while i < n && chars[i] != q {
                        if chars[i] == '\\' && i + 1 < n {
                            bump!();
                        }
                        if q != '`' && chars[i] == '\n' {
                            break;
                        }
                        bump!();
                    }
                    if i < n {
                        bump!();
                    }
                    regex_ok = false;
                }
                '/' if regex_ok => {
                    bump!();
                    let mut class = false;
                    while i < n && chars[i] != '\n' {
                        match chars[i] {
                            '\\' if i + 1 < n => {
                                bump!();
                            }
                            '[' => class = true,
                            ']' => class = false,
                            '/' if !class => break,
                            _ => {}
                        }
                        bump!();
                    }
                    if i < n && chars[i] == '/' {
                        bump!();
                    }
                    while i < n && chars[i].is_ascii_alphabetic() {
                        bump!();
                    }
                    regex_ok = false;
                }
                c if c.is_alphabetic() || c == '_' || c == '$' => {
                    let start_line = line;
                    let s = i;
                    while i < n && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                        bump!();
                    }
                    let word: String = chars[s..i].iter().collect();
                    if word == "function" {
                        pending = Some((start_line, parens));
                    }
                    regex_ok = REGEX_KEYWORDS.contains(&word.as_str());
                }
                c if c.is_ascii_digit() => {
                    while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                        bump!();
                    }
                    regex_ok = false;
                }
                '=' if i + 1 < n && chars[i + 1] == '>' => {
                    let l = line;
                    bump!();
                    bump!();
                    // `=> {` opens a body; `=> expr` has none.
                    let mut j = i;
                    while j < n && chars[j].is_whitespace() {
                        j += 1;
                    }
                    if j < n && chars[j] == '{' {
                        pending = Some((l, parens));
                    }
                    regex_ok = true;
                }
                '{' => {
                    braces += 1;
                    if let Some((l, p)) = pending {
                        if p == parens {
                            stack.push((l, (line, col), braces));
                            pending = None;
                        }
                    }
                    bump!();
                    regex_ok = true;
                }
                '}' => {
                    if stack.last().is_some_and(|f| f.2 == braces) {
                        let (l, open, _) = stack.pop().expect("open function");
                        out.push((l, open, (line, col)));
                    }
                    braces -= 1;
                    bump!();
                    regex_ok = true;
                }
                '(' => {
                    parens += 1;
                    bump!();
                    regex_ok = true;
                }
                ')' => {
                    parens -= 1;
                    bump!();
                    regex_ok = false;
                }
                ']' => {
                    bump!();
                    regex_ok = false;
                }
                c if c.is_whitespace() => bump!(),
                _ => {
                    bump!();
                    regex_ok = true;
                }
            }
        }
        // Unclosed bodies extend to the end.
        for (l, open, _) in stack {
            out.push((l, open, (u32::MAX, u32::MAX)));
        }
        FunctionLines { functions: out }
    }

    /// Start line of the innermost function whose body contains `(line, col)`
    /// (1-based), or `None` for global code.
    pub(crate) fn function_start(&self, line: u32, col: u32) -> Option<u32> {
        let p = (line, col);
        self.functions
            .iter()
            .filter(|(_, open, close)| *open < p && p <= *close)
            .max_by_key(|(_, open, _)| *open)
            .map(|(l, _, _)| *l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_functions_and_global_code() {
        let src = "console.log(1);\nfunction f() {\n  var s = '}';\n  console.log(2);\n}\nsetTimeout(function () {\n  g(/}/);\n}, 1);\n";
        let fl = FunctionLines::scan(src);
        assert_eq!(fl.function_start(1, 1), None);
        assert_eq!(fl.function_start(4, 3), Some(2));
        assert_eq!(fl.function_start(7, 3), Some(6));
        assert_eq!(fl.function_start(8, 4), None);
    }

    #[test]
    fn division_is_not_a_regex() {
        let src = "var a = b / 2; function f() { return a / 3 / 4; }\nx();";
        let fl = FunctionLines::scan(src);
        assert_eq!(fl.function_start(1, 32), Some(1));
        assert_eq!(fl.function_start(2, 1), None);
    }
}
