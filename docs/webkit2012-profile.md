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
| `font-variant: small-caps` | applied | kept; lowercase letters uppercased at 0.7 of the font size, rounded to whole pixels, like WebKit's smallCapsFontData (blitz-dom inline construction) | probe |
| `-webkit-text-stroke` | applied | dropped (unsupported by stylo) | probe |
| `border-image` with a gradient | ignored | kept (also ignored by us) | probe |
| Hex colours with alpha (`#rgba`, `#rrggbbaa`) | invalid (the declaration is dropped) | dropped | `tests/fidelity/cases/typo-colors.html` |
| other `-webkit-*`, `-moz-*`, `-ms-*` | — | dropped | |
| `position: sticky`, `object-fit`, `aspect-ratio`, logical properties, `inset`, … | did not exist | dropped | WebKit 534 sources |

## Layout and text metrics

| Behaviour | QtWebKit 2.2 | Where |
|---|---|---|
| Font size | the computed size rounded to whole pixels (`computedPixelSize`): 11pt = 14.67px is set at 15px; `em` lengths keep the unrounded size | blitz-dom `stylo_to_parley.rs` |
| Glyph advances | linear (unhinted) advances in 26.6 fixed point, truncated | parley_engine `shaped_text.rs` |
| Kerning, ligatures | none in measurement (the simple text path adds advances) | blitz-dom `stylo_to_parley.rs` |
| Word widths | the line breaker measures "spaces + word" segments, each a whole number of pixels (`QFontMetrics::width`), within one text node; a word starting a line is measured alone | parley_engine `shaped_text.rs` |
| Max-content (shrink-to-fit) width | each word and each space rounded on its own (`RenderText::computePreferredLogicalWidths`): a float can wrap its own widest line | parley `layout/data.rs` |
| Painted glyph positions | the unrounded advances from the text box's laid-out start (the segment rounding only moves the following boxes) | parley_engine `ShapedCluster::paint_delta`, `atom.rs` |
| Alignment and justification | free space from the line's text box widths: a whole text node is its preferred width, part of one the rounded sum of its unrounded advances | parley `alignment.rs` |
| `letter-spacing`, `word-spacing` | integers, truncated (`computeLength<int>`): 0.5px is 0, 0.2em on 14px is 2px | blitz-dom `stylo_to_parley.rs` |
| `@font-face` | shadows an installed family of the same name for the CSS names asking for it (not for generic families); a font that fails to load leaves its text laid out with the fallback font but unpainted | blitz-dom `document.rs`, `stylo_to_parley.rs`, blitz-paint `text.rs` |
| `@font-face` bold face | painted bold, but its line metrics are the family's regular face's (DejaVu Serif Bold at 16px makes 19px lines, not 20px) | parley `style_metrics.rs` (the regular face of the same `@font-face` family) |
| Unknown family with no generic fallback (`Arial`) | the default face, with the run's own weight and style | parley `shape/mod.rs` (last-resort font per attributes) |
| `line-height: <number>` | `trunc(number × rounded font size)` | blitz-dom `stylo_to_parley.rs` |
| `line-height: normal` | ascent and descent each rounded up | parley (P2a) |
| `line-height: <length> \| <percentage>` | whole pixels, truncated (`1.3em` on 13px is 16px, `150%` of 13px is 19px) | blitz-dom `stylo_to_parley.rs` |
| Half-leading | `(lineHeight - fontHeight) / 2` in integers, truncated towards zero | parley `style_metrics.rs` |
| `vertical-align: sub \| super` | `fontSize / 5 + 1` down, `fontSize / 3 + 1` up (parent's whole-pixel size) | parley `style_metrics.rs` |
| Box positions on a line | after each text box's width (whole node: preferred width; part of one: rounded unrounded advances), not after the line breaker's segments | parley `line.rs`, `alignment.rs` |
| Kerning, ligatures with `letter-spacing`/`word-spacing` | Qt's complex path: kerning (rounded to whole pixels) and `fi` ligatures | blitz-dom `stylo_to_parley.rs`, parley_engine `shaped_text.rs` |
| Soft hyphen (`&shy;`) | a break opportunity; a line broken there ends with a hyphen (U+2010) | blitz-paint `text.rs` (drawn after the line's last run; the line breaker does not count its width) |
| Tab character | drawn as a space, also in `pre` | blitz-dom `layout/construct.rs` |
| `display: inline-table` on a line | sits on its bottom margin edge (no table baseline), the line's text on that edge | blitz-dom `layout/inline.rs` |
| Smart shrinking width | includes text overflowing its box (`nowrap`), at its text boxes' widths (a whole text node is its preferred width), unless the box clips | wk-webkit `page.rs` |
| Counters and quotes in `content` | resolved in document order; default quotes `"` then `'` | blitz-dom `generated_content.rs` |
| `attr()` | only in `content` (the attribute's value, empty if missing); a declaration using it elsewhere is dropped | stylo `specified/counters.rs`, blitz-dom `generated_content.rs`, wk-webkit `profile.rs` |
| `::first-letter` | the first letter (after white space and punctuation) of a block's first text, in a box with the pseudo-element's style (a float when floated); `::first-line` is not supported | blitz-dom `layout/construct.rs` (the box takes the place of `::before`, unless the block has one) |
| List markers | WebKit 2012 numbering systems; inside disc/circle/square (left to right) a shape in a box as wide as the font's ascent at the start of the first line; outside disc/circle/square painted as shapes (`(a*2/3+1)/2` px, `a*2/3+7` px before the content); text markers end `a*2/3/2` px before it | blitz-dom `layout/list.rs`, blitz-paint `render.rs` |
| Lengths from `em`, `pt`, … | whole pixels, truncated (`roundForImpreciseConversion`) | stylo_taffy `convert.rs` |
| Auto table layout | AutoTableLayout: columns get their min width, then the rest in proportion to their max widths; fixed and percent columns first; spanning cells widen the columns under them in proportion to their max widths (`calcEffectiveLogicalWidth`); `<col>` widths count | blitz-dom `layout/table_auto.rs` |
| Fixed table layout | FixedTableLayout: `<col>`/first-row widths as specified (no padding or border added), the rest shared equally, the last column taking the remainder | same |
| Row heights | a cell spanning rows only grows the last row it spans (`calcRowLogicalHeight`) | same |
| Table attributes | `border` (outset table, inset cells, grey), `frame`, `rules` (collapsing), `cellpadding`, `cellspacing`, `bordercolor`, `align`, `valign` mapped as `HTMLTableElement` does | blitz-dom `table_hints.rs` |
| Captions | the first caption only, at the table's content width, above or below it, outside its borders | blitz-dom `layout/table.rs`, `layout/mod.rs` |
| Collapsed borders | resolved per cell edge (width, then style; cell, row, table); each cell takes half, the extra pixel on the top/left; the table's start/end borders come from the first row only | blitz-dom `layout/table.rs` |
| Floats in a BFC root | contained above the box's bottom padding and border | taffy `compute/block.rs` |

## Paint

| Behaviour | QtWebKit 2.2 | Ours |
|---|---|---|
| Blurred box shadows | blurred (an image whose mask is about a Gaussian of σ = 0.4 × blur across the edge) | approximated by nested rings taking that Gaussian's coverage (a PDF has no blur) |
| Dashed and dotted borders (square corners) | a `w`×`w` square at each end of a side, then Qt dashes from the line start: `3w` on, `3w` off (dotted `w`, `w`), flat ends | same, the dashes filled as rectangles |
| Inset, outset, groove, ridge | one side darkened with `Color::dark` (black → #545454), the other unchanged; groove/ridge split at `(a + b + 1) / 2` px | same |
| Double borders | lines of `(width + 1) / 3` px | same |
| Repeated backgrounds (images, gradients) | a Qt pattern brush whose tiles start at the first page's top left corner (Qt's PDF engine drops the brush origin), unless one tile covers the area | PDF tiling pattern, tiles anchored the same way (later pages are not distinguished) |
| Inline boxes | horizontal margin/border/padding at the first and last fragment; background and border per line fragment, font ascent to descent plus vertical padding/border; outline per fragment | `<wkedge>` inline-blocks hold the horizontal edges; fragments painted by blitz-paint |
| `unicode-bidi` on inline elements | embeddings and overrides (2011 WebKit: no isolation) | LRE/RLE/LRO/RLO … PDF characters in the paragraph text, not drawn |
| `list-style-image` | the image replaces the marker once loaded: outside, on the first line's baseline, 7px before the content box | blitz-dom loads it with the background images (`list_marker_images`); blitz-paint `render.rs` (inside and right-to-left items keep the bullet) |
| List bullets | disc, circle and square drawn as shapes `(ascent·2/3 + 1)/2` px; right of the content in a right-to-left item, string markers written suffix first (` .1`) | same |
| Form controls | Qt Windows style: sunken checkbox and radio indicators, 2px raised push buttons, 0.7px field frames, list boxes with greyed scroll bars, slider, progress chunks, meter gradient, file button; color/date inputs are text fields | drawn in blitz-paint / rebuilt as spans in wk-webkit (`page.rs`) |
| `--zoom`, `--dpi` | frame zoom `zoom × dpi / 96`: lengths, font sizes and borders rounded in device pixels, a border at least 1 device px | same rounding (stylo_taffy `device_zoom`); glyph advances are not rounded at that scale |
| `--no-images` | an unsized image is an empty replaced box that keeps its line | 1px minimum height |
| Inset box shadows | painted inside the padding box | padding box minus the offset hole, no compositing |
| `<hr>` | inset 1px rule in #9a9a9a over #eee whatever its `color`; `noshade` solid #808080 (border and background), `color` solid in that color, `size` n > 1 a height of n − 2 px, `size=1` no bottom border, `align` the side margins | `default.css`, blitz-dom `table_hints.rs` |
| Dotted border with `border-radius` | drawn solid | drawn solid (dashed stays dashed) |
| Outlines | rectangles (no radius), `outline-offset` away from the border box | same |
| Collapsed table borders | painted after the cell backgrounds | same |
| Collapsed dashed/double borders | dashes 4 widths on, 2 off; dotted solid (Qt square caps); double = outer thirds | same |
| `empty-cells: hide` | no background or border on an empty cell (separated borders) | same |
| Inline box backgrounds | ascent and descent rounded up | same |
| Text decorations | 1px lines from the text box top: overline at the top, line-through `2a/3` below, underline 1px below the baseline (`a` = whole-pixel ascent) | same (blitz-paint `text.rs`) |
