//! The effects. Every effect is mono and processes blocks in place. Each keeps its own
//! `Spec` and recomputes derived values in `configure`, so `live` parameters can change
//! on a running effect without resetting its state (no clicks, tails survive).

use crate::params::{Spec, Value};
use fundsp::prelude32 as fs;
use fundsp::prelude32::AudioUnit;
use signalsmith_stretch::Stretch;
use std::f32::consts::{PI, TAU};

pub trait Effect: Send {
    fn process(&mut self, buf: &mut [f32]);
    /// Change a parameter flagged `live` in the schema.
    fn set(&mut self, _key: &str, _value: Value) {}
}

macro_rules! live_set {
    () => {
        fn set(&mut self, key: &str, value: Value) {
            self.spec.set(key, value);
            self.configure();
        }
    };
}

pub fn parse_spec(name: &str, raw: &str) -> Result<Spec, String> {
    Spec::parse(name, raw)
}

pub fn build(spec: &Spec, sr: f32) -> Box<dyn Effect> {
    let spec = spec.clone();
    match spec.def.name {
        "overdrive" => Box::new(Overdrive::new(spec, sr)),
        "compressor" => Box::new(Compressor::new(spec, sr)),
        "echo" | "delay" => Box::new(DelayFx::new(spec, sr)),
        "chorus" => Box::new(Chorus::new(spec, sr)),
        "flanger" => Box::new(Flanger::new(spec, sr)),
        "phaser" => Box::new(Phaser::new(spec, sr)),
        "tremolo" => Box::new(Tremolo::new(spec, sr)),
        "wah" => Box::new(Wah::new(spec, sr)),
        "harmonics" => Box::new(Harmonics::new(spec, sr)),
        "reverb" => Box::new(Reverb::new(spec, sr)),
        "tone-shift" => Box::new(ToneShift::new(spec, sr)),
        "autotune" => Box::new(Autotune::new(spec, sr)),
        "vocoder" => Box::new(Vocoder::new(spec, sr)),
        other => unreachable!("no effect named {other}"),
    }
}

fn unit(mut u: Box<dyn AudioUnit>, sr: f32) -> Box<dyn AudioUnit> {
    u.set_sample_rate(sr as f64);
    u
}

fn coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms * 0.001 * sr)).exp()
}

fn one_pole(cutoff: f32, sr: f32) -> f32 {
    1.0 - (-TAU * cutoff / sr).exp()
}

// ---------------------------------------------------------------- overdrive

struct Overdrive {
    spec: Spec,
    sr: f32,
    gain: f32,
    makeup: f32,
    a: f32,
    lp: f32,
}

impl Overdrive {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, gain: 1.0, makeup: 1.0, a: 1.0, lp: 0.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.gain = 1.0 + self.spec.num("drive") * 39.0;
        self.makeup = self.spec.num("level") / self.gain.powf(0.35);
        self.a = one_pole(500.0 * 24.0_f32.powf(self.spec.num("tone")), self.sr); // 500 Hz .. 12 kHz
    }
}

impl Effect for Overdrive {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let y = (self.gain * *x).tanh();
            self.lp += self.a * (y - self.lp);
            *x = self.lp * self.makeup;
        }
    }
    live_set!();
}

// --------------------------------------------------------------- compressor

struct Compressor {
    spec: Spec,
    sr: f32,
    threshold: f32,
    slope: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    env: f32,
}

impl Compressor {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, threshold: 0.0, slope: 0.0, attack: 0.0, release: 0.0, makeup: 0.0, env: 0.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        let ratio = self.spec.num("ratio");
        self.threshold = self.spec.num("threshold");
        self.slope = 1.0 - 1.0 / ratio;
        self.attack = coef(self.spec.num("attack"), self.sr);
        self.release = coef(self.spec.num("release"), self.sr);
        self.makeup = match self.spec.get("makeup") {
            Value::Num(db) => db,
            _ => -self.threshold * self.slope * 0.25,
        };
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
    live_set!();
}

// --------------------------------------------------------------- echo/delay

