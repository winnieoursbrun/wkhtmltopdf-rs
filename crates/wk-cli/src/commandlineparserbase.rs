//! Switch registry, single-switch parser and switch listing. Ports upstream
//! `src/shared/commandlineparserbase.hh`, `src/shared/commandlineparserbase.cc`
//! and the registration helpers (`section`, `mode`, `qthack`, `extended`,
//! `addarg`) of `src/shared/commonarguments.cc`.
//!
//! Only the patched-Qt build is reproduced (all release binaries are patched,
//! spec "Patched vs unpatched Qt"): QHACK switches are always accepted and
//! always listed.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::arghandler::{Action, ArgHandler, Handle, Outcome};
use crate::outputter::{Outputter, SwitchDoc};

/// `FULL_VERSION` of the release being reproduced.
pub const FULL_VERSION: &str = "0.12.6";

/// `CommandLineParserBase::appVersion()` for a patched-Qt build.
pub fn app_version() -> String {
    format!("{FULL_VERSION} (with patched qt)")
}

/// Why `parse_arg` stopped. Upstream prints and calls `exit` directly; here
/// the owner of the parser does it (see [`ParseError::Usage`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Print these bytes to stderr, then `usage(stderr, false)`, then exit 1.
    Usage(Vec<u8>),
    /// A documentation switch was reached: run it, then exit 0.
    Action(Action),
}

/// `CommandLineParserBase`. `G` is the global settings target and `P` the
/// per-object target of the switches.
pub struct CommandLineParserBase<G, P> {
    current_mode: i32,
    current_section: String,
    current_extended: bool,
    current_hack: bool,
    pub sections: Vec<String>,
    pub handlers: Vec<ArgHandler<G, P>>,
    long_to_handler: HashMap<Vec<u8>, usize>,
    short_to_handler: HashMap<u8, usize>,
    section_argument_handles: HashMap<String, Vec<usize>>,
    section_desc: HashMap<String, String>,
}

impl<G, P> Default for CommandLineParserBase<G, P> {
    fn default() -> Self {
        CommandLineParserBase {
            current_mode: 0,
            current_section: String::new(),
            current_extended: false,
            current_hack: false,
            sections: Vec::new(),
            handlers: Vec::new(),
            long_to_handler: HashMap::new(),
            short_to_handler: HashMap::new(),
            section_argument_handles: HashMap::new(),
            section_desc: HashMap::new(),
        }
    }
}

/// `ahsort`: compare long names with the `no-`, `enable-`, `disable-`,
/// `include-in-` and `exclude-from-` prefixes removed; on a tie compare the
/// full names with `no-` replaced by `zzzz` (so `--x` sorts before `--no-x`).
pub fn ahsort(a: &str, b: &str) -> Ordering {
    fn strip(s: &str) -> &str {
        for p in ["no-", "enable-", "disable-", "include-in-", "exclude-from-"] {
            if let Some(r) = s.strip_prefix(p) {
                return r;
            }
        }
        s
    }
    fn zzzz(s: &str) -> String {
        match s.strip_prefix("no-") {
            Some(r) => format!("zzzz{r}"),
            None => s.to_string(),
        }
    }
    fn qcmp(a: &str, b: &str) -> Ordering {
        a.encode_utf16().cmp(b.encode_utf16())
    }
    let (x, y) = (strip(a), strip(b));
    if x == y { qcmp(&zzzz(a), &zzzz(b)) } else { qcmp(x, y) }
}

impl<G, P> CommandLineParserBase<G, P> {
    /// `section(s, desc)`: following switches belong to this help section.
    pub fn section(&mut self, s: &str, desc: &str) {
        self.current_section = s.to_string();
        self.section_desc.insert(s.to_string(), desc.to_string());
        self.sections.push(s.to_string());
    }

    /// `mode(m)`: scope bit mask of the following switches.
    pub fn mode(&mut self, m: i32) {
        self.current_mode = m;
    }

    /// `qthack(h)`: following switches need patched Qt.
    pub fn qthack(&mut self, h: bool) {
        self.current_hack = h;
    }

    /// `extended(e)`: following switches only appear in the extended help.
    pub fn extended(&mut self, e: bool) {
        self.current_extended = e;
    }

    /// `addarg(long, short, desc, handler)`.
    pub fn addarg(&mut self, l: &str, s: Option<u8>, d: &str, h: impl Handle<G, P> + 'static) {
        let idx = self.handlers.len();
        self.handlers.push(ArgHandler {
            long_name: l.to_string(),
            desc: d.to_string(),
            short_switch: s,
            argn: h.argn(),
            display: true,
            extended: self.current_extended,
            qthack: self.current_hack,
            section: self.current_mode,
            handler: Box::new(h),
        });
        self.long_to_handler.insert(l.as_bytes().to_vec(), idx);
        if let Some(c) = s {
            self.short_to_handler.insert(c, idx);
        }
        self.section_argument_handles.entry(self.current_section.clone()).or_default().push(idx);
    }

    /// Look up a switch by long name.
    pub fn handler(&self, long_name: &str) -> Option<&ArgHandler<G, P>> {
        self.long_to_handler.get(long_name.as_bytes()).map(|i| &self.handlers[*i])
    }

