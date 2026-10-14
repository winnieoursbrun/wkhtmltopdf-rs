#!/usr/bin/env bash
# Build the Rust wkhtmltopdf for aarch64-unknown-linux-musl (static) from macOS arm64.
#
# Primary method: rustup target + rust-lld (ships with rustup, needs no C cross toolchain;
# the workspace is pure Rust). Fallback: build natively inside an arm64 rust:alpine container
# (alpine's host target *is* aarch64-unknown-linux-musl), for the day a dependency needs a C
# compiler. Prints the path of the produced binary on the last line of stdout.
set -euo pipefail

BENCH_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$BENCH_DIR/.." && pwd)"
TARGET=aarch64-unknown-linux-musl
TARGET_DIR="$BENCH_DIR/.target"
BIN="$TARGET_DIR/$TARGET/release/wkhtmltopdf"
RUST_IMAGE="${RUST_IMAGE:-rust:1.90-alpine}"

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"

build_lld() {
  command -v rustup >/dev/null || return 1
  rustup target list --installed | grep -qx "$TARGET" || rustup target add "$TARGET" >&2 || return 1
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
    cargo build --release --locked --manifest-path "$ROOT/Cargo.toml" \
      --target "$TARGET" --target-dir "$TARGET_DIR" -p wkhtmltopdf >&2
}

build_docker() {
  docker run --rm --platform linux/arm64 \
    -v "$ROOT":/src:ro -v "$TARGET_DIR":/target \
    -v wkbench-cargo-registry:/usr/local/cargo/registry \
    -e CARGO_TARGET_DIR=/target \
    "$RUST_IMAGE" sh -c "apk add --no-cache musl-dev >/dev/null && cd /src && cargo build --release --locked --target $TARGET -p wkhtmltopdf" >&2
}

mkdir -p "$TARGET_DIR"
if [[ "${BUILD_METHOD:-auto}" != docker ]] && build_lld; then
  echo "built with rustup + rust-lld" >&2
else
  echo "rust-lld build unavailable or failed; falling back to $RUST_IMAGE" >&2
  build_docker
fi

file "$BIN" | grep -q 'ARM aarch64.*statically linked' || { echo "unexpected binary: $(file "$BIN")" >&2; exit 1; }
echo "$BIN"
