# Real-time guitar effects apps: landscape and assessment

Research date: 2026-11-18. Method: web searches restricted to each store plus vendor pages,
docs and release notes. **Evidence quality is mixed**: most latency and "free" claims come
from vendor listings, not measurements, and are marked *(vendor claim)*. Nothing here was
installed or tested. Where a store search found nothing, that is stated, not guessed.

## Summary

- **Microsoft Store** has no heavyweight guitar processor. The category leaders (Guitar Rig,
  AmpliTube, BIAS) ship as their own installers. Store apps are small: Practice Rack,
  Pedalboard, Infinite Amp Lab, Amperium Live, Fender Studio, n-Track, Hyperion DAW.
  None of the Store listings states a latency number.
- **GitHub** is the most active area: Guitarix, PiPedal, Neural Amp Modeler, AIDA-X, BYOD,
  0xFX, Open Riff Box, go-dsp-guitar, AmpForge, Guitar Virtual Amp (Rust), Open-Amp, Amplitron.
- **Snap Store**: no Guitarix snap was found. Guitar content there is thin: `rkrlv2`
  (Rakarrack effects as LV2, flagged by the store as possibly unmaintained), `n-trackstudio`
  (a DAW with guitar amp plugins), and a tuner (`guitarfinetune`). Linux users get Guitarix
  through distro packages, not Snap.
- **Android** is dominated by Deplike, Tonebridge and a few indie apps. Latency over the
  phone's own audio path is the persistent problem; vendors work around it with their own
  USB driver (Deplike) or hardware DSP (iRig UA).
- **Gaps nobody fills well:** a zero-config *CLI/GUI hybrid with the same engine*, pick-aware
  effects (freeze, pick noise, kalimba, swell, transient shaper) built in as first-class
  pedals, and presets that are plain text files.

## Per-app assessment

### Microsoft Store (Windows)

