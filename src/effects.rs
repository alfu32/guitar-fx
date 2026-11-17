//! The effects. Every effect is mono and processes blocks in place; a chain is
//! built per audio channel so stereo files keep their image.

use crate::params::Params;
use fundsp::prelude32 as fs;
use fundsp::prelude32::AudioUnit;
use signalsmith_stretch::Stretch;

pub trait Effect: Send {
    fn process(&mut self, buf: &mut [f32]);
}

/// A parsed effect from the command line (not yet bound to a sample rate).
pub struct Spec {
    pub name: &'static str,
    pub raw: String,
    pub params: Params,
}

pub const EFFECT_NAMES: &[&str] = &[
    "overdrive",
    "compressor",
    "echo",
    "delay",
    "chorus",
    "harmonics",
    "tone-shift",
    "vocoder",
    "autotune",
    "tremolo",
    "flanger",
    "phaser",
    "wah",
    "reverb",
];

/// (primary parameter, all parameters) for an effect name.
fn schema(name: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match name {
        "overdrive" => ("drive", &["drive", "tone", "level"]),
        "compressor" => ("ratio", &["threshold", "ratio", "attack", "release", "makeup"]),
        "echo" => ("mix", &["time", "feedback", "tone", "mix"]),
        "delay" => ("time", &["time", "feedback", "mix"]),
        "chorus" => ("depth", &["rate", "depth", "mix"]),
        "harmonics" => ("mix", &["drive", "freq", "mix"]),
        "tone-shift" => ("semitones", &["semitones", "cents", "mix", "mode", "window"]),
        "vocoder" => ("mix", &["bands", "carrier", "release", "mix"]),
        "autotune" => ("amount", &["key", "scale", "speed", "amount", "mode", "window"]),
        "tremolo" => ("depth", &["rate", "depth"]),
        "flanger" => ("depth", &["rate", "depth", "feedback", "mix"]),
        "phaser" => ("depth", &["rate", "depth", "feedback", "mix"]),
        "wah" => ("sensitivity", &["sensitivity", "rate", "q", "min", "max", "mix"]),
        "reverb" => ("mix", &["mix", "time", "size", "damping"]),
        _ => return None,
    })
}

pub fn parse_spec(name: &str, raw: &str) -> Result<Spec, String> {
    let (primary, allowed) = schema(name).ok_or_else(|| format!("unknown effect '{name}'"))?;
    let name: &'static str = EFFECT_NAMES.iter().find(|n| **n == name).unwrap();
    Ok(Spec { name, raw: raw.trim().to_string(), params: Params::parse(name, raw, primary, allowed)? })
}

