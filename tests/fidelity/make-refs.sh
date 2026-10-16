#!/bin/sh
# Regenerate the references of the rendering-fidelity corpus (tests/fidelity/ref/<case>.*)
# with the real wkhtmltopdf 0.12.6 (patched qt) in the Docker image wkref:local (amd64,
# fonts-dejavu-core only; docker/wkref/Dockerfile), the same image and fonts as
# tests/corpus, tests/parts and tests/js.
#
# Usage: WK_BIN=/path/to/wkhtmltopdf-linux-amd64 tests/fidelity/make-refs.sh [case ...]
# (the binary of the wkhtmltopdf-binary-edge 0.12.6.0 gem, libexec/wkhtmltopdf-linux-amd64;
# by default the repository's .ref/wkhtmltopdf-linux-amd64).
#
# Cases are the lines of cases.tsv: a name, the input page (in cases/), then extra arguments,
# all tab-separated. Every case runs, from the cases/ directory, as
#
#   wkhtmltopdf <DEFAULTS> <extra arguments...> <input page> out.pdf
#
# where DEFAULTS are pdfkit's options (Letter, 0.75in margins, UTF-8) with `--log-level warn`
# instead of `--quiet` (so warnings are recorded) and `--enable-local-file-access` (0.12.6
# blocks local sub-resources otherwise). An argument `{IN}` places the input page explicitly.
# The test (crates/wkhtmltopdf/tests/fidelity.rs) runs our binary the same way.
#
# Outputs per case: ref/<case>.pdf, .stderr, .rc (exit code) and .txt (pdftotext -layout).
# One container runs every case (amd64 emulation is slow to start), without network, with
# TZ=UTC and the C locale. The PDFs are not byte-reproducible (Qt writes the creation date);
# their rendering is. The references are committed: the test runs without Docker.
set -eu

DEFAULTS="--log-level warn --page-size Letter --margin-top 0.75in --margin-right 0.75in --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8 --enable-local-file-access"

if [ "${1:-}" = "--inside" ]; then
  shift
  cd /wk/repo/tests/fidelity/cases
  TAB=$(printf '\t')
  SEL=" $* "
  while IFS="$TAB" read -r name input rest || [ -n "$name" ]; do
    [ -n "$name" ] || continue
    if [ "$SEL" != "  " ]; then
      case "$SEL" in *" $name "*) ;; *) continue ;; esac
    fi
    # One argument per line: the defaults, the extra arguments, the input, the output.
    printf '%s\n' $DEFAULTS > /tmp/args
    if [ -n "$rest" ]; then printf '%s\n' "$rest" | tr '\t' '\n' >> /tmp/args; fi
    if grep -qx '{IN}' /tmp/args; then
      sed -i "s#^{IN}\$#$input#" /tmp/args
    else
      echo "$input" >> /tmp/args
    fi
    echo /tmp/out.pdf >> /tmp/args
    rm -f /tmp/out.pdf
    set --
    while IFS= read -r a; do set -- "$@" "$a"; done < /tmp/args
    rc=0
    /wk/bin/wkhtmltopdf "$@" > /dev/null 2> "/wk/ref/$name.stderr" || rc=$?
    echo "$rc" > "/wk/ref/$name.rc"
    if [ -f /tmp/out.pdf ]; then
      cp /tmp/out.pdf "/wk/ref/$name.pdf"
      pdftotext -layout /tmp/out.pdf "/wk/ref/$name.txt"
    fi
    echo "$name: rc=$rc $(cat "/wk/ref/$name.pdf" 2>/dev/null | wc -c) bytes"
  done < /wk/repo/tests/fidelity/cases.tsv
  exit 0
fi

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
WK_BIN=${WK_BIN:-$REPO/.ref/wkhtmltopdf-linux-amd64}
[ -f "$WK_BIN" ] || { echo "set WK_BIN to the wkhtmltopdf-linux-amd64 binary" >&2; exit 2; }
mkdir -p "$HERE/ref"

docker run --rm --platform linux/amd64 --network none -e TZ=UTC -e LANG= -e LC_ALL=C \
  -v "$REPO":/wk/repo:ro -v "$HERE/ref":/wk/ref -v "$WK_BIN":/wk/bin/wkhtmltopdf:ro \
  --entrypoint sh wkref:local /wk/repo/tests/fidelity/make-refs.sh --inside "$@"
