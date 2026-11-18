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
        "gate" => Box::new(Gate::new(spec, sr)),
        "eq" => Box::new(Eq::new(spec, sr)),
        "amp" => Box::new(Amp::new(spec, sr)),
        "cabinet" => Box::new(Cabinet::new(spec, sr)),
        "bitcrusher" => Box::new(Bitcrusher::new(spec)),
        "ringmod" => Box::new(RingMod::new(spec, sr)),
        "reverse" => Box::new(Reverse::new(spec, sr)),
        "wind" => Box::new(Wind::new(spec, sr)),
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
pub(crate) struct PitchTracker {
    hist: Vec<f32>,
    since: usize,
    sr: f32,
}

impl PitchTracker {
    pub(crate) fn new(sr: f32) -> Self {
        Self { hist: vec![0.0; WINDOW], since: 0, sr }
    }

    /// Feed a block; returns true when it is time to run `detect` again.
    pub(crate) fn push(&mut self, buf: &[f32]) -> bool {
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
    pub(crate) fn detect(&self) -> Option<f32> {
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

// ------------------------------------------------------------------ biquads

/// A second-order filter section (RBJ cookbook), direct form II transposed.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

#[derive(Clone, Copy)]
enum Shape {
    LowPass,
    HighPass,
    LowShelf,
    HighShelf,
    Peak,
}

impl Biquad {
    const OFF: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 };

    fn new(shape: Shape, f: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let mut b = Self::OFF;
        b.set(shape, f, q, gain_db, sr);
        b
    }

    /// Changes the response, keeping the filter state (so live edits don't click).
    fn set(&mut self, shape: Shape, f: f32, q: f32, gain_db: f32, sr: f32) {
        let f = f.clamp(10.0, sr * 0.45);
        let w = TAU * f / sr;
        let (sn, cs) = w.sin_cos();
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = sn / (2.0 * q.max(0.05));
        let (b0, b1, b2, a0, a1, a2) = match shape {
            Shape::LowPass => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            Shape::HighPass => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0, 1.0 + alpha, -2.0 * cs, 1.0 - alpha),
            Shape::Peak => (
                1.0 + alpha * a,
                -2.0 * cs,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cs,
                1.0 - alpha / a,
            ),
            Shape::LowShelf => {
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cs + t),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cs),
                    a * ((a + 1.0) - (a - 1.0) * cs - t),
                    (a + 1.0) + (a - 1.0) * cs + t,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cs),
                    (a + 1.0) + (a - 1.0) * cs - t,
                )
            }
            Shape::HighShelf => {
                let t = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cs + t),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cs),
                    a * ((a + 1.0) + (a - 1.0) * cs - t),
                    (a + 1.0) - (a - 1.0) * cs + t,
                    2.0 * ((a - 1.0) - (a + 1.0) * cs),
                    (a + 1.0) - (a - 1.0) * cs - t,
                )
            }
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

// -------------------------------------------------------------------- gate

/// Noise gate: closes when the playing stops so hum and hiss disappear between notes.
struct Gate {
    spec: Spec,
    sr: f32,
    env: f32,
    open: bool,
    hold_left: usize,
    gain: f32,
    env_rel: f32,
    thr: f32,
    close_thr: f32,
    att: f32,
    rel: f32,
    hold: usize,
    floor: f32,
}

impl Gate {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            env: 0.0,
            open: false,
            hold_left: 0,
            gain: 0.0,
            env_rel: coef(15.0, sr),
            thr: 0.0,
            close_thr: 0.0,
            att: 0.0,
            rel: 0.0,
            hold: 0,
            floor: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.thr = 10f32.powf(self.spec.num("threshold") / 20.0);
        self.close_thr = self.thr * 0.7;
        // "attack" is the time to fully open, "release" the time to fall by about 60 dB
        self.att = coef(self.spec.num("attack") / 2.3, self.sr);
        self.rel = coef(self.spec.num("release") / 6.9, self.sr);
        self.hold = (self.spec.num("hold") * 0.001 * self.sr) as usize;
        self.floor = 10f32.powf(self.spec.num("depth") / 20.0);
    }
}

impl Effect for Gate {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let a = x.abs();
            self.env = if a > self.env { a } else { a + (self.env - a) * self.env_rel };
            if !self.open {
                if self.env > self.thr {
                    self.open = true;
                    self.hold_left = self.hold;
                }
            } else if self.env > self.close_thr {
                self.hold_left = self.hold;
            } else if self.hold_left > 0 {
                self.hold_left -= 1;
            } else {
                self.open = false;
            }
            let target = if self.open { 1.0 } else { self.floor };
            let k = if target > self.gain { self.att } else { self.rel };
            self.gain = target + (self.gain - target) * k;
            *x *= self.gain;
        }
    }
    live_set!();
}

