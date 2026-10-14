#!/usr/bin/env bash
# Runs inside the wkjava:local container (see run.sh). Benchmarks OpenHTMLtoPDF against the
# Rust port on the static corpus pages and writes raw results into $1.
#
#   inside.sh <results-dir> [quick]
#
# Layout of <results-dir>:
#   env.txt                                  versions, CPU, iteration counts
#   cold/<case>.<variant>.{pdf,meta,stderr}  preflight output (size, pages, exit code)
#   cold/<case>.<variant>.hyperfine.json     one process per conversion
#   cold/<case>.<variant>.time<k>.txt        /usr/bin/time -v runs (peak RSS, CPU)
#   warm/<case>.<impl>.{json,time.txt}       steady-state loops (JSON from the drivers)
#   heap/<case>.xmx<M>.{json,time.txt,rc}    smallest working Java heap
#   visual/<case>.png                        page 1, rust | java, side by side
# A failing variant is recorded with its exit code; the harness never aborts on it.
set -uo pipefail

OUT="$1"
QUICK="${2:-}"
CORPUS=/bench/corpus
RUST=/opt/rust/wkhtmltopdf
INPROC=/opt/rust/wkhtmltopdf-inproc
JAR=/opt/bench/convert.jar
CDS=/opt/bench/app.jsa

# Static pages only: OpenHTMLtoPDF runs no JavaScript and has no equivalent of
# --header-html/--footer-html or the generated toc, so js, hf_toc and multi are left out.
# hf-body is the hf_toc body page converted on its own.
CASES=(small medium large hf-body)

if [[ "$QUICK" == quick ]]; then
  WARMUP=1; RUNS=3; TIME_RUNS=1; LOOP_WARMUP=3; LOOP=5; HEAP_WARMUP=2; HEAP_LOOP=3
else
  WARMUP=3; RUNS=10; TIME_RUNS=3; LOOP_WARMUP=20; LOOP=30; HEAP_WARMUP=5; HEAP_LOOP=10
fi
HEAPS=(16 24 32 48 64 96 128 192 256)
HEAP_CASES=(medium large)

# pdfkit's exact invocation, as in bench/scripts/inside.sh.
PDFKIT=(--quiet --page-size Letter --margin-top 0.75in --margin-right 0.75in
        --margin-bottom 0.75in --margin-left 0.75in --encoding UTF-8)
pdfkit_q="$(printf '%q ' "${PDFKIT[@]}")"

# Cold variants: one process per conversion. Java's stderr carries PDFBox font-fallback
# warnings; it is discarded in every variant.
declare -A COLD=(
  [rust]="$RUST $pdfkit_q - - < @IN@ > @OUT@"
  [java]="java -jar $JAR @IN@ @OUT@ 2>/dev/null"
  [java-tuned]="java -XX:TieredStopAtLevel=1 -Xshare:auto -jar $JAR @IN@ @OUT@ 2>/dev/null"
  [java-cds]="java -XX:TieredStopAtLevel=1 -XX:SharedArchiveFile=$CDS -Xshare:auto -jar $JAR @IN@ @OUT@ 2>/dev/null"
)
VARIANTS=(rust java java-tuned java-cds)

mkdir -p "$OUT"/{cold,warm,heap,visual}
cd "$CORPUS"

timev() { # timev <file> <command string>
  /usr/bin/time -v -o "$1" bash -c "$2" > /dev/null 2>&1
}

{
  echo "date_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "mode=${QUICK:-full} warmup=$WARMUP runs=$RUNS time_runs=$TIME_RUNS loop_warmup=$LOOP_WARMUP loop=$LOOP heap_warmup=$HEAP_WARMUP heap_loop=$HEAP_LOOP"
  echo "uname=$(uname -srm)"
  echo "cpus=$(nproc)"
  echo "mem_total=$(awk '/MemTotal/ {printf "%.1f GiB", $2/1048576}' /proc/meminfo)"
  echo "loadavg=$(cut -d' ' -f1-3 /proc/loadavg)"
  echo "java=$(java -version 2>&1 | sed -n 2p)"
  echo "java_flags_default=$(java -XX:+PrintFlagsFinal -version 2>/dev/null | awk '/ MaxHeapSize | UseG1GC | UseSerialGC | UseParallelGC / && /true|MaxHeap/ {printf "%s=%s ", $2, $4}')"
  echo "deps=$(grep -oE '[a-z0-9.-]+:[a-z0-9.-]+:jar:[^:]+' /opt/bench/dependencies.txt | sed 's/:jar:/:/' | sort | paste -sd' ' -)"
  echo "rust_version=$($RUST --version 2>&1)"
  echo "rust_sha256=$(sha256sum $RUST | cut -c1-16)"
  echo "rust_size=$(stat -c %s $RUST)"
  echo "inproc=$([[ -x $INPROC ]] && echo yes || echo no)"
  echo "hyperfine=$(hyperfine --version)"
  echo "fonts=$(fc-list : family | sort -u | paste -sd, -)"
} > "$OUT/env.txt"

