#!/usr/bin/env bash
# Benchmark the original wkhtmltopdf 0.12.6 against the Rust port, both run natively
# (linux/arm64) in the same container. See bench/README.md.
#
#   bench/run.sh [--quick] [--no-build] [--pdfkit] [--visual]
#
#   (no phase flag)  everything: timing + pdfkit integration + rendering parity
#   --pdfkit    only the pdfkit gem integration run (outcome table)
#   --visual    only rendering parity (runs each conversion once, the pdfkit cases, and the
#               pipeline self-test; no timing). Combine with --pdfkit freely.
#   --quick     1 warmup + 3 runs per scenario, 2 runs for the batch (smoke test, not for numbers)
#   --no-build  reuse the existing Rust binary and image as they are
#
# Results: bench/results/<UTC timestamp>/ (raw JSON + REPORT.md), copied to bench/results/LATEST.md.
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE="${WKBENCH_IMAGE:-wkbench:local}"
QUICK=""
BUILD=1
PHASES=""
for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=quick ;;
    --pdfkit) PHASES+=" pdfkit" ;;
    --visual) PHASES+=" pdfkit visual" ;;
    --no-build) BUILD=0 ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

TS="$(date -u +%Y%m%dT%H%M%SZ)"
RESULTS="$BENCH_DIR/results/$TS"

echo "--> image $IMAGE"
if (( BUILD )) || ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  docker build --quiet --platform linux/arm64 -t "$IMAGE" "$BENCH_DIR" >/dev/null
fi

echo "--> Rust binary (aarch64-unknown-linux-musl)"
BIN="$BENCH_DIR/.target/aarch64-unknown-linux-musl/release/wkhtmltopdf"
if (( BUILD )) || [[ ! -x "$BIN" ]]; then
  BIN="$("$BENCH_DIR/scripts/build-rust.sh" | tail -1)"
fi

echo "--> corpus"
python3 -I "$BENCH_DIR/scripts/gen_corpus.py" "$BENCH_DIR/corpus"

echo "--> running scenarios into results/$TS"
mkdir -p "$RESULTS"
docker run --rm --platform linux/arm64 \
  --network none \
  -v "$BENCH_DIR":/bench \
  -v "$BIN":/opt/rust/wkhtmltopdf:ro \
  "$IMAGE" bash /bench/scripts/inside.sh "/bench/results/$TS" "$QUICK" \
  "$(echo $PHASES | tr ' ' '\n' | sort -u | paste -sd' ' -)"

# LATEST.md sits one level up: rebase the report's relative links onto the run directory.
sed -E "s#\]\((visual|pdfkit)/#](${TS}/\1/#g" "$RESULTS/REPORT.md" > "$BENCH_DIR/results/LATEST.md"
echo "--> $RESULTS/REPORT.md (copied to results/LATEST.md)"
