# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). The behaviour being
reproduced is that of wkhtmltopdf 0.12.6 (with patched Qt).

## [Unreleased]

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

## [0.1.0-alpha.1] - unreleased

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

[Unreleased]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/winnieoursbrun/wkhtmltopdf-rs/releases/tag/v0.1.0-alpha.1
