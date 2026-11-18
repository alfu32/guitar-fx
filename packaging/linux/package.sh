#!/usr/bin/env bash
# usage: package.sh VERSION TARGET   -> dist/*.tar.gz and dist/*.deb
set -euo pipefail
V=$1; T=$2; ROOT=$(cd "$(dirname "$0")/../.." && pwd); REL="$ROOT/target/$T/release"
case $T in aarch64*) DEBARCH=arm64; ARCH=aarch64;; *) DEBARCH=amd64; ARCH=x86_64;; esac
OUT="$ROOT/dist"; mkdir -p "$OUT"
S=$(mktemp -d); P="$S/guitar-fx-$V-linux-$ARCH"; mkdir -p "$P"
cp "$REL/guitar-fx" "$REL/guitar-fx-ui" "$ROOT/README.md" "$P"; cp -r "$ROOT/presets" "$P"
tar -C "$S" -czf "$OUT/guitar-fx-$V-linux-$ARCH.tar.gz" "guitar-fx-$V-linux-$ARCH"
D="$S/deb"; mkdir -p "$D/DEBIAN" "$D/usr/bin" "$D/usr/share/applications" "$D/usr/share/icons/hicolor/256x256/apps" "$D/usr/share/doc/guitar-fx"
install -m755 "$REL/guitar-fx" "$REL/guitar-fx-ui" "$D/usr/bin/"
install -m644 "$ROOT/packaging/linux/guitar-fx.desktop" "$D/usr/share/applications/"
install -m644 "$ROOT/packaging/guitar-fx.png" "$D/usr/share/icons/hicolor/256x256/apps/"
install -m644 "$ROOT/README.md" "$D/usr/share/doc/guitar-fx/"
cat > "$D/DEBIAN/control" <<C
Package: guitar-fx
Version: $V
Section: sound
Priority: optional
Architecture: $DEBARCH
Depends: libasound2t64 | libasound2, libvulkan1 | libgl1
Recommends: mesa-vulkan-drivers
Maintainer: K3D0 <noreply@example.invalid>
Description: Multi-effect guitar processor
 Ordered, repeatable effect chain with live low-latency monitoring,
 amp models, cabinet impulse responses, tuner, looper and metronome.
C
dpkg-deb --root-owner-group --build "$D" "$OUT/guitar-fx_${V}_${DEBARCH}.deb"
