//! Man page outputter used by `--manpage`. Ports upstream
//! `src/shared/manoutputter.cc`.

use crate::outputter::{Fd, Outputter, Streams, SwitchDoc};

/// `ManOutputter`.
pub struct ManOutputter<'a> {
    s: &'a mut Streams,
    fd: Fd,
    order: i32,
}

impl<'a> ManOutputter<'a> {
    /// Creating the outputter writes the `.TH` header, like the upstream
    /// constructor.
    pub fn new(s: &'a mut Streams, fd: Fd) -> Self {
        let mut o = ManOutputter { s, fd, order: -1 };
        o.put(".TH WKHTMLTOPDF 1 \"2009 February 23\"\n\n");
        o
    }

    fn put(&mut self, t: &str) {
        self.s.fd(self.fd).extend_from_slice(t.as_bytes());
    }
}

fn esc(t: &str) -> String {
    t.replace('-', "\\-")
}

impl Outputter for ManOutputter<'_> {
    fn begin_section(&mut self, name: &str) {
        self.put(&format!(".SH {name}\n"));
    }

    fn end_section(&mut self) {
        self.put("\n");
    }

    fn begin_paragraph(&mut self) {}

    fn end_paragraph(&mut self) {
        self.put("\n\n");
    }

    fn text(&mut self, t: &str) {
        self.put(&esc(t));
    }

    fn section_link(&mut self, s: &str) {
        self.text(s);
    }

    fn bold(&mut self, t: &str) {
        self.put(&format!("\\fB{t}\\fP"));
    }

    fn italic(&mut self, t: &str) {
        self.put(&format!("\\fB{t}\\fP"));
    }

    fn link(&mut self, l: &str) {
        self.put(&format!("<{l}>"));
    }

    fn verbatim(&mut self, t: &str) {
        let s = esc(t);
        let mut lines: Vec<&str> = s.split('\n').collect();
        while lines.last() == Some(&"") {
            lines.pop();
        }
        for line in lines {
            self.put(&format!("  {line}\n"));
        }
        self.put("\n");
    }

    fn begin_switch(&mut self) {
        self.put(".PD 0\n");
    }

    fn begin_list(&mut self, ordered: bool) {
        self.order = if ordered { 1 } else { -1 };
    }

    fn end_list(&mut self) {
        self.put("\n");
    }

    fn list_item(&mut self, t: &str) {
        if self.order < 0 {
            self.put(" * ");
        } else {
            let n = format!("{:3} ", self.order);
            self.put(&n);
            self.order += 1;
        }
        self.put(&format!("{t}\n"));
    }

    fn cswitch(&mut self, h: &SwitchDoc) {
        self.put(".TP\n");
        self.put("\\fB");
        match h.short_switch {
            Some(c) => {
                self.put("\\-");
                self.s.fd(self.fd).push(c);
                self.put(", ");
            }
            None => self.put("    "),
        }
        self.put(&format!("\\-\\-{}\\fR", h.long_name));
        for a in &h.argn {
            self.put(&format!(" \\fI<{a}>\\fR"));
        }
        self.put(&format!("\n{}\n", esc(&h.desc)));
    }

    fn end_switch(&mut self) {
        self.put(".PD\n");
        self.put("\n");
    }
}
