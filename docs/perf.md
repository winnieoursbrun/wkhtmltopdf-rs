# Performance pass (October 2026)

Goal: beat wkhtmltopdf 0.12.6 on wall time, CPU and peak RSS for large documents
(`bench/corpus/large.html`, a 30-page invoice with 1,450 table rows), without changing the
output. Result: **wall and CPU beaten (1.6x and 2x less than the original); peak RSS cut by
29 % but still 45 % above the original** (see "Peak RSS: what is left"). The memory pass that
followed (next section) brings the peak RSS **below the original's: 48.9 MiB vs 56.6 MiB**.

# Pass 5 (October 2026)

Goal: lower the wall time of a conversion, `large.html` first (148 ms in the bench container
after pass 4, target under 100 ms), using several cores if needed. Conditions: PDFs
byte-identical to pass 4 and identical from run to run, peak RSS at most 10 % above pass 4
on every bench scenario, a bounded number of threads, no slowdown on a single core, pure
Rust.

Method: phase timings (`WKHTMLTOPDF_DEBUG_TIMING=1`, new: the elapsed time at the main
pipeline steps on stderr), `perf record` inside the bench VM (`wkbench:local` plus Debian's
`linux-perf`, container run with `--privileged`; the conversion thread is `main`, the pool
threads `wk-worker-N`, so each can be profiled on its own) and samply on macOS; A/B timings
interleaved run by run in one container session.

## Results

`wkbench:local` container (linux/arm64, 4 vCPU), static musl binaries, bench arguments
(`small` and `large` with `--javascript-delay 0`). All binaries measured in one session,
interleaved run by run (the host is shared: absolute times drift by ±10 % between sessions,
so *base* here, pass 4 at `ddaea2a`, is 159 ms on `large` against 148 ms in the pass 4
session). Wall and CPU (user + sys of the process): median of 15 runs after 3 warm-ups; peak
RSS: median of 7 runs.

| Scenario | Wall ms base → pass 5 | CPU ms base → pass 5 | Peak RSS MiB base → pass 5 |
|---|---|---|---|
| small page | 4.4 → 3.7 (−16 %) | 5.3 → 5.0 | 14.1 → 15.2 (+7.7 %) |
| medium, 4 pages | 10.7 → 8.9 (−17 %) | 13.3 → 12.6 | 15.4 → 16.4 (+6.2 %) |
| large invoice, 30 pages | 158.9 → **105.5** (**−34 %**) | 194.5 → 212.2 (+9 %) | 50.1 → 46.9 (−6.5 %) |
| header/footer HTML + TOC, 8 pages | 60.8 → 48.6 (−20 %) | 64.6 → 54.9 | 25.1 → 26.5 (+5.8 %) |
| jQuery page | 119.4 → 96.0 (−20 %) | 121.4 → 101.3 | 28.8 → 30.9 (+7.3 %) |
| multi-object (cover, TOC, 3 parts) | 15.5 → 12.0 (−23 %) | 18.6 → 16.6 | 15.9 → 16.9 (+6.1 %) |

Inside the process (`WKHTMLTOPDF_DEBUG_TIMING`, median of 20 runs, quiet host), `large.html`
takes 91 ms from the start of the conversion to the written PDF: parse 8, style, boxes and
layout 39, paint 13, PDF 25 (the rest is the process start and exit around it). The 100 ms
target is met on a quiet host and missed by 5 % in the session of the table above.

On a single CPU (`docker --cpus=1 --cpuset-cpus=0`, where the pool has one thread and every
parallel path is off), pass 5 is faster too, thanks to the allocator: small 6.4 → 5.4 ms,
`large` 183 → 162 ms, header/footer + TOC 69.5 → 58.7 ms, jQuery 121.7 → 103.7 ms.

## Threads

The binary builds one rayon pool of `min(available_parallelism, 4)` threads
(`RAYON_NUM_THREADS` overrides it), named `wk-worker-N`; the conversion runs on its own
thread as before. Everything parallel runs on that pool, with results collected in document
order: Stylo's traversal (on the pool of the calling thread instead of Stylo's own pool,
which would add up to 6 more threads), inline layout construction, page drafts, deflate,
background drops and the start-up work. A single core gets a pool of one thread and the
sequential paths. The output is the same with 1, 4 or 8 threads (checked).

## What changed

Per commit, same session as the results above (median wall, median CPU, median peak RSS).