pub fn build(spec: &Spec, sr: f32) -> Result<Box<dyn Effect>, String> {
    let p = &spec.params;
    Ok(match spec.name {
        "overdrive" => Box::new(Overdrive::new(
            p.num("drive", 0.5, 0.0, 1.0)?,
            p.num("tone", 0.6, 0.0, 1.0)?,
            p.num("level", 1.0, 0.0, 2.0)?,
            sr,
        )),
        "compressor" => {
            let threshold = p.num("threshold", -18.0, -60.0, 0.0)?;
            let ratio = p.num("ratio", 4.0, 1.0, 20.0)?;
            let makeup = match p.text("makeup", "auto").as_str() {
                "auto" => -threshold * (1.0 - 1.0 / ratio) * 0.25,
                v => v
                    .parse::<f32>()
                    .map_err(|_| format!("--compressor: makeup='{v}' is not a number or 'auto'"))?,
            };
            Box::new(Compressor::new(
                threshold,
                ratio,
                p.num("attack", 10.0, 0.1, 200.0)?,
                p.num("release", 120.0, 5.0, 2000.0)?,
                makeup,
                sr,
            ))
        }
        "echo" => Box::new(DelayFx::new(
            p.num("time", 350.0, 1.0, 2000.0)?,
            p.num("feedback", 0.45, 0.0, 0.95)?,
            Some(p.num("tone", 0.5, 0.0, 1.0)?),
            p.num("mix", 0.4, 0.0, 1.0)?,
            sr,
        )),
        "delay" => Box::new(DelayFx::new(
            p.num("time", 300.0, 1.0, 2000.0)?,
            p.num("feedback", 0.3, 0.0, 0.95)?,
            None,
            p.num("mix", 0.5, 0.0, 1.0)?,
            sr,
        )),
        "chorus" => Box::new(Chorus::new(
            p.num("rate", 0.6, 0.05, 10.0)?,
            p.num("depth", 0.5, 0.0, 1.0)?,
            p.num("mix", 0.5, 0.0, 1.0)?,
            sr,
        )),
        "harmonics" => Box::new(Harmonics::new(
            p.num("drive", 0.5, 0.0, 1.0)?,
            p.num("freq", 2000.0, 200.0, 12000.0)?,
            p.num("mix", 0.3, 0.0, 1.0)?,
            sr,
        )),
        "tone-shift" => Box::new(ToneShift::new(
            p.num("semitones", 0.0, -24.0, 24.0)? + p.num("cents", 0.0, -100.0, 100.0)? / 100.0,
            p.num("mix", 1.0, 0.0, 1.0)?,
            shifter(&p.text("mode", "high"), p.num("window", 10.0, 4.0, 40.0)?, sr)?,
        )),
        "vocoder" => Box::new(Vocoder::new(
            p.num("bands", 16.0, 4.0, 32.0)? as usize,
            p.num("carrier", 110.0, 30.0, 880.0)?,
            p.num("release", 15.0, 1.0, 200.0)?,
            p.num("mix", 1.0, 0.0, 1.0)?,
            sr,
        )),
        "autotune" => Box::new(Autotune::new(
            &p.text("key", "C"),
            &p.text("scale", "chromatic"),
            p.num("speed", 40.0, 0.0, 500.0)?,
            p.num("amount", 1.0, 0.0, 1.0)?,
            sr,
            shifter(&p.text("mode", "high"), p.num("window", 10.0, 4.0, 40.0)?, sr)?,
        )?),
        "tremolo" => Box::new(Tremolo::new(p.num("rate", 5.0, 0.1, 20.0)?, p.num("depth", 0.5, 0.0, 1.0)?, sr)),
        "flanger" => Box::new(Flanger::new(
            p.num("rate", 0.3, 0.02, 5.0)?,
            p.num("depth", 0.7, 0.0, 1.0)?,
            p.num("feedback", 0.5, -0.95, 0.95)?,
            p.num("mix", 0.5, 0.0, 1.0)?,
            sr,
        )),
        "phaser" => Box::new(Phaser::new(
            p.num("rate", 0.5, 0.02, 5.0)?,
            p.num("depth", 0.7, 0.0, 1.0)?,
            p.num("feedback", 0.5, -0.95, 0.95)?,
            p.num("mix", 0.5, 0.0, 1.0)?,
            sr,
        )),
        "wah" => Box::new(Wah::new(
            p.num("sensitivity", 0.6, 0.0, 1.0)?,
            p.num("rate", 0.0, 0.0, 10.0)?,
            p.num("q", 5.0, 1.0, 15.0)?,
            p.num("min", 350.0, 100.0, 2000.0)?,
            p.num("max", 2200.0, 500.0, 6000.0)?,
            p.num("mix", 1.0, 0.0, 1.0)?,
            sr,
        )),
        "reverb" => Box::new(Reverb::new(
            p.num("mix", 0.3, 0.0, 1.0)?,
            p.num("time", 2.0, 0.2, 10.0)?,
            p.num("size", 10.0, 3.0, 30.0)?,
            p.num("damping", 0.5, 0.0, 1.0)?,
            sr,
        )),
        other => return Err(format!("unknown effect '{other}'")),
    })
}

fn unit(mut u: Box<dyn AudioUnit>, sr: f32) -> Box<dyn AudioUnit> {
    u.set_sample_rate(sr as f64);
    u
}

fn coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms * 0.001 * sr)).exp()
}

// ---------------------------------------------------------------- overdrive

struct Overdrive {
    shaper: Box<dyn AudioUnit>,
    tone: Box<dyn AudioUnit>,
    makeup: f32,
}

impl Overdrive {
    fn new(drive: f32, tone: f32, level: f32, sr: f32) -> Self {
        let gain = 1.0 + drive * 39.0;
        let cutoff = 500.0 * 24.0_f32.powf(tone); // 500 Hz .. 12 kHz
        Self {
            shaper: unit(Box::new(fs::shape(fs::Tanh(gain))), sr),
            tone: unit(Box::new(fs::lowpole_hz(cutoff)), sr),
            makeup: level / gain.powf(0.35),
        }
    }
}

impl Effect for Overdrive {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let y = self.shaper.filter_mono(*x);
            *x = self.tone.filter_mono(y) * self.makeup;
        }
    }
}

