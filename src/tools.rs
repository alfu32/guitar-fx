//! Practice tools that run beside the effect chain on the audio thread: a metronome and
//! a looper. (The tuner reads the same input and lives in `chain.rs`.)

use crate::chain::Meters;
use std::f32::consts::TAU;
use std::sync::atomic::Ordering::Relaxed;

// ---------------------------------------------------------------- metronome

pub struct Metronome {
    sr: f32,
    on: bool,
    period: f32,
    beats: u32,
    volume: f32,
    counter: f32,
    beat: u32,
    clicks: u32,
    ph: f32,
    step: f32,
    env: f32,
}

impl Metronome {
    pub fn new(sr: f32) -> Self {
        Self { sr, on: false, period: sr, beats: 4, volume: 0.5, counter: 0.0, beat: 0, clicks: 0, ph: 0.0, step: 0.0, env: 0.0 }
    }

    pub fn configure(&mut self, on: bool, bpm: f32, beats: u32, volume: f32) {
        if on && !self.on {
            self.counter = self.period; // click straight away
            self.beat = self.beats.saturating_sub(1);
        }
        self.on = on;
        self.period = self.sr * 60.0 / bpm.clamp(20.0, 400.0);
        self.beats = beats.max(1);
        self.volume = volume.clamp(0.0, 1.0);
    }

    /// Adds the clicks to `buf`.
    pub fn process(&mut self, buf: &mut [f32], meters: &Meters) {
        if !self.on {
            return;
        }
        let decay = (-1.0 / (0.012 * self.sr)).exp();
        for x in buf {
            self.counter += 1.0;
            if self.counter >= self.period {
                self.counter -= self.period;
                self.beat = (self.beat + 1) % self.beats;
                let accent = self.beat == 0;
                self.step = TAU * if accent { 1600.0 } else { 1000.0 } / self.sr;
                self.ph = 0.0;
                self.env = 1.0;
                self.clicks = self.clicks.wrapping_add(1);
                meters.beat.store(self.clicks << 1 | accent as u32, Relaxed);
            }
            if self.env > 1e-4 {
                self.ph += self.step;
                *x += self.ph.sin() * self.env * self.volume * 0.6;
                self.env *= decay;
            }
        }
    }
}