| Commit | Change |
|---|---|
| `c32cb77` | dlmalloc 0.2.14 vendored, unchanged |
| `5e388a5` | **Allocator of the musl build** (`crates/wkhtmltopdf/src/alloc.rs`, pure Rust): per-thread free lists for requests up to 1016 bytes (one per dlmalloc chunk size, so nothing is padded), refilled and drained in batches under the lock of a shared dlmalloc heap; medium requests from that heap; 128 KiB and more mmapped on their own (`mremap` to grow). The vendored dlmalloc gives the whole pages of freed chunks back to the system (`madvise`). musl's malloc was 15–20 % of the CPU and takes one process-wide lock. macOS, Windows and glibc keep their system allocator; `--features dlmalloc` and `system-alloc` select the other ones |
| `cebec04` | **Style and inline layouts on the pool** for large documents: Stylo's parallel traversal (2,000 nodes or more) on the rayon pool of the calling thread (`traverse_dom_on_current_pool`), the 256 or more inline layouts of a resolve built on the pool with per-thread layout and font contexts (blitz's `parallel-construct`, chosen at run time). Pool sized `min(CPUs, 4)` |
| `e794bbe` | **krilla page drafts** (`krilla::draft`): a page drawn on a surface of its own (private serializer, any thread); what depends on the document (graphics-state numbering, the text: CIDs, font objects, `/ActualText` spans) is kept aside and done by `Page::apply_draft` in page order. Drafts using images, opacity groups, masks, gradients, patterns, outlined or colour glyphs or tagging are refused and drawn in the document as before |
| `2bf2e7a` | **PDF pages drafted on the pool**, at most two per thread ahead of the page being written; faces and culling data shared |
| `2ec8b06` | Freed pages given back 16 KiB at a time (every page: `madvise` was 5 % of the conversion thread) |
| `695b968` | Drafts refused under PDF validators (none in wkhtmltopdf) |
| `0876fdd` | The conversion thread drafts the next pages while it waits for one |
| `587e01b` | The DOM of a rendered page and the display lists at the end dropped on the pool (3.7 + 4.3 ms off the conversion thread on `large`) |
| `505a1f0` | **Text of a draft written by the compressing job**: merging only registers the glyphs and texts in the font (CIDs, spans) and the font name; the operators of plain CID text are written by the job that compresses the page |
| `4f9087d` | Glyph-run texts in a vector sorted by command index (the SipHash map was rehashed as it grew: 4.8 % of the conversion thread) |
| `0be9568` | Culling boxes of the display lists computed on the pool before the drafts start |
| `95eeca4` | Font scan on a start-up worker, a page parsed with an empty font context and given the real one before styling |
| `1ef0989` | 4 KiB of cached blocks per class and thread (instead of 8: 0.2–0.3 MiB less RSS, same time) |
| `8e310f4` | Pages drafted only from four pages per thread (the 4- and 8-page bench documents gained 0.5–1 ms for 0.5 MiB of RSS) |
| `e4db04a` | Glyph-run walks skipped when they have nothing to do (inline boxes to place, inline backgrounds to paint): resolve 42.2 → 38.9 ms, paint 13.9 → 13.1 ms on `large` |

Wall time, ms:

| Commit | small | medium | large | hf+toc | js | multi |
|---|---|---|---|---|---|---|
| base `ddaea2a` | 4.4 | 10.7 | 158.9 | 60.8 | 119.4 | 15.5 |
| `5e388a5` allocator | 4.2 | 9.7 | 140.3 | 52.3 | 105.2 | 13.3 |
| `cebec04` style, inline layouts | 4.6 | 9.8 | 126.8 | 51.6 | 102.8 | 13.5 |
| `2bf2e7a` page drafts | 4.2 | 9.8 | 117.2 | 52.2 | 102.2 | 13.5 |
| `2ec8b06` purge 16 KiB | 4.2 | 9.6 | 115.4 | 48.1 | 95.3 | 13.0 |
| `0876fdd` conversion thread drafts | 4.2 | 9.4 | 116.5 | 48.9 | 99.8 | 12.9 |
| `587e01b` background drops | 4.2 | 9.4 | 112.2 | 48.5 | 96.5 | 12.8 |
| `505a1f0` text on the compressing job | 4.1 | 8.8 | 110.6 | 48.4 | 95.3 | 12.5 |
| `4f9087d` run texts vector | 4.2 | 8.8 | 111.2 | 47.7 | 95.8 | 12.1 |
| `0be9568` culling on the pool | 4.1 | 8.9 | 111.0 | 47.9 | 95.5 | 12.1 |
| `95eeca4` font scan on a worker | 4.0 | 8.5 | 110.9 | 48.2 | 97.2 | 11.7 |
| `1ef0989` 4 KiB caches | 3.9 | 8.6 | 112.2 | 47.5 | 96.2 | 11.9 |
| `8e310f4` drafts from 16 pages | 3.8 | 9.0 | 108.6 | 49.6 | 96.3 | 12.3 |
| `e4db04a` glyph-run walks | 3.7 | 8.9 | **105.5** | 48.6 | 96.0 | 12.0 |

