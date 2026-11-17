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
        "transient" => Box::new(Transient::new(spec, sr)),
        "freeze" => Box::new(Freeze::new(spec, sr)),
        "pick-noise" => Box::new(PickNoise::new(spec, sr)),
        "bitcrusher" => Box::new(Bitcrusher::new(spec)),
        "ringmod" => Box::new(RingMod::new(spec, sr)),
        "reverse" => Box::new(Reverse::new(spec, sr)),
        "kalimba" => Box::new(Kalimba::new(spec, sr)),
        "wind" => Box::new(Wind::new(spec, sr)),
        "swell" => Box::new(Swell::new(spec, sr)),
        "shimmer" => Box::new(Shimmer::new(spec, sr)),
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

// ------------------------------------------------- experimental and zen effects

/// Cheap white noise in -1..1.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// Spots the moment a string is picked: a fast envelope jumping above a slow one.
struct Onset {
    fast: f32,
    slow: f32,
    hold: usize,
    hold_len: usize,
    ratio: f32,
    fa: f32,
    fr: f32,
    sa: f32,
    sr_: f32,
}

impl Onset {
    fn new(sr: f32) -> Self {
        Self {
            fast: 0.0,
            slow: 0.0,
            hold: 0,
            hold_len: (0.08 * sr) as usize,
            ratio: 2.2,
            fa: coef(0.5, sr),
            fr: coef(8.0, sr),
            sa: coef(30.0, sr),
            sr_: coef(150.0, sr),
        }
    }

    fn set_sensitivity(&mut self, s: f32) {
        self.ratio = 3.0 - 1.6 * s.clamp(0.0, 1.0);
    }

    /// Returns the level when a note is picked at this sample.
    fn tick(&mut self, x: f32) -> Option<f32> {
        let a = x.abs();
        self.fast = a + (self.fast - a) * if a > self.fast { self.fa } else { self.fr };
        self.slow = a + (self.slow - a) * if a > self.slow { self.sa } else { self.sr_ };
        if self.hold > 0 {
            self.hold -= 1;
            return None;
        }
        if self.fast > self.slow * self.ratio + 0.004 && self.fast > 0.02 {
            self.hold = self.hold_len;
            return Some(self.fast);
        }
        None
    }
}

// --------------------------------------------------------- transient shaper

/// Transient shaper (a.k.a. attack control / pick softener). Two envelope followers with
/// different attack times; while the fast one leads, a note is being struck, and that
/// excess is turned down (`attack` < 0, softer pick) or up (`attack` > 0, snappier).
struct Transient {
    spec: Spec,
    sr: f32,
    fast: f32,
    slow: f32,
    gain: f32,
    fa: f32,
    sa: f32,
    rel: f32,
    fa_rel: f32,
    gk: f32,
    amount: f32,
    mix: f32,
}

impl Transient {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            fast: 0.0,
            slow: 0.0,
            gain: 1.0,
            fa: coef(0.3, sr),
            sa: 0.0,
            rel: coef(40.0, sr),
            fa_rel: coef(3.0, sr),
            gk: coef(0.5, sr),
            amount: 0.0,
            mix: 1.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.amount = self.spec.num("attack") * 1.4;
        self.sa = coef(self.spec.num("time"), self.sr);
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Transient {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let a = x.abs();
            self.fast = a + (self.fast - a) * if a > self.fast { self.fa } else { self.rel };
            // The slow follower trails the fast envelope (not the raw rectified wave), so a
            // steady or decaying note gives fast == slow and only a rising attack differs.
            let f = self.fast;
            self.slow = f + (self.slow - f) * if f > self.slow { self.sa } else { self.fa_rel };
            let d = ((self.fast + 1e-4) / (self.slow + 1e-4)).ln().max(0.0);
            let target = (self.amount * d).exp().clamp(0.0, 4.0);
            self.gain = target + (self.gain - target) * self.gk;
            *x += (*x * self.gain - *x) * self.mix;
        }
    }
    live_set!();
}

// ------------------------------------------------------------------ freeze

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Idle,
    Settle(usize),
    Record(usize),
    Play,
}

/// Prolonger: after each pick, waits for the attack to pass, samples one cycle (a whole
/// number of detected periods) and loops it under the dry sound until the next pick.
struct Freeze {
    spec: Spec,
    sr: f32,
    onset: Onset,
    tracker: PitchTracker,
    buf: Vec<f32>,
    n: usize,
    xf: usize,
    phase: Phase,
    pos: usize,
    fade: f32,
    env: f32,
    mix: f32,
    decay: f32,
    settle: usize,
    length: f32,
}

