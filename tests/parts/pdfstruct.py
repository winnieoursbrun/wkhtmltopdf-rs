#!/usr/bin/env python3
"""Extract structure facts from a PDF as JSON (P3 document-parts corpus).

Usage: pdfstruct.py <file.pdf> [--base <url prefix>]

Only depends on pypdf (pinned in make-refs.sh). The output is meant to be compared
field by field against the same extraction run on the Rust binary's output, so every
value is normalised: numbers rounded, dates dropped, lists in document order.

--base: the URL prefix of the case directory at generation time (for example
file:///work/links/). Every occurrence in link URIs and destination names is replaced by
the literal token "{BASE}", so the comparison does not depend on where the case ran.
"""
import json
import re
import sys

from pypdf import PdfReader
from pypdf.generic import ContentStream, IndirectObject, NameObject

COLOR_OPS = {"rg", "RG", "g", "G", "k", "K", "sc", "SC", "scn", "SCN", "cs", "CS"}


def r(x, nd=2):
    return round(float(x), nd)


def deref(o):
    return o.get_object() if isinstance(o, IndirectObject) else o


def base_sub(s, base):
    return s.replace(base, "{BASE}") if base and isinstance(s, str) else s


def page_number(reader, d):
    """1-based page of a pypdf Destination; Qt writes named dests with a bare page index."""
    pg = d.page
    if isinstance(pg, int):
        return pg + 1
    n = reader.get_destination_page_number(d)
    return None if n is None or n < 0 else n + 1


def dest_facts(reader, dest, base):
    """Resolve a destination (name, string or explicit array) to {name, page, y}."""
    out = {}
    dest = deref(dest)
    if isinstance(dest, (str, bytes)):
        name = dest.decode("latin-1") if isinstance(dest, bytes) else str(dest)
        out["dest_name"] = base_sub(name, base)
        nd = reader.named_destinations.get(name)
        if nd is None:
            out["dest_page"] = None
            return out
        out["dest_page"] = page_number(reader, nd)
        top = nd.get("/Top")
        out["dest_y"] = r(top, 1) if top is not None else None
        return out
    # Explicit array [page /XYZ left top zoom]
    arr = list(dest)
    page_ref = arr[0]
    for i, p in enumerate(reader.pages):
        if p.indirect_reference == page_ref:
            out["dest_page"] = i + 1
            break
    else:
        out["dest_page"] = None
    if len(arr) > 3 and arr[1] == "/XYZ" and arr[3] is not None:
        out["dest_y"] = r(arr[3], 1)
    return out


def links_of(reader, page, base):
    res = []
    for a in page.get("/Annots") or []:
        a = deref(a)
        if a.get("/Subtype") != "/Link":
            continue
        item = {"rect": [r(v, 1) for v in a["/Rect"]]}
        act = deref(a.get("/A"))
        if act is not None and act.get("/S") == "/URI":
            item["uri"] = base_sub(str(act["/URI"]), base)
        elif act is not None and act.get("/S") == "/GoTo":
            item.update(dest_facts(reader, act["/D"], base))
        elif "/Dest" in a:
            item.update(dest_facts(reader, a["/Dest"], base))
        else:
            item["other"] = str(act)
        res.append(item)
    return res


def outline_of(reader):
    flat = []

    def walk(items, level):
        for it in items:
            if isinstance(it, list):
                walk(it, level + 1)
                continue
            pg = page_number(reader, it)
            top = it.get("/Top")
            flat.append({"level": level, "title": str(it.title), "page": pg,
                         "y": r(top, 1) if top is not None else None})

    walk(reader.outline, 1)
    return flat


FIELD_FLAGS = {"readonly": 1, "multiline": 1 << 12, "password": 1 << 13}


