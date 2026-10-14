# Benchmark: wkhtmltopdf 0.12.6 vs wkhtmltopdf-rs

Run `20261014T161447Z` — full warmup=3 runs=10 batch_runs=10 — phases: timing pdfkit visual.

## Environment

| | |
|---|---|
| Date (UTC) | 2026-10-14T16:16:44Z |
| Container | `wkbench:local`, Linux 7.0.14-linuxkit aarch64 |
| CPUs / memory | 4 vCPU (Apple) / 3.8 GiB |
| Original | `wkhtmltopdf 0.12.6.1 (with patched qt)` (sha256 55127a0e37fb2d2b…) |
| Rust port | `wkhtmltopdf 0.12.6 (with patched qt)` (sha256 c2e4bf4e026c5863…, 19.51 MiB static musl) |
| Tools | hyperfine 1.15.0, GNU time, poppler (pdfinfo/pdftoppm/pdftotext), ImageMagick ImageMagick 6.9.11-60, pdfkit 0.8.7.3 (ruby 3.1.2) |
| Fonts | DejaVu Sans,DejaVu Sans Mono,DejaVu Serif |

## Performance

Iterations: full warmup=3 runs=10 batch_runs=10. Times are hyperfine wall-clock mean ± σ. Ratio = rust / original
(below 1 means the port is faster). Peak RSS and CPU (user+sys seconds) come from one
`/usr/bin/time -v` run each. Batch rows cover 20 conversions per measurement.

**Read wall times with care**: the original applies its default `--javascript-delay` of 200 ms
(pdfkit does not override it) to every loaded page, header/footer pages included. The pdfkit
rows are therefore dominated by sleeping (compare CPU time, and the `--javascript-delay 0`
rows). The port must keep that delay to stay behaviour-compatible, so the same floor will
apply to it.

| Scenario | Original mean±σ | Rust mean±σ | Ratio | Peak RSS orig / rust | CPU s orig / rust | Output size orig / rust | Pages orig / rust |
|---|---|---|---|---|---|---|---|
| CLI startup: --version | 7.2 ms ± 4.3 ms | 1.1 ms ± 2.4 ms | 0.15× (6.7× faster) | 17.6 MiB / 1.6 MiB | 0.00+0.00 / 0.00+0.00 | 39 B / 37 B | - / - |
| CLI startup: -H > /dev/null | 9.5 ms ± 4.2 ms | 1.9 ms ± 2.5 ms | 0.20× (4.9× faster) | 17.9 MiB / 2.2 MiB | 0.00+0.00 / 0.00+0.00 | 21.7 KiB / 21.7 KiB | - / - |
| Small page (a.html) | 248.4 ms ± 12.3 ms | 17.5 ms ± 12.4 ms | 0.07× (14.2× faster) | 37.8 MiB / 14.9 MiB | 0.06+0.02 / 0.00+0.00 | 16.9 KiB / 18.2 KiB | 1 / 1 |
| Medium page (4 pages) | 292.6 ms ± 21.4 ms | 26.1 ms ± 17.2 ms | 0.09× (11.2× faster) | 40.0 MiB / 18.2 MiB | 0.07+0.00 / 0.01+0.00 | 49.8 KiB / 56.8 KiB | 4 / 4 |
| Large invoice (30 pages) | 666.7 ms ± 24.6 ms | 406.9 ms ± 28.7 ms | 0.61× (1.6× faster) | 59.6 MiB / 103.8 MiB | 0.43+0.01 / 0.33+0.03 | 330.6 KiB / 460.8 KiB | 30 / 32 |
| Small page, --javascript-delay 0 | 25.3 ms ± 3.0 ms | 6.9 ms ± 2.1 ms | 0.27× (3.7× faster) | 37.6 MiB / 14.9 MiB | 0.02+0.00 / 0.00+0.00 | 16.9 KiB / 18.2 KiB | 1 / 1 |
| Large invoice, --javascript-delay 0 | 460.7 ms ± 48.7 ms | 631.1 ms ± 447.2 ms | 1.37× (slower) | 59.2 MiB / 102.4 MiB | 0.43+0.00 / 0.87+0.16 | 330.6 KiB / 460.8 KiB | 30 / 32 |
| Header/footer HTML + toc + outline | 920.5 ms ± 8.6 ms | 24.1 ms ± 8.9 ms | 0.03× (38.1× faster) | 45.7 MiB / 16.9 MiB | 0.08+0.01 / 0.01+0.00 | 77.9 KiB / 55.1 KiB | 8 / 8 |
| jQuery DOM build + --javascript-delay 200 | 281.9 ms ± 15.9 ms | 14.6 ms ± 52.8 ms | 0.05× (19.3× faster) | 47.2 MiB / 13.4 MiB | 0.10+0.01 / 0.00+0.00 | 32.2 KiB / 7.9 KiB | 3 / 1 |
| Multi-object: cover + toc + 3 pages | 778.9 ms ± 130.9 ms | 19.8 ms ± 5.9 ms | 0.03× (39.3× faster) | 42.7 MiB / 19.0 MiB | 0.29+0.06 / 0.01+0.00 | 66.7 KiB / 62.5 KiB | 8 / 8 |
| Batch: 20 x medium, sequential | 5.900 s ± 112.1 ms | 350.1 ms ± 39.6 ms | 0.06× (16.9× faster) | 42.9 MiB / 18.4 MiB | 1.42+0.22 / 0.22+0.09 | 49.8 KiB / 56.8 KiB | 4 / 4 |

