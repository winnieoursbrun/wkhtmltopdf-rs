# Benchmark: wkhtmltopdf 0.12.6 vs wkhtmltopdf-rs

Run `20261014T213215Z` — full warmup=3 runs=10 batch_runs=10 — phases: timing pdfkit visual.

## Environment

| | |
|---|---|
| Date (UTC) | 2026-10-14T21:37:29Z |
| Container | `wkbench:local`, Linux 7.0.14-linuxkit aarch64 |
| CPUs / memory | 4 vCPU (Apple) / 3.8 GiB |
| Original | `wkhtmltopdf 0.12.6.1 (with patched qt)` (sha256 55127a0e37fb2d2b…) |
| Rust port | `wkhtmltopdf 0.12.6 (with patched qt)` (sha256 b725cbe2b32b622c…, 29.83 MiB static musl) |
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
| CLI startup: --version | 3.1 ms ± 0.5 ms | 0.4 ms ± 0.1 ms | 0.12× (8.0× faster) | 17.7 MiB / 2.0 MiB | 0.00+0.00 / 0.00+0.00 | 39 B / 37 B | - / - |
| CLI startup: -H > /dev/null | 5.8 ms ± 0.6 ms | 0.6 ms ± 0.2 ms | 0.10× (9.7× faster) | 18.1 MiB / 2.7 MiB | 0.00+0.00 / 0.00+0.00 | 21.7 KiB / 21.7 KiB | - / - |
| Small page (a.html) | 281.8 ms ± 74.7 ms | 5.9 ms ± 1.4 ms | 0.02× (47.9× faster) | 38.5 MiB / 14.5 MiB | 0.04+0.00 / 0.00+0.00 | 16.9 KiB / 18.8 KiB | 1 / 1 |
| Medium page (4 pages) | 282.9 ms ± 22.2 ms | 16.4 ms ± 3.2 ms | 0.06× (17.3× faster) | 40.7 MiB / 15.9 MiB | 0.06+0.00 / 0.00+0.00 | 49.8 KiB / 60.4 KiB | 4 / 4 |
| Large invoice (30 pages) | 660.9 ms ± 24.4 ms | 260.7 ms ± 7.8 ms | 0.39× (2.5× faster) | 58.6 MiB / 49.1 MiB | 0.46+0.00 / 0.23+0.03 | 330.6 KiB / 471.7 KiB | 30 / 30 |
| Small page, --javascript-delay 0 | 25.0 ms ± 2.1 ms | 5.9 ms ± 1.5 ms | 0.23× (4.3× faster) | 38.4 MiB / 14.6 MiB | 0.02+0.00 / 0.00+0.00 | 16.9 KiB / 18.8 KiB | 1 / 1 |
| Large invoice, --javascript-delay 0 | 431.7 ms ± 26.3 ms | 256.8 ms ± 6.9 ms | 0.59× (1.7× faster) | 58.1 MiB / 49.1 MiB | 0.42+0.01 / 0.22+0.02 | 330.6 KiB / 471.7 KiB | 30 / 30 |
| Header/footer HTML + toc + outline | 961.3 ms ± 11.7 ms | 202.6 ms ± 68.6 ms | 0.21× (4.7× faster) | 46.6 MiB / 32.8 MiB | 0.14+0.01 / 0.14+0.03 | 77.9 KiB / 89.1 KiB | 8 / 8 |
| jQuery DOM build + --javascript-delay 200 | 305.5 ms ± 21.6 ms | 147.9 ms ± 61.1 ms | 0.48× (2.1× faster) | 47.7 MiB / 29.4 MiB | 0.08+0.00 / 0.10+0.01 | 32.2 KiB / 44.8 KiB | 3 / 3 |
| Multi-object: cover + toc + 3 pages | 735.9 ms ± 10.4 ms | 21.3 ms ± 3.5 ms | 0.03× (34.5× faster) | 42.1 MiB / 17.3 MiB | 0.10+0.00 / 0.01+0.00 | 66.7 KiB / 78.9 KiB | 8 / 8 |
| Batch: 20 x medium, sequential | 5.851 s ± 209.4 ms | 315.7 ms ± 11.0 ms | 0.05× (18.5× faster) | 43.6 MiB / 16.0 MiB | 1.54+0.15 / 0.22+0.08 | 49.8 KiB / 60.4 KiB | 4 / 4 |

