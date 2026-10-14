#!/usr/bin/env python3
"""Per-page text similarity of out/<page>.{blitz,fulgur}.txt against out/<page>.ref.txt.

Words are compared after NFKC normalisation (folds the ﬁ/ﬂ ligatures wkhtmltopdf emits).
Prints a markdown table: pages, whole-document word similarity, and per-page similarity.
"""
import difflib, os, re, sys, unicodedata

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")


def pages(path):
    if not os.path.exists(path):
        return None
    txt = open(path, encoding="utf-8", errors="replace").read()
    parts = re.split(r"=== page \d+ ===\n", txt)[1:]
    return [unicodedata.normalize("NFKC", p).split() for p in parts]


def ratio(a, b):
    if not a and not b:
        return 1.0
    return difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()


names = sys.argv[1:] or ["a", "b", "invoice", "report", "fonts", "layout", "statement"]
print("| page | ref pages | variant | pages | doc text sim | per-page text sim |")
print("|---|---|---|---|---|---|")
for n in names:
    ref = pages(os.path.join(OUT, f"{n}.ref.txt"))
    for v in ("blitz", "fulgur"):
        got = pages(os.path.join(OUT, f"{n}.{v}.txt"))
        if got is None or ref is None:
            print(f"| {n} | {len(ref or [])} | {v} | n/a | n/a | n/a |")
            continue
        doc = ratio(sum(ref, []), sum(got, []))
        per = [ratio(ref[i] if i < len(ref) else [], got[i] if i < len(got) else []) for i in range(max(len(ref), len(got)))]
        print(f"| {n} | {len(ref)} | {v} | {len(got)} | {doc:.2f} | {' '.join(f'{x:.2f}' for x in per)} |")
