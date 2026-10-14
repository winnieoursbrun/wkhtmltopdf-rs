---
name: Rendering difference
about: The Rust port produces a PDF (or exit code, or output) that differs from wkhtmltopdf 0.12.6
title: "Rendering: "
labels: ["rendering"]
---

<!-- One difference per issue, please. The smaller the reproduction, the faster the fix. -->

### What differs

<!-- Page count, page size, text, layout, fonts, links, outline, header/footer, exit code,
     stderr messages... A sentence or two is enough. -->

### HTML input

<!-- Attach the HTML file (and its CSS/images), or paste a minimal self-contained page below.
     Remove anything confidential. -->

```html

```

### Command line

<!-- The exact wkhtmltopdf command line, identical for both binaries. If you call it through a
     library (pdfkit, wicked_pdf, python-pdfkit...), give the library, its version and the
     generated command if you can (pdfkit logs it). -->

```sh
wkhtmltopdf ...
```

### PDFs

- [ ] Original wkhtmltopdf 0.12.6 output attached (`original.pdf`)
- [ ] wkhtmltopdf-rs output attached (`ours.pdf`)

<!-- Drag and drop both PDFs here. A screenshot of the differing area helps too. -->

### Versions

- Original: <!-- output of `wkhtmltopdf --version`, e.g. "wkhtmltopdf 0.12.6.1 (with patched qt)" -->
- wkhtmltopdf-rs: <!-- release tag or commit -->
- OS / architecture: <!-- e.g. Debian 12 arm64, macOS 15 arm64, Windows 11 x64 -->
- Fonts installed, if text or metrics differ: <!-- e.g. fonts-dejavu-core only -->