impl Freeze {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            onset: Onset::new(sr),
            tracker: PitchTracker::new(sr),
            buf: vec![0.0; (0.5 * sr) as usize],
            n: 1,
            xf: 1,
            phase: Phase::Idle,
            pos: 0,
            fade: 0.0,
            env: 0.0,
            mix: 0.0,
            decay: 1.0,
            settle: 0,
            length: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.mix = self.spec.num("mix");
        self.decay = (-1.0 / (self.spec.num("sustain") * self.sr)).exp();
        self.settle = (self.spec.num("settle") * 0.001 * self.sr) as usize;
        self.length = self.spec.num("length") * 0.001 * self.sr;
        self.onset.set_sensitivity(self.spec.num("sensitivity"));
    }

    fn start_recording(&mut self) {
        let cap = self.buf.len();
        let mut n = self.length;
        if let Some(f0) = self.tracker.detect() {
            let period = self.sr / f0;
            n = (self.length / period).round().max(1.0) * period;
        }
        self.xf = ((0.002 * self.sr) as usize).max(8);
        self.n = (n.round() as usize).clamp(64, cap - self.xf - 1);
        self.xf = self.xf.min(self.n / 4);
    }
}

impl Effect for Freeze {
    fn process(&mut self, buf: &mut [f32]) {
        self.tracker.push(buf);
        let down = 1.0 / (0.008 * self.sr);
        for x in buf {
            let dry = *x;
            if self.onset.tick(dry).is_some() {
                self.phase = Phase::Settle(self.settle);
            }
            match self.phase {
                Phase::Settle(0) => {
                    self.start_recording();
                    self.phase = Phase::Record(0);
                }
                Phase::Settle(c) => self.phase = Phase::Settle(c - 1),
                Phase::Record(i) => {
                    self.buf[i] = dry;
                    self.phase = if i + 1 >= self.n + self.xf {
                        self.pos = 0;
                        self.env = 1.0;
                        Phase::Play
                    } else {
                        Phase::Record(i + 1)
                    };
                }
                _ => {}
            }
            let playing = self.phase == Phase::Play;
            self.fade = if playing { (self.fade + down * 2.0).min(1.0) } else { (self.fade - down).max(0.0) };
            if self.fade > 0.0 {
                let p = self.pos;
                let s = if p < self.xf {
                    let w = p as f32 / self.xf as f32;
                    self.buf[p] * w + self.buf[p + self.n] * (1.0 - w)
                } else {
                    self.buf[p]
                };
                *x = dry + s * self.mix * self.fade * self.env;
                self.env *= self.decay;
                self.pos = if p + 1 >= self.n { 0 } else { p + 1 };
            }
        }
    }
    live_set!();
}

// -------------------------------------------------------------- pick noise

/// White noise on the picked note: a short burst at each pick, or noise riding the
/// note's own envelope.
struct PickNoise {
    spec: Spec,
    sr: f32,
    onset: Onset,
    rng: Noise,
    burst: f32,
    lvl: f32,
    since: usize,
    follow: f32,
    lp: f32,
    amount: f32,
    decay_k: f32,
    release_k: f32,
    tone_k: f32,
    comp: f32,
    follow_mode: bool,
}

impl PickNoise {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            onset: Onset::new(sr),
            rng: Noise(0x9E37_79B9),
            burst: 0.0,
            lvl: 0.0,
            since: usize::MAX,
            follow: 0.0,
            lp: 0.0,
            amount: 0.0,
            decay_k: 0.0,
            release_k: 0.0,
            tone_k: 1.0,
            comp: 1.0,
            follow_mode: false,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.amount = self.spec.num("amount");
        let ms = self.spec.num("decay");
        self.decay_k = coef(ms, self.sr);
        self.release_k = coef(ms, self.sr);
        let tone = self.spec.num("tone");
        if tone >= 0.99 {
            self.tone_k = 1.0;
            self.comp = 1.0;
        } else {
            self.tone_k = one_pole(200.0 * (15000.0f32 / 200.0).powf(tone), self.sr);
            self.comp = ((2.0 - self.tone_k) / self.tone_k).sqrt();
        }
        self.follow_mode = self.spec.option("mode") == "follow";
        self.onset.set_sensitivity(self.spec.num("sensitivity"));
    }
}

