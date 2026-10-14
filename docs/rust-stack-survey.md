# Rust stack survey for a pure-Rust wkhtmltopdf

Survey date: 2026-10-14. Versions, licenses, last-update dates and download counts were taken from the
crates.io API on that date. Project status comes from the upstream READMEs and status pages where
cited. Statements marked **[uncertain]** were not verified against source code and need checking
before we rely on them.

Goal: reproduce wkhtmltopdf's architecture (load page, run JS, style, lay out, paginate, print to
PDF) with Rust components only. No Chromium, no WebKit/Gecko, no C++ engines.

Legend: **Pure** = pure Rust with no C/C++ in the default build. **C-opt** = pure by default, with
an optional C dependency. **C** = needs C/C++ or assembly to build.

---

## 1. HTML parsing and DOM

| Crate | Version | License | Pure | Notes |
|---|---|---|---|---|
| `html5ever` | 0.40.1 (2026-09-14) | MIT/Apache-2.0 | Pure | WHATWG-conformant tokenizer and tree builder from Servo. It ships no DOM. You implement `TreeSink`, and it exposes the hooks a browser needs (`document.write`, script handling, template contents, quirks mode). Industry standard, used by Servo, Blitz and scraper. |
| `markup5ever` | 0.40.0 | MIT/Apache-2.0 | Pure | Shared atoms and interfaces for html5ever and xml5ever. |
| `xml5ever` | 0.40.0 | MIT/Apache-2.0 | Pure | XML/XHTML parser with the same `TreeSink` model. |
| `markup5ever_rcdom` | 0.39.0+unofficial | MIT/Apache-2.0 | Pure | `Rc<RefCell>` reference DOM, labelled "unsupported, for tests". Not suitable as an engine DOM. |
| `kuchikiki` (Brave fork of kuchiki) | 0.8.2 | MIT | Pure | Tree manipulation plus selectors. Fine for scraping, too thin for a live DOM. |
| `scraper` / `ego-tree` | 0.27.0 / 0.11.0 | ISC | Pure | Read-only querying. Not a mutable live DOM. |
| `dom_query` | 0.28.0 | MIT | Pure | Mutable, jQuery-like. Still not a browser DOM. |
| `html5gum`, `tl`, `lol_html` | 0.8.4, 0.7.8, 3.0.1 | MIT, MIT, BSD-3 | Pure | Tokenizers, fast parsers and streaming rewriters. None of them builds a spec tree. Not suitable. |
| `blitz-dom` + `blitz-html` | 0.2.4 / 0.2.0 (2026-10-12) | MIT/Apache-2.0 | Pure | A real engine DOM: a slab-allocated node tree that implements Stylo's `TElement`/`TNode` traits, with html5ever/xml5ever sinks. **Best starting point.** |

**Fit.** Use `html5ever` for parsing. For the DOM, either adopt or fork `blitz-dom`, which is already
wired to Stylo, Taffy and Parley, or write our own arena DOM that implements Stylo's traits. A
JS-visible DOM needs stable node identities (handles), owner documents, live collections and
mutation observers/events. Blitz has none of the JS-facing parts.

---

## 2. CSS: parsing, selector matching, cascade, computed styles

| Crate | Version | License | Pure | Notes |
|---|---|---|---|---|
| `stylo` (+ `stylo_dom`, `stylo_traits`, `stylo_config`) | 0.22.0 (2026-09-30) | MPL-2.0 | Pure | The Servo/Firefox style system: parsing, cascade, `@media`, `@font-face`, custom properties, computed values, parallel restyle and invalidation. **Published on crates.io as standalone crates and usable outside Servo.** Blitz, fulgur and hyper-render consume it from crates.io. It is built in "servo" mode, and some Gecko-only properties are absent there. |
| `selectors` | 0.41.0 | MPL-2.0 | Pure | Selector matching with bloom filters. Lives in the stylo repo. Used standalone by several projects (sghtmltopdf, spindrift). |
| `cssparser` | 0.38.0 | MPL-2.0 | Pure | Tokenizer and parser primitives (CSS Syntax Level 3). |
| `lightningcss` | 1.0.0-alpha.72 | MPL-2.0 | Pure | Parser, minifier and transformer with a typed property model. It has **no cascade or computed-value engine** and is built for bundling, not rendering. ironpress uses it as a parser. |
| `stylo_taffy` | (bundled with blitz) | MIT/Apache-2.0 | Pure | Converts Stylo computed styles into Taffy styles. |