CPU (user + sys), ms:

| Commit | small | medium | large | hf+toc | js | multi |
|---|---|---|---|---|---|---|
| base `ddaea2a` | 5.3 | 13.3 | 194.5 | 64.6 | 121.4 | 18.6 |
| `5e388a5` | 4.9 | 12.1 | 174.5 | 56.0 | 107.0 | 15.9 |
| `cebec04` | 5.7 | 13.0 | 201.4 | 56.3 | 107.4 | 17.0 |
| `2bf2e7a` | 5.3 | 13.5 | 211.9 | 57.6 | 107.1 | 17.6 |
| `2ec8b06` | 5.2 | 13.1 | 213.3 | 53.4 | 100.8 | 17.1 |
| `0876fdd` | 5.2 | 12.9 | 211.8 | 54.4 | 104.8 | 17.0 |
| `587e01b` | 5.3 | 13.0 | 213.6 | 54.9 | 101.9 | 17.4 |
| `505a1f0` | 5.2 | 12.8 | 214.4 | 55.3 | 101.0 | 17.6 |
| `4f9087d` | 5.3 | 12.9 | 217.5 | 55.6 | 102.1 | 17.0 |
| `0be9568` | 5.1 | 12.7 | 218.7 | 55.3 | 102.3 | 17.0 |
| `95eeca4` | 5.3 | 13.0 | 225.0 | 55.5 | 103.9 | 17.4 |
| `1ef0989` | 5.3 | 13.1 | 220.5 | 55.4 | 102.2 | 17.0 |
| `8e310f4` | 5.2 | 12.5 | 214.7 | 55.6 | 101.3 | 16.9 |
| `e4db04a` | 5.0 | 12.6 | 212.2 | 54.9 | 101.3 | 16.6 |

Peak RSS, MiB:

| Commit | small | medium | large | hf+toc | js | multi |
|---|---|---|---|---|---|---|
| base `ddaea2a` | 14.13 | 15.41 | 50.11 | 25.06 | 28.83 | 15.88 |
| `5e388a5` | 14.73 | 15.53 | 45.33 | 25.53 | 30.27 | 16.22 |
| `cebec04` | 14.97 | 16.23 | 47.52 | 25.65 | 29.43 | 16.97 |
| `2bf2e7a` | 14.97 | 16.53 | 46.21 | 26.27 | 29.95 | 17.11 |
| `2ec8b06` | 14.89 | 16.43 | 46.86 | 26.64 | 31.13 | 17.19 |
| `0876fdd` | 14.97 | 16.52 | 47.05 | 26.86 | 30.63 | 17.18 |
| `587e01b` | 15.30 | 16.65 | 47.71 | 26.82 | 30.80 | 17.27 |
| `505a1f0` | 15.03 | 16.56 | 47.89 | 27.03 | 30.75 | 17.41 |
| `4f9087d` | 15.11 | 16.52 | 47.01 | 26.94 | 30.46 | 17.43 |
| `0be9568` | 15.32 | 17.03 | 47.45 | 27.40 | 30.66 | 17.60 |
| `95eeca4` | 15.41 | 16.95 | 47.58 | 27.27 | 30.95 | 17.72 |
| `1ef0989` | 15.25 | 16.79 | 47.43 | 27.21 | 31.10 | 17.43 |
| `8e310f4` | 15.34 | 16.43 | 46.81 | 26.73 | 31.02 | 16.99 |
| `e4db04a` | 15.22 | 16.37 | 46.86 | 26.52 | 30.93 | 16.86 |

