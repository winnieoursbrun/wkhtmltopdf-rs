#!/usr/bin/env bash
# Benchmark the Rust port against OpenHTMLtoPDF (Java), both run natively (linux/arm64) in
# the same container. See bench/README.md, section "OpenHTMLtoPDF comparison".
#
#   bench/java/run.sh [--quick] [--no-build]
#
#   --quick     1 warmup + 3 runs per cold scenario, short warm loops (smoke test, not for numbers)
#   --no-build  reuse the existing wkjava:local image and Rust binaries as they are
#
# Results: bench/results/java-<UTC timestamp>/ (raw files + REPORT.md), copied to
# bench/results/JAVA-LATEST.md. Independent of bench/run.sh and its wkbench:local image.
set -euo pipefail

JAVA_DIR="$(cd "$(dirname "$0")" && pwd)"
BENCH_DIR="$(cd "$JAVA_DIR/.." && pwd)"
IMAGE="${WKJAVA_IMAGE:-wkjava:local}"
QUICK=""
BUILD=1
for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=quick ;;
    --no-build) BUILD=0 ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

TS="$(date -u +%Y%m%dT%H%M%SZ)"
RESULTS="$BENCH_DIR/results/java-$TS"

echo "--> image $IMAGE"
if (( BUILD )) || ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  docker build --quiet --platform linux/arm64 -t "$IMAGE" "$JAVA_DIR" >/dev/null
fi

echo "--> Rust binaries (aarch64-unknown-linux-musl, CLI + in-process harness)"
REL="$BENCH_DIR/.target/aarch64-unknown-linux-musl/release"
if (( BUILD )) || [[ ! -x "$REL/wkhtmltopdf" || ! -x "$REL/wkhtmltopdf-inproc" ]]; then
  WITH_INPROC=1 "$BENCH_DIR/scripts/build-rust.sh" >/dev/null
fi

echo "--> corpus"
python3 -I "$BENCH_DIR/scripts/gen_corpus.py" "$BENCH_DIR/corpus"

echo "--> running into results/java-$TS"
mkdir -p "$RESULTS"
docker run --rm --platform linux/arm64 \
  --network none \
  -v "$BENCH_DIR":/bench \
  -v "$REL/wkhtmltopdf":/opt/rust/wkhtmltopdf:ro \
  -v "$REL/wkhtmltopdf-inproc":/opt/rust/wkhtmltopdf-inproc:ro \
  "$IMAGE" bash /bench/java/inside.sh "/bench/results/java-$TS" "$QUICK"

# JAVA-LATEST.md sits one level up: rebase the report's relative links onto the run directory.
sed -E "s#\]\(visual/#](java-${TS}/visual/#g" "$RESULTS/REPORT.md" > "$BENCH_DIR/results/JAVA-LATEST.md"
echo "--> $RESULTS/REPORT.md (copied to results/JAVA-LATEST.md)"
