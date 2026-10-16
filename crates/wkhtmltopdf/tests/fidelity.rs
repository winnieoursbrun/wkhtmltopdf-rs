//! Rendering fidelity against the real wkhtmltopdf 0.12.6 (`tests/fidelity`):
//! ~80 small, focused cases and realistic documents, each converted by our
//! binary and compared page by page with the reference PDF made in
//! `wkref:local` by `tests/fidelity/make-refs.sh`.
//!
//! Per page: page size, text (words in reading order, like `pdftotext`,
//! compared with a word-level LCS ratio), mean SSIM at [`DPI`] (hayro on a
//! white background, gray, 8×8 windows with stride 4, windows blank in both
//! skipped: `common::ssim`), the share of pixels differing by more than 32
//! gray levels, and the global offset of our ink against the reference's
//! (from row and column ink profiles), with the SSIM once that offset is
//! compensated.
//!
//! The regular run checks every page against `tests/fidelity/thresholds.txt`
//! (`<case> <page | *> <min SSIM>`), which records the current state rounded
//! down: a regression fails, an improvement should raise the line. A page
//! without a line fails; missing pages count as SSIM 0.
//!
//! Environment:
//! - `FIDELITY_ONLY=case1,case2` restricts the run to some cases;
//! - `FIDELITY_REPORT=1` writes `tests/fidelity/REPORT.md` (worst first) and,
//!   for every page, `ref | ours | diff` PNGs (reference-only ink in red,
//!   ours-only in blue) and our PDF into `tests/fidelity/out/` (gitignored;
//!   `FIDELITY_OUT=<dir>` moves it, `FIDELITY_DUMP_DPI` sets the PNG
//!   resolution, default [`DPI`]). With `FIDELITY_ONLY` the report is not
//!   written, only the images;
//! - `FIDELITY_WRITE_THRESHOLDS=1` rewrites `thresholds.txt` from the run:
//!   each page gets `max(current line, measured SSIM rounded down - 0.01)`,
//!   so it only ever raises a line (new cases get their measured value).
//!
//! Quick loop on one case:
//! `FIDELITY_REPORT=1 FIDELITY_ONLY=tables-spans cargo test --test fidelity -- --nocapture`.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Resolution of the comparison (higher than the L3 test's 50 dpi).
const DPI: f32 = 96.0;

/// pdfkit's options, `--log-level warn` instead of `--quiet` and local file
/// access enabled (same list as `make-refs.sh`).
const DEFAULTS: &[&str] = &[
    "--log-level",
    "warn",
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
    "--enable-local-file-access",
];

/// Largest offset searched by [`offset`], in pixels at [`DPI`].
const MAX_SHIFT: isize = 24;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fidelity")
}

struct Case {
    name: String,
    input: String,
    extra: Vec<String>,
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string(root().join("cases.tsv")).expect("cases.tsv");
    let only: Option<Vec<String>> =
        std::env::var("FIDELITY_ONLY").ok().map(|o| o.split(',').map(|s| s.trim().to_string()).collect());
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut f = l.split('\t');
            let name = f.next().unwrap().to_string();
            let input = f.next().expect("input column").to_string();
            Case { name, input, extra: f.map(str::to_string).collect() }
        })
        .filter(|c| only.as_ref().is_none_or(|o| o.contains(&c.name)))
        .collect()
}

/// The full argument list of a case, without the output file.
fn args(case: &Case) -> Vec<String> {
    let mut v: Vec<String> = DEFAULTS.iter().map(|s| s.to_string()).collect();
    let mut placed = false;
    for a in &case.extra {
        if a == "{IN}" {
            v.push(case.input.clone());
            placed = true;
        } else {
            v.push(a.clone());
        }
    }
    if !placed {
        v.push(case.input.clone());
    }
    v
}

