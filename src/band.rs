//! The Band: a drummer and a bassist that follow what the Listener hears.
//!
//! It works in three ways:
//! - **Following** (`follow` on): the band's beat clock runs at the tempo the Listener measures and
//!   is nudged onto every beat it detects, and the bass plays the root of the chord it names.
//! - **Fixed rhythm** (`follow` off): the tempo and the beat grid are fixed; the bass still follows
//!   the chord you play.
//! - **Learned loop**: `Learn` plays a count-in bar and a quiet click, listens for N bars and
//!   writes down the chord of every beat and where your strums fall. Then the band loops that
//!   progression and ignores the Listener completely, so nothing drifts and nothing can be pulled
//!   off course, not even by the band's own sound reaching the input.
//!
//! The sounds are synthesised here, so nothing is sampled and nothing has a licence.

use crate::chain::Meters;
use std::f32::consts::TAU;

pub const STYLES: [&str; 9] = ["Rock", "Four on the floor", "Half-time", "Funk", "Reggae", "Ballad", "Bossa", "Country", "Punk"];

/// How long the Listener takes to name a chord after it is played (its window and smoothing).
const CHORD_LAG_S: f32 = 0.35;
/// Time from a pick to the Listener's note-onset flag.
const ONSET_LAG_S: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandSettings {
    pub on: bool,
    pub style: usize,
    pub drums: f32,
    pub bass: f32,
    /// Follow the tempo and beats the Listener hears; otherwise run at `bpm`.
    pub follow: bool,
    /// The fixed tempo, and the starting tempo while following.
    pub bpm: f32,
    /// In the learned loop, let the bass play where the guitar played.
    pub copy_rhythm: bool,
    /// Begin a new bar now (and forget the old tempo and chord).
    pub restart: bool,
}