### Notes

- `batch20` (orig): 3.39 conversions/s (295.0 ms per conversion).
- `batch20` (rust): 57.13 conversions/s (17.5 ms per conversion).

### Commands

| Scenario | Binary | Exit | Runs | Command (run in `/bench/corpus`) |
|---|---|---|---|---|
| `version` | orig | 0 | 433 | `/usr/local/bin/wkhtmltopdf --version` |
| `version` | rust | 0 | 4244 | `/opt/rust/wkhtmltopdf --version` |
| `help` | orig | 0 | 121 | `/usr/local/bin/wkhtmltopdf -H` |
| `help` | rust | 0 | 2970 | `/opt/rust/wkhtmltopdf -H` |
| `small` | orig | 0 | 12 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `small` | rust | 0 | 135 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `medium` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `medium` | rust | 0 | 85 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `large` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `large` | rust | 0 | 10 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `small_nd` | orig | 0 | 123 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `small_nd` | rust | 0 | 414 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `large_nd` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `large_nd` | rust | 0 | 10 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `hf_toc` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `hf_toc` | rust | 0 | 152 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `js` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `js` | rust | 0 | 505 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `multi` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -` |
| `multi` | rust | 0 | 140 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -` |
| `batch20` | orig | 0 | 10 | `for i in $(seq 20); do /usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html > /dev/null || exit $?; done` |
| `batch20` | rust | 0 | 10 | `for i in $(seq 20); do /opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html > /dev/null || exit $?; done` |

## pdfkit integration

pdfkit 0.8.7.3 (ruby 3.1.2) driven by `bench/pdfkit/generate.rb`, `PDFKit.configure { |c| c.wkhtmltopdf = … }`
per binary. "Outcome" is `ok` or the exception class raised by the gem; ⚠ marks an outcome other
than the expected one. Times include the gem (process spawn, stdin/stdout piping).
Same outcome: 16/16 cases. PDFs, page-1 PNGs (50 dpi) and the exact
command line of every case are in `pdfkit/<binary>/` (`results.json`).

