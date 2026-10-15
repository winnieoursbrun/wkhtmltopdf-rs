# Benchmark: wkhtmltopdf 0.12.6 vs wkhtmltopdf-rs

Run `20261015T034630Z` — full warmup=3 runs=10 batch_runs=10 — phases: timing pdfkit visual.

## Environment

| | |
|---|---|
| Date (UTC) | 2026-10-15T03:51:06Z |
| Container | `wkbench:local`, Linux 7.0.14-linuxkit aarch64 |
| CPUs / memory | 4 vCPU (Apple) / 3.8 GiB |
| Original | `wkhtmltopdf 0.12.6.1 (with patched qt)` (sha256 55127a0e37fb2d2b…) |
| Rust port | `wkhtmltopdf 0.12.6 (with patched qt)` (sha256 d88be90e6e5f51a6…, 24.44 MiB static musl) |
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
| CLI startup: --version | 2.9 ms ± 0.3 ms | 0.4 ms ± 0.1 ms | 0.15× (6.5× faster) | 17.7 MiB / 1.9 MiB | 0.00+0.00 / 0.00+0.00 | 39 B / 37 B | - / - |
| CLI startup: -H > /dev/null | 5.5 ms ± 0.2 ms | 0.8 ms ± 0.1 ms | 0.14× (7.1× faster) | 18.1 MiB / 2.6 MiB | 0.00+0.00 / 0.00+0.00 | 21.7 KiB / 21.7 KiB | - / - |
| Small page (a.html) | 259.0 ms ± 9.5 ms | 4.4 ms ± 0.7 ms | 0.02× (58.4× faster) | 39.8 MiB / 14.2 MiB | 0.03+0.00 / 0.00+0.00 | 16.9 KiB / 18.8 KiB | 1 / 1 |
| Medium page (4 pages) | 295.3 ms ± 5.0 ms | 10.8 ms ± 0.7 ms | 0.04× (27.3× faster) | 41.4 MiB / 15.2 MiB | 0.08+0.01 / 0.00+0.00 | 49.8 KiB / 61.4 KiB | 4 / 4 |
| Large invoice (30 pages) | 647.5 ms ± 5.4 ms | 147.9 ms ± 2.0 ms | 0.23× (4.4× faster) | 56.5 MiB / 48.5 MiB | 0.41+0.00 / 0.15+0.02 | 330.6 KiB / 472.5 KiB | 30 / 30 |
| Small page, --javascript-delay 0 | 25.4 ms ± 5.4 ms | 4.3 ms ± 0.5 ms | 0.17× (5.9× faster) | 38.2 MiB / 14.2 MiB | 0.02+0.00 / 0.00+0.00 | 16.9 KiB / 18.8 KiB | 1 / 1 |
| Large invoice, --javascript-delay 0 | 398.0 ms ± 6.7 ms | 156.0 ms ± 15.8 ms | 0.39× (2.6× faster) | 60.2 MiB / 48.4 MiB | 0.38+0.00 / 0.16+0.02 | 330.6 KiB / 472.5 KiB | 30 / 30 |
| Header/footer HTML + toc + outline | 986.2 ms ± 5.2 ms | 59.5 ms ± 1.9 ms | 0.06× (16.6× faster) | 45.3 MiB / 25.6 MiB | 0.14+0.01 / 0.05+0.01 | 77.9 KiB / 91.6 KiB | 8 / 8 |
| jQuery DOM build + --javascript-delay 200 | 301.2 ms ± 10.6 ms | 116.1 ms ± 2.0 ms | 0.39× (2.6× faster) | 47.7 MiB / 30.7 MiB | 0.08+0.00 / 0.09+0.01 | 32.2 KiB / 45.2 KiB | 3 / 3 |
| Multi-object: cover + toc + 3 pages | 746.7 ms ± 9.2 ms | 14.6 ms ± 0.6 ms | 0.02× (51.3× faster) | 43.2 MiB / 16.0 MiB | 0.12+0.00 / 0.01+0.00 | 66.7 KiB / 79.9 KiB | 8 / 8 |
| Batch: 20 x medium, sequential | 5.876 s ± 23.9 ms | 212.5 ms ± 13.9 ms | 0.04× (27.7× faster) | 43.4 MiB / 17.1 MiB | 1.49+0.12 / 0.18+0.08 | 49.8 KiB / 61.4 KiB | 4 / 4 |

