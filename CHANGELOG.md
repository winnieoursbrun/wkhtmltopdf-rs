# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The behaviour being
reproduced is that of wkhtmltopdf 0.12.6 (with patched Qt).

## [Unreleased]

### Added

- Rendering fidelity corpus (`tests/fidelity/`): 75 feature-focused cases compared with PDFs
  rendered by wkhtmltopdf 0.12.6, per-page SSIM thresholds and a scoreboard (`REPORT.md`).

### Fixed

Mean per-page SSIM on the fidelity corpus went from 0.698 to 0.985.

- Pagination: page-top margins are truncated like QtWebKit, the CSS3 `break-*` names are ignored,
  fixed and absolute boxes are placed against the document span, pages are clipped horizontally
  to the printable area, `position: relative` moves inline elements.
- Tables: HTML table attributes (`border`, `frame`, `rules`, `cellpadding`, `cellspacing`,
  `align`, `valign`, `nowrap`), captions, WebKit's column and row sizing (spans, fixed layout,
  `<col>`), collapsed border styles, `empty-cells: hide`.
- Text: QtWebKit's word measurement and line breaking, glyphs at unrounded advances, whole-pixel
  `letter-spacing`, `word-spacing` and `line-height`, kerning and ligatures on spaced text,
  `@font-face` naming and failed loads, the bold fallback run bug, sub/super offsets, text
  decorations, CSS counters and quotes, all list marker types and WebKit bullet shapes.
- Forms drawn in Qt's Windows style (checkboxes, radios, buttons, list boxes, fieldset legends,
  range/progress/meter/file inputs, placeholders).
- `--zoom` and `--dpi` round lengths in device pixels and truncate border widths; `--no-images`
  keeps unsized images on their line.
- Inline boxes take their margin, border and padding in the line and paint them per fragment;
  outlines, Qt dash patterns, inset/outset/groove/ridge colours, blurred shadows, repeated
  backgrounds as tiling patterns, `unicode-bidi` embeddings and overrides, right-to-left list
  markers, `visibility: hidden` inline text.
- Zoom and DPI: QPrinter's whole-pixel page rectangle, text and box geometry rounded in device
  pixels, image natural sizes and table column widths in device pixels.
- Text: line breaks around punctuation as QtWebKit (`2026-10-01`, `/`, `?`), `::first-letter`,
  `::first-line`, `attr()` in `content`, soft hyphens, `list-style-image`, inside markers,
  bold `@font-face` metrics, `nowrap` overflow, inline-table baselines.
- Tables: a cell's height is a minimum, `min-`/`max-` sizes ignored on tables, middle-aligned
  content rounded down, cell line content kept on the baseline, collapsed dotted and dashed
  borders drawn like QtWebKit, auto column shares in WebKit's arithmetic order.
- Pagination: boxes run down to the page bottom when the content after them is pushed to the
  next page (background and side borders).
- Floats next to collapsing margins, replaced and percentage sizes truncated to device pixels,
  unthemed `--enable-forms` widgets, `repeat-x`/`repeat-y` anchoring, broken image icons.
- SVG text is drawn with the engine's fonts (it was missing on hosts without system fonts).

## [0.1.0-rc.1] - 2026-10-16

First release candidate for 0.1.0. No functional change since 0.1.0-alpha.3: the command-line
contract, the rendering and the performance are those of alpha.3. Remaining divergences from
wkhtmltopdf 0.12.6 are listed in the README (Known divergences).

## [0.1.0-alpha.3] - 2026-10-15

### Changed

- Performance pass 5 (`docs/perf.md`): Stylo traversal and inline layout construction on a
  bounded worker pool (min(CPUs, 4) threads, `RAYON_NUM_THREADS` overrides), PDF pages drafted
  in parallel and applied in order, document drop and system font scan off the critical path,
  and a dlmalloc-based allocator with per-thread caches for the static musl build (64-bit).
  Large invoice: 148 → 95 ms in the benchmark container. Output is byte-identical.
- `wkhtmltopdf-inproc` (benchmark harness) uses the same allocator as the CLI.

### Fixed

- The allocator purges whole pages of the kernel's real page size (16/64 KiB page kernels).
- A panic while drafting a page no longer hangs the conversion; allocator consistency failures
  abort instead of deadlocking.

### Added

- CI runs the test suite on `x86_64-unknown-linux-musl`.
- `tests/threads.rs`: drafted pages give the same PDF as pages drawn sequentially.

## [0.1.0-alpha.2] - 2026-10-15

### Added

- JavaScript (P4, `crates/wk-js`): Boa realm per page with DOM bindings, scripts in document
  order (script-pausing parser, external scripts through the loader, `document.write`),
  `DOMContentLoaded`/`load`/`<body onload>`, timers and `Date` on a virtual clock,
  `--javascript-delay`, `--window-status`, `--run-script`, `window.print()`,
  `--debug-javascript` and dialog output like 0.12.6, `--stop-slow-scripts`,
  `--disable-javascript`, XMLHttpRequest over the loader, canvas 2D painted as vector.
- `tests/js`: JavaScript corpus (jQuery 3.7.1 and 1.12.4, Chart.js 2.9.4, footer substitution,
  timers, `document.write`, …) with outputs of the real binary.

### Changed

- Performance passes 3 and 4 (`docs/perf.md`): user-agent stylesheet parsed once, on a worker;
  layout at print size while loading; display list indexed by position; JS prelude compiled
  once; page content streams and font subsets compressed in parallel (at most 2 workers,
  `RAYON_NUM_THREADS` overrides); deflate with 48 hash-chain probes (same decompressed content,
  output +0.4 %); faster number formatting. Large invoice: 261 → 148 ms in the benchmark
  container.
- Benchmark against OpenHTMLtoPDF (Java) in `bench/java`.

### Fixed

- A missing local file's error reads `Error opening <path>`, like Qt.
- Outside string list markers end one space width before the item text, like WebKit.

## [0.1.0-alpha.1] - 2026-10-14

First public pre-release. Not a drop-in replacement yet: see the status table in the README.

### Added

- `wkhtmltopdf` binary with the complete 0.12.6 command line: all 122 options, the object
  grammar (`cover`, `toc`, pages), `--read-args-from-stdin`, and the documentation outputs
  (`-h`, `-H`, `--readme`, `--license`, `--version`, `--manpage`, `--htmldoc`), byte-identical
  to the original on every captured CLI fixture.
- Settings model (`PdfGlobal`, `PdfObject`, `LoadPage`, `Web`, …) with upstream defaults,
  value parsers and dotted-name get/set.
- Static builds for Linux (x86_64 and aarch64, musl), macOS (x86_64 and aarch64) and Windows
  (x86_64, MSVC).

### Known limitations

-

[Unreleased]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/compare/v0.1.0-alpha.3...v0.1.0-rc.1
[0.1.0-alpha.3]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/compare/v0.1.0-alpha.2...v0.1.0-alpha.3
[0.1.0-alpha.2]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/releases/tag/v0.1.0-alpha.1
