#!/usr/bin/env python3
"""Build REPORT.md from the raw files written by inside.sh, pdfkit/generate.rb and visual.py.

    report.py <results-dir>

Every section is optional: it is rendered only when its raw data exists.
"""
import json
import pathlib
import re
import sys

D = pathlib.Path(sys.argv[1])
BINS = ("orig", "rust")
CFG = json.loads((pathlib.Path(__file__).resolve().parent.parent / "visual-thresholds.json").read_text())


def read_kv(path):
    out = {}
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            if "=" in line:
                k, v = line.split("=", 1)
                out[k] = v
    return out


def fmt_t(s):
    return f"{s * 1000:.1f} ms" if s < 1 else f"{s:.3f} s"


def fmt_bytes(n):
    n = int(n)
    if n < 1024:
        return f"{n} B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f} KiB"
    return f"{n / 1048576:.2f} MiB"


env = read_kv(D / "env.txt")
tsv = D / "scenarios.tsv"
scenarios = [l.split("\t") for l in tsv.read_text().splitlines() if l.strip()] if tsv.exists() else []
out = []

out.append(f"""# Benchmark: wkhtmltopdf 0.12.6 vs wkhtmltopdf-rs

Run `{D.name}` — {env.get('mode', '?')} — phases: {env.get('phases', '?')}.

## Environment

| | |
|---|---|
| Date (UTC) | {env.get('date_utc', '?')} |
| Container | `wkbench:local`, {env.get('uname', '?')} |
| CPUs / memory | {env.get('cpus', '?')} vCPU ({env.get('cpu_model', '').strip() or 'n/a'}) / {env.get('mem_total', '?')} |
| Original | `{env.get('orig_version', '?')}` (sha256 {env.get('orig_sha256', '?')}…) |
| Rust port | `{env.get('rust_version', '?')}` (sha256 {env.get('rust_sha256', '?')}…, {fmt_bytes(env.get('rust_size', 0))} static musl) |
| Tools | {env.get('hyperfine', '?')}, GNU time, poppler (pdfinfo/pdftoppm/pdftotext), ImageMagick {env.get('imagemagick', '?')}, pdfkit {env.get('pdfkit', '?')} |
| Fonts | {env.get('fonts', '?')} |
""")