// ---------------------------------------------------------------------- eq

/// Guitar EQ: low shelf, sweepable mid, high shelf, plus low-cut and high-cut filters.
struct Eq {
    spec: Spec,
    sr: f32,
    hp: Biquad,
    low: Biquad,
    mid: Biquad,
    high: Biquad,
    lp: Biquad,
    use_hp: bool,
    use_lp: bool,
}

impl Eq {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr,
            hp: Biquad::OFF,
            low: Biquad::OFF,
            mid: Biquad::OFF,
            high: Biquad::OFF,
            lp: Biquad::OFF,
            use_hp: false,
            use_lp: false,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        let sp = &self.spec;
        let lowcut = sp.num("lowcut");
        self.use_hp = lowcut > 21.0;
        self.hp.set(Shape::HighPass, lowcut, 0.707, 0.0, self.sr);
        let highcut = sp.num("highcut");
        self.use_lp = highcut < 19500.0;
        self.lp.set(Shape::LowPass, highcut, 0.707, 0.0, self.sr);
        self.low.set(Shape::LowShelf, 120.0, 0.8, sp.num("low"), self.sr);
        self.mid.set(Shape::Peak, sp.num("mid-freq"), sp.num("mid-q"), sp.num("mid"), self.sr);
        self.high.set(Shape::HighShelf, 4000.0, 0.8, sp.num("high"), self.sr);
    }
}

impl Effect for Eq {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            let mut v = *x;
            if self.use_hp {
                v = self.hp.tick(v);
            }
            v = self.high.tick(self.mid.tick(self.low.tick(v)));
            if self.use_lp {
                v = self.lp.tick(v);
            }
            *x = v;
        }
    }
    live_set!();
}

// --------------------------------------------------------------------- amp

const HB_TAPS: usize = 31;

/// 2x oversampling with a linear-phase half-band filter, so the clipping stages don't
/// alias back into the audible range.
struct Over2 {
    h: [f32; HB_TAPS],
    up_hist: [f32; HB_TAPS / 2 + 1],
    down_hist: [f32; HB_TAPS],
}

impl Over2 {
    fn new() -> Self {
        let mut h = [0.0f32; HB_TAPS];
        let c = (HB_TAPS / 2) as f32;
        for (k, v) in h.iter_mut().enumerate() {
            let t = k as f32 - c;
            let sinc = if t == 0.0 { 1.0 } else { (PI * 0.5 * t).sin() / (PI * 0.5 * t) };
            let w = 0.42 - 0.5 * (TAU * k as f32 / (HB_TAPS - 1) as f32).cos()
                + 0.08 * (2.0 * TAU * k as f32 / (HB_TAPS - 1) as f32).cos();
            *v = 0.5 * sinc * w;
        }
        let sum: f32 = h.iter().sum();
        h.iter_mut().for_each(|v| *v /= sum);
        Self { h, up_hist: [0.0; HB_TAPS / 2 + 1], down_hist: [0.0; HB_TAPS] }
    }

    /// One input sample becomes two at the doubled rate.
    fn up(&mut self, x: f32) -> [f32; 2] {
        self.up_hist.rotate_right(1);
        self.up_hist[0] = x;
        let mut even = 0.0;
        let mut odd = 0.0;
        for (m, xv) in self.up_hist.iter().enumerate() {
            even += self.h[2 * m] * xv;
            if 2 * m + 1 < HB_TAPS {
                odd += self.h[2 * m + 1] * xv;
            }
        }
        [2.0 * even, 2.0 * odd]
    }

    /// Two samples at the doubled rate become one.
    fn down(&mut self, pair: [f32; 2]) -> f32 {
        let mut out = 0.0;
        for z in pair {
            self.down_hist.rotate_right(1);
            self.down_hist[0] = z;
            out = self.h.iter().zip(&self.down_hist).map(|(a, b)| a * b).sum();
        }
        out
    }
}

struct Voicing {
    stages: usize,
    max_gain: f32,
    bias: f32,
    coupling_hz: f32,
    stage_lp_hz: f32,
    bass_hz: f32,
    mid_hz: f32,
    mid_q: f32,
    treble_hz: f32,
    mid_db: f32,
    power: f32,
    fizz_hz: f32,
    out: f32,
}

