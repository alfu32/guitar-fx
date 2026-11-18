#!/usr/bin/env bash
# usage: appimage.sh VERSION TARGET  -> dist/guitar-fx-VERSION-<arch>.AppImage
# Uses checksum-pinned appimagetool and type2 runtime (same pins as the other K3D0 projects).
set -euo pipefail
V=$1; T=$2; ROOT=$(cd "$(dirname "$0")/../.." && pwd); REL="$ROOT/target/$T/release"
case $T in
  aarch64*) ARCH=aarch64
    TOOL_SHA=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
    RT_SHA=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444;;
  *) ARCH=x86_64
    TOOL_SHA=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
    RT_SHA=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d;;
esac
TOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-$ARCH.AppImage
RT_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-$ARCH
S=$(mktemp -d)
fetch() { curl -fsSL --retry 5 --retry-all-errors "$1" -o "$2"; echo "$3  $2" | sha256sum -c - >/dev/null; chmod +x "$2"; }
fetch "$TOOL_URL" "$S/appimagetool" "$TOOL_SHA"; fetch "$RT_URL" "$S/runtime" "$RT_SHA"

A="$S/guitar-fx.AppDir"; mkdir -p "$A/usr/bin" "$A/usr/lib"
install -m755 "$REL/guitar-fx" "$REL/guitar-fx-ui" "$A/usr/bin/"
# Bundle shared libraries other than glibc; AppRun uses them only when the host lacks libasound.
for b in "$REL/guitar-fx" "$REL/guitar-fx-ui"; do
  ldd "$b" | awk '/=> \//{print $3}' | grep -vE '/(libc|libm|libdl|libpthread|librt|libresolv|libutil|libgcc_s)\.so' | while read -r l; do cp -Ln "$l" "$A/usr/lib/" 2>/dev/null || true; done
done
cp -r "$ROOT/presets" "$A/usr/presets"
cat > "$A/AppRun" <<'R'
#!/bin/sh
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if ! /sbin/ldconfig -p 2>/dev/null | grep -q 'libasound\.so\.2'; then
  export LD_LIBRARY_PATH="$here/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
if [ "${1:-}" = "--cli" ]; then shift; exec "$here/usr/bin/guitar-fx" "$@"; fi
exec "$here/usr/bin/guitar-fx-ui" "$@"
R
chmod 755 "$A/AppRun"
cp "$ROOT/packaging/linux/guitar-fx.desktop" "$A/guitar-fx.desktop"
cp "$ROOT/packaging/guitar-fx.png" "$A/guitar-fx.png"; cp "$ROOT/packaging/guitar-fx.png" "$A/.DirIcon"
mkdir -p "$ROOT/dist"
OUTF="$ROOT/dist/guitar-fx-$V-$ARCH.AppImage"
ARCH=$ARCH APPIMAGE_EXTRACT_AND_RUN=1 "$S/appimagetool" --runtime-file "$S/runtime" --no-appstream --comp zstd "$A" "$OUTF"
chmod 755 "$OUTF"; ls -l "$OUTF"