# ---------------------------------------------------------------------------- timing
if "timing" in env.get("phases", "") and scenarios:
    def hyperfine(sid, b):
        p = D / f"{sid}.{b}.hyperfine.json"
        if not p.exists():
            return None
        r = json.loads(p.read_text())["results"][0]
        return r["mean"], r.get("stddev") or 0.0, len(r.get("times", []))

    def gnu_time(sid, b):
        p = D / f"{sid}.{b}.time.txt"
        if not p.exists():
            return None
        t = p.read_text()

        def num(label):
            m = re.search(re.escape(label) + r":\s*([\d.]+)", t)
            return float(m.group(1)) if m else None

        return {"rss_kb": num("Maximum resident set size (kbytes)"),
                "user": num("User time (seconds)"), "sys": num("System time (seconds)")}

    rows, notes, details, stderr_seen = [], [], [], {}
    for sid, label, repeat in scenarios:
        meta = {b: read_kv(D / f"{sid}.{b}.meta") for b in BINS}
        hf = {b: hyperfine(sid, b) for b in BINS}
        gt = {b: gnu_time(sid, b) for b in BINS}
        ok = {b: meta[b].get("rc") == "0" for b in BINS}

        def timing(b):
            if hf[b] is None:
                return f"n/a (exit {meta[b].get('rc', '?')})" if not ok[b] else "n/a (hyperfine failed)"
            return f"{fmt_t(hf[b][0])} ± {fmt_t(hf[b][1])}"

        if hf["orig"] and hf["rust"]:
            r = hf["rust"][0] / hf["orig"][0]
            ratio = f"{r:.2f}× ({1 / r:.1f}× faster)" if r < 1 else f"{r:.2f}× (slower)"
        else:
            ratio = "—"

        def rss(b):
            g = gt[b]
            return f"{g['rss_kb'] / 1024:.1f} MiB" if g and g["rss_kb"] is not None else "n/a"

        def cpu(b):
            g = gt[b]
            return f"{g['user']:.2f}+{g['sys']:.2f}" if g and g["user"] is not None else "n/a"

        def size(b):
            return fmt_bytes(meta[b]["size"]) if ok[b] else "n/a"

        def pages(b):
            return meta[b].get("pages", "-") if ok[b] else "n/a"

        rows.append(f"| {label} | {timing('orig')} | {timing('rust')} | {ratio} | {rss('orig')} / {rss('rust')} "
                    f"| {cpu('orig')} / {cpu('rust')} | {size('orig')} / {size('rust')} | {pages('orig')} / {pages('rust')} |")
        if int(repeat) > 1:
            for b in BINS:
                if hf[b]:
                    notes.append(f"- `{sid}` ({b}): {int(repeat) / hf[b][0]:.2f} conversions/s "
                                 f"({fmt_t(hf[b][0] / int(repeat))} per conversion).")
        for b in BINS:
            err = D / f"{sid}.{b}.stderr"
            text = err.read_text(errors="replace").strip() if err.exists() else ""
            if text:
                lines = text.splitlines()
                key = lines[0][:160] + (f" (+{len(lines) - 1} more lines)" if len(lines) > 1 else "")
                stderr_seen.setdefault(key, []).append(f"{sid} ({b})")
            runs = hf[b][2] if hf[b] else 0
            details.append(f"| `{sid}` | {b} | {meta[b].get('rc', '?')} | {runs} | `{meta[b].get('cmd', '')}` |")
    for msg, where in stderr_seen.items():
        notes.append(f"- stderr `{msg}` — {', '.join(where)}")

    out.append(f"""## Performance

Iterations: {env.get('mode', '?')}. Times are hyperfine wall-clock mean ± σ. Ratio = rust / original
(below 1 means the port is faster). Peak RSS and CPU (user+sys seconds) come from one
`/usr/bin/time -v` run each. Batch rows cover 20 conversions per measurement.

**Read wall times with care**: the original applies its default `--javascript-delay` of 200 ms
(pdfkit does not override it) to every loaded page, header/footer pages included. The pdfkit
rows are therefore dominated by sleeping (compare CPU time, and the `--javascript-delay 0`
rows). The port must keep that delay to stay behaviour-compatible, so the same floor will
apply to it.

| Scenario | Original mean±σ | Rust mean±σ | Ratio | Peak RSS orig / rust | CPU s orig / rust | Output size orig / rust | Pages orig / rust |
|---|---|---|---|---|---|---|---|
{chr(10).join(rows)}

### Notes

{chr(10).join(notes) if notes else '- none'}

### Commands

| Scenario | Binary | Exit | Runs | Command (run in `/bench/corpus`) |
|---|---|---|---|---|
{chr(10).join(details)}
""")

# ---------------------------------------------------------------------------- pdfkit
pk = {}
for label in ("original", "rust"):
    p = D / "pdfkit" / label / "results.json"
    if p.exists():
        pk[label] = {r["case"]: r for r in json.loads(p.read_text())}
if pk:
    def outcome(r):
        if r is None:
            return "not run"
        s = "ok" if r["outcome"] == "ok" else f"`{r['outcome']}`"
        return s + ("" if r["as_expected"] else " ⚠ unexpected")

    rows = []
    cases = list((pk.get("original") or pk.get("rust")).keys())
    for c in cases:
        o, r = pk.get("original", {}).get(c), pk.get("rust", {}).get(c)
        same = "yes" if o and r and o["outcome"] == r["outcome"] else "**no**"
        pages = f"{(o or {}).get('pages') or '—'} / {(r or {}).get('pages') or '—'}"
        times = f"{fmt_t(o['duration_s']) if o else '—'} / {fmt_t(r['duration_s']) if r else '—'}"
        png = f"[png](pdfkit/original/{c}.png)" if (D / "pdfkit" / "original" / f"{c}.png").exists() else ""
        desc = (o or r)["description"]
        rows.append(f"| `{c}` | {desc} | {outcome(o)} | {outcome(r)} | {same} | {pages} | {times} | {png} |")
    n_same = sum(1 for c in cases if pk.get("original", {}).get(c, {}).get("outcome")
                 == pk.get("rust", {}).get(c, {}).get("outcome", "?"))
    out.append(f"""## pdfkit integration

pdfkit {env.get('pdfkit', '?')} driven by `bench/pdfkit/generate.rb`, `PDFKit.configure {{ |c| c.wkhtmltopdf = … }}`
per binary. "Outcome" is `ok` or the exception class raised by the gem; ⚠ marks an outcome other
than the expected one. Times include the gem (process spawn, stdin/stdout piping).
Same outcome: {n_same}/{len(cases)} cases. PDFs, page-1 PNGs (50 dpi) and the exact
command line of every case are in `pdfkit/<binary>/` (`results.json`).

| Case | What it exercises | Original | Rust | Same outcome? | Pages orig / rust | Time orig / rust | Page 1 |
|---|---|---|---|---|---|---|---|
{chr(10).join(rows)}
""")

