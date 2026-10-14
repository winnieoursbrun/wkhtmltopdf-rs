//! L3 parity on the corpus (`tests/corpus`, DESIGN.md §1): every page of our
//! PDF is rasterised next to the same page of the reference PDF made by the
//! real wkhtmltopdf 0.12.6, and compared with a mean SSIM.
//!
//! Method: both PDFs are rendered by hayro (pure Rust, so no Docker or
//! poppler) at [`DPI`] on a white background, converted to gray, and
//! compared with [`common::ssim`]: 8×8 windows, stride 4, windows blank in
//! both images skipped (so empty margins do not inflate the score). The share
//! of pixels differing by more than 32 gray levels is printed as well.
//!
//! Thresholds live in `tests/corpus/l3-thresholds.txt`, one line per page
//! (or `*` for every page of a document) with the reason for any value below
//! the default. They record the current state rounded down, never an
//! aspiration: a regression fails the test, an improvement should raise the
//! line. A page without a threshold line fails.
//!
//! The same check runs on the document-parts cases (`tests/parts/<case>`,
//! run with their `cmd`; thresholds in `tests/parts/l3-thresholds.txt`) and
//! on the JavaScript corpus (`tests/js`, run with `cases.tsv`; thresholds in
//! `tests/js/l3-thresholds.txt`).
//!
//! `L3_DUMP=<dir>` writes `<page>-<n>.ppm` images (reference | ours | diff,
//! reference-only ink in red, ours-only in blue; `L3_DUMP_DPI` sets their
//! resolution) and our PDF `<page>.pdf`, for visual inspection; `L3_ONLY=a,b`
//! restricts the run to some pages.

mod common;
#[path = "support/paths.rs"]
mod paths;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DPI: f32 = 50.0;

/// `(document, page or 0 for every page) -> min SSIM`, read from a
/// threshold file (`<document> <page | *> <min SSIM>`, `#` comments).
fn thresholds(path: &Path) -> HashMap<(String, usize), f64> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 3, "bad threshold line: {line}");
        let page = if f[1] == "*" { 0 } else { f[1].parse().expect("page number") };
        out.insert((f[0].to_string(), page), f[2].parse().expect("ssim"));
    }
    out
}

/// `L3_ONLY` filter.
fn selected(name: &str) -> bool {
    std::env::var("L3_ONLY").ok().is_none_or(|o| o.split(',').any(|p| p == name))
}

/// Compare every page of `got` with `reference`, print one summary line and
/// push the pages under their threshold to `failures`.
fn compare(
    name: &str,
    reference: &[u8],
    got: &[u8],
    th: &HashMap<(String, usize), f64>,
    failures: &mut Vec<String>,
) {
    let (r, g) = (common::raster(reference, DPI), common::raster(got, DPI));
    let mut line = format!("{name}: pages ref {} ours {}; ssim", r.len(), g.len());
    if let Some(dir) = std::env::var_os("L3_DUMP").map(PathBuf::from) {
        std::fs::create_dir_all(&dir).unwrap();
        let dpi = std::env::var("L3_DUMP_DPI").ok().and_then(|v| v.parse().ok()).unwrap_or(DPI);
        let (dr, dg) = (common::raster(reference, dpi), common::raster(got, dpi));
        for i in 0..dr.len().max(dg.len()) {
            common::dump_side_by_side(&dir.join(format!("{name}-{}.ppm", i + 1)), dr.get(i), dg.get(i));
        }
        std::fs::write(dir.join(format!("{name}.pdf")), got).unwrap();
    }
    for i in 0..r.len().max(g.len()) {
        let (s, d) = match (r.get(i), g.get(i)) {
            (Some(a), Some(b)) => (common::ssim(a, b), common::diff_ratio(a, b)),
            _ => (0.0, 1.0),
        };
        line.push_str(&format!(" {s:.3} ({:.1}%)", d * 100.0));
        let min = th.get(&(name.to_string(), i + 1)).or_else(|| th.get(&(name.to_string(), 0)));
        match min {
            None => failures.push(format!("{name} page {}: no threshold (ssim {s:.3})", i + 1)),
            Some(&m) if s < m => failures.push(format!("{name} page {}: ssim {s:.3} < {m}", i + 1)),
            _ => {}
        }
    }
    eprintln!("{line}");
}

