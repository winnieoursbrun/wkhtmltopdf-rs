# P3 corpus: wkhtmltopdf-specific document parts

Reference cases for headers/footers, TOC, outline, `--dump-outline`, links, forms, page
objects, copies and grayscale (`docs/upstream-spec.md` §1.1, §3.4–§3.6). Every reference output
was produced by the real `wkhtmltopdf 0.12.6 (with patched qt)` in `wkref:local`.

## Layout

```
tests/parts/
  make-refs.sh        regenerates every ref.* (docker: wkref:local + python:3.11-slim)
  pdfstruct.py        PDF -> ref.json structure facts (pypdf 5.1.0)
  <case>/
    cmd               argument list, one argv entry per line (no shell quoting)
    *.html, *.xsl     inputs, self-contained (no network)
    ref.pdf           output of the reference binary
    ref.rc            exit code
    ref.stdout        stdout (empty for every case: output goes to out.pdf)
    ref.stderr        raw stderr (progress bars with \r, see "Normalisation")
    ref.txt           `pdftotext -layout ref.pdf`: one \f after each page
    ref.info          `pdfinfo ref.pdf`
    ref.outline.xml   the --dump-outline file (only cases whose cmd has --dump-outline)
    ref.json          structure facts (below)
```

### `cmd` conventions

- The working directory is the case directory, and every path in `cmd` is relative to it.
- The output file is always `out.pdf`, the last line of `cmd`.
- `--dump-outline` always writes `out.outline.xml`, which is stored as `ref.outline.xml`.
- `make-refs.sh` copies the case to `/work/<case>/` and runs it from there. File URLs therefore
  start with `file:///work/<case>/`. In `ref.json` that prefix is replaced by `{BASE}`.

## `ref.json`

```jsonc
{
 "pdf_version": "1.4",
 "page_count": 5,
 "page_mode": "/UseOutlines",            // null when the catalog has no /PageMode (--no-outline)
 "info": {"/Title": "...", "/Creator": "wkhtmltopdf 0.12.6", "/Producer": "Qt 4.8.7"},
 "outline": [                            // PDF bookmarks, flattened depth-first
   {"level": 1, "title": "Chapter One", "page": 2, "y": 813.5}
 ],
 "named_dests": ["/__WKANCHOR_2", "/{BASE}links.html#named"],   // sorted
 "acroform_fields": null | [             // null without /AcroForm
   {"name": "fullname", "type": "/Tx", "value": "Jane Doe", "flags": 0, "page": 1,
    "rect": [x0, y0, x1, y1], "maxlen": 20,
    "readonly"|"multiline"|"password": true,   // only when the /Ff bit is set
    "appearance_state": "/Yes"}                // checkboxes: /AS
 ],
 "pages": [{
   "n": 1, "size": [595.0, 842.0], "rotate": 0,
   "links": [                            // /Link annotations in /Annots order
     {"rect": [...], "uri": "https://..."},                              // external
     {"rect": [...], "dest_name": "/__WKANCHOR_o", "dest_page": 3, "dest_y": 813.5}  // internal
   ],
   "fonts": ["DejaVuSans", "DejaVuSans-Bold"],   // /BaseFont, subset tag stripped
   "images": [{"w": 10, "h": 10, "cs": "/DeviceRGB"}],
   "colors": {"ops": {"cs": 10, "scn": 9}, "non_gray_rgb": 0}
 }]
}
```

Coordinates are PDF points rounded to 0.1, and sizes to 0.01. `page` and `dest_page` are
1-based physical pages.

## What a Rust test should compare

For each case: run our binary with the arguments in `cmd`, from a temporary copy of the case
directory. Then compare the results with these strictness levels:

| Item | Comparison |
|---|---|
| exit code | equal to `ref.rc` |
| stdout | equal to `ref.stdout` (empty) |
| stderr | equal after normalisation (below) |
| `out.outline.xml` | byte-equal to `ref.outline.xml` (the anchors are deterministic). Normalise `[date]`-like values only if a title contains them |
| `page_count`, page `size`, `rotate`, `page_mode`, `info` (L2) | equal |
| `outline` titles, levels and pages; `named_dests`; link `uri`, `dest_name` and `dest_page`; field `name`, `type`, `value`, `flags`, `maxlen` and `appearance_state`; `non_gray_rgb == 0` for `grayscale` and for page 1 of `multi-object` | equal |
| `rect`, `y`, `dest_y` | within a tolerance (layout engine differs, L3): a few points |
| `ref.txt` | equal per page after normalisation and whitespace collapsing (split on `\f`, collapse runs of spaces, trim lines, drop empty lines). Column positions from `-layout` are only L3 |
| `fonts`, `colors.ops`, `images` | informative only |

Normalisation (the values change with each run):

- `[date]` → `\d{1,2} [A-Z][a-z]{2} \d{4}` (C locale, for example `14 Oct 2026`), `[time]` →
  `\d{2}:\d{2}:\d{2}`, `[isodate]` → `\d{4}-\d{2}-\d{2}`. They appear in `hf-text-vars`
  (footer-left) and `hf-html-js` (footer). Replace them with `<DATE>`, `<TIME>` and `<ISODATE>`
  on both sides, or with the values of the day the test runs.