struct DelayFx {
    spec: Spec,
    sr: f32,
    line: Vec<f32>,
    len: usize,
    pos: usize,
    feedback: f32,
    mix: f32,
    damp: Option<f32>, // one-pole lowpass in the feedback path (tape-style echo only)
    damp_state: f32,
}

impl DelayFx {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            line: vec![0.0; (2.0 * sr) as usize + 2],
            len: 1,
            pos: 0,
            feedback: 0.0,
            mix: 0.0,
            damp: None,
            damp_state: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.len = ((self.spec.num("time") * 0.001 * self.sr) as usize).clamp(1, self.line.len());
        self.pos %= self.len;
        self.feedback = self.spec.num("feedback");
        self.mix = self.spec.num("mix");
        self.damp = (self.spec.def.name == "echo")
            .then(|| one_pole(1000.0 * 12.0_f32.powf(self.spec.num("tone")), self.sr)); // 1 kHz .. 12 kHz
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
            self.pos = (self.pos + 1) % self.len;
            *x += self.mix * d;
        }
    }
    live_set!();
}

// -------------------------------------------------- modulated delay (shared)

struct ModDelay {
    buf: Vec<f32>,
    wpos: usize,
}

impl ModDelay {
    fn new(max_samples: usize) -> Self {
        Self { buf: vec![0.0; max_samples + 4], wpos: 0 }
    }

    /// Read `delay` samples back from the write head (fractional, linear interpolation).
    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let pos = (self.wpos as f32 - delay - 1.0).rem_euclid(len as f32);
        let i = (pos as usize) % len;
        let frac = (pos - i as f32).clamp(0.0, 1.0);
        let (a, b) = (self.buf[i], self.buf[(i + 1) % len]);
        a + (b - a) * frac
    }

    fn push(&mut self, x: f32) {
        self.buf[self.wpos] = x;
        self.wpos = (self.wpos + 1) % self.buf.len();
    }
}

// ------------------------------------------------------------------- chorus

struct Chorus {
    spec: Spec,
    sr: f32,
    line: ModDelay,
    phase: f32,
    step: f32,
    depth: f32, // samples
    base: f32,  // samples
    mix: f32,
}

impl Chorus {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            line: ModDelay::new((0.04 * sr) as usize),
            phase: 0.0,
            step: 0.0,
            depth: 0.0,
            base: 0.0,
            mix: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.step = self.spec.num("rate") / self.sr;
        self.depth = (0.0015 + self.spec.num("depth") * 0.0065) * self.sr;
        self.base = 0.014 * self.sr;
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Chorus {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.phase = (self.phase + self.step).fract();
            let mut wet = 0.0;
            for v in 0..3 {
                let p = (self.phase + v as f32 / 3.0).fract();
                wet += self.line.read(self.base + self.depth * (TAU * p).sin());
            }
            self.line.push(*x);
            *x = (*x + self.mix * wet / 3.0) / (1.0 + self.mix);
        }
    }
    live_set!();
}

// ------------------------------------------------------------------ flanger

struct Flanger {
    spec: Spec,
    sr: f32,
    line: ModDelay,
    phase: f32,
    step: f32,
    lo: f32,
    hi: f32,
    feedback: f32,
    mix: f32,
}

impl Flanger {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            line: ModDelay::new((0.02 * sr) as usize),
            phase: 0.0,
            step: 0.0,
            lo: 0.0,
            hi: 0.0,
            feedback: 0.0,
            mix: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.step = self.spec.num("rate") / self.sr;
        self.lo = 0.0005 * self.sr;
        self.hi = (0.001 + self.spec.num("depth") * 0.008) * self.sr;
        self.feedback = self.spec.num("feedback");
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Flanger {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.phase = (self.phase + self.step).fract();
            let d = self.lo + (self.hi - self.lo) * (0.5 + 0.5 * (TAU * self.phase).sin());
            let wet = self.line.read(d);
            self.line.push(*x + self.feedback * wet);
            *x = (*x + self.mix * wet) / (1.0 + self.mix);
        }
    }
    live_set!();
}

// ------------------------------------------------------------------- phaser

