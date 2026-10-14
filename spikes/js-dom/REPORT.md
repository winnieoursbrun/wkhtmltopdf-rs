# Spike: can Boa drive a blitz-dom document? (P4 feasibility)

Date 2026-10-14, Apple M1, macOS arm64, release build (thin LTO, stripped). Versions:
boa_engine 0.22.0, blitz-dom/blitz-html 0.3.0-beta.3, html5ever 0.40.1. The machine was heavily
loaded during the runs (load average 14–47), so treat the timings as upper bounds.

## Verdict

Yes. Boa 0.22 drives a blitz-dom 0.3 document with **no patch to either crate**. These pages
produce the expected DOM: the canonical footer substitution script, jQuery 3.7.1, a 500-row table
built with plain DOM calls, a `setTimeout` chain ending in `window.status`, and `document.write`.
Chart.js 2.9.4 and 4.4.4 load and draw a full chart against a stub 2D context that records calls.
Canvas 2D itself is the missing piece.

Blitz 0.3 already exposes most of what bindings need: `DocumentMutator` (damage, restyle hints,
`<style>`/`<link>` processing, id map), `set_inner_html` (html5ever fragment parsing), versioned
`NodeId`s (a stale id resolves to `None`), `query_selector(_all)_in`, `get_client_bounding_rect` and
`offset_rect`, `resolved_style_value` (`getComputedStyle`), `style_attr_get/set_property` (CSSOM on
the `style` attribute).

The real costs are the breadth of the binding surface and canvas 2D. Boa's speed is acceptable for
wkhtmltopdf workloads. Its fixed size cost is +10 MB.

## What was built

`jsdom-spike PAGE [--query Q] [--javascript-delay MS] [--window-status S] [--run-script JS]… [--debug-javascript] [--audit] [--canvas-recorder] [--loop-limit N] [--out F] [--expect T]… [--expect-not T]…`.
`./run.sh` runs every page with its assertions and takes the median of 5 timings. About 2,100
lines in `src/`:

- `src/dom.rs`: native bindings (Node, CharacterData, Element, Document, DocumentFragment
  prototypes, ParentNode methods, layout getters, `document.write`, an HTML serializer).
- `src/host.rs`: host state, the `__wk` natives (console, dialogs, timers, style, computed style),
  script execution, the virtual-clock event loop and `window.status` polling.
- `src/prelude.js`: JS glue (EventTarget/Event, `style` Proxy, `classList`, `location`,
  window/navigator/screen, constructors for `instanceof`, canvas call recorder, missing-API audit).
- `src/main.rs`: the lifecycle in upstream order: parse; scripts in document order;
  `DOMContentLoaded`; `<body onload>`/`load`; `--run-script`; the wait phase as in
  `multipageloader.cc` (poll `window.status` every 50 virtual ms, then wait `--javascript-delay`
  more; `window.print()` ends the wait).
- `src/baseline.rs`: blitz without Boa, to measure the size delta.

## Results per page

| Page | Result | scripts | total |
|---|---|---|---|
| 1 footer subst (`?page=2&topage=5&section=Intro`, `<body onload="subst()">`) | PASS | 0.3 ms | 5.3 ms |
| 2 jQuery 3.7.1 (`$(fn)`, `.text`, `.append`, `.css`, `.attr`, `$('<p/>',{…})`, `.html`, `.each`, `.height()`) | PASS (`.height()` = 19 px via forced layout) | 27.5 ms | 33.8 ms |
| 3 500-row table with plain DOM (3,500 nodes) | PASS (sum identical to Node.js) | 15.4 ms | 30.4 ms |
| 4 `setTimeout` chain, `setInterval`/`clearTimeout`, `--window-status ready` | PASS on the virtual clock | 0.6 ms | 6.9 ms |
| 5 Chart.js 4.4.4 bar chart | Completes with `--canvas-recorder`; without it `getContext` returns `null` | 135 ms | 141 ms |
| 5b Chart.js 2.9.4 | Same | 83 ms | 88 ms |
| 6 `while(true){}` with `--loop-limit` | Aborted with `RuntimeLimitError`; the next script still runs | — | — |
| 8 `document.write` | PASS (order correct, written script runs) | — | — |

A header or footer instance costs about 0.3 ms of JS plus about 3 ms to create the realm.

### Microbenchmarks (`pages/7-bench.html`)

| Benchmark | Time |
|---|---|
| Local loop, 10 M iterations | 481 ms |
| Global-variable loop, 1 M iterations | 240 ms |
| 1 M function calls | 141 ms |
| 1 M array pushes plus a sum | 314 ms |
| 100k string join | 50 ms |
| 10k `createElement` + `appendChild` | 11 ms |
| 100k `getAttribute` | 20 ms |
| `firstChild`/`nextSibling` walk, 10k children × 10 | **350 ms** (O(n²): no sibling links) |

Boa is roughly 3–5× slower than QuickJS on loops: fine for jQuery, chart libraries and header
scripts.

## jQuery 3.7.1 compatibility

Missing real APIs (needed in P4): `document.implementation.createHTMLDocument` (shimmed; jQuery
fails to load without it), `getRootNode`, `namespaceURI`, `getElementsByName`, form state
(`checked`, `value`, `defaultValue`, `selected`), `XMLHttpRequest`.
Harmless probes also absent in browsers: `msMatchesSelector`, `node.window`, queries on Text nodes,
`onDOMContentLoaded`, `define`/`module`, `window.opener`.
Implemented beyond the brief: `compareDocumentPosition`, `contains`, `matches`, `cloneNode`,
`createDocumentFragment`, `documentElement`, `removeAttribute`, `nodeName`/`nodeType`,
`defaultView`, `getComputedStyle`, `readyState`, `addEventListener` on document/window,
`offsetWidth`/`offsetHeight`.