fn voicing(model: &str) -> Voicing {
    match model {
        "clean" => Voicing { stages: 1, max_gain: 7.0, bias: 0.05, coupling_hz: 30.0, stage_lp_hz: 9000.0, bass_hz: 100.0, mid_hz: 500.0, mid_q: 0.7, treble_hz: 3000.0, mid_db: 6.0, power: 0.6, fizz_hz: 10000.0, out: 0.9 },
        "crunch" => Voicing { stages: 2, max_gain: 60.0, bias: 0.12, coupling_hz: 60.0, stage_lp_hz: 7000.0, bass_hz: 110.0, mid_hz: 650.0, mid_q: 0.8, treble_hz: 2800.0, mid_db: 8.0, power: 1.0, fizz_hz: 8000.0, out: 0.7 },
        "brit" => Voicing { stages: 3, max_gain: 250.0, bias: 0.15, coupling_hz: 80.0, stage_lp_hz: 6500.0, bass_hz: 120.0, mid_hz: 700.0, mid_q: 0.9, treble_hz: 2600.0, mid_db: 9.0, power: 1.2, fizz_hz: 7000.0, out: 0.55 },
        "lead" => Voicing { stages: 3, max_gain: 700.0, bias: 0.2, coupling_hz: 100.0, stage_lp_hz: 5500.0, bass_hz: 120.0, mid_hz: 800.0, mid_q: 1.0, treble_hz: 2500.0, mid_db: 10.0, power: 1.2, fizz_hz: 6000.0, out: 0.5 },
        _ => Voicing { stages: 4, max_gain: 1500.0, bias: 0.25, coupling_hz: 140.0, stage_lp_hz: 5000.0, bass_hz: 100.0, mid_hz: 550.0, mid_q: 0.9, treble_hz: 2400.0, mid_db: -10.0, power: 1.1, fizz_hz: 5500.0, out: 0.5 },
    }
}

#[derive(Clone, Copy, Default)]
struct StageState {
    lp: f32,
    hp_x: f32,
    hp_y: f32,
}

/// Tube-style amp: input filter, cascaded asymmetric clipping stages with coupling
/// filters, a three-band tone stack, a driven power stage, presence and a fizz filter,
/// all at 2x oversampling. The `model` picks the voicing: how many stages, how much gain,
/// where the mid sits. It is a modelled approximation, not a capture of any real amp;
/// put a Cabinet after it.
struct Amp {
    spec: Spec,
    sr2: f32,
    over: Over2,
    v: Voicing,
    hp_in_x: f32,
    hp_in_y: f32,
    stages: [StageState; 4],
    k_stage: f32,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    presence: Biquad,
    fizz: Biquad,
    fizz2: Biquad,
    power_k: f32,
    level: f32,
    dc_x: f32,
    dc_y: f32,
    lp_k: f32,
    hp_k: f32,
    in_hp_k: f32,
}

