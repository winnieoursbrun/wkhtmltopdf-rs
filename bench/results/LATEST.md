# Benchmark: wkhtmltopdf 0.12.6 vs wkhtmltopdf-rs

Run `20261014T145810Z` — full warmup=3 runs=10 batch_runs=10 — phases: timing pdfkit visual.

## Environment

| | |
|---|---|
| Date (UTC) | 2026-10-14T14:58:14Z |
| Container | `wkbench:local`, Linux 7.0.14-linuxkit aarch64 |
| CPUs / memory | 4 vCPU (Apple) / 3.8 GiB |
| Original | `wkhtmltopdf 0.12.6.1 (with patched qt)` (sha256 55127a0e37fb2d2b…) |
| Rust port | `wkhtmltopdf 0.12.6 (with patched qt)` (sha256 004d8a12d54e64ab…, 913.2 KiB static musl) |
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
| CLI startup: --version | 7.5 ms ± 4.1 ms | 1.0 ms ± 1.7 ms | 0.13× (7.7× faster) | 17.6 MiB / 1.3 MiB | 0.00+0.00 / 0.00+0.00 | 39 B / 37 B | - / - |
| CLI startup: -H > /dev/null | 7.2 ms ± 3.5 ms | 0.8 ms ± 1.4 ms | 0.11× (9.3× faster) | 17.9 MiB / 1.3 MiB | 0.00+0.00 / 0.00+0.00 | 21.7 KiB / 21.7 KiB | - / - |
| Small page (a.html) | 241.5 ms ± 8.1 ms | n/a (exit 1) | — | 37.7 MiB / n/a | 0.04+0.01 / n/a | 16.9 KiB / n/a | 1 / n/a |
| Medium page (4 pages) | 294.2 ms ± 5.5 ms | n/a (exit 1) | — | 40.9 MiB / n/a | 0.07+0.00 / n/a | 49.8 KiB / n/a | 4 / n/a |
| Large invoice (30 pages) | 729.7 ms ± 131.8 ms | n/a (exit 1) | — | 58.3 MiB / n/a | 0.55+0.00 / n/a | 330.6 KiB / n/a | 30 / n/a |
| Small page, --javascript-delay 0 | 27.8 ms ± 10.8 ms | n/a (exit 1) | — | 37.6 MiB / n/a | 0.01+0.00 / n/a | 16.9 KiB / n/a | 1 / n/a |
| Large invoice, --javascript-delay 0 | 768.3 ms ± 378.7 ms | n/a (exit 1) | — | 57.5 MiB / n/a | 0.97+0.03 / n/a | 330.6 KiB / n/a | 30 / n/a |
| Header/footer HTML + toc + outline | 975.6 ms ± 9.3 ms | n/a (exit 1) | — | 44.6 MiB / n/a | 0.14+0.01 / n/a | 77.9 KiB / n/a | 8 / n/a |
| jQuery DOM build + --javascript-delay 200 | 304.8 ms ± 5.5 ms | n/a (exit 1) | — | 47.6 MiB / n/a | 0.09+0.00 / n/a | 32.2 KiB / n/a | 3 / n/a |
| Multi-object: cover + toc + 3 pages | 741.8 ms ± 8.5 ms | n/a (exit 1) | — | 41.5 MiB / n/a | 0.12+0.01 / n/a | 66.7 KiB / n/a | 8 / n/a |
| Batch: 20 x medium, sequential | 5.796 s ± 70.0 ms | n/a (exit 1) | — | 41.0 MiB / n/a | 1.40+0.14 / n/a | 49.8 KiB / n/a | 4 / n/a |

### Notes

- `batch20` (orig): 3.45 conversions/s (289.8 ms per conversion).
- stderr `wkhtmltopdf-rs: conversion engine not implemented yet` — small (rust), medium (rust), large (rust), small_nd (rust), large_nd (rust), hf_toc (rust), js (rust), multi (rust), batch20 (rust)