/// Our PDF for `case`, run from `tests/fidelity/cases` like the reference.
fn ours(case: &Case) -> Vec<u8> {
    let out = std::env::temp_dir().join(format!("wk-fidelity-{}-{}.pdf", std::process::id(), case.name));
    let _ = std::fs::remove_file(&out);
    let o = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(args(case))
        .arg(&out)
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("SOURCE_DATE_EPOCH", "1791990835")
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .current_dir(root().join("cases"))
        .stdin(Stdio::null())
        .output()
        .expect("run wkhtmltopdf");
    let pdf = std::fs::read(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    assert!(pdf.starts_with(b"%PDF"), "{}: no PDF: {}", case.name, String::from_utf8_lossy(&o.stderr));
    pdf
}

/// Ink (255 - gray) summed per row and per column.
fn profiles(g: &common::Gray) -> (Vec<f64>, Vec<f64>) {
    let mut rows = vec![0.0; g.h];
    let mut cols = vec![0.0; g.w];
    for (row, line) in rows.iter_mut().zip(g.px.chunks(g.w)) {
        for (col, &p) in cols.iter_mut().zip(line) {
            let ink = f64::from(255 - p);
            *row += ink;
            *col += ink;
        }
    }
    (rows, cols)
}

/// The shift `d` (|d| ≤ [`MAX_SHIFT`]) minimising the mean absolute
/// difference between `a[i]` and `b[i + d]` over their overlap.
fn best_shift(a: &[f64], b: &[f64]) -> isize {
    let n = a.len().min(b.len()) as isize;
    let mut best: (f64, isize) = (f64::INFINITY, 0);
    for d in -MAX_SHIFT..=MAX_SHIFT {
        let (mut sum, mut cnt) = (0.0, 0usize);
        for i in 0..n {
            let j = i + d;
            if j < 0 || j >= n {
                continue;
            }
            sum += (a[i as usize] - b[j as usize]).abs();
            cnt += 1;
        }
        let score = sum / cnt.max(1) as f64;
        // Prefer the smallest shift among equal scores.
        if score < best.0 - 1e-9 || ((score - best.0).abs() <= 1e-9 && d.abs() < best.1.abs()) {
            best = (score, d);
        }
    }
    best.1
}

/// Estimated global offset `(dx, dy)` of our page against the reference, in
/// pixels at [`DPI`] (positive: ours is right of / below the reference).
fn offset(r: &common::Gray, g: &common::Gray) -> (isize, isize) {
    let (rr, rc) = profiles(r);
    let (gr, gc) = profiles(g);
    (best_shift(&rc, &gc), best_shift(&rr, &gr))
}

/// `g` moved by `(-dx, -dy)` (white where uncovered), to compensate an offset.
fn shifted(g: &common::Gray, dx: isize, dy: isize) -> common::Gray {
    let mut px = vec![255u8; g.px.len()];
    for y in 0..g.h as isize {
        let sy = y + dy;
        if sy < 0 || sy >= g.h as isize {
            continue;
        }
        for x in 0..g.w as isize {
            let sx = x + dx;
            if sx >= 0 && sx < g.w as isize {
                px[(y as usize) * g.w + x as usize] = g.px[(sy as usize) * g.w + sx as usize];
            }
        }
    }
    common::Gray { w: g.w, h: g.h, px, rgb: Vec::new() }
}

struct PageScore {
    ssim: f64,
    diff: f64,
    text: f64,
    size_ok: bool,
    ref_size: Option<(i64, i64)>,
    our_size: Option<(i64, i64)>,
    offset: (isize, isize),
    ssim_shifted: f64,
}

struct CaseScore {
    name: String,
    args: String,
    ref_pages: usize,
    our_pages: usize,
    pages: Vec<PageScore>,
    ref_rc: String,
}

impl CaseScore {
    fn mean(&self) -> f64 {
        self.pages.iter().map(|p| p.ssim).sum::<f64>() / self.pages.len().max(1) as f64
    }
    fn min(&self) -> f64 {
        self.pages.iter().map(|p| p.ssim).fold(1.0, f64::min)
    }
    /// Lowest text similarity of the pages (NaN pages ignored; NaN if none).
    fn text(&self) -> f64 {
        self.pages.iter().map(|p| p.text).filter(|t| !t.is_nan()).reduce(f64::min).unwrap_or(f64::NAN)
    }
}

fn out_dir() -> PathBuf {
    std::env::var_os("FIDELITY_OUT").map(PathBuf::from).unwrap_or_else(|| root().join("out"))
}

fn score(case: &Case, dump: bool) -> CaseScore {
    let reference = std::fs::read(root().join(format!("ref/{}.pdf", case.name)))
        .unwrap_or_else(|e| panic!("{}: no reference PDF ({e}); run make-refs.sh", case.name));
    let got = ours(case);
    let (r, g) = (common::raster(&reference, DPI), common::raster(&got, DPI));
    let (rs, gs) = (common::page_sizes(&reference), common::page_sizes(&got));
    // pdf-extract panics on some content streams: the text score is then unknown (NaN).
    let words = |pdf: &[u8]| std::panic::catch_unwind(|| common::words_by_page(pdf)).ok();
    let (rw, gw) = (words(&reference), words(&got));
    if dump {
        let dir = out_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let dpi = std::env::var("FIDELITY_DUMP_DPI").ok().and_then(|v| v.parse().ok()).unwrap_or(DPI);
        let (dr, dg) = if dpi == DPI {
            (None, None)
        } else {
            (Some(common::raster(&reference, dpi)), Some(common::raster(&got, dpi)))
        };
        let (dr, dg) = (dr.as_ref().unwrap_or(&r), dg.as_ref().unwrap_or(&g));
        for i in 0..dr.len().max(dg.len()) {
            let (w, h, rgb) = common::side_by_side(dr.get(i), dg.get(i));
            let path = dir.join(format!("{}-{}.png", case.name, i + 1));
            image::save_buffer(&path, &rgb, w as u32, h as u32, image::ExtendedColorType::Rgb8)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
        std::fs::write(dir.join(format!("{}.pdf", case.name)), &got).unwrap();
    }
    let empty = Vec::new();
    let pages = (0..r.len().max(g.len()))
        .map(|i| {
            let text = match (&rw, &gw) {
                (Some(rw), Some(gw)) => {
                    common::ratio(rw.get(i).unwrap_or(&empty), gw.get(i).unwrap_or(&empty))
                }
                _ => f64::NAN,
            };
            let (ref_size, our_size) = (rs.get(i).copied(), gs.get(i).copied());
            match (r.get(i), g.get(i)) {
                (Some(a), Some(b)) => {
                    let ssim = common::ssim(a, b);
                    let offset = offset(a, b);
                    let ssim_shifted = if offset == (0, 0) {
                        ssim
                    } else {
                        common::ssim(a, &shifted(b, offset.0, offset.1))
                    };
                    PageScore {
                        ssim,
                        diff: common::diff_ratio(a, b),
                        text,
                        size_ok: ref_size == our_size,
                        ref_size,
                        our_size,
                        offset,
                        ssim_shifted,
                    }
                }
                _ => PageScore {
                    ssim: 0.0,
                    diff: 1.0,
                    text,
                    size_ok: false,
                    ref_size,
                    our_size,
                    offset: (0, 0),
                    ssim_shifted: 0.0,
                },
            }
        })
        .collect();
    let ref_rc = std::fs::read_to_string(root().join(format!("ref/{}.rc", case.name)))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    CaseScore {
        name: case.name.clone(),
        args: case.extra.join(" "),
        ref_pages: r.len(),
        our_pages: g.len(),
        pages,
        ref_rc,
    }
}

/// Score every case on a few threads, results in case order.
fn score_all(cases: &[Case], dump: bool) -> Vec<CaseScore> {
    let next = Mutex::new(0usize);
    let results: Mutex<Vec<Option<CaseScore>>> = Mutex::new((0..cases.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 6);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        let i = *n;
                        *n += 1;
                        i
                    };
                    let Some(case) = cases.get(i) else { break };
                    let sc = score(case, dump);
                    results.lock().unwrap()[i] = Some(sc);
                }
            });
        }
    });
    results.into_inner().unwrap().into_iter().map(|r| r.expect("case scored")).collect()
}

