#!/usr/bin/env python3
"""Fill the Partner Center listing export (all fields empty) -> store/msstore/listing.csv (upload the store/msstore folder).
usage: make_listing.py EXPORT.csv"""
import csv, sys, pathlib

HERE = pathlib.Path(__file__).parent
REPO = "https://github.com/alfu32/guitar-fx"
DESCRIPTION = """guitar-fx is a multi-effect processor for electric guitar. Plug your guitar into an audio interface, build a chain of effects, and hear it live while you play. Everything runs on your computer: no account, no internet, no subscription.

BUILD YOUR TONE
- Amp models: five voicings (clean, crunch, brit, lead and modern high gain) with gain, bass, mid, treble, presence and master controls. They are modelled amps, not captures of specific hardware.
- Cabinets: four built-in speaker cabinets, or load your own impulse responses (.wav files).
- Pedals and studio effects: overdrive, compressor, noise gate, EQ, chorus, flanger, phaser, tremolo, auto-wah, echo, delay, reverb, harmonics, pitch shift, autotune, vocoder and a transient shaper.
- Ambient and experimental: shimmer, wind, bitcrusher, ring modulator and reverse.

A CHAIN YOU CAN SEE
Every effect is a colour-coded card with its own icon and controls. Drag cards to reorder them, bypass any of them, use the same effect more than once, and watch each one's level as you play. Parameters change live, without dropouts.

PLAY WITH CONFIDENCE
- Live monitoring built for low delay, with a buffer size you choose. The app watches the queue between input and output and trims any delay that builds up during long sessions.
- Tuner with note, cents and a needle, which can mute the output while you tune.
- Metronome with tap tempo, accent and a beat light.
- Looper: record, overdub, stop and clear, up to 60 seconds.

START QUICKLY
More than 20 ready-made chains, from clean sparkle and crunch rhythm to ambient wash and high-gain amp chains. Save your own chains and your last session is restored the next time you open the app.

NOTES
Actual latency depends on your audio interface, its Windows driver and the buffer size. A USB audio interface with a guitar (Hi-Z) input and headphones are recommended; speakers near an open microphone can cause feedback.
"""
FEATURES = [
    "Five amp models: clean, crunch, brit, lead and modern high gain",
    "Four built-in cabinets, plus your own .wav impulse responses",
    "24 effects: drive, dynamics, EQ, modulation, delay, reverb, pitch and ambient",
    "Drag-to-reorder effect chain with colour-coded cards and per-effect level meters",
    "Live parameter changes while you play",
    "Noise gate and 3-band EQ with sweepable mid, low-cut and high-cut",
    "Built-in tuner with note, cents and needle display",
    "Metronome with tap tempo and a looper with overdub",
    "More than 20 ready-made chains; save your own",
    "Works offline: no account, no ads, no data collected",
]
LICENSE = """guitar-fx is provided "as is", without warranty of any kind, express or implied. You may use it for personal and commercial music making. K3D0 is not liable for any damage, data loss or hearing damage arising from its use. Keep your output volume at a safe level, especially with headphones.

Privacy: the app does not collect or transmit personal data. See https://github.com/alfu32/guitar-fx/blob/master/PRIVACY.md"""
SHORT = "A multi-effect guitar processor: amp models, cabinets, pedals, tuner, looper and metronome, with live monitoring."
RELEASE = """Amp models, cabinet impulse responses, noise gate, EQ, tuner, metronome and looper.
New chain button; drag to reorder effects; level graph in each card; last session restored.
Fixed the input-to-output delay that could grow during long sessions with some effects."""
MINHW = ["Windows 10 version 1809 or later (x64 or ARM64)", "An audio input device (guitar audio interface or microphone) and an output device",
         "Graphics with DirectX 12 or Vulkan support", "4 GB of memory"]
RECHW = ["USB audio interface with a guitar (Hi-Z) input", "Headphones", "A multi-core processor"]
TERMS = ["guitar effects", "guitar amp simulator", "guitar pedalboard", "guitar tuner", "looper", "cabinet impulse response", "multi-effects"]
CAPTIONS = [
    "A crunch amp chain: noise gate, amp model, cabinet and reverb. Each effect is a colour-coded card with its own controls.",
    "An ambient chain with compressor, chorus, echo and a long reverb. Saved chains are listed under My chains.",
    "A lead chain: compressor, hot overdrive, slapback delay, echo and reverb, with 20+ built-in chains on the right.",
    "Robot voice: a vocoder and echo, one of the experimental chains.",
    "A modern high-gain chain with noise gate, amp, cabinet and EQ.",
    "A funk chain: squashed compression, envelope wah and a little drive.",
]
VALUES = {"Description": DESCRIPTION, "ReleaseNotes": RELEASE, "Title": "guitar-fx", "ShortTitle": "guitar-fx",
          "SortTitle": "guitar-fx", "VoiceTitle": "guitar effects", "ShortDescription": SHORT, "DevStudio": "K3D0",
          "CopyrightTrademarkInformation": "© 2026 K3D0", "AdditionalLicenseTerms": LICENSE,
          "StoreLogo720x1080": "images/poster-720x1080.png", "StoreLogo1080x1080": "images/logo-1080x1080.png",
          "StoreLogo300x300": "images/logo-300x300.png", "PromoImage1920x1080": "images/promo-1920x1080.png",
          "PromoImage2400x1200": "images/promo-2400x1200.png"}
for i, f in enumerate(FEATURES, 1): VALUES[f"Feature{i}"] = f
for i, f in enumerate(MINHW, 1): VALUES[f"MinimumHardwareReq{i}"] = f
for i, f in enumerate(RECHW, 1): VALUES[f"RecommendedHardwareReq{i}"] = f
for i, f in enumerate(TERMS, 1): VALUES[f"SearchTerm{i}"] = f
for i, c in enumerate(CAPTIONS, 1):
    VALUES[f"DesktopScreenshot{i}"] = f"images/screenshot{i}.png"; VALUES[f"DesktopScreenshotCaption{i}"] = c

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