struct Phaser {
    spec: Spec,
    sr: f32,
    phase: f32,
    step: f32,
    depth: f32,
    feedback: f32,
    mix: f32,
    z: [f32; 4],
    last: f32,
}

impl Phaser {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, phase: 0.0, step: 0.0, depth: 0.0, feedback: 0.0, mix: 0.0, z: [0.0; 4], last: 0.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.step = self.spec.num("rate") / self.sr;
        self.depth = self.spec.num("depth");
        self.feedback = self.spec.num("feedback");
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Phaser {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.phase = (self.phase + self.step).fract();
            let sweep = 0.5 + 0.5 * (TAU * self.phase).sin();
            let fc = (300.0 * 10f32.powf(self.depth * sweep)).min(self.sr * 0.4);
            let t = (PI * fc / self.sr).tan();
            let a = (t - 1.0) / (t + 1.0);
            let mut s = *x + self.feedback * self.last;
            for z in &mut self.z {
                let y = a * s + *z;
                *z = s - a * y;
                s = y;
            }
            self.last = s;
            *x = (*x + self.mix * s) / (1.0 + self.mix);
        }
    }
    live_set!();
}

// ------------------------------------------------------------------ tremolo

struct Tremolo {
    spec: Spec,
    sr: f32,
    phase: f32,
    step: f32,
    depth: f32,
}

impl Tremolo {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, phase: 0.0, step: 0.0, depth: 0.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.step = self.spec.num("rate") / self.sr;
        self.depth = self.spec.num("depth");
    }
}

impl Effect for Tremolo {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.phase = (self.phase + self.step).fract();
            let lfo = 0.5 + 0.5 * (TAU * self.phase).sin();
            *x *= 1.0 - self.depth * lfo;
        }
    }
    live_set!();
}

// ---------------------------------------------------------------------- wah

/// Auto-wah: a resonant band-pass swept by your picking (`rate=0`) or by an LFO.
struct Wah {
    spec: Spec,
    sr: f32,
    sens: f32,
    lfo_step: f32,
    lfo_phase: f32,
    damp: f32,
    gain: f32,
    min: f32,
    max: f32,
    mix: f32,
    env: f32,
    att: f32,
    rel: f32,
    low: f32,
    band: f32,
}

impl Wah {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            sens: 0.0,
            lfo_step: 0.0,
            lfo_phase: 0.0,
            damp: 0.0,
            gain: 0.0,
            min: 0.0,
            max: 0.0,
            mix: 0.0,
            env: 0.0,
            att: coef(4.0, sr),
            rel: coef(90.0, sr),
            low: 0.0,
            band: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        let q = self.spec.num("q");
        self.sens = self.spec.num("sensitivity");
        self.lfo_step = self.spec.num("rate") / self.sr;
        self.damp = 1.0 / q;
        self.gain = q.sqrt() * 2.0;
        self.min = self.spec.num("min");
        self.max = self.spec.num("max").max(self.min * 1.2);
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Wah {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let pos = if self.lfo_step > 0.0 {
                self.lfo_phase = (self.lfo_phase + self.lfo_step).fract();
                0.5 + 0.5 * (TAU * self.lfo_phase).sin()
            } else {
                let l = x.abs();
                let c = if l > self.env { self.att } else { self.rel };
                self.env = l + c * (self.env - l);
                (self.env * (3.0 + 40.0 * self.sens)).clamp(0.0, 1.0)
            };
            let fc = (self.min * (self.max / self.min).powf(pos)).min(self.sr / 6.0);
            let f = 2.0 * (PI * fc / self.sr).sin();
            self.low += f * self.band;
            let high = *x - self.low - self.damp * self.band;
            self.band += f * high;
            let wet = self.band * self.damp * self.gain;
            *x = *x * (1.0 - self.mix) + wet * self.mix;
        }
    }
    live_set!();
}

// ---------------------------------------------------------------- harmonics

#[derive(Default)]
struct HighPass {
    a: f32,
    x1: f32,
    y1: f32,
}

impl HighPass {
    fn set(&mut self, cutoff: f32, sr: f32) {
        self.a = 1.0 / (1.0 + TAU * cutoff / sr);
    }

