#!/usr/bin/env bash
# Runs inside the wkbench:local container (see ../run.sh). Benchmarks the original
# wkhtmltopdf and the Rust port on every scenario and writes raw results into $1.
#
#   inside.sh <results-dir> [quick] [phases]
#
# phases (default "timing pdfkit visual"):
#   timing  hyperfine + /usr/bin/time on every scenario
#   pdfkit  pdfkit gem integration run (pdfkit/generate.rb) for both binaries
#   visual  rendering parity (scripts/visual.py) + pipeline self-test; scenarios still run
#           once each (preflight) to produce the PDFs, without timing
#
# Per scenario and binary, it produces:
#   <id>.<bin>.out             output of one preflight run (PDF or text)
#   <id>.<bin>.stderr          its stderr
#   <id>.<bin>.meta            rc, command, output size, page count
#   <id>.<bin>.hyperfine.json  hyperfine --export-json (only if the preflight exited 0)
#   <id>.<bin>.time.txt        /usr/bin/time -v of one run (peak RSS, user/sys CPU)
# A binary that fails the preflight is recorded with its exit code and skipped: the
# harness never aborts on a failing scenario.
set -uo pipefail

OUT="$1"
QUICK="${2:-}"
PHASES=" ${3:-timing pdfkit visual} "
has() { [[ "$PHASES" == *" $1 "* ]]; }
CORPUS=/bench/corpus
declare -A BINS=([orig]=/usr/local/bin/wkhtmltopdf [rust]=/opt/rust/wkhtmltopdf)
ORDER=(orig rust)

if [[ "$QUICK" == quick ]]; then
  WARMUP=1; RUNS=3; BATCH_RUNS=2
else
  WARMUP=3; RUNS=10; BATCH_RUNS=10
fi

# pdfkit's exact invocation (pdfkit 0.8.x: default options + `- -`).
PDFKIT=(--quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in
        --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8)

mkdir -p "$OUT"
cd "$CORPUS"
: > "$OUT/scenarios.tsv"

quote() { local a out=""; for a in "$@"; do out+=" $(printf '%q' "$a")"; done; printf '%s' "${out# }"; }

# scenario <id> <label> <stdin-file|-> <repeat> -- <args...>
scenario() {
  local id="$1" label="$2" stdin="$3" repeat="$4"; shift 5
  local args; args="$(quote "$@")"
  printf '%s\t%s\t%s\n' "$id" "$label" "$repeat" >> "$OUT/scenarios.tsv"
  echo "== $id: $label"

  local which bin single bench rc size pages hf_opts
  for which in "${ORDER[@]}"; do
    bin="${BINS[$which]}"
    single="$bin $args"
    [[ "$stdin" != - ]] && single+=" < $stdin"
    if (( repeat > 1 )); then
      bench="for i in \$(seq $repeat); do $single > /dev/null || exit \$?; done"
      hf_opts=(--warmup 1 --runs "$BATCH_RUNS")
    else
      bench="$single"
      hf_opts=(--warmup "$WARMUP" --min-runs "$RUNS")
    fi

    sh -c "$single" > "$OUT/$id.$which.out" 2> "$OUT/$id.$which.stderr"
    rc=$?
    size=$(stat -c %s "$OUT/$id.$which.out")
    pages=-
    if [[ "$(head -c 4 "$OUT/$id.$which.out")" == %PDF ]]; then
      pages=$(pdfinfo "$OUT/$id.$which.out" 2>/dev/null | awk '/^Pages:/ {print $2}')
      [[ -n "$pages" ]] || pages="invalid"
    fi
    {
      echo "rc=$rc"; echo "size=$size"; echo "pages=$pages"; echo "cmd=$bench"
    } > "$OUT/$id.$which.meta"

    if (( rc != 0 )); then
      echo "   $which: n/a (exit $rc) — $(head -c 200 "$OUT/$id.$which.stderr" | head -1)"
      continue
    fi
    if ! has timing; then
      echo "   $which: ok (exit 0, ${pages} pages) — timing skipped"
      continue
    fi

    # No intermediate shell when the command has no redirection: more accurate for
    # millisecond-scale startup measurements.
    local shell_opt=()
    [[ "$stdin" == - && "$repeat" == 1 ]] && shell_opt=(-N)
    hyperfine "${shell_opt[@]}" "${hf_opts[@]}" --style basic \
      --command-name "$id/$which" --export-json "$OUT/$id.$which.hyperfine.json" \
      "$bench" > "$OUT/$id.$which.hyperfine.txt" 2>&1 \
      || echo "   $which: hyperfine failed (see $id.$which.hyperfine.txt)"
    /usr/bin/time -v -o "$OUT/$id.$which.time.txt" sh -c "$bench" > /dev/null 2>&1
    echo "   $which: $(grep -m1 'Time (mean' "$OUT/$id.$which.hyperfine.txt" | sed 's/^ *//')"
  done
}