def fields_of(reader):
    root = reader.trailer["/Root"]
    af = deref(root.get("/AcroForm"))
    if af is None:
        return None
    out = []
    for f in af.get("/Fields") or []:
        f = deref(f)
        ff = int(f.get("/Ff", 0))
        v = f.get("/V")
        page = None
        for i, p in enumerate(reader.pages):
            for a in p.get("/Annots") or []:
                if deref(a) is f:
                    page = i + 1
        rec = {
            "name": str(f.get("/T")),
            "type": str(f.get("/FT")),
            "value": str(v) if v is not None else None,
            "flags": ff,
            "page": page,
            "rect": [r(x, 1) for x in f["/Rect"]] if "/Rect" in f else None,
        }
        for k, bit in FIELD_FLAGS.items():
            if ff & bit:
                rec[k] = True
        if "/AS" in f:
            rec["appearance_state"] = str(f["/AS"])
        if "/MaxLen" in f:
            rec["maxlen"] = int(f["/MaxLen"])
        out.append(rec)
    return out


def colors_of(reader, page):
    """Count colour operators, and the 3-component colour settings that are not grey (r == g == b)."""
    counts = {}
    non_gray = 0
    contents = page.get_contents()
    if contents is None:
        return {"ops": counts, "non_gray_rgb": 0}
    cs = ContentStream(contents, reader)
    for operands, op in cs.operations:
        op = op.decode("latin-1") if isinstance(op, bytes) else str(op)
        if op in COLOR_OPS:
            counts[op] = counts.get(op, 0) + 1
            # Qt sets colours with "/CSp cs" (DeviceRGB) then "r g b scn".
            if op in ("rg", "RG", "scn", "SCN", "sc", "SC") and len(operands) == 3 \
                    and all(isinstance(x, (int, float)) or hasattr(x, "as_numeric") for x in operands):
                a, b, c = (r(x, 3) for x in operands)
                if not (a == b == c):
                    non_gray += 1
    return {"ops": dict(sorted(counts.items())), "non_gray_rgb": non_gray}


def fonts_of(page):
    names = set()
    res = deref(page.get("/Resources")) or {}
    fonts = deref(res.get("/Font")) or {}
    for f in fonts.values():
        bf = str(deref(f).get("/BaseFont", ""))
        names.add(re.sub(r"^/?[A-Z]{6}\+", "", bf).lstrip("/"))
    return sorted(names)


def images_of(reader, page):
    out = []
    res = deref(page.get("/Resources")) or {}
    xo = deref(res.get("/XObject")) or {}
    for x in xo.values():
        x = deref(x)
        if x.get("/Subtype") == "/Image":
            out.append({"w": int(x["/Width"]), "h": int(x["/Height"]),
                        "cs": str(deref(x.get("/ColorSpace")))})
    return out


def main():
    path = sys.argv[1]
    base = None
    if "--base" in sys.argv:
        base = sys.argv[sys.argv.index("--base") + 1]
    reader = PdfReader(path)
    root = reader.trailer["/Root"]
    info = reader.metadata or {}
    pages = []
    for i, p in enumerate(reader.pages):
        mb = p.mediabox
        pages.append({
            "n": i + 1,
            "size": [r(mb.width), r(mb.height)],
            "rotate": int(p.get("/Rotate", 0)),
            "links": links_of(reader, p, base),
            "fonts": fonts_of(p),
            "images": images_of(reader, p),
            "colors": colors_of(reader, p),
        })
    names = sorted(base_sub(str(k), base) for k in reader.named_destinations.keys())
    facts = {
        "pdf_version": reader.pdf_header.replace("%PDF-", ""),
        "page_count": len(reader.pages),
        "page_mode": str(root.get("/PageMode")) if "/PageMode" in root else None,
        "info": {k: str(info.get(k)) for k in ("/Title", "/Creator", "/Producer") if info.get(k) is not None},
        "outline": outline_of(reader),
        "named_dests": names,
        "acroform_fields": fields_of(reader),
        "pages": pages,
    }
    text = json.dumps(facts, indent=1, ensure_ascii=False)
    # Keep numeric arrays (rects, sizes) on one line.
    text = re.sub(r"\[\s*(-?[\d.]+(?:,\s*-?[\d.]+)*)\s*\]",
                  lambda m: "[" + ", ".join(x.strip() for x in m.group(1).split(",")) + "]", text)
    sys.stdout.write(text + "\n")


if __name__ == "__main__":
    main()
