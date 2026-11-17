# fxchain

An ordered, repeatable multi-effect chain for guitar and voice. Two front ends over one
engine: a command-line tool (`fxchain`) and a graphical one (`fxchain-ui`). Process WAV
files, or run live (input → effects → output) with a low-latency buffer.

```
cargo build --release                         # both binaries
cargo build --release --no-default-features   # command line only (no GUI dependencies)
./target/release/fxchain --help
./target/release/fxchain-ui
```

## The UI

`fxchain-ui` shows the chain as a stack of effect cards. Press **Start**, play, and drag
sliders: parameter changes reach the audio thread through a command channel and are heard
within one buffer, without resetting the effect (echo tails and reverb keep ringing).
Add effects from the right panel, drag a card by its grip (⠿) to reorder, bypass with the
checkbox, right-click a slider to reset it. Each card shows a level history of that
effect's output. The right panel also lists built-in chains (`presets/builtin/`) and your own
saved chains (`~/.config/fxchain/chains/`, `%APPDATA%\fxchain` on Windows,
`~/Library/Application Support/fxchain` on macOS). The chain, gains, buffer and devices are
restored on the next launch. Meters show input and output peaks; the dropout counter tells you when the
buffer is too small for your machine. Changing device or buffer size needs a Stop/Start.

Parameters marked non-live in the schema (reverb time/size/damping, pitch-shift mode and
window, vocoder bands/carrier) rebuild just that effect when changed, which resets its tail.

## Layout

- `src/params.rs` — one schema (names, ranges, defaults) drives the CLI, presets and the UI
- `src/effects.rs` — the effects; each updates live without losing state
- `src/chain.rs` — the audio-thread chain plus the `Control` command channel and meters
- `src/live.rs` — cpal input/output
- `src/main.rs` (CLI) and `src/bin/fxchain-ui.rs` (GUI)

## Examples

```
# live, default devices, 64-frame buffer
fxchain --compressor ratio=3 --overdrive drive=0.65 --echo time=380,mix=0.3

# order matters and effects may repeat
fxchain --overdrive 0.7 --echo --overdrive 0.2

# file in, file out
fxchain -i riff.wav -o riff.fx.wav --chorus --reverb 0.3

# presets
fxchain --preset presets/builtin/guitar-crunch.fx
fxchain --compressor 3 --overdrive 0.6 --save-preset mine.fx
```

## Effects

Each takes `name=value,name=value`; a bare number sets the first parameter (marked `*`).

| Flag | Parameters (default) |
|---|---|
| `--overdrive` | drive* 0..1 (0.5), tone 0..1 (0.6), level 0..2 (1) |
| `--compressor` | ratio* (4), threshold dB (-18), attack ms (10), release ms (120), makeup dB or `auto` |
| `--echo` | mix* (0.4), time ms (350), feedback (0.45), tone (0.5) — tape-style |
| `--delay` | time* ms (300), feedback (0.3), mix (0.5) — clean |
| `--chorus` | depth* (0.5), rate Hz (0.6), mix (0.5) |
| `--flanger` | depth* (0.7), rate (0.3), feedback (0.5), mix (0.5) |
| `--phaser` | depth* (0.7), rate (0.5), feedback (0.5), mix (0.5) |
| `--tremolo` | depth* (0.5), rate Hz (5) |
| `--wah` | sensitivity* (0.6), rate Hz (0 = follows picking), q (5), min Hz (350), max Hz (2200), mix (1) |
| `--harmonics` | mix* (0.3), drive (0.5), freq Hz (2000) — exciter |
| `--reverb` | mix* (0.3), time s (2), size m (10), damping (0.5) |
| `--tone-shift` | semitones* (0), cents (0), mix (1), mode `high`/`fast`, window ms (10, fast only) |
| `--autotune` | amount* (1), key (C), scale (chromatic, major, minor, pentatonic, minor-pentatonic, blues), speed ms (40; 0 = hard), mode, window |
| `--vocoder` | mix* (1), bands (16), carrier Hz (110), release ms (15) |

Pitch effects have two modes. `high` is the cleanest and works on chords but adds
50+ ms. `fast` locks to the detected note period: about 10 ms, accurate on one note at a
time (guitar down to low E), and it degrades on chords.

## Options

`--input-gain DB`, `--output-gain DB`, `--preset FILE`, `--save-preset FILE`,
`--buffer FRAMES` (live, default 64), `--input-device`, `--output-device`, `--channel N`,
`--tail SECONDS` (file mode), `-v` (live level meter), `--list-devices`.

## Low latency on PipeWire

`--buffer` sets `PIPEWIRE_LATENCY` for you. On a USB interface the device's own period and
headroom usually dominate; for a Momix-Cab this WirePlumber rule cut them from 512 frames to 48:

```
# ~/.config/wireplumber/wireplumber.conf.d/51-momix-lowlatency.conf
monitor.alsa.rules = [{
  matches = [{ node.name = "~alsa_(input|output)\\.usb-ShenZhen-JOYO.*" }]
  actions = { update-props = {
    api.alsa.period-size = 64
    api.alsa.period-num  = 3
    api.alsa.headroom    = 0
  } }
}]
```

Underruns while playing mean the CPU is too busy for the buffer: raise `--buffer` to 128.

## Tests

`cargo test --release` also sweeps every live parameter of every effect and checks the control
channel (set, insert, move, remove, bypass). It checks pitch accuracy (fast and high shifters, autotune, detector across
the guitar range), effect order and repetition, and parameter validation.