// --------------------------------------------------------------- compressor

struct Compressor {
    threshold: f32,
    slope: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    env: f32,
}

impl Compressor {
    fn new(threshold: f32, ratio: f32, attack_ms: f32, release_ms: f32, makeup: f32, sr: f32) -> Self {
        Self {
            threshold,
            slope: 1.0 - 1.0 / ratio,
            attack: coef(attack_ms, sr),
            release: coef(release_ms, sr),
            makeup,
            env: 0.0,
        }
    }
}

impl Effect for Compressor {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let level = x.abs();
            let c = if level > self.env { self.attack } else { self.release };
            self.env = level + c * (self.env - level);
            let db = 20.0 * self.env.max(1e-6).log10();
            let reduction = (db - self.threshold).max(0.0) * self.slope;
            *x *= 10f32.powf((self.makeup - reduction) / 20.0);
        }
    }
}

// --------------------------------------------------------------- echo/delay

struct DelayFx {
    line: Vec<f32>,
    pos: usize,
    feedback: f32,
    mix: f32,
    damp: Option<f32>, // one-pole lowpass coefficient in the feedback path (tape-style echo)
    damp_state: f32,
}

impl DelayFx {
    fn new(time_ms: f32, feedback: f32, tone: Option<f32>, mix: f32, sr: f32) -> Self {
        let len = ((time_ms * 0.001 * sr) as usize).max(1);
        Self {
            line: vec![0.0; len],
            pos: 0,
            feedback,
            mix,
            damp: tone.map(|t| {
                let cutoff = 1000.0 * 12.0_f32.powf(t); // 1 kHz .. 12 kHz
                1.0 - (-2.0 * std::f32::consts::PI * cutoff / sr).exp()
            }),
            damp_state: 0.0,
        }
    }
}

impl Effect for DelayFx {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let mut d = self.line[self.pos];
            if let Some(a) = self.damp {
                self.damp_state += a * (d - self.damp_state);
                d = self.damp_state;
            }
            self.line[self.pos] = *x + self.feedback * d;
            self.pos = (self.pos + 1) % self.line.len();
            *x += self.mix * d;
        }
    }
}

// ------------------------------------------------------------------- chorus

struct Chorus {
    inner: Box<dyn AudioUnit>,
    mix: f32,
}

impl Chorus {
    fn new(rate: f32, depth: f32, mix: f32, sr: f32) -> Self {
        let inner = fs::chorus(0, 0.015, 0.002 + depth * 0.008, rate);
        Self { inner: unit(Box::new(inner), sr), mix }
    }
}

impl Effect for Chorus {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let wet = self.inner.filter_mono(*x) * 2.0; // fundsp's chorus output is ~half level
            *x += self.mix * (wet - *x);
        }
    }
}

// ---------------------------------------------------------------- harmonics

/// Exciter: distorts the high band and adds it back for extra harmonics.
struct Harmonics {
    pre: Box<dyn AudioUnit>,
    shaper: Box<dyn AudioUnit>,
    post: Box<dyn AudioUnit>,
    mix: f32,
}

impl Harmonics {
    fn new(drive: f32, freq: f32, mix: f32, sr: f32) -> Self {
        Self {
            pre: unit(Box::new(fs::highpass_hz(freq, 0.7)), sr),
            shaper: unit(Box::new(fs::shape(fs::Tanh(1.0 + drive * 9.0))), sr),
            post: unit(Box::new(fs::highpass_hz(freq, 0.7)), sr),
            mix,
        }
    }
}

impl Effect for Harmonics {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let h = self.pre.filter_mono(*x);
            let h = self.shaper.filter_mono(h);
            let h = self.post.filter_mono(h);
            *x += self.mix * h;
        }
    }
}

// --------------------------------------------------------------- tone-shift

/// A pitch shifter backend. `high` is the phase-vocoder-style Signalsmith Stretch
/// (cleanest, ~50+ ms latency). `fast` is a pitch-synchronous delay-line shifter: the
/// detected note period locks its two taps in phase, so latency is only about one or
/// two periods (about 10 ms) and the pitch is exact for a single sustained note.
/// It tracks one note at a time, so chords and noise degrade it.
enum Shifter {
    High(Stretch, Vec<f32>),
    Fast(Granular, PitchTracker),
}

