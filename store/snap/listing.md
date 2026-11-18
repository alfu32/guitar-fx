# Snap Store listing: guitar-fx

Dashboard: https://snapcraft.io/guitar-fx/listing (log in with `snapcraft login`, or use the web form).
The summary, description and title below are also inside the snap (`meta/snap.yaml`, made by
`packaging/linux/snap.sh`); the dashboard values override them.

| Field | Value |
| --- | --- |
| Name | `guitar-fx` |
| Title | guitar-fx |
| Summary (max 78) | Multi-effect guitar processor with live, low-latency monitoring |
| Category | Music and audio |
| Website | https://github.com/alfu32/guitar-fx |
| Source code | https://github.com/alfu32/guitar-fx |
| Issues | https://github.com/alfu32/guitar-fx/issues |
| Contact | https://github.com/alfu32/guitar-fx/issues |
| Privacy policy | https://github.com/alfu32/guitar-fx/blob/master/PRIVACY.md |
| License | **not decided: the repo has no LICENSE file**, and `snap.sh` currently declares MIT. Pick one and add the file. |
| Icon | `icon-512.png` (512x512 PNG) |
| Banner | `banner-1920x640.png` (3:1) |
| Screenshots | `screenshot1.png` ... `screenshot6.png` (1920x1080; the store takes up to 5 per listing, so use 1-5) |

## Description

An ordered, repeatable chain of guitar effects with live monitoring, built for low delay.

**Tone**
- Five amp models (clean, crunch, brit, lead, modern) with gain, bass, mid, treble, presence and master. They are modelled amps, not captures of specific hardware.
- Four built-in cabinets, or load your own impulse responses (.wav files).
- 24 effects: overdrive, compressor, noise gate, EQ, chorus, flanger, phaser, tremolo, auto-wah, echo, delay, reverb, harmonics, pitch shift, autotune, vocoder, transient shaper, shimmer, wind, bitcrusher, ring modulator and reverse.

**Workflow**
- Drag cards to reorder, bypass any effect, repeat an effect as often as you like, and change parameters live.
- More than 20 ready-made chains; save your own, and your last session is restored.
- Tuner, metronome with tap tempo, and a 60-second looper with overdub.

**After installing**, audio access may need to be connected once (untested on a real system):

    sudo snap connect guitar-fx:audio-record
    sudo snap connect guitar-fx:alsa

Actual latency depends on your audio hardware and buffer size.

## Upload

    snapcraft upload --release=stable guitar-fx_X.Y.Z_amd64.snap   # the release assets are named *_x86_64.snap
    snapcraft upload --release=stable guitar-fx_X.Y.Z_aarch64.snap

Rename the files to the architecture the store expects only if the upload rejects them; the architecture is
read from `meta/snap.yaml` (`amd64` / `arm64`), not from the file name.