### Commands

| Scenario | Binary | Exit | Runs | Command (run in `/bench/corpus`) |
|---|---|---|---|---|
| `version` | orig | 0 | 322 | `/usr/local/bin/wkhtmltopdf --version` |
| `version` | rust | 0 | 3644 | `/opt/rust/wkhtmltopdf --version` |
| `help` | orig | 0 | 526 | `/usr/local/bin/wkhtmltopdf -H` |
| `help` | rust | 0 | 2207 | `/opt/rust/wkhtmltopdf -H` |
| `small` | orig | 0 | 12 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `small` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < small.html` |
| `medium` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `medium` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html` |
| `large` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `large` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < large.html` |
| `small_nd` | orig | 0 | 122 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `small_nd` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < small.html` |
| `large_nd` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `large_nd` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 0 - - < large.html` |
| `hf_toc` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `hf_toc` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access --header-html header.html --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - < hf-body.html` |
| `js` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `js` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --javascript-delay 200 - - < js.html` |
| `multi` | orig | 0 | 10 | `/usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -` |
| `multi` | rust | 1 | 0 | `/opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -` |
| `batch20` | orig | 0 | 10 | `for i in $(seq 20); do /usr/local/bin/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html > /dev/null || exit $?; done` |
| `batch20` | rust | 1 | 0 | `for i in $(seq 20); do /opt/rust/wkhtmltopdf --quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 - - < medium.html > /dev/null || exit $?; done` |

## pdfkit integration

pdfkit 0.8.7.3 (ruby 3.1.2) driven by `bench/pdfkit/generate.rb`, `PDFKit.configure { |c| c.wkhtmltopdf = … }`
per binary. "Outcome" is `ok` or the exception class raised by the gem; ⚠ marks an outcome other
than the expected one. Times include the gem (process spawn, stdin/stdout piping).
Same outcome: 2/16 cases. PDFs, page-1 PNGs (50 dpi) and the exact
command line of every case are in `pdfkit/<binary>/` (`results.json`).

| Case | What it exercises | Original | Rust | Same outcome? | Pages orig / rust | Time orig / rust | Page 1 |
|---|---|---|---|---|---|---|---|
| `string_to_pdf` | PDFKit.new(html).to_pdf (defaults) | ok | `Errno::EPIPE` ⚠ unexpected | **no** | 4 / — | 263.5 ms / 1.5 ms | [png](20261014T145810Z/pdfkit/original/string_to_pdf.png) |
| `options_a4` | page_size A4, mm margins, landscape | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 4 / — | 289.3 ms / 1.1 ms | [png](20261014T145810Z/pdfkit/original/options_a4.png) |
| `to_file` | kit.to_file(path) | ok | `Errno::ENOENT` ⚠ unexpected | **no** | 1 / — | 251.6 ms / 1.1 ms | [png](20261014T145810Z/pdfkit/original/to_file.png) |
| `file_source` | PDFKit.new(File.new(path)) | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 4 / — | 283.7 ms / 1.0 ms | [png](20261014T145810Z/pdfkit/original/file_source.png) |
| `url_source` | URL source from local HTTP server | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 4 / — | 270.7 ms / 1.0 ms | [png](20261014T145810Z/pdfkit/original/url_source.png) |
| `stylesheets` | kit.stylesheets << extra.css | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 4 / — | 265.5 ms / 1.2 ms | [png](20261014T145810Z/pdfkit/original/stylesheets.png) |
| `meta_tags` | <meta name=pdfkit-page_size content=Legal> + orientation | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 1 / — | 240.0 ms / 2.8 ms | [png](20261014T145810Z/pdfkit/original/meta_tags.png) |
| `header_footer` | header_html / footer_html (local files) | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 7 / — | 494.9 ms / 1.2 ms | [png](20261014T145810Z/pdfkit/original/header_footer.png) |
| `toc_outline` | outline: true, toc: true | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 8 / — | 732.4 ms / 1.3 ms | [png](20261014T145810Z/pdfkit/original/toc_outline.png) |
| `toc_then_outline` | toc: true, outline: true (wrong order, upstream rejects it) | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 40.8 ms / 1.4 ms |  |
| `cover` | cover: cover.html + toc | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 4 / — | 709.6 ms / 1.2 ms | [png](20261014T145810Z/pdfkit/original/cover.png) |
| `headers_cookies` | custom_header + cookie against /echo | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 1 / — | 255.0 ms / 1.1 ms | [png](20261014T145810Z/pdfkit/original/headers_cookies.png) |
| `print_media_type` | print_media_type: true | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 1 / — | 257.2 ms / 1.0 ms | [png](20261014T145810Z/pdfkit/original/print_media_type.png) |
| `javascript_delay` | jQuery page, javascript_delay: 500 | ok | `Errno::EPIPE` ⚠ unexpected | **no** | 3 / — | 659.0 ms / 1.3 ms | [png](20261014T145810Z/pdfkit/original/javascript_delay.png) |
| `broken_image_ignore` | 404 image, load_error_handling + load_media_error_handling ignore | ok | `PDFKit::ImproperWkhtmltopdfExitStatus` ⚠ unexpected | **no** | 1 / — | 240.5 ms / 1.3 ms | [png](20261014T145810Z/pdfkit/original/broken_image_ignore.png) |
| `unreachable_url` | unreachable URL must raise | `PDFKit::ImproperWkhtmltopdfExitStatus` | `PDFKit::ImproperWkhtmltopdfExitStatus` | yes | — / — | 13.1 ms / 0.9 ms |  |

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

22 of 22 cases are n/a (no PDF from the rust binary).

| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM | Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |
|---|---|---|---|---|---|---|---|---|---|
| `bench-small` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-medium` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-large` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-small_nd` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-large_nd` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-hf_toc` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-js` | n/a — no PDF from the rust binary | | | | | | | | |
| `bench-multi` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-broken_image_ignore` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-cover` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-file_source` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-header_footer` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-headers_cookies` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-javascript_delay` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-meta_tags` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-options_a4` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-print_media_type` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-string_to_pdf` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-stylesheets` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-to_file` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-toc_outline` | n/a — no PDF from the rust binary | | | | | | | | |
| `pdfkit-url_source` | n/a — no PDF from the rust binary | | | | | | | | |

