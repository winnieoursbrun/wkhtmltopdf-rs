"""Compare two directories of PDFs written by wkhtmltopdf-rs after decompressing their streams.

    python3 -I bench/scripts/pdf_equiv.py <dirA> <dirB>

Used by `bench/pdf-identity.sh equiv` when a change alters compressed bytes only (another
deflate implementation or setting): every PDF must have the same objects, in the same order,
with the same dictionaries and the same *decompressed* stream data. What legitimately follows
from different compressed sizes is masked: stream /Length values, the cross-reference tables,
`startxref` and `/Prev` offsets. The random spool-file name and the trailer /ID are masked as in
pdf-identity.sh. Non-PDF files (outline XML dumps) are compared byte for byte.

Also prints the total size of each side, so the size cost of the change is visible.
Exit status 1 on any difference.
"""

import os
import re
import sys
import zlib

OBJ = re.compile(rb'(\d+) (\d+) obj\s')
LENGTH = re.compile(rb'/Length (\d+)')
WKTEMP = re.compile(rb'wktemp-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}')
ID = re.compile(rb'/ID ?\[\(([A-Za-z0-9+/=]{24})\) ?\(([A-Za-z0-9+/=]{24})\)\]|/ID ?\[<[0-9A-Fa-f]+> ?<[0-9A-Fa-f]+>\]')
XREF = re.compile(rb'xref\r?\n(\d+ \d+\r?\n(\d{10} \d{5} [nf]\s*\r?\n)*)+')
STARTXREF = re.compile(rb'startxref\r?\n\d+')
PREV = re.compile(rb'/Prev \d+')


def mask(text):
    text = WKTEMP.sub(b'wktemp-*', text)
    text = ID.sub(b'/ID *', text)
    text = XREF.sub(b'xref *\n', text)
    text = STARTXREF.sub(b'startxref *', text)
    return PREV.sub(b'/Prev *', text)


def normalize(data):
    """Returns the list of parts of a PDF: masked text between objects, and per object its
    header, masked dictionary and decompressed stream data."""
    parts = []
    pos = 0
    while True:
        m = OBJ.search(data, pos)
        if not m:
            parts.append(('tail', mask(data[pos:])))
            return parts
        parts.append(('text', mask(data[pos:m.start()])))
        end_obj = data.find(b'endobj', m.end())
        start_stream = data.find(b'stream', m.end())
        if start_stream != -1 and (end_obj == -1 or start_stream < end_obj):
            head = data[m.end():start_stream]
            body_start = start_stream + len(b'stream')
            if data[body_start:body_start + 2] == b'\r\n':
                body_start += 2
            elif data[body_start:body_start + 1] == b'\n':
                body_start += 1
            length = int(LENGTH.search(head).group(1))
            body = data[body_start:body_start + length]
            if b'/FlateDecode' in head:
                body = zlib.decompress(body)
            end = data.find(b'endobj', body_start + length) + len(b'endobj')
            trailer = data[body_start + length:end]
            parts.append(('obj', m.group(0), mask(LENGTH.sub(b'/Length *', head)), body, trailer))
            pos = end
        else:
            end = end_obj + len(b'endobj')
            parts.append(('obj', m.group(0), mask(data[m.end():end])))
            pos = end


def compare(a, b):
    """Returns None when equivalent, else a description of the first difference."""
    if not a.startswith(b'%PDF'):
        return None if mask(a) == mask(b) else 'non-PDF content differs'
    pa, pb = normalize(a), normalize(b)
    if len(pa) != len(pb):
        return f'{len(pa)} vs {len(pb)} parts'
    for i, (x, y) in enumerate(zip(pa, pb)):
        if x != y:
            what = x[1][:40] if x[0] == 'obj' else x[0].encode()
            return f'part {i} ({what!r}) differs'
    return None


def main():
    da, db = sys.argv[1], sys.argv[2]
    names = sorted(set(os.listdir(da)) | set(os.listdir(db)))
    bad = 0
    size_a = size_b = 0
    for name in names:
        fa, fb = os.path.join(da, name), os.path.join(db, name)
        if not (os.path.exists(fa) and os.path.exists(fb)):
            print(f'MISSING on one side: {name}')
            bad += 1
            continue
        a, b = open(fa, 'rb').read(), open(fb, 'rb').read()
        size_a += len(a)
        size_b += len(b)
        err = compare(a, b)
        if err:
            print(f'DIFFERS: {name}: {err}')
            bad += 1
    delta = 100.0 * (size_b - size_a) / size_a if size_a else 0.0
    print(f'total size {size_a} -> {size_b} bytes ({delta:+.2f} %)')
    if bad:
        print(f'{bad} of {len(names)} files differ after decompression')
        sys.exit(1)
    print(f'OK: {len(names)} files equivalent after decompression')


if __name__ == '__main__':
    main()