# 1. Cold: one process per conversion.
for c in "${CASES[@]}"; do
  in="$CORPUS/$c.html"
  echo "== cold $c"
  for v in "${VARIANTS[@]}"; do
    base="$OUT/cold/$c.$v"
    cmd="${COLD[$v]//@IN@/$in}"
    bash -c "${cmd//@OUT@/$base.pdf}" 2> "$base.stderr"
    rc=$?
    pages=$(pdfinfo "$base.pdf" 2>/dev/null | awk '/^Pages:/ {print $2}')
    { echo "rc=$rc"; echo "size=$(stat -c %s "$base.pdf" 2>/dev/null || echo 0)"; echo "pages=${pages:--}"
      echo "words=$(pdftotext -q "$base.pdf" - 2>/dev/null | wc -w)"
      echo "fonts=$(pdffonts "$base.pdf" 2>/dev/null | tail -n +3 | awk '{sub(/^[A-Z]+\+/, "", $1); print $1}' | sort -u | paste -sd' ' -)"
      echo "cmd=${cmd//@OUT@/out.pdf}"; } > "$base.meta"
    if (( rc != 0 )); then echo "   $v: n/a (exit $rc)"; continue; fi
    hyperfine --warmup "$WARMUP" --min-runs "$RUNS" --style basic --command-name "$c/$v" \
      --export-json "$base.hyperfine.json" "${cmd//@OUT@//tmp/cold-$v.pdf}" > "$base.hyperfine.txt" 2>&1 \
      || echo "   $v: hyperfine failed"
    for k in $(seq "$TIME_RUNS"); do timev "$base.time$k.txt" "${cmd//@OUT@//tmp/cold-$v.pdf}"; done
    echo "   $v: $(grep -m1 'Time (mean' "$base.hyperfine.txt" | sed 's/^ *//')"
  done
done

# 2. Warm: steady state in one process (Java, Rust in-process) and per-process loop (Rust CLI).
for c in "${CASES[@]}"; do
  in="$CORPUS/$c.html"
  echo "== warm $c"
  base="$OUT/warm/$c"
  /usr/bin/time -v -o "$base.java.time.txt" \
    java -jar "$JAR" --warmup "$LOOP_WARMUP" --loop "$LOOP" "$in" /tmp/warm-java.pdf \
    > "$base.java.json" 2>/dev/null || echo "   java: failed"
  if [[ -x "$INPROC" ]]; then
    /usr/bin/time -v -o "$base.rust-inproc.time.txt" \
      "$INPROC" --warmup "$LOOP_WARMUP" --loop "$LOOP" --stdin "$in" --out /tmp/warm-rust.pdf -- "${PDFKIT[@]}" - - \
      > "$base.rust-inproc.json" 2>/dev/null || echo "   rust-inproc: failed"
  fi
  # Rust CLI in a loop: the per-process cost of a warm page cache, the CLI's steady state.
  loop="for i in \$(seq $LOOP); do $RUST $pdfkit_q - - < $in > /dev/null || exit 1; done"
  timev "$base.rust-loop.time.txt" "$loop" || echo "   rust-loop: failed"
  echo "{\"impl\":\"rust-loop\",\"loop\":$LOOP}" > "$base.rust-loop.json"
  for f in "$base".*.json; do echo "   $(basename "$f" .json): $(head -c 160 "$f")"; done
done

# 3. Smallest Java heap that converts the document repeatedly.
for c in "${HEAP_CASES[@]}"; do
  in="$CORPUS/$c.html"
  echo "== heap $c"
  for m in "${HEAPS[@]}"; do
    base="$OUT/heap/$c.xmx$m"
    /usr/bin/time -v -o "$base.time.txt" \
      java -Xmx${m}m -jar "$JAR" --warmup "$HEAP_WARMUP" --loop "$HEAP_LOOP" "$in" /tmp/heap.pdf \
      > "$base.json" 2> "$base.stderr"
    echo $? > "$base.rc"
    echo "   -Xmx${m}m: exit $(cat "$base.rc")"
  done
done

# 4. Visual sanity: page 1 of each engine's cold output, side by side.
for c in "${CASES[@]}"; do
  r="$OUT/cold/$c.rust.pdf"; j="$OUT/cold/$c.java.pdf"
  [[ -s "$r" && -s "$j" ]] || continue
  pdftoppm -r 60 -f 1 -l 1 -png -singlefile "$r" "/tmp/$c-rust"
  pdftoppm -r 60 -f 1 -l 1 -png -singlefile "$j" "/tmp/$c-java"
  java -Djava.awt.headless=true -jar "$JAR" --montage "$OUT/visual/$c.png" \
    "wkhtmltopdf-rs" "/tmp/$c-rust.png" "OpenHTMLtoPDF" "/tmp/$c-java.png" 2>/dev/null \
    || echo "   montage $c failed"
done

python3 -I /bench/java/report.py "$OUT"
