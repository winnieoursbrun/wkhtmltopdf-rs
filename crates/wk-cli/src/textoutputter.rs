//! Plain text outputter used by `--help`, `-H`, `--license`, `--readme` and
//! the usage printed on argument errors. Ports upstream
//! `src/shared/textoutputter.cc`, including its quirks (spec §2.4):
//!
//! - `text()` resets `first` on every call, so consecutive calls are glued
//!   with no space and words are split on single spaces (empty words kept);
//! - the wrap newline inside a switch description and the blank line after a
//!   switch section are written with `printf`, i.e. always to **stdout**,
//!   even when the usage goes to stderr.

use crate::outputter::{Fd, Outputter, Streams, SwitchDoc, qsize};

const LW: usize = 80;

/// `TextOutputter`.
pub struct TextOutputter<'a> {
    s: &'a mut Streams,
    fd: Fd,
    doc: bool,
    w: usize,
    first: bool,
    order: i32,
}

impl<'a> TextOutputter<'a> {
    pub fn new(s: &'a mut Streams, fd: Fd, doc: bool) -> Self {
        TextOutputter { s, fd, doc, w: 0, first: true, order: -1 }
    }

    /// `fprintf(fd, "%s", S(t))`: UTF-8 in doc mode, locale codec otherwise.
    fn put(&mut self, t: &str) {
        let bytes = if self.doc { t.as_bytes().to_vec() } else { self.s.local8bit(t) };
        self.s.fd(self.fd).extend_from_slice(&bytes);
    }

    /// `fprintf(fd, …)` of ASCII text.
    fn raw(&mut self, t: &str) {
        self.s.fd(self.fd).extend_from_slice(t.as_bytes());
    }

    /// `printf(…)`: always stdout.
    fn printf(&mut self, t: &str) {
        self.s.out.extend_from_slice(t.as_bytes());
    }

    fn new_line_indent(&mut self) {
        if self.doc {
            self.w = 0;
        } else {
            self.w = 2;
            self.raw("  ");
        }
    }
}

impl Outputter for TextOutputter<'_> {
    fn begin_section(&mut self, name: &str) {
        if self.doc {
            let mut x = 80 - qsize(name) as i64 - 4;
            if x < 6 {
                x = 60;
            }
            let left = "=".repeat((x / 2) as usize);
            let right = "=".repeat(((x + 1) / 2) as usize);
            self.raw(&left);
            self.raw("> ");
            self.put(name);
            self.raw(" <");
            self.raw(&right);
            self.raw("\n");
        } else {
            self.put(name);
            self.raw(":\n");
        }
    }

    fn end_section(&mut self) {}

    fn begin_paragraph(&mut self) {
        self.first = true;
        self.new_line_indent();
    }

    fn text(&mut self, t: &str) {
        self.first = true;
        for s in t.split(' ') {
            if self.w + qsize(s) + usize::from(!self.first) > LW {
                self.raw("\n");
                self.new_line_indent();
                self.first = true;
            }
            if self.first {
                self.first = false;
            } else {
                self.raw(" ");
                self.w += 1;
            }
            self.w += qsize(s);
            self.put(s);
        }
    }

    fn bold(&mut self, t: &str) {
        self.text(&format!("*{t}*"));
    }

    fn italic(&mut self, t: &str) {
        self.text(&format!("_{t}_"));
    }

    fn link(&mut self, l: &str) {
        self.text(&format!("<{l}>"));
    }

    fn section_link(&mut self, s: &str) {
        self.text(s);
    }

    fn end_paragraph(&mut self) {
        self.raw("\n\n");
    }

    fn verbatim(&mut self, t: &str) {
        if self.doc {
            self.put(t);
            self.raw("\n");
        } else {
            for line in t.split('\n') {
                self.raw("  ");
                self.put(line);
                self.raw("\n");
            }
        }
    }

    fn begin_list(&mut self, ordered: bool) {
        self.order = if ordered { 1 } else { -1 };
    }

    fn end_list(&mut self) {
        self.raw("\n");
    }

    fn list_item(&mut self, t: &str) {
        if self.order < 0 {
            self.raw(" * ");
        } else {
            let n = format!("{:3} ", self.order);
            self.raw(&n);
            self.order += 1;
        }
        self.put(t);
        self.raw("\n");
    }

    fn begin_switch(&mut self) {}

    fn cswitch(&mut self, h: &SwitchDoc) {
        self.w = 0;
        if !self.doc {
            self.raw("  ");
            self.w = 2;
        }
        match h.short_switch {
            Some(c) => {
                self.raw("-");
                self.s.fd(self.fd).push(c);
                self.raw(", ");
            }
            None => self.raw("    "),
        }
        self.raw("--");
        self.put(&h.long_name);
        self.w += 4 + 2 + qsize(&h.long_name);
        if self.doc && h.qthack {
            self.raw(" *");
            self.w += 2;
        }
        for arg in &h.argn {
            self.raw(" <");
            self.put(arg);
            self.raw(">");
            self.w += 3 + qsize(arg);
        }
        while self.w < 37 {
            self.raw(" ");
            self.w += 1;
        }
        for s in h.desc_with_default.split(' ') {
            if self.w + 1 + qsize(s) > LW {
                self.printf("\n");
                self.w = 0;
                while self.w < 37 {
                    self.raw(" ");
                    self.w += 1;
                }
            }
            self.raw(" ");
            self.put(s);
            self.w += qsize(s) + 1;
        }
        self.raw("\n");
    }

    fn end_switch(&mut self) {
        if self.doc {
            self.raw("\nItems marked * are only available using patched QT.\n");
        }
        self.printf("\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(desc: &str) -> SwitchDoc {
        SwitchDoc {
            long_name: "zoom".into(),
            short_switch: None,
            argn: vec!["float".into()],
            qthack: false,
            desc: desc.into(),
            desc_with_default: desc.into(),
        }
    }

    #[test]
    fn usage_wrap_newlines_go_to_stdout() {
        let mut s = Streams::new(false);
        {
            let mut o = TextOutputter::new(&mut s, Fd::Stderr, false);
            o.begin_switch();
            o.cswitch(&doc("Use this zoom factor and some more words so that the line wraps around"));
            o.end_switch();
        }
        // One wrap newline plus the end-of-section newline, both on stdout.
        assert_eq!(s.out, b"\n\n");
        assert_eq!(s.err.iter().filter(|b| **b == b'\n').count(), 1);
    }

    #[test]
    fn text_splits_on_single_spaces() {
        let mut s = Streams::new(false);
        {
            let mut o = TextOutputter::new(&mut s, Fd::Stdout, false);
            o.begin_paragraph();
            o.text("a");
            o.text(" and ");
            o.text("b");
            o.end_paragraph();
        }
        assert_eq!(s.out, b"  a and b\n\n");
    }

    #[test]
    fn non_ascii_dropped_in_c_locale_only_when_not_doc() {
        let mut s = Streams::new(false);
        TextOutputter::new(&mut s, Fd::Stdout, false).verbatim("Garc\u{ed}a");
        TextOutputter::new(&mut s, Fd::Stdout, true).verbatim("Garc\u{ed}a");
        assert_eq!(s.out, "  Garca\nGarc\u{ed}a\n".as_bytes());
    }
}
