# Snap Store listing: guitar-fx

Dashboard: https://snapcraft.io/guitar-fx/listing (log in with `snapcraft login`, or use the web form).
The summary, description and title below are also inside the snap (`meta/snap.yaml`, made by
`packaging/linux/snap.sh`); the dashboard values override them.

| Field | Value |
| --- | --- |
| Name | `guitar-fx` |
| Title | guitar-fx |
| Summary (max 78) | Guitar multi-effect processor with a practice band and live monitoring |
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

An ordered, repeatable chain of guitar effects with live monitoring, built for low delay, and a synthesized practice band that plays along.

**Tone**
- Five amp models (clean, crunch, brit, lead, modern) with gain, bass, mid, treble, presence and master. They are modelled amps, not captures of specific hardware.
- Four built-in cabinets, or load your own impulse responses (.wav files).
- 24 effects: overdrive, compressor, noise gate, EQ, chorus, flanger, phaser, tremolo, auto-wah, echo, delay, reverb, harmonics, pitch shift, autotune, vocoder, transient shaper, shimmer, wind, bitcrusher, ring modulator and reverse.

**Band**
- A multitrack mixer on one timeline: drums plus bass, rhythm, lead and solo guitar and organ.
- 20 drum loops with an editable step grid. Give each track its roots by recording them (with a 1-2 measure count-in) or by placing them, then pick a style: 14 bass, 13 rhythm, 12 lead, 10 solo, 10 organ.
- Or record a take of your own playing on any track. Mute, solo, volume and a meter per track.
- View the tracks as a waveform, as music notation (clefs, key, time signature, tempo) or as letters with bar and beat numbers.
- Save band setups in the My bands list. The Listener names the notes, chord, key and tempo you play.

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
