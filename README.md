# guitar-fx

An ordered, repeatable multi-effect chain for guitar and voice. Two front ends over one
engine: a command-line tool (`guitar-fx`) and a graphical one (`guitar-fx-ui`). Process WAV
files, or run live (input → effects → output) with a low-latency buffer.

```
cargo build --release                         # both binaries
cargo build --release --no-default-features   # command line only (no GUI dependencies)
./target/release/guitar-fx --help
./target/release/guitar-fx-ui
```

## The UI

`guitar-fx-ui` shows the chain as a stack of effect cards. Press **Start**, play, and drag
sliders: parameter changes reach the audio thread through a command channel and are heard
within one buffer, without resetting the effect (echo tails and reverb keep ringing).
Add effects from the right panel, drag a card by its grip (⠿) to reorder, bypass with the
checkbox, right-click a slider to reset it. Each card shows a level history of that
effect's output. While dragging, the spot between cards under the pointer lights up (the
slots are always reserved, so nothing shifts). The right panel also lists built-in chains
(`presets/builtin/`); your own saved chains are on the left
(`~/.config/guitar-fx/chains/`, `%APPDATA%\guitar-fx` on Windows,
`~/Library/Application Support/guitar-fx` on macOS). The chain, gains, buffer and devices are
restored on the next launch. Meters show input and output peaks; the dropout counter tells you when the
buffer is too small for your machine. Changing device or buffer size needs a Stop/Start.

Parameters marked non-live in the schema (reverb time/size/damping, pitch-shift mode and
window, vocoder bands/carrier) rebuild just that effect when changed, which resets its tail.

## Layout

- `src/params.rs` — one schema (names, ranges, defaults) drives the CLI, presets and the UI
- `src/effects.rs` — the effects; each updates live without losing state
- `src/chain.rs` — the audio-thread chain plus the `Control` command channel and meters
- `src/live.rs` — cpal input/output
- `src/main.rs` (CLI) and `src/bin/guitar-fx-ui.rs` (GUI)

## Examples

