# Benchmarks: wkhtmltopdf 0.12.6 vs wkhtmltopdf-rs

Before/after comparison of the original `wkhtmltopdf` and the Rust port. It covers
performance, the pdfkit gem integration and rendering parity. Both binaries run natively
(linux/arm64) in the same container, on the same CPU.

```sh
bench/run.sh                 # everything: timing + pdfkit + rendering parity (~6 min)
bench/run.sh --quick         # smoke test: 1 warmup + 3 runs (numbers not meaningful)
bench/run.sh --pdfkit        # only the pdfkit gem integration run
bench/run.sh --visual        # only rendering parity (+ the pdfkit cases it compares)
bench/run.sh --no-build      # reuse the existing image and Rust binary
```

Results land in `bench/results/<UTC timestamp>/`, holding the raw files and `REPORT.md`.
`bench/results/LATEST.md` is a copy of the newest report, with its links rebased. A failing
binary never aborts the run. Its row reads `n/a (exit N)`. Since P2a the port converts static
HTML/CSS; JavaScript, headers/footers and the TOC (P3/P4) still differ or fail.

## Why a dedicated image

The CLI oracle image `wkref:local` is amd64. On an Apple Silicon Mac it runs under emulation,
so it cannot be used for timings. `wkbench:local` (`bench/Dockerfile`) is native
`linux/arm64` Debian bookworm and contains:

| Component | Version / source |
|---|---|
| wkhtmltopdf | official `wkhtmltox_0.12.6.1-3.bookworm_arm64.deb` from github.com/wkhtmltopdf/packaging (sha256-pinned); reports `wkhtmltopdf 0.12.6.1 (with patched qt)` |
| Fonts | `fonts-dejavu-core` only (DejaVu Sans / Serif / Mono), so both binaries resolve the same faces |
| Timing | hyperfine 1.15, GNU time (`/usr/bin/time -v`: peak RSS, user/sys CPU) |
| PDF tools | poppler-utils (`pdfinfo`, `pdftoppm`, `pdftotext`), ImageMagick 6 (`compare`, `montage`), python3 + numpy + PIL |
| pdfkit | Ruby 3.1, `pdfkit` 0.8.7.3 and `webrick` pinned by `bench/pdfkit/Gemfile.lock` |

The container runs with `--network none`. The corpus is self-contained, and the pdfkit URL
cases use a WEBrick server on 127.0.0.1 inside the container.

## Building the Rust binary for the container

`scripts/build-rust.sh` cross-builds a static `aarch64-unknown-linux-musl` binary on macOS
into `bench/.target/`:

1. **Default**: `rustup target add aarch64-unknown-linux-musl`, link with `rust-lld`
   (`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`), and compile `ring` (rustls'
   crypto, the only C code) with the system clang
   (`CFLAGS=--target=aarch64-unknown-linux-musl -U__musl__`) archived by rustup's `llvm-ar`
   (`rustup component add llvm-tools`, added on demand). This is the recipe of the root
   `README.md`. No zig and no C cross toolchain are needed. The build uses the workspace's
   `[profile.release]` (`lto = true`, `codegen-units = 1`), about 6 min from scratch.
2. **Fallback**, automatic when 1 fails or forced with `BUILD_METHOD=docker`: a native build
   in `rust:1.90-alpine` (arm64), whose host target is already `aarch64-unknown-linux-musl`.

The binary is bind-mounted read-only at `/opt/rust/wkhtmltopdf`. The original is at
`/usr/local/bin/wkhtmltopdf`.

## Performance scenarios (`scripts/inside.sh`)