### Notes

- `batch20` (orig): 3.42 conversions/s (292.6 ms per conversion).
- `batch20` (rust): 63.36 conversions/s (15.8 ms per conversion).

### Commands

| Scenario | Binary | Exit | Runs | Command (run in `/bench/corpus`) |
|---|---|---|---|---|
| `version` | orig | 0 | 997 | `/usr/local/bin/wkhtmltopdf --version` |
| `version` | rust | 0 | 6653 | `/opt/rust/wkhtmltopdf --version` |
| `help` | orig | 0 | 482 | `/usr/local/bin/wkhtmltopdf -H` |
| `help` | rust | 0 | 4596 | `/opt/rust/wkhtmltopdf -H` |
| `small` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `small` | rust | 0 | 485 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `medium` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `medium` | rust | 0 | 154 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `large` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `large` | rust | 0 | 11 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `small_nd` | orig | 0 | 120 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `small_nd` | rust | 0 | 515 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `large_nd` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `large_nd` | rust | 0 | 11 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `hf_toc` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `hf_toc` | rust | 0 | 15 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `js` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `js` | rust | 0 | 23 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
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
| `string_to_pdf` | PDFKit.new(html).to_pdf (defaults) | ok | ok | yes | 4 / 4 | 270.0 ms / 17.3 ms | [png](20261014T213215Z/pdfkit/original/string_to_pdf.png) |
| `options_a4` | page_size A4, mm margins, landscape | ok | ok | yes | 4 / 4 | 283.9 ms / 17.3 ms | [png](20261014T213215Z/pdfkit/original/options_a4.png) |
| `to_file` | kit.to_file(path) | ok | ok | yes | 1 / 1 | 254.4 ms / 7.5 ms | [png](20261014T213215Z/pdfkit/original/to_file.png) |
| `file_source` | PDFKit.new(File.new(path)) | ok | ok | yes | 4 / 4 | 294.5 ms / 19.1 ms | [png](20261014T213215Z/pdfkit/original/file_source.png) |
| `url_source` | URL source from local HTTP server | ok | ok | yes | 4 / 4 | 297.5 ms / 20.2 ms | [png](20261014T213215Z/pdfkit/original/url_source.png) |
| `stylesheets` | kit.stylesheets << extra.css | ok | ok | yes | 4 / 4 | 296.5 ms / 18.9 ms | [png](20261014T213215Z/pdfkit/original/stylesheets.png) |
| `meta_tags` | <meta name=pdfkit-page_size content=Legal> + orientation | ok | ok | yes | 1 / 1 | 249.6 ms / 7.4 ms | [png](20261014T213215Z/pdfkit/original/meta_tags.png) |
| `header_footer` | header_html / footer_html (local files) | ok | ok | yes | 7 / 7 | 523.1 ms / 159.2 ms | [png](20261014T213215Z/pdfkit/original/header_footer.png) |
| `toc_outline` | outline: true, toc: true | ok | ok | yes | 8 / 8 | 736.5 ms / 30.2 ms | [png](20261014T213215Z/pdfkit/original/toc_outline.png) |
| `toc_then_outline` | toc: true, outline: true (wrong order, upstream rejects it) | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 43.5 ms / 2.5 ms |  |
| `cover` | cover: cover.html + toc | ok | ok | yes | 4 / 4 | 696.0 ms / 14.9 ms | [png](20261014T213215Z/pdfkit/original/cover.png) |
| `headers_cookies` | custom_header + cookie against /echo | ok | ok | yes | 1 / 1 | 252.1 ms / 7.5 ms | [png](20261014T213215Z/pdfkit/original/headers_cookies.png) |
| `print_media_type` | print_media_type: true | ok | ok | yes | 1 / 1 | 243.9 ms / 6.9 ms | [png](20261014T213215Z/pdfkit/original/print_media_type.png) |
| `javascript_delay` | jQuery page, javascript_delay: 500 | ok | ok | yes | 3 / 3 | 605.6 ms / 127.9 ms | [png](20261014T213215Z/pdfkit/original/javascript_delay.png) |
| `broken_image_ignore` | 404 image, load_error_handling + load_media_error_handling ignore | ok | ok | yes | 1 / 1 | 249.2 ms / 10.9 ms | [png](20261014T213215Z/pdfkit/original/broken_image_ignore.png) |
| `unreachable_url` | unreachable URL must raise | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 12.8 ms / 4.2 ms |  |

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
| `bench-small` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9993 | 0.9993 | 0.13 % | 2.52 (dx +0.15, dy +2.51) | [p1](20261014T213215Z/visual/bench-small/page-1-side.png) |
| `bench-medium` | **WARN** | 4 / 4 | yes | 100.00 % | 0.8794 | 0.7726 | 9.68 % | 1.85 (dx -0.03, dy +1.82) | [p3](20261014T213215Z/visual/bench-medium/page-3-side.png) |
| `bench-large` | **WARN** | 30 / 30 | yes | 100.00 % | 0.9363 | 0.9228 | 3.83 % | 1.26 (dx -0.15, dy +1.22) | [p15](20261014T213215Z/visual/bench-large/page-15-side.png) |
| `bench-small_nd` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9993 | 0.9993 | 0.13 % | 2.52 (dx +0.15, dy +2.51) | [p1](20261014T213215Z/visual/bench-small_nd/page-1-side.png) |
| `bench-large_nd` | **WARN** | 30 / 30 | yes | 100.00 % | 0.9363 | 0.9228 | 3.83 % | 1.26 (dx -0.15, dy +1.22) | [p15](20261014T213215Z/visual/bench-large_nd/page-15-side.png) |
| `bench-hf_toc` | **FAIL** | 8 / 8 | yes | 100.00 % | 0.8608 | 0.7812 | 10.76 % | 2.04 (dx -0.19, dy +1.85) | [p5](20261014T213215Z/visual/bench-hf_toc/page-5-side.png) |
| `bench-js` | **WARN** | 3 / 3 | yes | 99.50 % | 0.9603 | 0.9432 | 1.33 % | 1.94 (dx +0.00, dy +0.74) | [p2](20261014T213215Z/visual/bench-js/page-2-side.png) |
| `bench-multi` | **WARN** | 8 / 8 | yes | 100.00 % | 0.9547 | 0.9295 | 4.41 % | 1.73 (dx -0.03, dy +1.69) | [p4](20261014T213215Z/visual/bench-multi/page-4-side.png) |
| `pdfkit-broken_image_ignore` | **FAIL** | 1 / 1 | yes | 94.12 % | 0.9940 | 0.9940 | 0.19 % | 2.93 (dx -0.03, dy +2.92) | [p1](20261014T213215Z/visual/pdfkit-broken_image_ignore/page-1-side.png) |
| `pdfkit-cover` | **WARN** | 4 / 4 | yes | 100.00 % | 0.9619 | 0.9295 | 3.41 % | 1.81 (dx +0.02, dy +1.76) | [p4](20261014T213215Z/visual/pdfkit-cover/page-4-side.png) |
| `pdfkit-file_source` | **WARN** | 4 / 4 | yes | 100.00 % | 0.8794 | 0.7726 | 9.68 % | 1.85 (dx -0.03, dy +1.82) | [p3](20261014T213215Z/visual/pdfkit-file_source/page-3-side.png) |
| `pdfkit-header_footer` | **FAIL** | 7 / 7 | yes | 100.00 % | 0.8495 | 0.7812 | 11.81 % | 2.06 (dx -0.19, dy +1.86) | [p4](20261014T213215Z/visual/pdfkit-header_footer/page-4-side.png) |
| `pdfkit-headers_cookies` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9999 | 0.9999 | 0.05 % | 2.02 (dx -0.02, dy +2.02) | [p1](20261014T213215Z/visual/pdfkit-headers_cookies/page-1-side.png) |
| `pdfkit-javascript_delay` | **WARN** | 3 / 3 | yes | 99.50 % | 0.9603 | 0.9432 | 1.33 % | 1.94 (dx +0.00, dy +0.74) | [p2](20261014T213215Z/visual/pdfkit-javascript_delay/page-2-side.png) |
| `pdfkit-meta_tags` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9334 | 0.9334 | 3.77 % | 2.47 (dx +1.64, dy +1.66) | [p1](20261014T213215Z/visual/pdfkit-meta_tags/page-1-side.png) |
| `pdfkit-options_a4` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.6179 | 0.3794 | 16.64 % | 13.36 (dx -0.05, dy +13.34) | [p2](20261014T213215Z/visual/pdfkit-options_a4/page-2-side.png) |
| `pdfkit-print_media_type` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9998 | 0.9998 | 0.08 % | 2.93 (dx +0.05, dy +2.93) | [p1](20261014T213215Z/visual/pdfkit-print_media_type/page-1-side.png) |
| `pdfkit-string_to_pdf` | **WARN** | 4 / 4 | yes | 100.00 % | 0.8794 | 0.7726 | 9.68 % | 1.85 (dx -0.03, dy +1.82) | [p3](20261014T213215Z/visual/pdfkit-string_to_pdf/page-3-side.png) |
| `pdfkit-stylesheets` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.8758 | 0.7613 | 10.19 % | 1.88 (dx +0.16, dy +1.82) | [p3](20261014T213215Z/visual/pdfkit-stylesheets/page-3-side.png) |
| `pdfkit-to_file` | **WARN** | 1 / 1 | yes | 100.00 % | 0.9993 | 0.9993 | 0.13 % | 2.52 (dx +0.15, dy +2.51) | [p1](20261014T213215Z/visual/pdfkit-to_file/page-1-side.png) |
| `pdfkit-toc_outline` | **FAIL** | 8 / 8 | yes | 100.00 % | 0.8644 | 0.7852 | 10.53 % | 2.06 (dx -0.17, dy +1.88) | [p5](20261014T213215Z/visual/pdfkit-toc_outline/page-5-side.png) |
| `pdfkit-url_source` | **WARN** | 4 / 4 | yes | 100.00 % | 0.8794 | 0.7726 | 9.68 % | 1.85 (dx -0.03, dy +1.82) | [p3](20261014T213215Z/visual/pdfkit-url_source/page-3-side.png) |