fn shifter(mode: &str, window_ms: f32, sr: f32) -> Result<Shifter, String> {
    match mode {
        "high" => Ok(Shifter::High(Stretch::preset_default(1, sr as u32), Vec::new())),
        "fast" => Ok(Shifter::Fast(Granular::new(window_ms, sr), PitchTracker::new(sr))),
        other => Err(format!("mode='{other}' is not valid (use high or fast)")),
    }
}

impl Shifter {
    fn process(&mut self, buf: &mut [f32], semitones: f32) {
        match self {
            Shifter::High(stretch, scratch) => {
                stretch.set_transpose_factor_semitones(semitones, None);
                scratch.clear();
                scratch.extend_from_slice(buf);
                stretch.process(&*scratch, buf);
            }
            Shifter::Fast(g, tracker) => {
                if tracker.push(buf) {
                    if let Some(f0) = tracker.detect() {
                        g.set_period(tracker.sr / f0);
                    }
                }
                let ratio = 2f32.powf(semitones / 12.0);
                for x in buf {
                    *x = g.tick(*x, ratio);
                }
            }
        }
    }
}

/// Two taps `delta` samples apart (a whole number of note periods) sweep through a
/// delay line with a linear crossfade. Because the taps see the same waveform, the
/// crossfade is seamless and the output pitch is exactly `ratio` times the input.
struct Granular {
    buf: Vec<f32>,
    wpos: usize,
    u: f32,
    delta: f32,
    target: f32,
    max_delta: f32,
}

impl Granular {
    fn new(window_ms: f32, sr: f32) -> Self {
        let max_delta = 0.06 * sr;
        let target = (window_ms * 0.001 * sr).clamp(16.0, max_delta);
        Self { buf: vec![0.0; max_delta as usize * 2 + 8], wpos: 0, u: 0.0, delta: target, target, max_delta }
    }

    /// Lock the tap spacing to a whole number of note periods near the target window.
    fn set_period(&mut self, period: f32) {
        let k = (self.target / period).round().max(1.0);
        let d = (k * period).clamp(16.0, self.max_delta);
        if (d - self.delta).abs() / self.delta > 0.01 {
            self.u *= d / self.delta;
            self.delta = d;
        }
    }

    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let pos = (self.wpos as f32 - delay - 1.0).rem_euclid(len as f32);
        let i = (pos as usize) % len; // rem_euclid can round up to exactly len
        let frac = (pos - i as f32).clamp(0.0, 1.0);
        let (a, b) = (self.buf[i], self.buf[(i + 1) % len]);
        a + (b - a) * frac
    }

    fn tick(&mut self, x: f32, ratio: f32) -> f32 {
        self.buf[self.wpos] = x;
        self.u = (self.u + (1.0 - ratio)).rem_euclid(self.delta);
        let s = self.u / self.delta;
        let y = (1.0 - s) * self.read(self.u) + s * self.read(self.u + self.delta);
        self.wpos = (self.wpos + 1) % self.buf.len();
        y
    }
}

struct ToneShift {
    shifter: Shifter,
    semitones: f32,
    mix: f32,
    scratch: Vec<f32>,
}

impl ToneShift {
    fn new(semitones: f32, mix: f32, shifter: Shifter) -> Self {
        Self { shifter, semitones, mix, scratch: Vec::new() }
    }
}

impl Effect for ToneShift {
    fn process(&mut self, buf: &mut [f32]) {
        self.scratch.clear();
        self.scratch.extend_from_slice(buf);
        self.shifter.process(buf, self.semitones);
        for (x, dry) in buf.iter_mut().zip(&self.scratch) {
            *x = dry * (1.0 - self.mix) + *x * self.mix;
        }
    }
}

// ------------------------------------------------------------------ vocoder

/// Channel vocoder. The input is the modulator; the carrier is a built-in
/// saw stack (root, fifth, octave).
struct Vocoder {
    mod_filters: Vec<Box<dyn AudioUnit>>,
    car_filters: Vec<Box<dyn AudioUnit>>,
    envs: Vec<f32>,
    release: f32,
    phases: [f32; 3],
    steps: [f32; 3],
    mix: f32,
}

