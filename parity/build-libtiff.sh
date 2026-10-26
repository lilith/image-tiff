#!/usr/bin/env bash
# Build the libtiff used as the parity oracle, with every codec the system provides
# (libjpeg-turbo, zlib, libdeflate, LZMA, ZSTD, WebP, JBIG, LERC), into a cache directory.
# Prints the install prefix. Usage: parity/build-libtiff.sh [prefix]
set -euo pipefail
VERSION=4.7.2
SHA256=4996f0c4f93094719b1ca5c6279b20e588773ba8a247533e486416fb662ddb88  # recorded 2026-10-26
PREFIX=${1:-${XDG_CACHE_HOME:-$HOME/.cache}/image-tiff-parity/libtiff-$VERSION}
if [ -x "$PREFIX/bin/tiffcp" ]; then echo "$PREFIX"; exit 0; fi
WORK=$(mktemp -d "${TMPDIR:-$HOME/tmp}/libtiff-build.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
curl -sSfLO "https://download.osgeo.org/libtiff/tiff-$VERSION.tar.xz"
echo "$SHA256  tiff-$VERSION.tar.xz" | sha256sum -c - >&2
tar xf "tiff-$VERSION.tar.xz"
cmake -S "tiff-$VERSION" -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -Dtiff-docs=OFF -Dtiff-tests=OFF -Dtiff-contrib=OFF -Dtiff-cxx=OFF \
  -Djpeg=ON -Dzlib=ON -Dlibdeflate=ON -Dlzma=ON -Dzstd=ON -Dwebp=ON -Djbig=ON -Dlerc=ON \
  -Dold-jpeg=ON -Djpeg12=OFF -DCMAKE_INSTALL_RPATH="$PREFIX/lib" >&2
cmake --build build -j"${CARGO_BUILD_JOBS:-4}" >&2
cmake --install build >&2
echo "$PREFIX"