- stderr: split on `\r` and `\n`, then drop the progress bar lines (`[====>   ] 42%`) and the
  trailing padding spaces. What remains is the phase names and the messages.
- `ref.info`: `CreationDate` and `File size` differ; compare only `Title`, `Creator`, `Producer`,
  `Pages`, `Page size`, `PDF version`, `Tagged`, `Encrypted`.

## Cases

| Case | Exercises | Notable reference facts |
|---|---|---|
| `hf-text-vars` | every `[var]` in all 6 text slots, `--replace`, `[PAGE]` case-insensitive, unknown `[foo]`, 2 objects | `[frompage]`=1 for both objects; `[subsection]` (and `[subsubsection]`) carries over chapter and **object** boundaries (page 5, in `second.html`, still shows `Section 2.2`) |
| `hf-text-style` | `--header-line/--footer-line`, spacings, `--header/--footer-font-name/-size`, explicit margins | header in DejaVuSerif, footer in DejaVuSansMono (`fonts`) |
| `hf-default-header` | `--default-header` | `[webpage] … [page]/[topage]` + line, top margin 2cm |
| `hf-html-js` | `--header-html`/`--footer-html` with the canonical `subst()` script from the upstream docs | per-page values filled in by JS |
| `hf-html-static` | HTML header without JS (measured 20mm height, no `--margin-top`) + footer with `--margin-bottom` | `[page]` in HTML stays literal; the header's `<a>` becomes a URI link on every page |
| `toc-default` | `toc` + h1–h4 over 4 pages | the TOC lists its own caption as its first entry (page 1); 11 GoTo links on page 1 |
| `toc-options` | `--toc-header-text`, `--toc-level-indentation`, `--disable-toc-links`, `--toc-text-size-shrink`, `--disable-dotted-lines` | no links on the TOC page |
| `toc-header-unescaped` | `--toc-header-text "Contents & Index"` | the caption is spliced **unescaped** into the default XSL: stderr gets `Error XTSE0010 in file:///wk/wkhtmltopdf-linux-amd64, at line 12, column 26: …` (the XSL base URI is the executable path), twice; the TOC page is blank but still counts; rc 0 |
| `toc-xsl` | `--xsl-style-sheet` (XSLT 2.0 header, simple templates) | |
| `cover-toc-pages` | `cover` (2 physical pages) + `toc` + 2 docs, footer `[page]/[topage] [title] [section]` | the cover counts as 1 page for `[topage]` and TOC numbers but 2 physical pages, so the last footer reads `6/5`, the TOC numbers are one less than the footers, and `[section]` is shifted one page early (the TOC page shows `Alpha chapter`). The cover's outline root is `<item title="" page="0" link="" backLink=""/>`; the PDF bookmarks use physical pages |
| `outline-depth` | `--outline-depth 2` | bookmarks are h1/h2 only; dump still full |
| `no-outline` | `--no-outline` + `--dump-outline` | no bookmarks and no `/PageMode`; the XML dump is still written |
| `links` | `#name`, `#id`, missing fragment, other loaded object with/without fragment, https, mailto, relative not-loaded file | the missing fragment produces **no** annotation; a not-loaded relative file becomes `file:///…` URI; dest names are full file URLs |
| `links-disabled` | `--disable-internal-links --disable-external-links` | no annotations, only the outline dests |
| `page-offset` | `--page-offset 5`, footer `[page] of [topage] (from [frompage])`, header `[sitepage]/[sitepages]` | footers 6..9 of 9, from 6; outline XML pages are offset (root `page="5"`); PDF bookmarks are not |
| `forms` | `--enable-forms`: text, empty, password, readonly, untyped input, textarea, checkboxes, radio, select, submit | 7 fields. An `<input>` without `type` is **not** converted. The textarea has no `/V` (its content is dropped). A checked checkbox has `/AS /Yes` and no `/V`. Radio, select and submit are not converted. Checkboxes reference the `ZaDb` font without a `/DR`, so poppler prints `Unknown font tag 'ZaDb'` |
| `multi-object` | global `--header-center`; `a.html --no-background`; `b.html` with its own header/footer; `c.html` | option scoping per object; page 1 has no pink fill (`non_gray_rgb` 0 vs 1) |
| `dump-outline-escaping` | `& < > " '` in title and headings, whitespace-only heading, `<h7>`, multi-line heading, doc starting at h3, duplicate headings | the empty heading is skipped; `h7` is a level-4 child; whitespace collapsed |
| `a5-landscape` | `--page-size A5 --orientation Landscape` | 595 × 420 pt |
| `copies-collate` | `--copies 2 --collate` | page order 1,2,1,2; footer `[page]` restarts per copy |
| `copies-nocollate` | `--copies 2 --no-collate` | page order 1,1,2,2 |
| `grayscale` | `--grayscale` | colours are written as grey RGB triples (`/CSp cs r r r scn`), not DeviceGray |

## Regenerating

```sh
WK_BIN_DIR=.ref tests/parts/make-refs.sh            # all cases
WK_BIN_DIR=... tests/parts/make-refs.sh links forms                                  # some cases
```

The reference binary runs under amd64 emulation with `--network none`, `TZ=UTC` and the C
locale. The second container needs network access once, for `pip install pypdf==5.1.0`.
Regenerating changes the dates, times and stderr progress, so review the diff before you
commit it.