    fn tick(&mut self, x: f32) -> f32 {
        let y = self.a * (self.y1 + x - self.x1);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// Exciter: distorts the high band and adds it back for extra harmonics.
struct Harmonics {
    spec: Spec,
    sr: f32,
    pre: [HighPass; 2],
    post: [HighPass; 2],
    gain: f32,
    mix: f32,
}

impl Harmonics {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, pre: Default::default(), post: Default::default(), gain: 1.0, mix: 0.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        let freq = self.spec.num("freq");
        for f in self.pre.iter_mut().chain(self.post.iter_mut()) {
            f.set(freq, self.sr);
        }
        self.gain = 1.0 + self.spec.num("drive") * 9.0;
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Harmonics {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let mut h = *x;
            for f in &mut self.pre {
                h = f.tick(h);
            }
            h = (h * self.gain).tanh();
            for f in &mut self.post {
                h = f.tick(h);
            }
            *x += self.mix * h;
        }
    }
    live_set!();
}

// ------------------------------------------------------------------- reverb

struct Reverb {
    spec: Spec,
    inner: Box<dyn AudioUnit>,
    mix: f32,
}

impl Reverb {
    fn new(spec: Spec, sr: f32) -> Self {
        let inner = fs::reverb_stereo(spec.num("size"), spec.num("time"), spec.num("damping"));
        let mix = spec.num("mix");
        Self { spec, inner: unit(Box::new(inner), sr), mix }
    }

