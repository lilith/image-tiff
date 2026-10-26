#!/usr/bin/env bash
# Generate the parity matrix: base images (gen_base.py) re-encoded by libtiff's tiffcp with every
# compression x predictor x layout it accepts. Usage: parity/gen_matrix.sh <out-dir>
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$1
PREFIX=$("$ROOT/parity/build-libtiff.sh")
TIFFCP="$PREFIX/bin/tiffcp"
mkdir -p "$OUT/base" "$OUT/matrix"
uv run --quiet --with numpy --with tifffile --with imagecodecs python -I "$ROOT/parity/gen_base.py" "$OUT/base"
CODECS="none packbits lzw lzw:2 zip zip:2 zip:3 lzma lzma:2 zstd zstd:2 zstd:3 lerc lerc:0:s1 lerc:0:s2 webp jpeg jpeg:r g3 g3:2d g3:2d:fill g4 jbig sgilog"
LAYOUTS="s1:-r_1 s7:-r_7 tiles:-t_-w_16_-l_16 planar:-p_separate_-r_5 planartiles:-p_separate_-t_-w_16_-l_16"
made=0; refused=0
for base in "$OUT"/base/*.tif; do
  name=$(basename "$base" .tif)
  for codec in $CODECS; do
    for layout in $LAYOUTS; do
      lname=${layout%%:*}; largs=${layout#*:}; largs=${largs//_/ }
      out="$OUT/matrix/${name}__${codec//:/-}__${lname}.tif"
      # shellcheck disable=SC2086
      if "$TIFFCP" -c "$codec" $largs "$base" "$out" >/dev/null 2>&1; then
        made=$((made + 1))
      else
        rm -f "$out"; refused=$((refused + 1))
      fi
    done
  done
done
echo "matrix: $made files written, $refused combinations refused by tiffcp"