# ---------------------------------------------------------------------------- visual
vs = D / "visual" / "summary.json"
if vs.exists():
    summary = json.loads(vs.read_text())

    def row(r):
        if r["status"] == "n/a":
            return f"| `{r['case']}` | n/a — {r['reason']} | | | | | | | | |"
        size = "yes" if r["page_size_match"] else f"**no** ({r['page_size_a']} vs {r['page_size_b']})"
        off = "—" if r["word_offset_pt"] is None else f"{r['word_offset_pt']:.2f} (dx {r['word_dx_pt']:+.2f}, dy {r['word_dy_pt']:+.2f})"
        link = f"[p{r['worst_page']}]({r['side_by_side']})" if r.get("side_by_side") else ""
        return (f"| `{r['case']}` | **{r['status']}** | {r['pages_a']} / {r['pages_b']} | {size} | "
                f"{100 * r['text_similarity']:.2f} % | {r['ssim_mean']:.4f} | {r['ssim_worst']:.4f} | "
                f"{r['pixel_diff_percent']:.2f} % | {off} | {link} |")

    head = ("| Case | Status | Pages A / B | Page size match | Text similarity | Mean SSIM | Worst page SSIM "
            "| Pixels differing | Mean word offset (pt) | Side-by-side (worst page) |\n|---|---|---|---|---|---|---|---|---|---|")
    main_rows = [row(r) for r in summary if r["group"] != "selftest"]
    st_rows = [row(r) for r in summary if r["group"] == "selftest"]
    diffs = [f"- `{r['case']}`: " + " · ".join(f"`{l[:100]}`" for l in r["first_text_diff"][:4])
             for r in summary if r.get("first_text_diff")]
    t = CFG
    na = sum(1 for r in summary if r["group"] != "selftest" and r["status"] == "n/a")
    out.append(f"""## Rendering parity

The port renders with a different engine than QtWebKit, so pixel identity is not the goal.
Targets (DESIGN.md): **L2** — same structure and text (page count, page size, extracted text,
outline) — and **L3** — visually equivalent pages within a tolerance. Metrics, A = original,
B = rust, pages rasterised at {t['dpi']} dpi:

- *Text similarity*: word-level similarity of `pdftotext -layout`, whitespace-normalised (L2).
- *SSIM*: structural similarity per page, uniform {t['ssim_window']}×{t['ssim_window']} window, grayscale (L3).
- *Pixels differing*: `compare -metric AE -fuzz {t['fuzz_percent']}%`, mean over pages (L3, strict).
- *Mean word offset*: distance between the top-left corners of matching words
  (`pdftotext -bbox-layout`); dx/dy show the drift direction (margins, line heights).

Thresholds (`bench/visual-thresholds.json`), PASS / WARN, else FAIL: mean SSIM ≥ {t['ssim_mean']['pass']} / {t['ssim_mean']['warn']},
worst page SSIM ≥ {t['ssim_worst']['pass']} / {t['ssim_worst']['warn']}, text ≥ {100 * t['text_similarity']['pass']:.0f} % / {100 * t['text_similarity']['warn']:.0f} %,
pixels ≤ {t['pixel_diff_percent']['pass']} % / {t['pixel_diff_percent']['warn']} %, word offset ≤ {t['word_offset_pt']['pass']} / {t['word_offset_pt']['warn']} pt.
Page count or page size mismatch is FAIL. A case takes its worst metric.
Per-page images: `visual/<case>/page-N-{{original,rust,diff,side}}.png`.

### Original vs Rust

{na} of {len(main_rows)} cases are n/a (no PDF from the rust binary).

{head}
{chr(10).join(main_rows) if main_rows else '| none | | | | | | | | | |'}
""")
    if st_rows:
        out.append(f"""### Pipeline self-test (original vs original with another option)

Validates the comparison itself: `identical` must PASS, the variants must be flagged.

{head}
{chr(10).join(st_rows)}
""")
    if diffs:
        out.append("### First text differences\n\n" + "\n".join(diffs) + "\n")

(D / "REPORT.md").write_text("\n".join(out))
print(f"wrote {D / 'REPORT.md'}")
