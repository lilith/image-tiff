#!/usr/bin/env bash
# Generate the jbigkit-encoded JBIG parity sets with parity/jbig_tiff.c.
# Usage: parity/gen_jbig.sh <out-dir>   (writes <out-dir>/single and <out-dir>/layers)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$1
PREFIX=$("$ROOT/parity/build-libtiff.sh")
TOOL="$OUT/jbig_tiff"
mkdir -p "$OUT/single" "$OUT/layers"
gcc -O2 -Wall -o "$TOOL" "$ROOT/parity/jbig_tiff.c" -I"$PREFIX/include" -L"$PREFIX/lib" \
  -Wl,-rpath,"$PREFIX/lib" -ltiff -ljbig
# Lowest layer only: sizes x strips (0 = one) x stripe heights x MX x options.
for dims in "1728 300" "37 23" "1 9" "7 40" "1000 129"; do
  set -- $dims
  for rps in 0 50; do for l0 in 1 16 128; do for mx in 0 8 127; do
    for flags in - tpbon lrltwo lrltwo,tpbon sdrst tpbon,sdrst lrltwo,tpbon,sdrst delayat,tpbon fill2,tpbon newlen,tpbon; do
      "$TOOL" "$OUT/single/jb_w$1_h$2_r${rps}_l${l0}_m${mx}_${flags//,/+}.tif" "$1" "$2" "$rps" "$l0" "$mx" 0 "$flags"
    done
  done; done; done
done
# Differential layers.
for dims in "1728 300" "1000 129" "37 23" "7 40" "1 9"; do
  set -- $dims
  for layers in 1 2 4; do for l0 in 2 8 64; do
    for flags in tpbon tpdon dpon tpdon,dpon tpbon,tpdon,dpon lrltwo,tpbon,tpdon,dpon tpbon,tpdon,dpon,sdrst - delayat,tpdon,dpon; do
      "$TOOL" "$OUT/layers/jl_w$1_h$2_d${layers}_l${l0}_${flags//,/+}.tif" "$1" "$2" 0 "$l0" 8 "$layers" "$flags"
    done
  done; done
done
echo "jbig: $(ls "$OUT/single" | wc -l) single-layer and $(ls "$OUT/layers" | wc -l) layered files"
