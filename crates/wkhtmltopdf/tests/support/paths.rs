//! Path normalisation for tests that run the binary in a temporary
//! directory and compare its messages with references captured elsewhere
//! (`/wk/conv`, `/wk/js`, …).
//!
//! On Windows the binary prints local files as `file:///C:/…` URLs, the
//! temporary directory may be an 8.3 short name (`C:\Users\RUNNER~1\…`)
//! while its canonical form is the long name with a verbatim prefix
//! (`\\?\C:\Users\runneradmin\…`), so a plain `str::replace` of the
//! directory's display form misses. On Unix the URL is `file://` followed by
//! the path as typed, so these helpers behave like the plain replacement.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// `p` without Windows' verbatim prefix (`\\?\C:\…` → `C:\…`,
/// `\\?\UNC\server\share` → `\\server\share`). Unchanged elsewhere.
pub fn simplified(p: &Path) -> PathBuf {
    if cfg!(windows) {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if let Some(unc) = rest.strip_prefix(r"UNC\") {
                return PathBuf::from(format!(r"\\{unc}"));
            }
            if rest.as_bytes().get(1) == Some(&b':') {
                return PathBuf::from(rest);
            }
        }
    }
    p.to_path_buf()
}

/// The `file://` URL under which the binary prints the local path `p`
/// (`wk_loader::urlguess`): `file:///C:/…` on Windows, `file://` followed by
/// the path as typed elsewhere.
pub fn file_url(p: &Path) -> String {
    if cfg!(windows) {
        if let Ok(u) = url::Url::from_file_path(p) {
            return u.to_string();
        }
    }
    format!("file://{}", p.display())
}

/// The value to substitute for `{DIR}` in a case's arguments: `file://{DIR}`
/// must become a valid file URL (`file:///C:/…` on Windows, not
/// `file://\\?\C:\…`), a bare `{DIR}` a plain path without a verbatim prefix
/// (`\\?\` paths do not accept `/` as a separator).
pub fn substitute_dir(arg: &str, dir: &Path) -> String {
    let plain = simplified(dir);
    arg.replace("file://{DIR}", &file_url(&plain)).replace("{DIR}", &plain.display().to_string())
}

/// Replace every form under which the binary may print the directory `dir`
/// by `with`, an absolute Unix path such as `/wk/conv`: its `file://` URL
/// (becoming `file://<with>`) and its plain path, each for `dir` as given,
/// canonicalised, and without a verbatim prefix.
pub fn replace_dir(s: &str, dir: &Path, with: &str) -> String {
    let mut dirs = vec![dir.to_path_buf(), simplified(dir)];
    if let Ok(c) = std::fs::canonicalize(dir) {
        dirs.push(simplified(&c));
        dirs.push(c);
    }
    let mut forms: Vec<(String, String)> = Vec::new();
    for d in &dirs {
        forms.push((file_url(d), format!("file://{with}")));
        forms.push((d.display().to_string(), with.to_string()));
    }
    // Longest first, so that no form is pre-empted by a shorter one it
    // contains.
    forms.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
    forms.dedup();
    let mut out = s.to_string();
    for (from, to) in forms {
        if !from.is_empty() {
            out = out.replace(&from, &to);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_url_and_path_forms() {
        let dir = std::env::temp_dir().join(format!("wk-paths-selftest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let canon = std::fs::canonicalize(&dir).unwrap();
        let msg = format!(
            "Warning: Failed to load {}/a.css (ignore)\nWarning: {}/b.html:1 x\nopen {}",
            file_url(&dir),
            file_url(&canon),
            simplified(&canon).display()
        );
        let got = replace_dir(&msg, &dir, "/wk/conv");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            got,
            "Warning: Failed to load file:///wk/conv/a.css (ignore)\nWarning: file:///wk/conv/b.html:1 x\nopen /wk/conv"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_forms_are_plain() {
        let d = Path::new("/tmp/x y");
        assert_eq!(file_url(d), "file:///tmp/x y");
        assert_eq!(simplified(d), d);
        assert_eq!(substitute_dir("file://{DIR}/f.html?a=1", d), "file:///tmp/x y/f.html?a=1");
        assert_eq!(substitute_dir("{DIR}/f.html", d), "/tmp/x y/f.html");
    }

    #[cfg(windows)]
    #[test]
    fn windows_forms() {
        let v = Path::new(r"\\?\C:\Users\runneradmin\AppData\Local\Temp\wk-x");
        assert_eq!(simplified(v), Path::new(r"C:\Users\runneradmin\AppData\Local\Temp\wk-x"));
        assert_eq!(file_url(v), "file:///C:/Users/runneradmin/AppData/Local/Temp/wk-x");
        let short = Path::new(r"C:\Users\RUNNER~1\AppData\Local\Temp\wk-x");
        assert_eq!(file_url(short), "file:///C:/Users/RUNNER~1/AppData/Local/Temp/wk-x");
        assert_eq!(
            substitute_dir("file://{DIR}/f.html?page=2&a=b", v),
            "file:///C:/Users/runneradmin/AppData/Local/Temp/wk-x/f.html?page=2&a=b"
        );
        assert_eq!(simplified(Path::new(r"\\?\UNC\srv\share\d")), Path::new(r"\\srv\share\d"));
        let msg = "Warning: file:///C:/Users/RUNNER~1/AppData/Local/Temp/wk-x/a.html:4 hi";
        assert_eq!(replace_dir(msg, short, "/wk/js"), "Warning: file:///wk/js/a.html:4 hi");
    }
}