    /// `parseArg`: parse the switch at `argv[*arg]` (which starts with `-`),
    /// applying it to `g`/`p`, and advance `*arg` past its arguments.
    pub fn parse_arg(
        &self,
        sections: i32,
        argv: &[Vec<u8>],
        default_mode: &mut bool,
        arg: &mut usize,
        g: &mut G,
        p: &mut P,
    ) -> Result<(), ParseError> {
        let cur = &argv[*arg];
        let at = |i: usize| cur.get(i).copied().unwrap_or(0);
        let argc = argv.len();
        if at(1) == b'-' {
            // Long switch. Upstream meant `--` to enter "default mode" but
            // tests the character '0', so `--0…` does and `--` does not.
            if at(2) == b'0' {
                *default_mode = true;
                return Ok(());
            }
            let msg = |m: &str| {
                let mut v = m.as_bytes().to_vec();
                v.extend_from_slice(cur);
                v
            };
            let Some(&idx) = self.long_to_handler.get(&cur[2..]) else {
                let mut v = msg("Unknown long argument ");
                v.extend_from_slice(b"\n\n");
                return Err(ParseError::Usage(v));
            };
            let h = &self.handlers[idx];
            if h.section & sections == 0 {
                let mut v = cur.clone();
                v.extend_from_slice(b" specified in incorrect location\n\n");
                return Err(ParseError::Usage(v));
            }
            if argc - *arg < h.argn.len() + 1 {
                let mut v = msg("Not enough arguments parsed to ");
                v.extend_from_slice(b"\n\n");
                return Err(ParseError::Usage(v));
            }
            match h.handler.call(&argv[*arg + 1..*arg + 1 + h.argn.len()], g, p) {
                Outcome::Ok => {}
                Outcome::Invalid => {
                    let mut v = msg("Invalid argument(s) parsed to ");
                    v.extend_from_slice(b"\n\n");
                    return Err(ParseError::Usage(v));
                }
                Outcome::Action(a) => return Err(ParseError::Action(a)),
            }
            *arg += h.argn.len();
        } else {
            // Grouped short switches: each one consumes the following argv
            // entries.
            let c = *arg;
            for &ch in &argv[c][1..] {
                let fmt = |m: &str| {
                    let mut v = m.as_bytes().to_vec();
                    v.push(ch);
                    v.extend_from_slice(b"\n\n");
                    v
                };
                let Some(&idx) = self.short_to_handler.get(&ch) else {
                    return Err(ParseError::Usage(fmt("Unknown switch -")));
                };
                let h = &self.handlers[idx];
                if h.section & sections == 0 {
                    let mut v = vec![b'-', ch];
                    v.extend_from_slice(b" specified in incorrect location\n\n");
                    return Err(ParseError::Usage(v));
                }
                if argc - *arg < h.argn.len() + 1 {
                    return Err(ParseError::Usage(fmt("Not enough arguments parsed to -")));
                }
                match h.handler.call(&argv[*arg + 1..*arg + 1 + h.argn.len()], g, p) {
                    Outcome::Ok => {}
                    Outcome::Invalid => {
                        return Err(ParseError::Usage(fmt("Invalid argument(s) parsed to -")));
                    }
                    Outcome::Action(a) => return Err(ParseError::Action(a)),
                }
                *arg += h.argn.len();
            }
        }
        Ok(())
    }

    /// `outputSwitches`: list the switches of every section, sorted with
    /// [`ahsort`]. `g` is the current global state and `od` the pristine
    /// object template; together they give the "(default …)" values.
    pub fn output_switches(&self, o: &mut dyn Outputter, extended: bool, _doc: bool, g: &G, od: &P) {
        for section in &self.sections {
            let mut display: Vec<&ArgHandler<G, P>> = self
                .section_argument_handles
                .get(section)
                .map(|v| v.iter().map(|i| &self.handlers[*i]).collect())
                .unwrap_or_default();
            display.retain(|h| extended || !h.extended);
            display.sort_by(|a, b| ahsort(&a.long_name, &b.long_name));
            if display.is_empty() {
                continue;
            }
            o.begin_section(section);
            if let Some(desc) = self.section_desc.get(section).filter(|d| !d.is_empty()) {
                o.begin_paragraph();
                o.text(desc);
                o.end_paragraph();
            }
            o.begin_switch();
            for h in display {
                o.cswitch(&SwitchDoc {
                    long_name: h.long_name.clone(),
                    short_switch: h.short_switch,
                    argn: h.argn.clone(),
                    qthack: h.qthack,
                    desc: h.desc.clone(),
                    desc_with_default: h.get_desc(g, od),
                });
            }
            o.end_switch();
            o.end_section();
        }
    }
}

/// `CommandLineParserBase::version`: `"<app> <version>\n"`.
pub fn version_text(app_name: &str) -> String {
    format!("{} {}\n", app_name, app_version())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ahsort_orders_pairs() {
        assert_eq!(ahsort("collate", "no-collate"), Ordering::Less);
        assert_eq!(ahsort("disable-forms", "enable-forms"), Ordering::Less);
        assert_eq!(ahsort("exclude-from-outline", "include-in-outline"), Ordering::Less);
        assert_eq!(ahsort("outline", "no-outline"), Ordering::Less);
        assert_eq!(ahsort("no-outline", "outline-depth"), Ordering::Less);
        assert_eq!(ahsort("no-collate", "copies"), Ordering::Less);
        assert_eq!(ahsort("custom-header", "custom-header-propagation"), Ordering::Less);
        assert_eq!(ahsort("custom-header-propagation", "no-custom-header-propagation"), Ordering::Less);
    }
}
