//! The Listener: what the guitar is playing, in musical terms.
//!
//! It watches the dry input and keeps a running estimate of the tempo and the beat phase, the
//! key or mode, and the current chord. It never touches the audio: the audio thread only copies
//! samples into a ring buffer (`ListenTap`), and a worker thread does the analysis and publishes
//! an `Analysis` snapshot for the UI (and, later, for a backing band).
//!
//! - Tempo: an onset-strength curve (rises in short-time energy, 94 per second) is
//!   autocorrelated over the last 8 s; a prior around 110 BPM picks between a tempo and its
//!   double or half. The beat phase is the comb alignment with the strongest onsets.
//! - Chroma: a 4096-point FFT of the signal decimated to about 12 kHz, spectral peaks folded
//!   into the 12 pitch classes. A fast average feeds the chord, a slow one (6 s half-life) the key.
//! - Key/mode: the slow chroma is correlated with a profile of each of the 7 modes on each of
//!   the 12 roots. Relative modes share their notes, so the tonic emphasis decides between them;
//!   the result carries two confidences, one for the note collection and one for the tonic.
//! - Chord: cosine similarity with six chord shapes on each root, with hysteresis.

use crate::chain::Meters;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/// Modes in the order of the diatonic scale degrees (Ionian = 0 ... Locrian = 6).
pub const MODE_NAMES: [&str; 7] = ["major", "Dorian", "Phrygian", "Lydian", "Mixolydian", "minor", "Locrian"];
const MODES: [[usize; 7]; 7] = [
    [0, 2, 4, 5, 7, 9, 11],
    [0, 2, 3, 5, 7, 9, 10],
    [0, 1, 3, 5, 7, 8, 10],
    [0, 2, 4, 6, 7, 9, 11],
    [0, 2, 4, 5, 7, 9, 10],
    [0, 2, 3, 5, 7, 8, 10],
    [0, 1, 3, 5, 6, 8, 10],
];
/// Semitones from the root of the parent major scale up to the tonic of each mode.
const MODE_OFFSET: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];

