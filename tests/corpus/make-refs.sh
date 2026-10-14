#!/bin/sh
# Regenerate the L2 reference PDFs (tests/corpus/ref/<page>.pdf) with the real
# wkhtmltopdf 0.12.6 (patched qt) in the Docker image wkref:local (amd64,
# DejaVu fonts only), using pdfkit's default invocation: HTML on stdin, PDF on
# stdout, Letter, 0.75in margins, UTF-8.
#
# Usage: WK_BIN=/path/to/wkhtmltopdf-linux-amd64 tests/corpus/make-refs.sh [page ...]
# The binary comes from the wkhtmltopdf-binary-edge 0.12.6.0 gem
# (libexec/wkhtmltopdf-linux-amd64). Pages default to every *.html here.
# The references are committed: the L2 test (crates/wkhtmltopdf/tests/corpus_l2.rs)
# runs without Docker.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
WK_BIN=${WK_BIN:?set WK_BIN to the wkhtmltopdf-linux-amd64 binary}
ARGS="--quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8"
mkdir -p "$HERE/ref"
if [ $# -eq 0 ]; then
  set -- $(cd "$HERE" && ls *.html | sed 's/\.html$//')
fi
for p in "$@"; do
  docker run --rm -i --platform linux/amd64 -e TZ=UTC -v "$WK_BIN":/wk/wkhtmltopdf:ro \
    --entrypoint /wk/wkhtmltopdf wkref:local $ARGS - - < "$HERE/$p.html" > "$HERE/ref/$p.pdf"
  echo "ref/$p.pdf: $(wc -c < "$HERE/ref/$p.pdf") bytes"
done
