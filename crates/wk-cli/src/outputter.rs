//! Documentation outputter interface. Ports upstream `src/shared/outputter.hh`
//! and `src/shared/outputter.cc`.
//!
//! Upstream outputters write to a `FILE *` with `fprintf`, except for two
//! `printf` calls in the text outputter that always reach stdout (spec §2.4).
//! To reproduce that split byte for byte, outputters write into [`Streams`],
//! which buffers stdout and stderr separately; the program flushes both when
//! it exits.

use std::io::{self, Write};

/// Which standard stream an outputter writes to (`FILE *fd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fd {
    Stdout,
    Stderr,
}

/// Buffered stdout/stderr, plus the locale codec used by `toLocal8Bit`.
#[derive(Debug, Default, Clone)]
pub struct Streams {
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    /// Whether the locale codec is UTF-8. When false (C/POSIX locale), text
    /// converted with `toLocal8Bit` loses every non-ASCII character, which is
    /// what the 0.12.6 binary does in the C locale.
    pub utf8_locale: bool,
}

impl Streams {
    /// Empty buffers with the given locale codec.
    pub fn new(utf8_locale: bool) -> Self {
        Streams { out: Vec::new(), err: Vec::new(), utf8_locale }
    }

    /// Empty buffers with the locale codec of the process environment.
    pub fn from_env() -> Self {
        Streams::new(crate::locale::env_is_utf8())
    }

    /// The buffer of `fd`.
    pub fn fd(&mut self, fd: Fd) -> &mut Vec<u8> {
        match fd {
            Fd::Stdout => &mut self.out,
            Fd::Stderr => &mut self.err,
        }
    }

    /// `QString::toLocal8Bit()`.
    pub fn local8bit(&self, s: &str) -> Vec<u8> {
        if self.utf8_locale {
            s.as_bytes().to_vec()
        } else {
            s.chars().filter(char::is_ascii).collect::<String>().into_bytes()
        }
    }

    /// Write the buffered output to the real stderr and stdout, then clear
    /// the buffers. Stderr goes first: upstream's stderr is unbuffered while
    /// stdout is flushed at exit, so with `2>&1` the usage newlines that went
    /// to stdout appear last. Errors (for example a closed pipe) are ignored,
    /// like `fprintf`.
    pub fn flush_to_process(&mut self) {
        if !self.err.is_empty() {
            let mut e = io::stderr().lock();
            let _ = e.write_all(&self.err);
            let _ = e.flush();
            self.err.clear();
        }
        if !self.out.is_empty() {
            let mut o = io::stdout().lock();
            let _ = o.write_all(&self.out);
            let _ = o.flush();
            self.out.clear();
        }
    }
}

/// Everything an outputter needs to render one switch (`const ArgHandler *`).
#[derive(Debug, Clone)]
pub struct SwitchDoc {
    pub long_name: String,
    pub short_switch: Option<u8>,
    pub argn: Vec<String>,
    pub qthack: bool,
    /// Raw description (`h->desc`), used by the man outputter.
    pub desc: String,
    /// Description with the current default (`h->getDesc()`).
    pub desc_with_default: String,
}

/// `Outputter`.
pub trait Outputter {
    fn begin_section(&mut self, name: &str);
    fn end_section(&mut self);
    fn begin_paragraph(&mut self);
    fn text(&mut self, t: &str);
    fn bold(&mut self, t: &str);
    fn italic(&mut self, t: &str);
    fn link(&mut self, l: &str);
    fn section_link(&mut self, s: &str);
    fn end_paragraph(&mut self);
    fn verbatim(&mut self, t: &str);
    fn begin_list(&mut self, ordered: bool);
    fn end_list(&mut self);
    fn list_item(&mut self, t: &str);
    fn begin_switch(&mut self);
    fn cswitch(&mut self, h: &SwitchDoc);
    fn end_switch(&mut self);
    /// Called where upstream deletes the outputter (the HTML footer).
    fn finish(&mut self) {}

    /// `Outputter::paragraph`.
    fn paragraph(&mut self, t: &str) {
        self.begin_paragraph();
        self.text(t);
        self.end_paragraph();
    }
}

/// `QString::size()`: length in UTF-16 code units.
pub fn qsize(s: &str) -> usize {
    s.encode_utf16().count()
}
