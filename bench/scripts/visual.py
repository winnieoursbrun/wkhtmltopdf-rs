#!/usr/bin/env python3
"""Rendering parity between the original wkhtmltopdf and the Rust port.

    visual.py <results-dir>

Pairs compared (A = original, B = rust):
  bench-<id>        <id>.orig.out      vs <id>.rust.out        (scenarios.tsv, PDF outputs only)
  pdfkit-<case>     pdfkit/original/*  vs pdfkit/rust/*
  selftest-*        visual-selftest/*  (original vs original with another option; validates
                                        that the pipeline detects real differences)

Per pair: page count, page sizes (pdfinfo); per page at the configured dpi: % of pixels
differing (ImageMagick `compare -metric AE -fuzz`), SSIM (numpy, uniform window: ImageMagick 6
has no SSIM metric), red-highlight diff and a side-by-side composite (montage); text similarity
(`pdftotext -layout`, whitespace-normalised, word-level) with the first differing lines; mean
offset of matching words (`pdftotext -bbox-layout`) in points. Writes visual/<case>/metrics.json
and visual/summary.json. A missing B side yields status "n/a", never an error.
"""
import concurrent.futures as cf
import difflib
import html
import json
import math
import pathlib
import re
import subprocess
import sys

import numpy as np
from PIL import Image

D = pathlib.Path(sys.argv[1])
CFG = json.loads((pathlib.Path(__file__).resolve().parent.parent / "visual-thresholds.json").read_text())
VIS = D / "visual"
SKIP_SCENARIOS = {"batch20"}  # same input and options as `medium`


def run(*cmd, check=True):
    p = subprocess.run(cmd, capture_output=True, text=True)
    if check and p.returncode not in (0, 1):  # compare exits 1 when images differ
        raise RuntimeError(f"{cmd[0]} failed ({p.returncode}): {p.stderr.strip()[:300]}")
    return p


def is_pdf(p):
    return p.exists() and p.stat().st_size > 4 and p.read_bytes()[:4] == b"%PDF"


def page_sizes(pdf):
    out = run("pdfinfo", "-f", "1", "-l", "100000", str(pdf)).stdout
    return [(float(w), float(h)) for w, h in re.findall(r"^Page\s+\d+ size:\s+([\d.]+) x ([\d.]+)", out, re.M)]


def rasterise(pdf, page, out_png):
    prefix = str(out_png)[:-4]
    run("pdftoppm", "-r", str(CFG["dpi"]), "-f", str(page), "-l", str(page), "-singlefile", "-png", str(pdf), prefix)


def ssim(a, b, win):
    """Mean SSIM of two grayscale float arrays, uniform win x win window (as skimage's default)."""
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2

    def box(x):
        s = np.pad(x, ((1, 0), (1, 0))).cumsum(0).cumsum(1)
        return (s[win:, win:] - s[:-win, win:] - s[win:, :-win] + s[:-win, :-win]) / (win * win)

    mu_a, mu_b = box(a), box(b)
    n = win * win
    cov = n / (n - 1)
    va = (box(a * a) - mu_a * mu_a) * cov
    vb = (box(b * b) - mu_b * mu_b) * cov
    vab = (box(a * b) - mu_a * mu_b) * cov
    s = ((2 * mu_a * mu_b + c1) * (2 * vab + c2)) / ((mu_a ** 2 + mu_b ** 2 + c1) * (va + vb + c2))
    return float(s.mean())


