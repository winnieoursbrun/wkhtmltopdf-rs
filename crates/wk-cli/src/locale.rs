//! Locale detection standing in for `setlocale(LC_ALL, "")` +
//! `QTextCodec::codecForLocale()` in upstream `src/pdf/wkhtmltopdf.cc`.

/// True when the effective `LC_CTYPE` (from `LC_ALL`, `LC_CTYPE`, then
/// `LANG`) names a UTF-8 codeset.
pub fn env_is_utf8() -> bool {
    let lookup = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let value = lookup("LC_ALL").or_else(|| lookup("LC_CTYPE")).or_else(|| lookup("LANG"));
    value.is_some_and(|v| locale_is_utf8(&v))
}

/// True when a locale name such as `fr_FR.UTF-8` or `C.utf8` uses UTF-8.
pub fn locale_is_utf8(name: &str) -> bool {
    let codeset = name.split_once('.').map(|(_, c)| c).unwrap_or("");
    let codeset = codeset.split('@').next().unwrap_or("");
    let norm: String = codeset.chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase();
    norm == "utf8"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_names() {
        assert!(locale_is_utf8("fr_FR.UTF-8"));
        assert!(locale_is_utf8("C.utf8"));
        assert!(locale_is_utf8("en_US.UTF-8@euro"));
        assert!(!locale_is_utf8("C"));
        assert!(!locale_is_utf8("POSIX"));
        assert!(!locale_is_utf8("de_DE.ISO-8859-1"));
    }
}
