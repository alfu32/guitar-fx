//! The running effect chain, and the control channel a UI uses to change it live.
//!
//! The audio thread owns the `Chain`. Anything else talks to it through a `Control`,
//! which queues `Cmd`s that the chain applies at the start of its next block, so edits
//! land within one audio buffer and never block the audio thread.

use crate::effects::{build, Effect};
use crate::params::{Spec, Value};
use crate::effects::PitchTracker;
use crate::tools::{LoopCmd, LoopState, Looper, Metronome};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc};

/// Levels and dropout counter shared with the UI.
/// How many per-effect level meters exist (chain positions beyond this are not metered).
pub const SLOT_METERS: usize = 128;

pub struct Meters {
    in_peak: AtomicU32,
    out_peak: AtomicU32,
    xruns: AtomicU32,
    buffered: AtomicU32,
    trimmed: AtomicU32,
    slots: Vec<AtomicU32>,
    pub(crate) beat: AtomicU32,
    pub(crate) loop_state: AtomicU32,
    pub(crate) loop_len: AtomicU32,
    pub(crate) loop_pos: AtomicU32,
    pitch: AtomicU32,
    tuner_on: AtomicBool,
    tuner_mute: AtomicBool,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            in_peak: AtomicU32::default(),
            out_peak: AtomicU32::default(),
            xruns: AtomicU32::default(),
            buffered: AtomicU32::default(),
            trimmed: AtomicU32::default(),
            beat: AtomicU32::default(),
            loop_state: AtomicU32::default(),
            loop_len: AtomicU32::default(),
            loop_pos: AtomicU32::default(),
            pitch: AtomicU32::default(),
            tuner_on: AtomicBool::default(),
            tuner_mute: AtomicBool::default(),
            slots: (0..SLOT_METERS).map(|_| AtomicU32::default()).collect(),
        }
    }
}

impl Meters {
    /// Peak level of the input (before input gain), with a short decay.
    pub fn in_peak(&self) -> f32 {
        f32::from_bits(self.in_peak.load(Relaxed))
    }

    /// Peak level of the output (after output gain), with a short decay.
    pub fn out_peak(&self) -> f32 {
        f32::from_bits(self.out_peak.load(Relaxed))
    }

    /// Peak level leaving the effect at chain position `i` (held with a short decay).
    pub fn slot_peak(&self, i: usize) -> f32 {
        self.slots.get(i).map_or(0.0, |c| f32::from_bits(c.load(Relaxed)))
    }

    /// Samples waiting between the input and output streams (part of the latency).
    pub fn buffered(&self) -> u32 {
        self.buffered.load(Relaxed)
    }

    /// Total samples dropped to keep that queue from growing (see `live.rs`).
    pub fn trimmed(&self) -> u32 {
        self.trimmed.load(Relaxed)
    }

    pub(crate) fn set_buffered(&self, n: u32) {
        self.buffered.store(n, Relaxed);
    }

    pub(crate) fn add_trimmed(&self, n: u32) {
        self.trimmed.fetch_add(n, Relaxed);
    }

    /// (click count, was it the accented first beat) of the metronome; changes on every click.
    pub fn beat(&self) -> (u32, bool) {
        let b = self.beat.load(Relaxed);
        (b >> 1, b & 1 == 1)
    }

    pub fn loop_state(&self) -> LoopState {
        LoopState::from_u32(self.loop_state.load(Relaxed))
    }

    /// (length, position) of the loop in seconds, given the sample rate.
    pub fn loop_seconds(&self, sr: f32) -> (f32, f32) {
        (self.loop_len.load(Relaxed) as f32 / sr, self.loop_pos.load(Relaxed) as f32 / sr)
    }

    /// Fundamental of the input in Hz while the tuner is on; 0 when there is no clear note.
    pub fn pitch(&self) -> f32 {
        f32::from_bits(self.pitch.load(Relaxed))
    }

    /// Switch the tuner on or off; with `mute` the output is silent while it is on.
    pub fn set_tuner(&self, on: bool, mute: bool) {
        self.tuner_on.store(on, Relaxed);
        self.tuner_mute.store(mute, Relaxed);
        if !on {
            self.pitch.store(0, Relaxed);
        }
    }

    pub fn xruns(&self) -> u32 {
        self.xruns.load(Relaxed)
    }

    pub(crate) fn add_xrun(&self) {
        self.xruns.fetch_add(1, Relaxed);
    }
}

pub struct Slot {
    pub id: u64,
    pub fx: Box<dyn Effect>,
    pub bypass: bool,
}