```
# live, default devices, 64-frame buffer
guitar-fx --compressor ratio=3 --overdrive drive=0.65 --echo time=380,mix=0.3

# order matters and effects may repeat
guitar-fx --overdrive 0.7 --echo --overdrive 0.2

# file in, file out
guitar-fx -i riff.wav -o riff.fx.wav --chorus --reverb 0.3

# presets
guitar-fx --preset presets/builtin/guitar-crunch.fx
guitar-fx --compressor 3 --overdrive 0.6 --save-preset mine.fx
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
| `--gate` | threshold* dB (-55), attack ms (1), hold ms (60), release ms (120, time to fall ~60 dB), depth dB (-80) |
| `--eq` | mid* dB (0), low dB, high dB, mid-freq Hz (800), mid-q (0.8), lowcut Hz (20 = off), highcut Hz (20000 = off) |
| `--amp` | gain* (0.5), model `clean`/`crunch`/`brit`/`lead`/`modern` (crunch), bass, mid, treble, presence, master (0.5 each), level (1) — modelled tube amp, 2x oversampled; put a cabinet after it |
| `--nam` | model FILE (a `.nam` capture: a path, or a name inside the `nam` config folder), quality `full`/`lite`, input dB (0), level dB (0), normalize `on`/`off` (on), mix (1) — runs a Neural Amp Modeler capture of an amp, pedal or rig as a neural network; no added delay at 48 kHz |
| `--cabinet` | cab* `1x12-open`/`2x12`/`4x12`/`1x8-vintage`, ir FILE (a `.wav` impulse response; name in the `ir` config folder or a path), trim ms (50), mix (1), level dB (0) |
| `--octave` | sub* (0.8), dry (1), tone Hz (350) — the classic analog octave-down: a flip-flop on the note's period makes a sub tone one octave lower. No delay, a few MFLOPS. A small pitch tracker keeps it an octave down while the string rings, even when a harmonic gets louder than the fundamental. Follows one note at a time (play single notes; chords growl) |
| `--arpeggio` | mix* (0.7), chord (major, minor, major7, minor7, sus4, power, dim, octaves, pentatonic, minor-pentatonic), pattern (up, down, bounce, random), rate (1/4, 1/8, 1/8T, 1/16, 1/16T), bpm (100), octaves 1-2, gate (0.6), swing, release ms — turns a single sustained note into an arpeggio of pitch-shifted chord tones (about 10 ms of delay; follows one note, so play single notes or the lowest note of a chord) |
| `--transient` | attack* -1..1 (-0.6; negative softens the pick, positive sharpens), time ms (15), mix (1) — transient shaper |
| `--bitcrusher` | bits* (8), rate Hz (12000), mix (1) |
| `--ringmod` | freq* Hz (150), mix (0.5), shape `sine`/`square` |
| `--reverse` | mix* (0.7), time ms (400) — reversed swells |
| `--wind` | mix* (0.5), level (0.3), follow (0.7), rate Hz (0.15), q (5) — breathy wind that follows your playing |
| `--shimmer` | mix* (0.4), shimmer (0.5), pitch st (12), time s (4), size m (15) — reverb fed back through a pitch shifter |

The experimental and zen effects (bitcrusher, ringmod, reverse, wind, shimmer) are creative extras; shimmer adds about 50 ms to its wet sound only.

Pitch effects have two modes. `high` is the cleanest and works on chords but adds
50+ ms. `fast` locks to the detected note period: about 10 ms, accurate on one note at a
time (guitar down to low E), and it degrades on chords.

## Practice tools (UI)

A second bar under the controls holds a **metronome** (bpm, beats per bar, click volume, tap
tempo, a light that flashes on every beat), a **looper** (Record, then Close loop, Overdub,
Stop, Clear; it records what comes out of the chain, up to 60 s) and a **tuner** (note, cents
and a needle; the output can be muted while tuning). The **Listener** window shows what you are
playing in musical terms: the chord, the key or mode (with how sure it is), the tempo with a beat light, and
the 12 pitch classes. The **Band** window is a small multitrack mixer on one timeline: a drum track and five played tracks (bass, rhythm guitar, lead guitar, solo guitar, organ), locked to one loop
(tempo and 1-8 bars) with a playhead running across all of them. Each track has a volume, mute, solo and a level meter. The
**drum track** is a loop you pick from a catalog of 20 (rock, rock shuffle, four on the floor, half-time, funk, disco,
hip-hop, Motown, reggae, ballad, bossa nova, samba, country, punk, metal, blues shuffle, jazz swing, slow
12/8 blues, waltz, waltz ballad) and can edit: click a step to add a hit, click again for an accent, right-click for ghost notes.
The **bass** and **guitar** tracks each have a source. *Style*: you indicate the root notes, by playing them once over the
loop (the band names each note and where it fell) or by clicking the lane to place them; the band then plays the style you chose
(14 bass styles from whole notes to walking, funk, reggae, boogie; 13 rhythm guitar styles: strums, skanks, chops, arpeggios,
boom-chick...; 12 lead guitar riffs and 10 solo phrases built from the chord's notes, the solo a singing monophonic voice with vibrato; 10 organ styles: held chords, swells, stabs, reggae bubble...) on those roots, changing root exactly where you indicated. *Rec*: one pass of your own playing is recorded
(as it comes out of the effect chain) and looped, shown as a waveform; a take is silent if you change the tempo or length afterwards.
Pressing a record button erases that track's take, returns to the start of the loop, plays a click for the count-in (0-4 bars, default 2) and then records one pass. The mixer sits in a panel at the bottom of the window; the splitters between it, the side panels and the effect chain can be dragged.
The left panel's **My bands** list saves and loads a whole band setup (mixer, styles, drum loop and roots; not recorded takes) in `~/.config/guitar-fx/bands/`, like the saved chains.
Chord types come from the roots (a major or minor key that contains them) or can be set by hand. While recording roots play single notes
(one at a time, a low string works best; it names notes from about 60 Hz up). No samples are used. The Listener
listens to the dry guitar on a separate thread, so it adds no delay; it needs a few
seconds of playing, works best on steady playing, and reads a single line or strummed chords (the key is a
guess between a mode and its relative modes, so a C major and an A minor tune can swap). Neural Amp Modeler captures (`.nam`, from [tone3000.com](https://www.tone3000.com) and elsewhere)
go in `~/.config/guitar-fx/nam/` (`%APPDATA%\guitar-fx\nam`), folders allowed, and appear in the Neural Amp
card. They are not bundled: each capture carries its own licence, so check it before you share one.
Files in the current "slimmable" format (a full and a lite network in one file) and the older WaveNet
and LSTM files load; models with FiLM layers do not. `lite` costs about a third of `full`. Captures
are made at 48 kHz; on a 44.1 kHz interface the signal is resampled around the model (a few samples of delay).
Cabinet impulse responses go in
`~/.config/guitar-fx/ir/` (`%APPDATA%\guitar-fx\ir` on Windows) and appear in the Cabinet card.

## Latency stays flat

Input and output are separate streams with separate clocks, so a CPU stall or clock drift
could leave extra audio queued and the delay would creep up while you play. The live engine
now measures the standing queue every quarter second and trims the excess (one averaged
sample at a time, or in one step after a big stall). The UI shows `queue x ms`, and `-v`
prints it with the number of samples trimmed.

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

## Releases

Pushing a tag `vX.Y.Z` that matches `version` in `Cargo.toml` runs `.github/workflows/release.yml`:
it runs the tests, builds six targets in parallel and publishes one GitHub release.

| Target | Files |
| --- | --- |
| Linux x86_64 / aarch64 | `.tar.gz`, `.deb`, `.snap`, `.AppImage` |
| Windows x64 / ARM64 | `.zip`, `.msix` (Store identity `K3D0.guitar-fx`) |
| macOS Intel / Apple Silicon | `.zip` (app bundle), `.dmg` |

To release: bump `version`, commit, `git tag vX.Y.Z`, `git push origin master --tags`.
To re-run an existing tag: `gh workflow run release.yml --ref vX.Y.Z`.

Store identities are in `packaging/store-identity.json` (Microsoft Store ID `9P816T97D1CC`;
Snap Store name `guitar-fx`). The workflow builds the packages but does not upload them to the
stores. The MSIX is unsigned unless the repository secrets `GUITARFX_PFX_B64` and
`GUITARFX_PFX_PASSWORD` exist; the Store signs submissions itself. macOS bundles are ad-hoc
signed, not notarised.
