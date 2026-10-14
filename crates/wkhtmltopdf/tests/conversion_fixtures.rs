//! Replays the conversion cases of
//! `docs/upstream-fixtures/0.12.6/conversion/run.sh` against the built
//! binary: stderr and exit code must match what the real wkhtmltopdf 0.12.6
//! printed (spec §4).
//!
//! Inputs. `a.html` is the fixture's. The fixture's `b.html` is not usable:
//! `run.sh` captures `twoout a.html b.html` (quirk §10.2: the second input
//! silently becomes the output) *before* `rs`, which overwrote the original
//! `b.html` with a PDF. `tests/conversion/b.html` is a reconstruction (title
//! `Doc B`, `<h1>B</h1>`, a missing stylesheet, a missing script, an inline
//! script logging `hello console` and alerting `hi` on line 1, a missing
//! image). Fed to the reference binary (`wkref:local`), it reproduces
//! `info_b.stderr` byte for byte, and `warn.stderr` except that the
//! `Javascript alert` line comes before the image's `Blocked access` line
//! (a load-order race inside QtWebKit; both orders are deterministic per
//! input). The cases run in `run.sh` order in one directory, so `twoout`
//! overwrites `b.html` before `rs` loads it, like the capture did.
//!
//! Normalisation. Paths: the capture ran in `/wk/conv`; the working
//! directory's path is replaced by `/wk/conv`, and since a message padded
//! after a progress bar depends on its length, the padding of every line is
//! then recomputed with `ProgressFeedback`'s rule on both sides (`repad`).
//! Progress percentages: our loader is deterministic and reproduces the
//! captured sequences, so they are compared as is.
//!
//! Known gaps, tested separately:
//! - `warn` and `info_b` contain JavaScript output (`Javascript alert`,
//!   console messages): JavaScript is P4. They are compared exactly in an
//!   ignored test, and without the JavaScript lines in a regular one.
//! - `toc` (with `--dump-outline`) loads `b.html`, whose script alerts:
//!   compared without the JavaScript line (and its `outline.xml` byte for
//!   byte) in a regular test, exactly in an ignored one.
//!
//! `missing_page*` and `badhost` need name resolution to fail for
//! `nonexist.html` / `nonexistent.invalid` (true without network too).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/upstream-fixtures/0.12.6/conversion")
}

fn inputs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conversion")
}

/// `(name, args, stdin)` in `run.sh` order, for the cases with a committed
/// fixture.
fn cases() -> Vec<(&'static str, Vec<&'static str>, Option<&'static str>)> {
    vec![
        ("basic", vec!["a.html", "basic.pdf"], None),
        ("quiet", vec!["-q", "a.html", "quiet.pdf"], None),
        ("warn", vec!["--log-level", "warn", "b.html", "warn.pdf"], None),
        ("info_b", vec!["--debug-javascript", "--enable-local-file-access", "b.html", "info_b.pdf"], None),
        ("missing_page", vec!["nonexist.html", "out.pdf"], None),
        ("missing_page_ignore", vec!["--load-error-handling", "ignore", "nonexist.html", "out2.pdf"], None),
        (
            "missing_page_skip",
            vec!["a.html", "--load-error-handling", "skip", "nonexist.html", "out3.pdf"],
            None,
        ),
        ("toc", vec!["--dump-outline", "outline.xml", "toc", "a.html", "b.html", "toc.pdf"], None),
        ("badhost", vec!["http://nonexistent.invalid/", "net.pdf"], None),
        ("unwritable", vec!["a.html", "/nonexistent/dir/x.pdf"], None),
        ("twoout", vec!["a.html", "b.html"], None),
        ("marginmix", vec!["-T", "1cm", "-B", "10mm", "a.html", "mm.pdf"], None),
        ("rs", vec!["--read-args-from-stdin"], Some("a.html rs1.pdf\n-q b.html rs2.pdf\n")),
    ]
}

struct Outcome {
    name: &'static str,
    stderr: String,
    rc: i32,
    want_stderr: String,
    want_rc: i32,
    /// `outline.xml` after the case (written by `toc`'s `--dump-outline`).
    outline: Option<String>,
}

