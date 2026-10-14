#!/bin/sh
# Render every corpus page with the reference wkhtmltopdf 0.12.6 (Docker image wkref:local),
# route (a) fulgur and route (b) blitz, then compare page count, page size and text per page,
# and build side-by-side PNGs (ref | blitz | fulgur) under out/.
#
# Usage: WK_BIN_DIR=<dir containing wkhtmltopdf-linux-amd64> ./compare.sh [page ...]
# Reference invocation = pdfkit default: stdin -> stdout, Letter, 0.75in margins, UTF-8.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=$HERE/out
# The corpus moved to tests/corpus (P2a); fonts are in crates/wk-fonts/fonts.
CORPUS=$HERE/../../tests/corpus
WK_BIN_DIR=${WK_BIN_DIR:?set WK_BIN_DIR to the directory holding wkhtmltopdf-linux-amd64}
SPIKE_B=$HERE/target/release/spike
SPIKE_A=$HERE/route-a-fulgur/target/release/spike
PDFKIT_ARGS="--quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8"
mkdir -p "$OUT"
PAGES=${*:-a b invoice report fonts layout statement}

for p in $PAGES; do
  # Reference (pdfkit default invocation, HTML on stdin, PDF on stdout).
  docker run --rm -i --platform linux/amd64 -v "$WK_BIN_DIR":/wk:ro --entrypoint /wk/wkhtmltopdf-linux-amd64 \
    wkref:local $PDFKIT_ARGS - - < "$CORPUS/$p.html" > "$OUT/$p.ref.pdf" || echo "ref failed: $p"
  # Same geometry for both spike routes, same stdin/stdout contract.
  ( cd "$CORPUS" && SPIKE_FONT_DIR="$HERE/../../crates/wk-fonts/fonts" /usr/bin/time -p "$SPIKE_B" --page-size Letter --margin-in 0.75 - - \
      < "$p.html" > "$OUT/$p.blitz.pdf" 2> "$OUT/$p.blitz.log" ) || echo "blitz failed: $p"
  ( cd "$CORPUS" && SPIKE_FONT_DIR="$HERE/../../crates/wk-fonts/fonts" /usr/bin/time -p "$SPIKE_A" --page-size Letter --margin-in 0.75 - - \
      < "$p.html" > "$OUT/$p.fulgur.pdf" 2> "$OUT/$p.fulgur.log" ) || echo "fulgur failed: $p"
done

# Structure + text + rasters, with poppler from the reference image.
docker run --rm --platform linux/amd64 -v "$OUT":/out --entrypoint sh wkref:local -c '
  cd /out
  for f in *.pdf; do
    b=${f%.pdf}
    if ! pdfinfo "$f" > "$b.info" 2>/dev/null; then echo "unreadable" > "$b.info"; continue; fi
    n=$(awk "/^Pages:/{print \$2}" "$b.info")
    i=1; : > "$b.txt"
    while [ "$i" -le "$n" ]; do
      echo "=== page $i ===" >> "$b.txt"; pdftotext -f $i -l $i -layout "$f" - >> "$b.txt" 2>/dev/null; i=$((i+1))
    done
    rm -f "$b"-*.png; pdftoppm -r 50 -png "$f" "$b"
  done'

# Side-by-side sheets: one row per page, columns ref | blitz | fulgur.
for p in $PAGES; do
  n=0
  for v in ref blitz fulgur; do
    c=$(ls "$OUT/$p.$v"-*.png 2>/dev/null | wc -l | tr -d ' '); [ "$c" -gt "$n" ] && n=$c
  done
  rows=""
  i=1
  while [ "$i" -le "$n" ]; do
    row="$OUT/.row-$p-$i.png"; cols=""
    for v in ref blitz fulgur; do
      f=$(ls "$OUT/$p.$v"-*.png 2>/dev/null | sed -n "${i}p")
      if [ -n "$f" ]; then cols="$cols $f"; else cols="$cols xc:white[425x550]"; fi
    done
    magick $cols -bordercolor gray50 -border 2 +append "$row"; rows="$rows $row"; i=$((i+1))
  done
  [ -n "$rows" ] && magick $rows -append "$OUT/$p.sheet.png" && rm -f $rows
done
echo done