### Notes

- `batch20` (orig): 3.40 conversions/s (293.8 ms per conversion).
- `batch20` (rust): 94.14 conversions/s (10.6 ms per conversion).

### Commands

| Scenario | Binary | Exit | Runs | Command (run in `/bench/corpus`) |
|---|---|---|---|---|
| `version` | orig | 0 | 1052 | `/usr/local/bin/wkhtmltopdf --version` |
| `version` | rust | 0 | 6188 | `/opt/rust/wkhtmltopdf --version` |
| `help` | orig | 0 | 530 | `/usr/local/bin/wkhtmltopdf -H` |
| `help` | rust | 0 | 4107 | `/opt/rust/wkhtmltopdf -H` |
| `small` | orig | 0 | 11 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `small` | rust | 0 | 550 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `medium` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `medium` | rust | 0 | 277 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `large` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `large` | rust | 0 | 19 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `small_nd` | orig | 0 | 123 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `small_nd` | rust | 0 | 632 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `large_nd` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `large_nd` | rust | 0 | 20 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `hf_toc` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `hf_toc` | rust | 0 | 51 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `js` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `js` | rust | 0 | 26 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `multi` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -` |
| `multi` | rust | 0 | 206 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -` |
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
| `string_to_pdf` | PDFKit.new(html).to_pdf (defaults) | ok | ok | yes | 4 / 4 | 289.6 ms / 13.5 ms | [png](20261015T034630Z/pdfkit/original/string_to_pdf.png) |
| `options_a4` | page_size A4, mm margins, landscape | ok | ok | yes | 4 / 4 | 269.0 ms / 12.2 ms | [png](20261015T034630Z/pdfkit/original/options_a4.png) |
| `to_file` | kit.to_file(path) | ok | ok | yes | 1 / 1 | 235.1 ms / 5.8 ms | [png](20261015T034630Z/pdfkit/original/to_file.png) |
| `file_source` | PDFKit.new(File.new(path)) | ok | ok | yes | 4 / 4 | 270.0 ms / 12.8 ms | [png](20261015T034630Z/pdfkit/original/file_source.png) |
| `url_source` | URL source from local HTTP server | ok | ok | yes | 4 / 4 | 279.8 ms / 15.5 ms | [png](20261015T034630Z/pdfkit/original/url_source.png) |
| `stylesheets` | kit.stylesheets << extra.css | ok | ok | yes | 4 / 4 | 306.2 ms / 12.4 ms | [png](20261015T034630Z/pdfkit/original/stylesheets.png) |
| `meta_tags` | <meta name=pdfkit-page_size content=Legal> + orientation | ok | ok | yes | 1 / 1 | 262.5 ms / 5.9 ms | [png](20261015T034630Z/pdfkit/original/meta_tags.png) |
| `header_footer` | header_html / footer_html (local files) | ok | ok | yes | 7 / 7 | 506.1 ms / 53.9 ms | [png](20261015T034630Z/pdfkit/original/header_footer.png) |
| `toc_outline` | outline: true, toc: true | ok | ok | yes | 8 / 8 | 756.8 ms / 20.9 ms | [png](20261015T034630Z/pdfkit/original/toc_outline.png) |
| `toc_then_outline` | toc: true, outline: true (wrong order, upstream rejects it) | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 29.8 ms / 1.9 ms |  |
| `cover` | cover: cover.html + toc | ok | ok | yes | 4 / 4 | 723.1 ms / 10.7 ms | [png](20261015T034630Z/pdfkit/original/cover.png) |
| `headers_cookies` | custom_header + cookie against /echo | ok | ok | yes | 1 / 1 | 254.0 ms / 6.2 ms | [png](20261015T034630Z/pdfkit/original/headers_cookies.png) |
| `print_media_type` | print_media_type: true | ok | ok | yes | 1 / 1 | 234.3 ms / 6.1 ms | [png](20261015T034630Z/pdfkit/original/print_media_type.png) |
| `javascript_delay` | jQuery page, javascript_delay: 500 | ok | ok | yes | 3 / 3 | 602.6 ms / 117.4 ms | [png](20261015T034630Z/pdfkit/original/javascript_delay.png) |
| `broken_image_ignore` | 404 image, load_error_handling + load_media_error_handling ignore | ok | ok | yes | 1 / 1 | 246.0 ms / 8.8 ms | [png](20261015T034630Z/pdfkit/original/broken_image_ignore.png) |
| `unreachable_url` | unreachable URL must raise | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 12.8 ms / 3.6 ms |  |

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
| `bench-small` | **PASS** | 1 / 1 | yes | 100.00 % | 0.9993 | 0.9993 | 0.13 % | 0.15 (dx +0.15, dy -0.00) | [p1](20261015T034630Z/visual/bench-small/page-1-side.png) |
| `bench-medium` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9393 | 0.9260 | 7.55 % | 0.25 (dx -0.03, dy +0.00) | [p3](20261015T034630Z/visual/bench-medium/page-3-side.png) |
| `bench-large` | **WARN** | 30 / 30 | yes | 100.00 % | 0.9363 | 0.9228 | 3.83 % | 0.20 (dx -0.15, dy -0.00) | [p15](20261015T034630Z/visual/bench-large/page-15-side.png) |
| `bench-small_nd` | **PASS** | 1 / 1 | yes | 100.00 % | 0.9993 | 0.9993 | 0.13 % | 0.15 (dx +0.15, dy -0.00) | [p1](20261015T034630Z/visual/bench-small_nd/page-1-side.png) |
| `bench-large_nd` | **WARN** | 30 / 30 | yes | 100.00 % | 0.9363 | 0.9228 | 3.83 % | 0.20 (dx -0.15, dy -0.00) | [p15](20261015T034630Z/visual/bench-large_nd/page-15-side.png) |
| `bench-hf_toc` | **WARN** | 8 / 8 | yes | 100.00 % | 0.9221 | 0.8955 | 8.49 % | 0.44 (dx -0.19, dy +0.00) | [p4](20261015T034630Z/visual/bench-hf_toc/page-4-side.png) |
| `bench-js` | **PASS** | 3 / 3 | yes | 99.50 % | 0.9603 | 0.9432 | 1.33 % | 0.88 (dx +0.00, dy -0.59) | [p2](20261015T034630Z/visual/bench-js/page-2-side.png) |
| `bench-multi` | **WARN** | 8 / 8 | yes | 100.00 % | 0.9641 | 0.9371 | 4.08 % | 0.27 (dx -0.03, dy -0.00) | [p5](20261015T034630Z/visual/bench-multi/page-5-side.png) |
| `pdfkit-broken_image_ignore` | **PASS** | 1 / 1 | yes | 100.00 % | 0.9964 | 0.9964 | 0.17 % | 0.83 (dx -0.02, dy +0.80) | [p1](20261015T034630Z/visual/pdfkit-broken_image_ignore/page-1-side.png) |
| `pdfkit-cover` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9741 | 0.9463 | 2.98 % | 0.27 (dx +0.02, dy -0.00) | [p3](20261015T034630Z/visual/pdfkit-cover/page-3-side.png) |
| `pdfkit-file_source` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9393 | 0.9260 | 7.55 % | 0.25 (dx -0.03, dy +0.00) | [p3](20261015T034630Z/visual/pdfkit-file_source/page-3-side.png) |
| `pdfkit-header_footer` | **WARN** | 7 / 7 | yes | 100.00 % | 0.9196 | 0.8955 | 9.22 % | 0.45 (dx -0.19, dy +0.00) | [p3](20261015T034630Z/visual/pdfkit-header_footer/page-3-side.png) |
| `pdfkit-headers_cookies` | **PASS** | 1 / 1 | yes | 100.00 % | 0.9999 | 0.9999 | 0.05 % | 0.03 (dx -0.02, dy -0.00) | [p1](20261015T034630Z/visual/pdfkit-headers_cookies/page-1-side.png) |
| `pdfkit-javascript_delay` | **PASS** | 3 / 3 | yes | 99.50 % | 0.9603 | 0.9432 | 1.33 % | 0.88 (dx +0.00, dy -0.59) | [p2](20261015T034630Z/visual/pdfkit-javascript_delay/page-2-side.png) |
| `pdfkit-meta_tags` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9334 | 0.9334 | 3.77 % | 1.64 (dx +1.64, dy -0.00) | [p1](20261015T034630Z/visual/pdfkit-meta_tags/page-1-side.png) |
| `pdfkit-options_a4` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9260 | 0.8968 | 8.01 % | 0.33 (dx -0.04, dy -0.02) | [p3](20261015T034630Z/visual/pdfkit-options_a4/page-3-side.png) |
| `pdfkit-print_media_type` | **PASS** | 1 / 1 | yes | 100.00 % | 0.9998 | 0.9998 | 0.08 % | 0.05 (dx +0.05, dy -0.00) | [p1](20261015T034630Z/visual/pdfkit-print_media_type/page-1-side.png) |
| `pdfkit-string_to_pdf` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9393 | 0.9260 | 7.55 % | 0.25 (dx -0.03, dy +0.00) | [p3](20261015T034630Z/visual/pdfkit-string_to_pdf/page-3-side.png) |
| `pdfkit-stylesheets` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9276 | 0.9071 | 8.30 % | 0.33 (dx +0.16, dy +0.00) | [p3](20261015T034630Z/visual/pdfkit-stylesheets/page-3-side.png) |
| `pdfkit-to_file` | **PASS** | 1 / 1 | yes | 100.00 % | 0.9993 | 0.9993 | 0.13 % | 0.15 (dx +0.15, dy -0.00) | [p1](20261015T034630Z/visual/pdfkit-to_file/page-1-side.png) |
| `pdfkit-toc_outline` | **WARN** | 8 / 8 | yes | 100.00 % | 0.9258 | 0.8992 | 8.27 % | 0.41 (dx -0.17, dy +0.00) | [p4](20261015T034630Z/visual/pdfkit-toc_outline/page-4-side.png) |
| `pdfkit-url_source` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9393 | 0.9260 | 7.55 % | 0.25 (dx -0.03, dy +0.00) | [p3](20261015T034630Z/visual/pdfkit-url_source/page-3-side.png) |