/// `(case, page or 0 for every page) -> min SSIM` (same format as the L3
/// threshold files).
fn thresholds(path: &Path) -> BTreeMap<(String, usize), f64> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut out = BTreeMap::new();
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

fn threshold_of(th: &BTreeMap<(String, usize), f64>, case: &str, page: usize) -> Option<f64> {
    th.get(&(case.to_string(), page)).or_else(|| th.get(&(case.to_string(), 0))).copied()
}

const THRESHOLDS_HEADER: &str = "\
# Per-page minimum SSIM of the rendering-fidelity corpus (crates/wkhtmltopdf/tests/fidelity.rs).
#
# Format: <case> <page number | *> <minimum mean SSIM>
# SSIM at 96 dpi in gray, 8x8 windows (stride 4), windows blank in both rasters skipped,
# between our PDF and the reference PDF of the real wkhtmltopdf 0.12.6 (tests/fidelity/ref,
# made by make-refs.sh). A missing page scores 0.
#
# A value is the measured SSIM rounded down to the hundredth, minus 0.01 (rasteriser noise
# across platforms): it records where we are, not where we want to be. Raise a line when a
# change improves a page (FIDELITY_WRITE_THRESHOLDS=1 does it and never lowers a line); never
# lower one to make a change pass.
";

