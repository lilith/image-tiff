#!/usr/bin/env bash
# Generate 12-bit JPEG TIFFs with the 12-bit oracle libtiff (build-libtiff-jpeg12.sh): 12-bit
# gray and RGB bases (gen_base12.c) re-encoded by tiffcp as JPEG (YCbCr 4:2:0 and raw RGB;
# q30/75/95; strips, one strip and 16x16 tiles). Usage: parity/gen_jpeg12.sh <out-dir>
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$1
PREFIX=$("$ROOT/parity/build-libtiff-jpeg12.sh")
mkdir -p "$OUT/base" "$OUT/matrix"
gcc -O2 -Wall -o "$OUT/gen_base12" "$ROOT/parity/gen_base12.c" -I"$PREFIX/include" -L"$PREFIX/lib" \
  -Wl,-rpath,"$PREFIX/lib" -ltiff
"$OUT/gen_base12" "$OUT/base"
made=0; refused=0
for base in "$OUT"/base/*.tif; do
  name=$(basename "$base" .tif)
  for codec in jpeg jpeg:r jpeg:30 jpeg:r:95 jpeg:95; do
    for layout in "s16:-r 16" "whole:" "tiles:-t -w 16 -l 16"; do
      lname=${layout%%:*}; largs=${layout#*:}
      out="$OUT/matrix/${name}__${codec//:/-}__${lname}.tif"
      # shellcheck disable=SC2086
      if "$PREFIX/bin/tiffcp" -c "$codec" $largs "$base" "$out" 2>/dev/null; then
        made=$((made + 1))
      else
        rm -f "$out"; refused=$((refused + 1))
      fi
    done
  done
done
echo "jpeg12: $made files written, $refused combinations refused by tiffcp"
