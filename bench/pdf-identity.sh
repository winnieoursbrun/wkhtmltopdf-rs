#!/usr/bin/env bash
# Byte-identity check of the PDFs produced by two builds of wkhtmltopdf-rs.
#
# Used to prove that a performance change does not alter the output:
#
#   bench/pdf-identity.sh run  <binary> <outdir>   # convert the whole corpus into <outdir>
#   bench/pdf-identity.sh diff <dirA> <dirB>       # compare two runs, exit 1 on any difference
#   bench/pdf-identity.sh equiv <dirA> <dirB>      # same, after decompressing every stream
#
# `equiv` (bench/scripts/pdf_equiv.py) is for changes that alter compressed bytes only: same
# objects, dictionaries and decompressed data; lengths and offsets masked; prints total sizes.
#
# Typical flow: build the base commit, `run` into before/; build the change, `run` into after/;
# `diff before after`.
#
# Corpus: bench/corpus scenarios (same arguments as bench/scripts/inside.sh), tests/corpus/*.html
# (pdfkit arguments plus <name>.args), tests/js/cases.tsv, tests/parts/*/cmd and the upstream
# conversion fixtures. Every run uses a fixed SOURCE_DATE_EPOCH (CreationDate, JS clock) and
# WKHTMLTOPDF_SYSTEM_FONTS=0 (embedded fonts only), so two runs of the same binary are identical.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export SOURCE_DATE_EPOCH=1700000000
export WKHTMLTOPDF_SYSTEM_FONTS=0
PDFKIT=(--quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in
  --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8)

run() {
  local bin out
  bin="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
  out="$2"
  mkdir -p "$out"
  out="$(cd "$out" && pwd)"

  # bench/corpus scenarios.
  (
    cd "$ROOT/bench/corpus" || exit 1
    "$bin" "${PDFKIT[@]}" - - < small.html > "$out/bench-small.pdf" 2>/dev/null
    "$bin" "${PDFKIT[@]}" - - < medium.html > "$out/bench-medium.pdf" 2>/dev/null
    "$bin" "${PDFKIT[@]}" - - < large.html > "$out/bench-large.pdf" 2>/dev/null
    "$bin" "${PDFKIT[@]}" --enable-local-file-access --header-html header.html \
      --footer-html footer.html --header-spacing 3 --footer-spacing 3 --outline toc - - \
      < hf-body.html > "$out/bench-hf_toc.pdf" 2>/dev/null
    "$bin" "${PDFKIT[@]}" --javascript-delay 200 - - < js.html > "$out/bench-js.pdf" 2>/dev/null
    "$bin" "${PDFKIT[@]}" --enable-local-file-access cover cover.html toc part1.html \
      part2.html part3.html - > "$out/bench-multi.pdf" 2>/dev/null
  )

  # tests/corpus (L2/L3 corpus).
  for f in "$ROOT"/tests/corpus/*.html; do
    local name extra=()
    name="$(basename "$f" .html)"
    if [ -f "$ROOT/tests/corpus/$name.args" ]; then
      read -r -a extra < "$ROOT/tests/corpus/$name.args"
    fi
    (cd "$ROOT/tests/corpus" && "$bin" "${PDFKIT[@]}" ${extra[@]+"${extra[@]}"} - - \
      < "$f" > "$out/corpus-$name.pdf" 2>/dev/null)
  done

  # tests/js (JavaScript corpus).
  while IFS=$'\t' read -r -a fields; do
    [ "${#fields[@]}" -gt 0 ] || continue
    local name="${fields[0]}" args=()
    local a
    for a in "${fields[@]:1}"; do
      args+=("${a//\{DIR\}/$ROOT/tests/js}")
    done
    (cd "$ROOT/tests/js" && "$bin" "${args[@]}" "$out/js-$name.pdf" > /dev/null 2>&1)
  done < "$ROOT/tests/js/cases.tsv"

  # tests/parts (headers/footers, TOC, outline, forms...). Each case writes out.pdf in its
  # directory: run it in a scratch copy. The copy's path ends up in file:// link targets, so it
  # is the same on every run.
  local tmp="${TMPDIR:-/tmp}/wk-pdf-identity-parts"
  rm -rf "$tmp"
  mkdir -p "$tmp"
  for d in "$ROOT"/tests/parts/*/; do
    local name
    name="$(basename "$d")"
    [ -f "$d/cmd" ] || continue
    rm -rf "${tmp:?}/$name"
    cp -R "$d" "$tmp/$name"
    local args=()
    while IFS= read -r line || [ -n "$line" ]; do args+=("$line"); done < "$d/cmd"
    (cd "$tmp/$name" && "$bin" "${args[@]}" > /dev/null 2>&1)
    for o in "$tmp/$name"/out*; do
      [ -e "$o" ] && cp "$o" "$out/parts-$name-$(basename "$o")"
    done
  done
  rm -rf "$tmp"

  # Upstream conversion fixtures (inputs only).
  for f in "$ROOT"/docs/upstream-fixtures/0.12.6/conversion/*.html; do
    (cd "$(dirname "$f")" && "$bin" "${PDFKIT[@]}" - - < "$f" \
      > "$out/fixture-$(basename "$f" .html).pdf" 2>/dev/null)
  done

  ls "$out" | wc -l | awk '{print $1 " files written"}'
}

# HTML read from stdin is spooled to a temporary file named wktemp-<uuid>.html (as upstream
# does), and that name ends up in link destinations: mask the random part, which keeps the
# length, hence every offset, unchanged. The trailer /ID is a digest of the document, so it
# follows that name: mask it too (everything it digests is compared anyway).
normalized() {
  LC_ALL=C perl -pe '
    s/wktemp-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/wktemp-00000000-0000-0000-0000-000000000000/g;
    s{/ID\[\(([A-Za-z0-9+/=]{24})\)\(([A-Za-z0-9+/=]{24})\)\]}{"/ID[(" . ("0" x 24) . ")(" . ("0" x 24) . ")]"}ge;
  ' "$1"
}

diff_dirs() {
  local a="$1" b="$2" bad=0 n=0
  for f in "$a"/*; do
    local name
    name="$(basename "$f")"
    n=$((n + 1))
    if [ ! -e "$b/$name" ]; then
      echo "MISSING in $b: $name"
      bad=1
    elif ! cmp -s <(normalized "$f") <(normalized "$b/$name"); then
      echo "DIFFERS: $name ($(wc -c < "$f") vs $(wc -c < "$b/$name") bytes)"
      bad=1
    fi
  done
  for f in "$b"/*; do
    [ -e "$a/$(basename "$f")" ] || { echo "EXTRA in $b: $(basename "$f")"; bad=1; }
  done
  if [ "$bad" = 0 ]; then
    echo "OK: $n files byte-identical"
  fi
  return "$bad"
}

case "${1:-}" in
  run) run "$2" "$3" ;;
  diff) diff_dirs "$2" "$3" ;;
  equiv) python3 -I "$ROOT/bench/scripts/pdf_equiv.py" "$2" "$3" ;;
  *)
    echo "usage: $0 run <binary> <outdir> | diff <dirA> <dirB> | equiv <dirA> <dirB>" >&2
    exit 2
    ;;
esac