// ------------------------------------------------------------------- looper

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoopCmd {
    /// Start a fresh loop (throws away the old one).
    Record,
    /// Close the loop being recorded, or resume a stopped one.
    Play,
    Overdub,
    Stop,
    Clear,
    Level(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopState {
    Empty = 0,
    Recording = 1,
    Playing = 2,
    Overdubbing = 3,
    Stopped = 4,
}

impl LoopState {
    pub fn from_u32(n: u32) -> Self {
        match n {
            1 => Self::Recording,
            2 => Self::Playing,
            3 => Self::Overdubbing,
            4 => Self::Stopped,
            _ => Self::Empty,
        }
    }
}

/// Records what comes out of the effect chain and plays it back on repeat. The memory is
/// allocated up front, so nothing is allocated while playing.
pub struct Looper {
    sr: f32,
    buf: Vec<f32>,
    len: usize,
    pos: usize,
    state: LoopState,
    level: f32,
}

impl Looper {
    pub fn new(sr: f32, max_seconds: f32) -> Self {
        Self { sr, buf: vec![0.0; (sr * max_seconds) as usize], len: 0, pos: 0, state: LoopState::Empty, level: 1.0 }
    }

    pub fn state(&self) -> LoopState {
        self.state
    }

    pub fn command(&mut self, cmd: LoopCmd) {
        use LoopState::*;
        match cmd {
            LoopCmd::Record => {
                self.len = 0;
                self.pos = 0;
                self.state = Recording;
            }
            LoopCmd::Play => match self.state {
                Recording => self.finish(),
                Stopped | Overdubbing => {
                    if self.state == Stopped {
                        self.pos = 0;
                    }
                    self.state = Playing;
                }
                _ => {}
            },
            LoopCmd::Overdub => {
                if self.len > 0 && matches!(self.state, Playing | Stopped) {
                    self.state = Overdubbing;
                }
            }
            LoopCmd::Stop => {
                if matches!(self.state, Playing | Overdubbing) {
                    self.state = Stopped;
                } else if self.state == Recording {
                    self.finish();
                    self.state = Stopped;
                }
            }
            LoopCmd::Clear => {
                self.len = 0;
                self.pos = 0;
                self.state = Empty;
            }
            LoopCmd::Level(l) => self.level = l.clamp(0.0, 1.5),
        }
    }

    /// Ends recording: short fades at the edges keep the seam from clicking.
    fn finish(&mut self) {
        if self.len < (0.05 * self.sr) as usize {
            self.len = 0;
            self.state = LoopState::Empty;
            return;
        }
        let fade = ((0.003 * self.sr) as usize).min(self.len / 2);
        for i in 0..fade {
            let g = i as f32 / fade as f32;
            self.buf[i] *= g;
            self.buf[self.len - 1 - i] *= g;
        }
        self.pos = 0;
        self.state = LoopState::Playing;
    }

    pub fn process(&mut self, buf: &mut [f32], meters: &Meters) {
        use LoopState::*;
        for x in buf.iter_mut() {
            match self.state {
                Recording => {
                    self.buf[self.len] = *x;
                    self.len += 1;
                    if self.len >= self.buf.len() {
                        self.finish();
                    }
                }
                Playing => {
                    *x += self.buf[self.pos] * self.level;
                    self.pos = (self.pos + 1) % self.len;
                }
                Overdubbing => {
                    let old = self.buf[self.pos];
                    *x += old * self.level;
                    self.buf[self.pos] = (old + *x - old * self.level).clamp(-1.5, 1.5);
                    self.pos = (self.pos + 1) % self.len;
                }
                Empty | Stopped => {}
            }
        }
        meters.loop_state.store(self.state as u32, Relaxed);
        let (len, pos) = if self.state == Recording { (self.len, self.len) } else { (self.len, self.pos) };
        meters.loop_len.store(len as u32, Relaxed);
        meters.loop_pos.store(pos as u32, Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metronome_clicks_on_the_beat() {
        let m = Meters::default();
        let mut met = Metronome::new(48000.0);
        met.configure(true, 120.0, 4, 1.0);
        let mut out = vec![0.0f32; 48000]; // one second = two beats at 120 bpm
        for block in out.chunks_mut(64) {
            met.process(block, &m);
        }
        // each click starts with a burst; count separate bursts
        let mut bursts = 0;
        let mut quiet = 1000;
        for v in &out {
            if v.abs() > 0.05 && quiet > 2000 {
                bursts += 1;
            }
            quiet = if v.abs() > 0.05 { 0 } else { quiet + 1 };
        }
        assert_eq!(bursts, 2, "two clicks in a second at 120 bpm");
        assert!(m.beat.load(Relaxed) >> 1 >= 2);
    }

    #[test]
    fn looper_records_plays_and_overdubs() {
        let m = Meters::default();
        let mut lp = Looper::new(48000.0, 5.0);
        lp.command(LoopCmd::Record);
        let tone: Vec<f32> = (0..24000).map(|n| 0.3 * (TAU * 220.0 * n as f32 / 48000.0).sin()).collect();
        let mut rec = tone.clone();
        lp.process(&mut rec, &m);
        lp.command(LoopCmd::Play);
        assert_eq!(lp.state(), LoopState::Playing);
        let mut silence = vec![0.0f32; 48000]; // two loop lengths
        lp.process(&mut silence, &m);
        let a: f32 = silence[3000..9000].iter().map(|v| v * v).sum();
        let b: f32 = silence[3000 + 24000..9000 + 24000].iter().map(|v| v * v).sum();
        assert!(a > 1.0 && (a / b - 1.0).abs() < 0.01, "the loop repeats: {a} {b}");
        lp.command(LoopCmd::Overdub);
        let mut more = tone.clone();
        lp.process(&mut more, &m);
        lp.command(LoopCmd::Play);
        let mut again = vec![0.0f32; 24000];
        lp.process(&mut again, &m);
        let c: f32 = again[3000..9000].iter().map(|v| v * v).sum();
        assert!(c > a * 1.5, "overdub adds a layer: {c} vs {a}");
        lp.command(LoopCmd::Clear);
        let mut none = vec![0.0f32; 1000];
        lp.process(&mut none, &m);
        assert!(none.iter().all(|v| *v == 0.0));
    }
}
