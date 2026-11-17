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
        "tone-shift" => ("semitones", &["semitones", "cents", "mix"]),
        "vocoder" => ("mix", &["bands", "carrier", "release", "mix"]),
        "autotune" => ("amount", &["key", "scale", "speed", "amount"]),
        _ => return None,
    })
}

pub fn parse_spec(name: &str, raw: &str) -> Result<Spec, String> {
    let (primary, allowed) = schema(name).ok_or_else(|| format!("unknown effect '{name}'"))?;
    let name: &'static str = EFFECT_NAMES.iter().find(|n| **n == name).unwrap();
    Ok(Spec { name, params: Params::parse(name, raw, primary, allowed)? })
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
            sr,
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
        )?),
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

struct ToneShift {
    stretch: Stretch,
    mix: f32,
    scratch: Vec<f32>,
}

impl ToneShift {
    fn new(semitones: f32, mix: f32, sr: f32) -> Self {
        let mut stretch = Stretch::preset_default(1, sr as u32);
        stretch.set_transpose_factor_semitones(semitones, None);
        Self { stretch, mix, scratch: Vec::new() }
    }
}

impl Effect for ToneShift {
    fn process(&mut self, buf: &mut [f32]) {
        self.scratch.clear();
        self.scratch.extend_from_slice(buf);
        let mut out = vec![0.0; buf.len()];
        self.stretch.process(&self.scratch, &mut out);
        for ((x, wet), dry) in buf.iter_mut().zip(out).zip(&self.scratch) {
            *x = dry * (1.0 - self.mix) + wet * self.mix;
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

const WINDOW: usize = 1024;
const HOP: usize = 256;

struct Autotune {
    stretch: Stretch,
    allowed: [bool; 12], // pitch classes (absolute), already rotated by key
    speed_ms: f32,
    amount: f32,
    sr: f32,
    hist: Vec<f32>,
    since: usize,
    target: f32,
    current: f32,
    scratch: Vec<f32>,
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
    fn new(key: &str, scale: &str, speed_ms: f32, amount: f32, sr: f32) -> Result<Self, String> {
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
            stretch: Stretch::preset_default(1, sr as u32),
            allowed,
            speed_ms,
            amount,
            sr,
            hist: vec![0.0; WINDOW],
            since: 0,
            target: 0.0,
            current: 0.0,
            scratch: Vec::new(),
        })
    }

    /// YIN fundamental-frequency estimate of the history window; None if unvoiced.
    fn detect(&self) -> Option<f32> {
        let half = WINDOW / 2;
        let rms = (self.hist.iter().map(|x| x * x).sum::<f32>() / WINDOW as f32).sqrt();
        if rms < 0.01 {
            return None;
        }
        let min_tau = (self.sr / 900.0) as usize;
        let mut d = vec![0.0f32; half];
        for (tau, slot) in d.iter_mut().enumerate().skip(1) {
            let mut sum = 0.0;
            for j in 0..half {
                let diff = self.hist[j] - self.hist[j + tau];
                sum += diff * diff;
            }
            *slot = sum;
        }
        // cumulative mean normalised difference
        let mut running = 0.0;
        for tau in 1..half {
            running += d[tau];
            d[tau] = if running > 0.0 { d[tau] * tau as f32 / running } else { 1.0 };
        }
        let mut tau = min_tau.max(2);
        while tau < half - 1 {
            if d[tau] < 0.15 {
                while tau + 1 < half - 1 && d[tau + 1] < d[tau] {
                    tau += 1;
                }
                // parabolic interpolation
                let (a, b, c) = (d[tau - 1], d[tau], d[tau + 1]);
                let denom = a - 2.0 * b + c;
                let shift = if denom.abs() > 1e-9 { 0.5 * (a - c) / denom } else { 0.0 };
                return Some(self.sr / (tau as f32 + shift));
            }
            tau += 1;
        }
        None
    }

    fn update_target(&mut self) {
        self.target = match self.detect() {
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
        if n >= WINDOW {
            self.hist.copy_from_slice(&buf[n - WINDOW..]);
        } else {
            self.hist.copy_within(n.., 0);
            self.hist[WINDOW - n..].copy_from_slice(buf);
        }
        self.since += n;
        if self.since >= HOP {
            self.since = 0;
            self.update_target();
        }
        let alpha = if self.speed_ms <= 0.0 {
            1.0
        } else {
            1.0 - (-(n as f32) / (self.speed_ms * 0.001 * self.sr)).exp()
        };
        self.current += (self.target - self.current) * alpha;
        self.stretch.set_transpose_factor_semitones(self.current, None);

        self.scratch.clear();
        self.scratch.extend_from_slice(buf);
        self.stretch.process(&self.scratch, buf);
    }
}

// ------------------------------------------------------------------- chain

pub struct Chain(pub Vec<Box<dyn Effect>>);

impl Chain {
    pub fn new(specs: &[Spec], sr: f32) -> Result<Self, String> {
        specs.iter().map(|s| build(s, sr)).collect::<Result<Vec<_>, _>>().map(Chain)
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for fx in &mut self.0 {
            fx.process(buf);
        }
    }
}
