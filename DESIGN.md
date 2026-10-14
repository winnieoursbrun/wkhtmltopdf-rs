# wkhtmltopdf-rs — design

A pure-Rust, single-binary reimplementation of `wkhtmltopdf` 0.12.6 (with patched Qt),
meant as a drop-in replacement behind the `pdfkit` Ruby gem.

Reference documents:

- `docs/upstream-spec.md` — behaviour of the original, extracted from the C++ sources and
  verified against the real 0.12.6 binary.
- `docs/upstream-fixtures/0.12.6/` — captured outputs of the real binary (CLI and conversions).
  The reference binary runs in the local Docker image `wkref:local` (amd64).
- `docs/rust-stack-survey.md` — state of the Rust ecosystem for each engine layer.

## 1. Compatibility target

"Iso" is defined at three levels, in decreasing order of strictness:

| Level | Target | How it is checked |
|---|---|---|
| L1 — process contract | Byte-identical stdout/stderr and identical exit code for every CLI path (help, version, docs, argument errors, progress output, load errors) | `tests/cli_fixtures.rs` replays `docs/upstream-fixtures/0.12.6/cli/*` |
| L2 — document structure | Same page count, page size, margins, text content per page, links, outline, TOC entries, header/footer text | PDF text + structure diff against `wkref:local` output |
| L3 — rendering | Visually equivalent pages (layout, fonts, colours, breaks) | Raster diff (hayro) with a tolerance |

Pixel-identity with QtWebKit (2012 WebKit) is a non-goal: the engine is different. L1 and L2 are
hard requirements; L3 is a quality metric tracked per test page.

The version string stays `wkhtmltopdf 0.12.6 (with patched qt)`: every patched-Qt-only option
(headers/footers, TOC, outline, forms, smart shrinking…) is implemented, so the binary must
advertise the patched mode that callers test for.

## 2. Architecture — same shape as upstream

Upstream is a thin C++ layer (~10 kLOC) over a patched QtWebKit. We keep the same layering and
module boundaries; the QtWebKit layer is rebuilt from Rust components.

```
wkhtmltopdf (bin)              ← src/pdf/*            CLI entry, object grammar, doc parts
 ├─ wk-cli                     ← src/shared/*         arg parser, outputters (text/man/html), progress feedback
 ├─ wk-settings                ← src/lib/*settings*   PdfGlobal / PdfObject / LoadPage / Web / HeaderFooter / TOC, reflect
 └─ wk-converter               ← src/lib/pdfconverter, converter, outline, tocstylesheet, utilities
     ├─ wk-loader              ← src/lib/multipageloader   resources, cookies, auth, proxy, load-error policy, exit codes
     ├─ wk-webkit              ← QtWebKit (QWebPage/QWebFrame/WebCore/JSC)
     │   ├─ dom                html5ever tree builder → arena DOM implementing Stylo traits (blitz-dom as base)
     │   ├─ style              stylo cascade, @media print/screen, minimum-font-size, zoom
     │   ├─ layout             taffy (block/flex/grid/floats) + parley (inline/text), tables
     │   ├─ js                 boa_engine + WebIDL-generated DOM bindings, event loop, timers (virtual clock)
     │   └─ paint              display list (backgrounds, borders, text runs, images, svg, transforms)
     └─ wk-print               ← QPrinter + QPdfEngine (patched)
         ├─ paginate           fragmentation into pages, smart shrinking, break-* rules
         ├─ pdf                display list → krilla (real text, font subsetting, links, outline, forms via pdf-writer)
         └─ parts              header/footer pages per page, cover, TOC (native generator + XSLT for custom XSL)
```

Rules:

- **Upstream parity is structural.** Each Rust module states, at its top, which upstream file(s) it
  ports. Behavioural quirks of upstream (documented in `docs/upstream-spec.md` §10) are reproduced
  deliberately and covered by a test named after the quirk.
- **No C/C++ engines, no browser process.** Allowed native code: `ring` (rustls crypto), linked
  statically. Fonts are discovered by scanning directories (no fontconfig) and the binary embeds a
  fallback font set so it renders on a bare container.
- **Determinism.** JS timers and `Date` run on a virtual clock driven by the page load
  (`--javascript-delay` advances it), so output does not depend on machine speed. The slow-script
  interrupt (`--stop-slow-scripts`, default) is a loop-iteration budget (~10 s of Boa), not a
  wall-clock one; `--no-stop-slow-scripts` removes it.
- **Static release build.** `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets,
  plus macOS for development.

## 3. Engine stack

| Upstream component | Replacement |
|---|---|
| WebCore HTML parser | `html5ever` (script-pausing for `document.write`) |
| WebCore DOM | arena DOM forked from `blitz-dom` |
| WebCore CSS | `stylo` (Servo/Firefox style system) |
| WebCore render tree / layout | `taffy` + `parley`, own table layout |
| JavaScriptCore | `boa_engine` |
| QtNetwork | `ureq` 3 + `rustls` + `cookie_store` |
| Qt fonts / HarfBuzz | `skrifa` + `harfrust` + directory font discovery |
| Qt image formats | `image`, `usvg` |
| QPrinter / QPdfEngine | own paginator + `krilla` (+ `pdf-writer` for AcroForm) |
| QXmlQuery (XSLT for TOC) | native default-TOC generator; `xrust` for custom `--xsl-style-sheet` |

## 4. Delivery phases

Each phase ends with a working binary and a green test suite; nothing later breaks an earlier
level.

| Phase | Scope | Exit criterion |
|---|---|---|
| P1 — CLI | Full option parser (all 122 options, object grammar, `--read-args-from-stdin`), all documentation outputs, argument errors, settings model | Every `cli/` fixture byte-identical (stdout, stderr, exit code) |
| P2 — static pipeline | Loader (file/http/stdin, cookies, headers, post, auth, proxy, local-file policy, exit codes 1/2/3), DOM + style + layout for static HTML/CSS, paginator, krilla painter, progress output | pdfkit's default invocation produces a PDF with L2 parity on the static corpus |
| P3 — wkhtmltopdf parts | header/footer HTML and text with substitution variables, cover, TOC, outline, links, `--dump-outline`, page offset, multiple objects, smart shrinking, zoom/dpi/viewport | L2 parity on the parts corpus |
| P4 — JavaScript | Boa + DOM bindings, event loop, `--javascript-delay`, `--window-status`, `--run-script`, `--debug-javascript`, canvas 2D | Corpus with jQuery and a chart library renders; header/footer JS substitution script works |
| P5 — fidelity | Tables, counters, forms, `--grayscale`, `--lowquality`, image dpi/quality, fonts fallback, L3 raster diffs | L3 metric above threshold on the whole corpus |
| P6 — release | musl static builds, size/perf budget, pdfkit integration test suite, packaging | `pdfkit` test suite green with `WKHTMLTOPDF` pointing at our binary |

## 5. Licence

The port reproduces upstream texts (help, manual, default TOC stylesheet) and mirrors its
structure: the project is **LGPL-3.0-or-later**, like wkhtmltopdf. Stylo is MPL-2.0 (file-level),
compatible.