impl Vocoder {
    fn new(bands: usize, carrier: f32, release_ms: f32, mix: f32, sr: f32) -> Self {
        let (lo, hi) = (120.0_f32, 7000.0_f32);
        let centers: Vec<f32> = (0..bands)
            .map(|i| lo * (hi / lo).powf(i as f32 / (bands - 1) as f32))
            .collect();
        let q = 4.0 + bands as f32 / 4.0;
        let mk = |f: f32| unit(Box::new(fs::bandpass_hz(f, q)), sr);
        Self {
            mod_filters: centers.iter().map(|f| mk(*f)).collect(),
            car_filters: centers.iter().map(|f| mk(*f)).collect(),
            envs: vec![0.0; bands],
            release: coef(release_ms, sr),
            phases: [0.0; 3],
            steps: [carrier / sr, carrier * 1.5 / sr, carrier * 2.0 / sr],
            mix,
        }
    }
}

impl Effect for Vocoder {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let mut carrier = 0.0;
            for (p, s) in self.phases.iter_mut().zip(self.steps) {
                *p = (*p + s).fract();
                carrier += 2.0 * *p - 1.0;
            }
            carrier *= 0.33;
            let mut sum = 0.0;
            for i in 0..self.envs.len() {
                let m = self.mod_filters[i].filter_mono(*x).abs();
                let e = &mut self.envs[i];
                *e = if m > *e { m } else { m + self.release * (*e - m) };
                sum += self.car_filters[i].filter_mono(carrier) * *e;
            }
            let wet = (sum * 0.4).tanh();
            *x = *x * (1.0 - self.mix) + wet * self.mix;
        }
    }
}

// ----------------------------------------------------------------- autotune

const WINDOW: usize = 1536;
const HOP: usize = 256;

/// Rolling window of recent input plus a YIN pitch detector.
struct PitchTracker {
    hist: Vec<f32>,
    since: usize,
    sr: f32,
}

impl PitchTracker {
    fn new(sr: f32) -> Self {
        Self { hist: vec![0.0; WINDOW], since: 0, sr }
    }

    /// Feed a block; returns true when it is time to run `detect` again.
    fn push(&mut self, buf: &[f32]) -> bool {
        let n = buf.len();
        if n >= WINDOW {
            self.hist.copy_from_slice(&buf[n - WINDOW..]);
        } else {
            self.hist.copy_within(n.., 0);
            self.hist[WINDOW - n..].copy_from_slice(buf);
        }
        self.since += n;
        if self.since >= HOP {
            self.since = 0;
            return true;
        }
        false
    }

    /// YIN fundamental-frequency estimate of the history window; None if unvoiced.
    /// Runs on a 4x downsampled copy (cheap enough for a 64-frame live callback);
    /// parabolic interpolation recovers the precision. Detects down to about 60 Hz.
    fn detect(&self) -> Option<f32> {
        const D: usize = 4;
        const N: usize = WINDOW / D;
        const HALF: usize = N / 2;
        let rms = (self.hist.iter().map(|x| x * x).sum::<f32>() / WINDOW as f32).sqrt();
        if rms < 0.01 {
            return None;
        }
        let mut x = [0.0f32; N];
        for (o, c) in x.iter_mut().zip(self.hist.chunks_exact(D)) {
            *o = c.iter().sum::<f32>() / D as f32;
        }
        let sr = self.sr / D as f32;
        let mut d = [0.0f32; HALF];
        for tau in 1..HALF {
            d[tau] = x[..HALF].iter().zip(&x[tau..tau + HALF]).map(|(a, b)| (a - b) * (a - b)).sum();
        }
        // cumulative mean normalised difference
        let mut running = 0.0;
        for tau in 1..HALF {
            running += d[tau];
            d[tau] = if running > 0.0 { d[tau] * tau as f32 / running } else { 1.0 };
        }
        let mut tau = ((sr / 900.0) as usize).max(2);
        while tau < HALF - 1 {
            if d[tau] < 0.15 {
                while tau + 1 < HALF - 1 && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
                let denom = a - 2.0 * b + c;
                let shift = if denom.abs() > 1e-9 { 0.5 * (a - c) / denom } else { 0.0 };
                return Some(sr / (tau as f32 + shift));
            }
            tau += 1;
        }
        None
    }
}

struct Autotune {
    shifter: Shifter,
    allowed: [bool; 12], // pitch classes (absolute), already rotated by key
    speed_ms: f32,
    amount: f32,
    sr: f32,
    tracker: PitchTracker,
    target: f32,
    current: f32,
}

fn parse_key(key: &str) -> Result<usize, String> {
    let k = key.trim().to_lowercase().replace('♯', "#").replace('♭', "b");
    Ok(match k.as_str() {
        "c" => 0,
        "c#" | "db" => 1,
        "d" => 2,
        "d#" | "eb" => 3,
        "e" => 4,
        "f" => 5,
        "f#" | "gb" => 6,
        "g" => 7,
        "g#" | "ab" => 8,
        "a" => 9,
        "a#" | "bb" => 10,
        "b" => 11,
        _ => return Err(format!("--autotune: unknown key '{key}' (use C, C#, Db, D ... B)")),
    })
}