impl Amp {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut s = Self {
            spec,
            sr2: sr * 2.0,
            over: Over2::new(),
            v: voicing("crunch"),
            hp_in_x: 0.0,
            hp_in_y: 0.0,
            stages: [StageState::default(); 4],
            k_stage: 1.0,
            bass: Biquad::OFF,
            mid: Biquad::OFF,
            treble: Biquad::OFF,
            presence: Biquad::OFF,
            fizz: Biquad::OFF,
            fizz2: Biquad::OFF,
            power_k: 1.0,
            level: 1.0,
            dc_x: 0.0,
            dc_y: 0.0,
            lp_k: 0.0,
            hp_k: 0.0,
            in_hp_k: 0.0,
        };
        s.configure();
        s
    }

    fn configure(&mut self) {
        let sp = &self.spec;
        let v = voicing(sp.option("model"));
        let sr2 = self.sr2;
        let gain = sp.num("gain");
        let total = 1.0 + gain * gain * v.max_gain;
        self.k_stage = total.powf(1.0 / v.stages as f32);
        self.lp_k = one_pole(v.stage_lp_hz, sr2);
        self.hp_k = (-TAU * v.coupling_hz / sr2).exp();
        self.in_hp_k = (-TAU * (v.coupling_hz * 0.8) / sr2).exp();
        let tone = |x: f32, range: f32| (x - 0.5) * 2.0 * range;
        self.bass.set(Shape::LowShelf, v.bass_hz, 0.7, tone(sp.num("bass"), 12.0), sr2);
        let mid_db = if v.mid_db >= 0.0 { tone(sp.num("mid"), v.mid_db) } else { v.mid_db + tone(sp.num("mid"), 8.0).max(-6.0) * 1.0 };
        self.mid.set(Shape::Peak, v.mid_hz, v.mid_q, mid_db, sr2);
        self.treble.set(Shape::HighShelf, v.treble_hz, 0.7, tone(sp.num("treble"), 12.0), sr2);
        self.presence.set(Shape::HighShelf, 4200.0, 0.7, sp.num("presence") * 9.0, sr2);
        self.fizz.set(Shape::LowPass, v.fizz_hz, 0.7, 0.0, sr2);
        self.fizz2.set(Shape::LowPass, v.fizz_hz * 1.2, 0.6, 0.0, sr2);
        self.power_k = (0.4 + sp.num("master") * 3.0) * v.power;
        self.level = sp.num("level") * v.out;
        self.v = v;
    }

    #[inline]
    fn core(&mut self, x: f32) -> f32 {
        let mut y = x;
        let n = self.v.stages;
        for i in 0..n {
            let st = &mut self.stages[i];
            let bias = self.v.bias * if i % 2 == 0 { 1.0 } else { -0.6 };
            let driven = (y * self.k_stage + bias).tanh() - bias.tanh();
            st.lp += self.lp_k * (driven - st.lp);
            // coupling capacitor: one-pole high-pass
            let hp = self.hp_k * (st.hp_y + st.lp - st.hp_x);
            st.hp_x = st.lp;
            st.hp_y = hp;
            y = hp;
        }
        y = self.treble.tick(self.mid.tick(self.bass.tick(y)));
        y = (y * self.power_k).tanh() / self.power_k.max(1.0).sqrt();
        y = self.presence.tick(y);
        self.fizz2.tick(self.fizz.tick(y))
    }
}

impl Effect for Amp {
    fn process(&mut self, buf: &mut [f32]) {
        for x in buf {
            // input high-pass keeps rumble out of the gain stages
            let hp = self.in_hp_k * (self.hp_in_y + *x - self.hp_in_x);
            self.hp_in_x = *x;
            self.hp_in_y = hp;
            let up = self.over.up(hp);
            let mut pair = [0.0; 2];
            for (o, u) in pair.iter_mut().zip(up) {
                *o = self.core(u);
            }
            let y = self.over.down(pair) * self.level;
            // final DC block
            let dc = y - self.dc_x + 0.995 * self.dc_y;
            self.dc_x = y;
            self.dc_y = dc;
            *x = dc.clamp(-1.5, 1.5);
        }
    }
    live_set!();
}

// ----------------------------------------------------------------- cabinet

/// Built-in cabinet impulse responses, synthesised from filters (no recordings): a
/// high-pass and a speaker-cone resonance at the bottom, a presence peak, a steep
/// roll-off at the top and a short baffle reflection. Use the `ir` parameter to load a
/// real impulse response instead.
fn builtin_ir(cab: &str, sr: f32) -> Vec<f32> {
    let (lo, res, res_db, pres, pres_db, hi, orders, refl_ms, refl) = match cab {
        "2x12" => (75.0, 100.0, 3.0, 2500.0, 3.0, 5000.0, 2, 0.9, 0.2),
        "4x12" => (70.0, 95.0, 5.0, 2200.0, 5.0, 4600.0, 3, 1.2, 0.25),
        "1x8-vintage" => (140.0, 180.0, 5.0, 3200.0, 6.0, 4200.0, 2, 0.5, 0.15),
        _ => (80.0, 110.0, 4.0, 2800.0, 4.0, 5200.0, 2, 0.7, 0.25),
    };
    let n = (0.1 * sr) as usize;
    let mut hp = Biquad::new(Shape::HighPass, lo, 0.8, 0.0, sr);
    let mut r = Biquad::new(Shape::Peak, res, 1.2, res_db, sr);
    let mut p = Biquad::new(Shape::Peak, pres, 1.0, pres_db, sr);
    let mut lps: Vec<Biquad> = (0..orders).map(|_| Biquad::new(Shape::LowPass, hi, 0.707, 0.0, sr)).collect();
    let mut h: Vec<f32> = (0..n)
        .map(|i| {
            let mut v = hp.tick(if i == 0 { 1.0 } else { 0.0 });
            v = p.tick(r.tick(v));
            for l in &mut lps {
                v = l.tick(v);
            }
            v
        })
        .collect();
    let d = (refl_ms * 0.001 * sr) as usize;
    for i in (d..n).rev() {
        h[i] -= refl * h[i - d];
    }
    h
}

