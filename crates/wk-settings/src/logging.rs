//! Log levels. Ports upstream `src/lib/logging.hh` and `src/lib/logging.cc`.
//!
//! Target is the 0.12.6 release: its level map has no `debug` entry, so
//! `--log-level debug` is rejected (spec §9). The `Debug` variant is kept for
//! the 0.12.7-dev progress feedback code but cannot be selected by name.

/// `wkhtmltopdf::settings::LogLevel`, ordered from quietest to most verbose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum LogLevel {
    None,
    Error,
    Warn,
    #[default]
    Info,
    Debug,
}

/// The `logLevels` map of 0.12.6, in `QMap` (sorted key) order.
const LOG_LEVELS: &[(&str, LogLevel)] = &[
    ("error", LogLevel::Error),
    ("info", LogLevel::Info),
    ("none", LogLevel::None),
    ("warn", LogLevel::Warn),
];

/// `strToLogLevel`: case-insensitive lookup. Unknown names give `Info` and
/// `ok == false`.
pub fn str_to_log_level(s: &str) -> (LogLevel, bool) {
    for (k, v) in LOG_LEVELS {
        if k.eq_ignore_ascii_case(s) {
            return (*v, true);
        }
    }
    (LogLevel::Info, false)
}

/// `logLevelToStr`: `None` when the level has no name.
pub fn log_level_to_str(l: LogLevel) -> Option<&'static str> {
    LOG_LEVELS.iter().find(|(_, v)| *v == l).map(|(k, _)| *k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_is_case_insensitive() {
        assert_eq!(str_to_log_level("WARN"), (LogLevel::Warn, true));
        assert_eq!(str_to_log_level("none"), (LogLevel::None, true));
    }

    #[test]
    fn log_level_debug_rejected_in_0_12_6() {
        assert_eq!(str_to_log_level("debug"), (LogLevel::Info, false));
        assert_eq!(log_level_to_str(LogLevel::Debug), None);
    }
}