| Case | What it exercises | Original | Rust | Same outcome? | Pages orig / rust | Time orig / rust | Page 1 |
|---|---|---|---|---|---|---|---|
| `string_to_pdf` | PDFKit.new(html).to_pdf (defaults) | ok | ok | yes | 4 / 4 | 303.6 ms / 23.5 ms | [png](20261014T161447Z/pdfkit/original/string_to_pdf.png) |
| `options_a4` | page_size A4, mm margins, landscape | ok | ok | yes | 4 / 4 | 296.2 ms / 21.8 ms | [png](20261014T161447Z/pdfkit/original/options_a4.png) |
| `to_file` | kit.to_file(path) | ok | ok | yes | 1 / 1 | 250.0 ms / 9.1 ms | [png](20261014T161447Z/pdfkit/original/to_file.png) |
| `file_source` | PDFKit.new(File.new(path)) | ok | ok | yes | 4 / 4 | 353.5 ms / 49.2 ms | [png](20261014T161447Z/pdfkit/original/file_source.png) |
| `url_source` | URL source from local HTTP server | ok | ok | yes | 4 / 4 | 300.9 ms / 22.4 ms | [png](20261014T161447Z/pdfkit/original/url_source.png) |
| `stylesheets` | kit.stylesheets << extra.css | ok | ok | yes | 4 / 4 | 293.8 ms / 19.5 ms | [png](20261014T161447Z/pdfkit/original/stylesheets.png) |
| `meta_tags` | <meta name=pdfkit-page_size content=Legal> + orientation | ok | ok | yes | 1 / 1 | 243.1 ms / 14.5 ms | [png](20261014T161447Z/pdfkit/original/meta_tags.png) |
| `header_footer` | header_html / footer_html (local files) | ok | ok | yes | 7 / 7 | 516.0 ms / 55.1 ms | [png](20261014T161447Z/pdfkit/original/header_footer.png) |
| `toc_outline` | outline: true, toc: true | ok | ok | yes | 8 / 8 | 734.1 ms / 52.7 ms | [png](20261014T161447Z/pdfkit/original/toc_outline.png) |
| `toc_then_outline` | toc: true, outline: true (wrong order, upstream rejects it) | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 130.3 ms / 36.8 ms |  |
| `cover` | cover: cover.html + toc | ok | ok | yes | 4 / 4 | 751.0 ms / 43.2 ms | [png](20261014T161447Z/pdfkit/original/cover.png) |
| `headers_cookies` | custom_header + cookie against /echo | ok | ok | yes | 1 / 1 | 275.6 ms / 13.7 ms | [png](20261014T161447Z/pdfkit/original/headers_cookies.png) |
| `print_media_type` | print_media_type: true | ok | ok | yes | 1 / 1 | 254.1 ms / 12.4 ms | [png](20261014T161447Z/pdfkit/original/print_media_type.png) |
| `javascript_delay` | jQuery page, javascript_delay: 500 | ok | ok | yes | 3 / 1 | 591.4 ms / 20.5 ms | [png](20261014T161447Z/pdfkit/original/javascript_delay.png) |
| `broken_image_ignore` | 404 image, load_error_handling + load_media_error_handling ignore | ok | ok | yes | 1 / 1 | 244.1 ms / 15.8 ms | [png](20261014T161447Z/pdfkit/original/broken_image_ignore.png) |
| `unreachable_url` | unreachable URL must raise | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 11.7 ms / 4.9 ms |  |

## Rendering parity

The port renders with a different engine than QtWebKit, so pixel identity is not the goal.
Targets (DESIGN.md): **L2** — same structure and text (page count, page size, extracted text,
outline) — and **L3** — visually equivalent pages within a tolerance. Metrics, A = original,
B = rust, pages rasterised at 96 dpi:

- *Text similarity*: word-level similarity of `pdftotext -layout`, whitespace-normalised (L2).
- *SSIM*: structural similarity per page, uniform 7×7 window, grayscale (L3).
- *Pixels differing*: `compare -metric AE -fuzz 5%`, mean over pages (L3, strict).
- *Mean word offset*: distance between the top-left corners of matching words
  (`pdftotext -bbox-layout`); dx/dy show the drift direction (margins, line heights).

Thresholds (`bench/visual-thresholds.json`), PASS / WARN, else FAIL: mean SSIM ≥ 0.95 / 0.85,
worst page SSIM ≥ 0.9 / 0.75, text ≥ 99 % / 95 %,
pixels ≤ 2.0 % / 10.0 %, word offset ≤ 1.0 / 5.0 pt.
Page count or page size mismatch is FAIL. A case takes its worst metric.
Per-page images: `visual/<case>/page-N-{original,rust,diff,side}.png`.

### Original vs Rust

0 of 22 cases are n/a (no PDF from the rust binary).

| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM | Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |
|---|---|---|---|---|---|---|---|---|---|
| `bench-small` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9909 | 0.9909 | 0.52 % | 4.06 (dx -0.31, dy +4.00) | [p1](20261014T161447Z/visual/bench-small/page-1-side.png) |
| `bench-medium` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.5856 | 0.4636 | 18.41 % | 27.25 (dx -0.23, dy +0.72) | [p3](20261014T161447Z/visual/bench-medium/page-3-side.png) |
| `bench-large` | **FAIL** | 30 / 32 | yes | 98.54 % | 0.4558 | 0.4513 | 18.95 % | 209.09 (dx +7.60, dy +203.70) | [p3](20261014T161447Z/visual/bench-large/page-3-side.png) |
| `bench-small_nd` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9909 | 0.9909 | 0.52 % | 4.06 (dx -0.31, dy +4.00) | [p1](20261014T161447Z/visual/bench-small_nd/page-1-side.png) |
| `bench-large_nd` | **FAIL** | 30 / 32 | yes | 98.54 % | 0.4558 | 0.4513 | 18.95 % | 209.09 (dx +7.60, dy +203.70) | [p3](20261014T161447Z/visual/bench-large_nd/page-3-side.png) |
| `bench-hf_toc` | **FAIL** | 8 / 8 | yes | 95.43 % | 0.5200 | 0.3953 | 19.61 % | 64.69 (dx +1.04, dy -40.91) | [p4](20261014T161447Z/visual/bench-hf_toc/page-4-side.png) |
| `bench-js` | **FAIL** | 3 / 1 | yes | 0.00 % | 0.6609 | 0.6609 | 12.03 % | — | [p1](20261014T161447Z/visual/bench-js/page-1-side.png) |
| `bench-multi` | **FAIL** | 8 / 8 | yes | 96.19 % | 0.7969 | 0.5768 | 8.86 % | 28.68 (dx +1.42, dy -6.23) | [p5](20261014T161447Z/visual/bench-multi/page-5-side.png) |
| `pdfkit-broken_image_ignore` | **FAIL** | 1 / 1 | yes | 94.12 % | 0.9913 | 0.9913 | 0.37 % | 3.40 (dx -0.29, dy +3.37) | [p1](20261014T161447Z/visual/pdfkit-broken_image_ignore/page-1-side.png) |
| `pdfkit-cover` | **FAIL** | 4 / 4 | yes | 96.19 % | 0.8384 | 0.6037 | 6.82 % | 30.78 (dx -0.10, dy -11.57) | [p3](20261014T161447Z/visual/pdfkit-cover/page-3-side.png) |
| `pdfkit-file_source` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.5856 | 0.4636 | 18.41 % | 27.25 (dx -0.23, dy +0.72) | [p3](20261014T161447Z/visual/pdfkit-file_source/page-3-side.png) |
| `pdfkit-header_footer` | **FAIL** | 7 / 7 | yes | 97.87 % | 0.4818 | 0.3953 | 21.70 % | 64.69 (dx +1.04, dy -40.91) | [p3](20261014T161447Z/visual/pdfkit-header_footer/page-3-side.png) |
| `pdfkit-headers_cookies` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9950 | 0.9950 | 0.34 % | 3.06 (dx -1.02, dy +2.47) | [p1](20261014T161447Z/visual/pdfkit-headers_cookies/page-1-side.png) |
| `pdfkit-javascript_delay` | **FAIL** | 3 / 1 | yes | 0.00 % | 0.6609 | 0.6609 | 12.03 % | — | [p1](20261014T161447Z/visual/pdfkit-javascript_delay/page-1-side.png) |
| `pdfkit-meta_tags` | **FAIL** | 1 / 1 | yes | 100.00 % | 0.9074 | 0.9074 | 4.45 % | 119.17 (dx +11.92, dy +1.30) | [p1](20261014T161447Z/visual/pdfkit-meta_tags/page-1-side.png) |
| `pdfkit-options_a4` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.5758 | 0.4271 | 18.27 % | 24.27 (dx -1.35, dy -0.70) | [p2](20261014T161447Z/visual/pdfkit-options_a4/page-2-side.png) |
| `pdfkit-print_media_type` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9956 | 0.9956 | 0.33 % | 3.35 (dx -0.71, dy +3.02) | [p1](20261014T161447Z/visual/pdfkit-print_media_type/page-1-side.png) |
| `pdfkit-string_to_pdf` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.5856 | 0.4636 | 18.41 % | 27.25 (dx -0.23, dy +0.72) | [p3](20261014T161447Z/visual/pdfkit-string_to_pdf/page-3-side.png) |
| `pdfkit-stylesheets` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.5880 | 0.4539 | 20.97 % | 28.24 (dx +0.31, dy +0.72) | [p3](20261014T161447Z/visual/pdfkit-stylesheets/page-3-side.png) |
| `pdfkit-to_file` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9909 | 0.9909 | 0.52 % | 4.06 (dx -0.31, dy +4.00) | [p1](20261014T161447Z/visual/pdfkit-to_file/page-1-side.png) |
| `pdfkit-toc_outline` | **FAIL** | 8 / 8 | yes | 97.73 % | 0.5377 | 0.4132 | 19.19 % | 64.69 (dx +1.04, dy -40.91) | [p4](20261014T161447Z/visual/pdfkit-toc_outline/page-4-side.png) |
| `pdfkit-url_source` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.5856 | 0.4636 | 18.41 % | 27.25 (dx -0.23, dy +0.72) | [p3](20261014T161447Z/visual/pdfkit-url_source/page-3-side.png) |