impl Autotune {
    fn new(key: &str, scale: &str, speed_ms: f32, amount: f32, sr: f32, shifter: Shifter) -> Result<Self, String> {
        let root = parse_key(key)?;
        let steps: &[usize] = match scale.to_lowercase().as_str() {
            "chromatic" => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
            "major" => &[0, 2, 4, 5, 7, 9, 11],
            "minor" => &[0, 2, 3, 5, 7, 8, 10],
            "pentatonic" | "major-pentatonic" => &[0, 2, 4, 7, 9],
            "minor-pentatonic" => &[0, 3, 5, 7, 10],
            "blues" => &[0, 3, 5, 6, 7, 10],
            other => {
                return Err(format!(
                    "--autotune: unknown scale '{other}' (chromatic, major, minor, pentatonic, minor-pentatonic, blues)"
                ));
            }
        };
        let mut allowed = [false; 12];
        for s in steps {
            allowed[(root + s) % 12] = true;
        }
        Ok(Self {
            shifter,
            allowed,
            speed_ms,
            amount,
            sr,
            tracker: PitchTracker::new(sr),
            target: 0.0,
            current: 0.0,
        })
    }

    fn update_target(&mut self) {
        self.target = match self.tracker.detect() {
            None => 0.0,
            Some(f0) => {
                let midi = 69.0 + 12.0 * (f0 / 440.0).log2();
                let base = midi.round() as i32;
                let nearest = (-6..=6)
                    .map(|o| base + o)
                    .filter(|n| self.allowed[n.rem_euclid(12) as usize])
                    .min_by(|a, b| {
                        (*a as f32 - midi).abs().partial_cmp(&(*b as f32 - midi).abs()).unwrap()
                    })
                    .unwrap_or(base);
                ((nearest as f32 - midi) * self.amount).clamp(-7.0, 7.0)
            }
        };
    }
}

impl Effect for Autotune {
    fn process(&mut self, buf: &mut [f32]) {
        let n = buf.len();
        if self.tracker.push(buf) {
            self.update_target();
        }
        let alpha = if self.speed_ms <= 0.0 {
            1.0
        } else {
            1.0 - (-(n as f32) / (self.speed_ms * 0.001 * self.sr)).exp()
        };
        self.current += (self.target - self.current) * alpha;
        self.shifter.process(buf, self.current);
    }
}

// ------------------------------------------------------------ new pedals

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

struct Tremolo {
    phase: f32,
    step: f32,
    depth: f32,
}

impl Tremolo {
    fn new(rate: f32, depth: f32, sr: f32) -> Self {
        Self { phase: 0.0, step: rate / sr, depth }
    }
}

impl Effect for Tremolo {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.phase = (self.phase + self.step).fract();
            let lfo = 0.5 + 0.5 * (std::f32::consts::TAU * self.phase).sin();
            *x *= 1.0 - self.depth * lfo;
        }
    }
}

struct Flanger {
    inner: Box<dyn AudioUnit>,
    mix: f32,
}

impl Flanger {
    fn new(rate: f32, depth: f32, feedback: f32, mix: f32, sr: f32) -> Self {
        let (lo, hi) = (0.001, 0.002 + depth * 0.008);
        let f = move |t: f32| lo + (hi - lo) * (0.5 + 0.5 * (std::f32::consts::TAU * rate * t).sin());
        Self { inner: unit(Box::new(fs::flanger(feedback, lo, hi, f)), sr), mix }
    }
}

impl Effect for Flanger {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let wet = self.inner.filter_mono(*x) * 0.5; // fundsp's flanger sums dry + delayed
            *x += self.mix * (wet - *x);
        }
    }
}

struct Phaser {
    inner: Box<dyn AudioUnit>,
    mix: f32,
}

impl Phaser {
    fn new(rate: f32, depth: f32, feedback: f32, mix: f32, sr: f32) -> Self {
        let f = move |t: f32| 0.5 + depth * 0.5 * (std::f32::consts::TAU * rate * t).sin();
        Self { inner: unit(Box::new(fs::phaser(feedback, f)), sr), mix }
    }
}