#[test]
fn corpus_l3_raster() {
    let th = thresholds(&common::corpus().join("l3-thresholds.txt"));
    let mut failures = Vec::new();
    for name in common::pages() {
        if !selected(&name) {
            continue;
        }
        let html = std::fs::read(common::corpus().join(format!("{name}.html"))).unwrap();
        let Ok(reference) = std::fs::read(common::corpus().join(format!("ref/{name}.pdf"))) else {
            failures.push(format!("{name}: no reference PDF"));
            continue;
        };
        let got = common::ours(&html, &common::page_args(&name));
        compare(&name, &reference, &got, &th, &mut failures);
    }
    assert!(failures.is_empty(), "L3 failures:\n{}", failures.join("\n"));
}

fn tests_dir(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests").join(sub)
}

/// Copy `from` to `to`, skipping `ref*` entries and `cmd`.
fn copy_inputs(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        let n = name.to_string_lossy();
        if n.starts_with("ref") || n == "cmd" || n.starts_with("l3-") {
            continue;
        }
        if e.path().is_dir() {
            copy_inputs(&e.path(), &to.join(&name));
        } else {
            std::fs::copy(e.path(), to.join(&name)).unwrap();
        }
    }
}

/// Run our binary with `args` in a fresh copy of `src` and return `out.pdf`
/// (the last argument), like `parts.rs` and `js_corpus.rs`.
fn run_in_copy(src: &Path, tag: &str, args: impl Fn(&Path) -> Vec<String>) -> Vec<u8> {
    let tmp = std::env::temp_dir().join(format!("wk-l3-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_inputs(src, &tmp);
    let dir = std::fs::canonicalize(&tmp).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(args(&dir))
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .env("WKHTMLTOPDF_SLOW_SCRIPT_LOOPS", "200000")
        .current_dir(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("run wkhtmltopdf");
    let pdf = std::fs::read(dir.join("out.pdf")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(pdf.starts_with(b"%PDF"), "{tag}: no PDF: {}", String::from_utf8_lossy(&o.stderr));
    pdf
}

/// L3 on the document-parts cases (`tests/parts/<case>/ref.pdf`),
/// thresholds in `tests/parts/l3-thresholds.txt`.
#[test]
fn parts_l3_raster() {
    let root = tests_dir("parts");
    let th = thresholds(&root.join("l3-thresholds.txt"));
    let mut cases: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("ref.pdf").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    cases.sort();
    let mut failures = Vec::new();
    for case in cases.into_iter().filter(|c| selected(c)) {
        let src = root.join(&case);
        let args: Vec<String> = std::fs::read_to_string(src.join("cmd"))
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        let got = run_in_copy(&src, &case, |_| args.clone());
        let reference = std::fs::read(src.join("ref.pdf")).unwrap();
        compare(&case, &reference, &got, &th, &mut failures);
    }
    assert!(failures.is_empty(), "L3 failures:\n{}", failures.join("\n"));
}

/// L3 on the JavaScript corpus (`tests/js/ref/<case>.pdf`, arguments in
/// `cases.tsv`), thresholds in `tests/js/l3-thresholds.txt`.
#[test]
fn js_l3_raster() {
    let root = tests_dir("js");
    let th = thresholds(&root.join("l3-thresholds.txt"));
    let cases = std::fs::read_to_string(root.join("cases.tsv")).unwrap();
    let mut failures = Vec::new();
    for line in cases.lines().filter(|l| !l.trim().is_empty()) {
        let mut f = line.split('\t');
        let case = f.next().unwrap().to_string();
        if !selected(&case) {
            continue;
        }
        let raw: Vec<String> = f.map(str::to_string).collect();
        let got = run_in_copy(&root, &case, |dir| {
            let mut v: Vec<String> = raw.iter().map(|a| paths::substitute_dir(a, dir)).collect();
            v.push("out.pdf".into());
            v
        });
        let reference = std::fs::read(root.join(format!("ref/{case}.pdf"))).unwrap();
        compare(&case, &reference, &got, &th, &mut failures);
    }
    assert!(failures.is_empty(), "L3 failures:\n{}", failures.join("\n"));
}

#[test]
fn ssim_of_identical_pages_is_one() {
    let g = common::Gray {
        w: 16,
        h: 16,
        px: (0..256).map(|i| (i % 255) as u8).collect(),
        rgb: vec![[0; 3]; 256],
    };
    assert!((common::ssim(&g, &g) - 1.0).abs() < 1e-9);
    let blank = common::Gray { w: 16, h: 16, px: vec![255; 256], rgb: vec![[255; 3]; 256] };
    assert!(common::ssim(&g, &blank) < 0.5);
    assert_eq!(common::ssim(&blank, &blank), 1.0);
}