### Pipeline self-test (original vs original with another option)

Validates the comparison itself: `identical` must PASS, the variants must be flagged.

| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM | Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |
|---|---|---|---|---|---|---|---|---|---|
| `selftest-identical` | **PASS** | 4 / 4 | yes | 100.00 % | 1.0000 | 1.0000 | 0.00 % | 0.00 (dx +0.00, dy +0.00) | [p1](20261014T161447Z/visual/selftest-identical/page-1-side.png) |
| `selftest-zoom-1.1` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.4940 | 0.4395 | 20.39 % | 173.19 (dx +0.80, dy +104.00) | [p3](20261014T161447Z/visual/selftest-zoom-1.1/page-3-side.png) |
| `selftest-margin-left-0.85in` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.6734 | 0.5819 | 15.69 % | 40.34 (dx +3.43, dy +5.31) | [p3](20261014T161447Z/visual/selftest-margin-left-0.85in/page-3-side.png) |

### First text differences

- `bench-medium`: `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive` · `-Rate period variance contract metric warehouse report budget rate customer client contract price pr`
- `bench-large`: `+# SKU Description Qty Unit Amount` · `-# SKU Description Qty Unit Amount` · `+# SKU Description Qty Unit Amount` · `-# SKU Description Qty Unit Amount`
- `bench-large_nd`: `+# SKU Description Qty Unit Amount` · `-# SKU Description Qty Unit Amount` · `+# SKU Description Qty Unit Amount` · `-# SKU Description Qty Unit Amount`
- `bench-hf_toc`: `-Operations Handbook — Table of Contents Page 1 / 8` · `-Table of Contents` · `-Table of Contents 1` · `-Operations Handbook 2`
- `bench-js`: `-Regional dashboard` · `-Total revenue: 2376527.00 over 78 rows.` · `-North` · `-Month Orders Revenue Delta Share`
- `bench-multi`: `-Table of Contents` · `-Table of Contents 2` · `-Part I — Finance 3` · `-Chapter 1: Client trend dispatch 3`
- `pdfkit-broken_image_ignore`: `-missing`
- `pdfkit-cover`: `-Table of Contents` · `-Table of Contents 2` · `-Part I — Finance 3` · `-Chapter 1: Client trend dispatch 3`
- `pdfkit-file_source`: `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive` · `-Rate period variance contract metric warehouse report budget rate customer client contract price pr`
- `pdfkit-header_footer`: `-Operations Handbook — Operations Handbook Page 1 / 7` · `-Demand review period forecast order route process cycle revenue account customer product quality` · `-volume estimate. Planning growth customer variance shipment price market delivery demand account` · `-analysis forecast contract price process cycle team review. Network freight metric schedule pipelin`
- `pdfkit-javascript_delay`: `-Regional dashboard` · `-Total revenue: 2376527.00 over 78 rows.` · `-North` · `-Month Orders Revenue Delta Share`
- `pdfkit-meta_tags`: `-Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dol` · `-ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dolor sit` · `+Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dol` · `-amet. Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ipsum dolor sit amet. Lorem ips`
- `pdfkit-options_a4`: `-Carrier balance budget cost period forecast trend route schedule delivery planning cost customer cl` · `-quarter margin schedule delivery lead cost. Client order metric process inventory revenue rate cust` · `-variance contract metric carrier tax revenue rate invoice service order. Market cycle revenue suppl` · `-customer product trend dispatch schedule. Budget account analysis variance trend capacity warehouse`
- `pdfkit-string_to_pdf`: `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive` · `-Rate period variance contract metric warehouse report budget rate customer client contract price pr`
- `pdfkit-stylesheets`: `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive` · `-Rate period variance contract metric warehouse report budget rate customer client contract price pr`
- `pdfkit-toc_outline`: `-Table of Contents` · `-Table of Contents 1` · `-Operations Handbook 2` · `-Chapter 1: Report revenue payment 2`
- `pdfkit-url_source`: `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive` · `-Rate period variance contract metric warehouse report budget rate customer client contract price pr`
- `selftest-zoom-1.1`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
- `selftest-margin-left-0.85in`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