/// Reads a WAV impulse response (mono mix, resampled to `sr`). The name is a path or a
/// file in the `ir` folder of the config directory.
pub fn load_ir(name: &str, sr: f32) -> Result<Vec<f32>, String> {
    let direct = std::path::PathBuf::from(name);
    let path = if direct.is_absolute() || direct.exists() {
        direct
    } else {
        let mut p = crate::paths::ir_dir().join(name);
        if p.extension().is_none() {
            p.set_extension("wav");
        }
        p
    };
    let mut reader = hound::WavReader::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    let ch = spec.channels as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 / scale)).collect::<Result<_, _>>().map_err(|e| e.to_string())?
        }
    };
    let mono: Vec<f32> = raw.chunks(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    if mono.is_empty() {
        return Err(format!("{}: empty file", path.display()));
    }
    if spec.sample_rate as f32 == sr {
        return Ok(mono);
    }
    let ratio = spec.sample_rate as f32 / sr;
    let n = (mono.len() as f32 / ratio) as usize;
    Ok((0..n)
        .map(|i| {
            let p = i as f32 * ratio;
            let k = p as usize;
            let f = p - k as f32;
            mono[k] * (1.0 - f) + mono[(k + 1).min(mono.len() - 1)] * f
        })
        .collect())
}

/// Convolves the signal with a cabinet impulse response (direct form, a few thousand
/// taps: cheap enough for a 64-frame live buffer).
struct Cabinet {
    spec: Spec,
    h_rev: Vec<f32>,
    ring: Vec<f32>,
    pos: usize,
    mix: f32,
    level: f32,
}

impl Cabinet {
    fn new(spec: Spec, sr: f32) -> Self {
        let mut h = match spec.text("ir") {
            "" => builtin_ir(spec.option("cab"), sr),
            name => load_ir(name, sr).unwrap_or_else(|e| {
                eprintln!("cabinet: {e}; using the built-in cabinet");
                builtin_ir(spec.option("cab"), sr)
            }),
        };
        // keep the first `trim` ms, fading the last fifth out, and match the energy of the input
        let keep = ((spec.num("trim") * 0.001 * sr) as usize).clamp(16, h.len().max(16));
        h.truncate(keep);
        let fade = keep / 5;
        for i in 0..fade {
            let g = 0.5 + 0.5 * (PI * i as f32 / fade as f32).cos();
            let at = keep - fade + i;
            if at < h.len() {
                h[at] *= g;
            }
        }
        // unity gain across the guitar's midrange (mean response at a few spot frequencies)
        let spots = [150.0f32, 300.0, 600.0, 1200.0, 2400.0];
        let gain = spots
            .iter()
            .map(|f| {
                let w = TAU * f / sr;
                let (re, im) = h.iter().enumerate().fold((0.0f32, 0.0f32), |(re, im), (n, v)| {
                    (re + v * (w * n as f32).cos(), im - v * (w * n as f32).sin())
                });
                (re * re + im * im).sqrt()
            })
            .sum::<f32>()
            / spots.len() as f32;
        h.iter_mut().for_each(|v| *v /= gain.max(1e-6));
        h.reverse();
        let len = h.len();
        let mut s = Self { spec, h_rev: h, ring: vec![0.0; len * 2], pos: 0, mix: 1.0, level: 1.0 };
        s.configure();
        s
    }

