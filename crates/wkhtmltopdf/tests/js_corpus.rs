//! L2 parity on the JavaScript corpus (`tests/js`, DESIGN.md §1, phase P4):
//! for each case of `tests/js/cases.tsv`, our PDF, stderr and exit code are
//! compared with the outputs of the real wkhtmltopdf 0.12.6 (`tests/js/ref/`,
//! regenerated with `tests/js/make-refs.sh`).
//!
//! Checks: the exit code and stderr exactly (the corpus runs with
//! `--log-level warn`, so stderr holds only warnings: dialogs and
//! `--debug-javascript` console lines), the page count and sizes, and a
//! word-level text similarity of at least [`MIN_SIMILARITY`] for the whole
//! document and for every page (same metric as `corpus_l2.rs`).
//!
//! `slow` stops a `while (true)` loop: the slow-script budget is lowered
//! with `WKHTMLTOPDF_SLOW_SCRIPT_LOOPS` so the test does not spend the
//! default ~10 s budget (QtWebKit's interrupt fires after about 10 s too).
//!
//! Cases run in a fresh copy of `tests/js`; its path is replaced by
//! `/wk/js` (where the references were made) in stderr. `TZ=UTC`, like the
//! reference container.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use support::paths;
use support::pdftext::{page_sizes, ratio, words_by_page};

const MIN_SIMILARITY: f64 = 0.95;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/js")
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        let name = e.file_name();
        if p.is_dir() {
            if name != "ref" {
                copy_dir(&p, &to.join(&name));
            }
        } else {
            fs::copy(&p, to.join(&name)).unwrap();
        }
    }
}

/// The arguments of `case` in `cases.tsv`, `{DIR}` resolved to `dir`.
fn case_args(case: &str, dir: &Path) -> Vec<String> {
    let cases = fs::read_to_string(corpus().join("cases.tsv")).unwrap();
    for line in cases.lines() {
        let mut parts = line.split('\t');
        if parts.next() == Some(case) {
            let mut v: Vec<String> = parts.map(|a| paths::substitute_dir(a, dir)).collect();
            v.push("out.pdf".into());
            return v;
        }
    }
    panic!("no case {case} in cases.tsv");
}

fn check(case: &str) {
    check_env(case, &[]);
}

fn check_env(case: &str, env: &[(&str, &str)]) {
    let dir = std::env::temp_dir().join(format!("wk-js-corpus-{case}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&corpus(), &dir);
    let canon = fs::canonicalize(&dir).unwrap();
    let args = case_args(case, &canon);
    let o = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(&args)
        .current_dir(&dir)
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .envs(env.iter().copied())
        .output()
        .expect("run wkhtmltopdf");
    let stderr = paths::replace_dir(&String::from_utf8_lossy(&o.stderr), &dir, "/wk/js");
    let got = fs::read(dir.join("out.pdf")).unwrap_or_default();
    let _ = fs::remove_dir_all(&dir);

    let ref_dir = corpus().join("ref");
    let want_stderr = fs::read_to_string(ref_dir.join(format!("{case}.stderr"))).unwrap();
    let want_rc: i32 =
        fs::read_to_string(ref_dir.join(format!("{case}.rc"))).unwrap().trim().parse().unwrap();
    assert_eq!(stderr, want_stderr, "{case}: stderr");
    assert_eq!(o.status.code(), Some(want_rc), "{case}: exit code");
    assert!(got.starts_with(b"%PDF"), "{case}: no PDF");

    let reference = fs::read(ref_dir.join(format!("{case}.pdf"))).unwrap();
    let (rs, gs) = (page_sizes(&reference), page_sizes(&got));
    let (rw, gw) = (words_by_page(&reference), words_by_page(&got));
    let doc = ratio(&rw.concat(), &gw.concat());
    let per: Vec<f64> = (0..rw.len().max(gw.len()))
        .map(|i| ratio(rw.get(i).map_or(&[][..], |v| v), gw.get(i).map_or(&[][..], |v| v)))
        .collect();
    let summary = format!(
        "{case}: pages ref {} ours {}; doc similarity {doc:.3}; per page {:?}",
        rs.len(),
        gs.len(),
        per.iter().map(|p| format!("{p:.2}")).collect::<Vec<_>>()
    );
    eprintln!("{summary}");
    if std::env::var_os("L2_DEBUG").is_some() {
        for (i, (r, g)) in rw.iter().zip(&gw).enumerate() {
            eprintln!("--- page {} ref: {}\n--- page {} ours: {}", i + 1, r.join(" "), i + 1, g.join(" "));
        }
    }
    assert_eq!(rs.len(), gs.len(), "page count\n{summary}");
    assert_eq!(rs, gs, "page sizes\n{summary}");
    assert!(doc >= MIN_SIMILARITY, "document text similarity\n{summary}");
    assert!(per.iter().all(|&p| p >= MIN_SIMILARITY), "per-page text similarity\n{summary}");
}

#[test]
fn js_footer_substitution() {
    check("footer-subst");
}

#[test]
fn js_jquery_3_7_1() {
    check("jquery3");
}

#[test]
fn js_jquery_1_12_4() {
    check("jquery1");
}

#[test]
fn js_chartjs_2_9_4_bar() {
    check("chart2");
}

#[test]
fn js_window_status_timer_chain() {
    check("window-status");
}

#[test]
fn js_document_write() {
    check("docwrite");
}

#[test]
fn js_run_script() {
    check("runscript");
}

#[test]
fn js_debug_javascript_output() {
    check("debugjs");
}

#[test]
fn js_window_print_ends_the_wait() {
    check("print");
}

#[test]
fn js_disabled() {
    check("nojs");
}

#[test]
fn js_xhr_and_inserted_scripts() {
    check("xhr");
}

#[test]
fn js_canvas_2d() {
    check("canvas");
}

#[test]
fn js_slow_script_is_stopped() {
    check_env("slow", &[("WKHTMLTOPDF_SLOW_SCRIPT_LOOPS", "200000")]);
}
