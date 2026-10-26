#!/usr/bin/env bash
# Decode every TIFF under the given files/directories with libtiff and with image-tiff, and
# compare the samples. Usage: parity/run.sh <out-dir> <file-or-dir>...
# Writes <out-dir>/dumps/ and <out-dir>/results.tsv; prints the class counts.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$1; shift
mkdir -p "$OUT/dumps"
PREFIX=$("$ROOT/parity/build-libtiff.sh")
gcc -O2 -Wall -o "$OUT/libtiff_dump" "$ROOT/parity/libtiff_dump.c" \
  -I"$PREFIX/include" -L"$PREFIX/lib" -ltiff -Wl,-rpath,"$PREFIX/lib"
cargo build --quiet --release --all-features --example parity_dump --manifest-path "$ROOT/Cargo.toml"
# Run a copy, so rebuilding during the sweep cannot change the binary under it, and record the
# commit it was built from (plus whether the working copy had changes).
FORK="$OUT/parity_dump"
cp "$ROOT/target/release/examples/parity_dump" "$FORK"
BUILT="$(git -C "$ROOT" rev-parse HEAD) dirty-files=$(git -C "$ROOT" status --porcelain --untracked-files=no | wc -l)"
LIST="$OUT/inputs.txt"
: > "$LIST"
: > "$OUT/keys.tsv"
for arg in "$@"; do
  if [ -d "$arg" ]; then
    find "$arg" -type f \( -iname '*.tif' -o -iname '*.tiff' \) | sort >> "$LIST"
  else
    echo "$arg" >> "$LIST"
  fi
done
i=0
while IFS= read -r f; do
  key=$(printf '%05d_%s' "$i" "$(basename "$f")")
  echo "$key	$f" >> "$OUT/keys.tsv"
  timeout 60 "$OUT/libtiff_dump" "$f" "$OUT/dumps/$key.libtiff" >/dev/null 2>&1 \
    || echo "decoded=0\nmessages=libtiff_dump exit $?" > "$OUT/dumps/$key.libtiff.timeout"
  timeout 60 "$FORK" "$f" "$OUT/dumps/$key.fork" >/dev/null 2>&1 \
    || echo "fork exit $?" > "$OUT/dumps/$key.fork.timeout"
  i=$((i + 1))
done < "$LIST"
{
  echo "# $("$OUT/libtiff_dump" --version | head -1)"
  echo "# image-tiff $BUILT, finished $(date -u +%Y-%m-%dT%H:%M:%SZ) on $(hostname)"
} > "$OUT/header.txt"
uv run --quiet --with numpy python -I "$ROOT/parity/compare.py" "$OUT/dumps" "$OUT/results.tsv"