impl Effect for Phaser {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let wet = self.inner.filter_mono(*x);
            *x += self.mix * (wet - *x);
        }
    }
}

/// Auto-wah: a resonant band-pass swept by your picking (`rate=0`) or by an LFO.
struct Wah {
    sens: f32,
    lfo_step: f32,
    lfo_phase: f32,
    damp: f32,
    gain: f32,
    min: f32,
    max: f32,
    mix: f32,
    sr: f32,
    env: f32,
    att: f32,
    rel: f32,
    low: f32,
    band: f32,
}

impl Wah {
    fn new(sens: f32, rate: f32, q: f32, min: f32, max: f32, mix: f32, sr: f32) -> Self {
        Self {
            sens,
            lfo_step: rate / sr,
            lfo_phase: 0.0,
            damp: 1.0 / q,
            gain: q.sqrt() * 2.0,
            min,
            max: max.max(min * 1.2),
            mix,
            sr,
            env: 0.0,
            att: coef(4.0, sr),
            rel: coef(90.0, sr),
            low: 0.0,
            band: 0.0,
        }
    }
}

impl Effect for Wah {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let pos = if self.lfo_step > 0.0 {
                self.lfo_phase = (self.lfo_phase + self.lfo_step).fract();
                0.5 + 0.5 * (std::f32::consts::TAU * self.lfo_phase).sin()
            } else {
                let l = x.abs();
                let c = if l > self.env { self.att } else { self.rel };
                self.env = l + c * (self.env - l);
                (self.env * (3.0 + 40.0 * self.sens)).clamp(0.0, 1.0)
            };
            let fc = (self.min * (self.max / self.min).powf(pos)).min(self.sr / 6.0);
            let f = 2.0 * (std::f32::consts::PI * fc / self.sr).sin();
            self.low += f * self.band;
            let high = *x - self.low - self.damp * self.band;
            self.band += f * high;
            let wet = self.band * self.damp * self.gain;
            *x = *x * (1.0 - self.mix) + wet * self.mix;
        }
    }
}

struct Reverb {
    inner: Box<dyn AudioUnit>,
    mix: f32,
}

impl Reverb {
    fn new(mix: f32, time: f32, size: f32, damping: f32, sr: f32) -> Self {
        Self { inner: unit(Box::new(fs::reverb_stereo(size, time, damping)), sr), mix }
    }
}

impl Effect for Reverb {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let (l, r) = self.inner.filter_stereo(*x, *x);
            *x += self.mix * 0.5 * (l + r);
        }
    }
}

// ------------------------------------------------------------------- chain

pub struct Chain {
    fx: Vec<Box<dyn Effect>>,
    in_gain: f32,
    out_gain: f32,
}

