# wkhtmltopdf-rs

A pure-Rust, single-binary reimplementation of `wkhtmltopdf` 0.12.6 (with patched Qt), meant as a
drop-in replacement behind the `pdfkit` Ruby gem. No Qt, no WebKit, no browser process.

The target is the observable behaviour of the released 0.12.6 binary, down to its quirks. See
[`DESIGN.md`](DESIGN.md) for the architecture and compatibility levels, and
[`docs/upstream-spec.md`](docs/upstream-spec.md) for the behaviour being reproduced.

## Status

| Phase | Scope | State |
|---|---|---|
| P1 — CLI | All 122 options, object grammar, `--read-args-from-stdin`, documentation outputs, argument errors, settings model | **Done**: every `docs/upstream-fixtures/0.12.6/cli` case is byte-identical |
| P2 — static pipeline | Loader, HTML/CSS layout, paginator, PDF output | **P2a done**: pdfkit's invocation (stdin → stdout) and file/URL inputs produce real PDFs. 10 of 13 `conversion` fixtures reproduce stderr and exit code exactly; `warn` and `info_b` match except for their JavaScript lines (P4); `toc` needs P3. L2 on `tests/corpus`: page count and size match on 6/6 pages, and text similarity is ≥ 0.95 on every corpus page. P2b: see below the table |
| P3 — wkhtmltopdf parts | Headers/footers, cover, TOC, outline, links | Started in P2a: PDF links (external and internal), outline bookmarks with `--outline-depth`, `--dump-outline` without TOC entries, multiple page/cover objects in one PDF (a TOC object prints a blank placeholder page). Headers/footers and the TOC are not started |
| P4 — JavaScript | Boa, DOM bindings, `--javascript-delay`, `--window-status` | Not started |
| P5 — fidelity | Tables, forms, fonts, raster parity | Not started |
| P6 — release | Static musl builds, pdfkit test suite | Not started |

P2b (rendering fidelity): text metrics, tables, the WebKit-2012 CSS profile ([`docs/webkit2012-profile.md`](docs/webkit2012-profile.md)), shadows, outlines, `--minimum-font-size`, `--viewport-size` (media queries), `--image-dpi`, and an L3 raster check (`crates/wkhtmltopdf/tests/corpus_l3.rs`, thresholds in `tests/corpus/l3-thresholds.txt`). Still divergent: QtWebKit's tall table rows at page boundaries (statement page 2), a float placed before a collapsing margin, `text-overflow: ellipsis`, `small-caps`, `-webkit-text-stroke`, web fonts picked by their internal family name, the server's HTTP reason phrase (canonical phrases are printed), `SSL error ignored` warnings, client certificates, `--cache-dir`, `--image-quality` and `--lowquality` (no visible effect found in the 0.12.6 binary), `orphans`/`widows` (not applied by 0.12.6 either).

## Layout

| Crate | Ports | Contents |
|---|---|---|
| `crates/wkhtmltopdf` | `src/pdf/*` | The `wkhtmltopdf` binary: object grammar, switch registration, doc parts, program flow |
| `crates/wk-cli` | `src/shared/*` | Hand-written switch parser, argument handlers, text/man/html outputters, progress feedback |
| `crates/wk-settings` | `src/lib/*settings*`, `reflect` | `PdfGlobal`, `PdfObject`, `LoadPage`, `Web`, … with defaults, value parsers and dotted-name get/set |
| `crates/wk-converter` | `src/lib/pdfconverter`, `converter`, `outline`, `utilities` | Phase sequence, progress output, page counting, link resolution, outline, `handleError` exit codes |
| `crates/wk-loader` | `src/lib/multipageloader` | stdin/file/`file://`/`http(s)`/`data:` loading (ureq + rustls), cookies and cookie jar, custom headers, POST, basic auth, proxies, local-file policy, `amfinished` error accounting |
| `crates/wk-webkit` | QtWebKit | Engine facade over blitz-dom/blitz-html/blitz-paint 0.3 (html5ever, stylo, taffy, parley): WebKit-2012 print layout (1 CSS px = 0.6 pt, smart shrinking, zoom, media type), resource ordering, display-list recording |
| `crates/wk-print` | QPrinter / QPdfEngine | Paginator (no cut lines/rows, forced breaks, thead/tfoot repetition, margin truncation) and krilla PDF painter (subset text with source text, JPEG passthrough, gradients, links, outline, Info dictionary) |
| `crates/wk-fonts` | fontconfig | Directory font scan, generic families → DejaVu Sans like the reference image, embedded `fonts-dejavu-core` faces (Bitstream Vera licence, `crates/wk-fonts/fonts/LICENSE`) |

Vendored crates (`vendor/`, wired through `[patch.crates-io]`), each change marked `wkhtmltopdf-rs`:

| Crate | Change |
|---|---|
| `stylo` 0.22.0 | `break-before/after/inside` and `page-break-*` enabled in servo mode; `--minimum-font-size` |
| `blitz-paint` 0.3.0-beta.3 | glyph runs publish their source text (`run_text.rs`); list markers painted before the item text, like WebKit |
| `blitz-dom` 0.3.0-beta.3 | WebKit auto table layout and collapsed borders, CSS rewrite hook (WebKit-2012 profile), whole-pixel font sizes, `line-height` numbers, no kerning/ligatures |
| `stylo_taffy` 0.3.0-beta.3 | whole-pixel lengths (`roundForImpreciseConversion`), collapsed-cell border halves |
| `taffy` 0.15.0 | BFC roots keep their bottom padding/border below their floats |
| `parley` 0.12.0 | `line-height: normal` rounds ascent and descent up (QtWebKit/FreeType); glyph runs expose their cluster range |
| `parley_engine` 0.12.0 | QtWebKit's text measurement: linear 26.6 advances, "spaces + word" segments rounded to whole pixels |

## Build and test

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
binary in Docker; the test itself needs no Docker). Tests set `WKHTMLTOPDF_SYSTEM_FONTS=0` so only
the embedded DejaVu faces are used.

`crates/wkhtmltopdf/tests/cli_fixtures.rs` replays every case of
`docs/upstream-fixtures/0.12.6/cli/capture.sh` and compares stdout, stderr and the exit code with
the outputs captured from the real binary. It runs the binary with `LC_ALL=C`, the locale the
fixtures were captured in (non-ASCII characters of `--license` depend on the locale, as upstream).

## Licence

LGPL-3.0-or-later, like wkhtmltopdf, whose help texts, manual and licence files are reproduced.
