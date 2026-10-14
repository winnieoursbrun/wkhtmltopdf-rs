# Spike: static render pipeline — which base do we build the engine on?

Date 2026-10-14, Apple M1, macOS arm64, rustc 1.99.0. Reference: wkhtmltopdf 0.12.6 (patched qt) in
`wkref:local` (DejaVu fonts only), pdfkit default invocation
`--quiet --page-size Letter --margin-* 0.75in --encoding UTF-8 - -`.

## Recommendation

Blitz 0.3 (`blitz-dom`/`blitz-html`/`blitz-paint` 0.3.0-beta.3) + own paginator + own krilla
painter. Do not fork fulgur; use its source as reading material for paged media (GCPM, counters,
margin boxes). Do not write our own DOM now: DOM, style and layout were not the weak layers. Fork
Blitz when JS (P4) needs node identity and mutation hooks.

1. fulgur 0.40 is on the previous stack (blitz 0.2, stylo 0.8, parley 0.6, taffy 0.9, krilla 0.7,
   html5ever 0.35). Current releases are blitz 0.3.0-beta.3, stylo 0.22, parley 0.12, taffy 0.15,
   krilla 0.8.2, html5ever 0.40. Stylo's `links = "servo_style_crate"` makes the two stacks
   unresolvable together. A fork means porting about 50 kLOC across four breaking upgrades.
2. fulgur is offline by design. Its net provider accepts only `file://`, so `data:` images and fonts
   (the usual pdfkit pattern, because stdin input has no base URL) are lost. Bundled fonts get no
   generic families, so a page without `font-family` is blank when system fonts are off. It forces
   blitz-dom default features, which pull in fontconfig.
3. Route (b) beats (a) on the corpus with about 670 lines: page count matches on 4 of 6 pages (5 of
   6 if b.html, which is not HTML, is set aside), text similarity is 0.97–1.00 on the Rails-like
   pages, thead is repeated, and data: images and fonts work.
4. Route (b) matches DESIGN.md and passes `cargo check` for x86_64-unknown-linux-musl with no C
   toolchain once system-font features are disabled.

## Built

- (a) `route-a-fulgur/` (separate package with its own `[workspace]`): `fulgur::Engine::render`,
  then lopdf rescaling (fulgur hard-codes 0.75 pt/px).
- (b) `src/`: html5ever → blitz-dom (stylo, taffy, parley) → blitz-paint into a recorded
  `anyrender::Scene` → own paginator (lay out one tall page, then slice it) → scene→krilla replayer.

CLI: `spike in|- out|- [--page-size A4|Letter] [--margin-in N|--margin-mm T,R,B,L]`. Defaults are
A4 with 10 mm margins. Fonts come from `SPIKE_FONT_DIR`: six DejaVu TTFs copied from the reference
image, with generic families mapped to DejaVu Sans and Sans Mono.

Geometry: wkhtmltopdf lays out at 1.25× the printable width at 96 dpi, so 1 CSS px = 0.6 pt.
Verified: the 8 px body margin lands at 33 pt, which is 28.35 + 4.8. Route (b) uses an 840 px
viewport for Letter with 0.75 in margins.

Tools: `corpus/build.py`, `compare.sh` (poppler pdftotext/pdftoppm at 50 dpi, ImageMagick sheets in
`out/*.sheet.png`, columns ref | blitz | fulgur), `textdiff.py` (word difflib ratio after NFKC
normalisation).

## Build facts

| | (a) fulgur | (b) blitz |
|---|---|---|
| Versions | fulgur 0.40.0, blitz-dom 0.2.4, stylo 0.8.0, parley 0.6.0, taffy 0.9.2, krilla 0.7.0, html5ever 0.35.0, lopdf 0.42.0 | blitz 0.3.0-beta.3, stylo 0.22.0, parley 0.12.0, taffy 0.15.0, krilla 0.8.2, anyrender 0.14.0, html5ever 0.40.1, skrifa 0.44 |
| Crates (musl graph) | 399 | 320 |
| Cold release build (deps already downloaded) | 139 s wall / 607 s CPU | 146 s wall / 559 s CPU |
| Stripped binary (thin LTO) | 16.2 MB | 15.9 MB |
| musl `cargo check` | passes; fontique 0.6's fontconfig backend is unconditional (dlopen, unusable in a static binary) | passes after removing `blitz-dom/system-fonts` and parley's default `system` feature; with them it fails in yeslogic-fontconfig-sys (pkg-config) |
| Render time per page | 10–40 ms | 20–40 ms |

A musl link was not attempted.

## Comparison (all pages 612×792 pt)