impl Effect for PickNoise {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let dry = *x;
            if let Some(l) = self.onset.tick(dry) {
                self.burst = 1.0;
                self.lvl = l;
                self.since = 0;
            }
            if self.since < (0.015 * self.sr) as usize {
                self.lvl = self.lvl.max(self.onset.fast);
                self.since += 1;
            }
            let a = dry.abs();
            self.follow = if a > self.follow { a } else { a + (self.follow - a) * self.release_k };
            let amp = if self.follow_mode { self.follow } else { self.burst * self.lvl };
            self.burst *= self.decay_k;
            let white = self.rng.next();
            self.lp += self.tone_k * (white - self.lp);
            let noise = if self.tone_k >= 1.0 { white } else { self.lp * self.comp };
            *x = dry + noise * amp * self.amount * 2.0;
        }
    }
    live_set!();
}

// -------------------------------------------------------------- bitcrusher

struct Bitcrusher {
    spec: Spec,
    sr: f32,
    held: f32,
    acc: f32,
    levels: f32,
    step: f32,
    mix: f32,
}

impl Bitcrusher {
    fn new(spec: Spec) -> Self {
        let mut s = Self { spec, sr: 48000.0, held: 0.0, acc: 1.0, levels: 128.0, step: 1.0, mix: 1.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.levels = 2f32.powf(self.spec.num("bits")) / 2.0;
        self.step = self.spec.num("rate") / self.sr;
        self.mix = self.spec.num("mix");
    }
}

impl Effect for Bitcrusher {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.acc += self.step;
            if self.acc >= 1.0 {
                self.acc -= 1.0;
                self.held = (*x * self.levels).round() / self.levels;
            }
            *x += (self.held - *x) * self.mix;
        }
    }
    live_set!();
}

// ----------------------------------------------------------------- ring mod

struct RingMod {
    spec: Spec,
    sr: f32,
    phase: f32,
    step: f32,
    mix: f32,
    square: bool,
}

impl RingMod {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, phase: 0.0, step: 0.0, mix: 0.5, square: false };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.step = self.spec.num("freq") / self.sr;
        self.mix = self.spec.num("mix");
        self.square = self.spec.option("shape") == "square";
    }
}

impl Effect for RingMod {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            self.phase = (self.phase + self.step).fract();
            let c = (TAU * self.phase).sin();
            let c = if self.square { c.signum() } else { c };
            *x += (*x * c - *x) * self.mix;
        }
    }
    live_set!();
}

// ------------------------------------------------------------------ reverse

/// Plays the last `time` ms backwards, two overlapping windows so it is seamless.
struct Reverse {
    spec: Spec,
    ring: Vec<f32>,
    w: usize,
    phase: f32,
    len: f32,
    mix: f32,
}

impl Reverse {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, ring: vec![0.0; (4.0 * sr) as usize + 8], w: 0, phase: 0.0, len: 1.0, mix: 0.7 };
        s.sr_set(sr);
        s
    }

    fn sr_set(&mut self, sr: f32) {
        self.len = self.spec.num("time") * 0.001 * sr;
        self.mix = self.spec.num("mix");
    }

    fn configure(&mut self) {
        let sr = (self.ring.len() - 8) as f32 / 4.0;
        self.sr_set(sr);
    }
}

impl Effect for Reverse {
    fn process(&mut self, buf: &mut [f32]) {
        let n = self.ring.len();
        for x in buf {
            self.ring[self.w] = *x;
            let mut wet = 0.0;
            for j in 0..2 {
                let ph = (self.phase + 0.5 * j as f32).fract();
                let pos = (self.w as f32 - 2.0 * ph * self.len).rem_euclid(n as f32);
                let i = pos as usize % n;
                let fr = pos - pos.floor();
                let v = self.ring[i] * (1.0 - fr) + self.ring[(i + 1) % n] * fr;
                wet += v * (PI * ph).sin().powi(2);
            }
            self.phase = (self.phase + 1.0 / self.len).fract();
            self.w = (self.w + 1) % n;
            *x += (wet - *x) * self.mix;
        }
    }
    live_set!();
}

// ----------------------------------------------------------------- kalimba

#[derive(Clone, Copy, Default)]
struct Tine {
    ph: f32,
    inc: f32,
    amp: f32,
    age: usize,
}

/// Turns each picked note into a kalimba tine: the pitch is tracked and a plucked,
/// slightly inharmonic metal tone is played at it (optionally octaves up).
struct Kalimba {
    spec: Spec,
    sr: f32,
    onset: Onset,
    tracker: PitchTracker,
    pending: Option<(usize, f32)>,
    tines: [Tine; 8],
    mix: f32,
    dry: f32,
    octave: f32,
    k_main: f32,
    k_over: f32,
}

