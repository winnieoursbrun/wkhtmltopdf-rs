//! Terminal progress feedback (phase lines, progress bar, log messages).
//! Ports upstream `src/shared/progressfeedback.hh` and
//! `src/shared/progressfeedback.cc` (spec §4.1).
//!
//! Upstream connects it to converter signals; here the converter
//! (`wk-converter`) calls the methods directly.

use std::io::Write;

use wk_settings::LogLevel;

use crate::outputter::qsize;

/// `ProgressFeedback`. Writes are best-effort, like `fprintf(stderr, …)`.
pub struct ProgressFeedback<W: Write> {
    log_level: LogLevel,
    out: W,
    /// Width of the last progress line drawn (0 after any newline).
    lw: usize,
}

impl<W: Write> ProgressFeedback<W> {
    pub fn new(log_level: LogLevel, out: W) -> Self {
        ProgressFeedback { log_level, out, lw: 0 }
    }

    /// The underlying writer.
    pub fn into_inner(self) -> W {
        self.out
    }

    fn put(&mut self, s: &str) {
        let _ = self.out.write_all(s.as_bytes());
    }

    fn finish_line(&mut self, start: usize) {
        let pad = self.lw.saturating_sub(start);
        self.put(&" ".repeat(pad));
        self.put("\n");
        self.lw = 0;
        let _ = self.out.flush();
    }

    fn message(&mut self, min: LogLevel, prefix: &str, message: &str) {
        if self.log_level < min {
            return;
        }
        self.put(prefix);
        self.put(message);
        self.finish_line(qsize(prefix) + qsize(message));
    }

    /// A message printed by Qt itself (`qWarning`), outside the log-level
    /// filter and without touching the progress line state.
    pub fn raw(&mut self, message: &str) {
        self.put(message);
        let _ = self.out.flush();
    }

    pub fn debug(&mut self, message: &str) {
        self.message(LogLevel::Debug, "Debug: ", message);
    }

    pub fn info(&mut self, message: &str) {
        self.message(LogLevel::Info, "Info: ", message);
    }

    pub fn warning(&mut self, message: &str) {
        self.message(LogLevel::Warn, "Warning: ", message);
    }

    pub fn error(&mut self, message: &str) {
        self.message(LogLevel::Error, "Error: ", message);
    }

    /// `phaseChanged`: `desc (current+1/count-1)`, except for the last phase.
    pub fn phase_changed(&mut self, desc: &str, current_phase: usize, phase_count: usize) {
        if self.log_level < LogLevel::Info {
            return;
        }
        self.put(desc);
        let mut l = qsize(desc);
        if current_phase + 1 < phase_count {
            let s = format!(" ({}/{})", current_phase + 1, phase_count - 1);
            l += s.len();
            self.put(&s);
        }
        let pad = self.lw.saturating_sub(l);
        self.put(&" ".repeat(pad));
        self.put("\n");
        self.lw = 0;
        let _ = self.out.flush();
    }

    /// `progressChanged`: a 60-column bar followed by the progress string.
    pub fn progress_changed(&mut self, progress: i32, progress_string: &str) {
        if self.log_level < LogLevel::Info {
            return;
        }
        const W: i32 = 60;
        let p = progress * W / 100;
        let mut bar = String::with_capacity(64);
        bar.push('[');
        for i in 0..W {
            bar.push(match i.cmp(&p) {
                std::cmp::Ordering::Less => '=',
                std::cmp::Ordering::Equal => '>',
                std::cmp::Ordering::Greater => ' ',
            });
        }
        bar.push(']');
        self.put(&bar);
        self.put(" ");
        self.put(progress_string);
        let l = 1 + W as usize + 2 + qsize(progress_string);
        let pad = self.lw.saturating_sub(l);
        self.put(&" ".repeat(pad));
        self.lw = l;
        self.put("\r");
        let _ = self.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_bar_and_phase_line() {
        let mut f = ProgressFeedback::new(LogLevel::Info, Vec::new());
        f.phase_changed("Loading pages", 0, 7);
        f.progress_changed(10, "10%");
        f.phase_changed("Counting pages", 1, 7);
        f.progress_changed(100, "Page 1 of 1");
        f.phase_changed("Done", 6, 7);
        let out = String::from_utf8(f.into_inner()).unwrap();
        let expected = format!(
            "Loading pages (1/6)\n[======>{}] 10%\rCounting pages (2/6){}\n[{}] Page 1 of 1\rDone{}\n",
            " ".repeat(53),
            " ".repeat(66 - 20),
            "=".repeat(60),
            " ".repeat(74 - 4),
        );
        assert_eq!(out, expected);
    }

    #[test]
    fn quiet_prints_nothing() {
        let mut f = ProgressFeedback::new(LogLevel::None, Vec::new());
        f.phase_changed("Loading pages", 0, 7);
        f.error("x");
        assert!(f.into_inner().is_empty());
    }

    #[test]
    fn error_overwrites_bar() {
        let mut f = ProgressFeedback::new(LogLevel::Warn, Vec::new());
        f.warning("Blocked access to file ");
        assert_eq!(f.into_inner(), b"Warning: Blocked access to file \n");
    }
}
