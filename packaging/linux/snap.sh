#!/usr/bin/env bash
# usage: snap.sh VERSION TARGET  -> dist/guitar-fx_VERSION_<arch>.snap
# Assembles a core24 snap from the release binaries with `snap pack` (no snapcraft/LXD, no store access).
set -euo pipefail
V=$1; T=$2; ROOT=$(cd "$(dirname "$0")/../.." && pwd); REL="$ROOT/target/$T/release"
case $T in aarch64*) SA=arm64; ARCH=aarch64;; *) SA=amd64; ARCH=x86_64;; esac
TRIPLET=$(gcc -dumpmachine)
S=$(mktemp -d); R="$S/snap"; mkdir -p "$R/bin" "$R/lib" "$R/meta/gui" "$R/usr/share"
install -m755 "$REL/guitar-fx" "$REL/guitar-fx-ui" "$R/bin/"
# Bundle shared libraries that core24 does not provide (everything except glibc itself).
bundle() { ldd "$1" | awk '/=> \//{print $3}' | grep -vE '/(libc|libm|libdl|libpthread|librt|libresolv|libutil|libgcc_s)\.so' | while read -r l; do cp -Ln "$l" "$R/lib/" 2>/dev/null || true; done; }
bundle "$REL/guitar-fx"; bundle "$REL/guitar-fx-ui"
# ALSA configuration and plugins (so the default device can route through PulseAudio/PipeWire).
cp -r /usr/share/alsa "$R/usr/share/alsa"
mkdir -p "$R/alsa-lib"; cp -L /usr/lib/$TRIPLET/alsa-lib/*.so "$R/alsa-lib/" 2>/dev/null || true
for p in "$R"/alsa-lib/*.so; do [ -e "$p" ] && bundle "$p"; done
cat > "$R/bin/launch" <<'L'
#!/bin/sh
export XDG_CONFIG_HOME="$SNAP_USER_COMMON/.config"
export ALSA_CONFIG_PATH="$SNAP/usr/share/alsa/alsa.conf"
export ALSA_PLUGIN_DIR="$SNAP/alsa-lib"
export LD_LIBRARY_PATH="$SNAP/lib:$SNAP/gpu-2404/lib:$LD_LIBRARY_PATH"
exec "$SNAP/bin/$1" "${@:2}"
L
chmod 755 "$R/bin/launch"
cp "$ROOT/packaging/guitar-fx.png" "$R/meta/gui/guitar-fx.png"
sed 's|^Icon=.*|Icon=${SNAP}/meta/gui/guitar-fx.png|; s|^Exec=.*|Exec=guitar-fx|' "$ROOT/packaging/linux/guitar-fx.desktop" > "$R/meta/gui/guitar-fx.desktop"
cat > "$R/meta/snap.yaml" <<Y
name: guitar-fx
version: '$V'
summary: Guitar multi-effect processor with a practice band and live monitoring
description: |
  A chain of guitar effects (amp models, Neural Amp Modeler captures, cabinet
  impulse responses, noise gate, EQ, modulation, delay, reverb, pitch effects)
  with live parameter changes, tuner, looper and metronome, plus a practice band:
  drums, bass, rhythm, lead and solo guitar and organ on one timeline, shown as
  waveform, music notation or letters. Save band setups next to your chains.
base: core24
architectures: [$SA]
grade: stable
confinement: strict
license: MIT
plugs:
  gpu-2404:
    interface: content
    content: gpu-2404
    default-provider: mesa-2404
    target: \$SNAP/gpu-2404
apps:
  guitar-fx:
    command: bin/launch guitar-fx-ui
    plugs: [home, removable-media, desktop, desktop-legacy, wayland, x11, opengl, gpu-2404, audio-playback, audio-record, alsa]
  cli:
    command: bin/launch guitar-fx
    plugs: [home, removable-media, audio-playback, audio-record, alsa]
Y
mkdir -p "$ROOT/dist"
snap pack "$R" "$S" --compression=xz
mv "$S/guitar-fx_${V}_${SA}.snap" "$ROOT/dist/guitar-fx_${V}_${ARCH}.snap"