impl Kalimba {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            onset: Onset::new(sr),
            tracker: PitchTracker::new(sr),
            pending: None,
            tines: [Tine::default(); 8],
            mix: 0.0,
            dry: 1.0,
            octave: 1.0,
            k_main: 0.0,
            k_over: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.mix = self.spec.num("mix");
        self.dry = self.spec.num("dry");
        self.octave = self.spec.num("octave").round();
        let ms = self.spec.num("decay");
        self.k_main = coef(ms, self.sr);
        self.k_over = coef(ms * 0.12, self.sr);
        self.onset.set_sensitivity(self.spec.num("sensitivity"));
    }
}

impl Effect for Kalimba {
    fn process(&mut self, buf: &mut [f32]) {
        self.tracker.push(buf);
        for x in buf {
            let dry = *x;
            if let Some(l) = self.onset.tick(dry) {
                self.pending = Some(((0.035 * self.sr) as usize, l));
            }
            if let Some((c, l)) = self.pending {
                if c == 0 {
                    self.pending = None;
                    if let Some(f0) = self.tracker.detect() {
                        let f = (f0 * 2f32.powf(self.octave)).min(4000.0);
                        let i = (0..8).min_by_key(|&i| if self.tines[i].amp < 1e-4 { 0 } else { 1 + self.tines[i].age.min(1 << 30) as i64 * -1 }).unwrap_or(0);
                        self.tines[i] = Tine { ph: 0.0, inc: f / self.sr, amp: (l * 3.0).clamp(0.05, 0.9), age: 0 };
                    }
                } else {
                    self.pending = Some((c - 1, l));
                }
            }
            let mut out = 0.0;
            for t in &mut self.tines {
                if t.amp < 1e-5 {
                    continue;
                }
                t.ph = (t.ph + t.inc).fract();
                let over = (TAU * t.ph * 5.4).sin() * 0.3 * (self.k_over.powi(t.age.min(1 << 20) as i32));
                out += t.amp * ((TAU * t.ph).sin() + over);
                t.amp *= self.k_main;
                t.age += 1;
            }
            *x = dry * self.dry + out * self.mix;
        }
    }
    live_set!();
}

// -------------------------------------------------------------------- wind

/// Ethereal wind: noise through two slowly drifting resonant band-passes, breathing
/// with your playing.
struct Wind {
    spec: Spec,
    sr: f32,
    rng: Noise,
    lfo: [f32; 3],
    state: [[f32; 2]; 2],
    follow: f32,
    mix: f32,
    level: f32,
    follow_amt: f32,
    rate: f32,
    q: f32,
    rel: f32,
}

impl Wind {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            rng: Noise(0x1234_5678),
            lfo: [0.0, 0.31, 0.62],
            state: [[0.0; 2]; 2],
            follow: 0.0,
            mix: 0.0,
            level: 0.0,
            follow_amt: 0.0,
            rate: 0.0,
            q: 5.0,
            rel: coef(400.0, sr),
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.mix = self.spec.num("mix");
        self.level = self.spec.num("level");
        self.follow_amt = self.spec.num("follow");
        self.rate = self.spec.num("rate");
        self.q = self.spec.num("q");
    }
}

impl Effect for Wind {
    fn process(&mut self, buf: &mut [f32]) {
        let rates = [self.rate, self.rate * 0.63, self.rate * 0.37];
        for x in buf {
            let dry = *x;
            for (p, r) in self.lfo.iter_mut().zip(rates) {
                *p = (*p + r / self.sr).fract();
            }
            let a = dry.abs();
            self.follow = if a > self.follow { a } else { a + (self.follow - a) * self.rel };
            let breath = (self.follow * 5.0).min(1.0);
            let gust = 0.65 + 0.35 * (TAU * self.lfo[2]).sin();
            let amp = (self.level * (1.0 - self.follow_amt) + breath * self.follow_amt) * gust;
            let white = self.rng.next();
            let mut wind = 0.0;
            for (b, st) in self.state.iter_mut().enumerate() {
                let (base, span) = if b == 0 { (400.0, 2.0) } else { (900.0, 1.5) };
                let fc = base * 2f32.powf(span * (0.5 + 0.5 * (TAU * self.lfo[b]).sin()));
                let f = 2.0 * (PI * fc / self.sr).sin();
                let high = white - st[0] - st[1] / self.q;
                st[1] += f * high;
                st[0] += f * st[1];
                wind += st[1];
            }
            *x = dry + wind * amp * self.mix * 0.35 * self.q.sqrt();
        }
    }
    live_set!();
}