Between `0be9568` and `1ef0989` the multi-object and medium pages went over the 10 % budget
(+11.6 % and +10.5 %); the last two allocator and drafting commits bring every scenario
back within +8 %. What the extra RSS of the small documents is: the three more worker
threads (about 0.3 MiB each on `medium` and `multi`: their stacks, per-thread caches and the
heap they touch; with `RAYON_NUM_THREADS=1` the medium page is at +3.3 %), and dlmalloc
keeping more than musl (+0.3 MiB). `large.html` is lower than pass 4: large buffers are
mapped on their own, freed pages are given back.

## Byte-identity and determinism

Every commit was checked with `bench/pdf-identity.sh diff` against pass 4 (67 PDFs of the
corpus: bench scenarios, `tests/corpus`, `tests/js`, `tests/parts`, upstream fixtures),
with the macOS build and with the musl build in the container, and the final state three
times in a row on each, plus once with `RAYON_NUM_THREADS=1` and once with 8: all
byte-identical. While the draft threshold was two pages (before `8e310f4`) every multi-page
document of the corpus went through the drafts, and was identical too. `cargo test
--workspace --locked` (199 tests), clippy `-D warnings` and `cargo fmt --check` pass; the
vendored crates changed (stylo, blitz-dom, blitz-paint, krilla, pdf-writer) type-check for
`x86_64-pc-windows-msvc` (the workspace crates need ring's C build for that target, not
available here; the new allocator module is `cfg(unix)`, its global use `cfg(target_env =
"musl")`).

## Options evaluated and not taken

- **Writing a draft's text in the draft** (assuming no `/ActualText` span, CIDs patched when
  merging): correct, but the drafts got heavier and the drafting threads were the
  bottleneck: `large` 121.5 → 129 ms. The text is now written by the compressing job
  instead, after the merge (`505a1f0`).
- **Stylo's own thread pool** (its `use_current_thread` pool, 3 more threads): it made the
  conversion thread a member of that pool, so every later rayon call ran there while the
  global pool idled. Replaced by a traversal on the calling thread's rayon pool.
- **Plain `GlobalDlmalloc` with threads**: its single lock made the parallel inline layout
  construction slower than the sequential one (resolve 97 vs 60 ms on `large`).
- **Larger per-thread caches** (32 KiB per class): −4 ms on header/footer + TOC, but
  +1.1 to +1.7 MiB of peak RSS on every scenario.
- **Giving every freed page back** (4 KiB threshold): 0.5 MiB less on the jQuery page,
  5 % of the conversion thread in `madvise` and refaults on `large`.
- **Drafting short documents** (from 2 pages): 0.5–1 ms on the 4- and 8-page bench
  documents for 0.5 MiB of peak RSS.
- **Header/footer pages loaded in parallel**: each needs a Boa context, and Boa's contexts,
  GC and interner are per thread: every thread would parse the 69 KB DOM prelude again
  (14 ms).

## Profiles after the pass