## Canvas 2D needed by Chart.js (recorded, not implemented)

Context lifecycle (`getContext('2d')`, `ctx.canvas`); state (`save`/`restore`, `setTransform`,
`resetTransform`, `translate`, `rotate`); paths (`beginPath`, `moveTo`, `lineTo`, `rect`, `clip`,
`fill`, `stroke`, `fillRect`, `clearRect`); styles (`fillStyle`, `strokeStyle`, `lineWidth`,
`lineCap`, `lineJoin`, `setLineDash`, `lineDashOffset`); text (`font`, `textAlign`, `textBaseline`,
`fillText`, `measureText`); canvas `width`/`height` reflection, `devicePixelRatio`, probes for
`Path2D`/`OffscreenCanvas`. Other charts add arcs, Béziers, gradients, `drawImage`.
Recommendation: record into our display list (kurbo paths, parley text metrics) and paint as vector
into krilla; rasterise only on `getImageData`/`toDataURL`.

## Boa findings

- Enable `annex-b` (not a default feature).
- Binary size (baseline 13.84 MB): `default-features=false, ["annex-b"]` → 23.79 MB (+9.9 MB,
  chosen); default features → 24.80 MB; `annex-b` + `intl_bundled` → 39.83 MB. QtWebKit 2.2 had no
  `Intl`/`Temporal`, so leaving both out keeps parity.
- `Context::stack_trace()` + `CallFrame::position()` give `Warning: <sourceURL>:<line> <msg>` for
  `--debug-javascript` (inline script lines are script-relative: blitz-html keeps no positions).
- `ContextBuilder::clock()` puts `Date.now()` on the virtual clock.
- `--stop-slow-scripts`: map to `RuntimeLimits::set_loop_iteration_limit` (deterministic, raises a
  catchable `RuntimeLimitError`); there is no wall-clock interrupt hook.
- `#[boa_class]` exists in 0.22 but has no inheritance and borrows `self` across re-entrant calls:
  unsuitable for the DOM.

## blitz-dom fork list (adds to P2's fork)

1. Script-pausing parser driver (`NextParserState::Suspend`), script start lines, `document.write`
   fed back into the input. Today a head script sees body elements.
2. HTML-spec serializer (`outer_html` is XML-style, `<tbody />`).
3. DocumentFragment node kind.
4. Sibling links (`nextSibling` is O(n)).
5. Element-level `matches` (expose stylo's `matches_selector_list`).
6. Inline style sync (`set_style_property` does not update the `style` attribute).
7. JS-visible removal must detach, not drop.
8. One arena per document: no `createHTMLDocument`/`DOMParser`/iframes without a shared arena.

## Recommended binding architecture

- Hand-written bindings behind a declarative `macro_rules!` layer (`FromJs`/`IntoJs`, `this`
  checks, DOMException mapping) plus a data table for reflected HTML attributes. ~25 interfaces and
  300–400 members: WebIDL codegen does not pay off.
- Wrapper `NodeW { id: NodeId }` (`JsData`) built with `from_proto_and_data`; one prototype per
  interface chained EventTarget → Node → Element → HTMLElement → HTMLxxxElement; constructors
  exposed for `instanceof`; add `Symbol.toStringTag`.
- `NodeId → JsObject` cache for stable identity (jQuery expandos), strong handles for the whole
  (one-shot) conversion; later `WeakGc` + `Finalize`.
- Thread-local `RefCell<Host>`; never hold the borrow while calling back into JS.
- Native: tree ops, attributes, queries, `innerHTML`, style, layout. JS prelude: event dispatch,
  DOMTokenList, `style` Proxy, `location`, stubs (~3 ms per realm).

## Integration with blitz mutation, restyle and relayout

- Every JS mutation goes through a short-lived `doc.mutate()`; sets `layout_dirty`.
- Layout getters call `doc.resolve(0.0)` lazily when dirty (jQuery forced 6 incremental layouts,
  0.7–9 ms in total).
- End of the wait phase: final `resolve`, then the P2 paginator. JS never runs during pagination.
- One realm per header/footer instance (~3.5 ms): a 100-page document with header and footer adds
  ~0.7 s.

## P4 estimate (person-weeks)

| Item | Estimate |
|---|---|
| Binding infrastructure | 2–3 |
| Core DOM (Node, Element, Document, CharacterData, fragments, live collections, reflected attributes, form state) | 4–6 |
| CSSOM (`style`, `getComputedStyle`, `classList`, `dataset`) | 1–2 |
| Parser driver with script pausing, script ordering, `document.write`, source lines, lifecycle, inline `on*` | 3–5 |
| wkhtmltopdf semantics (virtual clock, `window.status`, `--run-script`, `print()`, `--debug-javascript`, dialogs, slow-script budget) | 1–2 |
| Canvas 2D (recorder → display list → krilla, text via parley) | 4–6 |
| XHR/fetch over the loader, `Image`, storage stubs | 1–2 |
| blitz fork items 2–7 | 1–2 |
| Compat corpus (jQuery, Chart.js 2/4, Highcharts, moment, d3) and harness | 2–3 |
| **Total** | **19–31** |

Main risks: script-pausing parser semantics and canvas text metrics. Engine speed is not a risk.

## Reproduce

```sh
export PATH=/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH
cargo build --release   # ~4 min cold, ~1.3 GB target/
./run.sh
./target/release/jsdom-spike pages/2-jquery.html --audit --out /dev/null
./target/release/jsdom-spike pages/5b-chart2.html --canvas-recorder --out /dev/null
```