/// Run every case once, in order, in a fresh directory.
fn run_all() -> Vec<Outcome> {
    let dir = std::env::temp_dir().join(format!(
        "wk-conv-fixtures-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixtures_dir().join("a.html"), dir.join("a.html")).unwrap();
    fs::copy(inputs_dir().join("b.html"), dir.join("b.html")).unwrap();
    let canon = fs::canonicalize(&dir).unwrap();
    let mut out = Vec::new();
    for (name, args, stdin) in cases() {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"));
        cmd.args(&args)
            .env("LC_ALL", "C")
            .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
            .current_dir(&dir)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("run wkhtmltopdf");
        if let Some(input) = stdin {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        }
        let o = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&o.stderr)
            .replace(&canon.display().to_string(), "/wk/conv")
            .replace(&dir.display().to_string(), "/wk/conv");
        let want_stderr = fs::read_to_string(fixtures_dir().join(format!("{name}.stderr"))).unwrap();
        let want_rc =
            fs::read_to_string(fixtures_dir().join(format!("{name}.rc"))).unwrap().trim().parse().unwrap();
        let outline = fs::read_to_string(dir.join("outline.xml")).ok();
        out.push(Outcome {
            name,
            stderr,
            rc: o.status.code().expect("exit code"),
            want_stderr,
            want_rc,
            outline,
        });
    }
    let _ = fs::remove_dir_all(&dir);
    out
}

/// Recompute line padding like `ProgressFeedback`: a line ended by `\n`
/// is padded with spaces to the width of the progress bar drawn just before
/// it (`lw`), which is the length of that `\r`-terminated segment.
fn repad(s: &str) -> String {
    let mut out = String::new();
    let mut lw = 0usize;
    let mut seg = String::new();
    for c in s.chars() {
        match c {
            '\r' => {
                let t = seg.trim_end_matches(' ');
                // A bar is padded to the previous bar's width.
                let width = t.chars().count();
                out.push_str(t);
                out.push_str(&" ".repeat(lw.saturating_sub(width)));
                out.push('\r');
                lw = width;
                seg.clear();
            }
            '\n' => {
                let t = seg.trim_end_matches(' ');
                let width = t.chars().count();
                out.push_str(t);
                out.push_str(&" ".repeat(lw.saturating_sub(width)));
                out.push('\n');
                lw = 0;
                seg.clear();
            }
            c => seg.push(c),
        }
    }
    out.push_str(&seg);
    out
}

fn diff(o: &Outcome, want: &str) -> Option<String> {
    let mut msgs = Vec::new();
    let got = repad(&o.stderr);
    let want = repad(want);
    let want = want.as_str();
    let o = &Outcome {
        name: o.name,
        stderr: got,
        rc: o.rc,
        want_stderr: String::new(),
        want_rc: o.want_rc,
        outline: None,
    };
    if o.stderr != want {
        let at = o
            .stderr
            .bytes()
            .zip(want.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(o.stderr.len().min(want.len()));
        let lo = at.saturating_sub(80);
        msgs.push(format!(
            "{}: stderr differs at byte {at}\n  got:  {:?}\n  want: {:?}",
            o.name,
            o.stderr.get(lo..(at + 120).min(o.stderr.len())).unwrap_or(""),
            want.get(lo..(at + 120).min(want.len())).unwrap_or("")
        ));
    }
    if o.rc != o.want_rc {
        msgs.push(format!("{}: exit code {}, want {}", o.name, o.rc, o.want_rc));
    }
    if msgs.is_empty() { None } else { Some(msgs.join("\n")) }
}

/// Lines produced by JavaScript (P4).
fn is_js_line(l: &str) -> bool {
    l.starts_with("Warning: Javascript ") || l.starts_with("Warning: file:///wk/conv/b.html:")
}

fn without_js(s: &str) -> String {
    s.split_inclusive('\n').filter(|l| !is_js_line(l)).collect()
}

const JS_CASES: &[&str] = &["warn", "info_b"];
const P3_CASES: &[&str] = &["toc"];

#[test]
fn conversion_fixtures_reproduce_stderr_and_exit_code() {
    let all = run_all();
    let failures: Vec<String> = all
        .iter()
        .filter(|o| !JS_CASES.contains(&o.name) && !P3_CASES.contains(&o.name))
        .filter_map(|o| diff(o, &o.want_stderr))
        .collect();
    assert_eq!(all.len(), 13);
    assert!(failures.is_empty(), "{} mismatch(es):\n{}", failures.len(), failures.join("\n"));
}

/// `warn` and `info_b` with the JavaScript-produced lines removed from the
/// expected output (JavaScript does not run before P4); everything else —
/// blocked files, load errors, their order, padding and exit codes — must
/// match exactly.
#[test]
fn js_fixtures_match_without_javascript_lines() {
    let all = run_all();
    let failures: Vec<String> = all
        .iter()
        .filter(|o| JS_CASES.contains(&o.name))
        .filter_map(|o| diff(o, &without_js(&o.want_stderr)))
        .collect();
    assert!(failures.is_empty(), "{} mismatch(es):\n{}", failures.len(), failures.join("\n"));
}

#[test]
#[ignore = "needs JavaScript (P4): alert and console output"]
fn js_fixtures_exact() {
    let all = run_all();
    let failures: Vec<String> =
        all.iter().filter(|o| JS_CASES.contains(&o.name)).filter_map(|o| diff(o, &o.want_stderr)).collect();
    assert!(failures.is_empty(), "{} mismatch(es):\n{}", failures.len(), failures.join("\n"));
}

/// `toc`: stderr without the `Javascript alert` line of `b.html` (P4), exit
/// code, and the `--dump-outline` file byte for byte (`outline.xml`).
#[test]
fn toc_fixture_without_javascript_lines() {
    let all = run_all();
    let mut failures: Vec<String> = all
        .iter()
        .filter(|o| P3_CASES.contains(&o.name))
        .filter_map(|o| diff(o, &without_js(&o.want_stderr)))
        .collect();
    let want = fs::read_to_string(fixtures_dir().join("outline.xml")).unwrap();
    let toc = all.iter().find(|o| o.name == "toc").unwrap();
    if toc.outline.as_deref() != Some(want.as_str()) {
        failures.push(format!(
            "toc: outline.xml\n--- want\n{want}--- got\n{}",
            toc.outline.as_deref().unwrap_or("")
        ));
    }
    assert!(failures.is_empty(), "{} mismatch(es):\n{}", failures.len(), failures.join("\n"));
}

#[test]
#[ignore = "needs JavaScript (P4): b.html alerts"]
fn toc_fixture_exact() {
    let all = run_all();
    let failures: Vec<String> =
        all.iter().filter(|o| P3_CASES.contains(&o.name)).filter_map(|o| diff(o, &o.want_stderr)).collect();
    assert!(failures.is_empty(), "{} mismatch(es):\n{}", failures.len(), failures.join("\n"));
}

/// pdfkit's invocation: HTML on stdin, PDF on stdout, nothing on stderr.
#[test]
fn pdfkit_default_invocation() {
    let html = fs::read(fixtures_dir().join("a.html")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args([
            "--quiet",
            "--page-size",
            "Letter",
            "--margin-top",
            "0.75in",
            "--margin-right",
            "0.75in",
            "--margin-bottom",
            "0.75in",
            "--margin-left",
            "0.75in",
            "--encoding",
            "UTF-8",
            "-",
            "-",
        ])
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&html).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(o.stderr.is_empty(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(o.stdout.starts_with(b"%PDF-1.4"));
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("MediaBox[0 0 612 792]"), "Letter page size");
}

/// pdfkit relies on this: stdin HTML referencing a relative file is loaded
/// from a temp file whose directory is blocked, so the PDF is produced and
/// the exit code is 1 (spec §7.4.2).
#[test]
fn pdfkit_relative_asset_is_blocked_and_exits_1() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(["--quiet", "-", "-"])
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"<html><body><p>x</p><iframe src=\"x.html\"></iframe><link rel=stylesheet href=\"app.css\"></body></html>").unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "Exit with code 1 due to network error: ProtocolUnknownError\n"
    );
    assert!(o.stdout.starts_with(b"%PDF"));
}