impl Slot {
    pub fn new(id: u64, spec: &Spec, sr: f32) -> Self {
        Self { id, fx: build(spec, sr), bypass: false }
    }
}

pub enum Cmd {
    /// Change a `live` parameter.
    Set { id: u64, key: &'static str, value: Value },
    Bypass { id: u64, on: bool },
    /// Swap in a freshly built effect (for parameters that are not `live`).
    Replace { id: u64, fx: Box<dyn Effect> },
    Insert { index: usize, slot: Slot },
    Remove { id: u64 },
    Move { id: u64, to: usize },
    Gains { input_db: f32, output_db: f32 },
    Metronome { on: bool, bpm: f32, beats: u32, volume: f32 },
    Looper(LoopCmd),
}

#[derive(Clone)]
pub struct Control {
    tx: mpsc::Sender<Cmd>,
    pub meters: Arc<Meters>,
    pub sample_rate: f32,
}

impl Control {
    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd); // fails only once the stream has stopped
    }
}

pub struct Chain {
    slots: Vec<Slot>,
    in_gain: f32,
    out_gain: f32,
    rx: Option<mpsc::Receiver<Cmd>>,
    meters: Arc<Meters>,
    metronome: Option<Metronome>,
    looper: Option<Looper>,
    tuner: Option<PitchTracker>,
}

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

impl Chain {
    pub fn new(specs: &[Spec], sr: f32, input_db: f32, output_db: f32) -> Self {
        // Spare capacity so live inserts never reallocate on the audio thread.
        let mut slots = Vec::with_capacity(specs.len() + 64);
        slots.extend(specs.iter().enumerate().map(|(i, s)| Slot::new(i as u64, s, sr)));
        Self { slots, in_gain: db(input_db), out_gain: db(output_db), rx: None, meters: Arc::default(), metronome: None, looper: None, tuner: None }
    }

    /// A chain plus the handle that edits it while it runs. Slot ids are the spec indices.
    pub fn controlled(specs: &[Spec], sr: f32, input_db: f32, output_db: f32) -> (Self, Control) {
        let mut chain = Self::new(specs, sr, input_db, output_db);
        let (tx, rx) = mpsc::channel();
        chain.rx = Some(rx);
        chain.metronome = Some(Metronome::new(sr));
        chain.looper = Some(Looper::new(sr, 60.0));
        chain.tuner = Some(PitchTracker::new(sr));
        let control = Control { tx, meters: chain.meters.clone(), sample_rate: sr };
        (chain, control)
    }

    pub fn meters(&self) -> Arc<Meters> {
        self.meters.clone()
    }

    fn apply(&mut self, cmd: Cmd) {
        let find = |slots: &[Slot], id| slots.iter().position(|s| s.id == id);
        match cmd {
            Cmd::Set { id, key, value } => {
                if let Some(i) = find(&self.slots, id) {
                    self.slots[i].fx.set(key, value);
                }
            }
            Cmd::Bypass { id, on } => {
                if let Some(i) = find(&self.slots, id) {
                    self.slots[i].bypass = on;
                }
            }
            Cmd::Replace { id, fx } => {
                if let Some(i) = find(&self.slots, id) {
                    self.slots[i].fx = fx;
                }
            }
            Cmd::Insert { index, slot } => {
                let at = index.min(self.slots.len());
                self.slots.insert(at, slot);
            }
            Cmd::Remove { id } => {
                if let Some(i) = find(&self.slots, id) {
                    self.slots.remove(i);
                }
            }
            Cmd::Move { id, to } => {
                if let Some(i) = find(&self.slots, id) {
                    let slot = self.slots.remove(i);
                    let at = to.min(self.slots.len());
                    self.slots.insert(at, slot);
                }
            }
            Cmd::Gains { input_db, output_db } => {
                self.in_gain = db(input_db);
                self.out_gain = db(output_db);
            }
            Cmd::Metronome { on, bpm, beats, volume } => {
                if let Some(m) = &mut self.metronome {
                    m.configure(on, bpm, beats, volume);
                }
            }
            Cmd::Looper(c) => {
                if let Some(l) = &mut self.looper {
                    l.command(c);
                }
            }
        }
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        let mut pending = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(cmd) = rx.try_recv() {
                pending.push(cmd);
            }
        }
        for cmd in pending {
            self.apply(cmd);
        }

        let decay = 0.9995f32.powi(buf.len() as i32);
        let peak = |buf: &[f32]| buf.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let hold = |cell: &AtomicU32, now: f32| {
            let held = f32::from_bits(cell.load(Relaxed)) * decay;
            cell.store(now.max(held).to_bits(), Relaxed);
        };