def compare_page(case_dir, pdf_a, pdf_b, page, labels):
    pa, pb = case_dir / f"page-{page}-original.png", case_dir / f"page-{page}-rust.png"
    pd, ps = case_dir / f"page-{page}-diff.png", case_dir / f"page-{page}-side.png"
    rasterise(pdf_a, page, pa)
    rasterise(pdf_b, page, pb)
    ia, ib = Image.open(pa).convert("RGB"), Image.open(pb).convert("RGB")
    resized = ib.size != ia.size
    if resized:  # force the same pixel size so the metrics stay defined
        ib = ib.resize(ia.size, Image.LANCZOS)
        ib.save(pb)
    ae = run("compare", "-metric", "AE", "-fuzz", f"{CFG['fuzz_percent']}%", str(pa), str(pb), str(pd)).stderr
    m = re.match(r"\s*([\d.e+]+)", ae)
    diff_px = float(m.group(1)) if m else float("nan")
    total = ia.size[0] * ia.size[1]
    ga = np.asarray(ia.convert("L"), dtype=np.float64)
    gb = np.asarray(ib.convert("L"), dtype=np.float64)
    s = ssim(ga, gb, CFG["ssim_window"])
    run("montage", "-font", "DejaVu-Sans", "-pointsize", "14",
        "-label", labels[0], str(pa), "-label", labels[1], str(pb), "-label", "diff (red = differs)", str(pd),
        "-tile", "3x1", "-geometry", "+6+6", "-background", "#dddddd", str(ps))
    return {"page": page, "ssim": round(s, 5), "pixel_diff_percent": round(100 * diff_px / total, 3),
            "resized_rust_raster": resized, "side_by_side": ps.name}


def norm_text(pdf):
    out = run("pdftotext", "-layout", "-enc", "UTF-8", str(pdf), "-").stdout
    pages = out.split("\f")
    return [[" ".join(l.split()) for l in p.splitlines() if l.strip()] for p in pages]


def text_metrics(pdf_a, pdf_b):
    ta, tb = norm_text(pdf_a), norm_text(pdf_b)
    la = [l for p in ta for l in p]
    lb = [l for p in tb for l in p]
    wa, wb = " ".join(la).split(), " ".join(lb).split()
    sim = difflib.SequenceMatcher(None, wa, wb, autojunk=False).ratio() if (wa or wb) else 1.0
    diff = [l for l in difflib.unified_diff(la, lb, "original", "rust", n=0, lineterm="")
            if l[:1] in "+-" and not l.startswith(("+++", "---"))]
    return round(sim, 5), diff[:6]


WORD_RE = re.compile(r'<word xMin="([\d.]+)" yMin="([\d.]+)" xMax="[\d.]+" yMax="[\d.]+">(.*?)</word>')


def bbox_words(pdf):
    out = run("pdftotext", "-bbox-layout", str(pdf), "-").stdout
    return [[(html.unescape(t), float(x), float(y)) for x, y, t in WORD_RE.findall(p)]
            for p in out.split("<page ")[1:]]


def word_offset(pdf_a, pdf_b):
    pa, pb = bbox_words(pdf_a), bbox_words(pdf_b)
    offs, dxs, dys = [], [], []
    for wa, wb in zip(pa, pb):
        sm = difflib.SequenceMatcher(None, [w[0] for w in wa], [w[0] for w in wb], autojunk=False)
        for blk in sm.get_matching_blocks():
            for k in range(blk.size):
                (_, xa, ya), (_, xb, yb) = wa[blk.a + k], wb[blk.b + k]
                dxs.append(xb - xa)
                dys.append(yb - ya)
                offs.append(math.hypot(xb - xa, yb - ya))
    if not offs:
        return None, None, None, 0
    return (round(sum(offs) / len(offs), 3), round(sum(dxs) / len(dxs), 3),
            round(sum(dys) / len(dys), 3), len(offs))


def grade(value, key):
    t = CFG[key]
    if value is None or (isinstance(value, float) and math.isnan(value)):
        return "WARN"
    higher_better = t["pass"] > t["warn"]
    if higher_better:
        return "PASS" if value >= t["pass"] else "WARN" if value >= t["warn"] else "FAIL"
    return "PASS" if value <= t["pass"] else "WARN" if value <= t["warn"] else "FAIL"


def worst(statuses):
    for s in ("FAIL", "WARN", "PASS"):
        if s in statuses:
            return s
    return "PASS"


