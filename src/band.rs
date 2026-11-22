//! The Band: a drummer and a bassist that follow what the Listener hears.
//!
//! The Band has its own beat clock. While `follow` is on, the clock runs at the tempo the
//! Listener measures and is nudged onto every beat it detects in the guitar, so the band stays
//! with the player instead of the player chasing a click. The bass plays the root of the chord
//! the Listener names (the key's tonic until there is one). The sounds are synthesised here, so
//! nothing is sampled and nothing has a licence.
//!
//! The Listener does not yet know which beat is "1": the bar starts when the band is switched on,
//! or when "Restart" is pressed on the guitarist's downbeat.

use crate::chain::Meters;
use std::f32::consts::TAU;

pub const STYLES: [&str; 3] = ["Rock", "Four on the floor", "Half-time"];

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
    /// Begin a new bar now.
    pub restart: bool,
}

impl Default for BandSettings {
    fn default() -> Self {
        Self { on: false, style: 0, drums: 0.8, bass: 0.7, follow: true, bpm: 100.0, restart: false }
    }
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

const PATTERNS: [Pattern; 3] = [
    // rock
    Pattern {
        kick: merge(steps(&[0, 8], 1.0), steps(&[10], 0.7)),
        snare: steps(&[4, 12], 1.0),
        hat: merge(steps(&[0, 4, 8, 12], 0.8), steps(&[2, 6, 10, 14], 0.5)),
        open_hat: false,
        bass: [1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 2, 0],
    },
    // four on the floor
    Pattern {
        kick: steps(&[0, 4, 8, 12], 1.0),
        snare: steps(&[4, 12], 0.8),
        hat: steps(&[2, 6, 10, 14], 0.7),
        open_hat: true,
        bass: [0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0],
    },
    // half-time
    Pattern {
        kick: merge(steps(&[0], 1.0), steps(&[10], 0.8)),
        snare: steps(&[8], 1.0),
        hat: merge(steps(&[0, 4, 8, 12], 0.8), steps(&[2, 6, 10, 14], 0.5)),
        open_hat: false,
        bass: [1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 3, 0],
    },
];

pub struct Band {
    sr: f32,
    s: BandSettings,
    /// Position in beats since the bar began.
    phase: f64,
    last_step: i64,
    bpm: f32,
    last_heard_beats: u32,
    root: Option<u8>,
    noise: u32,
    // voices
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
            phase: 0.0,
            last_step: -1,
            bpm: 100.0,
            last_heard_beats: 0,
            root: None,
            noise: 0x1234_5678,
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

    pub fn configure(&mut self, s: BandSettings) {
        if (s.on && !self.s.on) || s.restart {
            self.phase = 0.0;
            self.last_step = -1;
            if s.restart {
                self.root = None; // forget the old chord; the next one heard decides
            }
            if !self.s.on || !s.follow || s.restart {
                self.bpm = s.bpm.clamp(40.0, 220.0);
            }
        }
        if !s.follow {
            self.bpm = s.bpm.clamp(40.0, 220.0);
        }
        self.s = BandSettings { restart: false, ..s };
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

    fn trigger(&mut self, step: usize) {
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
        let note = p.bass[step];
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

    /// Adds the band to `buf`.
    pub fn process(&mut self, buf: &mut [f32], meters: &Meters) {
        if !self.s.on {
            return;
        }
        let heard = meters.heard();
        if self.s.follow {
            if heard.tempo > 0.0 && heard.tempo_conf >= 0.15 {
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
                    let f = self.phase.fract();
                    let off = if f > 0.5 { f - 1.0 } else { f };
                    self.phase -= 0.2 * off;
                }
            }
        }
        if let Some(k) = heard.key_root {
            self.root.get_or_insert(k);
        }
        let dt = 1.0 / self.sr;
        let advance = (self.bpm / 60.0 * dt) as f64;
        let (dv, bv) = (self.s.drums * 0.8, self.s.bass * 0.55);
        for x in buf {
            self.phase += advance;
            let step = (self.phase * 4.0).floor() as i64;
            if step != self.last_step {
                self.last_step = step;
                if let Some(r) = heard.chord_root {
                    self.root = Some(r);
                }
                self.trigger(step.rem_euclid(16) as usize);
            }
            let n = self.rand();
            let hp = n - self.prev_n;
            self.prev_n = n;
            let mut d = 0.0;
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
            *x += (d * dv + b * bv * 1.4).clamp(-1.2, 1.2).tanh();
        }
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

    #[test]
    fn the_bass_plays_the_heard_chord_root() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        m.set_listening(true);
        m.publish_analysis(&Analysis { chord: Some(Chord { root: 9, quality: 1, conf: 1.0 }), ..Default::default() });
        b.configure(BandSettings { drums: 0.0, ..settings(1) });
        let x = render(&mut b, &m, 2.0);
        // first bass note of the floor pattern is on step 2 (0.25 s); look 60-200 ms into it
        let (from, to) = ((SR * 0.31) as usize, (SR * 0.45) as usize);
        let seg = &x[from..to];
        let ac = |lag: usize| (0..seg.len() - lag).map(|i| seg[i] * seg[i + lag]).sum::<f32>();
        let best = (SR as usize / 200..SR as usize / 30).max_by(|&a, &b| ac(a).total_cmp(&ac(b))).unwrap();
        let hz = SR / best as f32;
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
    }
}
