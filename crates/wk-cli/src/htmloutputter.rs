//! HTML outputter used by `--htmldoc`. Ports upstream
//! `src/shared/htmloutputter.cc`, including the malformed DOCTYPE produced by
//! C string concatenation.

use crate::outputter::{Fd, Outputter, Streams, SwitchDoc};

/// `Qt::escape` (Qt 4): `<`, `>`, `&` and `"`.
pub fn qt_escape(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => r.push_str("&lt;"),
            '>' => r.push_str("&gt;"),
            '&' => r.push_str("&amp;"),
            '"' => r.push_str("&quot;"),
            _ => r.push(c),
        }
    }
    r
}

/// `HtmlOutputter`.
pub struct HtmlOutputter<'a> {
    s: &'a mut Streams,
    fd: Fd,
    ordered: bool,
}

impl<'a> HtmlOutputter<'a> {
    /// Creating the outputter writes the HTML header, like the upstream
    /// constructor.
    pub fn new(s: &'a mut Streams, fd: Fd) -> Self {
        let mut o = HtmlOutputter { s, fd, ordered: false };
        o.put(concat!(
            "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN",
            "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n",
            "<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"en\" lang=\"en\" dir=\"ltr\">\n",
            "<head>\n",
            "  <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\" />\n",
            "  <title>wkhtmltopdf - Manual</title>\n",
            "  <style type=\"text/css\">\n",
            "    body {width: 70em}\n",
            "    .short {font-weight: bold; width:2em}\n",
            "    .long {font-weight: bold; width: 15em}\n",
            "    .arg {font-style: italic; width: 12em}\n",
            "    tr {vertical-align: top}\n",
            "  </style>\n",
            "</head><body>"
        ));
        o
    }

    fn put(&mut self, t: &str) {
        self.s.fd(self.fd).extend_from_slice(t.as_bytes());
    }
}

impl Outputter for HtmlOutputter<'_> {
    fn begin_section(&mut self, name: &str) {
        let n = qt_escape(name);
        self.put(&format!("<a name=\"{n}\"><h1>{n}</h1></a>\n"));
    }

    fn end_section(&mut self) {}

    fn begin_paragraph(&mut self) {
        self.put("<p>");
    }

    fn end_paragraph(&mut self) {
        self.put("</p>\n");
    }

    fn text(&mut self, t: &str) {
        self.put(&qt_escape(t));
    }

    fn section_link(&mut self, s: &str) {
        let n = qt_escape(s);
        self.put(&format!("<a href=\"#{n}\">{n}</a>"));
    }

    fn bold(&mut self, t: &str) {
        self.put(&format!("<b>{}</b>", qt_escape(t)));
    }

    fn italic(&mut self, t: &str) {
        self.put(&format!("<i>{}</i>", qt_escape(t)));
    }

    fn link(&mut self, l: &str) {
        let n = qt_escape(l);
        self.put(&format!("<a href=\"{n}\">{n}</a>"));
    }

    fn verbatim(&mut self, t: &str) {
        self.put(&format!("<pre>{}</pre>", qt_escape(t)));
    }

    fn begin_list(&mut self, ordered: bool) {
        self.ordered = ordered;
        self.put(if ordered { "<ol>" } else { "<ul>" });
    }

    fn end_list(&mut self) {
        self.put(if self.ordered { "</ol>" } else { "</ul>" });
    }

    fn list_item(&mut self, t: &str) {
        self.put(&format!("<li>{}</li>\n", qt_escape(t)));
    }

    fn begin_switch(&mut self) {
        self.put("<table>\n");
    }

    fn cswitch(&mut self, h: &SwitchDoc) {
        self.put("<tr><td class=\"short\">");
        if let Some(c) = h.short_switch {
            self.put("-");
            self.s.fd(self.fd).push(c);
            self.put(",");
        }
        let star = if h.qthack {
            "<span style=\"font-weight: normal; font-size: 80%; color:red;\">*</span>"
        } else {
            ""
        };
        self.put(&format!(
            "</td><td class=\"long\">--{}{}</td><td class=\"arg\">",
            qt_escape(&h.long_name),
            star
        ));
        for a in &h.argn {
            self.put(&format!("&lt;{}&gt; ", qt_escape(a)));
        }
        self.put(&format!("</td><td class=\"desc\">{}</td></tr>\n", qt_escape(&h.desc_with_default)));
    }

    fn end_switch(&mut self) {
        self.put("</table>\n");
        self.put(
            "<p>Items marked <span style=\"font-weight: normal; font-size: 80%; color:red;\">*</span> are only available using patched QT.</p>",
        );
    }

    fn finish(&mut self) {
        self.put("</body></html>\n");
    }
}