### Pipeline self-test (original vs original with another option)

Validates the comparison itself: `identical` must PASS, the variants must be flagged.

| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM | Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |
|---|---|---|---|---|---|---|---|---|---|
| `selftest-identical` | **PASS** | 4 / 4 | yes | 100.00 % | 1.0000 | 1.0000 | 0.00 % | 0.00 (dx +0.00, dy +0.00) | [p1](20261014T213215Z/visual/selftest-identical/page-1-side.png) |
| `selftest-zoom-1.1` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.4940 | 0.4395 | 20.39 % | 173.19 (dx +0.80, dy +104.00) | [p3](20261014T213215Z/visual/selftest-zoom-1.1/page-3-side.png) |
| `selftest-margin-left-0.85in` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.6734 | 0.5819 | 15.69 % | 40.34 (dx +3.43, dy +5.31) | [p3](20261014T213215Z/visual/selftest-margin-left-0.85in/page-3-side.png) |

### First text differences

- `bench-hf_toc`: `-analysis variance order volume schedule balance revenue supplier target service shipment volume sch` · `-balance.` · `+analysis variance order volume schedule balance revenue supplier target service shipment volume` · `+schedule balance.`
- `bench-js`: `-571 31976.00` · `-Overseas 07 -12736` · `+Overseas 07 571 31976.00 -12736`
- `pdfkit-broken_image_ignore`: `-missing`
- `pdfkit-header_footer`: `-analysis variance order volume schedule balance revenue supplier target service shipment volume sch` · `-balance.` · `+analysis variance order volume schedule balance revenue supplier target service shipment volume` · `+schedule balance.`
- `pdfkit-javascript_delay`: `-571 31976.00` · `-Overseas 07 -12736` · `+Overseas 07 571 31976.00 -12736`
- `pdfkit-toc_outline`: `-analysis variance order volume schedule balance revenue supplier target service shipment volume sch` · `-balance.` · `+analysis variance order volume schedule balance revenue supplier target service shipment volume` · `+schedule balance.`
- `selftest-zoom-1.1`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
- `selftest-margin-left-0.85in`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
