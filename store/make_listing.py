#!/usr/bin/env python3
"""Fill the Partner Center listing export (all fields empty) -> store/msstore/listing.csv (upload the store/msstore folder).
usage: make_listing.py EXPORT.csv"""
import csv, sys, pathlib

HERE = pathlib.Path(__file__).parent
ROOT = "msstore/"   # paths start with the name of the uploaded folder (store/msstore), as in the other store listings
REPO = "https://github.com/alfu32/guitar-fx"
DESCRIPTION = """guitar-fx is a multi-effect processor and practice band for electric guitar. Plug your guitar into an audio interface, build a chain of effects, hear it live while you play, and let a synthesized band play along. Everything runs on your computer: no account, no internet, no subscription.

BUILD YOUR TONE
- Amp models: five voicings (clean, crunch, brit, lead and modern high gain) with gain, bass, mid, treble, presence and master controls. They are modelled amps, not captures of specific hardware.
- Neural Amp Modeler: load your own .nam captures and run them live.
- Cabinets: four built-in speaker cabinets, or load your own impulse responses (.wav files).
- Pedals and studio effects: overdrive, compressor, noise gate, EQ, chorus, flanger, phaser, tremolo, auto-wah, echo, delay, reverb, harmonics, pitch shift, autotune, vocoder, arpeggiator and a transient shaper.
- Ambient and experimental: shimmer, wind, bitcrusher, ring modulator and reverse.

A BAND THAT FOLLOWS YOU
The Band panel is a multitrack mixer on one timeline: a drum track plus bass, rhythm guitar, lead guitar, solo guitar and organ.
- Pick a drum loop from 20 rhythms and edit its step grid, or tap in your own pattern.
- Give each track its roots: record them by playing (with a one or two measure count-in) or click the lane to place them.
- Choose a style for each track from a catalog: 14 bass styles, 13 rhythm guitar, 12 lead, 10 solo and 10 organ styles. The band plays your chords in that style.
- Record a take of your own playing on any track instead, and loop it with the band.
- Mute, solo and balance every track, with a level meter on each.
- Save whole band setups in the My bands list, next to your saved chains.

SEE IT THREE WAYS
Switch the tracks between a waveform with the live picture of what each one played, real music notation (clefs, key signature, time signature, tempo and measures) and a letter view that shows the notes C D E F G A B against time with bar and beat numbers.

LISTENER
The Listener names the notes you play, finds the chord, key and tempo, and feeds the Band so it can follow your playing.

A CHAIN YOU CAN SEE
Every effect is a colour-coded card with its own icon and controls. Drag cards to reorder them, bypass any of them, use the same effect more than once, and watch each one's level as you play. Parameters change live, without dropouts.

PLAY WITH CONFIDENCE
- Live monitoring built for low delay, with a buffer size you choose. The app watches the queue between input and output and trims any delay that builds up during long sessions.
- Tuner with note, cents and a needle, which can mute the output while you tune.
- Metronome with tap tempo, accent and a beat light.
- Looper: record, overdub, stop and clear, up to 60 seconds.

START QUICKLY
More than 20 ready-made chains, from clean sparkle and crunch rhythm to ambient wash and high-gain amp chains. Save your own chains and band setups; your last session is restored the next time you open the app.

NOTES
Actual latency depends on your audio interface, its Windows driver and the buffer size. A USB audio interface with a guitar (Hi-Z) input and headphones are recommended; speakers near an open microphone can cause feedback. The Listener needs single notes, and works best from about 60 Hz up.
"""
FEATURES = [
    "Band panel: drums, bass, rhythm, lead and solo guitar and organ on one timeline",
    "Record your own roots with a count-in, or place them; pick a style per track from a catalog",
    "Tracks as a waveform, real music notation or letters with bar and beat numbers",
    "Save band setups next to your saved effect chains",
    "Five amp models, Neural Amp Modeler (.nam) captures and four built-in cabinets",
    "Own .wav impulse responses for cabinets",
    "Drag-to-reorder effect chain with colour-coded cards and per-effect level meters",
    "Listener: names the notes, chord, key and tempo you play",
    "Tuner, metronome with tap tempo, and a looper with overdub",
    "Works offline: no account, no ads, no data collected",
]
LICENSE = """guitar-fx is provided "as is", without warranty of any kind, express or implied. You may use it for personal and commercial music making. K3D0 is not liable for any damage, data loss or hearing damage arising from its use. Keep your output volume at a safe level, especially with headphones.

Privacy: the app does not collect or transmit personal data. See https://github.com/alfu32/guitar-fx/blob/master/PRIVACY.md"""
SHORT = "A guitar multi-effect processor with a practice band: amps, cabinets, pedals, drums, bass, guitars and organ, with live monitoring."
RELEASE = """New Band panel: a drum track plus bass, rhythm, lead and solo guitar and organ on one timeline. Record roots with a count-in or place them, pick a style per track, record your own takes, and view the tracks as a waveform, music notation or letters.
Save band setups in the new My bands list. New Listener window, resizable side panels, Neural Amp Modeler support."""
MINHW = ["Windows 10 version 1809 or later (x64 or ARM64)", "An audio input device (guitar audio interface or microphone) and an output device",
         "Graphics with DirectX 12 or Vulkan support", "4 GB of memory"]