### Pipeline self-test (original vs original with another option)

Validates the comparison itself: `identical` must PASS, the variants must be flagged.

| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM | Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |
|---|---|---|---|---|---|---|---|---|---|
| `selftest-identical` | **PASS** | 4 / 4 | yes | 100.00 % | 1.0000 | 1.0000 | 0.00 % | 0.00 (dx +0.00, dy +0.00) | [p1](20261015T034630Z/visual/selftest-identical/page-1-side.png) |
| `selftest-zoom-1.1` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.4940 | 0.4395 | 20.39 % | 173.19 (dx +0.80, dy +104.00) | [p3](20261015T034630Z/visual/selftest-zoom-1.1/page-3-side.png) |
| `selftest-margin-left-0.85in` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.6734 | 0.5819 | 15.69 % | 40.34 (dx +3.43, dy +5.31) | [p3](20261015T034630Z/visual/selftest-margin-left-0.85in/page-3-side.png) |

### First text differences

- `bench-hf_toc`: `-analysis variance order volume schedule balance revenue supplier target service shipment volume sch` · `-balance.` · `+analysis variance order volume schedule balance revenue supplier target service shipment volume` · `+schedule balance.`
- `bench-js`: `-571 31976.00` · `-Overseas 07 -12736` · `+Overseas 07 571 31976.00 -12736`
- `pdfkit-header_footer`: `-analysis variance order volume schedule balance revenue supplier target service shipment volume sch` · `-balance.` · `+analysis variance order volume schedule balance revenue supplier target service shipment volume` · `+schedule balance.`
- `pdfkit-javascript_delay`: `-571 31976.00` · `-Overseas 07 -12736` · `+Overseas 07 571 31976.00 -12736`
- `pdfkit-toc_outline`: `-analysis variance order volume schedule balance revenue supplier target service shipment volume sch` · `-balance.` · `+analysis variance order volume schedule balance revenue supplier target service shipment volume` · `+schedule balance.`
- `selftest-zoom-1.1`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
- `selftest-margin-left-0.85in`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