    fn configure(&mut self) {
        self.mix = self.spec.num("mix");
        self.level = 10f32.powf(self.spec.num("level") / 20.0);
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let tail: f32 = ca.remainder().iter().zip(cb.remainder()).map(|(x, y)| x * y).sum();
    for (x, y) in ca.zip(cb) {
        for k in 0..8 {
            acc[k] += x[k] * y[k];
        }
    }
    acc.iter().sum::<f32>() + tail
}

impl Effect for Cabinet {
    fn process(&mut self, buf: &mut [f32]) {
        let n = self.h_rev.len();
        for x in buf {
            self.ring[self.pos] = *x;
            self.ring[self.pos + n] = *x;
            let wet = dot(&self.ring[self.pos + 1..self.pos + 1 + n], &self.h_rev) * self.level;
            self.pos = (self.pos + 1) % n;
            *x += (wet - *x) * self.mix;
        }
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
                        Kind::Text { default } => Value::Text(default),
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
    fn bitcrusher_quantises() {
        let out = run_chain(&[("bitcrusher", "bits=2,rate=48000")], &sine(220.0, 0.2));
        let mut distinct: Vec<i32> = out.iter().map(|v| (v * 1000.0).round() as i32).collect();
        distinct.sort();
        distinct.dedup();
        assert!(distinct.len() <= 5, "{} levels", distinct.len());
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

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn gate_silences_noise_and_passes_notes() {
        let mut input: Vec<f32> = (0..(SR * 1.0) as usize).map(|n| 0.003 * (n as f32 * 12.9898).sin()).collect();
        for n in 0..(SR * 0.3) as usize {
            input[n + 4800] += 0.4 * (TAU * 196.0 * n as f32 / SR).sin();
        }
        let out = run_chain(&[("gate", "threshold=-45")], &input);
        assert!(rms_range(&out, 0.2, 0.35) > 0.2, "note passes");
        assert!(rms_range(&out, 0.7, 0.95) < rms_range(&input, 0.7, 0.95) * 0.05, "hiss is gated");
    }

    #[test]
    fn eq_boosts_and_cuts_where_asked() {
        let level = |spec: &str, hz: f32| rms(&run_chain(&[("eq", spec)], &sine(hz, 0.5))[(SR * 0.2) as usize..]);
        let flat = rms(&sine(1000.0, 0.5)[(SR * 0.2) as usize..]);
        assert!((level("", 1000.0) / flat - 1.0).abs() < 0.02);
        assert!(level("mid=12,mid-freq=1000", 1000.0) > flat * 3.0);
        assert!(level("mid=-12,mid-freq=1000", 1000.0) < flat * 0.3);
        assert!(level("low=12", 60.0) > rms(&sine(60.0, 0.5)[(SR * 0.2) as usize..]) * 2.5);
        assert!(level("high=-12", 8000.0) < rms(&sine(8000.0, 0.5)[(SR * 0.2) as usize..]) * 0.4);
        assert!(level("lowcut=300", 60.0) < rms(&sine(60.0, 0.5)[(SR * 0.2) as usize..]) * 0.1);
    }

    #[test]
    fn amps_are_bounded_louder_when_driven_and_keep_the_pitch() {
        for model in ["clean", "crunch", "brit", "lead", "modern"] {
            let quiet = run_chain(&[("amp", &format!("model={model},gain=0.1"))], &sine(196.0, 1.0));
            let hot = run_chain(&[("amp", &format!("model={model},gain=1"))], &sine(196.0, 1.0));
            let (q, h) = (rms(&quiet[24000..]), rms(&hot[24000..]));
            println!("{model}: gain0.1 rms {q:.3}  gain1 rms {h:.3} peak {:.2}", hot.iter().fold(0.0f32, |m, v| m.max(v.abs())));
            assert!(hot.iter().all(|v| v.is_finite() && v.abs() <= 1.5), "{model} bounded");
            assert!(h > 0.03, "{model} audible");
            let hz = measure(&hot);
            assert!((hz / 196.0 - 1.0).abs() < 0.03, "{model} pitch {hz}");
            assert!(h > q * 0.8, "{model}: more gain is not quieter");
        }
    }

    #[test]
    fn cabinet_filters_highs_and_falls_back_when_the_file_is_missing() {
        let low = rms(&run_chain(&[("cabinet", "")], &sine(200.0, 0.5))[(SR * 0.2) as usize..]);
        let high = rms(&run_chain(&[("cabinet", "")], &sine(9000.0, 0.5))[(SR * 0.2) as usize..]);
        assert!(high < low * 0.2, "cab rolls off the top: {low} vs {high}");
        let out = run_chain(&[("cabinet", "ir=no-such-file.wav")], &sine(200.0, 0.3));
        assert!(rms(&out[7200..]) > 0.05, "falls back to the built-in cab");
    }

    #[test]
    fn cabinet_loads_a_wav_impulse_response() {
        let dir = std::env::temp_dir().join("guitar-fx-test-ir");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pulse.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        w.write_sample(i16::MAX).unwrap(); // a unit impulse: the cab should leave the sound alone
        for _ in 0..100 {
            w.write_sample(0i16).unwrap();
        }
        w.finalize().unwrap();
        let input = sine(300.0, 0.3);
        let out = run_chain(&[("cabinet", &format!("ir={}", path.display()))], &input);
        let err: f32 = input.iter().zip(&out).skip(2000).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.02, "impulse IR is transparent, max error {err}");
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
