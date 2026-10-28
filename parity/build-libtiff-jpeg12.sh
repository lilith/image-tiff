#!/usr/bin/env bash
# Build the 12-bit JPEG parity oracle: libjpeg-turbo 3.2.0 (whose 8/12-bit dual mode libtiff
# uses for 12-bit JPEG) and libtiff 4.7.2 against it, into a cache directory. The default oracle
# (build-libtiff.sh) uses the system libjpeg-turbo, which has no 12-bit support. Its jpeglib.h
# goes first on the include path: libtiff's dual-mode check otherwise sees the system header.
# Prints the install prefix. Usage: parity/build-libtiff-jpeg12.sh [prefix]
set -euo pipefail
TIFF_VERSION=4.7.2
TIFF_SHA256=4996f0c4f93094719b1ca5c6279b20e588773ba8a247533e486416fb662ddb88
TURBO_VERSION=3.2.0
TURBO_SHA256=6f30092cef9fb839779646608f4ee14ae3cbac989c47fa05e841b0841f09878e  # as zenjpeg's oracle
PREFIX=${1:-${XDG_CACHE_HOME:-$HOME/.cache}/image-tiff-parity/libtiff-$TIFF_VERSION-turbo-$TURBO_VERSION}
if [ -x "$PREFIX/bin/tiffcp" ]; then echo "$PREFIX"; exit 0; fi
WORK=$(mktemp -d "${TMPDIR:-$HOME/tmp}/libtiff-jpeg12-build.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
curl -sSfL -o turbo.tar.gz \
  "https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/$TURBO_VERSION/libjpeg-turbo-$TURBO_VERSION.tar.gz"
echo "$TURBO_SHA256  turbo.tar.gz" | sha256sum -c - >&2
tar xzf turbo.tar.gz
cmake -S "libjpeg-turbo-$TURBO_VERSION" -B turbo -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_INSTALL_LIBDIR=lib -DENABLE_STATIC=OFF \
  -DCMAKE_INSTALL_RPATH="$PREFIX/lib" >&2
cmake --build turbo -j"${CARGO_BUILD_JOBS:-4}" >&2
cmake --install turbo >&2
curl -sSfLO "https://download.osgeo.org/libtiff/tiff-$TIFF_VERSION.tar.xz"
echo "$TIFF_SHA256  tiff-$TIFF_VERSION.tar.xz" | sha256sum -c - >&2
tar xf "tiff-$TIFF_VERSION.tar.xz"
cmake -S "tiff-$TIFF_VERSION" -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX" \
  -Dtiff-docs=OFF -Dtiff-tests=OFF -Dtiff-contrib=OFF -Dtiff-cxx=OFF \
  -Djpeg=ON -Dold-jpeg=ON -Djpeg12=OFF -Dzlib=ON -Dlzma=OFF -Dzstd=OFF -Dwebp=OFF -Djbig=OFF \
  -Dlerc=OFF -DJPEG_INCLUDE_DIR="$PREFIX/include" -DJPEG_LIBRARY="$PREFIX/lib/libjpeg.so" \
  -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_C_FLAGS="-I$PREFIX/include" \
  -DCMAKE_INSTALL_RPATH="$PREFIX/lib" >&2
cmake --build build -j"${CARGO_BUILD_JOBS:-4}" >&2
grep -q "HAVE_JPEGTURBO_DUAL_MODE_8_12 1" build/libtiff/tif_config.h \
  || { echo "libtiff did not detect libjpeg-turbo's 8/12-bit dual mode" >&2; exit 1; }
cmake --install build >&2
echo "$PREFIX"
