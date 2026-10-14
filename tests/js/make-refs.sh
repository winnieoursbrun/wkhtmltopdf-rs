#!/bin/sh
# Regenerate the JavaScript corpus references (tests/js/ref/<case>.{pdf,stderr,rc}) with the
# real wkhtmltopdf 0.12.6 (patched qt) in the Docker image wkref:local (amd64, DejaVu fonts
# only, TZ=UTC).
#
# Cases are the lines of cases.tsv: a name, then the arguments (tab-separated, `{DIR}`
# standing for the corpus directory); the output file `out.pdf` is appended. Each case runs
# with the corpus directory mounted at /wk/js as the working directory.
#
# Usage: WK_BIN=/path/to/wkhtmltopdf-linux-amd64 tests/js/make-refs.sh [case ...]
# The references are committed: the test (crates/wkhtmltopdf/tests/js_corpus.rs) runs
# without Docker.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
WK_BIN=${WK_BIN:?set WK_BIN to the wkhtmltopdf-linux-amd64 binary}
mkdir -p "$HERE/ref"
TAB=$(printf '\t')
while IFS="$TAB" read -r name rest; do
  [ -n "$name" ] || continue
  if [ $# -gt 0 ]; then
    case " $* " in *" $name "*) ;; *) continue ;; esac
  fi
  # One argument per line, {DIR} resolved inside the container.
  printf '%s\n' "$rest" | tr '\t' '\n' | sed 's#{DIR}#/wk/js#g' > "$HERE/ref/$name.args"
  echo out.pdf >> "$HERE/ref/$name.args"
  docker run --rm --platform linux/amd64 -e TZ=UTC \
    -v "$WK_BIN":/wk/wkhtmltopdf:ro -v "$HERE":/wk/js -w /wk/js --entrypoint sh wkref:local -c \
    'set --; while IFS= read -r a; do set -- "$@" "$a"; done < ref/'"$name"'.args;
     /wk/wkhtmltopdf "$@" 2> ref/'"$name"'.stderr; echo $? > ref/'"$name"'.rc'
  mv "$HERE/out.pdf" "$HERE/ref/$name.pdf"
  rm -f "$HERE/ref/$name.args"
  echo "ref/$name.pdf: $(wc -c < "$HERE/ref/$name.pdf") bytes, rc $(cat "$HERE/ref/$name.rc")"
done < "$HERE/cases.tsv"