All conversions use pdfkit's exact invocation: `--quiet --page-size Letter --margin-* 0.75in
--encoding UTF-8 - -`, with HTML on stdin and the PDF on stdout. Both binaries get identical
arguments.

| id | Scenario |
|---|---|
| `version`, `help` | CLI startup: `--version`, `-H > /dev/null` |
| `small` | `corpus/small.html` (copy of `docs/upstream-fixtures/0.12.6/conversion/a.html`) |
| `medium` | `corpus/medium.html`, a generated 4-page article |
| `large` | `corpus/large.html`, a generated 30-page invoice (1,450 table rows) |
| `small_nd`, `large_nd` | same as above with `--javascript-delay 0` (see below) |
| `hf_toc` | `--header-html` / `--footer-html` (JS page-number substitution) + `--outline` + `toc` |
| `js` | jQuery 1.12.4 inlined, DOM built at load, `--javascript-delay 200` |
| `multi` | `cover cover.html toc part1.html part2.html part3.html -` |
| `batch20` | 20 sequential conversions of `medium` per measurement |

Per scenario and binary: one preflight run (its output gives the size and page count via
`pdfinfo`), then hyperfine (`--warmup 3 --min-runs 10`, `--export-json`; 10 runs of the whole
batch for `batch20`), then one `/usr/bin/time -v` run.

**Default JavaScript delay.** wkhtmltopdf waits `--javascript-delay` (default 200 ms) after
loading each page, header and footer pages included. pdfkit does not override it, so pdfkit
wall times are mostly sleep: `small` takes about 260 ms with the default and about 60 ms with
`--javascript-delay 0`. To compare engines, look at CPU time and at the `*_nd` rows. The port
has to keep the delay to stay compatible.

## pdfkit integration (`pdfkit/generate.rb`)

For each binary (`original` | `rust`), the script sets `PDFKit.configure { |c| c.wkhtmltopdf = path }`
and drives the gem the way Rails apps do. The cases are: `to_pdf` on a string, A4 with mm
margins and landscape, `to_file`, a `File` source, a URL source, `stylesheets <<`,
`<meta name="pdfkit-…">` options, `header_html`/`footer_html`, `outline` + `toc`, `cover`,
`custom_header` + `cookie` (checked against an echo endpoint), `print_media_type`,
`javascript_delay`, a 404 image with `load_error_handling: 'ignore'`, and an unreachable URL
that must raise `PDFKit::ImproperWkhtmltopdfExitStatus`.

Each case records its outcome (`ok` or the exception class and message), the duration, the
size, the page count and pdfkit's exact command line. Output goes to
`results/<ts>/pdfkit/<binary>/`: `<case>.pdf`, a page-1 `<case>.png` at 50 dpi, and
`results.json`.

Two quirks are worth knowing:

- pdfkit emits options in hash order. `toc: true, outline: true` produces `toc --outline`,
  which the real binary rejects (`--outline specified in incorrect location`).
  `toc_then_outline` keeps that case as an expected failure. `toc_outline` uses the working
  order.
- If the binary fails early, pdfkit raises errors other than
  `ImproperWkhtmltopdfExitStatus`: `to_file` raises `Errno::ENOENT` and a large stdin raises
  `Errno::EPIPE`.

## Rendering parity (`scripts/visual.py`)

The comparison covers every PDF-producing scenario and every pdfkit case where both binaries
produced a PDF. The other cases render as `n/a`. Metrics:

- **Page count and page sizes** from `pdfinfo`.
- Per page, rasterised at 96 dpi by `pdftoppm` (rust pages are resized to the original's pixel
  size if they differ):
  - the share of differing pixels, from `compare -metric AE -fuzz 5%`;
  - SSIM, computed in numpy with a 7×7 window, because ImageMagick 6.9.11 has no SSIM metric;
  - a red-highlight diff image and a montage of `original | rust | diff`.
- **Text similarity**: word-level similarity of whitespace-normalised `pdftotext -layout`,
  plus the first differing lines.
- **Geometry**: the mean offset in points of matching words (`pdftotext -bbox-layout`), with
  signed dx/dy to show drift from margins or line heights.

Thresholds live in `visual-thresholds.json` and set PASS, WARN or FAIL. A page count or page
size mismatch is always FAIL. The goal is L2 (same structure and text) and L3 (visually
equivalent within a tolerance). It is not pixel identity, because the engine is not QtWebKit.

**Self-test.** Every `--visual` run also compares the original against itself:

| Comparison | Expected result |
|---|---|
| `identical` (a rerun) | must PASS |
| `--zoom 1.1` | must be flagged |
| `--margin-left 0.85in` | must be flagged |

This proves that the pipeline detects real differences independently of the port.
Images go to `results/<ts>/visual/<case>/page-N-{original,rust,diff,side}.png`. Expect about
20 MB per run today, and more once the port renders.

## Files

| Path | Role |
|---|---|
| `run.sh` | Host entry point: image, Rust build, corpus, container run, LATEST.md |
| `Dockerfile`, `.dockerignore` | `wkbench:local` |
| `scripts/build-rust.sh` | aarch64 musl cross-build |
| `scripts/gen_corpus.py` | Deterministic corpus generator (medium, large, hf-body, part1-3, js) |
| `scripts/inside.sh` | Runs inside the container: scenarios, pdfkit, parity |
| `scripts/visual.py`, `visual-thresholds.json` | Rendering parity |
| `scripts/report.py` | Writes `REPORT.md` |
| `pdfkit/Gemfile`, `Gemfile.lock`, `generate.rb` | pdfkit integration |
| `corpus/` | Static pages (small, header, footer, cover, extra.css, js.template), vendored jQuery, generated pages |

## Known caveat

`docs/upstream-fixtures/0.12.6/conversion/b.html` is a PDF, not HTML. The `twoout` case of
the fixture capture script (`cap twoout a.html b.html`) overwrote it with the converted
`a.html`. The bench therefore uses its own generated `medium.html` as the medium page.
