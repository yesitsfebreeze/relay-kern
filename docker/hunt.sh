#!/usr/bin/env bash
# Build kern with the hunt profile + feature, run under heaptrack.
# Run inside the dev container.
set -euo pipefail

OUTDIR="/work/docker/out"
mkdir -p "$OUTDIR"

HUNT_SECS="${HUNT_SECS:-300}"

echo "[1/2] cargo build --profile hunt --features hunt"
RUSTFLAGS="${RUSTFLAGS:-} -C force-frame-pointers=yes" \
  cargo build --profile hunt --features hunt

BIN_DIR="$CARGO_TARGET_DIR/hunt"
OUT="$OUTDIR/heaptrack.kern.$(date +%Y%m%d-%H%M%S).gz"

echo "[2/2] kern (${HUNT_SECS}s) -> $OUT"
heaptrack -o "$OUT" "$BIN_DIR/kern" hunt --secs "$HUNT_SECS"

echo
echo "Analyse:    heaptrack_print $OUT | less"
echo "Top leaks:  heaptrack_print --print-leaks $OUT | head -60"
