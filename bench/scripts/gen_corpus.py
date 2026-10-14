#!/usr/bin/env python3
"""Generate the deterministic part of the benchmark corpus.

Writes into bench/corpus/ (or the directory given as argv[1]):
  medium.html          ~14 KB article (4 Letter pages): headings, paragraphs, list, table
  large.html           invoice/report with ~1,450 table rows (30 Letter pages)
  hf-body.html         ~8-page document with h1/h2 used for header/footer + toc + outline
  part1..3.html        the three pages of the multi-object scenario
  js.html              js.template.html with jQuery inlined (stdin input has no base URL)

Same input, same bytes: a fixed LCG drives every "random" choice, no clock, no locale.
"""
import html
import pathlib
import sys

OUT = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else pathlib.Path(__file__).resolve().parent.parent / "corpus"

WORDS = (
    "account analysis balance budget capacity carrier client contract cost customer delivery "
    "demand dispatch estimate forecast freight growth invoice inventory ledger margin market "
    "operation order payment period pipeline planning price process product quarter rate "
    "region report revenue route schedule service shipment supplier target tax team volume "
    "warehouse network quality review audit cycle lead metric baseline variance trend"
).split()


class Lcg:
    def __init__(self, seed):
        self.s = seed

    def next(self):
        self.s = (self.s * 1103515245 + 12345) % 2**31
        return self.s

    def pick(self, seq):
        return seq[self.next() % len(seq)]

    def between(self, lo, hi):
        return lo + self.next() % (hi - lo + 1)


def sentence(r, n=None):
    n = n or r.between(8, 18)
    words = [r.pick(WORDS) for _ in range(n)]
    return words[0].capitalize() + " " + " ".join(words[1:]) + "."


def paragraph(r, n=None):
    return " ".join(sentence(r) for _ in range(n or r.between(3, 6)))


BASE_CSS = """
body { font: 11pt "DejaVu Serif", serif; line-height: 1.4; color: #222; }
h1, h2, h3 { font-family: "DejaVu Sans", sans-serif; color: #1f3a5f; }
table { border-collapse: collapse; width: 100%; font: 9pt "DejaVu Sans", sans-serif; }
th, td { border: 1px solid #bbb; padding: 3px 5px; }
th { background: #e8eef6; }
td.num { text-align: right; font-family: "DejaVu Sans Mono", monospace; }
tr:nth-child(even) td { background: #f7f7f7; }
"""


def page(title, body, extra_css=""):
    return (
        "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">"
        f"<title>{html.escape(title)}</title>\n<style>{BASE_CSS}{extra_css}</style>\n</head>\n"
        f"<body>\n{body}\n</body></html>\n"
    )


def medium():
    r = Lcg(1)
    parts = ["<h1>Quarterly Operations Review</h1>", f"<p><em>{sentence(r)}</em></p>"]
    for s in range(1, 9):
        parts.append(f"<h2>{s}. {sentence(r, 4)[:-1]}</h2>")
        for _ in range(3):
            parts.append(f"<p>{paragraph(r)}</p>")
        if s == 2:
            parts.append("<ul>" + "".join(f"<li>{sentence(r)}</li>" for _ in range(6)) + "</ul>")
        if s == 3:
            rows = "".join(
                f"<tr><td>{r.pick(WORDS).capitalize()}</td><td class=\"num\">{r.between(100, 9999)}</td>"
                f"<td class=\"num\">{r.between(1, 99)}.{r.between(10, 99)}%</td></tr>"
                for _ in range(15)
            )
            parts.append(f"<table><tr><th>Metric</th><th>Value</th><th>Change</th></tr>{rows}</table>")
    return page("Quarterly Operations Review", "\n".join(parts))


def large():
    r = Lcg(2)
    rows = []
    total = 0
    for i in range(1, 1451):
        qty = r.between(1, 40)
        unit = r.between(150, 99999) / 100
        amount = qty * unit
        total += amount
        rows.append(
            f"<tr><td class=\"num\">{i:05d}</td><td>SKU-{r.between(10000, 99999)}</td>"
            f"<td>{r.pick(WORDS).capitalize()} {r.pick(WORDS)} {r.pick(WORDS)}</td>"
            f"<td class=\"num\">{qty}</td><td class=\"num\">{unit:,.2f}</td><td class=\"num\">{amount:,.2f}</td></tr>"
        )
    head = "<thead><tr><th>#</th><th>SKU</th><th>Description</th><th>Qty</th><th>Unit</th><th>Amount</th></tr></thead>"
    body = (
        "<h1>Invoice INV-2025-00042</h1>"
        "<table style=\"width:60%\"><tr><th>Customer</th><td>ACME Analytics</td></tr>"
        "<tr><th>Period</th><td>2025-01-01 to 2025-12-31</td></tr></table>"
        f"<p>{paragraph(r)}</p>"
        f"<table>{head}<tbody>{''.join(rows)}</tbody></table>"
        f"<h2>Total due: {total:,.2f}</h2><p>{paragraph(r)}</p>"
    )
    return page("Invoice INV-2025-00042", body, "thead { display: table-header-group; } tr { page-break-inside: avoid; }")


def chaptered(title, seed, chapters, sections, paras):
    r = Lcg(seed)
    parts = [f"<h1>{html.escape(title)}</h1>", f"<p>{paragraph(r)}</p>"]
    for c in range(1, chapters + 1):
        parts.append(f"<h1>Chapter {c}: {sentence(r, 3)[:-1]}</h1>")
        for s in range(1, sections + 1):
            parts.append(f"<h2>{c}.{s} {sentence(r, 4)[:-1]}</h2>")
            parts.extend(f"<p>{paragraph(r)}</p>" for _ in range(paras))
    return page(title, "\n".join(parts))


def js_page():
    jq = (OUT / "vendor" / "jquery-1.12.4.min.js").read_text(encoding="utf-8")
    tpl = (OUT / "js.template.html").read_text(encoding="utf-8")
    assert "/*@JQUERY@*/" in tpl
    # The minified source contains no "</script>" sequence; guard anyway.
    assert "</script" not in jq
    return tpl.replace("/*@JQUERY@*/", jq)


def main():
    files = {
        "medium.html": medium(),
        "large.html": large(),
        "hf-body.html": chaptered("Operations Handbook", 3, 6, 3, 3),
        "part1.html": chaptered("Part I — Finance", 11, 2, 2, 2),
        "part2.html": chaptered("Part II — Logistics", 12, 2, 2, 2),
        "part3.html": chaptered("Part III — Outlook", 13, 2, 2, 2),
        "js.html": js_page(),
    }
    for name, content in files.items():
        (OUT / name).write_text(content, encoding="utf-8")
        print(f"  corpus/{name}: {len(content.encode('utf-8')):,} bytes")


if __name__ == "__main__":
    main()