### Pipeline self-test (original vs original with another option)

Validates the comparison itself: `identical` must PASS, the variants must be flagged.

| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM | Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |
|---|---|---|---|---|---|---|---|---|---|
| `selftest-identical` | **PASS** | 4 / 4 | yes | 100.00 % | 1.0000 | 1.0000 | 0.00 % | 0.00 (dx +0.00, dy +0.00) | [p1](20261014T145810Z/visual/selftest-identical/page-1-side.png) |
| `selftest-zoom-1.1` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.4940 | 0.4395 | 20.39 % | 173.19 (dx +0.80, dy +104.00) | [p3](20261014T145810Z/visual/selftest-zoom-1.1/page-3-side.png) |
| `selftest-margin-left-0.85in` | **FAIL** | 4 / 4 | yes | 100.00 % | 0.6734 | 0.5819 | 15.69 % | 40.34 (dx +3.43, dy +5.31) | [p3](20261014T145810Z/visual/selftest-margin-left-0.85in/page-3-side.png) |

### First text differences

- `selftest-zoom-1.1`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
- `selftest-margin-left-0.85in`: `-Region client contract price warehouse delivery team payment invoice service trend route market inv` · `-ledger account target network. Metric carrier pipeline demand account customer variance quality.` · `-Estimate report team growth period forecast quarter route estimate inventory budget growth analysis` · `-network contract price warehouse. Lead supplier analysis forecast shipment dispatch estimate delive`