        hold(&self.meters.in_peak, peak(buf));
        if self.in_gain != 1.0 {
            buf.iter_mut().for_each(|x| *x *= self.in_gain);
        }
        if let (Some(t), true) = (&mut self.tuner, self.meters.tuner_on.load(Relaxed)) {
            if t.push(buf) {
                let f = t.detect().unwrap_or(0.0);
                self.meters.pitch.store(f.to_bits(), Relaxed);
            }
        }
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if !slot.bypass {
                slot.fx.process(buf);
            }
            if let Some(cell) = self.meters.slots.get(i) {
                hold(cell, peak(buf));
            }
        }
        for cell in self.meters.slots.iter().skip(self.slots.len()) {
            cell.store(0, Relaxed);
        }
        if let Some(l) = &mut self.looper {
            l.process(buf, &self.meters);
        }
        if self.out_gain != 1.0 {
            buf.iter_mut().for_each(|x| *x *= self.out_gain);
        }
        if self.meters.tuner_on.load(Relaxed) && self.meters.tuner_mute.load(Relaxed) {
            buf.fill(0.0);
        }
        if let Some(m) = &mut self.metronome {
            m.process(buf, &self.meters);
        }
        hold(&self.meters.out_peak, peak(buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn tone() -> Vec<f32> {
        (0..4800).map(|n| 0.3 * (std::f32::consts::TAU * 220.0 * n as f32 / SR).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    fn spec(name: &str, raw: &str) -> Spec {
        Spec::parse(name, raw).unwrap()
    }

    #[test]
    fn live_set_changes_the_sound_immediately() {
        let (mut chain, ctl) = Chain::controlled(&[spec("overdrive", "drive=0.0,level=1")], SR, 0.0, 0.0);
        let mut quiet = tone();
        chain.process(&mut quiet);
        ctl.send(Cmd::Set { id: 0, key: "level", value: Value::Num(0.1) });
        let mut softer = tone();
        chain.process(&mut softer);
        assert!(rms(&softer) < rms(&quiet) * 0.3, "{} vs {}", rms(&softer), rms(&quiet));
    }

    #[test]
    fn insert_move_remove_bypass_and_gains() {
        let (mut chain, ctl) = Chain::controlled(&[], SR, 0.0, 0.0);
        let dry = rms(&tone());

        let mut x = tone();
        chain.process(&mut x);
        assert!((rms(&x) - dry).abs() < 1e-4, "empty chain is transparent");

        ctl.send(Cmd::Insert { index: 0, slot: Slot::new(10, &spec("overdrive", "drive=1"), SR) });
        ctl.send(Cmd::Insert { index: 1, slot: Slot::new(11, &spec("tremolo", "depth=1,rate=20"), SR) });
        let mut x = tone();
        chain.process(&mut x);
        assert!((rms(&x) - dry).abs() > 0.01, "effects are applied");

        ctl.send(Cmd::Bypass { id: 10, on: true });
        ctl.send(Cmd::Bypass { id: 11, on: true });
        let mut x = tone();
        chain.process(&mut x);
        assert!((rms(&x) - dry).abs() < 1e-4, "bypassed effects are transparent");

        ctl.send(Cmd::Bypass { id: 10, on: false });
        ctl.send(Cmd::Move { id: 10, to: 1 });
        ctl.send(Cmd::Remove { id: 11 });
        ctl.send(Cmd::Gains { input_db: 0.0, output_db: -6.0 });
        let mut x = tone();
        chain.process(&mut x);
        assert_eq!(chain.slots.len(), 1);
        assert_eq!(chain.slots[0].id, 10);
        assert!(rms(&x) > 0.0);

        ctl.send(Cmd::Replace { id: 10, fx: build(&spec("overdrive", "drive=0"), SR) });
        chain.process(&mut tone());
        assert!(ctl.meters.slot_peak(0) > 0.0 && ctl.meters.slot_peak(5) == 0.0);
        assert!(ctl.meters.out_peak() > 0.0 && ctl.meters.in_peak() > 0.0);
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let (mut chain, ctl) = Chain::controlled(&[spec("chorus", "")], SR, 0.0, 0.0);
        ctl.send(Cmd::Set { id: 99, key: "depth", value: Value::Num(0.1) });
        ctl.send(Cmd::Remove { id: 99 });
        ctl.send(Cmd::Move { id: 99, to: 0 });
        chain.process(&mut tone());
        assert_eq!(chain.slots.len(), 1);
    }
}
