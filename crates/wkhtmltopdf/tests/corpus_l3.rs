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
//! `L3_DUMP=<dir>` writes `<page>-<n>.ppm` images (reference | ours | diff,
//! reference-only ink in red, ours-only in blue; `L3_DUMP_DPI` sets their
//! resolution) and our PDF `<page>.pdf`, for visual inspection; `L3_ONLY=a,b`
//! restricts the run to some pages.

mod common;

use std::collections::HashMap;

const DPI: f32 = 50.0;

/// `(document, page or 0 for every page) -> min SSIM`.
fn thresholds() -> HashMap<(String, usize), f64> {
    let text =
        std::fs::read_to_string(common::corpus().join("l3-thresholds.txt")).expect("l3-thresholds.txt");
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

#[test]
fn corpus_l3_raster() {
    let th = thresholds();
    let dump = std::env::var_os("L3_DUMP").map(std::path::PathBuf::from);
    let mut failures = Vec::new();
    let only = std::env::var("L3_ONLY").ok();
    for name in common::pages() {
        if only.as_deref().is_some_and(|o| !o.split(',').any(|p| p == name)) {
            continue;
        }
        let html = std::fs::read(common::corpus().join(format!("{name}.html"))).unwrap();
        let Ok(reference) = std::fs::read(common::corpus().join(format!("ref/{name}.pdf"))) else {
            failures.push(format!("{name}: no reference PDF"));
            continue;
        };
        let got = common::ours(&html, &common::page_args(&name));
        let (r, g) = (common::raster(&reference, DPI), common::raster(&got, DPI));
        let mut line = format!("{name}: pages ref {} ours {}; ssim", r.len(), g.len());
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).unwrap();
            let dpi = std::env::var("L3_DUMP_DPI").ok().and_then(|v| v.parse().ok()).unwrap_or(DPI);
            let (dr, dg) = (common::raster(&reference, dpi), common::raster(&got, dpi));
            for i in 0..dr.len().max(dg.len()) {
                common::dump_side_by_side(&dir.join(format!("{name}-{}.ppm", i + 1)), dr.get(i), dg.get(i));
            }
            std::fs::write(dir.join(format!("{name}.pdf")), &got).unwrap();
        }
        for i in 0..r.len().max(g.len()) {
            let (a, b) = (r.get(i), g.get(i));
            let (s, d) = match (a, b) {
                (Some(a), Some(b)) => (common::ssim(a, b), common::diff_ratio(a, b)),
                _ => (0.0, 1.0),
            };
            line.push_str(&format!(" {s:.3} ({:.1}%)", d * 100.0));
            let min = th.get(&(name.clone(), i + 1)).or_else(|| th.get(&(name.clone(), 0)));
            match min {
                None => failures.push(format!("{name} page {}: no threshold (ssim {s:.3})", i + 1)),
                Some(&m) if s < m => failures.push(format!("{name} page {}: ssim {s:.3} < {m}", i + 1)),
                _ => {}
            }
        }
        eprintln!("{line}");
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