impl Default for BandSettings {
    fn default() -> Self {
        Self { on: false, style: 0, drums: 0.8, bass: 0.7, follow: true, bpm: 100.0, copy_rhythm: true, restart: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BandCmd {
    /// Count in one bar, listen for this many bars, then loop what was played.
    Learn { bars: u32 },
    /// Throw the learned loop away and go back to following.
    Clear,
}

/// What the band learned.
#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub bpm: f32,
    pub bars: u32,
    /// (root, quality) for every beat of the loop.
    pub chords: Vec<(u8, u8)>,
    /// For each 16th of the bar, how many of the learned bars had a note there (0..1).
    pub rhythm: [f32; 16],
}

impl Song {
    /// The progression as text: one group per bar, e.g. `Am | F | C G`.
    pub fn text(&self) -> String {
        use crate::listen::Chord;
        let name = |c: (u8, u8)| Chord { root: c.0, quality: c.1, conf: 1.0 }.name();
        self.chords
            .chunks(4)
            .map(|bar| {
                let mut v: Vec<String> = Vec::new();
                for &c in bar {
                    let n = name(c);
                    if v.last() != Some(&n) {
                        v.push(n);
                    }
                }
                v.join(" ")
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Free,
    CountIn,
    Learning,
    Looping,
}

struct Rec {
    bars: usize,
    votes: Vec<[f32; 72]>,
    mask: [u32; 16],
    last_onsets: u32,
}

struct Pattern {
    kick: [f32; 16],
    snare: [f32; 16],
    hat: [f32; 16],
    open_hat: bool,
    /// 0 rest, 1 root, 2 fifth, 3 octave
    bass: [u8; 16],
}

const fn steps(on: &[usize], v: f32) -> [f32; 16] {
    let mut a = [0.0; 16];
    let mut i = 0;
    while i < on.len() {
        a[on[i]] = v;
        i += 1;
    }
    a
}

const fn merge(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut o = a;
    let mut i = 0;
    while i < 16 {
        if b[i] > o[i] {
            o[i] = b[i];
        }
        i += 1;
    }
    o
}

const EVENS: [usize; 8] = [0, 2, 4, 6, 8, 10, 12, 14];
const BEATS: [usize; 4] = [0, 4, 8, 12];
const OFFS: [usize; 4] = [2, 6, 10, 14];

const PATTERNS: [Pattern; 9] = [
    // rock
    Pattern {
        kick: merge(steps(&[0, 8], 1.0), steps(&[10], 0.7)),
        snare: steps(&[4, 12], 1.0),
        hat: merge(steps(&BEATS, 0.8), steps(&OFFS, 0.5)),
        open_hat: false,
        bass: [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 2, 0],
    },
    // four on the floor
    Pattern {
        kick: steps(&BEATS, 1.0),
        snare: steps(&[4, 12], 0.8),
        hat: steps(&OFFS, 0.7),
        open_hat: true,
        bass: [0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0],
    },
    // half-time
    Pattern {
        kick: merge(steps(&[0], 1.0), steps(&[10], 0.8)),
        snare: steps(&[8], 1.0),
        hat: merge(steps(&BEATS, 0.8), steps(&OFFS, 0.5)),
        open_hat: false,
        bass: [1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 3, 0],
    },
    // funk: sixteenth hats, ghost snares, syncopated kick and bass
    Pattern {
        kick: merge(steps(&[0, 6, 10], 1.0), steps(&[3, 14], 0.7)),
        snare: merge(steps(&[4, 12], 1.0), steps(&[2, 7, 9, 15], 0.25)),
        hat: merge(steps(&BEATS, 0.8), merge(steps(&OFFS, 0.55), steps(&[1, 3, 5, 7, 9, 11, 13, 15], 0.3))),
        open_hat: false,
        bass: [1, 0, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 0, 3],
    },
    // reggae one-drop: kick and snare together on beat 3, offbeat hats
    Pattern {
        kick: steps(&[8], 1.0),
        snare: steps(&[8], 0.8),
        hat: steps(&OFFS, 0.6),
        open_hat: false,
        bass: [1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0],
    },
    // ballad: soft, spacious
    Pattern {
        kick: merge(steps(&[0], 0.9), steps(&[10], 0.5)),
        snare: steps(&[4, 12], 0.55),
        hat: steps(&EVENS, 0.3),
        open_hat: false,
        bass: [1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
    },
    // bossa nova: clave-like rim, steady kick
    Pattern {
        kick: merge(steps(&[0, 8], 0.9), steps(&[6, 14], 0.6)),
        snare: steps(&[3, 6, 10, 13], 0.4),
        hat: steps(&EVENS, 0.35),
        open_hat: false,
        bass: [1, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 0, 0, 2, 0],
    },
    // country: boom-chick with a root-fifth bass
    Pattern {
        kick: steps(&[0, 8], 1.0),
        snare: merge(steps(&[4, 12], 0.75), steps(&OFFS, 0.2)),
        hat: steps(&BEATS, 0.4),
        open_hat: false,
        bass: [1, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0],
    },
    // punk: straight eighths, hard
    Pattern {
        kick: merge(steps(&[0, 8], 1.0), steps(&[6, 14], 0.8)),
        snare: steps(&[4, 12], 1.0),
        hat: steps(&EVENS, 0.9),
        open_hat: false,
        bass: [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0],
    },
];

pub struct Band {
    sr: f32,
    s: BandSettings,
    mode: Mode,
    failed: bool,
    song: Option<Song>,
    rec: Option<Rec>,
    /// Position in beats (negative during the count-in).
    pos: f64,
    last_step: i64,
    bpm: f32,
    last_heard_beats: u32,
    root: Option<u8>,
    noise: u32,
    // voices
    click_t: f32,
    click_hi: bool,
    kick_t: f32,
    kick_ph: f32,
    kick_v: f32,
    snare_t: f32,
    snare_v: f32,
    hat_t: f32,
    hat_v: f32,
    hat_decay: f32,
    prev_n: f32,
    bass_t: f32,
    bass_hz: f32,
    bass_ph: f32,
    bass_v: f32,
    bass_lp: f32,
}

impl Band {
    pub fn new(sr: f32) -> Self {
        Self {
            sr,
            s: BandSettings::default(),
            mode: Mode::Free,
            failed: false,
            song: None,
            rec: None,
            pos: 0.0,
            last_step: -1,
            bpm: 100.0,
            last_heard_beats: 0,
            root: None,
            noise: 0x1234_5678,
            click_t: 9.0,
            click_hi: false,
            kick_t: 9.0,
            kick_ph: 0.0,
            kick_v: 0.0,
            snare_t: 9.0,
            snare_v: 0.0,
            hat_t: 9.0,
            hat_v: 0.0,
            hat_decay: 90.0,
            prev_n: 0.0,
            bass_t: 9.0,
            bass_hz: 55.0,
            bass_ph: 0.0,
            bass_v: 0.0,
            bass_lp: 0.0,
        }
    }

    fn start_clock(&mut self, pos: f64) {
        self.pos = pos;
        self.last_step = (pos * 4.0).floor() as i64 - 1;
    }

    pub fn configure(&mut self, s: BandSettings) {
        let was_on = self.s.on;
        self.s = BandSettings { restart: false, ..s };
        if !s.on {
            if matches!(self.mode, Mode::CountIn | Mode::Learning) {
                self.mode = Mode::Free;
                self.rec = None;
            }
            return;
        }
        if !was_on || s.restart {
            match &self.song {
                Some(song) if self.mode != Mode::Learning && self.mode != Mode::CountIn => {
                    self.mode = Mode::Looping;
                    self.bpm = song.bpm;
                }
                _ => {
                    if self.mode == Mode::Free {
                        self.bpm = s.bpm.clamp(40.0, 220.0);
                    }
                }
            }
            if s.restart {
                self.root = None;
                self.failed = false;
            }
            if self.mode == Mode::Free || self.mode == Mode::Looping {
                self.start_clock(0.0);
            }
        }
        if self.mode == Mode::Free && !s.follow {
            self.bpm = s.bpm.clamp(40.0, 220.0);
        }
    }

    pub fn command(&mut self, c: BandCmd, meters: &Meters) {
        match c {
            BandCmd::Learn { bars } => {
                let bars = bars.clamp(1, 8) as usize;
                self.bpm = self.s.bpm.clamp(40.0, 220.0);
                self.rec = Some(Rec { bars, votes: vec![[0.0; 72]; bars * 4], mask: [0; 16], last_onsets: meters.heard().onsets });
                self.mode = Mode::CountIn;
                self.failed = false;
                self.root = None;
                self.song = None;
                meters.set_song(None);
                self.start_clock(-4.0);
            }
            BandCmd::Clear => {
                self.song = None;
                self.rec = None;
                self.mode = Mode::Free;
                self.failed = false;
                self.root = None;
                meters.set_song(None);
                self.start_clock(0.0);
                self.bpm = self.s.bpm.clamp(40.0, 220.0);
            }
        }
    }

    /// The tempo the band is playing at.
    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    fn rand(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    fn trigger(&mut self, step_abs: i64) {
        let step = step_abs.rem_euclid(16) as usize;
        if matches!(self.mode, Mode::CountIn | Mode::Learning) {
            if step % 4 == 0 {
                self.click_t = 0.0;
                self.click_hi = step == 0;
            }
            return;
        }
        let p = &PATTERNS[self.s.style.min(PATTERNS.len() - 1)];
        if p.kick[step] > 0.0 {
            self.kick_t = 0.0;
            self.kick_ph = 0.0;
            self.kick_v = p.kick[step];
        }
        if p.snare[step] > 0.0 {
            self.snare_t = 0.0;
            self.snare_v = p.snare[step];
        }
        if p.hat[step] > 0.0 {
            self.hat_t = 0.0;
            self.hat_v = p.hat[step];
            self.hat_decay = if p.open_hat { 22.0 } else { 90.0 };
        }
        let mut note = p.bass[step];
        if self.mode == Mode::Looping && self.s.copy_rhythm {
            if let Some(song) = &self.song {
                if song.rhythm.iter().filter(|&&v| v >= 0.5).count() >= 2 {
                    note = if song.rhythm[step] >= 0.5 { 1 } else { 0 };
                }
            }
        }
        if note > 0 {
            if let Some(r) = self.root {
                let e1 = 41.203f32;
                let root_hz = e1 * 2f32.powf(((r as usize + 12 - 4) % 12) as f32 / 12.0);
                self.bass_hz = match note {
                    2 => root_hz * 1.4983,
                    3 => root_hz * 2.0,
                    _ => root_hz,
                };
                self.bass_t = 0.0;
                self.bass_v = if step % 4 == 0 { 1.0 } else { 0.8 };
            }
        }
    }

    /// Learning: note the chord heard and the strums, for the block that just began.
    fn record(&mut self, heard: &crate::chain::Heard, frames: usize) {
        let Some(rec) = &mut self.rec else { return };
        let beats_per_s = self.bpm as f64 / 60.0;
        if let Some((r, q)) = heard.chord {
            // what is heard now was played a moment ago
            let t = self.pos - CHORD_LAG_S as f64 * beats_per_s;
            if t >= 0.0 && (t as usize) < rec.votes.len() {
                rec.votes[t as usize][q as usize * 12 + r as usize] += frames as f32;
            }
        }
        let new = if heard.onsets >= rec.last_onsets { heard.onsets - rec.last_onsets } else { heard.onsets };
        rec.last_onsets = heard.onsets;
        if new > 0 && self.pos >= 0.0 {
            let at = ((self.pos - ONSET_LAG_S as f64 * beats_per_s) * 4.0).round();
            if at >= 0.0 && (at as usize) < rec.bars * 16 {
                let at = at as usize;
                rec.mask[at % 16] |= 1 << (at / 16);
            }
        }
    }

    fn finish_learning(&mut self, meters: &Meters) {
        let Some(rec) = self.rec.take() else { return };
        let mut chords: Vec<Option<(u8, u8)>> = rec
            .votes
            .iter()
            .map(|v| {
                let (i, &m) = v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap();
                (m > 0.0).then_some(((i % 12) as u8, (i / 12) as u8))
            })
            .collect();
        let Some(first) = chords.iter().flatten().next().copied() else {
            self.mode = Mode::Free;
            self.failed = true;
            self.start_clock(0.0);
            return;
        };
        let mut prev = first;
        for c in &mut chords {
            match c {
                Some(x) => prev = *x,
                None => *c = Some(prev),
            }
        }
        let mut chords: Vec<(u8, u8)> = chords.into_iter().flatten().collect();
        // a single beat that differs from both neighbours is a detection slip
        for i in 1..chords.len().saturating_sub(1) {
            if chords[i - 1] == chords[i + 1] && chords[i] != chords[i - 1] {
                chords[i] = chords[i - 1];
            }
        }
        let mut rhythm = [0.0f32; 16];
        for (r, m) in rhythm.iter_mut().zip(rec.mask) {
            *r = m.count_ones() as f32 / rec.bars as f32;
        }
        let song = Song { bpm: self.bpm, bars: rec.bars as u32, chords, rhythm };
        meters.set_song(Some(song.clone()));
        self.song = Some(song);
        self.mode = Mode::Looping;
        let loop_beats = rec.bars as f64 * 4.0;
        self.pos -= loop_beats;
        self.last_step -= (loop_beats * 4.0) as i64;
    }

    /// Adds the band to `buf`.
    pub fn process(&mut self, buf: &mut [f32], meters: &Meters) {
        if !self.s.on {
            meters.set_band_state(if self.failed { 4 } else { 0 }, 0);
            return;
        }
        let heard = meters.heard();
        if self.mode == Mode::Free {
            if self.s.follow && heard.tempo > 0.0 && heard.tempo_conf >= 0.15 {
                let mut t = heard.tempo;
                // stay in a sensible range, in whole octaves
                while t > 200.0 {
                    t /= 2.0;
                }
                while t < 55.0 {
                    t *= 2.0;
                }
                self.bpm += 0.03 * (t - self.bpm);
                // a beat in the guitar: pull the band's beat towards it
                if heard.beats != self.last_heard_beats {
                    self.last_heard_beats = heard.beats;
                    let f = self.pos.rem_euclid(1.0);
                    let off = if f > 0.5 { f - 1.0 } else { f };
                    self.pos -= 0.2 * off;
                }
            }
            if let Some(k) = heard.key {
                self.root.get_or_insert(k);
            }
        } else if self.mode == Mode::Learning {
            self.record(&heard, buf.len());
        } else if self.mode == Mode::CountIn {
            if let Some(rec) = &mut self.rec {
                rec.last_onsets = heard.onsets;
            }
        }
        let dt = 1.0 / self.sr;
        let advance = (self.bpm / 60.0 * dt) as f64;
        let (dv, bv) = (self.s.drums * 0.8, self.s.bass * 0.55);
        for x in buf.iter_mut() {
            self.pos += advance;
            match self.mode {
                Mode::CountIn if self.pos >= 0.0 => self.mode = Mode::Learning,
                Mode::Learning if self.rec.as_ref().is_some_and(|r| self.pos >= r.bars as f64 * 4.0) => {
                    self.finish_learning(meters);
                }
                Mode::Looping => {
                    if let Some(song) = &self.song {
                        let loop_beats = song.bars as f64 * 4.0;
                        if self.pos >= loop_beats {
                            self.pos -= loop_beats;
                            self.last_step -= (loop_beats * 4.0) as i64;
                        }
                    }
                }
                _ => {}
            }
            let step = (self.pos * 4.0).floor() as i64;
            if step != self.last_step {
                self.last_step = step;
                match self.mode {
                    Mode::Free => {
                        if let Some((r, _)) = heard.chord {
                            self.root = Some(r);
                        }
                    }
                    Mode::Looping => {
                        if let Some(song) = &self.song {
                            if let Some(c) = song.chords.get((step.max(0) / 4) as usize) {
                                self.root = Some(c.0);
                            }
                        }
                    }
                    _ => {}
                }
                self.trigger(step);
            }
            let n = self.rand();
            let hp = n - self.prev_n;
            self.prev_n = n;
            let mut d = 0.0;
            let mut click = 0.0;
            if self.click_t < 0.05 {
                let f = if self.click_hi { 1600.0 } else { 1000.0 };
                click = (TAU * f * self.click_t).sin() * (-self.click_t * 90.0).exp() * 0.25;
                self.click_t += dt;
            }
            if self.kick_t < 0.5 {
                let f = 45.0 + 95.0 * (-self.kick_t * 38.0).exp();
                self.kick_ph += TAU * f * dt;
                d += self.kick_ph.sin() * (-self.kick_t * 11.0).exp() * self.kick_v;
                self.kick_t += dt;
            }
            if self.snare_t < 0.4 {
                let body = (TAU * 185.0 * self.snare_t).sin() * (-self.snare_t * 32.0).exp() * 0.5;
                d += (hp * 0.45 * (-self.snare_t * 22.0).exp() + body) * self.snare_v;
                self.snare_t += dt;
            }
            if self.hat_t < 0.4 {
                d += hp * 0.22 * (-self.hat_t * self.hat_decay).exp() * self.hat_v;
                self.hat_t += dt;
            }
            let mut b = 0.0;
            if self.bass_t < 1.2 {
                self.bass_ph += self.bass_hz * dt;
                self.bass_ph -= self.bass_ph.floor();
                let saw = 2.0 * self.bass_ph - 1.0;
                let sine = (TAU * self.bass_ph).sin();
                let a = 0.12 + 0.88 * (-self.bass_t * 4.0).exp();
                self.bass_lp += 0.08 * (saw - self.bass_lp);
                let attack = (self.bass_t * 400.0).min(1.0);
                b = (0.7 * sine + 0.6 * self.bass_lp) * a * attack * self.bass_v;
                self.bass_t += dt;
            }
            *x += (d * dv + b * bv * 1.4 + click).clamp(-1.2, 1.2).tanh();
        }
        let (phase, beat) = match self.mode {
            Mode::Free => (if self.failed { 4 } else { 0 }, 0),
            Mode::CountIn => (1, self.pos.floor() as i32),
            Mode::Learning => (2, self.pos.floor() as i32),
            Mode::Looping => (3, self.pos.floor() as i32),
        };
        meters.set_band_state(phase, beat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listen::{Analysis, Chord, Key};

    const SR: f32 = 48000.0;

    fn settings(style: usize) -> BandSettings {
        BandSettings { on: true, style, follow: false, bpm: 120.0, ..Default::default() }
    }

    fn render(b: &mut Band, m: &Meters, secs: f32) -> Vec<f32> {
        let mut out = vec![0.0; (secs * SR) as usize];
        for c in out.chunks_mut(64) {
            b.process(c, m);
        }
        out
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum::<f32>()
    }

    #[test]
    fn off_adds_nothing() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        assert!(render(&mut b, &m, 1.0).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn the_kick_lands_on_every_beat_of_the_floor_pattern() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(BandSettings { bass: 0.0, ..settings(1) });
        let x = render(&mut b, &m, 4.0);
        let beat = (SR * 0.5) as usize;
        let w = (SR * 0.04) as usize;
        for k in 1..7 {
            let at = k * beat;
            let after = energy(&x[at..at + w]);
            let before = energy(&x[at - w..at]);
            assert!(after > 2.0 * before, "beat {k}: {after} vs {before}");
        }
    }

    /// Dominant frequency of the bass in a stretch of audio.
    fn bass_hz(x: &[f32]) -> f32 {
        let ac = |lag: usize| (0..x.len() - lag).map(|i| x[i] * x[i + lag]).sum::<f32>();
        let best = (SR as usize / 200..SR as usize / 30).max_by(|&a, &b| ac(a).total_cmp(&ac(b))).unwrap();
        SR / best as f32
    }

    #[test]
    fn the_bass_plays_the_heard_chord_root() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        m.set_listening(true);
        m.publish_analysis(&Analysis { chord: Some(Chord { root: 9, quality: 1, conf: 1.0 }), ..Default::default() });
        b.configure(BandSettings { drums: 0.0, ..settings(1) });
        let x = render(&mut b, &m, 2.0);
        // first bass note of the floor pattern is on step 2 (0.25 s); look 60-200 ms into it
        let hz = bass_hz(&x[(SR * 0.31) as usize..(SR * 0.45) as usize]);
        assert!((hz - 55.0).abs() < 10.0, "A1 expected, heard about {hz} Hz");
    }

    #[test]
    fn it_follows_the_heard_tempo_and_beats() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        m.set_listening(true);
        m.publish_analysis(&Analysis {
            tempo: 90.0,
            tempo_conf: 0.6,
            key: Some(Key { root: 0, mode: 0, conf: 1.0, tonic_conf: 1.0 }),
            ..Default::default()
        });
        b.configure(BandSettings { follow: true, bpm: 130.0, ..settings(0) });
        render(&mut b, &m, 6.0);
        assert!((b.bpm() - 90.0).abs() < 2.0, "{}", b.bpm());
        // not following: the set tempo wins
        b.configure(BandSettings { follow: false, bpm: 130.0, ..settings(0) });
        render(&mut b, &m, 0.1);
        assert!((b.bpm() - 130.0).abs() < 0.1);
    }

    #[test]
    fn a_fixed_rhythm_ignores_the_heard_tempo_but_the_bass_still_follows_the_chord() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        m.set_listening(true);
        m.publish_analysis(&Analysis { tempo: 170.0, tempo_conf: 0.9, chord: Some(Chord { root: 9, quality: 1, conf: 1.0 }), ..Default::default() });
        b.configure(BandSettings { drums: 0.0, ..settings(1) });
        let x = render(&mut b, &m, 2.0);
        assert!((b.bpm() - 120.0).abs() < 0.01);
        assert!((bass_hz(&x[(SR * 0.31) as usize..(SR * 0.45) as usize]) - 55.0).abs() < 10.0);
        // the chord changes to F: the next bass note moves with it
        m.publish_analysis(&Analysis { tempo: 170.0, tempo_conf: 0.9, chord: Some(Chord { root: 5, quality: 0, conf: 1.0 }), ..Default::default() });
        let y = render(&mut b, &m, 2.0);
        assert!((b.bpm() - 120.0).abs() < 0.01);
        let hz = bass_hz(&y[(SR * 0.31) as usize..(SR * 0.45) as usize]);
        assert!((hz - 43.65).abs() < 8.0, "F1 expected, heard {hz}");
    }

    #[test]
    fn every_style_makes_sound_and_stays_bounded() {
        for style in 0..STYLES.len() {
            let mut b = Band::new(SR);
            let m = Meters::default();
            m.set_listening(true);
            m.publish_analysis(&Analysis { chord: Some(Chord { root: 4, quality: 0, conf: 1.0 }), ..Default::default() });
            b.configure(settings(style));
            let x = render(&mut b, &m, 4.0);
            let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
            assert!(peak > 0.1 && peak <= 1.01, "style {style} peak {peak}");
        }
        assert_eq!(STYLES.len(), PATTERNS.len());
    }

    /// Plays the guitar part for the learning test: Am F C G, a bar each, strummed on beats 1 and 3,
    /// reported to the band the way the Listener would (late, and one onset per strum).
    fn play_along(b: &mut Band, m: &Meters, from_s: f32, to_s: f32, count: &mut u32) {
        let prog = [(9u8, 1u8), (5, 0), (0, 0), (7, 0)];
        let bpm = 120.0f32;
        let mut n = (from_s * SR) as usize;
        let end = (to_s * SR) as usize;
        let mut last_strum = -1i64;
        while n < end {
            let t = n as f32 / SR;
            let pos = t * bpm / 60.0 - 4.0; // beats since the count-in ended
            let strum = ((pos - ONSET_LAG_S * bpm / 60.0) * 0.5).floor() as i64; // a strum every 2 beats
            if pos > 0.0 && strum != last_strum {
                last_strum = strum;
                *count += 1;
            }
            let heard_pos = pos - CHORD_LAG_S * bpm / 60.0;
            let chord = (heard_pos >= 0.0).then(|| {
                let c = prog[(heard_pos as usize / 4) % 4];
                Chord { root: c.0, quality: c.1, conf: 1.0 }
            });
            m.publish_analysis(&Analysis { chord, onsets: *count, tempo: 200.0, tempo_conf: 1.0, ..Default::default() });
            let mut blk = [0.0f32; 64];
            b.process(&mut blk, m);
            n += 64;
        }
    }

    #[test]
    fn it_learns_a_progression_and_then_stays_on_it() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        m.set_listening(true);
        b.configure(BandSettings { drums: 0.0, ..settings(0) });
        b.command(BandCmd::Learn { bars: 4 }, &m);
        let mut strums = 0;
        // 2 s count-in, then 4 bars of 2 s
        play_along(&mut b, &m, 0.0, 10.1, &mut strums);
        let song = m.song().expect("a learned song");
        assert_eq!(song.bars, 4);
        assert_eq!(song.text(), "Am | F | C | G", "{:?}", song.chords);
        assert!(song.rhythm[0] > 0.9 && song.rhythm[8] > 0.9, "{:?}", song.rhythm);
        assert!(song.rhythm.iter().enumerate().all(|(i, &v)| i == 0 || i == 8 || v < 0.2), "{:?}", song.rhythm);
        assert_eq!(m.band_state().0, 3, "looping now");

        // now the guitar plays something unrelated and claims another tempo: nothing changes
        m.publish_analysis(&Analysis { chord: Some(Chord { root: 2, quality: 0, conf: 1.0 }), tempo: 60.0, tempo_conf: 1.0, beats: 99, ..Default::default() });
        // the loop is where it was at 10.1 s: bar 1 began at 10.0 s, so bar 2 (F) starts 2 s later
        let rest = render(&mut b, &m, 6.0);
        assert!((b.bpm() - 120.0).abs() < 0.01);
        // bar 2 begins 1.9 s from now; bass on step 0 of F (the strum rhythm: steps 0 and 8)
        let at = (SR * 1.9) as usize;
        let hz = bass_hz(&rest[at + (SR * 0.06) as usize..at + (SR * 0.2) as usize]);
        assert!((hz - 43.65).abs() < 8.0, "F1 expected in bar 2, heard {hz}");
        // and no new bass note where the guitar did not play (step 4 of bar 2 = 0.5 s later); the
        // old note is still ringing out, so compare with its start rather than with silence
        let quiet = &rest[at + (SR * 0.42) as usize..at + (SR * 0.49) as usize];
        let loud = &rest[at + (SR * 0.06) as usize..at + (SR * 0.13) as usize];
        assert!(energy(quiet) < 0.3 * energy(loud), "{} vs {}", energy(quiet), energy(loud));
    }

    #[test]
    fn learning_nothing_fails_cleanly_and_clear_returns_to_following() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(settings(0));
        b.command(BandCmd::Learn { bars: 2 }, &m);
        render(&mut b, &m, 7.0);
        assert!(m.song().is_none());
        assert_eq!(m.band_state().0, 4);
        b.command(BandCmd::Clear, &m);
        render(&mut b, &m, 0.1);
        assert_eq!(m.band_state().0, 0);
    }
}