// ------------------------------------------------------------------- swell

/// Volume swell: each pick fades in slowly, like a bowed or pad-like note.
struct Swell {
    spec: Spec,
    sr: f32,
    onset: Onset,
    gain: f32,
    down: bool,
    t: usize,
    rise: usize,
}

impl Swell {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self { spec, sr, onset: Onset::new(sr), gain: 1.0, down: false, t: usize::MAX, rise: 1 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.rise = ((self.spec.num("rise") * 0.001 * self.sr) as usize).max(1);
        self.onset.set_sensitivity(self.spec.num("sensitivity"));
    }
}

impl Effect for Swell {
    fn process(&mut self, buf: &mut [f32]) {
        let dn = 1.0 / (0.002 * self.sr);
        for x in buf {
            if self.onset.tick(*x).is_some() {
                self.down = true;
            }
            if self.down {
                self.gain -= dn;
                if self.gain <= 0.0 {
                    self.gain = 0.0;
                    self.down = false;
                    self.t = 0;
                }
            } else if self.t < self.rise {
                self.t += 1;
                let u = self.t as f32 / self.rise as f32;
                self.gain = u * u * (3.0 - 2.0 * u);
            } else {
                self.gain = 1.0;
            }
            *x *= self.gain;
        }
    }
    live_set!();
}

// ----------------------------------------------------------------- shimmer

/// Reverb whose tail is fed back through a pitch shifter (an octave up by default),
/// so the wash climbs into shimmering overtones.
struct Shimmer {
    spec: Spec,
    reverb: Box<dyn AudioUnit>,
    stretch: Stretch,
    fed: Vec<f32>,
    wet: Vec<f32>,
    up: Vec<f32>,
    mix: f32,
    shimmer: f32,
    pitch: f32,
}