| App | What it is | Features (from listing) | Assessment |
|---|---|---|---|
| [Practice Rack](https://apps.microsoft.com/detail/9pk87fr1mzb3?hl=en-US&gl=US) | Free rack | 16 effects in one chain, neural amp capture, cabinet IRs, tempo-synced delay and phaser, looper, recorder, FFT tuner; recommends small buffers for low-latency monitoring | Closest Store app to guitar-fx in scope. Latency depends on the audio interface and buffer; no numbers. Worth installing to compare. |
| [Pedalboard](https://apps.microsoft.com/detail/9nnhppx0hbxj?hl=en-GB&gl=ZA) | Browser-based pedalboard | About 20 pedals including tube amp and cab, WAV recorder, works offline, no account | Web audio means browser latency; fine for practice, risky for a guitarist sensitive to 10 ms. |
| [Infinite Amp Lab](https://apps.microsoft.com/detail/9pk3g3jsqrtq?hl=en-US&gl=US) | Tone builder and multitrack studio | Simple and Advanced signal-chain modes, live monitoring while recording | Studio-oriented; no latency data. |
| [Amperium Live](https://apps.microsoft.com/detail/9pcnx0d25txp?hl=en-US&gl=US) | Companion app for the Amperium Live amp hardware | Tube-amp emulation with a stereo FX loop, remote control over USB or Bluetooth | Not a standalone processor; tied to hardware. Listing is old (about 1,300 days). |
| [Fender Studio](https://apps.microsoft.com/detail/9nzklscrwrsq?hl=en-US&gl=US) | Jam and record app | Free tier: '65 Twin Reverb plus 4 effects and a tuner; more amps via free registration | Polished and brand-backed; limited free gear. |
| [n-Track Studio](https://apps.microsoft.com/detail/xp8lg4st7h3gbf?hl=en-US&gl=US) | DAW | Guitar Amp, Bass Amp, Tube Distortion, Fuzz, Autowah plugins; free edition limited to 8 tracks, two effects per track | A recorder first; guitar processing is an add-on. Also on the [Snap Store](https://snapcraft.io/n-trackstudio). |
| [Hyperion DAW](https://apps.microsoft.com/detail/9mx3249pj1vm?hl=en-US&gl=US) | DAW | Guitar amp module: 5 amps, 4 pedals, gate, cabinets, analog delay, reverb, tuner | A recorder first. |

Outside the Store, the Windows reference points are [Blue Cat's Free Amp](https://www.bluecataudio.com/Products/Product_FreeAmp/)
(standalone over ASIO, MME or WASAPI, ASIO recommended), AmpliTube 5 (vendor says standalone
low-latency monitoring needs an ASIO sound card), Guitar Rig 7 Player (free, ASIO recommended) and
[EdgeSound Simulator Pedalboard](https://edgesoundsimulator.com/) (free, NAM amp modeling, needs a
low-latency ASIO driver). **Takeaway for Windows: ASIO is the dividing line** between "usable
live" and "demo"; Store apps generally run on shared-mode drivers.

### GitHub and Linux (also the Snap Store question)

| Project | Licence | Features | Assessment |
|---|---|---|---|
| [Guitarix](https://github.com/brummer10/guitarix) | Free software (check repo LICENSE; not confirmed in my sources) | Amp section (preamp, tone stack, cabinet, custom IRs) plus a rack of effects; JACK or PipeWire; v0.45 added NAM, v0.46/0.47 added RTNeural/AIDA-X models, MIDI switching of neural models, a limiter and multiband controls | The reference Linux processor. Sub-10 ms is claimed by third-party guides, but one user found it noticeable until tuned. Steep setup: JACK/PipeWire routing, qpwgraph. No Snap found. |
| [PiPedal](https://rerdavies.github.io/pipedal) | MIT | Raspberry Pi or Ubuntu pedalboard controlled from a phone or tablet; LV2 plugins (ToobAmp bundled), NAM A1/A2, Tone3000 downloads | Best "pedal in a box" story. A forum user reports 5 ms round trip with a complex preset (anecdote). |
| [Neural Amp Modeler](https://github.com/sdatkinson/NeuralAmpModelerPlugin) | MIT | Neural captures of real amps; VST3/AU plugin, LV2 port, huge capture library on Tone3000 | Defines the current quality bar for amp tone. A "handful of milliseconds" is the author's secondhand quote. Newer architecture (A2) reported in 2026 sources; verify against the repo. |
| [AIDA-X](https://github.com/AidaDSP/AIDA-X) | GPL-3.0-or-later | Plays AI-trained amp and pedal models | Neural-capture competitor to NAM; popular on embedded Linux. |
| [BYOD](https://github.com/Chowdhury-DSP/BYOD) | (not checked) | "Build your own distortion": modular pedal builder plugin | Excellent for experimenting with distortion, not a live chain host. |
| [Open Riff Box](https://github.com/dlujic/open-riff-box) | GPL-3.0 | Compressor, wah, gate, drive, distortion, delay, reverb, modulation, EQ; three amp-sim engines; Windows, macOS, Linux | Closest open-source desktop match to guitar-fx. |
| [go-dsp-guitar](https://pkg.go.dev/github.com/andrepxx/go-dsp-guitar) | Apache-2.0 | Multichannel multi-effects over JACK, web remote control, Linux and Windows | Niche; JACK-only. |
| [0xFX](https://github.com/averagenative/0xFX) | (not checked) | 12 circuit-modeled amps, 41 pedals, 9 rack processors, 10 mic models; CLAP/VST3 plus standalone; allocation-free audio thread | Large feature set, young project. |
| [Guitar Virtual Amp](https://github.com/Breno-de-Angelo/guitar_virtual_amp) | (not checked) | Rust, 17 pedals in any order, convolution cabinet, presets, tuner, looper; desktop and Android; quotes a 20.8 us per-sample budget at 48 kHz | **Same stack idea as guitar-fx** (Rust, ordered chain); has cab IR, tuner and looper that guitar-fx lacks. |
| [Open-Amp](https://github.com/Blackshield-Company/Open-Amp) | (not checked) | Gate, compressor, EQ, distortion, amp, cabinet, modulation, reverb; Linux, Android, iOS | Cross-platform ambition; early. |
| [Amplitron](https://github.com/sudip-mondal-2002/Amplitron) | MIT (per a search summary) | Pedalboard with Twin, JCM800, Rectifier, JC-120 voicings, I/O metering, DSP profiling, WASAPI/ASIO on Windows | A small hobby build. |
| [Rakarrack-plus](https://www.linuxlinks.com/rakarrack-plus-rewrite/) | GPL | Rakarrack rewrite: large effect set (distortion, modulation, reverb, harmonizer and more) | Long-lived but aging; the original is considered legacy. |
| [GNUitar](https://awesome.ecosyste.ms/projects/github.com%2Fanarsoul%2Fgnuitar) | GPL | Old-school real-time guitar processor | Its README itself warns that Windows has latency problems. |

### Android (Google Play)

| App | Features | Latency path | Assessment |
|---|---|---|---|
| [Deplike (iRig Guitar Amp and FX)](https://play.google.com/store/apps/details?id=com.deplike.andrig&hl=en_US) | Amps, stompboxes, presets; also on iOS, Windows, Mac | Own USB audio driver, claims to be the only Android app supporting USB audio interfaces *(vendor claim)*; needs an OTG cable | Most capable on Android if the claim holds. Subscription and lifetime plans. Older third-party review rated its Windows 1.0 as poor. |
| [Tonebridge](https://play.google.com/store/apps/details?id=com.ultimateguitar.tonebridge&hl=en_US) | Song-based presets, big preset library | Headphone-jack adapters (iRig, AmpKit Link); USB audio interfaces not supported per one comparison | Users praise presets but report static and some delay. |
| [opiqo](https://play.google.com/store/apps/details?id=org.acoustixaudio.opiqo.multi&hl=en_US) | 4 serial LV2 slots, 200+ bundled LV2 effects, NAM and IR loading, recording; open source, ad-free | Google Oboe | The most interesting indie app: open source, NAM, LV2. No measured latency or confirmed USB support found. |
| [GAAP: Guitar Amp and Effects Sim](https://play.google.com/store/apps/details?id=com.victorhantelmann.gaap&hl=en_CA) | Amp and effects simulation | not checked | Indie; not assessed further. |
| [MobileEffector](https://play.google.com/store/apps/details?id=com.nser.mobileeffector) | Amp, effects, pedalboard; free tier and paid library | not found | Listing only; no details found. |
| [Guitar-Effects](https://play.google.com/store/apps/details?id=com.xtraid.guitareffects&gl=US) | Digital amp and pedalboard | n/a | Marked as no longer supported by its developer. |
| AmpliTube with iRig UA | Full AmpliTube rig | Hardware DSP in the iRig UA gives near-zero monitoring latency | Old (2015) but the right idea: avoid the phone's audio path. Hardware cost is the barrier. |
| [Guiitamp](https://github.com/RaffaeleNachos/Guiitamp) (GitHub) | Oboe plus STK effects; needs an iRig-style interface | Oboe | Hobby app, shows the minimum viable Android design. |
| [FXLab](https://android.googlesource.com/platform/external/oboe/+/HEAD/apps/fxlab/docs/Dev-Guide.md) (Google, in Oboe repo) | Reorderable, toggleable real-time effect chain in C++ with a Kotlin UI | Oboe | The official reference architecture. |

Not found: **BIAS FX 2** shows only App Store listings, and no Android version turned up.
**Guitar Rig Mobile** and **Fender Studio on Android** returned nothing useful. Treat them as
unconfirmed, not absent.

## Feature matrix (what the category offers)

| Feature | Common in | Guitar-fx today |
|---|---|---|
| Ordered, repeatable effect chain | all | yes |
| Drive / compressor / delay / chorus / reverb / modulation | all | yes |
| **Amp models** (tube or neural) | Guitarix, NAM, AIDA-X, Deplike, Open Riff Box | **no** |
| **Cabinet IR loader** | Guitarix, Open Riff Box, Guitar Virtual Amp, opiqo | **no** |
| Noise gate | Open Riff Box, BlueSteel, Open-Amp | no |
| Tuner, looper, recorder | Practice Rack, Guitar Virtual Amp, Deplike | no |
| EQ | most | no |
| Presets | all | yes (text files, built-in and user) |
| Live parameter changes | all | yes |
| Pitch shift / harmonizer / autotune | Rakarrack-plus, some mobile apps | yes (single-note fast mode) |
| Pick-triggered effects (freeze, kalimba, swell, pick noise, transient shaper) | rare | **yes** |
| Plugin hosting (LV2, VST3, CLAP) | Guitarix (LV2), PiPedal, opiqo | no |
| MIDI or phone remote control | PiPedal, Guitarix | no (a JSON control port was started earlier but is not enabled in the UI) |
| Low-latency drivers | JACK/PipeWire (Linux), ASIO (Windows), Oboe (Android) | PipeWire tuning only; no ASIO |

## What this suggests for guitar-fx

1. **Biggest tone gap: amp and cab.** Neural amp captures (NAM, AIDA-X) plus an IR loader are
   what most guitarists expect first. A convolution cabinet is small to build; NAM inference
   in Rust is the larger job (RTNeural-style LSTM or WaveNet).
2. **Cheap, high-value adds:** noise gate, EQ, tuner. Open Riff Box and Guitar Virtual Amp
   show the expected baseline.
3. **Differentiator to keep:** the pick-aware effects and the plain-text, CLI-first chains.
   Competitors rarely have either.
4. **Windows will need ASIO** to be taken seriously for a player who notices 10 ms.
5. **Distribution:** Linux users mainly install via distro packages or Flatpak; a Snap would
   be an easy first listing because none of the main projects has one. A Microsoft Store
   listing is realistic only after ASIO or WASAPI exclusive mode is in.

## Caveats

- Counts, licences marked "not checked", and star or download numbers were not verified.
- Latency claims (Deplike, iRig UA, Guitarix, PiPedal, NAM) are vendor or anecdote, not
  measurements. A fair comparison would need a loopback test on one machine.
- Store listings change; several found listings are years old.

## Sources

- Microsoft Store: [Practice Rack](https://apps.microsoft.com/detail/9pk87fr1mzb3?hl=en-US&gl=US), [Pedalboard](https://apps.microsoft.com/detail/9nnhppx0hbxj?hl=en-GB&gl=ZA), [Infinite Amp Lab](https://apps.microsoft.com/detail/9pk3g3jsqrtq?hl=en-US&gl=US), [Amperium Live](https://apps.microsoft.com/detail/9pcnx0d25txp?hl=en-US&gl=US), [Fender Studio](https://apps.microsoft.com/detail/9nzklscrwrsq?hl=en-US&gl=US), [n-Track Studio](https://apps.microsoft.com/detail/xp8lg4st7h3gbf?hl=en-US&gl=US), [Hyperion DAW](https://apps.microsoft.com/detail/9mx3249pj1vm?hl=en-US&gl=US)
- Windows outside the Store: [Blue Cat Free Amp](https://www.bluecataudio.com/Products/Product_FreeAmp/), [EdgeSound Simulator](https://edgesoundsimulator.com/), [AmpliTube 5 (Thomann)](https://www.thomann.co.uk/ik_multimedia_amplitube_5.htm), [Sound on Sound: native ASIO](https://www.soundonsound.com/music-business/asio-goes-native)
- Snap Store: [rkrlv2](https://snapcraft.io/rkrlv2), [n-Track Studio](https://snapcraft.io/n-trackstudio), [guitarfinetune](https://snapcraft.io/guitarfinetune)
- GitHub and Linux: [Guitarix on ArchWiki](https://wiki.archlinux.org/title/Electric_guitar_interface_setup), [Guitarix release V0.47.0 thread](https://lists.linuxaudio.org/hyperkitty/list/linux-audio-dev@lists.linuxaudio.org/message/BLDHA5O44KATDFXCCH4E4C6UCWADDU25), [PiPedal](https://rerdavies.github.io/pipedal), [PiPedal 2.0](https://rerdavies.github.io/pipedal/PiPedal2.html), [NAM plugin](https://www.github.com/StriderHND/NeuralAmpModelerPlugin), [AIDA-X](https://github.com/AidaDSP/AIDA-X), [Open Riff Box](https://github.com/dlujic/open-riff-box), [go-dsp-guitar](https://pkg.go.dev/github.com/andrepxx/go-dsp-guitar), [0xFX](https://github.com/averagenative/0xFX), [Guitar Virtual Amp](https://github.com/Breno-de-Angelo/guitar_virtual_amp), [Open-Amp](https://github.com/Blackshield-Company/Open-Amp), [Amplitron](https://github.com/sudip-mondal-2002/Amplitron), [Rakarrack-plus](https://www.linuxlinks.com/rakarrack-plus-rewrite/)
- Android: [Deplike on Google Play](https://play.google.com/store/apps/details?id=com.deplike.andrig&hl=en_US), [Tonebridge](https://play.google.com/store/apps/details?id=com.ultimateguitar.tonebridge&hl=en_US), [opiqo](https://play.google.com/store/apps/details?id=org.acoustixaudio.opiqo.multi&hl=en_US), [Deplike app comparison blog](https://blog.deplike.com/top-5-guitar-amp-apps-on-mobile), [iRig UA review (Android Police)](https://www.androidpolice.com/2015/06/04/ik-multimedia-irig-ua-review-finally-real-guitar-amp-emulation-on-android-with-no-latency/), [Guiitamp](https://github.com/RaffaeleNachos/Guiitamp), [Oboe FXLab](https://android.googlesource.com/platform/external/oboe/+/HEAD/apps/fxlab/docs/Dev-Guide.md)