`large.html`, conversion thread (`perf` in the bench VM): table layout ~20 % (grid track
sizing, cell min/max-content measures, line breaking), paint ~14 % (`stroke_text` and the
display list), HTML parse 9 %, box construction 7 %, merging the drafts (registering the
glyphs and texts of every run in the font: `check_text_span_prop` 4 %), page faults of
fresh memory ~9 % (also there under musl's allocator). The worker threads are the
bottleneck of the PDF phase: drafting the pages and the compression jobs (deflate, and now
the text operators) take about 45 and 30 ms of CPU.

Header/footer + TOC: Boa, 59 % of the conversion thread: parsing the DOM prelude once
(29 %) and evaluating it in each of the 16 header/footer realms (25 %).

jQuery page: Boa (parse of jQuery, the `onload` DOM build), sequential.

Remaining leads: measuring table cells (min/max-content) on the pool before the sequential
track sizing; painting large subtrees in parallel; the glyph-to-font registration of the
merge (per glyph hash lookups and a string per new glyph).

# Pass 4 (October 2026)

Goal: cut CPU, wall time and peak RSS further, `large.html` first (a warm-JVM
OpenHTMLtoPDF renders it in ~315 ms), and the fixed per-process cost of small pages. For the
first time the compressed bytes were allowed to change (a cheaper deflate setting), under
these conditions: total output size within ±1 %, every PDF still parses, all tests pass and
the *decompressed* content is identical. Only the first commit changes bytes; every other
one was checked byte for byte with `bench/pdf-identity.sh` against the build before it.

## Results

macOS arm64 host, release profile, `WKHTMLTOPDF_SYSTEM_FONTS=0`, bench arguments (`small`
and `large` with `--javascript-delay 0`). Instructions retired are the median, peak RSS the
lowest, of 11 `/usr/bin/time -l` runs; wall is the hyperfine mean of 30 runs. *base* is
`8144e44` (pass 3), *pass 4* is `e321b99`.

| Scenario | Instructions (M) base → pass 4 | Wall ms base → pass 4 | Peak RSS MiB base → pass 4 |
|---|---|---|---|
| small page | 51.3 → 47.0 (−8 %) | 5.9 → 5.1 | 12.8 → 13.1 |
| medium, 4 pages | 140.3 → 131.3 (−6 %) | 13.4 → 10.6 | 15.4 → 15.8 |
| large invoice, 30 pages | 2,177 → 2,000 (**−8 %**) | 172.9 → **119.0** (**−31 %**) | 57.0 → 58.6 |
| header/footer HTML + TOC, 8 pages | 578 → 570 (−1 %) | | 29.7 → 30.4 |
| jQuery page | 1,070 → 1,060 (−1 %) | | 37.3 → 37.9 |
| multi-object (cover, TOC, 3 parts) | 176 → 168 (−4 %) | | 16.0 → 16.1 |

The macOS RSS grows by 0.3–1.6 MiB with the two rayon worker threads (their stacks and the
system allocator's per-thread caches); on Linux/musl it does not move (below).

Heap peak (dhat, `--features dhat-heap`): small 1.49 → 1.27 MB, medium 2.22 → 1.97 MB, large
35.07 → 35.09 MB, hf+toc 13.19 → 13.20 MB, jQuery 12.04 → 12.06 MB, multi 4.15 → 4.05 MB.

Confirmation in the `wkbench:local` container (linux/arm64, static musl binaries of both
commits, same session, system fonts enabled as in the bench). Wall: hyperfine median of 20
runs; CPU: user+sys per conversion, median of 3 batches of 20 conversions (`/usr/bin/time`
only has 10 ms resolution per run); peak RSS: lowest of 9 runs.

| Scenario | Wall ms base → pass 4 | CPU ms base → pass 4 | Peak RSS MiB base → pass 4 |
|---|---|---|---|
| small page | 5.2 → 4.1 | 4.5 → 4.5 | 13.7 → 13.7 |
| medium, 4 pages | 13.8 → 10.6 | 13.0 → 12.5 | 14.9 → 14.8 |
| large invoice, 30 pages | 203.8 → **148.3** (**−27 %**) | 203 → **184** (−9 %) | 48.4 → 48.2 |
| header/footer HTML + TOC | 64.3 → 58.6 | 64.0 → 63.5 | 24.8 → 25.0 |
| jQuery page | 118.1 → 115.9 | 119.0 → 119.5 | 28.8 → 28.7 |
| multi-object | 21.0 → 16.0 | 17.5 → 17.5 | 15.4 → 15.6 |

`large.html` now converts in 148 ms of wall time in the container, against ~315 ms for the
warm-JVM OpenHTMLtoPDF run. CPU is above wall on `large` because page streams are compressed
on a worker thread while the next page is built.

## What changed

Instruction counts per scenario after each step (macOS, median of 7 runs; wall time on
`large` where the step targets it).

| Commit | Change | small / medium / large / hf+toc / js / multi |
|---|---|---|
| | base (`8144e44`) | 51.5 / 140.1 / 2,179 / 579 / 1,070 / 177 |
| `65a3bad` | `krilla`: zlib streams compressed by `miniz_oxide` in one call with lazy parsing and **48 hash-chain probes** instead of flate2's streaming writer at level 6 (128 probes). Content streams repeat the same operators over and over, and the long chains made deflate a quarter of `large.html`. **Changes compressed bytes** (below) | 51.0 / 137.4 / 2,048 / 574 / 1,067 / 174 |
| `f6e7375` | krilla's `rayon` feature: each page's content stream is a `Deferred` chunk compressed on the rayon pool while the next page is built; chunks are written in page order. The binary sizes the global pool to 2 threads (`RAYON_NUM_THREADS` overrides): one worker keeps up, and one thread per CPU only cost thread creation on short documents (+2.6 M instructions on the small page with 8 threads) | 51.6 / 138.9 / 2,046 / 575 / 1,067 / 175; `large` wall 155 → 121 ms |
| `76a17f8` | `wk-print`: a face's reverse cmap (a SipHash map of ~6,000 DejaVu entries) and its advance widths were built eagerly for every face used; the reverse cmap is now a dense vector built on first use (runs without source text only) and advances are read per glyph and cached | **46.7** / 131.4 / 2,041 / 569 / 1,062 / 168 |
| `348d300` | `blitz-dom`: shared user-agent sheets move from a per-thread to a process-wide cache (a `OnceLock` per sheet text, parsed against `about:blank`; the stylo prefs that decide what parses are set once before any parse). The binary parses `default.css` (23 KB) on the pool when a conversion starts (`wk_converter::warm_up`), overlapping the input read and the font collection | 46.8 / 131.0 / 2,046 / 569 / 1,062 / 168; small wall −5 % |
| `936437c` | `pdf-writer` vendored (0.15.0): `push_decimal` writes colour components `k / 255.0` from a 256-entry table built with ryu, and values whose shortest form has at most 4 decimals from the nearest multiple of 10^-d, accepted by an exact interval test (products are exact in f64; ties and powers of two go to ryu). Same bytes as ryu: unit test, plus an exhaustive comparison over every f32 in [1e-4, 9e6] and its negation (611 M values, 200 M through the fast path) | 46.9 / 131.3 / **2,002** / 570 / 1,061 / 168 |
| `f503be7` | `krilla`: with `rayon`, `serialize_fonts` subsets and compresses the programs of all used CID fonts in parallel (`CIDFont::font_program`) before writing the fonts in their usual order | 46.8 / 131.3 / 1,997 / 570 / 1,059 / 169; small: font serialisation 26 % → 10 % of the main thread |

## Output bytes and how equivalence was checked

`65a3bad` changes the compressed bytes of every stream (content streams, font programs,
CMaps, images). Over the 67 files of the identity corpus the total size goes from 2,696,829
to 2,708,806 bytes (**+0.44 %**; `large.html` +0.16 %). Checks:

- `bench/pdf-identity.sh equiv <before> <after>` (new, `bench/scripts/pdf_equiv.py`): every
  PDF has the same objects in the same order, the same dictionaries and the same
  **decompressed** stream data; stream `/Length`, xref tables, `startxref` and `/Prev` are
  masked (they follow from the sizes). It reports a change of a dictionary value (tested) and
  prints the total sizes. All 67 files pass, for the macOS build and for the musl builds run
  in the container.
- `pdfinfo` and `pdftotext` (poppler, in the container) on the 58 PDFs: all parse, with the
  same page counts and the same extracted text as the base build.
- `cargo test --workspace` (196 tests, including the L3 corpus rasterised by hayro), clippy
  `-D warnings` and `cargo fmt --check` pass. The test build now compresses like the
  release build: krilla called `flate2`, whose `zlib-rs` backend the dev-dependency `hayro`
  switched on in test builds only.

Every later commit is byte-identical to the one before it (`bench/pdf-identity.sh diff`).

## Profiles after the pass

`large.html`, main thread (samply, 5 runs): load 57 % (inline layout construction and
shaping 14 %, layout 15 % of which table track sizing 7 %, style 7 %, HTML parse 7 %), PDF
content building 23 % (`draw_path` 10 %, `draw_glyphs` 8 %; float formatting 8 %, of which
ryu 2 % for positions such as `183.59961`), painting 11 %, allocator 15 % (spread over every
phase). Deflate (22 % of all samples) runs on the worker.

Small page: in-process work is ~2 ms of the ~5 ms wall time on macOS; the rest is process
start-up and exit. Fixed costs left: the first document's style flush (UA cascade data,
~4 %), font subsetting.

jQuery and header/footer pages: Boa. The prelude is parsed once per process (~14 ms on
`hf_toc`), each page's realm evaluates it again (~1 ms each), jQuery's parse is 24 % and its
compilation 9 % of the jQuery page.

## Options evaluated and not taken

- **zlib-rs** (pure Rust, through flate2's `zlib-rs` feature) at level 6: 1.6x faster than
  miniz_oxide level 6 over the corpus, +0.7 % size; miniz_oxide with 48 probes is as fast on
  content streams (31.7 vs 31.9 ms for `large.html`'s 7.3 MB), smaller (+0.54 % vs +0.7 % of
  compressed bytes) and needs no new crate. 32 probes (level 5): +1.07 % on `large.html`,
  over the ±1 % budget. Greedy parsing: +2.9 % size.
- **blitz-dom `parallel-construct`** (inline layouts built on the pool, with a 512-task
  threshold): byte-identical, but each worker starts with a cold copy of the font and shaping
  caches: building took 37 ms of CPU instead of 16 ms on `large.html`, for −2.5 % wall.
- **dlmalloc** (`--features dlmalloc`, pure Rust) on the static musl build, measured again on
  this pass: CPU per conversion −23 % on `large` (184 → 157 ms), −16 to −22 % elsewhere,
  but peak RSS +0.8 MiB on the small page and +3 MiB (+10 %) on the jQuery page. musl's
  allocator is a large share of the remaining CPU in the container; the default stays the
  system allocator, as decided in the first pass.
- **System font registration**: with system fonts enabled (the default) every process opens,
  maps and scans every font file. In the bench container the 6 DejaVu files (duplicates of
  the embedded faces) cost 1.6 ms of the small page's 4.6 ms; on a macOS host with ~90 font
  files, 89 M instructions and 12 MiB of RSS. Fixing it needs a persistent scan cache like
  fontconfig's (keyed by path, size and mtime); left for a dedicated change.
- **Boa's optimizer off** (constant folding, strength reduction): 1.5 % of the jQuery page,
  not worth a bytecode change.
- **Parsing the DOM prelude on a worker**: Boa's `Context`, GC and interner are per thread.

# Pass 3 (October 2026)

Goal: lower CPU and peak RSS on every bench scenario, the table-heavy `large.html` first,
with the output unchanged. Every commit was checked with `bench/pdf-identity.sh` (67 PDFs
byte-identical to the base), and the branch again after merging `main` at `5d4cb8d` (the
fidelity pass that changed output on purpose): byte-identical to that `main`.

## Results

macOS arm64 host, release profile, `WKHTMLTOPDF_SYSTEM_FONTS=0`, `SOURCE_DATE_EPOCH` fixed,
bench arguments (`bench/scripts/inside.sh`; `small` and `large` with `--javascript-delay 0`).
Instructions retired and CPU are the median, peak RSS the lowest, of 11 `/usr/bin/time -l`
runs. Wall and CPU times are noisy on this shared host (other builds running); instruction
counts are reproducible to ±1 M. *main* is `5d4cb8d`, *pass 3* is this branch merged with it.

| Scenario | Instructions (M) main → pass 3 | CPU ms main → pass 3 | Peak RSS MiB main → pass 3 |
|---|---|---|---|
| small page | 53 → 52 (−2 %) | <10 | 13.0 → 12.8 |
| medium, 4 pages | 149 → 140 (−6 %) | 10 → 10 | 15.7 → 15.6 |
| large invoice, 30 pages | 2,675 → 2,173 (**−19 %**) | 200 → 160 | 60.0 → 57.4 |
| header/footer HTML + TOC, 8 pages | 1,332 → 579 (**−57 %**) | 120 → 40 | 34.2 → 29.9 |
| jQuery page | 1,083 → 1,071 (−1 %) | 90 → 90 | 37.5 → 37.2 |
| multi-object (cover, TOC, 3 parts) | 205 → 177 (−14 %) | 10 → 10 | 17.1 → 16.1 |

Confirmation in the `wkbench:local` container (linux/arm64, static musl binaries of both
commits, same session, same arguments; median user+sys and lowest peak RSS of 9
`/usr/bin/time` runs):

| Scenario | CPU s main → pass 3 | Peak RSS MiB main → pass 3 |
|---|---|---|
| small page | 0.00 → 0.00 | 13.7 → 13.7 |
| medium, 4 pages | 0.01 → 0.01 | 15.0 → 14.9 |
| large invoice, 30 pages | 0.24 → **0.19** | 48.6 → 48.4 |
| header/footer HTML + TOC | 0.16 → **0.05** | 31.5 → **24.8** |
| jQuery page | 0.11 → 0.11 | 28.9 → 28.7 |
| multi-object | 0.01 → 0.01 | 16.0 → 15.5 |

## What changed

Instruction counts per scenario after each step (macOS, median of 7 runs; the steps were
measured before the merge with `main`, on `6245fa4`, whose figures match `main`'s within 1 M).

| Commit | Change | small / medium / large / hf+toc / js / multi |
|---|---|---|
| | base (`6245fa4`) | 53 / 149 / 2,671 / 1,333 / 1,082 / 206 |
| `1a5476e` | `blitz-dom`: user-agent sheets (`default.css`, 23 KB, and the Qt form-control sheet) parsed once per thread under a read-only stylo lock, readable from every document's guard, so stylo's UA cascade-data cache also hits. A conversion loads one document per object and per header/footer page | 53 / 149 / 2,675 / 1,268 / 1,083 / 187 |
| `c0627fd` | `WebPage::load` lays the page out at the print viewport (the converter passes the printable area, `WebSettings::print_area`) instead of 800×600, so `render` finds nothing to lay out again. Media queries see `--viewport-size`, not this viewport, so resource discovery is unchanged | – / 144 / 2,402 / 1,258 / – / 182 |
| `73f0559` | `wk-print`: display-list commands bucketed by vertical extent (256 px) once per scene; each band (page body, repeated `thead`/`tfoot`) visits only its candidates, in order, with the same culling test, instead of the whole document's list | 53 / 144 / 2,346 / 1,260 / 1,084 / 182 |
| `8b32f31` | `wk-js`: the 69 KB DOM prelude is parsed and compiled once per thread. `boa_engine` is vendored with `Script::with_realm` (evaluate a compiled script in another realm); the prelude's global code declares nothing, so its bytecode does not depend on the realm. Each scripted header/footer page had paid ~70 M instructions for it | 53 / 144 / 2,345 / **584** / 1,082 / 181 |
| `2239bf2` | `wk-print`: solid rectangle fills (cell backgrounds, collapsed borders) replayed from the rectangle, without expanding them into an anyrender `Fill` with a heap `BezPath` (culling box and PDF path alike) | – / 143 / 2,228 / – / – / – |
| `b1b012a` | `blitz-dom`: grid row sizing measures each cell's height twice at the same width (min- then max-content available height); for an inline formatting context without inline boxes rooting its own BFC both give the same result, so the second reuses the first (one-entry memo in `LayoutPassState`) | – / 143 / 2,188 / – / – / – |
| `df93a86` | `blitz-dom`: the set of every created node (SipHash insert per node, only tested for emptiness) replaced by a flag | – / 143 / 2,187 / – / – / – |
| `7fb3d8b` | `wk-print`: the `/PageMode /UseOutlines` incremental update (every conversion with the default outline) parses only the catalog: it is copied with the xref table and trailer into a minimal PDF for lopdf, instead of lopdf parsing every object; full load kept for forms and unknown layouts; a unit test compares both paths byte for byte | 52 / 140 / 2,178 / 579 / 1,072 / 176 |

## Profiles after the pass

`large.html` (samply, 20 kHz): deflate 25 % (krilla's `miniz_oxide` at level 6, fixed by
the output), PDF content building and float formatting ~15 %, HTML parsing ~6 %, style ~5 %
(a third of it revalidating the style-sharing cache for `tr:nth-child(even) td`), layout ~8 %
(table track sizing, inline line breaking), inline layout construction ~7 % (shaping 6 %),
painting ~7 %. Load-time and render-time layouts are no longer both run (render's resolve is
0.2 %).

jQuery page: parsing jQuery itself 24 % (Boa's parser), compiling it 9 %, running the
`onload` DOM build 49 % (RegExp 6 %, GC 6 %), the realm and prelude 10 %.

Small page: heap peak 1.5 MB (dhat); the 13 MiB peak RSS is mostly code and font pages of the
binary. Per-process fixed costs: the UA sheet parse (~16 % of the instructions) and font
registration (~7 %).

## Options evaluated and not taken

- **Compressing page streams on a worker thread** (deflate is a quarter of `large.html`):
  would cut wall time, not CPU, and needs krilla to write each page's stream after
  compressing it off-thread (vendored krilla, placeholder lengths). Left for a pass that
  targets wall time.
- **Caching shaped text runs**: the invoice's cell texts are nearly all distinct (SKUs,
  amounts); the hit rate would not pay for the key hashing.
- **Shrinking parley layouts less often** (`shrink_to_fit` reallocations, ~2 % on macOS):
  the memory pass's trade-off; musl's `realloc` shrinks in place more cheaply than macOS's.
- **Raising Boa's GC threshold** for the jQuery page: CPU for RSS.
- **Minifying the prelude**: it has 51 comment lines; the parse cost is per token, and
  it is now paid once per process anyway.

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
# When only compressed bytes may change (pass 4): compare after decompressing every stream.
bench/pdf-identity.sh equiv /tmp/before /tmp/after

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