    fn configure(&mut self) {
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Reverb {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let (l, r) = self.inner.filter_stereo(*x, *x);
            *x += self.mix * 0.5 * (l + r);
        }
    }
    live_set!();
}

// ------------------------------------------------------------- pitch tracker

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

// ------------------------------------------------------------ pitch shifter

/// A pitch shifter backend. `high` is the phase-vocoder-style Signalsmith Stretch
/// (cleanest, ~50+ ms latency). `fast` is a pitch-synchronous delay-line shifter: the
/// detected note period locks its two taps in phase, so latency is only about one or
/// two periods (about 10 ms) and the pitch is exact for a single sustained note.
/// It tracks one note at a time, so chords and noise degrade it.
enum Shifter {
    High(Stretch, Vec<f32>),
    Fast(Granular, PitchTracker),
}

impl Shifter {
    fn new(spec: &Spec, sr: f32) -> Self {
        match spec.option("mode") {
            "fast" => Shifter::Fast(Granular::new(spec.num("window"), sr), PitchTracker::new(sr)),
            _ => Shifter::High(Stretch::preset_default(1, sr as u32), Vec::new()),
        }
    }

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
    line: ModDelay,
    u: f32,
    delta: f32,
    target: f32,
    max_delta: f32,
}

impl Granular {
    fn new(window_ms: f32, sr: f32) -> Self {
        let max_delta = 0.06 * sr;
        let target = (window_ms * 0.001 * sr).clamp(16.0, max_delta);
        Self { line: ModDelay::new(max_delta as usize * 2 + 4), u: 0.0, delta: target, target, max_delta }
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

    fn tick(&mut self, x: f32, ratio: f32) -> f32 {
        self.line.push(x);
        self.u = (self.u + (1.0 - ratio)).rem_euclid(self.delta);
        let s = self.u / self.delta;
        // `push` already advanced the write head, so a delay of 0 reads the newest sample.
        (1.0 - s) * self.line.read(self.u) + s * self.line.read(self.u + self.delta)
    }
}

struct ToneShift {
    spec: Spec,
    shifter: Shifter,
    semitones: f32,
    mix: f32,
    scratch: Vec<f32>,
}

impl ToneShift {
    fn new(spec: Spec, sr: f32) -> Self {
        let shifter = Shifter::new(&spec, sr);
        let mut s = Self { spec, shifter, semitones: 0.0, mix: 1.0, scratch: Vec::new() };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.semitones = self.spec.num("semitones") + self.spec.num("cents") / 100.0;
        self.mix = self.spec.num("mix");
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
    live_set!();
}

// ------------------------------------------------------------------ vocoder

/// Channel vocoder. The input is the modulator; the carrier is a built-in
/// saw stack (root, fifth, octave).
struct Vocoder {
    spec: Spec,
    sr: f32,
    mod_filters: Vec<Box<dyn AudioUnit>>,
    car_filters: Vec<Box<dyn AudioUnit>>,
    envs: Vec<f32>,
    release: f32,
    phases: [f32; 3],
    steps: [f32; 3],
    mix: f32,
}

impl Vocoder {
    fn new(spec: Spec, sr: f32) -> Self {
        let bands = spec.num("bands") as usize;
        let carrier = spec.num("carrier");
        let (lo, hi) = (120.0_f32, 7000.0_f32);
        let centers: Vec<f32> = (0..bands).map(|i| lo * (hi / lo).powf(i as f32 / (bands - 1) as f32)).collect();
        let q = 4.0 + bands as f32 / 4.0;
        let mk = |f: f32| unit(Box::new(fs::bandpass_hz(f, q)), sr);
        let mut s = Self {
            spec,
            sr,
            mod_filters: centers.iter().map(|f| mk(*f)).collect(),
            car_filters: centers.iter().map(|f| mk(*f)).collect(),
            envs: vec![0.0; bands],
            release: 0.0,
            phases: [0.0; 3],
            steps: [carrier / sr, carrier * 1.5 / sr, carrier * 2.0 / sr],
            mix: 1.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.release = coef(self.spec.num("release"), self.sr);
        self.mix = self.spec.num("mix");
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
    live_set!();
}

// ----------------------------------------------------------------- autotune

struct Autotune {
    spec: Spec,
    shifter: Shifter,
    tracker: PitchTracker,
    allowed: [bool; 12], // absolute pitch classes
    speed_ms: f32,
    amount: f32,
    sr: f32,
    target: f32,
    current: f32,
}

/// Scale steps in the order of `params::SCALES`.
const SCALE_STEPS: [&[usize]; 6] = [
    &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], // chromatic
    &[0, 2, 4, 5, 7, 9, 11],                 // major
    &[0, 2, 3, 5, 7, 8, 10],                 // minor
    &[0, 2, 4, 7, 9],                        // pentatonic
    &[0, 3, 5, 7, 10],                       // minor-pentatonic
    &[0, 3, 5, 6, 7, 10],                    // blues
];

impl Autotune {
    fn new(spec: Spec, sr: f32) -> Self {
        let shifter = Shifter::new(&spec, sr);
        let mut s = Self {
            spec,
            shifter,
            tracker: PitchTracker::new(sr),
            allowed: [true; 12],
            speed_ms: 0.0,
            amount: 1.0,
            sr,
            target: 0.0,
            current: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        let root = self.spec.choice("key");
        self.allowed = [false; 12];
        for step in SCALE_STEPS[self.spec.choice("scale")] {
            self.allowed[(root + step) % 12] = true;
        }
        self.speed_ms = self.spec.num("speed");
        self.amount = self.spec.num("amount");
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
                    .min_by(|a, b| (*a as f32 - midi).abs().partial_cmp(&(*b as f32 - midi).abs()).unwrap())
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
    live_set!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::Chain;

    const SR: f32 = 48000.0;

    fn sine(freq: f32, secs: f32) -> Vec<f32> {
        (0..(SR * secs) as usize).map(|n| 0.5 * (TAU * freq * n as f32 / SR).sin()).collect()
    }

    /// Frequency from upward zero crossings over the second half of the buffer.
    fn measure(x: &[f32]) -> f32 {
        let tail = &x[x.len() / 2..];
        let crossings = tail.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 / (tail.len() as f32 / SR)
    }

    fn run_chain(items: &[(&str, &str)], input: &[f32]) -> Vec<f32> {
        let specs: Vec<Spec> = items.iter().map(|(n, v)| parse_spec(n, v).unwrap()).collect();
        let mut chain = Chain::new(&specs, SR, 0.0, 0.0);
        let mut out = input.to_vec();
        for block in out.chunks_mut(256) {
            chain.process(block);
        }
        out
    }

    #[test]
    fn fast_shifter_hits_target_pitch() {
        for semis in [-12.0f32, -7.0, -2.0, 2.0, 7.0, 12.0] {
            let out = run_chain(&[("tone-shift", &format!("semitones={semis},mode=fast"))], &sine(220.0, 1.5));
            let want = 220.0 * 2f32.powf(semis / 12.0);
            let got = measure(&out);
            assert!((got / want - 1.0).abs() < 0.03, "{semis:+} st: wanted {want:.1} Hz, got {got:.1} Hz");
        }
    }

    #[test]
    fn high_quality_shifter_hits_target_pitch() {
        let out = run_chain(&[("tone-shift", "semitones=7")], &sine(220.0, 2.0));
        let want = 220.0 * 2f32.powf(7.0 / 12.0);
        let got = measure(&out);
        assert!((got / want - 1.0).abs() < 0.02, "wanted {want:.1} Hz, got {got:.1} Hz");
    }

    #[test]
    fn autotune_snaps_to_nearest_note() {
        let sharp = 220.0 * 2f32.powf(0.35 / 12.0); // A3 + 35 cents
        for mode in ["high", "fast"] {
            let out = run_chain(&[("autotune", &format!("speed=0,mode={mode}"))], &sine(sharp, 2.0));
            let got = measure(&out);
            assert!((got / 220.0 - 1.0).abs() < 0.01, "{mode}: wanted ~220 Hz, got {got:.1} Hz");
        }
    }

    #[test]
    fn detects_guitar_range() {
        for f in [82.41f32, 110.0, 196.0, 329.63, 659.26] {
            let x: Vec<f32> = (0..WINDOW).map(|n| 0.4 * (TAU * f * n as f32 / SR).sin()).collect();
            let mut t = PitchTracker::new(SR);
            t.push(&x);
            let got = t.detect().unwrap_or(0.0);
            assert!((got / f - 1.0).abs() < 0.01, "wanted {f} Hz, got {got:.2} Hz");
        }
    }

    #[test]
    fn every_effect_is_finite_and_bounded() {
        for def in crate::params::EFFECTS {
            let out = run_chain(&[(def.name, "")], &sine(196.0, 1.0));
            assert!(out.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{} produced bad samples", def.name);
        }
    }

    #[test]
    fn live_parameter_changes_stay_finite() {
        // Sweep every live parameter of every effect across its range while audio runs.
        use crate::params::Kind;
        for def in crate::params::EFFECTS {
            let mut fx = build(&Spec::new(def), SR);
            let mut block = sine(196.0, 0.01);
            for step in 0..40 {
                for p in def.params.iter().filter(|p| p.live) {
                    let t = (step % 8) as f32 / 7.0;
                    let v = match p.kind {
                        Kind::Num { min, max, .. } | Kind::AutoNum { min, max, .. } => Value::Num(min + (max - min) * t),
                        Kind::Choice { options, .. } => Value::Choice(step % options.len()),
                    };
                    fx.set(p.key, v);
                }
                block.copy_from_slice(&sine(196.0, 0.01));
                fx.process(&mut block);
                assert!(block.iter().all(|v| v.is_finite() && v.abs() < 20.0), "{} went bad at step {step}", def.name);
            }
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

    #[test]
    fn bad_parameters_are_rejected() {
        assert!(parse_spec("overdrive", "tune=1").is_err());
        assert!(parse_spec("compressor", "ratio=99").is_err());
        assert!(parse_spec("autotune", "key=H").is_err());
        assert!(parse_spec("tone-shift", "mode=slow").is_err());
        assert!(parse_spec("compressor", "makeup=auto").is_ok());
        assert!(parse_spec("autotune", "key=Db,scale=minor").is_ok());
    }

    #[test]
    fn spec_round_trips_through_raw() {
        let s = parse_spec("autotune", "key=F#,scale=blues,speed=0,mode=fast").unwrap();
        let again = parse_spec("autotune", &s.raw()).unwrap();
        assert_eq!(s.values, again.values);
        assert_eq!(parse_spec("overdrive", "").unwrap().raw(), "");
    }
}