| Page | Ref pages | Blitz pages | Blitz sim (doc / per page) | Fulgur pages | Fulgur sim (doc / per page) |
|---|---|---|---|---|---|
| a.html fixture | 1 | 1 | 1.00 | 1 (blank) | 0.00 |
| b.html fixture (a PDF file) | 2 | 4 | 0.57 / .52 .15 0 0 | 1 (blank) | 0.00 |
| invoice | 2 | 2 | 0.99 / .97 .96 | 2 | 0.98 / .96 .96 |
| report | 7 | 7 | 1.00 / 1 .99 .46 1 1 1 1 | 8 | 0.98 / 1 0 .01 .05 … |
| fonts | 1 | 1 | 0.99 | 1 | 0.93 |
| layout | 1 | 1 | 0.97 | 1 | 0.86 |
| statement | 4 | 3 | 0.98 / .93 .83 .04 0 | 4 | 0.97 / .95 .88 .45 .92 |

Rows per page:

- invoice: ref 39+26, blitz 41+24, fulgur 37+28
- statement: ref 53+54+13, blitz 60+60, fulgur 56+60+4

## Works (b)

- Pages without a doctype parse fine.
- `@media screen` applies rather than `print`, as with pdfkit's default.
- Supported: `:nth-child`, floats (wrapping text and columns), inline-block, lists, absolute
  positioning, border-radius, dashed and dotted borders, justify, nowrap.
- `@font-face` loads from a data: URI.
- Weights, synthetic oblique, and UTF-8 accents and symbols render correctly.
- Real subset text is written to the PDF.
- data: PNG images load.
- Forced breaks work, and adjacent forced breaks are merged.
- No line or table row is cut across pages.
- thead is repeated on continuation pages.
- Text is emitted on exactly one page.

## Gaps

- **HTML:** b.html (PDF bytes) gives 4 pages vs 2.
- **Style:** `page-break-*`, `break-*`, `orphans` and `widows` are `engine="gecko"` in stylo
  (`properties/longhands.toml` and `shorthands.toml`), so servo mode drops them. Fix with a stylo
  fork or a side parser; the spike smuggles them in as custom properties. `-webkit-` prefixes are
  unsupported. Unprefixed `transform` is applied by Blitz but ignored by QtWebKit 2.2, so the PAYÉ
  stamp is rotated in ours and upright in the reference. Iso needs a "WebKit-2012 profile".
- **Layout:** `line-height: normal` is about 13 % shorter than QtWebKit's with DejaVu; this is the
  main cause of page-count drift. Blitz rowspan/colspan is broken. thead, tbody and tr get no box of
  their own (the table is a grid of cells), so row extents are rebuilt from cells. tfoot appears
  once, but wkhtmltopdf repeats it on every page. The first line sits about 8 px lower.
- **Paginator:** missing tfoot repetition, orphans/widows, splitting cells taller than a page,
  margin truncation, and QtWebKit's exact heuristics (report page 3: the reference keeps 2 lines,
  we keep 1).
- **Paint/PDF:** anyrender glyph runs carry no source text. The spike rebuilds it by reverse cmap
  lookup; a blitz-paint `text.rs` fork that passes cluster ranges is needed. Gradients use the
  first stop colour only; shadows and filters are dropped. There is no JPEG passthrough, no links
  and no outline. Content outside a page must be culled, not just clipped, or it stays extractable
  (fixed for glyph runs).
- **Resources:** blitz-dom's `image` dependency ships with no codecs, so the embedder must enable
  png/jpeg/gif. We need our own font discovery (directory scan, fontconfig aliases, embedded
  fallback). Only data: and file: URLs are served.
- **fulgur specifics:** blank pages without `font-family`, data: assets dropped (the BrandMono line
  disappears), no thead repetition, tfoot drawn under the header on page 1, float columns stacked,
  no list markers, an empty page between cover and chapter.

## P2 estimate (person-weeks)

| Item | Estimate |
|---|---|
| Fork blitz 0.3 and stylo, enable break properties | 1–2 |
| Paginator | 4–6 |
| krilla painter | 3–4 |
| Fonts backend | 1–2 |
| WebKit-2012 compatibility (scale, smart shrink, line-height, UA sheet, prefixes, tables) | 4–8 |
| Loader | 3–4 |
| Comparison harness | 1–2 |
| **Total** | **17–28** |

The main uncertainty is matching QtWebKit's line heights and table layout.

## Reproduce

```sh
python3 corpus/build.py
cargo build --release
(cd route-a-fulgur && cargo build --release)
WK_BIN_DIR=<dir> ./compare.sh
python3 textdiff.py
```
