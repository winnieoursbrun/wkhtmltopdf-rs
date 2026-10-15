# wkhtmltopdf-rs

[![CI](https://github.com/winnieoursbrun/wkhtmltopdf-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/winnieoursbrun/wkhtmltopdf-rs/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/winnieoursbrun/wkhtmltopdf-rs?include_prereleases)](https://github.com/winnieoursbrun/wkhtmltopdf-rs/releases)
[![License: LGPL v3](https://img.shields.io/badge/license-LGPL--3.0--or--later-blue.svg)](LICENSE)

A pure-Rust, single-binary reimplementation of `wkhtmltopdf` 0.12.6 (with patched Qt), meant as a
drop-in replacement behind the [`pdfkit`](https://github.com/pdfkit/pdfkit) Ruby gem (and any other
wrapper that shells out to `wkhtmltopdf`). No Qt, no WebKit, no headless browser: the web engine
that QtWebKit provided is rebuilt from Rust components (html5ever, Stylo, Taffy, Parley, Boa,
krilla). One static binary, embedded fonts, runs on a bare `alpine` or `scratch`-like image.

> **Status: alpha.** It renders real-world invoice/report pages with the same page count and text
> as wkhtmltopdf 0.12.6, and every pdfkit scenario we test behaves the same. Rendering is close but
> not pixel-identical. See [Known divergences](#known-divergences) before putting it in production.

> **Built with [Claude](https://claude.com/claude-code).** This port was designed, written, tested
> and benchmarked by Claude (Anthropic) in Claude Code, under the direction of its maintainer.

## Why

wkhtmltopdf is archived and ships a 2012 QtWebKit. Many Rails apps still depend on it through
pdfkit or wicked_pdf. This project keeps the exact command-line contract (all 122 options, the same
help texts, error messages, exit codes and progress output) so those apps can swap the binary
without touching code, and gets a much faster, smaller, memory-safe binary in exchange.

```sh
# pdfkit
PDFKit.configure { |c| c.wkhtmltopdf = "/usr/local/bin/wkhtmltopdf" }   # just point it at this binary
# CLI, same as before
wkhtmltopdf --page-size A4 --header-html header.html toc input.html out.pdf
```

## Performance

Measured with `bench/run.sh` on the same machine, both binaries native linux/arm64 in the same
container (4 vCPU, hyperfine, 10 runs after 3 warmups). Original: official `wkhtmltopdf 0.12.6.1
(with patched qt)` package. Full report: [`bench/results/LATEST.md`](bench/results/LATEST.md).

| Scenario | wkhtmltopdf 0.12.6 | wkhtmltopdf-rs | Speed-up | Peak RSS orig → rs |
|---|---:|---:|---:|---:|
| `--version` (process startup) | 2.9 ms | 0.4 ms | **6.5×** | 17.7 → 1.9 MiB |
| Small page, pdfkit defaults | 259.0 ms | 4.4 ms | **58×** | 39.8 → 14.2 MiB |
| Medium page (4 pages), pdfkit defaults | 295.3 ms | 10.8 ms | **27×** | 41.4 → 15.2 MiB |
| Large invoice (30 pages), pdfkit defaults | 647.5 ms | 147.9 ms | **4.4×** | 56.5 → 48.5 MiB |
| Large invoice, `--javascript-delay 0` | 398.0 ms | 156.0 ms | **2.6×** | 60.2 → 48.4 MiB |
| Header/footer HTML + TOC + outline (8 pages) | 986.2 ms | 59.5 ms | **16.6×** | 45.3 → 25.6 MiB |
| jQuery page, `--javascript-delay 200` | 301.2 ms | 116.1 ms | **2.6×** | 47.7 → 30.7 MiB |
| Cover + TOC + 3 pages | 746.7 ms | 14.6 ms | **51×** | 43.2 → 16.0 MiB |
| Batch: 20 × medium page, sequential | 5.88 s | 0.21 s | **27.7×** | 43.4 → 17.1 MiB |

Part of the gap on small pages is the original's default 200 ms `--javascript-delay`, which it
really sleeps. The port honours the same delay on a virtual clock: timers and `Date` advance
deterministically, so no wall-clock time is spent waiting (see [Known divergences](#known-divergences)).
The `--javascript-delay 0` rows compare pure engine cost.

### Against OpenHTMLtoPDF (Java)

Measured with `bench/java/run.sh` in the same kind of container, against
[OpenHTMLtoPDF](https://github.com/openhtmltopdf/openhtmltopdf) 1.1.87 on Temurin 21, same inputs
and fonts. Full report and methodology: [`bench/results/JAVA-LATEST.md`](bench/results/JAVA-LATEST.md).

| Document | Cold: rs CLI vs Java CLI (best JVM flags, AppCDS) | Warm: rs in-process vs one warm JVM (median) | Peak RSS rs vs JVM (warm) |
|---|---:|---:|---:|
| Small page | 3.1 ms vs 200.6 ms (**64×**) | 1.3 ms vs 8.7 ms (**6.6×**) | 16 vs 211 MiB |
| Medium page | 9.4 ms vs 299.1 ms (**32×**) | 7.3 ms vs 25.0 ms (**3.4×**) | 16 vs 457 MiB |
| Large invoice (30 pages) | 147.8 ms vs 845.8 ms (**5.7×**) | 146.2 ms vs 278.4 ms (**1.9×**) | 54 vs 644 MiB |
| Handbook body | 12.8 ms vs 296.3 ms (**23×**) | 10.7 ms vs 28.4 ms (**2.7×**) | 16 vs 412 MiB |

Even with the JVM fully warmed up and no process start-up to pay, the Rust port converts faster on
every document while using 8 to 28 times less memory; the JVM also needs a heap of at least 64 MiB
just to convert the large invoice.

## Compatibility with wkhtmltopdf 0.12.6

"Iso" is measured at three levels against the real 0.12.6 binary ([`DESIGN.md`](DESIGN.md)):
**L1** byte-identical process behaviour, **L2** same document structure and text, **L3** visually
equivalent pages. Every number below is produced by tests in this repository.

| Level | What is compared | Result |
|---|---|---|
| L1 | CLI: `--help`, `--extended-help`, `--manpage`, `--htmldoc`, `--readme`, `--license`, `--version`, argument errors (stdout, stderr, exit code, byte for byte) | **23 / 23 (100 %)** |
| L1 | Conversions: progress output, warnings, load errors and exit codes (stderr + exit code, byte for byte) | **13 / 13 (100 %)** |
| L1 | pdfkit gem 0.8.7.3: same outcome (PDF or the same exception) for 16 real-world call patterns | **16 / 16 (100 %)** |
| L2 | Same page count and page size (22 benchmark cases + 12 corpus documents + 22 document-parts cases + 13 JavaScript cases) | **69 / 69 (100 %)** |
| L2 | Extracted text, word-level similarity (mean over the 22 benchmark cases) | **99.9 %** (22 cases ≥ 99 %) |
| L2 | Outline, links, named destinations, TOC entries, header/footer text (`tests/parts`) | **22 / 22** |
| L3 | Visual similarity, mean per-page SSIM at 96 dpi (22 benchmark cases) | **0.96** (22 cases ≥ 0.85) |

How to read L3: the port uses a different layout engine than QtWebKit 2.2, so pixel identity is a
non-goal. Glyphs are rasterised and antialiased differently and words can sit half a pixel apart,
and some tall table rows break at a different place, which costs SSIM even when the content is
identical. Per-page side-by-side images are produced by `bench/run.sh --visual`.

Test suite: 182 tests (`cargo test --workspace`), run on Linux, macOS and Windows in CI, plus a
parity job that replays the fixtures captured from the original binary.

## Install

Download a binary from the [releases](https://github.com/winnieoursbrun/wkhtmltopdf-rs/releases)
(static Linux musl for x86_64 and aarch64, macOS x86_64 and arm64, Windows x86_64), or build it:

```sh
cargo build --release -p wkhtmltopdf   # target/release/wkhtmltopdf
```

The binary prints `wkhtmltopdf 0.12.6 (with patched qt)` for `--version`, like the original, because
wrappers test that string to enable the patched-Qt features (headers, footers, TOC, outline).

## Known divergences

Deliberate:

- **Virtual clock.** `--javascript-delay` and timers run on a virtual clock: a timer set for longer
  than the delay never fires, and a `--window-status` that never comes ends the wait after 60 s
  of virtual time (the original waits forever).
- `[date]` and `[time]` header/footer variables are in UTC.
- Custom `--xsl-style-sheet` TOCs run on an XSLT 1.0 subset (the default TOC is native).
- `document.write` after the load event is ignored.

Not yet matched:

- QtWebKit's pagination of tall table rows (one page of `tests/corpus/statement` differs).
- `<select>` boxes are drawn by hand in Qt's style (list box scroll bars are plain); canvas is painted
  as vectors where the original rasterises it.
- Quirks mode (no DOCTYPE) is rendered as standards mode: a line holding only an image keeps the
  text strut's descent (4 px taller at 16 px than in QtWebKit).
- A broken image (the alt-text box) loses the sizes given to `img` by style-sheet rules.
- `-webkit-text-stroke`, a float placed before a collapsing margin, flex/grid text extraction order
  in the WebKit-2012 profile.
- `--image-quality`, `--lowquality`, `--cache-dir` and client certificates have no effect.

## Implementation status

| Phase | Scope | State |
|---|---|---|
| P1 — CLI | All 122 options, object grammar, `--read-args-from-stdin`, documentation outputs, argument errors, settings model | **Done**: every `docs/upstream-fixtures/0.12.6/cli` case is byte-identical |
| P2 — static pipeline | Loader, HTML/CSS layout, paginator, PDF output | **P2a done**: pdfkit's invocation (stdin → stdout) and file/URL inputs produce real PDFs. 13 of 13 `conversion` fixtures reproduce stderr and exit code exactly (`toc` included). L2 on `tests/corpus`: page count and size match on 6/6 pages, and text similarity is ≥ 0.95 on every corpus page. **P2b done** (rendering fidelity): see below the table |
| P3 — wkhtmltopdf parts | Headers/footers, cover, TOC, outline, links | **Done**: text and HTML headers/footers with every variable, cover/TOC/page objects with per-object options, default and custom-XSL TOCs, outline and `--dump-outline`, named-destination links, `--page-offset`, forms, copies/collate, grayscale, including the upstream quirks of spec §10. `tests/parts`: 22 of 22 reference cases pass at L2 (`parts.rs`). Deviations: see below the table |
| P4 — JavaScript | Boa, DOM bindings, `--javascript-delay`, `--window-status` | **Done** (`crates/wk-js`): scripts run in document order with a script-pausing parser, `document.write`, events, timers and `Date` on a virtual clock, `--window-status`, `--run-script`, `window.print()`, `--debug-javascript`, dialogs, the slow-script budget, XMLHttpRequest over the loader, canvas 2D painted as vector. `warn` and `info_b` reproduce exactly; `tests/js` (jQuery 3.7.1 and 1.12.4, Chart.js 2.9.4, footer substitution, …) is at L2 parity on 13/13 cases. Gaps: see below the table |
| P5 — fidelity | Tables, forms, fonts, raster parity | **In progress**: the L3 raster check covers `tests/corpus`, `tests/parts` and `tests/js` (per-page thresholds in each `l3-thresholds.txt`). Header/footer geometry like QPrinter (device-pixel margins, FreeType text metrics, measured HTML header height), form controls in Qt's default style, canvas fill rule and dashes like QtWebKit 2.2, `text-overflow: ellipsis`, `small-caps`. Still divergent: QtWebKit's table-row pagination struts (statement page 2), canvas painted as vector where the reference rasterises it |
| P6 — release | Static musl builds, pdfkit test suite | **Alpha**: static musl/macOS/Windows release builds, pdfkit integration run, benchmark and visual parity reports (`bench/`) |

P4 gaps: timers fire on a virtual clock that stops at the end of the wait, so a timer set
during loading for more than `--javascript-delay` never fires (upstream's load time lets some of
them through); a `--window-status` that never comes ends the wait when no timer is pending or
after 60 s of virtual time (upstream waits forever); canvas pixel access (`getImageData`, `toDataURL`), patterns,
shadows and compositing modes are not implemented; a failed synchronous XHR prints its loader
error before the script continues (upstream: later); `document.write` after the load is ignored.

P2b (rendering fidelity): text metrics, tables, the WebKit-2012 CSS profile ([`docs/webkit2012-profile.md`](docs/webkit2012-profile.md)), shadows, outlines, `--minimum-font-size`, `--viewport-size` (media queries), `--image-dpi`, and an L3 raster check (`crates/wkhtmltopdf/tests/corpus_l3.rs`, thresholds in `tests/corpus/l3-thresholds.txt`). Still divergent: QtWebKit's tall table rows at page boundaries (statement page 2), a float placed before a collapsing margin, `-webkit-text-stroke`, web fonts picked by their internal family name, the server's HTTP reason phrase (canonical phrases are printed), `SSL error ignored` warnings, client certificates, `--cache-dir`, `--image-quality` and `--lowquality` (no visible effect found in the 0.12.6 binary), `orphans`/`widows` (not applied by 0.12.6 either).

P3 deviations: header/footer HTML is rendered without running its scripts (P4 plugs into
`wk_converter::scripts::run_scripts`); `[date]`/`[time]` are UTC; the custom TOC style sheet runs on
an XSLT 1.0 subset (`wk-converter/src/xslt.rs` lists what is supported; XSLT 2.0-only constructs
fail like a QtXmlPatterns static error, with an empty TOC); `Could not read the TOC XSL` stops the
conversion (upstream goes on after `fail()`); the measured height of a `--header-html` page is its
unshrunk body height, which is within a few points of QtWebKit's; form fields have no appearance
streams, like Qt's (`/NeedAppearances true`).

## Layout

| Crate | Ports | Contents |
|---|---|---|
| `crates/wkhtmltopdf` | `src/pdf/*` | The `wkhtmltopdf` binary: object grammar, switch registration, doc parts, program flow |
| `crates/wk-cli` | `src/shared/*` | Hand-written switch parser, argument handlers, text/man/html outputters, progress feedback |
| `crates/wk-settings` | `src/lib/*settings*`, `reflect` | `PdfGlobal`, `PdfObject`, `LoadPage`, `Web`, … with defaults, value parsers and dotted-name get/set |
| `crates/wk-converter` | `src/lib/pdfconverter`, `converter`, `outline`, `tocstylesheet`, `utilities` | Phase sequence, progress output, header/footer measuring, page counting, TOC loading (XML parser and XSLT subset), link resolution, header/footer variables and pages, printing, outline, `handleError` exit codes |
| `crates/wk-loader` | `src/lib/multipageloader` | stdin/file/`file://`/`http(s)`/`data:` loading (ureq + rustls), cookies and cookie jar, custom headers, POST, basic auth, proxies, local-file policy, `amfinished` error accounting |
| `crates/wk-webkit` | QtWebKit | Engine facade over blitz-dom/blitz-html/blitz-paint 0.3 (html5ever, stylo, taffy, parley): WebKit-2012 print layout (1 CSS px = 0.6 pt, smart shrinking, zoom, media type), resource ordering, display-list recording, the script-pausing parser and the page lifecycle with JavaScript (`scripting.rs`) |
| `crates/wk-js` | JavaScriptCore + WebCore bindings | Boa 0.22 realm per page: native DOM bindings with a `NodeId → JsObject` cache, a JS prelude (events, CSSOM, forms, location, navigator, XHR, canvas state), virtual-clock timers, console/dialog output like 0.12.6, canvas 2D vector recorder |
| `crates/wk-print` | QPrinter / QPdfEngine | Paginator (no cut lines/rows, forced breaks, thead/tfoot repetition, margin truncation) and krilla PDF painter (subset text with source text, JPEG passthrough, gradients, header/footer bands, texts and lines, named destinations, links, outline, Info dictionary, grayscale); AcroForm and `/PageMode` appended with lopdf |
| `crates/wk-fonts` | fontconfig | Directory font scan, generic families → DejaVu Sans like the reference image, embedded `fonts-dejavu-core` faces (Bitstream Vera licence, `crates/wk-fonts/fonts/LICENSE`) |

Vendored crates (`vendor/`, wired through `[patch.crates-io]`), each change marked `wkhtmltopdf-rs`:

| Crate | Change |
|---|---|
| `stylo` 0.22.0 | `break-before/after/inside` and `page-break-*` enabled in servo mode; `--minimum-font-size` |
| `blitz-paint` 0.3.0-beta.3 | glyph runs publish their source text (`run_text.rs`); list markers painted before the item text, like WebKit, and string markers (`1. `) ending one space width before it; canvas pictures recorded by script replayed in the canvas box (`canvas.rs`); blurred/inset box shadows without filters, `text-shadow`, rectangular outlines with `outline-offset`, collapsed table borders per cell edge; `text-overflow: ellipsis`; checkboxes and radios in Qt's default style |
| `blitz-dom` 0.3.0-beta.3 | WebKit auto table layout and collapsed borders, CSS rewrite hook (WebKit-2012 profile), whole-pixel font sizes, `line-height` numbers, no kerning/ligatures; `font-variant: small-caps`; text field and textarea sizes from `size`/`cols` with QtWebKit's baselines; a `<style>` appended in chunks is re-parsed |
| `stylo_taffy` 0.3.0-beta.3 | whole-pixel lengths (`roundForImpreciseConversion`), collapsed-cell border halves |
| `taffy` 0.15.0 | BFC roots keep their bottom padding/border below their floats |
| `parley` 0.12.0 | `line-height: normal` rounds ascent and descent up (QtWebKit/FreeType); glyph runs expose their cluster range |
| `parley_engine` 0.12.0 | QtWebKit's text measurement: linear 26.6 advances, "spaces + word" segments rounded to whole pixels |
| `boa_engine` 0.22 | `Script::with_realm`: the DOM prelude compiled once per thread |
| `krilla` 0.8.2 | hhea ascent/descent in font descriptors, cheaper deflate, fonts subset in parallel, page drafts drawn on other threads (`draft.rs`) |
| `pdf-writer` 0.15.0 | colour components and short decimals without ryu (same bytes) |
| `dlmalloc` 0.2.14 | freed pages given back to the system; the shared heap of the musl build's allocator (`crates/wkhtmltopdf/src/alloc.rs`) |

## Build and test (development)

```sh
cargo build --release          # target/release/wkhtmltopdf
cargo test                     # unit tests, quirk tests, CLI fixture replay
cargo clippy --all-targets -- -D warnings
```

Static Linux binaries cross-build from macOS with `rust-lld`; `ring` (rustls' crypto, the only C
code) is compiled by the system clang and archived with rustup's `llvm-ar`
(`rustup component add llvm-tools`):

```sh
BIN=$(dirname $(ls ~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-ar | head -1))
# aarch64: ring builds without libc headers; Apple clang's stddef.h needs __musl__ undefined.
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld CC_aarch64_unknown_linux_musl=clang \
CFLAGS_aarch64_unknown_linux_musl="--target=aarch64-unknown-linux-musl -U__musl__" \
AR_aarch64_unknown_linux_musl=$BIN/llvm-ar \
  cargo build --release --target aarch64-unknown-linux-musl -p wkhtmltopdf
# x86_64: ring needs musl's headers, e.g. copied from Alpine's musl-dev into $MUSL/include.
CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld CC_x86_64_unknown_linux_musl=clang \
CFLAGS_x86_64_unknown_linux_musl="--target=x86_64-unknown-linux-musl -nostdlibinc -isystem $MUSL/include" \
AR_x86_64_unknown_linux_musl=$BIN/llvm-ar \
  cargo build --release --target x86_64-unknown-linux-musl -p wkhtmltopdf
```

Stripped sizes (P2a): 16.6 MB (aarch64) and 19.2 MB (x86_64), embedded fonts included. Both run on
a bare `alpine` image.

`crates/wkhtmltopdf/tests/conversion_fixtures.rs` replays `docs/upstream-fixtures/0.12.6/conversion`
(stderr and exit code), and `crates/wkhtmltopdf/tests/corpus_l2.rs` compares our PDFs with the
reference PDFs of `tests/corpus/ref/` (made once with `tests/corpus/make-refs.sh` and the real
binary in Docker; the test itself needs no Docker). `crates/wkhtmltopdf/tests/parts.rs` does the same
for the document-parts cases of `tests/parts` (stderr, exit code, `--dump-outline`, structure facts
and text; see `tests/parts/README.md`). Tests set `WKHTMLTOPDF_SYSTEM_FONTS=0` so only
the embedded DejaVu faces are used.

`crates/wkhtmltopdf/tests/js_corpus.rs` does the same for the JavaScript corpus `tests/js`
(references made with `tests/js/make-refs.sh`), comparing stderr and the exit code exactly too.

`crates/wkhtmltopdf/tests/cli_fixtures.rs` replays every case of
`docs/upstream-fixtures/0.12.6/cli/capture.sh` and compares stdout, stderr and the exit code with
the outputs captured from the real binary. It runs the binary with `LC_ALL=C`, the locale the
fixtures were captured in (non-ASCII characters of `--license` depend on the locale, as upstream).

## Licence

LGPL-3.0-or-later, like wkhtmltopdf, whose help texts, manual and licence files are reproduced.