impl Shimmer {
    fn new(spec: Spec, sr: f32) -> Self {
        let reverb = fs::reverb_stereo(spec.num("size"), spec.num("time"), 0.5);
        let mut s = Self {
            spec,
            reverb: unit(Box::new(reverb), sr),
            stretch: Stretch::preset_default(1, sr as u32),
            fed: Vec::new(),
            wet: Vec::new(),
            up: Vec::new(),
            mix: 0.0,
            shimmer: 0.0,
            pitch: 12.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.mix = self.spec.num("mix");
        self.shimmer = self.spec.num("shimmer");
        self.pitch = self.spec.num("pitch");
    }
}

impl Effect for Shimmer {
    fn process(&mut self, buf: &mut [f32]) {
        let n = buf.len();
        if self.up.len() != n {
            self.up = vec![0.0; n]; // block size changed: drop the old feedback
        }
        self.wet.clear();
        for (x, up) in buf.iter().zip(&self.up) {
            let fed = *x + (up * self.shimmer * 0.6).tanh();
            let (l, r) = self.reverb.filter_stereo(fed, fed);
            self.wet.push(0.5 * (l + r));
        }
        self.stretch.set_transpose_factor_semitones(self.pitch, None);
        self.fed.clear();
        self.fed.extend_from_slice(&self.wet);
        self.stretch.process(&self.fed, &mut self.up);
        for ((x, w), up) in buf.iter_mut().zip(&self.wet).zip(&self.up) {
            *x += self.mix * (w * (1.0 - self.shimmer * 0.5) + up * self.shimmer);
        }
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

    fn pluck(freq: f32, at: f32, secs: f32, total: f32) -> Vec<f32> {
        (0..(SR * total) as usize)
            .map(|n| {
                let t = n as f32 / SR;
                if t < at || t > at + secs {
                    0.0
                } else {
                    0.5 * (-(t - at) * 6.0).exp() * (TAU * freq * (t - at)).sin()
                }
            })
            .collect()
    }

    fn rms_range(x: &[f32], from: f32, to: f32) -> f32 {
        let s = &x[(from * SR) as usize..(to * SR) as usize];
        (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
    }

    #[test]
    fn freeze_sustains_after_the_note_dies() {
        let input = pluck(196.0, 0.1, 0.5, 3.0);
        let dry = rms_range(&input, 1.5, 2.5);
        let out = run_chain(&[("freeze", "mix=1,sustain=20")], &input);
        assert!(dry < 1e-6);
        assert!(rms_range(&out, 1.5, 2.5) > 0.03, "loop should keep sounding");
        let hz = measure(&out[(1.0 * SR) as usize..(3.0 * SR) as usize]);
        assert!((hz / 196.0 - 1.0).abs() < 0.03, "loop pitch {hz}");
    }

    #[test]
    fn freeze_restarts_on_the_next_pick() {
        let mut input = pluck(196.0, 0.1, 0.5, 4.0);
        let second = pluck(294.0, 2.0, 0.5, 4.0);
        input.iter_mut().zip(&second).for_each(|(a, b)| *a += b);
        let out = run_chain(&[("freeze", "mix=1,sustain=30")], &input);
        let hz = measure(&out[(3.2 * SR) as usize..(4.0 * SR) as usize]);
        assert!((hz / 294.0 - 1.0).abs() < 0.03, "after the second pick the loop follows it: {hz}");
    }

    #[test]
    fn pick_noise_adds_a_burst_at_the_pick() {
        let input = pluck(196.0, 0.3, 0.05, 1.0);
        let with = run_chain(&[("pick-noise", "amount=1,decay=100")], &input);
        let without = run_chain(&[("pick-noise", "amount=0")], &input);
        let diff: Vec<f32> = with.iter().zip(&without).map(|(a, b)| a - b).collect();
        assert!(rms_range(&diff, 0.3, 0.4) > 0.005, "noise at the pick");
        assert!(rms_range(&diff, 0.0, 0.25) < 1e-6, "none before it");
    }

    #[test]
    fn bitcrusher_quantises() {
        let out = run_chain(&[("bitcrusher", "bits=2,rate=48000")], &sine(220.0, 0.2));
        let mut distinct: Vec<i32> = out.iter().map(|v| (v * 1000.0).round() as i32).collect();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() <= 5, "{} levels", distinct.len());
    }

    #[test]
    fn kalimba_plays_a_tine_at_the_note() {
        let input = pluck(196.0, 0.1, 0.4, 2.0);
        let out = run_chain(&[("kalimba", "mix=1,dry=0,octave=1")], &input);
        assert!(rms_range(&out, 0.3, 0.8) > 0.01, "a tine sounds");
        let hz = measure(&out[(0.3 * SR) as usize..(1.0 * SR) as usize]);
        assert!((hz / 392.0 - 1.0).abs() < 0.08, "an octave up, got {hz}");
    }

    #[test]
    fn swell_fades_each_pick_in() {
        let input: Vec<f32> = (0..(SR * 1.0) as usize)
            .map(|n| if n as f32 / SR >= 0.2 { 0.4 * (TAU * 196.0 * n as f32 / SR).sin() } else { 0.0 })
            .collect();
        let out = run_chain(&[("swell", "rise=500")], &input);
        assert!(rms_range(&out, 0.2, 0.25) < rms_range(&out, 0.8, 0.9) * 0.3);
    }

    #[test]
    fn wind_and_shimmer_make_sound_from_a_pluck() {
        let input = pluck(196.0, 0.1, 0.5, 2.0);
        for name in ["wind", "shimmer", "reverse", "ringmod"] {
            let out = run_chain(&[(name, "mix=1")], &input);
            assert!(out.iter().all(|v| v.is_finite() && v.abs() < 4.0), "{name}");
            let diff: f32 = out.iter().zip(&input).map(|(a, b)| (a - b).abs()).sum();
            assert!(diff > 1.0, "{name} changes the sound");
        }
    }

    #[test]
    fn transient_softens_the_pick_but_keeps_the_sustain() {
        let input: Vec<f32> = (0..(SR * 1.0) as usize)
            .map(|n| {
                let t = n as f32 / SR;
                if t < 0.2 { 0.0 } else { 0.5 * ((t - 0.2) / 0.001).min(1.0) * (-(t - 0.2) * 3.0).exp() * (TAU * 196.0 * (t - 0.2)).sin() }
            })
            .collect();
        let soft = run_chain(&[("transient", "attack=-1")], &input);
        let flat = run_chain(&[("transient", "attack=0")], &input);
        let peak = |x: &[f32]| x[(0.2 * SR) as usize..(0.215 * SR) as usize].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak(&soft) < peak(&flat) * 0.8, "{} vs {}", peak(&soft), peak(&flat));
        let (a, b) = (rms_range(&soft, 0.5, 0.7), rms_range(&flat, 0.5, 0.7));
        assert!((a / b - 1.0).abs() < 0.1, "sustain untouched: {a} vs {b}");
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