RECHW = ["USB audio interface with a guitar (Hi-Z) input", "Headphones", "A multi-core processor"]
TERMS = ["guitar effects", "guitar amp simulator", "guitar pedalboard", "guitar tuner", "looper", "backing band", "drum machine"]
CAPTIONS = [
    "The Band panel: a drum loop and five played tracks on one timeline, shown as waveforms with the live sound of each track.",
    "The same band as real music notation: clefs, key signature, time signature, tempo and measures for every track.",
    "The letter view: notes C to B against time, with bar and beat numbers and chord names.",
    "A lead chain: compressor, hot overdrive, delay, echo and reverb, with 20+ built-in chains on the right.",
    "A modern high-gain chain with noise gate, amp, cabinet and EQ.",
    "An ambient chain with compressor, chorus, echo and a long reverb.",
]
VALUES = {"Description": DESCRIPTION, "ReleaseNotes": RELEASE, "Title": "guitar-fx", "ShortTitle": "guitar-fx",
          "SortTitle": "guitar-fx", "VoiceTitle": "guitar effects", "ShortDescription": SHORT, "DevStudio": "K3D0",
          "CopyrightTrademarkInformation": "© 2026 K3D0", "AdditionalLicenseTerms": LICENSE,
          "StoreLogo720x1080": ROOT + "images/poster-720x1080.png", "StoreLogo1080x1080": ROOT + "images/logo-1080x1080.png",
          "StoreLogo300x300": ROOT + "images/logo-300x300.png", "PromoImage1920x1080": ROOT + "images/promo-1920x1080.png",
          "PromoImage2400x1200": ROOT + "images/promo-2400x1200.png"}
for i, f in enumerate(FEATURES, 1): VALUES[f"Feature{i}"] = f
for i, f in enumerate(MINHW, 1): VALUES[f"MinimumHardwareReq{i}"] = f
for i, f in enumerate(RECHW, 1): VALUES[f"RecommendedHardwareReq{i}"] = f
for i, f in enumerate(TERMS, 1): VALUES[f"SearchTerm{i}"] = f
for i, c in enumerate(CAPTIONS, 1):
    VALUES[f"DesktopScreenshot{i}"] = ROOT + f"images/screenshot{i}.png"; VALUES[f"DesktopScreenshotCaption{i}"] = c

src = pathlib.Path(sys.argv[1]); rows = list(csv.reader(open(src, encoding="utf-8-sig", newline="")))
used = set()
for r in rows[1:]:
    if r and r[0] == 'OverrideLogosForWin10': r[3:] = ['False'] * (len(r) - 3)
    if r and r[0] in VALUES and VALUES[r[0]] != "":
        r[3:] = [VALUES[r[0]]] * (len(r) - 3); used.add(r[0])   # default + every language column identical
assert used == {k for k, v in VALUES.items() if v}, set(VALUES) - used
for k in ("Title", "ShortTitle"): assert len(VALUES[k]) <= 100
assert len(DESCRIPTION) <= 10000 and all(len(f) <= 200 for f in FEATURES) and all(len(c) <= 200 for c in CAPTIONS)
assert all(len(t) <= 30 for t in TERMS) and sum(len(t.split()) for t in TERMS) <= 21 and len(SHORT) <= 1000
out = HERE / "msstore" / "listing.csv"
with open(out, "w", encoding="utf-8-sig", newline="") as f: csv.writer(f).writerows(rows)
print("filled", len(used), "fields")