const CHORDS: [(&str, &[usize]); 6] = [
    ("", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("dim", &[0, 3, 6]),
    ("7", &[0, 4, 7, 10]),
    ("m7", &[0, 3, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    /// Pitch class of the tonic (0 = C).
    pub root: u8,
    /// Index into `MODE_NAMES`.
    pub mode: u8,
    /// How clearly this note collection beats every other one (0..1).
    pub conf: f32,
    /// How clearly this tonic beats the other modes of the same collection (0..1).
    pub tonic_conf: f32,
}

impl Key {
    pub fn name(&self) -> String {
        format!("{} {}", NOTE_NAMES[self.root as usize % 12], MODE_NAMES[self.mode as usize % 7])
    }

    /// Root of the major scale that has the same notes (a D Dorian is "C major").
    pub fn scale_root(&self) -> u8 {
        ((self.root as usize + 12 - MODE_OFFSET[self.mode as usize % 7]) % 12) as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chord {
    pub root: u8,
    /// Index into the chord shapes (major, minor, dim, 7, m7, maj7).
    pub quality: u8,
    pub conf: f32,
}

impl Chord {
    pub fn name(&self) -> String {
        format!("{}{}", NOTE_NAMES[self.root as usize % 12], CHORDS[self.quality as usize % CHORDS.len()].0)
    }
}

/// One snapshot of what the Listener hears.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// Loudness of the input (RMS of the last window).
    pub level: f32,
    /// Beats per minute, 0 until there is enough playing to tell.
    pub tempo: f32,
    /// 0..1; below about 0.15 the tempo is a guess.
    pub tempo_conf: f32,
    /// Position inside the current beat, 0..1 (0 = on the beat).
    pub beat_phase: f32,
    /// Counts up on every beat.
    pub beats: u32,
    /// Counts up on every note or strum the guitar starts.
    pub onsets: u32,
    pub key: Option<Key>,
    pub chord: Option<Chord>,
    /// Fast pitch-class energy (C..B), the strongest scaled to 1.
    pub chroma: [f32; 12],
}

// ---- FFT ----------------------------------------------------------------------------------

struct Fft {
    n: usize,
    cos: Vec<f32>,
    sin: Vec<f32>,
    rev: Vec<u32>,
}

impl Fft {
    fn new(n: usize) -> Self {
        assert!(n.is_power_of_two());
        let bits = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        let w = |k: usize| std::f32::consts::TAU * k as f32 / n as f32;
        Self { n, cos: (0..n / 2).map(|k| w(k).cos()).collect(), sin: (0..n / 2).map(|k| w(k).sin()).collect(), rev }
    }

    fn run(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = (self.cos[k * step], -self.sin[k * step]);
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }
}

// ---- helpers ------------------------------------------------------------------------------

const FFT_N: usize = 4096;
const FFT_HOP: usize = 1024;
const ENV_HOP: usize = 128;
const HISTORY_SECS: f32 = 8.0;
const MIN_BPM: f32 = 50.0;
const MAX_BPM: f32 = 200.0;
const PRIOR_BPM: f32 = 110.0;

fn pearson(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let ma = a.iter().sum::<f32>() / 12.0;
    let mb = b.iter().sum::<f32>() / 12.0;
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let (x, y) = (a[i] - ma, b[i] - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa < 1e-9 || bb < 1e-9 { 0.0 } else { ab / (aa * bb).sqrt() }
}

/// Weight of each pitch class for a key of the given mode with its tonic on 0.
fn profile(mode: usize) -> [f32; 12] {
    let mut p = [1.0f32; 12];
    for &iv in &MODES[mode] {
        p[iv] = 3.0;
    }
    p[0] = 6.0;
    if MODES[mode].contains(&7) {
        p[7] = 4.5;
    }
    for third in [3, 4] {
        if MODES[mode].contains(&third) {
            p[third] = 4.0;
        }
    }
    p
}

fn estimate_key(long: &[f32; 12]) -> Option<Key> {
    if long.iter().sum::<f32>() < 4.0 {
        return None; // not enough playing yet
    }
    let profiles: Vec<[f32; 12]> = (0..7).map(profile).collect();
    // score of (root, mode)
    let mut scores = [[0.0f32; 7]; 12];
    for (root, row) in scores.iter_mut().enumerate() {
        let mut x = [0.0f32; 12];
        for (i, v) in x.iter_mut().enumerate() {
            *v = long[(root + i) % 12];
        }
        for (mode, s) in row.iter_mut().enumerate() {
            *s = pearson(&x, &profiles[mode]);
        }
    }
    let scale_of = |root: usize, mode: usize| (root + 12 - MODE_OFFSET[mode]) % 12;
    let (mut best, mut br, mut bm) = (f32::MIN, 0, 0);
    for (root, row) in scores.iter().enumerate() {
        for (mode, &s) in row.iter().enumerate() {
            if s > best {
                (best, br, bm) = (s, root, mode);
            }
        }
    }
    if best < 0.35 {
        return None;
    }
    let parent = scale_of(br, bm);
    let (mut other_scale, mut other_mode) = (f32::MIN, f32::MIN);
    for (root, row) in scores.iter().enumerate() {
        for (mode, &s) in row.iter().enumerate() {
            if root == br && mode == bm {
                continue;
            }
            if scale_of(root, mode) == parent {
                other_mode = other_mode.max(s);
            } else {
                other_scale = other_scale.max(s);
            }
        }
    }
    Some(Key {
        root: br as u8,
        mode: bm as u8,
        conf: ((best - other_scale) / 0.08).clamp(0.0, 1.0),
        tonic_conf: ((best - other_mode) / 0.08).clamp(0.0, 1.0),
    })
}

fn estimate_chord(c: &[f32; 12]) -> Option<Chord> {
    let norm = c.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 0.2 {
        return None;
    }
    let mut best = (0.0f32, 0usize, 0usize);
    for root in 0..12 {
        for (q, (_, shape)) in CHORDS.iter().enumerate() {
            let dot: f32 = shape.iter().map(|&iv| c[(root + iv) % 12]).sum();
            let mut s = dot / (norm * (shape.len() as f32).sqrt());
            if shape.len() > 3 {
                s *= if shape[3] == 11 { 0.95 } else { 0.97 }; // prefer the plain triad; the major seventh most of all, since the 3rd harmonic of the major third lands on it
            }
            if s > best.0 {
                best = (s, root, q);
            }
        }
    }
    (best.0 >= 0.75).then_some(Chord { root: best.1 as u8, quality: best.2 as u8, conf: best.0 })
}

// ---- the analyser -------------------------------------------------------------------------

pub struct Listener {
    decim: usize,
    sr_d: f32,
    env_rate: f32,
    acc: f32,
    acc_n: usize,
    ring: Vec<f32>,
    wpos: usize,
    since_fft: usize,
    fft: Fft,
    hann: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    mag: Vec<f32>,
    e_acc: f32,
    e_n: usize,
    prev_lg: f32,
    onset: VecDeque<f32>,
    peak_flux: f32,
    cool: u32,
    frames: u64,
    since_tempo: usize,
    period: f32,
    jump: Option<(f32, u32)>,
    beat_ref: Option<f64>,
    last_beat_idx: i64,
    short: [f32; 12],
    long: [f32; 12],
    chord_vote: Option<(u8, u8, u32)>,
    quiet_frames: u32,
    out: Analysis,
}

impl Listener {
    pub fn new(sr: f32) -> Self {
        let decim = ((sr / 12000.0).round() as usize).max(1);
        let sr_d = sr / decim as f32;
        Self {
            decim,
            sr_d,
            env_rate: sr_d / ENV_HOP as f32,
            acc: 0.0,
            acc_n: 0,
            ring: vec![0.0; FFT_N],
            wpos: 0,
            since_fft: 0,
            fft: Fft::new(FFT_N),
            hann: (0..FFT_N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FFT_N as f32).cos()).collect(),
            re: vec![0.0; FFT_N],
            im: vec![0.0; FFT_N],
            mag: vec![0.0; FFT_N / 2],
            e_acc: 0.0,
            e_n: 0,
            prev_lg: 0.0,
            onset: VecDeque::new(),
            peak_flux: 0.0,
            cool: 0,
            frames: 0,
            since_tempo: 0,
            period: 0.0,
            jump: None,
            beat_ref: None,
            last_beat_idx: 0,
            short: [0.0; 12],
            long: [0.0; 12],
            chord_vote: None,
            quiet_frames: 0,
            out: Analysis::default(),
        }
    }

    /// Forget everything heard so far.
    pub fn reset(&mut self) {
        *self = Self::new(self.sr_d * self.decim as f32);
    }

    pub fn analysis(&self) -> &Analysis {
        &self.out
    }

    pub fn feed(&mut self, input: &[f32]) {
        for &x in input {
            self.acc += x;
            self.acc_n += 1;
            if self.acc_n == self.decim {
                let s = self.acc / self.decim as f32;
                self.acc = 0.0;
                self.acc_n = 0;
                self.sample(s);
            }
        }
    }

    fn sample(&mut self, s: f32) {
        self.ring[self.wpos] = s;
        self.wpos = (self.wpos + 1) % FFT_N;
        self.since_fft += 1;
        if self.since_fft >= FFT_HOP {
            self.since_fft = 0;
            self.spectrum();
        }
        self.e_acc += s * s;
        self.e_n += 1;
        if self.e_n == ENV_HOP {
            let e = self.e_acc / ENV_HOP as f32;
            self.e_acc = 0.0;
            self.e_n = 0;
            let lg = e.sqrt() * 20.0; // amplitude, not log: loud beats must outweigh soft off-beats
            let flux = (lg - self.prev_lg).max(0.0);
            self.prev_lg = lg;
            self.env_frame(flux);
        }
    }

    // ---- chroma, key, chord ----

    fn spectrum(&mut self) {
        for i in 0..FFT_N {
            self.re[i] = self.ring[(self.wpos + i) % FFT_N] * self.hann[i];
            self.im[i] = 0.0;
        }
        let rms = (self.ring.iter().map(|x| x * x).sum::<f32>() / FFT_N as f32).sqrt();
        self.out.level = rms;
        if rms < 0.004 {
            self.quiet_frames += 1;
            for v in &mut self.short {
                *v *= 0.8;
            }
            if self.quiet_frames > 8 {
                self.chord_vote = None;
                self.out.chord = None;
            }
            self.out.chroma = normalised(&self.short);
            return;
        }
        self.quiet_frames = 0;
        self.fft.run(&mut self.re, &mut self.im);
        let bin_hz = self.sr_d / FFT_N as f32;
        let mut top = 0.0f32;
        for k in 0..FFT_N / 2 {
            self.mag[k] = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt();
            if (65.0..1800.0).contains(&(k as f32 * bin_hz)) {
                top = top.max(self.mag[k]);
            }
        }
        let mut c = [0.0f32; 12];
        for k in 1..FFT_N / 2 - 1 {
            let f = k as f32 * bin_hz;
            let (a, b, d) = (self.mag[k - 1], self.mag[k], self.mag[k + 1]);
            if !(65.0..1800.0).contains(&f) || b < 0.03 * top || b <= a || b < d {
                continue;
            }
            let denom = a - 2.0 * b + d;
            let delta = if denom.abs() > 1e-9 { (0.5 * (a - d) / denom).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = (k as f32 + delta) * bin_hz;
            let midi = 69.0 + 12.0 * (hz / 440.0).log2();
            let pc = (midi.round() as i32).rem_euclid(12) as usize;
            let r = hz / 350.0;
            c[pc] += b / (1.0 + r * r); // upper partials mostly repeat other notes, so they count less
        }
        for v in &mut c {
            *v = v.sqrt();
        }
        let m = c.iter().cloned().fold(0.0, f32::max);
        if m <= 0.0 {
            return;
        }
        for v in &mut c {
            *v /= m;
        }
        let weight = (rms / 0.02).min(1.0);
        let keep = 0.5f32.powf(FFT_HOP as f32 / self.sr_d / 6.0);
        for i in 0..12 {
            self.short[i] = 0.65 * self.short[i] + 0.35 * c[i];
            self.long[i] = self.long[i] * keep + c[i] * weight;
        }
        self.out.chroma = normalised(&self.short);
        self.out.key = estimate_key(&self.long);
        // a chord must win two analysis frames in a row before it is shown
        match estimate_chord(&self.short) {
            Some(ch) => match self.chord_vote {
                Some((r, q, n)) if r == ch.root && q == ch.quality => {
                    self.chord_vote = Some((r, q, n + 1));
                    if n + 1 >= 2 || self.out.chord.is_none() && n + 1 >= 1 {
                        self.out.chord = Some(ch);
                    }
                }
                _ => self.chord_vote = Some((ch.root, ch.quality, 1)),
            },
            None => {
                self.chord_vote = None;
                if self.out.chord.is_some() && self.short.iter().sum::<f32>() < 0.5 {
                    self.out.chord = None;
                }
            }
        }
    }

    // ---- tempo and beat ----

    fn env_frame(&mut self, flux: f32) {
        self.frames += 1;
        // a note or strum starts where the level jumps well above what has been playing lately
        self.peak_flux = (self.peak_flux * 0.997).max(flux);
        if self.cool > 0 {
            self.cool -= 1;
        } else if flux > 0.35 * self.peak_flux && flux > 0.04 {
            self.out.onsets = self.out.onsets.wrapping_add(1);
            self.cool = 6;
        }
        let cap = (HISTORY_SECS * self.env_rate) as usize;
        self.onset.push_back(flux);
        if self.onset.len() > cap {
            self.onset.pop_front();
        }
        self.since_tempo += 1;
        if self.since_tempo as f32 >= self.env_rate / 2.0 {
            self.since_tempo = 0;
            self.tempo();
        }
        if self.period > 0.0 {
            if let Some(r) = self.beat_ref {
                let pos = (self.frames as f64 - r) / self.period as f64;
                let idx = pos.floor() as i64;
                if idx == self.last_beat_idx + 1 && self.out.tempo_conf >= 0.15 {
                    self.out.beats = self.out.beats.wrapping_add(1);
                }
                self.last_beat_idx = idx;
                self.out.beat_phase = (pos - pos.floor()) as f32;
            }
        }
    }

    fn tempo(&mut self) {
        let n = self.onset.len();
        if (n as f32) < 4.0 * self.env_rate {
            return;
        }
        let mean = self.onset.iter().sum::<f32>() / n as f32;
        let o: Vec<f32> = self.onset.iter().map(|v| (v - mean).max(0.0)).collect();
        let power = o.iter().map(|v| v * v).sum::<f32>();
        if power < 1e-6 {
            self.out.tempo = 0.0;
            self.out.tempo_conf = 0.0;
            self.period = 0.0;
            return;
        }
        let fr = self.env_rate;
        let lmin = (60.0 * fr / MAX_BPM).floor() as usize;
        let lmax = (60.0 * fr / MIN_BPM).ceil() as usize;
        let ac = |lag: usize| -> f32 {
            if lag >= n {
                return 0.0;
            }
            let mut s = 0.0;
            for i in lag..n {
                s += o[i] * o[i - lag];
            }
            s / (n - lag) as f32
        };
        let a0 = ac(0).max(1e-9);
        let prior = |lag: f32| -> f32 {
            let bpm = 60.0 * fr / lag;
            let z = (bpm / PRIOR_BPM).log2() / 0.9;
            (-0.5 * z * z).exp()
        };
        let mut score = vec![0.0f32; lmax + 2];
        let mut raw = vec![0.0f32; lmax + 2];
        for lag in lmin.saturating_sub(1).max(1)..=lmax + 1 {
            raw[lag] = ac(lag) + 0.5 * ac(2 * lag);
            score[lag] = raw[lag] * prior(lag as f32);
        }
        let mut best = lmin.max(1);
        for lag in lmin.max(1)..=lmax {
            if score[lag] > score[best] {
                best = lag;
            }
        }
        let (a, b, c) = (score[best - 1], score[best], score[best + 1]);
        let denom = a - 2.0 * b + c;
        let delta = if denom.abs() > 1e-12 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
        let period = best as f32 + delta;
        let conf = (2.0 * ac(best) / a0).clamp(0.0, 1.0);

        if self.period == 0.0 {
            self.period = period;
        } else if (period / self.period).ln().abs() < 0.05 {
            self.period = 0.6 * self.period + 0.4 * period;
            self.jump = None;
        } else {
            // a different tempo must be seen three times before it replaces the current one
            match self.jump {
                Some((p, k)) if (period / p).ln().abs() < 0.05 => {
                    if k + 1 >= 3 {
                        self.period = period;
                        self.beat_ref = None;
                        self.jump = None;
                    } else {
                        self.jump = Some((period, k + 1));
                    }
                }
                _ => self.jump = Some((period, 1)),
            }
        }
        self.out.tempo = 60.0 * fr / self.period;
        self.out.tempo_conf = conf;

        // beat phase: the alignment of a comb with this period that catches the most onset strength
        let t = self.period;
        let mut best_p = (f32::MIN, 0usize);
        for p in 0..t.ceil() as usize {
            let (mut s, mut cnt) = (0.0, 0);
            let mut pos = (n - 1) as f32 - p as f32;
            while pos >= 1.0 {
                let i = pos.round() as usize;
                s += o[i - 1].max(o[i]).max(o[(i + 1).min(n - 1)]);
                cnt += 1;
                pos -= t;
            }
            if cnt > 0 && s / cnt as f32 > best_p.0 {
                best_p = (s / cnt as f32, p);
            }
        }
        let measured = self.frames as f64 - 1.0 - best_p.1 as f64;
        self.beat_ref = Some(match self.beat_ref {
            None => measured,
            Some(old) => {
                let d = measured - old;
                let err = d - (d / t as f64).round() * t as f64;
                old + 0.35 * err
            }
        });
        if let Some(r) = self.beat_ref {
            self.last_beat_idx = ((self.frames as f64 - r) / self.period as f64).floor() as i64;
        }
    }
}

fn normalised(c: &[f32; 12]) -> [f32; 12] {
    let m = c.iter().cloned().fold(0.0, f32::max);
    let mut o = *c;
    if m > 1e-6 {
        o.iter_mut().for_each(|v| *v /= m);
    }
    o
}

// ---- running beside the audio thread ------------------------------------------------------

/// The audio thread's end: copies samples into a ring and never blocks.
pub struct ListenTap {
    prod: HeapProd<f32>,
    stop: Arc<AtomicBool>,
}

impl ListenTap {
    pub fn push(&mut self, buf: &[f32]) {
        self.prod.push_slice(buf);
    }
}

impl Drop for ListenTap {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

/// Start the analysis thread. It idles while `meters.listening()` is off and starts afresh
/// every time it is switched on; it ends when the tap is dropped.
pub fn spawn(sr: f32, meters: Arc<Meters>) -> ListenTap {
    let (prod, mut cons): (HeapProd<f32>, HeapCons<f32>) = HeapRb::<f32>::new(sr as usize).split();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let _ = std::thread::Builder::new().name("listener".into()).spawn(move || {
        let mut l = Listener::new(sr);
        let mut was_on = false;
        let mut resets = meters.listener_resets();
        let mut buf = vec![0.0f32; 4096];
        while !flag.load(Relaxed) {
            let on = meters.listening();
            if !on {
                cons.clear();
                was_on = false;
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            if !was_on {
                l.reset();
                was_on = true;
            }
            if meters.listener_resets() != resets {
                resets = meters.listener_resets();
                l.reset();
                cons.clear();
                meters.publish_analysis(l.analysis()); // overwrites anything published mid-reset
            }
            let n = cons.pop_slice(&mut buf);
            if n == 0 {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            l.feed(&buf[..n]);
            meters.publish_analysis(l.analysis());
        }
    });
    ListenTap { prod, stop }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    const SR: f32 = 48000.0;

    /// A plucked-string-like note: fast attack, decaying harmonics.
    fn note(buf: &mut [f32], start: usize, hz: f32, amp: f32, decay: f32) {
        for (i, v) in buf.iter_mut().skip(start).enumerate() {
            let t = i as f32 / SR;
            let env = (t * 600.0).min(1.0) * (-t * decay).exp();
            if env < 1e-4 && t > 0.1 {
                break;
            }
            let mut s = 0.0;
            for h in 1..=5 {
                s += (TAU * hz * h as f32 * t).sin() / h as f32;
            }
            *v += amp * env * s;
        }
    }

    fn midi_hz(m: f32) -> f32 {
        440.0 * 2f32.powf((m - 69.0) / 12.0)
    }

    fn run(x: &[f32], chunk: usize) -> Analysis {
        let mut l = Listener::new(SR);
        for c in x.chunks(chunk) {
            l.feed(c);
        }
        l.analysis().clone()
    }

    fn rhythm(bpm: f32, secs: f32, offbeats: bool) -> Vec<f32> {
        let mut x = vec![0.0; (secs * SR) as usize];
        let beat = 60.0 / bpm;
        let mut k = 0;
        loop {
            let t = k as f32 * beat;
            if t >= secs - 0.5 {
                break;
            }
            note(&mut x, (t * SR) as usize, if k % 4 == 0 { 110.0 } else { 165.0 }, if k % 4 == 0 { 0.5 } else { 0.35 }, 9.0);
            if offbeats {
                note(&mut x, ((t + beat / 2.0) * SR) as usize, 220.0, 0.15, 9.0);
            }
            k += 1;
        }
        x
    }

    #[test]
    fn finds_the_tempo() {
        for (bpm, off) in [(100.0, false), (80.0, true), (128.0, false), (90.0, true)] {
            let a = run(&rhythm(bpm, 24.0, off), 64);
            assert!((a.tempo - bpm).abs() / bpm < 0.04, "{bpm} bpm (offbeats {off}) heard as {}", a.tempo);
            assert!(a.tempo_conf > 0.15, "confidence {}", a.tempo_conf);
        }
    }

    #[test]
    fn counts_the_notes_you_start() {
        let a = run(&rhythm(100.0, 12.0, false), 64);
        // 100 bpm for 11.5 s of beats is about 19 notes
        assert!((17..=22).contains(&a.onsets), "{} onsets", a.onsets);
    }

    #[test]
    fn beats_count_at_the_tempo_and_stay_on_the_beat() {
        let x = rhythm(100.0, 30.0, false);
        let mut l = Listener::new(SR);
        let (mut at10, mut at30) = (0, 0);
        for (i, c) in x.chunks(480).enumerate() {
            l.feed(c);
            if i == 1000 {
                at10 = l.analysis().beats;
            }
        }
        at30 += l.analysis().beats;
        // 20 s at 100 bpm is 33 beats
        let n = at30 - at10;
        assert!((31..=35).contains(&n), "{n} beats in 20 s");
    }

    fn melody(scale: &[usize], root: usize, tonic_every: usize) -> Vec<f32> {
        let mut x = vec![0.0; (20.0 * SR) as usize];
        for k in 0..60 {
            // every degree comes round equally often; the tonic is added every `tonic_every` notes
            let step = if k % tonic_every == 0 { 0 } else { scale[(k / 2 * 3) % scale.len()] };
            let m = 48.0 + ((root + step) % 12) as f32 + if k % 3 == 0 { 12.0 } else { 0.0 };
            note(&mut x, (k as f32 * 0.32 * SR) as usize, midi_hz(m), 0.3, 3.0);
        }
        x
    }

    #[test]
    fn finds_the_key_and_mode() {
        for (mode, root) in [(0usize, 0usize), (5, 9), (1, 2), (4, 7), (0, 6), (5, 4)] {
            let a = run(&melody(&MODES[mode], root, 2), 480);
            let k = a.key.expect("a key");
            assert_eq!((k.root as usize, k.mode as usize), (root, mode), "got {} want {} {}", k.name(), NOTE_NAMES[root], MODE_NAMES[mode]);
        }
    }

    fn chord(notes: &[f32]) -> Vec<f32> {
        let mut x = vec![0.0; (3.0 * SR) as usize];
        for &m in notes {
            note(&mut x, 0, midi_hz(m), 0.2, 0.4);
        }
        x
    }

    #[test]
    fn names_chords() {
        let cases: [(&[f32], &str); 5] = [
            (&[48.0, 55.0, 60.0, 64.0], "C"),
            (&[45.0, 52.0, 57.0, 60.0, 64.0], "Am"),
            (&[43.0, 50.0, 55.0, 59.0, 65.0], "G7"),
            (&[50.0, 57.0, 62.0, 66.0], "D"),
            (&[41.0, 48.0, 53.0, 57.0, 64.0], "Fmaj7"),
        ];
        for (notes, want) in cases {
            let a = run(&chord(notes), 480);
            assert_eq!(a.chord.map(|c| c.name()).as_deref(), Some(want), "chroma {:?}", a.chroma);
        }
    }

    #[test]
    fn reset_makes_it_forget() {
        let meters: Arc<Meters> = Arc::default();
        meters.set_listening(true);
        let mut tap = spawn(SR, meters.clone());
        for c in chord(&[48.0, 55.0, 60.0, 64.0]).chunks(480) {
            tap.push(c);
        }
        for _ in 0..100 {
            if meters.analysis().chord.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(meters.analysis().chord.is_some());
        meters.reset_listener();
        std::thread::sleep(Duration::from_millis(300));
        let a = meters.analysis();
        assert!(a.chord.is_none() && a.key.is_none());
    }

    #[test]
    fn silence_says_nothing_and_chunking_does_not_matter() {
        let a = run(&vec![0.0; 96000], 64);
        assert!(a.key.is_none() && a.chord.is_none() && a.tempo == 0.0);
        let x = rhythm(100.0, 12.0, false);
        let (p, q) = (run(&x, 64), run(&x, 1000));
        assert_eq!(p.tempo.to_bits(), q.tempo.to_bits());
        assert_eq!(p.chord, q.chord);
    }

    #[test]
    fn the_worker_thread_publishes_what_it_hears() {
        let meters: Arc<Meters> = Arc::default();
        meters.set_listening(true);
        let mut tap = spawn(SR, meters.clone());
        let x = chord(&[48.0, 55.0, 60.0, 64.0]);
        for c in x.chunks(480) {
            tap.push(c);
            std::thread::sleep(Duration::from_micros(300));
        }
        let mut heard = None;
        for _ in 0..100 {
            heard = meters.analysis().chord.map(|c| c.name());
            if heard.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(heard.as_deref(), Some("C"));
    }
}