fn write_thresholds(path: &Path, scores: &[CaseScore], old: &BTreeMap<(String, usize), f64>) {
    let mut out = String::from(THRESHOLDS_HEADER);
    out.push('\n');
    // Keep the lines of cases that did not run (FIDELITY_ONLY).
    let ran: Vec<&str> = scores.iter().map(|s| s.name.as_str()).collect();
    let mut lines: BTreeMap<(String, usize), f64> =
        old.iter().filter(|((c, _), _)| !ran.contains(&c.as_str())).map(|(k, v)| (k.clone(), *v)).collect();
    for s in scores {
        for (i, p) in s.pages.iter().enumerate() {
            let measured = ((p.ssim * 100.0).floor() / 100.0 - 0.01).max(0.0);
            let cur = threshold_of(old, &s.name, i + 1).unwrap_or(0.0);
            lines.insert((s.name.clone(), i + 1), measured.max(cur));
        }
    }
    let width = lines.keys().map(|(c, _)| c.len()).max().unwrap_or(0);
    for ((c, p), v) in &lines {
        let page = if *p == 0 { "*".to_string() } else { p.to_string() };
        out.push_str(&format!("{c:width$} {page:>2} {v:.2}\n"));
    }
    std::fs::write(path, out).unwrap();
}

fn size_str(s: Option<(i64, i64)>) -> String {
    s.map_or("-".to_string(), |(w, h)| format!("{w}×{h}"))
}

fn text_str(t: f64) -> String {
    if t.is_nan() { "n/a".to_string() } else { format!("{t:.3}") }
}

