//! Shared helpers of the corpus tests (`corpus_l2.rs`, `corpus_l3.rs`):
//! running our binary like pdfkit does, and rasterising PDFs with hayro.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `tests/corpus`.
pub fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus")
}

/// Pages of the corpus (`tests/corpus/*.html`, sorted).
pub fn pages() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(corpus())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".html").map(str::to_string))
        .collect();
    v.sort();
    v
}

/// Extra options of a corpus page (`tests/corpus/<page>.args`, whitespace
/// separated, also read by `make-refs.sh`).
pub fn page_args(name: &str) -> Vec<String> {
    std::fs::read_to_string(corpus().join(format!("{name}.args")))
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

/// Our PDF for `html` with pdfkit's default invocation (HTML on stdin, PDF
/// on stdout, Letter, 0.75in margins, UTF-8) plus `extra` options.
pub fn ours(html: &[u8], extra: &[String]) -> Vec<u8> {
    let mut args: Vec<String> = [
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(extra.iter().cloned());
    args.extend(["-".to_string(), "-".to_string()]);
    let mut child = Command::new(env!("CARGO_BIN_EXE_wkhtmltopdf"))
        .args(&args)
        .env("WKHTMLTOPDF_SYSTEM_FONTS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run wkhtmltopdf");
    child.stdin.take().unwrap().write_all(html).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.stdout.starts_with(b"%PDF"), "no PDF: {}", String::from_utf8_lossy(&o.stderr));
    o.stdout
}

/// A grayscale raster of one page (white background).
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    /// RGB pixels, for dumps.
    pub rgb: Vec<[u8; 3]>,
}

/// Rasterise every page of `pdf` at `dpi` with hayro.
pub fn raster(pdf: &[u8], dpi: f32) -> Vec<Gray> {
    use hayro::hayro_interpret::InterpreterSettings;
    use hayro::hayro_syntax::Pdf;
    use hayro::vello_cpu::color::palette::css::WHITE;
    use hayro::{PixmapSettings, RenderCache, RenderSettings, render};
    let pdf = Pdf::new(pdf.to_vec()).expect("hayro: parse PDF");
    let cache = RenderCache::new();
    let settings = InterpreterSettings::default();
    let scale = dpi / 72.0;
    let ps = PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE };
    pdf.pages()
        .iter()
        .map(|page| {
            let pm = render(page, &cache, &settings, &RenderSettings::default(), &ps);
            let (w, h) = (pm.width() as usize, pm.height() as usize);
            let rgb: Vec<[u8; 3]> = pm.data().iter().map(|p| [p.r, p.g, p.b]).collect();
            let px = rgb
                .iter()
                .map(|c| (0.299 * f32::from(c[0]) + 0.587 * f32::from(c[1]) + 0.114 * f32::from(c[2])).round() as u8)
                .collect();
            Gray { w, h, px, rgb }
        })
        .collect()
}

/// Mean SSIM of two same-sized grayscale rasters over 8×8 windows with a
/// stride of 4 (constants of Wang et al. 2004). Windows that are blank in
/// both images are skipped, so the score measures the painted content, not
/// the page margins. Returns 1.0 when both pages are blank.
pub fn ssim(a: &Gray, b: &Gray) -> f64 {
    let (w, h) = (a.w.min(b.w), a.h.min(b.h));
    let (c1, c2) = ((0.01f64 * 255.0).powi(2), (0.03f64 * 255.0).powi(2));
    let mut sum = 0.0;
    let mut n = 0usize;
    let win = 8;
    let mut y = 0;
    while y + win <= h {
        let mut x = 0;
        while x + win <= w {
            let (mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0f64, 0.0, 0.0, 0.0, 0.0);
            let mut blank = true;
            for j in 0..win {
                for i in 0..win {
                    let pa = f64::from(a.px[(y + j) * a.w + x + i]);
                    let pb = f64::from(b.px[(y + j) * b.w + x + i]);
                    if pa < 250.0 || pb < 250.0 {
                        blank = false;
                    }
                    sa += pa;
                    sb += pb;
                    saa += pa * pa;
                    sbb += pb * pb;
                    sab += pa * pb;
                }
            }
            if !blank {
                let k = (win * win) as f64;
                let (ma, mb) = (sa / k, sb / k);
                let va = saa / k - ma * ma;
                let vb = sbb / k - mb * mb;
                let cov = sab / k - ma * mb;
                sum += ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2));
                n += 1;
            }
            x += 4;
        }
        y += 4;
    }
    if n == 0 { 1.0 } else { sum / n as f64 }
}

/// Share of pixels whose gray levels differ by more than 32.
pub fn diff_ratio(a: &Gray, b: &Gray) -> f64 {
    let (w, h) = (a.w.min(b.w), a.h.min(b.h));
    let mut d = 0usize;
    for y in 0..h {
        for x in 0..w {
            if a.px[y * a.w + x].abs_diff(b.px[y * b.w + x]) > 32 {
                d += 1;
            }
        }
    }
    d as f64 / (w * h).max(1) as f64
}

/// Write `ref | ours | diff` side by side as a binary PPM.
pub fn dump_side_by_side(path: &Path, a: Option<&Gray>, b: Option<&Gray>) {
    let w = a.map_or(0, |g| g.w).max(b.map_or(0, |g| g.w));
    let h = a.map_or(0, |g| g.h).max(b.map_or(0, |g| g.h));
    let gap = 4;
    let tw = w * 3 + gap * 2;
    let mut out = vec![[128u8, 128, 128]; tw * h];
    let pix = |g: Option<&Gray>, x: usize, y: usize| -> [u8; 3] {
        match g {
            Some(g) if x < g.w && y < g.h => g.rgb[y * g.w + x],
            _ => [255, 255, 255],
        }
    };
    for y in 0..h {
        for x in 0..w {
            let pa = pix(a, x, y);
            let pb = pix(b, x, y);
            out[y * tw + x] = pa;
            out[y * tw + w + gap + x] = pb;
            // Diff: reference only in red, ours only in blue, both in black.
            let ga = pa.iter().map(|&c| u32::from(c)).sum::<u32>() / 3;
            let gb = pb.iter().map(|&c| u32::from(c)).sum::<u32>() / 3;
            let (ia, ib) = (255 - ga, 255 - gb);
            let c = [
                (255 - ib.min(255)) as u8,
                (255 - ia.max(ib).min(255)) as u8,
                (255 - ia.min(255)) as u8,
            ];
            out[y * tw + 2 * (w + gap) + x] = c;
        }
    }
    let mut f = std::fs::File::create(path).unwrap();
    write!(f, "P6\n{tw} {h}\n255\n").unwrap();
    let bytes: Vec<u8> = out.into_iter().flatten().collect();
    f.write_all(&bytes).unwrap();
}
