# The WebKit-2012 profile

wkhtmltopdf 0.12.6 renders with QtWebKit 2.2 (WebKit 534.34, 2011). Our engine (stylo, taffy,
parley) is a 2020s engine, so it has to be held back to what that WebKit understood, and taught the
`-webkit-` properties pages still use for it. This document lists the decisions; each was checked
against the real binary with the probe pages `tests/corpus/webkit2012.html` and
`webkit2012b.html` (references in `tests/corpus/ref/`, compared by `corpus_l2` and `corpus_l3`).

Implementation:

- **CSS rewrite** (`crates/wk-webkit/src/profile.rs`): author CSS (`<style>`, linked and imported
  sheets, `style` attributes, `--user-style-sheet`) is rewritten before stylo parses it, through the
  hook `blitz_dom::css_rewrite`. A declaration is kept, dropped, renamed or has its value converted;
  `@supports` blocks are removed; everything else is copied.
- **Layout and text metrics** (vendored blitz-dom, stylo_taffy, taffy, parley_engine): see the
  second table.
- **Paint** (vendored blitz-paint): see the third table.

## CSS properties and values

"Ignored" means the reference renders as if the declaration were absent; we drop it.

| CSS | QtWebKit 2.2 (0.12.6) | Ours | Evidence |
|---|---|---|---|
| `transform` (unprefixed) | ignored | dropped | probe: box upright |
| `-webkit-transform`, `-webkit-transform-origin` | applied (rotate, translate, scale, skew) | mapped to `transform*` | probe |
| `box-shadow` / `-webkit-box-shadow` | both applied, incl. blur, spread, inset, several shadows | prefixed mapped; blur approximated (see paint) | probe |
| `border-radius` / `-webkit-border-radius` / `-webkit-border-*-radius` | both applied | prefixed mapped | probe |
| `box-sizing` / `-webkit-box-sizing` | both applied | prefixed mapped | probe |
| `background-size` / `-webkit-background-size` (also `-clip`, `-origin`) | both applied | prefixed mapped | probe |
| `linear-gradient()`, `radial-gradient()` (unprefixed, also `repeating-`) | ignored (declaration invalid) | dropped | probe: plain background |
| `-webkit-linear-gradient(<side> \| <angle>, …)` | applied, legacy semantics (`left` = starts at the left; angles counter-clockwise from east) | rewritten to `linear-gradient(to <opposite side> \| 90°−a, …)` | probe |
| `-webkit-radial-gradient([pos,] [shape size,] …)` | applied | rewritten to `radial-gradient(shape size at pos, …)` | probe |
| `-webkit-gradient(linear\|radial, …, from(), to(), color-stop())` | applied | rewritten to a standard gradient (direction from the points) | probe |
| `display: flex \| inline-flex \| grid \| -webkit-flex` and all flex/grid properties, `gap` | ignored (2012 flexbox and grid did not exist) | dropped | probe: inline flow |
| `display: -webkit-box \| -webkit-inline-box` + `-webkit-box-orient/-direction/-pack/-align/-flex/-ordinal-group` | applied (2009 flexbox) | mapped to flexbox (`justify` = `space-between`, `box-flex: n` = grow and shrink `n`) | box probe (divs) |
| `calc()`, `-webkit-calc()`, `min()`, `max()`, `clamp()` | ignored | declaration dropped | probe |
| `var()` and custom properties `--x` | ignored | dropped | probe |
| `vw`, `vh`, `vmin`, `vmax`, `ch` units | ignored | declaration dropped | probe |
| `rem` | applied | kept | probe |
| `opacity` | parsed, but the PDF shows the content fully opaque (Qt's PDF engine drops the transparency layer) | dropped | probe (text and box) |
| `rgba()`, `hsla()` colours | applied | kept | probe |
| `filter`, `-webkit-filter` | ignored | dropped | probe |
| `column-count` / `-webkit-column-count` | no columns in the PDF | dropped | probe |
| `@supports` | unknown at-rule: block ignored | block removed | probe |
| `break-before`, `break-after`, `break-inside` (CSS3 names) | ignored (only `page-break-*` existed) | dropped | probe and `tests/fidelity/cases/breaks.html` |
| `page-break-before/after: left \| right` | a plain page break, no blank page inserted | treated as `always` | probe |
| `-webkit-column-break-*` | no break (no columns) | dropped | probe |
| `overflow-wrap` | ignored (only `word-wrap` existed) | dropped (`word-wrap` kept) | probe |
| `text-decoration-color` and the other longhands | ignored | dropped | probe |
| `outline-offset` | applied | kept (see paint) | probe |
| `text-shadow` | applied | kept, **not painted yet** | probe |
| `text-overflow: ellipsis` | applied | kept; glyphs past (box width - `…` width) dropped, `…` drawn after them (blitz-paint) | probe |
| `font-variant: small-caps` | applied | kept; lowercase letters uppercased at 0.7 of the font size, like WebKit's smallCapsFontData (blitz-dom inline construction) | probe |
| `-webkit-text-stroke` | applied | dropped (unsupported by stylo) | probe |
| `border-image` with a gradient | ignored | kept (also ignored by us) | probe |
| other `-webkit-*`, `-moz-*`, `-ms-*` | — | dropped | |
| `position: sticky`, `object-fit`, `aspect-ratio`, logical properties, `inset`, … | did not exist | dropped | WebKit 534 sources |

## Layout and text metrics

| Behaviour | QtWebKit 2.2 | Where |
|---|---|---|
| Font size | the computed size rounded to whole pixels (`computedPixelSize`): 11pt = 14.67px is set at 15px; `em` lengths keep the unrounded size | blitz-dom `stylo_to_parley.rs` |
| Glyph advances | linear (unhinted) advances in 26.6 fixed point, truncated | parley_engine `shaped_text.rs` |
| Kerning, ligatures | none in measurement (the simple text path adds advances) | blitz-dom `stylo_to_parley.rs` |
| Word widths | the line breaker measures "spaces + word" segments, each a whole number of pixels (`QFontMetrics::width`) | parley_engine `shaped_text.rs` |
| `line-height: <number>` | `trunc(number × rounded font size)` | blitz-dom `stylo_to_parley.rs` |
| `line-height: normal` | ascent and descent each rounded up | parley (P2a) |
| Lengths from `em`, `pt`, … | whole pixels, truncated (`roundForImpreciseConversion`) | stylo_taffy `convert.rs` |
| Auto table layout | AutoTableLayout: columns get their min width, then the rest in proportion to their max widths; fixed and percent columns first | blitz-dom `layout/table_auto.rs` |
| Collapsed borders | resolved per cell edge (width, then style; cell, row, table); each cell takes half, the extra pixel on the top/left | blitz-dom `layout/table.rs` |
| Floats in a BFC root | contained above the box's bottom padding and border | taffy `compute/block.rs` |

## Paint

| Behaviour | QtWebKit 2.2 | Ours |
|---|---|---|
| Blurred box shadows | blurred | approximated by nested rings with a linear alpha ramp (a PDF has no blur) |
| Inset box shadows | painted inside the padding box | padding box minus the offset hole, no compositing |
| Dotted border with `border-radius` | drawn solid | drawn solid (dashed stays dashed) |
| Outlines | rectangles (no radius), `outline-offset` away from the border box | same |
| Collapsed table borders | painted after the cell backgrounds | same |
| Inline box backgrounds | ascent and descent rounded up | same |