# Environment record.
{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "mode=${QUICK:-full} warmup=$WARMUP runs=$RUNS batch_runs=$BATCH_RUNS"
  echo "phases=$(echo $PHASES)"
  echo "uname=$(uname -srm)"
  echo "cpus=$(nproc)"
  echo "cpu_model=$(lscpu 2>/dev/null | awk -F: '/Model name|Vendor ID/ {gsub(/^ +/,"",$2); if ($2 != "-") printf "%s ", $2}')"
  echo "mem_total=$(awk '/MemTotal/ {printf "%.1f GiB", $2/1048576}' /proc/meminfo)"
  echo "orig_version=$(${BINS[orig]} --version 2>&1)"
  echo "orig_sha256=$(sha256sum ${BINS[orig]} | cut -c1-16)"
  echo "rust_version=$(${BINS[rust]} --version 2>&1)"
  echo "rust_sha256=$(sha256sum ${BINS[rust]} | cut -c1-16)"
  echo "rust_size=$(stat -c %s ${BINS[rust]})"
  echo "hyperfine=$(hyperfine --version)"
  echo "pdfkit=$(bundle exec ruby -e 'require "pdfkit"; print PDFKit::VERSION') (ruby $(ruby -e 'print RUBY_VERSION'))"
  echo "imagemagick=$(convert -version | head -1 | cut -d" " -f2-3)"
  echo "fonts=$(fc-list : family | sort -u | paste -sd, -)"
} > "$OUT/env.txt"

if has timing || has visual; then
# 1. CLI startup
scenario version   "CLI startup: --version"        - 1 -- --version
scenario help      "CLI startup: -H > /dev/null"   - 1 -- -H

# 2. Conversions, pdfkit invocation (HTML on stdin, PDF on stdout)
scenario small     "Small page (a.html)"           small.html  1 -- "${PDFKIT[@]}" - -
scenario medium    "Medium page (4 pages)"         medium.html 1 -- "${PDFKIT[@]}" - -
scenario large     "Large invoice (30 pages)"      large.html  1 -- "${PDFKIT[@]}" - -
# Same as above minus the default 200 ms --javascript-delay sleep: isolates engine cost.
scenario small_nd  "Small page, --javascript-delay 0"  small.html 1 -- "${PDFKIT[@]}" --javascript-delay 0 - -
scenario large_nd  "Large invoice, --javascript-delay 0" large.html 1 -- "${PDFKIT[@]}" --javascript-delay 0 - -
scenario hf_toc    "Header/footer HTML + toc + outline" hf-body.html 1 -- "${PDFKIT[@]}" \
  --enable-local-file-access --header-html header.html --footer-html footer.html \
  --header-spacing 3 --footer-spacing 3 --outline toc - -
scenario js        "jQuery DOM build + --javascript-delay 200" js.html 1 -- "${PDFKIT[@]}" \
  --javascript-delay 200 - -
scenario multi     "Multi-object: cover + toc + 3 pages" - 1 -- "${PDFKIT[@]}" \
  --enable-local-file-access cover cover.html toc part1.html part2.html part3.html -

# 3. Batch throughput
scenario batch20   "Batch: 20 x medium, sequential" medium.html 20 -- "${PDFKIT[@]}" - -
fi

if has pdfkit; then
  echo "== pdfkit gem $(bundle exec ruby -e 'require "pdfkit"; print PDFKit::VERSION') integration"
  for which in "${ORDER[@]}"; do
    label=$([[ $which == orig ]] && echo original || echo rust)
    mkdir -p "$OUT/pdfkit/$label"
    CORPUS="$CORPUS" bundle exec ruby /bench/pdfkit/generate.rb "$label" "${BINS[$which]}" "$OUT/pdfkit/$label" \
      2> "$OUT/pdfkit/$label/stderr.log" || echo "   pdfkit run for $label crashed (see pdfkit/$label/stderr.log)"
  done
fi

if has visual; then
  # Pipeline self-test: original vs original. "identical" must PASS; the variants must be caught.
  ST="$OUT/visual-selftest"
  mkdir -p "$ST"
  W="${BINS[orig]}"
  "$W" "${PDFKIT[@]}" - - < medium.html > "$ST/base.pdf"
  "$W" "${PDFKIT[@]}" - - < medium.html > "$ST/base-rerun.pdf"
  "$W" "${PDFKIT[@]}" --zoom 1.1 - - < medium.html > "$ST/zoom-1.1.pdf"
  "$W" "${PDFKIT[@]}" --margin-left 0.85in - - < medium.html > "$ST/margin-left-0.85in.pdf"
  cat > "$ST/manifest.json" <<'JSON'
[
  {"case": "selftest-identical", "a": "base.pdf", "b": "base-rerun.pdf", "label_a": "original", "label_b": "original (rerun)"},
  {"case": "selftest-zoom-1.1", "a": "base.pdf", "b": "zoom-1.1.pdf", "label_a": "original", "label_b": "original --zoom 1.1"},
  {"case": "selftest-margin-left-0.85in", "a": "base.pdf", "b": "margin-left-0.85in.pdf", "label_a": "original", "label_b": "original --margin-left 0.85in"}
]
JSON
  echo "== rendering parity"
  python3 -I /bench/scripts/visual.py "$OUT"
fi

python3 -I /bench/scripts/report.py "$OUT"
