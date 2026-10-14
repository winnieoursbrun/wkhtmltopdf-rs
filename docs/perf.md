# Performance pass (October 2026)

Goal: beat wkhtmltopdf 0.12.6 on wall time, CPU and peak RSS for large documents
(`bench/corpus/large.html`, a 30-page invoice with 1,450 table rows), without changing the
output. Result: **wall and CPU beaten (1.6x and 2x less than the original); peak RSS cut by
29 % but still 45 % above the original** (see "Peak RSS: what is left"). The memory pass that
followed (next section) brings the peak RSS **below the original's: 48.9 MiB vs 56.6 MiB**.

# Memory pass (October 2026)

Goal: peak RSS on `large.html` at or below the original's, output unchanged. Every step was
checked with `bench/pdf-identity.sh`: the 67 PDFs of the corpus are byte-identical to `main`
(`5a19b78`) after each commit; `cargo test --workspace`, clippy `-D warnings` and
`cargo fmt --check` pass.

## Results

`wkbench:local` container (linux/arm64, static musl binaries), pdfkit's arguments plus
`--javascript-delay 0`. Wall: hyperfine median of 20 runs after 3 warmups; CPU (user+sys,
median) and peak RSS (lowest) of 9 `/usr/bin/time -v` runs. *orig* is wkhtmltopdf 0.12.6.1,
*main* is `5a19b78`, *final* is this branch. All three measured in the same session.

| Case | Wall orig / main / final | CPU s orig / main / final | Peak RSS MiB orig / main / final |
|---|---|---|---|
| `bench/corpus/large.html` | 536 / 313 / **261 ms** | 0.42 / 0.30 / **0.25** | 56.6 / 81.8 / **48.9** |
| `bench/corpus/medium.html` | 49.0 / 15.5 / 15.3 ms | 0.04 / 0.01 / 0.01 | 40.0 / 18.9 / **15.6** |
| `bench/corpus/small.html` | 25.0 / 6.0 / 5.9 ms | 0.02 / 0.00 / 0.00 | 38.3 / 16.5 / **14.1** |