def compare_pair(name, group, pdf_a, pdf_b, labels=("original", "rust")):
    case_dir = VIS / name
    case_dir.mkdir(parents=True, exist_ok=True)
    rec = {"case": name, "group": group, "a": str(pdf_a.relative_to(D)), "labels": list(labels),
           "b": str(pdf_b.relative_to(D)) if pdf_b else None}
    if not is_pdf(pdf_a):
        return None
    if pdf_b is None or not is_pdf(pdf_b):
        rec.update(status="n/a", reason="no PDF from the rust binary")
        (case_dir / "metrics.json").write_text(json.dumps(rec, indent=2))
        return rec

    sa, sb = page_sizes(pdf_a), page_sizes(pdf_b)
    tol = CFG["page_size_tolerance_pt"]
    size_match = len(sa) == len(sb) and all(abs(x[0] - y[0]) <= tol and abs(x[1] - y[1]) <= tol for x, y in zip(sa, sb))
    n = min(len(sa), len(sb))
    with cf.ThreadPoolExecutor(max_workers=4) as ex:
        pages = list(ex.map(lambda p: compare_page(case_dir, pdf_a, pdf_b, p, labels), range(1, n + 1)))
    text_sim, first_diff = text_metrics(pdf_a, pdf_b)
    off, dx, dy, nwords = word_offset(pdf_a, pdf_b)
    ssims = [p["ssim"] for p in pages] or [float("nan")]
    worst_page = min(pages, key=lambda p: p["ssim"]) if pages else None
    pix = sum(p["pixel_diff_percent"] for p in pages) / len(pages) if pages else float("nan")
    rec.update(
        pages_a=len(sa), pages_b=len(sb), page_size_a=sa[0] if sa else None, page_size_b=sb[0] if sb else None,
        page_size_match=size_match, text_similarity=text_sim, first_text_diff=first_diff,
        ssim_mean=round(sum(ssims) / len(ssims), 5), ssim_worst=min(ssims),
        worst_page=worst_page["page"] if worst_page else None,
        side_by_side=f"visual/{name}/{worst_page['side_by_side']}" if worst_page else None,
        pixel_diff_percent=round(pix, 3), word_offset_pt=off, word_dx_pt=dx, word_dy_pt=dy,
        matched_words=nwords, per_page=pages,
    )
    grades = {k: grade(rec[k], k) for k in ("ssim_mean", "ssim_worst", "text_similarity", "pixel_diff_percent")}
    grades["word_offset_pt"] = grade(off, "word_offset_pt") if off is not None else "WARN"
    if len(sa) != len(sb):
        grades["page_count"] = "FAIL"
    if not size_match:
        grades["page_size"] = "FAIL"
    rec.update(grades=grades, status=worst(grades.values()))
    (case_dir / "metrics.json").write_text(json.dumps(rec, indent=2))
    return rec


def main():
    VIS.mkdir(exist_ok=True)
    pairs = []
    tsv = D / "scenarios.tsv"
    if tsv.exists():
        for line in tsv.read_text().splitlines():
            sid = line.split("\t")[0]
            if sid not in SKIP_SCENARIOS:
                pairs.append((f"bench-{sid}", "bench", D / f"{sid}.orig.out", D / f"{sid}.rust.out", None))
    po = D / "pdfkit" / "original"
    if po.exists():
        for pdf in sorted(po.glob("*.pdf")):
            pairs.append((f"pdfkit-{pdf.stem}", "pdfkit", pdf, D / "pdfkit" / "rust" / pdf.name, None))
    st = D / "visual-selftest"
    if st.exists():
        manifest = json.loads((st / "manifest.json").read_text())
        for m in manifest:
            pairs.append((m["case"], "selftest", st / m["a"], st / m["b"], (m["label_a"], m["label_b"])))

    summary = []
    for name, group, a, b, labels in pairs:
        rec = compare_pair(name, group, a, b, labels or ("original", "rust"))
        if rec:
            summary.append({k: v for k, v in rec.items() if k != "per_page"})
            extra = (f"ssim {rec['ssim_mean']} / text {rec['text_similarity']} / px {rec['pixel_diff_percent']}%"
                     if rec["status"] != "n/a" else rec["reason"])
            print(f"   {name:32s} {rec['status']:5s} {extra}")
    (VIS / "summary.json").write_text(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
