#!/bin/sh
# Regenerate the reference outputs of the P3 document-parts corpus (tests/parts/<case>/ref.*)
# with the real wkhtmltopdf 0.12.6 (patched Qt) running in the wkref:local image.
#
# Usage: WK_BIN_DIR=<dir holding wkhtmltopdf-linux-amd64> tests/parts/make-refs.sh [case ...]
#
# Step 1 (wkref:local, amd64, --network none, TZ=UTC, C locale): each case directory is copied
#   to /work/<case>, the argument list in `cmd` is run from there, and the outputs are copied
#   back as ref.pdf, ref.stdout, ref.stderr, ref.rc, ref.outline.xml (when the case dumps
#   out.outline.xml), ref.txt (pdftotext -layout, pages separated by \f) and ref.info (pdfinfo).
# Step 2 (python:3.11-slim, pypdf pinned): ref.json = pdfstruct.py ref.pdf.
set -eu

if [ "${1:-}" = "--inside" ]; then
  shift
  W=/wk/wkhtmltopdf-linux-amd64
  for c in "$@"; do
    rm -rf "/work/$c" /parts/"$c"/ref.*
    mkdir -p /work && cp -r "/parts/$c" "/work/$c"
    (
      cd "/work/$c"
      set --
      while IFS= read -r line || [ -n "$line" ]; do set -- "$@" "$line"; done < cmd
      rc=0; "$W" "$@" > ref.stdout 2> ref.stderr || rc=$?
      echo "$rc" > ref.rc
      cp ref.stdout ref.stderr ref.rc "/parts/$c/"
      [ -f out.outline.xml ] && cp out.outline.xml "/parts/$c/ref.outline.xml"
      if [ -f out.pdf ]; then
        cp out.pdf "/parts/$c/ref.pdf"
        pdftotext -layout out.pdf "/parts/$c/ref.txt"
        pdfinfo out.pdf > "/parts/$c/ref.info"
      fi
      echo "$c: rc=$rc"
    )
  done
  exit 0
fi

HERE=$(cd "$(dirname "$0")" && pwd)
WK_BIN_DIR=${WK_BIN_DIR:?set WK_BIN_DIR to the directory holding wkhtmltopdf-linux-amd64}
if [ $# -eq 0 ]; then
  set --
  for d in "$HERE"/*/; do [ -f "$d/cmd" ] && set -- "$@" "$(basename "$d")"; done
fi

docker run --rm --platform linux/amd64 --network none -e TZ=UTC -e LANG= -e LC_ALL= \
  -v "$HERE":/parts -v "$WK_BIN_DIR":/wk:ro --entrypoint sh wkref:local \
  /parts/make-refs.sh --inside "$@"

docker run --rm -v "$HERE":/parts python:3.11-slim sh -c '
  pip install -q --disable-pip-version-check --root-user-action=ignore pypdf==5.1.0 >/dev/null
  for c in "$@"; do
    [ -f "/parts/$c/ref.pdf" ] || continue
    python -I /parts/pdfstruct.py "/parts/$c/ref.pdf" --base "file:///work/$c/" > "/parts/$c/ref.json"
  done' sh "$@"