(The original's wall time on `large` drifts between sessions: 446 ms in the previous pass.)

macOS arm64 host (release profile, `WKHTMLTOPDF_SYSTEM_FONTS=0`, `/usr/bin/time -l`, lowest
of 9): `large` 82.0 → 59.0 MiB, `medium` 19.5 → 15.6 MiB, `small` 15.2 → 12.8 MiB, wall and
CPU unchanged within noise (230 → 220 ms). macOS' allocator keeps more of the freed memory
resident than musl's, hence the higher figure for the same heap.

## What changed

| Commit | Change | Heap peak on `large.html` (dhat) |
|---|---|---|
| | `main` | 72.6 MB |
| `7f319a7` | `parley`: `Layout::shrink_to_fit` (and `ShapedText::shrink_to_fit` in `parley_engine`), called by blitz-dom once a layout is built and after each line-breaking pass. A one-line cell held its runs, lines, styles and clusters in 4-element minimum allocations plus the speculative per-character `reserve` | 55.6 MB |
| `dbef450` | `blitz-dom` tables: cells index a per-table list of distinct grid item styles (compared with the last style of the same column and the last one added) instead of holding a ~1.2 KB `taffy::Style` each; the row placement, different on every row, is applied by a `CellStyle` wrapper (`get_grid_child_style`) | 46.5 MB |
| `e4ed047` | `WebPage::render_final` (consumes the page; every page is rendered once and dropped): the inline layouts are moved out of the DOM before painting and handed to `blitz_paint::inline_layouts`, which drops each one right after painting its text, so the display list reuses their memory | 35.1 MB |
| `ec64bac` | PDF writer: krilla shares the font blob instead of a copy of each face's file (2.2 MB for the four DejaVu faces); the culling boxes are built in an exactly sized `Vec` shared by `Rc` instead of a doubling `Vec` copied into an `Rc<[_]>` | 35.1 MB; PDF phase 11.4 → 6.6 MB |

The peak is still at the end of painting: DOM nodes (slotmap 7.1 MB), taffy layout data
(5.6 MB), the display list (chunks 5.1 MB, boxed glyph runs 3.9 MB, glyph vectors 1.4 MB, run
texts 2.3 MB, boxed path fills 2.9 MB) and elements (2.5 MB). The heap-to-RSS gap is the
allocator: freed DOM memory is reused by the display list, but the PDF phase's large buffers
(content streams, deflate state, `lopdf` re-parse for the AcroForm/PageMode update) come on top.

## Options evaluated and not taken

- **Streaming pages to krilla / dropping the display list per page**: the PDF phase is no
  longer the peak (its live allocations are 6.6 MB above the display list), and every page is
  culled from the single display list of the document (a command may span several pages, and
  repeated table header and footer groups are replayed on every page).
- **Dropping the taffy layout caches before painting** (~3 MB): would need the cache split out
  of `LayoutData`; not needed to reach the goal.
- **Compacting glyph-run commands and run texts** (`u32` glyph ranges, a solid-fill glyph run
  without the 464-byte `RenderCommand`): ~3 MB, but every consumer of `Command::get` would pay a
  conversion; left for a later pass.
- **Releasing stylo `ComputedValues` of painted nodes**: they do not show among the allocation
  sites alive at the peak (cells share their computed values through stylo's sharing cache).

# Speed pass (October 2026)

## Results

`wkbench:local` container (linux/arm64, 4 vCPU, static musl binaries, the bench image's
original `wkhtmltopdf 0.12.6.1 (with patched qt)`), 20 hyperfine runs after 3 warmups, median
wall time; CPU (user+sys) and peak RSS from `/usr/bin/time -v` (median CPU, lowest RSS of 5
runs). *main* is `714ac1d`, *final* is this branch merged with it. Same arguments as the bench:
pdfkit's (`--quiet --page-size Letter --margin-* 0.75in --encoding UTF-8`) plus
`--javascript-delay 0` for the three bench pages; `tests/js/cases.tsv` arguments for the jQuery
page; `tests/parts/toc-default/cmd` for the TOC case.

| Case | Wall orig / main / final | CPU s orig / main / final | Peak RSS MiB orig / main / final |
|---|---|---|---|
| `bench/corpus/large.html` | 446 / 433 / **283 ms** | 0.51 / 0.43 / **0.26** | 56.4 / 115.7 / **81.7** |
| `bench/corpus/medium.html` | 49.2 / 15.5 / 15.6 ms | 0.04 / 0.01 / 0.01 | 39.2 / 20.0 / 19.0 |
| `bench/corpus/small.html` | 24.5 / 6.7 / 7.3 ms | 0.02 / 0.00 / 0.00 | 37.5 / 16.8 / 16.7 |
| `tests/js/jquery1.html` | 258.7 / 68.7 / 68.3 ms | 0.04 / 0.06 / 0.07 | 44.3 / 27.9 / 27.9 |
| `tests/parts/toc-default` | 691 / 10.2 / 10.0 ms | 0.07 / 0.00 / 0.00 | 39.3 / 18.1 / 17.3 |

The original's wall time on `jquery1.html` and `toc-default` is mostly its default 200 ms
`--javascript-delay`, applied to every loaded page. The container shares the host with other
work: means drift by ±20 % between sessions, medians and CPU are stable.

macOS arm64 host (release profile, `WKHTMLTOPDF_SYSTEM_FONTS=0`), `large.html`: median wall
360 → 293 ms, CPU 357 → 269 ms, peak RSS 111.7 → 82.4 MiB. The other cases are unchanged
within noise.

## What changed

Every change was checked with `bench/pdf-identity.sh` (below): the 67 PDFs of the corpus are
byte-identical before and after each step, and between `main` and the final branch.

| Commit | Change | Effect on `large.html` |
|---|---|---|
| `a70ebad` | Named destinations registered in name order (`BTreeMap`) instead of `HashMap` order | Output was not deterministic: object numbers of named destinations changed from run to run. Needed to compare outputs byte for byte; no measurable cost |
| `4ffb75f` | `wk-print`: culling boxes of the display list computed once per scene, not once per page and band | −23 % of the time (`BezPath::bounding_box` was the top function) |
| `eaf2bb8` | `wk-converter`: each page's DOM (styles, layout, parley layouts) dropped once rendered; only its URL key is kept | Frees the DOM before the PDF is written; does not lower the peak, which is during painting |
| `817a4cc` | `wk-webkit` recorder: solid `Rect` fills stored as a 104-byte `RectFill` (an anyrender `RenderCommand` is 464 bytes plus a heap path) and expanded on demand into the exact command `Scene::fill` records; other commands boxed | Heap peak −28 MB, RSS 106 → 82 MiB (macOS) |
| `3a7d800` | Display list in 4096-command chunks; `PopLayer` unboxed | Heap peak 77.9 → 72.5 MB |
| `c17332f` | `wk-print` paginator: atomic intervals sorted once, `is_safe` answered by binary search over the running max of their ends (same comparisons) | −3 % |
| `7407caa` | `--features dhat-heap`: dhat heap profiler as global allocator | tooling |
| `561c0d0` | `--features dlmalloc`: pure-Rust dlmalloc as global allocator, **off by default** | see below |

## Profiles

CPU, `samply record` on the macOS release build, `large.html`:

- **Before**: `write_pdf` 56 % (of which `Replayer::replay` 40 %, `BezPath::bounding_box` 23 %
  self, deflate 13 %), `WebPage::load` 26 % (parse 6 %, style 3 %, layout 12 %),
  `WebPage::render` 11 %. A single thread: stylo's traversal is sequential
  (`StyleThreading::Sequential`) and `parallel-construct` is off, so the CPU > wall figure of
  the first bench run was measurement noise, not rayon.
- **After**: deflate 20 % (krilla's `miniz_oxide` at its fixed level), load 38 % (parse 9 %,
  first resolve 24 %: style, box construction and a full layout at the 800 px load viewport),
  render 13 % (relayout at the print width + paint), PDF content serialisation the rest.

Heap, `dhat` at the global peak (end of painting, DOM and display list both alive):

| Before (106.5 MB) | After (72.5 MB) |
|---|---|
| display list `Vec<RenderCommand>` + paths: 45 MB | display list (chunks 5.1, boxed glyph runs 7.7, other boxed commands 2.9, run texts 1.1): 17 MB |
| table cells (`TableCell` with a full `taffy::Style`): 10.4 MB | 10.4 MB |
| parley shaping + line breaking (8,700 cell layouts): 24.6 MB | 24.6 MB |
| DOM nodes, layout data, anonymous wrappers, elements: 19 MB | 19 MB |

## Peak RSS: what is left

*Superseded by the memory pass above.* The original holds `large.html` in ~18 MB above its 39 MB baseline (Qt + WebKit libraries);
the port holds it in ~65 MB above its 17 MB baseline. What remains is the engine's own data,
about 7 KB per table cell, all of it in the vendored engine crates:

- `TableCell` carries a whole `taffy::Style` (~1.2 KB) per cell (`vendor/blitz-dom/src/layout/table.rs`);
- every cell's text gets its own parley `Layout` (~2.8 KB: shaped runs, clusters, lines);
- taffy layout data and an anonymous block wrapper per cell (~1 KB).

Reaching the original's figure needs changes there (deriving cell styles on demand, a
lighter inline layout for single-run cells), which this pass left out to keep the vendored
crates easy to merge with the fidelity work.

## Options evaluated and not taken

- **Lower compression level / zlib-rs backend**: deflate is 20 % of the time, but any change
  of level or backend changes the PDF bytes.
- **Allocators**: `dlmalloc` (pure Rust, behind `--features dlmalloc`) on the static musl
  build: `large` 257 → 231 ms, `medium` 15.0 → 12.1 ms, same RSS on `large`, but the jQuery
  page grows from 27.9 to 32.9 MiB (dlmalloc keeps freed memory). mimalloc and jemalloc
  were not tried: they are C, and the gain dlmalloc shows is the upper bound of what the
  allocator brings here.
- **Skipping the layout of the load-time resolve** (~9 % of the time): `WebPage::load` calls
  `BaseDocument::resolve` to discover resources, which lays the document out at the 800 px
  viewport before `render` lays it out again at the print width. Style and box construction
  alone would do, but that needs a new entry point in `vendor/blitz-dom`.
- **Profile settings**: the release profile already has `lto = true`, `codegen-units = 1` and
  no debug assertions; `panic = "abort"` was not tried (unwinding costs nothing until a panic,
  and it would change the behaviour of a panicking conversion).
- **Font loading**: the six embedded DejaVu faces are registered without parsing their
  outlines (`fontique` scans the name table only, 1 % of the time); nothing to gain.

## Tools

```sh
# Byte-identity of the whole corpus (bench/corpus, tests/corpus, tests/js, tests/parts,
# upstream conversion fixtures) between two builds:
bench/pdf-identity.sh run  target-before/release/wkhtmltopdf /tmp/before
bench/pdf-identity.sh run  target/release/wkhtmltopdf        /tmp/after
bench/pdf-identity.sh diff /tmp/before /tmp/after

# CPU profile (macOS):
samply record --save-only --unstable-presymbolicate -o prof.json.gz -- \
  target/release/wkhtmltopdf --quiet --javascript-delay 0 bench/corpus/large.html out.pdf

# Heap profile (writes dhat-heap.json, open with dhat's dh_view.html):
cargo build --release -p wkhtmltopdf --features dhat-heap
```

`pdf-identity.sh` fixes `SOURCE_DATE_EPOCH` and `WKHTMLTOPDF_SYSTEM_FONTS=0`, and masks the
two parts of a PDF that legitimately change between runs: the random name of the temporary
file HTML read from stdin is spooled to (`wktemp-<uuid>.html`, which ends up in link targets)
and the trailer `/ID` digest that follows from it.