fn report(scores: &[CaseScore]) -> String {
    let pages: Vec<&PageScore> = scores.iter().flat_map(|s| s.pages.iter()).collect();
    let n = pages.len().max(1);
    let mean = pages.iter().map(|p| p.ssim).sum::<f64>() / n as f64;
    let count = |f: &dyn Fn(f64) -> bool| pages.iter().filter(|p| f(p.ssim)).count();
    let count_mismatch = scores.iter().filter(|s| s.ref_pages != s.our_pages).count();
    let size_mismatch = scores.iter().filter(|s| s.pages.iter().any(|p| !p.size_ok)).count();
    let texts: Vec<f64> = pages.iter().map(|p| p.text).filter(|t| !t.is_nan()).collect();
    let text_mean = texts.iter().sum::<f64>() / texts.len().max(1) as f64;
    let mut out = String::new();
    out.push_str("# Rendering fidelity scoreboard\n\n");
    out.push_str(
        "Generated by `FIDELITY_REPORT=1 cargo test -p wkhtmltopdf --test fidelity -- --nocapture` \
         (`crates/wkhtmltopdf/tests/fidelity.rs`): our PDF against the reference of the real \
         wkhtmltopdf 0.12.6 (`tests/fidelity/ref`, made by `make-refs.sh` in `wkref:local`). \
         Side-by-side images (`reference | ours | diff`, reference-only ink in red, ours-only in \
         blue) are written to `tests/fidelity/out/<case>-<page>.png`.\n\n",
    );
    out.push_str("## Summary\n\n");
    out.push_str("| Metric | Value |\n|---|---:|\n");
    out.push_str(&format!("| Cases / pages compared | {} / {} |\n", scores.len(), pages.len()));
    out.push_str(&format!("| Mean SSIM (96 dpi, per page) | {mean:.4} |\n"));
    out.push_str(&format!("| Pages with SSIM ≥ 0.99 | {} |\n", count(&|s| s >= 0.99)));
    out.push_str(&format!("| Pages with SSIM ≥ 0.95 | {} |\n", count(&|s| s >= 0.95)));
    out.push_str(&format!("| Pages with SSIM < 0.90 | {} |\n", count(&|s| s < 0.90)));
    out.push_str(&format!("| Cases with a page-count mismatch | {count_mismatch} |\n"));
    out.push_str(&format!("| Cases with a page-size mismatch | {size_mismatch} |\n"));
    out.push_str(&format!("| Mean text similarity (words, per page) | {text_mean:.4} |\n"));
    out.push_str(&format!(
        "| Pages whose text could not be extracted (pdf-extract) | {} |\n\n",
        n - texts.len()
    ));

    let mut order: Vec<&CaseScore> = scores.iter().collect();
    order.sort_by(|a, b| {
        a.min().total_cmp(&b.min()).then(a.mean().total_cmp(&b.mean())).then(a.name.cmp(&b.name))
    });
    out.push_str("## Cases, worst first\n\n");
    out.push_str(
        "Min/mean SSIM over the case's pages; text = lowest per-page word similarity; diff = mean \
         share of pixels differing by more than 32 gray levels; offset = (dx, dy) of our ink \
         against the reference on the worst page, in 96-dpi pixels, and the SSIM once it is \
         compensated.\n\n",
    );
    out.push_str("| # | Case | Extra arguments | Pages ref/ours | Min SSIM | Mean SSIM | Text | Diff | Worst page offset → SSIM |\n");
    out.push_str("|---:|---|---|---:|---:|---:|---:|---:|---|\n");
    for (i, s) in order.iter().enumerate() {
        let worst = s.pages.iter().min_by(|a, b| a.ssim.total_cmp(&b.ssim));
        let diff = s.pages.iter().map(|p| p.diff).sum::<f64>() / s.pages.len().max(1) as f64;
        let off = worst
            .map_or(String::new(), |p| format!("({}, {}) → {:.3}", p.offset.0, p.offset.1, p.ssim_shifted));
        let pages = if s.ref_pages == s.our_pages {
            format!("{}", s.ref_pages)
        } else {
            format!("**{}/{}**", s.ref_pages, s.our_pages)
        };
        let args =
            if s.args.is_empty() { String::new() } else { format!("`{}`", s.args.replace('|', "\\|")) };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {:.3} | {:.3} | {} | {:.1}% | {} |\n",
            i + 1,
            s.name,
            args,
            pages,
            s.min(),
            s.mean(),
            text_str(s.text()),
            diff * 100.0,
            off
        ));
    }

    out.push_str("\n## Pages below 0.99\n\n");
    out.push_str(
        "| Case | Page | SSIM | Diff | Text | Size ref / ours | Offset (dx, dy) | SSIM at offset |\n",
    );
    out.push_str("|---|---:|---:|---:|---:|---|---|---:|\n");
    for s in &order {
        for (i, p) in s.pages.iter().enumerate() {
            if p.ssim >= 0.99 {
                continue;
            }
            let size = if p.size_ok {
                size_str(p.ref_size)
            } else {
                format!("**{} / {}**", size_str(p.ref_size), size_str(p.our_size))
            };
            out.push_str(&format!(
                "| {} | {} | {:.3} | {:.1}% | {} | {} | ({}, {}) | {:.3} |\n",
                s.name,
                i + 1,
                p.ssim,
                p.diff * 100.0,
                text_str(p.text),
                size,
                p.offset.0,
                p.offset.1,
                p.ssim_shifted
            ));
        }
    }
    let nonzero: Vec<String> = scores
        .iter()
        .filter(|s| s.ref_rc != "0")
        .map(|s| format!("{} (exit {})", s.name, s.ref_rc))
        .collect();
    if !nonzero.is_empty() {
        out.push_str(&format!(
            "\nThe reference exits non-zero (but writes a PDF) for: {}. See `ref/<case>.stderr`.\n",
            nonzero.join(", ")
        ));
    }
    out
}