impl Chain {
    pub fn new(specs: &[Spec], sr: f32, input_db: f32, output_db: f32) -> Result<Self, String> {
        let fx = specs.iter().map(|s| build(s, sr)).collect::<Result<Vec<_>, _>>()?;
        Ok(Chain { fx, in_gain: db(input_db), out_gain: db(output_db) })
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        if self.in_gain != 1.0 {
            buf.iter_mut().for_each(|x| *x *= self.in_gain);
        }
        for fx in &mut self.fx {
            fx.process(buf);
        }
        if self.out_gain != 1.0 {
            buf.iter_mut().for_each(|x| *x *= self.out_gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn sine(freq: f32, secs: f32) -> Vec<f32> {
        (0..(SR * secs) as usize)
            .map(|n| 0.5 * (std::f32::consts::TAU * freq * n as f32 / SR).sin())
            .collect()
    }

    /// Frequency from upward zero crossings over the second half of the buffer.
    fn measure(x: &[f32]) -> f32 {
        let tail = &x[x.len() / 2..];
        let crossings = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 / (tail.len() as f32 / SR)
    }

    fn run(spec: &str, value: &str, input: &[f32]) -> Vec<f32> {
        let spec = parse_spec(spec, value).unwrap();
        let mut chain = Chain::new(&[spec], SR, 0.0, 0.0).unwrap();
        let mut out = input.to_vec();
        for block in out.chunks_mut(256) {
            chain.process(block);
        }
        out
    }

    #[test]
    fn fast_shifter_hits_target_pitch() {
        for semis in [-12.0f32, -7.0, -2.0, 2.0, 7.0, 12.0] {
            let out = run("tone-shift", &format!("semitones={semis},mode=fast"), &sine(220.0, 1.5));
            let want = 220.0 * 2f32.powf(semis / 12.0);
            let got = measure(&out);
            assert!((got / want - 1.0).abs() < 0.03, "{semis:+} st: wanted {want:.1} Hz, got {got:.1} Hz");
        }
    }

    #[test]
    fn high_quality_shifter_hits_target_pitch() {
        let out = run("tone-shift", "semitones=7", &sine(220.0, 2.0));
        let want = 220.0 * 2f32.powf(7.0 / 12.0);
        let got = measure(&out);
        assert!((got / want - 1.0).abs() < 0.02, "wanted {want:.1} Hz, got {got:.1} Hz");
    }

    #[test]
    fn autotune_snaps_to_nearest_note() {
        let sharp = 220.0 * 2f32.powf(0.35 / 12.0); // A3 + 35 cents
        let out = run("autotune", "speed=0", &sine(sharp, 2.0));
        let got = measure(&out);
        assert!((got / 220.0 - 1.0).abs() < 0.01, "wanted ~220 Hz, got {got:.1} Hz");
    }

    #[test]
    fn every_effect_is_finite_and_bounded() {
        for name in EFFECT_NAMES {
            let out = run(name, "", &sine(196.0, 1.0));
            assert!(out.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{name} produced bad samples");
        }
    }

    #[test]
    fn order_matters() {
        let input = sine(220.0, 0.5);
        let a = run_chain(&[("overdrive", "0.9"), ("echo", "time=50")], &input);
        let b = run_chain(&[("echo", "time=50"), ("overdrive", "0.9")], &input);
        assert!(a.iter().zip(&b).any(|(x, y)| (x - y).abs() > 0.01));
    }

    #[test]
    fn effects_can_repeat() {
        let input = sine(220.0, 0.2);
        let once = run_chain(&[("overdrive", "0.5")], &input);
        let twice = run_chain(&[("overdrive", "0.5"), ("overdrive", "0.5")], &input);
        assert!(once.iter().zip(&twice).any(|(x, y)| (x - y).abs() > 0.01));
    }

    fn run_chain(items: &[(&str, &str)], input: &[f32]) -> Vec<f32> {
        let specs: Vec<Spec> = items.iter().map(|(n, v)| parse_spec(n, v).unwrap()).collect();
        let mut chain = Chain::new(&specs, SR, 0.0, 0.0).unwrap();
        let mut out = input.to_vec();
        for block in out.chunks_mut(256) {
            chain.process(block);
        }
        out
    }

    #[test]
    fn bad_parameters_are_rejected() {
        assert!(parse_spec("overdrive", "tune=1").is_err());
        assert!(build(&parse_spec("compressor", "ratio=99").unwrap(), SR).is_err());
        assert!(build(&parse_spec("autotune", "key=H").unwrap(), SR).is_err());
        assert!(build(&parse_spec("tone-shift", "mode=slow").unwrap(), SR).is_err());
    }
}

#[cfg(test)]
mod fast_autotune_test {
    use super::*;

    #[test]
    fn fast_autotune_snaps_to_nearest_note() {
        let sr = 48000.0f32;
        let sharp = 220.0 * 2f32.powf(0.35 / 12.0);
        let input: Vec<f32> =
            (0..96000).map(|n| 0.5 * (std::f32::consts::TAU * sharp * n as f32 / sr).sin()).collect();
        let spec = parse_spec("autotune", "speed=0,mode=fast").unwrap();
        let mut chain = Chain::new(&[spec], sr, 0.0, 0.0).unwrap();
        let mut out = input.clone();
        for b in out.chunks_mut(256) {
            chain.process(b);
        }
        let tail = &out[48000..];
        let crossings = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        let f = crossings as f32 / (tail.len() as f32 / sr);
        assert!((f / 220.0 - 1.0).abs() < 0.01, "got {f:.1} Hz");
    }
}

#[cfg(test)]
mod pitch_tests {
    use super::*;

    #[test]
    fn detects_guitar_range() {
        let sr = 48000.0f32;
        for f in [82.41f32, 110.0, 196.0, 329.63, 659.26] {
            let x: Vec<f32> = (0..WINDOW)
                .map(|n| 0.4 * (std::f32::consts::TAU * f * n as f32 / sr).sin())
                .collect();
            let mut t = PitchTracker::new(sr);
            t.push(&x);
            let got = t.detect().unwrap_or(0.0);
            assert!((got / f - 1.0).abs() < 0.01, "wanted {f} Hz, got {got:.2} Hz");
        }
    }
}