**Fit.** Use `stylo`. Writing a cascade, inheritance, computed-value resolution, `calc()`,
custom properties and media queries ourselves would cost person-years. Stylo's MPL-2.0 is
file-level copyleft: modified Stylo files must stay MPL, but our own code can use any license.

**Paged-media properties in Stylo.** **[uncertain]** We need to check which of `break-before`,
`break-after`, `break-inside`, `page-break-*`, `orphans`, `widows` and `@page` rules (with
`size`, margins and margin boxes) Stylo parses and computes in servo mode. The Blitz CSS status page
lists `size`, `break-*` and `page-break-inside` as "Not supported", but that may describe the layout
side, not the parser. fulgur implements `break-*` and `@page` on top of Stylo, possibly with its own
parsing. If Stylo lacks them, we must add them to Stylo through its property templates (a Mako code
generator in the build) or parse them on the side.

---

## 3. Layout

| Crate / project | Version | License | Pure | Block | Inline | Floats | Tables | Flex | Grid | Fragmentation |
|---|---|---|---|---|---|---|---|---|---|---|
| `taffy` | 0.15.0 (2026-10-12) | MIT | Pure | yes | no (leaf measure callback only) | yes (`float_layout` feature) | no | yes | yes | **no** |
| Blitz (`blitz-dom` layout on Taffy + Parley) | 0.2.x | MIT/Apache-2.0 | Pure | yes | yes (Parley inline formatting context) | supported per status page | partial (`display:table` and `border-collapse` partial) | yes | yes (no subgrid) | **no** (`break-*` and `size` "Not supported") |
| `servo-layout` (Servo layout, formerly layout_2020) | 0.7.0 (2026-10-05, now on crates.io) | MPL-2.0 | Pure (the crate itself) | yes | yes | yes | yes | yes | yes (via Taffy) | **no** (Servo has no print pipeline; issue #3788 "Vector graphics pipeline", labelled A-printing, has been open since 2014) |
| `azul-layout` | 0.0.16 | MIT | Pure | yes | yes | ? | basic | yes | ? | basic page breaks (used by printpdf's experimental `html` feature) **[uncertain detail]** |
| fulgur (own pagination layer over Blitz) | 0.40.0 (2026-07-31) | MIT/Apache-2.0 | Pure | via Blitz | via Blitz | via Blitz | via Blitz | via Blitz | via Blitz | **yes**: `break-before/after/inside`, orphans/widows, `@page` size and margins, margin boxes, `position: running()`, page counters |

Notes:
- `servo-layout` is published but tightly coupled to Servo's `script` and `fonts` crates, and Servo's
  DOM runs on SpiderMonkey (`mozjs`, C++). Reusing it outside Servo means a fork. **[uncertain]**
  We have not tested whether it builds standalone.
- **Paged media and fragmentation: no general-purpose Rust layout engine supports block
  fragmentation in its core layout.** fulgur is the only crate found that paginates HTML with CSS
  break properties, and it does so *after* Blitz/Taffy layout by converting the box tree into a
  "Pageable" tree and splitting it at page boundaries. That is the same post-layout approach as
  wkhtmltopdf/QtWebKit, which lays out one tall page and slices it with break heuristics, so it is
  a valid, faithful model for our goal.
- **wkhtmltopdf fidelity note.** QtWebKit renders the page at a fixed viewport width, then
  `QPrinter`/`QWebFrame::print` slices it with WebKit's 2012-era page-break logic. It honours
  `page-break-before/after/inside: avoid` partially, and in tables only `thead` repeats. Matching
  wkhtmltopdf output is closer to "layout once, slice smartly" than to true CSS fragmentation.

---

## 4. Text: shaping, line breaking, bidi, fonts

| Crate | Version | License | Pure | Role / fit |
|---|---|---|---|---|
| `parley` | 0.12.0 (2026-10-09) | Apache-2.0/MIT | Pure | Rich-text layout: itemization, bidi, shaping, line breaking, alignment, inline boxes. Used by Blitz, fulgur and spindrift. **Recommended.** The 0.12 release split it into `parley_engine` and `parlance`. |
| `fontique` | 0.12.0 | Apache-2.0/MIT | C-opt | System font enumeration, CSS-style matching and fallback. On Linux it uses **fontconfig (C)** by default **[uncertain whether dlopen or link]**. For a static binary we need its pure backend or our own collection. |
| `swash` | 0.2.10 (2026-07-17) | Apache-2.0/MIT | Pure | Shaping and scaling. Being superseded by fontations + harfrust in the Linebender stack. |
| `skrifa` / `read-fonts` | 0.48.0 / 0.45.0 | MIT/Apache-2.0 | Pure | Google Fontations: font parsing, metrics, outlines, hinting. Used by krilla, Blitz and parley. |
| `harfrust` | 0.14.0 (2026-10-04) | MIT | Pure | Official HarfBuzz port to Rust on top of read-fonts. Successor of rustybuzz. |
| `rustybuzz` | 0.20.1 (2024-11, stale) | MIT | Pure | Superseded by harfrust. krilla and ironpress still use it optionally. |
| `fontdb` | 0.24.0 | MIT | Pure | Simple in-memory font database that scans font directories and parses fontconfig XML in pure Rust. Good for a static binary. |
| `cosmic-text` | 0.19.0 (2026-04) | MIT/Apache-2.0 | Pure | All-in-one text layout for GUIs, built on swash and fontdb. Less CSS-oriented than parley. |
| `unicode-linebreak` | 0.1.5 (2023) | Apache-2.0 | Pure | UAX #14. Stable but old Unicode data. |
| `icu_segmenter` / `icu_properties` | 2.3.0 | Unicode-3.0 | Pure | ICU4X line, word and grapheme segmentation, including dictionary/LSTM segmentation for Thai, Lao and similar scripts. Parley uses ICU4X data. |
| `unicode-bidi` | 0.3.18 | MIT/Apache-2.0 | Pure | UAX #9. |
| `hypher` / `hyphenation` | 0.1.8 / 0.8.4 | MIT/Apache-2.0 | Pure | Hyphenation for `hyphens: auto`. hypher is maintained by the Typst team. |
| `allsorts` | 0.17.0 | Apache-2.0 | Pure | Parsing, shaping and subsetting, including WOFF/WOFF2. An alternative path. |

**Fit.** Use parley, fontique (or fontdb for static Linux), skrifa, harfrust and ICU4X. Shaping,
bidi and line breaking are solved. Missing: web-font loading glue (`@font-face` from network with
WOFF2 decoding, for which Blitz uses `wuff`/`woff`), and a static-friendly system-font backend.

---

## 5. JavaScript engine (pure Rust)

| Engine | Version | License | Pure | Conformance / maturity |
|---|---|---|---|---|
| **Boa** (`boa_engine`, `boa_gc`, `boa_runtime`, `boa_wintertc`) | 0.22.0 (2026-08-28) | Unlicense/MIT | Pure | **95.60 % test262** per the v0.22 release notes (v0.21: 94.12 %, v0.20: 89.92 %). Bytecode VM with polymorphic inline caches since 0.22. Temporal on by default. `boa_wintertc` (new in 0.22) provides WinterTC web APIs: console, fetch Request/Response, AbortController, atob/btoa. **Performance:** an interpreter only, with no JIT. It is generally reported to be several times to an order of magnitude slower than QuickJS/V8-jitless on benchmarks **[uncertain, measure ourselves]**. That is fine for the typical wkhtmltopdf workload (chart libraries, jQuery DOM tweaks, a `window.status` wait), but risky for heavy SPAs. |
| **Nova** (`nova_vm`) | 1.0.0 (2026-03-15) | MPL-2.0 | Pure | Data-oriented arena VM with a safepoint GC, using the oxc parser. The README states known gaps: RegExp has no lookaround or backreferences, `Array` has no sparse storage, and Promise subclassing is unsupported. Performance is "acceptable". The test262 rate is tracked on trynova.dev (not retrieved). Less complete than Boa. |
| `rquickjs` | 0.14.0 | MIT | **C** (QuickJS) | Excluded by the directive (C engine). Listed only as the fallback if Boa proves too slow. |
| `deno_core` / `mozjs` | 0.412.0 / 0.26.8 | MIT / MPL-2.0 | **C++** (V8 / SpiderMonkey) | Excluded. |

**DOM bindings on Boa.** Boa exposes native classes through the `boa_engine::class::Class` trait:

```rust
impl Class for Element {
    const NAME: &'static str = "Element";
    const LENGTH: usize = 0;
    fn data_constructor(new_target: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<Self>;
    fn init(class: &mut ClassBuilder<'_>) -> JsResult<()>;   // methods, accessors, statics
}
// native data: #[derive(Trace, Finalize, JsData)]; register with ctx.register_global_class::<T>()
```

Methods use `NativeFunction` and `this.as_object()?.downcast_ref::<T>()`. Accessors and prototype
chains (`Node` → `Element` → `HTMLElement` → `HTMLDivElement`) need manual
`ObjectInitializer`/`FunctionObjectBuilder` work. `boa_interop` (last published at 0.20) offered
helper macros, and there are `boa_macros` derives. **[uncertain]** We have not checked whether a
`#[boa_class]`-style attribute macro is stable in 0.22.

**Existing Rust DOM-on-Boa projects.** Nothing production-grade was found. Blitz explicitly has no
JS bindings ("contributions welcome"). Boa's own `boa_runtime` covers console and fetch, not the
DOM. **We will write the DOM binding layer ourselves**, ideally generated from WebIDL (Servo's
`script` crate does this with a Python codegen for SpiderMonkey, which serves as an architecture
reference).

---

## 6. Images and SVG

| Crate | Version | License | Pure | Notes |
|---|---|---|---|---|
| `image` | 0.25.10 | MIT/Apache-2.0 | C-opt | PNG, JPEG (zune-jpeg), GIF, WebP, BMP, ICO, TIFF. AVIF decode needs `dav1d` (C), which is optional. |
| `zune-jpeg` / `zune-png` / `zune-image` | 0.5.15 / 0.5.2 / 0.5.0 | MIT/Apache-2.0/Zlib | Pure | Fast decoders. `image` already uses zune-jpeg. |
| `png`, `gif`, `image-webp` | 0.18.1, 0.14.2, 0.2.4 | MIT/Apache-2.0 | Pure | Individual codecs. |
| `resvg` / `usvg` | 0.48.1 (2026-08-02) | Apache-2.0/MIT | Pure | Most complete Rust SVG renderer. `usvg` parses into a simplified tree. |
| `krilla-svg` | 0.8.1 | MIT/Apache-2.0 | Pure | Converts a usvg tree into **vector PDF** content through krilla. Replaces `svg2pdf` (0.13.0, 2025). |

**Fit.** Solved. Keep JPEG passthrough (DCT) into PDF instead of re-encoding, as wkhtmltopdf does.
Inline `<svg>` in HTML needs DOM-to-usvg conversion: Blitz serialises the subtree and reparses it
with usvg.

---

## 7. Networking

| Crate | Version | License | Pure | Notes |
|---|---|---|---|---|
| `ureq` | 3.4.2 (2026-09-13) | MIT/Apache-2.0 | see TLS | Blocking client. Supports HTTP/SOCKS proxies, cookies (`cookie_store` feature), redirects, gzip/brotli, and rustls or native-tls. Small dependency tree, no async runtime. **Good fit for a CLI.** |
| `reqwest` | 0.13.5 | MIT/Apache-2.0 | see TLS | Async (tokio + hyper 1.12). Cookie jar, proxies (including `NO_PROXY`), basic/bearer auth, HTTP/2. Blitz's `blitz-net` uses it. Heavier. |
| `rustls` | 0.23.46 | Apache-2.0/ISC/MIT | **C/asm in crypto provider** | Pure-Rust TLS protocol. The default crypto provider is `aws-lc-rs` (C, needs cmake). The alternative `ring` provider has C and assembly, built with `cc`, and links statically without trouble. A fully pure-Rust provider (`rustls-rustcrypto`) exists but is experimental **[uncertain status]**. |
| `cookie_store` | 0.22.1 | MIT/Apache-2.0 | Pure | RFC 6265 jar with load/save. Maps to `--cookie-jar`. |
| `webpki-roots` / `rustls-native-certs` | — | — | Pure | Bundled Mozilla roots vs system store. |

**Fit.** Use `ureq` 3 with rustls (`ring` provider) and cookie_store. The page loader must also
provide `file://` and `data:` URLs, custom headers (`--custom-header`, `--custom-header-propagation`),
POST (`--post`, `--post-file`), basic auth (`--username/--password`), proxy (`--proxy`, with
`--bypass-proxy-for`) and SSL client certificates (`--ssl-crt-path`, `--ssl-key-path`). All of
these are reachable with ureq. Concurrent subresource loading can use a thread pool. An async
runtime is not required.

---

## 8. PDF output

| Crate | Version | License | Pure | Level / features |
|---|---|---|---|---|
| **`krilla`** | 0.8.2 (2026-06-04) | MIT/Apache-2.0 | Pure | High-level surface: fills, strokes, gradients, glyph runs, images, SVG (via krilla-svg). **Real text with font subsetting** (TTF and CFF flavours, colour fonts), ToUnicode. **Links, named destinations, outlines.** Tagged PDF, PDF/A-1 to 4, PDF/UA-1, PDF 1.4 to 2.0. Out of scope: layout, page breaking, **encryption and signatures**. **Forms are not listed.** Used by Typst, fulgur and hyper-render. |
| `pdf-writer` | 0.15.0 (2026-05-27) | MIT/Apache-2.0 | Pure | Low-level, typed, streaming object writer, used by krilla. Supports **AcroForm/widget annotations** at object level, so forms can be emitted here alongside krilla **[uncertain: how to mix with krilla's document writer; may need a krilla patch]**. |
| `subsetter` | 0.2.6 | MIT/Apache-2.0 | Pure | OpenType/CFF subsetting (krilla uses it). |
| `printpdf` | 0.12.8 (2026-09-05) | MIT | Pure | Mid-level. Font subsetting, links, bookmarks. Has an **experimental HTML feature** (azul-layout): "basic layouts: tables, page breaks, headers/footers", no JS. |
| `lopdf` | 0.45.0 | MIT | Pure | Read/modify/merge existing PDFs. Useful for post-processing (merging the cover, TOC and page parts as wkhtmltopdf does). |
| `hayro` | 0.8.0 | Apache-2.0/MIT | Pure | PDF renderer/interpreter. Useful for **visual regression tests** (render our PDF to PNG). |
| `pdf_oxide` | 0.3.78 | MIT/Apache-2.0 | Pure | PDF reading/extraction plus an HTML+CSS-to-PDF pipeline on Taffy (no JS). |

**Fit.** Use krilla as the PDF painter backend: a `PdfPainter` that walks our display list and
emits krilla surface calls. Add AcroForm emission (wkhtmltopdf `--enable-forms`) via pdf-writer or
a krilla contribution. Encryption is not a wkhtmltopdf feature, so that does not matter.

---

## 9. XSLT (for the TOC)

wkhtmltopdf builds an XML outline and transforms it with XSLT 1.0 (`--xsl-style-sheet`, default
stylesheet via `--dump-default-toc-xsl`) through Qt's `QXmlQuery`, then renders the resulting HTML.

| Crate | Version | License | Pure | Notes |
|---|---|---|---|---|
| `xrust` | 2.3.0 (2026-10-13) | Apache-2.0 | Pure | XPath plus XSLT, active (GNOME World/markup-rs). Coverage of XSLT 1.0 corner cases is **[uncertain]**. **Most promising.** |
| `xee` (`xee-xpath`, `xee-xslt-compiler`) | 0.1.5 / 0.1.6 (2025-08) | MIT | Pure | XPath 3.1 is solid. The XSLT 3.0 compiler is early. |
| `sxd-xpath` | 0.4.2 (2018) | MIT/Apache-2.0 | Pure | XPath 1.0 only, unmaintained. |
| libxslt bindings | — | — | C | Excluded. |

**Fit and alternatives.**
1. Default TOC: generate the TOC HTML natively in Rust. That covers almost everyone, since custom
   XSL is rare.
2. Custom `--xsl-style-sheet`: try `xrust`. If it fails on real stylesheets, implement a minimal
   XSLT 1.0 interpreter over `xee-xpath` (XPath 1.0 is a subset), covering the stylesheets
   people actually use: `template`, `apply-templates`, `for-each`, `value-of`, `if`, `choose`,
   `attribute`, `element`.

---

## 10. Whole-engine projects: reuse or learn from

| Project | Language / deps | JS | Paged media | How close to our target |
|---|---|---|---|---|
| **Blitz** (DioxusLabs), crates `blitz-*` 0.2.x | Rust: html5ever + Stylo + Taffy + Parley, painting via `anyrender` (vello, vello_cpu, skia backends) | **None** (explicitly a "no-JS" renderer) | None | **Closest reusable foundation.** About 60 % of the static pipeline (parse, style, layout, paint commands). Beta: tables partial, counters unsupported, `::before/::after` only for strings. No JS, no pagination, no PDF backend in-tree (anyrender has no PDF backend **[uncertain]**). |
| **fulgur** 0.40.0 | Blitz + Stylo + Taffy + Parley + krilla, MIT/Apache | None, and offline (no network) | **Yes**: break-*, orphans/widows, `@page`, margin boxes, running headers, counters, bookmarks, tagged PDF | **Proves the Blitz-to-paginate-to-krilla pipeline works.** Lacks JS, networking and the wkhtmltopdf CLI. Small community (about 89 stars). Read its pagination code first, and consider vendoring it. |
| `hyper-render` 0.2.2 | Blitz + anyrender_vello_cpu + krilla | None | **[uncertain]** | Small PNG/PDF renderer over Blitz. A reference for the Blitz-to-krilla painter. |
| `spindrift` 0.1.0 (2026-09) | html5ever + selectors + taffy + parley + pdf-writer, own cascade | None | Aims at spec compliance **[uncertain]** | Young (single release). Interesting as an independent design. |
| `sghtmltopdf` 0.5.1 | html5ever + selectors + taffy + harfrust + pdf-writer + ureq | None | **[uncertain]** | "No Chromium/WebKit/Gecko". Own cascade. Young. |
| `ironpress` 1.7.0 | html5ever + lightningcss + rustybuzz + own layout | None | Basic | Own layout engine (flex, grid, floats, tables). A fast-moving solo project. |
| `fullbleed` 2.5.22 | Own engine (few public deps; earlier 0.x was AGPL, now MIT) | None | Yes (transactional docs) **[uncertain]** | A deterministic document generator, not a browser. |
| printpdf `html` feature | azul-layout | None | Basic page breaks | Experimental. Not a browser engine. |
| `html2pdf` 0.9.0 and others | headless Chrome | — | — | **Excluded** (Chromium). |
| **Servo** (`servo` 0.7.0 on crates.io, libservo embedding API) | Rust, but JS = **SpiderMonkey (C++, `mozjs`)**, and WebRender | Full | **None**: no print or PDF pipeline (A-printing issue #3788 open since 2014) | Fails the "no C++ engine" rule because of SpiderMonkey. The best **architecture reference** for the Rust-side DOM, the script/layout split, and WebIDL bindings codegen. `servo-layout` is the most complete Rust CSS layout, but it is coupled to Servo. |
| **Ladybird** (C++) | LibWeb, LibJS | Full | Has some print support **[uncertain]** | Not reusable. A clean, readable reference for spec-faithful implementations (layout, IDL bindings generator, event loop). |
| **WeasyPrint** (Python) | own engine | None | **Best-in-class CSS Paged Media** | The algorithmic reference for fragmentation: `layout/page.py` and `layout/block.py` with `page_break` and `skip_stack`. Port its ideas. |
| **Typst** (`typst-pdf`, `typst-html` 0.15.1) | Rust | No | Typst layout, not CSS | Its HTML export goes Typst → HTML, the wrong direction. Its value is the **krilla/subsetter/hypher toolchain**. |
| `kuchiki` / `kuchikiki` | — | — | — | A DOM-manipulation library only. |

**Honest assessment.** No Rust project combines JS execution, a CSS engine and paginated PDF output.
Blitz plus fulgur covers static HTML/CSS to paginated PDF reasonably well today. JavaScript (Boa
plus DOM bindings plus an event loop) is the biggest piece nobody has built.

---

## 11. Recommended stack

| Layer | Recommendation | Version | License | Alternative |
|---|---|---|---|---|
| HTML parse | `html5ever` (+ `xml5ever` for XHTML) | 0.40.x | MIT/Apache | — |
| DOM | Fork/adopt `blitz-dom`, extended with JS-facing identity, events and mutation | 0.2.4 | MIT/Apache | Own arena DOM implementing Stylo traits |
| CSS | `stylo` + `selectors` + `cssparser` | 0.22 / 0.41 / 0.38 | MPL-2.0 | — |
| Box layout | `taffy` (block, flex, grid, floats) via Blitz | 0.15.0 | MIT | `servo-layout` (fork) |
| Inline/text layout | `parley` | 0.12.0 | Apache/MIT | `cosmic-text` |
| Shaping / fonts | `harfrust` + `skrifa`/`read-fonts`; `fontique` or `fontdb` for system fonts | 0.14 / 0.48 / 0.12 / 0.24 | MIT/Apache | `swash`, `allsorts` |
| Segmentation / bidi / hyphenation | `icu_segmenter`, `unicode-bidi`, `hypher` | 2.3 / 0.3.18 / 0.1.8 | Unicode-3.0 / MIT/Apache | `unicode-linebreak` |
| Pagination | **Own fragmentation layer**, starting from fulgur's Pageable design and WeasyPrint's algorithms | — | — | Vendor fulgur 0.40 |
| Paint → PDF | **Own display list → `krilla` painter** | 0.8.2 | MIT/Apache | `pdf-writer` direct |
| PDF forms | `pdf-writer` AcroForm objects (or a krilla contribution) | 0.15.0 | MIT/Apache | — |
| PDF post-process (merge, TOC pages) | `lopdf` | 0.45.0 | MIT | Do everything in a single krilla document |
| SVG | `usvg` + `krilla-svg` | 0.48.1 / 0.8.1 | Apache/MIT | `resvg` for raster fallback |
| Raster images | `image` (+ `zune-jpeg`) | 0.25.10 | MIT/Apache | — |
| JS engine | `boa_engine` (+ `boa_gc`, `boa_wintertc`) | 0.22.0 | Unlicense/MIT | `nova_vm` (less complete) |
| HTTP | `ureq` 3 + `rustls` (ring provider) + `cookie_store` | 3.4.2 / 0.23.46 / 0.22.1 | MIT/Apache/ISC | `reqwest` 0.13 |
| XSLT (custom TOC XSL) | Native TOC generation; `xrust` for custom XSL | 2.3.0 | Apache-2.0 | Minimal XSLT 1.0 interpreter on `xee-xpath` |
| Visual tests | `hayro` (PDF → PNG) | 0.8.0 | Apache/MIT | — |

License summary: everything is permissive except Stylo, `servo-layout` and `nova_vm` (MPL-2.0,
file-level copyleft). The ICU4X data license (Unicode-3.0) is also permissive. Compatible with
an MIT/Apache or LGPL project. wkhtmltopdf itself is LGPLv3.

Static binary: everything is achievable as pure Rust except the rustls crypto provider (`ring`
compiles C/asm via `cc` but links statically with musl) and the fontique fontconfig backend
(avoid it on Linux and use fontdb-style directory scanning).

---

## 12. Gaps we will have to write ourselves

1. **DOM-to-JS bindings.** WebIDL-driven codegen to Boa `Class`/`ClassBuilder`: `Node`,
   `Element`, `HTMLElement` and subclasses, `Document`, `DocumentFragment`, `Text`, `Comment`,
   `NodeList`/`HTMLCollection` (live), `Event`/`EventTarget`, `CSSStyleDeclaration`
   (`element.style`, `getComputedStyle`), `getBoundingClientRect`/`offset*` (forces a synchronous
   layout), `classList`, `dataset`, `innerHTML`/`outerHTML` (fragment parsing via html5ever),
   `XMLHttpRequest`/`fetch`, `Image`, `canvas` (2D at least, since chart libraries depend on it;
   paint with vello_cpu/tiny-skia to raster, or record vector), `window.print`, `window.status`
   (wkhtmltopdf `--window-status`), `location`, `history` stubs, `localStorage` stub.
   GC integration: JS wrappers must keep DOM nodes alive (Boa `Trace` on node handles).
2. **Browser event loop and page lifecycle.** Script execution order (sync, `defer`, `async`),
   `document.write` during parsing (html5ever's script-pausing hooks), task and microtask queues,
   `setTimeout`/`setInterval`/`requestAnimationFrame` with a virtual clock, `DOMContentLoaded` and
   `load`, and the wkhtmltopdf semantics `--javascript-delay`, `--window-status`, `--no-stop-slow-scripts`
   and `--run-script`.
3. **Resource loader.** Subresource fetching (CSS `@import`, images, fonts, scripts), caching,
   `--allow`/`--disable-local-file-access` policy, and timeouts. `blitz-net` is a reference but is
   tokio/reqwest-based.
4. **Paged-media fragmentation.** Splitting the laid-out box tree across pages: block fragmentation,
   line-level splitting in inline formatting contexts, orphans/widows, `break-*`/`page-break-*`,
   table row splitting with repeated `thead`/`tfoot`, image avoidance, `@page` size and margins,
   and the `--print-media-type` switch (`@media print`). Base it on fulgur and WeasyPrint. Decide
   between true fragmentation and wkhtmltopdf's "tall page + slice" behaviour for compatibility.
5. **PDF painter.** Display list (backgrounds, borders including radius and styles,
   box-shadow, gradients, text runs with real glyphs, images, SVG, clipping, opacity, transforms) →
   krilla. Include link annotations for `<a href>` (external and internal anchors), outline from
   `h1`–`h6` (`--outline`, `--outline-depth`), and AcroForm fields for `<input>`/`<select>`/`<textarea>`
   (`--enable-forms`). Blitz's `blitz-paint` emits anyrender commands, so a krilla-backed
   `anyrender` implementation is one route.
6. **wkhtmltopdf features on top.** Header/footer HTML per page with `[page]`, `[topage]`,
   `[section]`, `[title]` and similar substitution variables (passed as a query string to
   header/footer pages run with JS). Cover page, TOC object with dot leaders and page numbers (a
   two-pass layout), multiple input objects in one PDF, `--zoom`, `--dpi`, `--viewport-size`,
   `--disable-smart-shrinking` (the shrink-to-fit heuristic), and `--minimum-font-size`.
7. **CSS gaps in Blitz/Stylo integration.** Full tables, counters and `content: counter()`,
   generated images in `::before/::after`, `list-style-image`, and the paged-media properties if
   Stylo does not parse them in servo mode **[verify]**.
8. **XSLT 1.0.** Only if `xrust` cannot run real-world TOC stylesheets.
9. **Fonts for a static binary.** A pure-Rust system font discovery backend for Linux (no
   fontconfig), plus web-font loading with WOFF/WOFF2 decoding (`wuff`/`woff` exist).
10. **Test harness.** Comparing against wkhtmltopdf 0.12.6 output: PDF text extraction plus a
    raster diff via `hayro`.

---

## Sources

- crates.io API (`https://crates.io/api/v1/crates/<name>`), queried 2026-10-14, for all versions,
  licenses and dependency lists.
- Blitz README and CSS status page: https://github.com/DioxusLabs/blitz, https://blitz.is/status/css
- fulgur: https://github.com/fulgur-rs/fulgur
- Boa v0.22 release: https://github.com/boa-dev/boa/releases/tag/v0.22. Boa class API:
  https://docs.rs/boa_engine/0.22.0/boa_engine/class/index.html
- Nova: https://github.com/trynova/nova
- krilla README: https://github.com/LaurenzV/krilla
- printpdf README: https://github.com/fschutt/printpdf
- Servo printing issue: https://github.com/servo/servo/issues/3788