#[test]
fn fidelity_ssim() {
    let cases = cases();
    let dump = std::env::var_os("FIDELITY_REPORT").is_some();
    let scores = score_all(&cases, dump);
    let th_path = root().join("thresholds.txt");
    let th = thresholds(&th_path);
    let mut failures = Vec::new();
    for s in &scores {
        let line: Vec<String> = s.pages.iter().map(|p| format!("{:.3}", p.ssim)).collect();
        eprintln!("{}: pages ref {} ours {}; ssim {}", s.name, s.ref_pages, s.our_pages, line.join(" "));
        for (i, p) in s.pages.iter().enumerate() {
            match threshold_of(&th, &s.name, i + 1) {
                None => {
                    failures.push(format!("{} page {}: no threshold (ssim {:.3})", s.name, i + 1, p.ssim))
                }
                Some(m) if p.ssim < m => {
                    failures.push(format!("{} page {}: ssim {:.3} < {m}", s.name, i + 1, p.ssim))
                }
                _ => {}
            }
        }
    }
    if std::env::var_os("FIDELITY_WRITE_THRESHOLDS").is_some() {
        write_thresholds(&th_path, &scores, &th);
        eprintln!("wrote {}", th_path.display());
        return;
    }
    if dump && std::env::var_os("FIDELITY_ONLY").is_none() {
        std::fs::write(root().join("REPORT.md"), report(&scores)).unwrap();
        eprintln!("wrote {} and images in {}", root().join("REPORT.md").display(), out_dir().display());
    } else if dump {
        eprint!("{}", report(&scores));
    }
    assert!(failures.is_empty(), "fidelity failures:\n{}", failures.join("\n"));
}

#[test]
fn offset_of_a_shifted_page_is_found() {
    let (w, h) = (64, 48);
    let mut px = vec![255u8; w * h];
    for y in 10..20 {
        for x in 8..30 {
            px[y * w + x] = 0;
        }
    }
    let a = common::Gray { w, h, px: px.clone(), rgb: Vec::new() };
    let mut moved = vec![255u8; w * h];
    for y in 0..h - 3 {
        for x in 0..w - 5 {
            moved[(y + 3) * w + x + 5] = px[y * w + x];
        }
    }
    let b = common::Gray { w, h, px: moved, rgb: Vec::new() };
    assert_eq!(offset(&a, &b), (5, 3));
    assert!((common::ssim(&a, &shifted(&b, 5, 3)) - 1.0).abs() < 1e-9);
}
