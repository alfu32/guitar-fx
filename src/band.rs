//! The Band: a small multitrack mixer with a drum track, a bass track and a guitar track, all
//! locked to one loop (tempo, bars) and shown on one timeline.
//!
//! - The **drum track** holds a loop you choose from the catalog and can edit step by step.
//! - The **bass** and **guitar** tracks each have a source:
//!   - *Style*: you indicate the root notes by playing them once over the loop (or place them on
//!     the timeline) and the band plays the chosen style on them, changing root where you
//!     indicated;
//!   - *Recorded*: one pass of your own playing is captured and looped as audio.
//! - Each track has a volume, mute and solo.
//!
//! While roots are being recorded the band already plays the style, so you hear it on every note
//! as you play it; at the end of the pass it loops. Everything is synthesised here, so nothing is
//! sampled and nothing has a licence.
//!
//! Patterns are written as strings over a bar, one character per step (16 per 4/4 bar). For the
//! drums: `X` hard, `x` normal, `o` soft, `g` ghost, `.` rest. Bass: `r` root, `f` fifth,
//! `o` octave, `t` third, `s` sixth, `a` approach to the next root (capitals are accented).
//! Guitar: `D`/`d` down-strum hard/soft, `U`/`u` up-strum, `c`/`C` muted chop, `x` scratch,
//! `1`..`6` a single string of the chord (an arpeggio).

use crate::chain::Meters;
use std::f32::consts::TAU;

// ------------------------------------------------------------------------------ drum rhythms

pub struct Rhythm {
    pub name: &'static str,
    pub beats: u32,
    /// Delay of the off-beat eighths, in beats (0 straight, 1/6 a triplet shuffle).
    pub swing: f32,
    kick: &'static str,
    snare: &'static str,
    hat: &'static str,
    open: &'static str,
    rim: &'static str,
    ride: &'static str,
}

const SH: f32 = 1.0 / 6.0;

const fn rhythm(name: &'static str, beats: u32, swing: f32, kick: &'static str, snare: &'static str, hat: &'static str, open: &'static str, rim: &'static str, ride: &'static str) -> Rhythm {
    Rhythm { name, beats, swing, kick, snare, hat, open, rim, ride }
}

pub const RHYTHMS: [Rhythm; 20] = [
    rhythm("Rock", 4, 0.0, "X.......x.o.....", "....X.......X...", "x.o.x.o.x.o.x.o.", "", "", ""),
    rhythm("Rock shuffle", 4, SH, "X.......x.o.....", "....X.......X...", "x.o.x.o.x.o.x.o.", "", "", ""),
    rhythm("Four on the floor", 4, 0.0, "X...X...X...X...", "....x.......x...", "", "..x...x...x...x.", "", ""),
    rhythm("Half-time", 4, 0.0, "X.........x.....", "........X.......", "x.o.x.o.x.o.x.o.", "", "", ""),
    rhythm("Funk", 4, 0.0, "X..o..x...x....o", "....X..g.g..X..g", "xgogxgogxgogxgog", "", "", ""),
    rhythm("Disco", 4, 0.0, "X...X...X...X...", "....x.......x...", "x...x...x...x...", "..x...x...x...x.", "", ""),
    rhythm("Hip-hop", 4, 0.04, "X......o..x.....", "....X.......X...", "x.o.x.o.x.o.x.o.", "", "", ""),
    rhythm("Motown", 4, 0.0, "X.......x.o.....", "o...X...o...X...", "x.o.x.o.x.o.x.o.", "", "", ""),
    rhythm("Reggae one-drop", 4, 0.0, "........X.......", "........o.......", "..x...x...x...x.", "", "", ""),
    rhythm("Ballad", 4, 0.0, "X.........o.....", "....o.......o...", "o.g.o.g.o.g.o.g.", "", "", ""),
    rhythm("Bossa nova", 4, 0.0, "X.....o.X.....o.", "", "o.g.o.g.o.g.o.g.", "", "...o..o...o..o..", ""),
    rhythm("Samba", 4, 0.0, "o...X...o...X...", "", "xgogxgogxgogxgog", "", "x..x..x..x..x...", ""),
    rhythm("Country", 4, 0.0, "X.......X.......", "..g.x.g...g.x.g.", "o...o...o...o...", "", "", ""),
    rhythm("Punk", 4, 0.0, "X.....o.X.....o.", "....X.......X...", "X.x.X.x.X.x.X.x.", "", "", ""),
    rhythm("Metal", 4, 0.0, "XxoxXxoxXxoxXxox", "....X.......X...", "", "", "", "x.x.x.x.x.x.x.x."),
    rhythm("Blues shuffle", 4, SH, "X.....x.....", "...X.....X..", "x.ox.ox.ox.o", "", "", ""),
    rhythm("Jazz swing", 4, SH, "g.....g.....", "", "...o.....o..", "", "", "x..x.ox..x.o"),
    rhythm("Slow 12/8 blues", 4, SH, "X.....x.....", "...X.....X..", "xooxooxooxoo", "", "", ""),
    rhythm("Waltz", 3, 0.0, "X...........", "....x...x...", "o.o.o.o.o.o.", "", "", ""),
    rhythm("Waltz ballad", 3, 0.0, "X...........", "", "o.g.o.g.o.g.", "", "....o...o...", ""),
];

pub fn rhythm_names() -> Vec<&'static str> {
    RHYTHMS.iter().map(|r| r.name).collect()
}

// -------------------------------------------------------------------------- bass and guitar styles

pub const BASS_STYLES: [(&str, &str); 14] = [
    ("Whole notes", "R..............."),
    ("Half notes", "R.......r......."),
    ("Quarter roots", "R...r...r...r..."),
    ("Eighth roots", "R.r.r.r.r.r.r.r."),
    ("Root and fifth", "R.......f......."),
    ("Country (root–fifth)", "R...f...r...f..."),
    ("Disco octaves", "R.o.R.o.R.o.R.o."),
    ("Walking", "R...t...f...a..."),
    ("Funk", "R..r..r...r..f.o"),
    ("Reggae", "R.....r...r....."),
    ("Bossa", "R.....f.r.....f."),
    ("Boogie", "R.r.f.s.R.r.f.s."),
    ("Syncopated", "R..r.r..R..r.r.."),
    ("Pedal sixteenths", "RrrrRrrrRrrrRrrr"),
];

pub const GUITAR_STYLES: [(&str, &str); 13] = [
    ("Whole strum", "D..............."),
    ("Quarter strums", "D...d...d...d..."),
    ("Folk strum", "D...D.U...U.D.U."),
    ("Eighth strums", "D.U.D.U.D.U.D.U."),
    ("Reggae skank", "..c...c...c...c."),
    ("Backbeat stabs", "....C.......C..."),
    ("Funk 16ths", "c.xcx.xCc.xcx.xc"),
    ("Power pulse", "C.C.C.C.C.C.C.C."),
    ("Arpeggio", "1.3.5.6.5.3.4.2."),
    ("Boom-chick", "1...d...2...d..."),
    ("Bossa comping", "D..D..D...D..D.."),
    ("Charleston", "D.....d........."),
    ("Disco chop", "c.xcc.xcc.xcc.xc"),
];

pub const CHORD_TYPES: [&str; 6] = ["Auto (from your roots)", "Major", "Minor", "Dominant 7", "Minor 7", "Power chord"];

pub const DRUM_ROWS: [&str; 6] = ["Kick", "Snare", "Hi-hat", "Open hat", "Rim", "Ride"];
pub const MAX_STEPS: usize = 16;
/// Velocity of a step level: off, ghost, soft, normal, accent.
const LEVEL_VEL: [f32; 5] = [0.0, 0.3, 0.55, 0.8, 1.0];

/// The drum loop of one bar: six rows of steps, each a level 0 (off) to 4 (accent).
#[derive(Clone, Debug, PartialEq)]
pub struct DrumGrid {
    pub steps: usize,
    pub rows: [[u8; MAX_STEPS]; 6],
}

impl Default for DrumGrid {
    fn default() -> Self {
        Self { steps: 16, rows: [[0; MAX_STEPS]; 6] }
    }
}

fn grid_of(r: &Rhythm) -> DrumGrid {
    let pats = [r.kick, r.snare, r.hat, r.open, r.rim, r.ride];
    let steps = if pats.iter().any(|p| p.len() == 12) { 12 } else { 16 };
    let mut g = DrumGrid { steps, ..Default::default() };
    for (row, pat) in pats.iter().enumerate() {
        for (i, c) in pat.chars().take(steps).enumerate() {
            g.rows[row][i] = match c {
                'X' => 4,
                'x' => 3,
                'o' => 2,
                'g' => 1,
                _ => 0,
            };
        }
    }
    g
}

/// Where a played part comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// The band plays a style on the root notes.
    Style,
    /// One pass of your own playing, looped.
    Audio,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Snap {
    Beat,
    Eighth,
    Sixteenth,
}

impl Snap {
    fn grid(self) -> f32 {
        match self {
            Snap::Beat => 1.0,
            Snap::Eighth => 0.5,
            Snap::Sixteenth => 0.25,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandSettings {
    pub on: bool,
    pub rhythm: usize,
    pub bpm: f32,
    pub bars: u32,
    pub drums: f32,
    pub bass: f32,
    pub guitar: f32,
    pub bass_style: usize,
    pub guitar_style: usize,
    /// Index into `CHORD_TYPES`.
    pub chord: usize,
    /// Where the roots you play are rounded to.
    pub snap: Snap,
    /// Mute and solo for drums, bass, guitar.
    pub mute: [bool; 3],
    pub solo: [bool; 3],
    pub bass_src: Source,
    pub guitar_src: Source,
    /// Bars of count-in (with a click) before a recording pass starts.
    pub count_in: u32,
}

impl Default for BandSettings {
    fn default() -> Self {
        Self { on: false, rhythm: 0, bpm: 100.0, bars: 4, drums: 0.8, bass: 0.7, guitar: 0.6, bass_style: 2, guitar_style: 1, chord: 0, snap: Snap::Eighth, mute: [false; 3], solo: [false; 3], bass_src: Source::Style, guitar_src: Source::Style, count_in: 2 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Track {
    Bass = 1,
    Guitar = 2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BandCmd {
    Settings(BandSettings),
    /// Erase the track's current take, go back to the start, count in, then record one pass.
    Record(Track),
    CancelRecord,
    /// Empty what the track currently plays (its roots, or its recording).
    Clear(Track),
    /// Put a root at a position (beats), rounded to the snap grid.
    SetRoot(Track, f32, u8),
    /// Remove the root nearest to a position (beats).
    RemoveRoot(Track, f32),
    /// Set one step of the drum loop: row, step, level 0..4.
    SetDrumStep(u8, u8, u8),
    ClearDrums,
    CopyBassToGuitar,
    /// Back to the start of the loop.
    Restart,
}

/// The roots recorded for each part: (position in beats from the loop start, pitch class).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BandEvents {
    pub loop_beats: f32,
    pub beats_per_bar: u32,
    pub bass: Vec<(f32, u8)>,
    pub guitar: Vec<(f32, u8)>,
    pub drums: DrumGrid,
    /// Where the chosen style plays within a bar (beats), for drawing.
    pub bass_hits: Vec<f32>,
    pub guitar_hits: Vec<f32>,
    /// Peaks of the recorded take (empty: none) and whether it fits the current tempo and loop.
    pub bass_wave: Vec<f32>,
    pub guitar_wave: Vec<f32>,
    pub bass_wave_ok: bool,
    pub guitar_wave_ok: bool,
}

impl BandEvents {
    /// The roots as text, one group per bar: `A | A D | E | —`.
    pub fn text(&self, track: Track) -> String {
        let ev = match track {
            Track::Bass => &self.bass,
            Track::Guitar => &self.guitar,
        };
        if ev.is_empty() || self.beats_per_bar == 0 {
            return "—".into();
        }
        let name = |pc: u8| crate::listen::NOTE_NAMES[pc as usize % 12];
        let bars = (self.loop_beats / self.beats_per_bar as f32).round().max(1.0) as usize;
        (0..bars)
            .map(|b| {
                let (from, to) = (b as f32 * self.beats_per_bar as f32, (b + 1) as f32 * self.beats_per_bar as f32);
                let mut v: Vec<&str> = Vec::new();
                if let Some(pc) = root_at(ev, from, true) {
                    v.push(name(pc));
                }
                for &(t, pc) in ev {
                    if t > from && t < to && v.last() != Some(&name(pc)) {
                        v.push(name(pc));
                    }
                }
                v.join(" ")
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// The root in force at `p` beats: the last event at or before it; before the first event, the
/// last one of the loop when `cyclic` (a loop continues its last chord across the wrap).
fn root_at(ev: &[(f32, u8)], p: f32, cyclic: bool) -> Option<u8> {
    let mut found = None;
    for &(t, pc) in ev {
        if t <= p + 1e-4 {
            found = Some(pc);
        }
    }
    found.or(if cyclic { ev.last().map(|e| e.1) } else { None })
}

/// The first root after `p` that differs from the current one.
fn next_root(ev: &[(f32, u8)], p: f32) -> Option<u8> {
    let cur = root_at(ev, p, true)?;
    let after = ev.iter().filter(|e| e.0 > p + 1e-4).map(|e| e.1);
    let before = ev.iter().map(|e| e.1);
    after.chain(before).find(|&pc| pc != cur)
}

// ----------------------------------------------------------------------------- chords

#[derive(Clone, Copy, Debug, PartialEq)]
enum Quality {
    Major,
    Minor,
    Dom7,
    Min7,
    Power,
}

/// The diatonic chord quality of each root, judged from all the roots that were played: the key
/// that contains most of them (major wins a tie; pick Minor yourself when it matters).
fn diatonic_qualities(roots: &[u8]) -> impl Fn(u8) -> Quality + use<> {
    const MAJOR: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
    const MINOR: [usize; 7] = [0, 2, 3, 5, 7, 8, 10];
    let mut best = (f32::MIN, 0usize, false);
    for tonic in 0..12 {
        for minor in [false, true] {
            let scale = if minor { MINOR } else { MAJOR };
            let mut score = 0.0;
            for &r in roots {
                let d = (r as usize + 12 - tonic) % 12;
                if scale.contains(&d) {
                    score += 1.0;
                }
                if d == 0 {
                    score += 0.3;
                }
            }
            if roots.first().is_some_and(|&r| r as usize == tonic) {
                score += 0.2;
            }
            if score > best.0 + 1e-6 {
                best = (score, tonic, minor);
            }
        }
    }
    let (_, tonic, minor) = best;
    move |pc: u8| {
        let d = (pc as usize + 12 - tonic) % 12;
        let is_minor = if minor { matches!(d, 0 | 2 | 5 | 7) } else { matches!(d, 2 | 4 | 9 | 11) };
        if is_minor { Quality::Minor } else { Quality::Major }
    }
}

/// Six-string voicing of a chord, low to high, as MIDI notes with the root in the low E..D# range.
fn voicing(pc: u8, q: Quality) -> ([u8; 6], usize) {
    let r = 40 + ((pc as usize + 8) % 12) as u8;
    match q {
        Quality::Major => ([r, r + 7, r + 12, r + 16, r + 19, r + 24], 6),
        Quality::Minor => ([r, r + 7, r + 12, r + 15, r + 19, r + 24], 6),
        Quality::Dom7 => ([r, r + 7, r + 10, r + 16, r + 19, r + 24], 6),
        Quality::Min7 => ([r, r + 7, r + 10, r + 15, r + 19, r + 24], 6),
        Quality::Power => ([r, r + 7, r + 12, r + 19, 0, 0], 4),
    }
}

fn midi_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

// ----------------------------------------------------------------------------- voices

#[derive(Clone, Copy, PartialEq, Debug)]
enum Drum {
    Kick,
    Snare,
    Hat,
    Open,
    Rim,
    Ride,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Deg {
    Root,
    Fifth,
    Octave,
    Third,
    Sixth,
    Approach,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Down,
    Up,
    Chop,
    Scratch,
    String(u8),
}

/// One Karplus-Strong string.
struct Ks {
    buf: Vec<f32>,
    len: usize,
    pos: usize,
    wait: u32,
    active: bool,
    gain: f32,
    loop_gain: f32,
    env: f32,
    env_k: f32,
}

const VOICES: usize = 16;

struct Part {
    events: Vec<(f32, u8)>,
    style: usize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Rec {
    /// Waiting for the loop to come round.
    Armed(Track),
    Recording(Track),
}

/// One recorded take of a track.
struct Clip {
    data: Vec<f32>,
    len: usize,
    bpm: f32,
    loop_beats: f32,
    wave: Vec<f32>,
}

impl Clip {
    fn new(cap: usize) -> Self {
        Self { data: vec![0.0; cap], len: 0, bpm: 0.0, loop_beats: 0.0, wave: Vec::new() }
    }

    fn fits(&self, bpm: f32, loop_beats: f32) -> bool {
        self.len > 0 && (self.bpm - bpm).abs() < 1e-3 && (self.loop_beats - loop_beats).abs() < 1e-3
    }
}

fn peaks(x: &[f32]) -> Vec<f32> {
    const BINS: usize = 480;
    (0..BINS)
        .map(|i| {
            let (a, b) = (i * x.len() / BINS, ((i + 1) * x.len() / BINS).max(i * x.len() / BINS + 1).min(x.len()));
            x[a.min(b)..b].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        })
        .collect()
}

pub struct Band {
    sr: f32,
    grid: DrumGrid,
    bass_clip: Clip,
    gtr_clip: Clip,
    rec_data: Vec<f32>,
    rec_len: usize,
    rec_peak: f32,
    loop_len: usize,
    click_beat: i64,
    click_t: f32,
    click_hz: f32,
    pre_note: Option<u8>,
    s: BandSettings,
    running: bool,
    /// Beats since the loop began.
    pos: f64,
    loop_beats: f32,
    bpb: u32,
    last_bar: i64,
    sample_no: u64,
    drum_ev: Vec<(f32, Drum, f32)>,
    bass_ev: Vec<(f32, Deg, f32)>,
    gtr_ev: Vec<(f32, Hit, f32)>,
    di: usize,
    bi: usize,
    gi: usize,
    bci: usize,
    gci: usize,
    bass_part: Part,
    gtr_part: Part,
    rec: Option<Rec>,
    rec_buf: Vec<(f32, u8)>,
    last_seq: u32,
    dirty: bool,
    key: Box<dyn Fn(u8) -> Quality + Send>,
    bass_fired: u64,
    gtr_fired: u64,
    noise: u32,
    prev_n: f32,
    // voices
    kick_t: f32,
    kick_ph: f32,
    kick_v: f32,
    snare_t: f32,
    snare_v: f32,
    hat_t: f32,
    hat_v: f32,
    open_t: f32,
    open_v: f32,
    rim_t: f32,
    rim_v: f32,
    ride_t: f32,
    ride_v: f32,
    bass_t: f32,
    bass_hz: f32,
    bass_ph: f32,
    bass_v: f32,
    bass_lp: f32,
    scratch_t: f32,
    scratch_v: f32,
    ks: Vec<Ks>,
    gtr_lp: f32,
}

impl Band {
    pub fn new(sr: f32) -> Self {
        let cap = (sr / 55.0) as usize + 8;
        let clip_cap = (sr * 49.0) as usize;
        let mut b = Self {
            sr,
            grid: grid_of(&RHYTHMS[0]),
            bass_clip: Clip::new(clip_cap),
            gtr_clip: Clip::new(clip_cap),
            rec_data: vec![0.0; clip_cap],
            rec_len: 0,
            rec_peak: 0.0,
            loop_len: 0,
            click_beat: i64::MIN,
            click_t: 9.0,
            click_hz: 1500.0,
            pre_note: None,
            s: BandSettings::default(),
            running: false,
            pos: 0.0,
            loop_beats: 16.0,
            bpb: 4,
            last_bar: -1,
            sample_no: 0,
            drum_ev: Vec::new(),
            bass_ev: Vec::new(),
            gtr_ev: Vec::new(),
            di: 0,
            bi: 0,
            gi: 0,
            bci: 0,
            gci: 0,
            bass_part: Part { events: Vec::new(), style: 2 },
            gtr_part: Part { events: Vec::new(), style: 1 },
            rec: None,
            rec_buf: Vec::new(),
            last_seq: 0,
            dirty: true,
            key: Box::new(|_| Quality::Major),
            bass_fired: u64::MAX,
            gtr_fired: u64::MAX,
            noise: 0x1234_5678,
            prev_n: 0.0,
            kick_t: 9.0,
            kick_ph: 0.0,
            kick_v: 0.0,
            snare_t: 9.0,
            snare_v: 0.0,
            hat_t: 9.0,
            hat_v: 0.0,
            open_t: 9.0,
            open_v: 0.0,
            rim_t: 9.0,
            rim_v: 0.0,
            ride_t: 9.0,
            ride_v: 0.0,
            bass_t: 9.0,
            bass_hz: 55.0,
            bass_ph: 0.0,
            bass_v: 0.0,
            bass_lp: 0.0,
            scratch_t: 9.0,
            scratch_v: 0.0,
            ks: (0..VOICES)
                .map(|_| Ks { buf: vec![0.0; cap], len: 1, pos: 0, wait: 0, active: false, gain: 0.0, loop_gain: 0.99, env: 0.0, env_k: 0.9999 })
                .collect(),
            gtr_lp: 0.0,
        };
        b.rebuild();
        b.update_len();
        b
    }

    // ---- setup -----------------------------------------------------------------------------

    fn swing_t(&self, t: f32) -> f32 {
        let r = &RHYTHMS[self.s.rhythm.min(RHYTHMS.len() - 1)];
        if (t.fract() - 0.5).abs() < 1e-4 { t + r.swing } else { t }
    }

    fn rebuild(&mut self) {
        let r = &RHYTHMS[self.s.rhythm.min(RHYTHMS.len() - 1)];
        self.bpb = r.beats;
        self.loop_beats = (self.s.bars.clamp(1, 8) * r.beats) as f32;
        let step = r.beats as f32 / self.grid.steps as f32;
        let mut d: Vec<(f32, Drum, f32)> = Vec::new();
        for (row, kind) in [Drum::Kick, Drum::Snare, Drum::Hat, Drum::Open, Drum::Rim, Drum::Ride].into_iter().enumerate() {
            for i in 0..self.grid.steps {
                let lv = self.grid.rows[row][i].min(4) as usize;
                if lv > 0 {
                    d.push((self.swing_t(i as f32 * step), kind, LEVEL_VEL[lv]));
                }
            }
        }
        d.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.drum_ev = d;

        let bars_beats = r.beats as f32;
        let mut b: Vec<(f32, Deg, f32)> = Vec::new();
        let pat = BASS_STYLES[self.bass_part.style.min(BASS_STYLES.len() - 1)].1;
        for (i, c) in pat.chars().enumerate() {
            let t = i as f32 * 0.25;
            if t >= bars_beats {
                break;
            }
            let (deg, v) = match c {
                'R' => (Deg::Root, 1.0),
                'r' => (Deg::Root, 0.8),
                'f' => (Deg::Fifth, 0.85),
                'o' => (Deg::Octave, 0.8),
                't' => (Deg::Third, 0.8),
                's' => (Deg::Sixth, 0.8),
                'a' => (Deg::Approach, 0.8),
                _ => continue,
            };
            b.push((self.swing_t(t), deg, v));
        }
        self.bass_ev = b;

        let mut g: Vec<(f32, Hit, f32)> = Vec::new();
        let pat = GUITAR_STYLES[self.gtr_part.style.min(GUITAR_STYLES.len() - 1)].1;
        for (i, c) in pat.chars().enumerate() {
            let t = i as f32 * 0.25;
            if t >= bars_beats {
                break;
            }
            let (h, v) = match c {
                'D' => (Hit::Down, 1.0),
                'd' => (Hit::Down, 0.65),
                'U' => (Hit::Up, 0.7),
                'u' => (Hit::Up, 0.5),
                'C' => (Hit::Chop, 1.0),
                'c' => (Hit::Chop, 0.75),
                'x' => (Hit::Scratch, 0.6),
                '1'..='6' => (Hit::String(c as u8 - b'1'), 0.85),
                _ => continue,
            };
            g.push((self.swing_t(t), h, v));
        }
        self.gtr_ev = g;
        self.reset_cursors();
    }

    fn reset_cursors(&mut self) {
        self.di = 0;
        self.bi = 0;
        self.gi = 0;
        self.bci = 0;
        self.gci = 0;
        self.last_bar = -1;
    }

    fn update_key(&mut self) {
        let mut roots: Vec<u8> = Vec::new();
        for (_, pc) in self.bass_part.events.iter().chain(&self.gtr_part.events).chain(&self.rec_buf) {
            roots.push(*pc);
        }
        self.key = Box::new(diatonic_qualities(&roots));
    }

    fn quality(&self, pc: u8) -> Quality {
        match self.s.chord {
            1 => Quality::Major,
            2 => Quality::Minor,
            3 => Quality::Dom7,
            4 => Quality::Min7,
            5 => Quality::Power,
            _ => (self.key)(pc),
        }
    }

    pub fn configure(&mut self, s: BandSettings) {
        let old = self.s;
        self.s = BandSettings { rhythm: s.rhythm.min(RHYTHMS.len() - 1), bars: s.bars.clamp(1, 8), bpm: s.bpm.clamp(40.0, 240.0), ..s };
        self.bass_part.style = s.bass_style.min(BASS_STYLES.len() - 1);
        self.gtr_part.style = s.guitar_style.min(GUITAR_STYLES.len() - 1);
        let bpb_before = self.bpb;
        if old.rhythm != self.s.rhythm {
            self.grid = grid_of(&RHYTHMS[self.s.rhythm]);
            self.dirty = true;
        }
        for (t, was, now) in [(Track::Bass, old.bass_src, self.s.bass_src), (Track::Guitar, old.guitar_src, self.s.guitar_src)] {
            if was != now {
                if matches!(self.rec, Some(Rec::Armed(r) | Rec::Recording(r)) if r == t) {
                    self.cancel_rec();
                }
                self.dirty = true;
            }
        }
        if old.rhythm != self.s.rhythm || old.bars != self.s.bars || old.bass_style != self.s.bass_style || old.guitar_style != self.s.guitar_style {
            self.rebuild();
        }
        if self.bpb != bpb_before {
            // a different time signature: the recorded bars no longer line up
            self.bass_part.events.clear();
            self.gtr_part.events.clear();
            self.rec = None;
            self.dirty = true;
        }
        let lb = self.loop_beats;
        for p in [&mut self.bass_part, &mut self.gtr_part] {
            let n = p.events.len();
            p.events.retain(|e| e.0 < lb);
            if p.events.len() != n {
                self.dirty = true;
            }
        }
        if self.pos >= lb as f64 {
            self.pos = 0.0;
            self.reset_cursors();
        }
        if !s.on {
            self.running = false;
            self.cancel_rec();
        } else if !self.running {
            self.running = true;
            self.pos = 0.0;
            self.reset_cursors();
        }
        if old.chord != self.s.chord {
            self.update_key();
        }
        if old.bpm != self.s.bpm || old.bars != self.s.bars || old.rhythm != self.s.rhythm {
            self.update_len();
            self.dirty = true; // whether the takes still fit changes
            if matches!(self.rec, Some(Rec::Recording(_))) && self.rec_len != self.loop_len {
                self.rec_len = 0; // the pass no longer matches the loop; it will be discarded
            }
        }
    }

    fn cancel_rec(&mut self) {
        self.rec = None;
        self.rec_buf.clear();
        self.pre_note = None;
        if self.pos < 0.0 {
            self.pos = 0.0;
            self.reset_cursors();
        }
    }

    fn update_len(&mut self) {
        let n = (self.loop_beats as f64 * 60.0 / self.s.bpm as f64 * self.sr as f64).round() as usize;
        self.loop_len = n.min(self.bass_clip.data.len());
    }

    fn src(&self, t: Track) -> Source {
        match t {
            Track::Bass => self.s.bass_src,
            Track::Guitar => self.s.guitar_src,
        }
    }

    pub fn command(&mut self, c: BandCmd, meters: &Meters) {
        match c {
            BandCmd::Settings(s) => self.configure(s),
            BandCmd::Record(t) => {
                if self.running {
                    // the previous take goes, the loop goes back to the start, a count-in follows
                    match (t, self.src(t)) {
                        (Track::Bass, Source::Audio) => self.bass_clip.len = 0,
                        (Track::Guitar, Source::Audio) => self.gtr_clip.len = 0,
                        (Track::Bass, Source::Style) => self.bass_part.events.clear(),
                        (Track::Guitar, Source::Style) => self.gtr_part.events.clear(),
                    }
                    self.update_key();
                    self.dirty = true;
                    self.rec_buf.clear();
                    self.pre_note = None;
                    self.reset_cursors();
                    self.click_beat = i64::MIN;
                    self.rec = Some(Rec::Armed(t));
                    let lead = self.s.count_in.min(4) as f64 * self.bpb as f64;
                    if lead > 0.0 {
                        self.pos = -lead;
                    } else {
                        self.pos = 0.0;
                        self.on_loop_wrap();
                    }
                }
            }
            BandCmd::CancelRecord => self.cancel_rec(),
            BandCmd::Clear(t) => {
                if self.src(t) == Source::Audio {
                    match t {
                        Track::Bass => self.bass_clip.len = 0,
                        Track::Guitar => self.gtr_clip.len = 0,
                    }
                } else {
                    match t {
                        Track::Bass => self.bass_part.events.clear(),
                        Track::Guitar => self.gtr_part.events.clear(),
                    }
                    self.update_key();
                }
                self.dirty = true;
            }
            BandCmd::SetRoot(t, pos, pc) => {
                let q = self.snap(pos.rem_euclid(self.loop_beats));
                let ev = match t {
                    Track::Bass => &mut self.bass_part.events,
                    Track::Guitar => &mut self.gtr_part.events,
                };
                ev.retain(|e| (e.0 - q).abs() > 1e-3);
                ev.push((q, pc % 12));
                ev.sort_by(|a, b| a.0.total_cmp(&b.0));
                self.update_key();
                self.dirty = true;
            }
            BandCmd::RemoveRoot(t, pos) => {
                let ev = match t {
                    Track::Bass => &mut self.bass_part.events,
                    Track::Guitar => &mut self.gtr_part.events,
                };
                if let Some(i) = (0..ev.len()).min_by(|&a, &b| (ev[a].0 - pos).abs().total_cmp(&(ev[b].0 - pos).abs())) {
                    if (ev[i].0 - pos).abs() < 0.3 {
                        ev.remove(i);
                    }
                }
                self.update_key();
                self.dirty = true;
            }
            BandCmd::SetDrumStep(row, step, level) => {
                if (row as usize) < 6 && (step as usize) < self.grid.steps {
                    self.grid.rows[row as usize][step as usize] = level.min(4);
                    self.rebuild();
                    self.dirty = true;
                }
            }
            BandCmd::ClearDrums => {
                self.grid.rows = [[0; MAX_STEPS]; 6];
                self.rebuild();
                self.dirty = true;
            }
            BandCmd::CopyBassToGuitar => {
                self.gtr_part.events = self.bass_part.events.clone();
                self.update_key();
                self.dirty = true;
            }
            BandCmd::Restart => {
                if matches!(self.rec, Some(Rec::Armed(_))) {
                    self.cancel_rec();
                }
                self.pos = 0.0;
                self.reset_cursors();
            }
        }
        let _ = meters;
    }

    /// The tempo the band is playing at.
    pub fn bpm(&self) -> f32 {
        self.s.bpm
    }

    #[cfg(test)]
    fn events(&self, t: Track) -> &[(f32, u8)] {
        match t {
            Track::Bass => &self.bass_part.events,
            Track::Guitar => &self.gtr_part.events,
        }
    }

    // ---- the roots a part is playing ---------------------------------------------------------

    fn events_of(&self, t: Track) -> &[(f32, u8)] {
        if self.rec == Some(Rec::Recording(t)) {
            return &self.rec_buf;
        }
        match t {
            Track::Bass => &self.bass_part.events,
            Track::Guitar => &self.gtr_part.events,
        }
    }

    fn cyclic(&self, t: Track) -> bool {
        self.rec != Some(Rec::Recording(t))
    }

    // ---- sound -------------------------------------------------------------------------------

    fn rand(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    fn fire_drum(&mut self, d: Drum, v: f32) {
        match d {
            Drum::Kick => {
                self.kick_t = 0.0;
                self.kick_ph = 0.0;
                self.kick_v = v;
            }
            Drum::Snare => {
                self.snare_t = 0.0;
                self.snare_v = v;
            }
            Drum::Hat => {
                self.hat_t = 0.0;
                self.hat_v = v;
                self.open_t = 9.0; // a closed hat chokes an open one
            }
            Drum::Open => {
                self.open_t = 0.0;
                self.open_v = v;
            }
            Drum::Rim => {
                self.rim_t = 0.0;
                self.rim_v = v;
            }
            Drum::Ride => {
                self.ride_t = 0.0;
                self.ride_v = v;
            }
        }
    }

    fn fire_bass(&mut self, deg: Deg, v: f32) {
        let pos = self.pos as f32;
        let Some(root) = root_at(self.events_of(Track::Bass), pos, self.cyclic(Track::Bass)) else { return };
        let q = self.quality(root);
        let midi = |pc: u8| 28 + (pc as usize + 8) % 12;
        let m = match deg {
            Deg::Root => midi(root),
            Deg::Fifth => midi(root) + 7,
            Deg::Octave => midi(root) + 12,
            Deg::Third => midi(root) + if matches!(q, Quality::Minor | Quality::Min7) { 3 } else { 4 },
            Deg::Sixth => midi(root) + 9,
            Deg::Approach => match next_root(self.events_of(Track::Bass), pos) {
                // a semitone below the next root, from the same octave
                Some(n) => {
                    let target = midi(n);
                    if target > 28 { target - 1 } else { target + 11 }
                }
                None => midi(root) + 9,
            },
        };
        self.bass_hz = midi_hz(m as f32);
        self.bass_t = 0.0;
        self.bass_v = v;
        self.bass_fired = self.sample_no;
    }

    fn fire_guitar(&mut self, hit: Hit, v: f32) {
        let pos = self.pos as f32;
        let Some(root) = root_at(self.events_of(Track::Guitar), pos, self.cyclic(Track::Guitar)) else { return };
        self.gtr_fired = self.sample_no;
        let (notes, n) = voicing(root, self.quality(root));
        let sr = self.sr;
        match hit {
            Hit::Scratch => {
                self.scratch_t = 0.0;
                self.scratch_v = v;
            }
            Hit::String(i) => {
                let k = notes[(i as usize) % n];
                self.pluck(k, 0, v, false);
            }
            Hit::Down | Hit::Chop => {
                let chop = hit == Hit::Chop;
                let gap = ((if chop { 0.003 } else { 0.007 }) * sr) as u32;
                for (i, &k) in notes[..n].iter().enumerate() {
                    self.pluck(k, i as u32 * gap, v, chop);
                }
            }
            Hit::Up => {
                let gap = (0.005 * sr) as u32;
                // an upstroke catches only the top strings, high to low
                for (j, i) in (n.saturating_sub(4)..n).rev().enumerate() {
                    self.pluck(notes[i], j as u32 * gap, v * 0.8, false);
                }
            }
        }
    }

    fn pluck(&mut self, midi: u8, wait: u32, vel: f32, chop: bool) {
        let len = ((self.sr / midi_hz(midi as f32)) - 0.5).round().max(2.0) as usize;
        let mut best = 0;
        let mut quiet = f32::MAX;
        for (i, v) in self.ks.iter().enumerate() {
            let e = if v.active { v.env } else { -1.0 };
            if e < quiet {
                quiet = e;
                best = i;
            }
        }
        let mut lp = 0.0;
        let mut noise = self.noise;
        let v = &mut self.ks[best];
        let len = len.min(v.buf.len());
        for x in v.buf[..len].iter_mut() {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let n = noise as f32 / u32::MAX as f32 * 2.0 - 1.0;
            lp += 0.6 * (n - lp);
            *x = lp * 1.6;
        }
        self.noise = noise;
        v.len = len;
        v.pos = 0;
        v.wait = wait;
        v.active = true;
        v.gain = vel * 0.22;
        v.env = 1.0;
        if chop {
            v.loop_gain = 0.95;
            v.env_k = (-1.0 / (0.1 * self.sr)).exp();
        } else {
            v.loop_gain = 0.997;
            v.env_k = (-1.0 / (2.2 * self.sr)).exp();
        }
    }

    // ---- recording ---------------------------------------------------------------------------

    fn snap(&self, t: f32) -> f32 {
        let g = self.s.snap.grid();
        let mut q = (t / g).round() * g;
        if q >= self.loop_beats {
            q -= self.loop_beats;
        }
        self.swing_t(q)
    }

    fn on_note(&mut self, pc: u8, lag: f32) {
        if let Some(Rec::Armed(t)) = self.rec {
            // a root played just ahead of the downbeat belongs to the first beat
            if self.src(t) == Source::Style && self.pos > -0.3 {
                self.pre_note = Some(pc);
            }
            return;
        }
        let Some(Rec::Recording(t)) = self.rec else { return };
        if self.src(t) != Source::Style {
            return;
        }
        let bps = self.s.bpm / 60.0;
        let t = (self.pos as f32 - lag * bps).rem_euclid(self.loop_beats);
        let t = self.snap(t);
        if let Some(e) = self.rec_buf.iter_mut().find(|e| (e.0 - t).abs() < 1e-3) {
            e.1 = pc;
        } else {
            self.rec_buf.push((t, pc));
            self.rec_buf.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        self.update_key();
    }

    fn on_loop_wrap(&mut self) {
        match self.rec {
            Some(Rec::Armed(t)) => {
                self.rec = Some(Rec::Recording(t));
                self.rec_buf.clear();
                self.rec_len = self.loop_len;
                self.rec_peak = 0.0;
                if let Some(pc) = self.pre_note.take() {
                    self.rec_buf.push((0.0, pc));
                    self.update_key();
                }
            }
            Some(Rec::Recording(t)) if self.src(t) == Source::Audio => {
                self.commit_audio(t);
                self.rec = None;
            }
            Some(Rec::Recording(t)) => {
                if !self.rec_buf.is_empty() {
                    let ev = std::mem::take(&mut self.rec_buf);
                    match t {
                        Track::Bass => self.bass_part.events = ev,
                        Track::Guitar => self.gtr_part.events = ev,
                    }
                }
                self.rec = None;
                self.rec_buf.clear();
                self.update_key();
                self.dirty = true;
            }
            None => {}
        }
        self.reset_cursors();
    }

    /// Keeps the pass just recorded as the track's take, unless it is empty or the loop changed.
    fn commit_audio(&mut self, t: Track) {
        let n = self.rec_len;
        if n == 0 || n != self.loop_len || self.rec_peak < 0.004 {
            return;
        }
        let f = ((0.003 * self.sr) as usize).min(n / 2);
        for i in 0..f {
            let g = i as f32 / f as f32;
            self.rec_data[i] *= g;
            self.rec_data[n - 1 - i] *= g;
        }
        let clip = match t {
            Track::Bass => &mut self.bass_clip,
            Track::Guitar => &mut self.gtr_clip,
        };
        std::mem::swap(&mut clip.data, &mut self.rec_data);
        clip.len = n;
        clip.bpm = self.s.bpm;
        clip.loop_beats = self.loop_beats;
        clip.wave = peaks(&clip.data[..n]);
        self.dirty = true;
    }

    // ---- the audio ---------------------------------------------------------------------------

    /// Adds the band to `buf`.
    pub fn process(&mut self, buf: &mut [f32], meters: &Meters) {
        let heard = meters.heard();
        if let Some((seq, pc, lag)) = heard.note {
            if seq != self.last_seq {
                self.last_seq = seq;
                self.on_note(pc, lag);
            }
        }
        if !self.running {
            meters.set_band_pos(0.0, (0, 0));
            return;
        }
        let dt = 1.0 / self.sr;
        let advance = self.s.bpm as f64 / 60.0 / self.sr as f64;
        let any_solo = self.s.solo.iter().any(|&v| v);
        let aud = |i: usize| (!self.s.mute[i] && (!any_solo || self.s.solo[i])) as u8 as f32;
        let (dv, bv, gv) = (self.s.drums * 0.8 * aud(0), self.s.bass * 0.55 * 1.4 * aud(1), self.s.guitar * aud(2));
        let (cb, cg) = (self.s.bass * aud(1), self.s.guitar * aud(2));
        let rec_t = match self.rec {
            Some(Rec::Recording(t)) => Some(t),
            _ => None,
        };
        let rec_audio = rec_t.filter(|&t| self.src(t) == Source::Audio);
        // a take plays back only if it fits the loop, and not while it is being replaced
        let bass_clip_on = self.s.bass_src == Source::Audio && rec_audio != Some(Track::Bass) && self.bass_clip.fits(self.s.bpm, self.loop_beats);
        let gtr_clip_on = self.s.guitar_src == Source::Audio && rec_audio != Some(Track::Guitar) && self.gtr_clip.fits(self.s.bpm, self.loop_beats);
        let (style_bass, style_gtr) = (self.s.bass_src == Source::Style, self.s.guitar_src == Source::Style);
        let mut levels = [0.0f32; 3];
        let bpb = self.bpb as f64;
        for x in buf.iter_mut() {
            let player = *x;
            self.sample_no += 1;
            let leading = self.pos < 0.0;
            self.pos += advance;
            if leading {
                let beat = self.pos.floor() as i64;
                if self.pos < 0.0 && beat != self.click_beat {
                    self.click_beat = beat;
                    self.click_t = 0.0;
                    self.click_hz = if beat.rem_euclid(self.bpb as i64) == 0 { 2200.0 } else { 1500.0 };
                }
            }
            if leading && self.pos >= 0.0 {
                self.on_loop_wrap();
            } else if self.pos >= self.loop_beats as f64 {
                self.pos -= self.loop_beats as f64;
                self.on_loop_wrap();
            }
            let bar = (self.pos / bpb).floor();
            if bar as i64 != self.last_bar {
                self.last_bar = bar as i64;
                self.di = 0;
                self.bi = 0;
                self.gi = 0;
            }
            let bar_pos = (self.pos - bar * bpb) as f32;
            while self.di < self.drum_ev.len() && self.drum_ev[self.di].0 <= bar_pos {
                let (_, d, v) = self.drum_ev[self.di];
                self.fire_drum(d, v);
                self.di += 1;
            }
            while self.bi < self.bass_ev.len() && self.bass_ev[self.bi].0 <= bar_pos {
                let (_, d, v) = self.bass_ev[self.bi];
                self.fire_bass(d, v);
                self.bi += 1;
            }
            while self.gi < self.gtr_ev.len() && self.gtr_ev[self.gi].0 <= bar_pos {
                let (_, h, v) = self.gtr_ev[self.gi];
                self.fire_guitar(h, v);
                self.gi += 1;
            }
            // where the root changes the part must sound, even if its pattern rests there
            let pos = self.pos as f32;
            while self.bci < self.events_of(Track::Bass).len() && self.events_of(Track::Bass)[self.bci].0 <= pos {
                self.bci += 1;
                if self.bass_fired != self.sample_no {
                    self.fire_bass(Deg::Root, 0.9);
                }
            }
            while self.gci < self.events_of(Track::Guitar).len() && self.events_of(Track::Guitar)[self.gci].0 <= pos {
                self.gci += 1;
                if self.gtr_fired != self.sample_no {
                    self.fire_guitar(Hit::Down, 0.9);
                }
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
            if self.hat_t < 0.3 {
                d += hp * 0.22 * (-self.hat_t * 90.0).exp() * self.hat_v;
                self.hat_t += dt;
            }
            if self.open_t < 0.6 {
                d += hp * 0.22 * (-self.open_t * 18.0).exp() * self.open_v;
                self.open_t += dt;
            }
            if self.rim_t < 0.1 {
                d += ((TAU * 820.0 * self.rim_t).sin() * (-self.rim_t * 70.0).exp() * 0.4 + hp * 0.2 * (-self.rim_t * 120.0).exp()) * self.rim_v;
                self.rim_t += dt;
            }
            if self.ride_t < 1.2 {
                let ring = ((TAU * 3400.0 * self.ride_t).sin() + (TAU * 5100.0 * self.ride_t).sin()) * 0.05;
                d += (hp * 0.14 * (-self.ride_t * 7.0).exp() + ring * (-self.ride_t * 6.0).exp()) * self.ride_v;
                self.ride_t += dt;
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
            let mut g = 0.0;
            for v in self.ks.iter_mut().filter(|v| v.active) {
                if v.wait > 0 {
                    v.wait -= 1;
                    continue;
                }
                let y = v.buf[v.pos];
                let next = v.buf[if v.pos + 1 == v.len { 0 } else { v.pos + 1 }];
                v.buf[v.pos] = v.loop_gain * 0.5 * (y + next);
                v.pos = if v.pos + 1 == v.len { 0 } else { v.pos + 1 };
                g += y * v.env * v.gain;
                v.env *= v.env_k;
                if v.env < 1e-3 {
                    v.active = false;
                }
            }
            if self.scratch_t < 0.08 {
                g += hp * 0.3 * (-self.scratch_t * 140.0).exp() * self.scratch_v * 0.5;
                self.scratch_t += dt;
            }
            self.gtr_lp += 0.4 * (g - self.gtr_lp);
            let (sb, sg) = (if style_bass { b * bv } else { 0.0 }, if style_gtr { self.gtr_lp * gv * 1.6 } else { 0.0 });
            let idx = if self.loop_len > 0 { ((self.pos.rem_euclid(self.loop_beats as f64) / self.loop_beats as f64 * self.loop_len as f64) as usize).min(self.loop_len - 1) } else { 0 };
            let mut click = 0.0;
            if self.click_t < 0.06 {
                click = (TAU * self.click_hz * self.click_t).sin() * (-self.click_t * 70.0).exp() * 0.5;
                self.click_t += dt;
            }
            let (mut ab, mut ag) = (0.0, 0.0);
            if bass_clip_on {
                ab = self.bass_clip.data[idx] * cb;
            }
            if gtr_clip_on {
                ag = self.gtr_clip.data[idx] * cg;
            }
            if let (Some(_), true) = (rec_audio, self.rec_len == self.loop_len && self.loop_len > 0) {
                self.rec_data[idx] = player;
                self.rec_peak = self.rec_peak.max(player.abs());
            }
            levels[0] = levels[0].max((d * dv).abs());
            levels[1] = levels[1].max((sb + ab).abs());
            levels[2] = levels[2].max((sg + ag).abs());
            *x += (d * dv + sb + sg).clamp(-1.2, 1.2).tanh() + ab + ag + click;
        }
        meters.set_band_levels(levels);
        let rec = match self.rec {
            None => (0, 0),
            Some(Rec::Armed(t)) => (t as u32, 0),
            Some(Rec::Recording(t)) => (t as u32, 1),
        };
        meters.set_band_pos(self.pos as f32, rec);
        if self.dirty {
            let ev = BandEvents {
                loop_beats: self.loop_beats,
                beats_per_bar: self.bpb,
                bass: self.bass_part.events.clone(),
                guitar: self.gtr_part.events.clone(),
                drums: self.grid.clone(),
                bass_hits: self.bass_ev.iter().map(|e| e.0).collect(),
                guitar_hits: self.gtr_ev.iter().map(|e| e.0).collect(),
                bass_wave: if self.bass_clip.len > 0 { self.bass_clip.wave.clone() } else { Vec::new() },
                guitar_wave: if self.gtr_clip.len > 0 { self.gtr_clip.wave.clone() } else { Vec::new() },
                bass_wave_ok: self.bass_clip.fits(self.s.bpm, self.loop_beats),
                guitar_wave_ok: self.gtr_clip.fits(self.s.bpm, self.loop_beats),
            };
            self.dirty = !meters.set_band_events(&ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listen::{Analysis, NoteEvent};

    const SR: f32 = 48000.0;

    fn settings() -> BandSettings {
        BandSettings { on: true, bpm: 120.0, bars: 2, drums: 0.0, bass: 0.0, guitar: 0.0, count_in: 1, ..Default::default() }
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

    fn period_hz(x: &[f32], lo: f32, hi: f32) -> f32 {
        let ac = |lag: usize| (0..x.len() - lag).map(|i| x[i] * x[i + lag]).sum::<f32>();
        let best = ((SR / hi) as usize..(SR / lo) as usize).max_by(|&a, &b| ac(a).total_cmp(&ac(b))).unwrap();
        SR / best as f32
    }

    /// Plays root notes the way the Listener would report them: each reported 80 ms after the pick.
    struct Player {
        seq: u32,
    }

    impl Player {
        fn note(&mut self, m: &Meters, pc: u8) {
            self.seq += 1;
            m.publish_analysis(&Analysis { note: Some(NoteEvent { seq: self.seq, pc, midi: 60.0, lag: 0.08 }), ..Default::default() });
        }
    }

    /// Render `secs`, reporting the (time, pc) notes at their time + 80 ms.
    fn play(b: &mut Band, m: &Meters, p: &mut Player, from: f32, secs: f32, notes: &[(f32, u8)]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut n = 0usize;
        let total = (secs * SR) as usize;
        let mut pending: Vec<(usize, u8)> = notes.iter().map(|&(t, pc)| ((((t + 0.08) - from) * SR) as usize, pc)).collect();
        pending.reverse();
        while n < total {
            if let Some(&(at, pc)) = pending.last() {
                if at <= n {
                    p.note(m, pc);
                    pending.pop();
                }
            }
            let mut blk = [0.0f32; 64];
            b.process(&mut blk, m);
            out.extend_from_slice(&blk);
            n += 64;
        }
        out
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
        b.configure(BandSettings { drums: 0.8, rhythm: 2, ..settings() });
        let x = render(&mut b, &m, 4.0);
        let beat = (SR * 0.5) as usize;
        let w = (SR * 0.04) as usize;
        for k in 1..7 {
            let at = k * beat;
            let (after, before) = (energy(&x[at..at + w]), energy(&x[at - w..at]));
            assert!(after > 2.0 * before, "beat {k}: {after} vs {before}");
        }
    }

    #[test]
    fn every_rhythm_has_sound_and_stays_bounded_and_the_waltz_has_three_beats() {
        for (i, r) in RHYTHMS.iter().enumerate() {
            for p in [r.kick, r.snare, r.hat, r.open, r.rim, r.ride] {
                assert!(p.is_empty() || p.len() == 16 || p.len() == 12, "{} pattern {:?}", r.name, p);
            }
            let mut b = Band::new(SR);
            let m = Meters::default();
            b.configure(BandSettings { drums: 1.0, rhythm: i, ..settings() });
            let x = render(&mut b, &m, 4.0);
            let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
            assert!(peak > 0.1 && peak <= 1.01, "{} peak {peak}", r.name);
        }
        let mut b = Band::new(SR);
        b.configure(BandSettings { rhythm: 18, ..settings() });
        assert_eq!(b.loop_beats, 6.0);
        for (name, pat) in BASS_STYLES.iter().chain(GUITAR_STYLES.iter()) {
            assert_eq!(pat.len(), 16, "{name}");
        }
    }

    #[test]
    fn it_records_the_bass_roots_you_play_and_plays_them_where_you_played() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        let mut p = Player { seq: 0 };
        // two bars at 120 bpm = 4 s; quarter-note roots, no drums
        b.configure(BandSettings { bass: 1.0, ..settings() });
        b.command(BandCmd::Record(Track::Bass), &m);
        assert_eq!(b.rec, Some(Rec::Armed(Track::Bass)));
        // one bar of count-in (2 s), then the pass: play A at its start and E at bar 2 (2 s in)
        play(&mut b, &m, &mut p, 0.0, 2.02, &[]);
        assert_eq!(b.rec, Some(Rec::Recording(Track::Bass)));
        let live = play(&mut b, &m, &mut p, 2.02, 4.0, &[(2.0, 9), (4.0, 4)]);
        assert_eq!(b.events(Track::Bass), &[(0.0, 9), (4.0, 4)], "recorded at the loop start and at bar 2");
        // the style was already sounding while recording: bar 1 at A1 (55 Hz) right after the note
        assert!((period_hz(&live[(SR * 0.45) as usize..(SR * 0.6) as usize], 30.0, 200.0) - 55.0).abs() < 8.0);
        // looping now: bar 1 plays A (55 Hz), bar 2 plays E (41.2 Hz); the pass ended at 6 s
        let x = play(&mut b, &m, &mut p, 6.02, 4.0, &[]);
        let a = period_hz(&x[(SR * 0.06) as usize..(SR * 0.2) as usize], 30.0, 200.0);
        let e = period_hz(&x[(SR * 2.06) as usize..(SR * 2.2) as usize], 30.0, 200.0);
        assert!((a - 55.0).abs() < 8.0, "bar 1: {a}");
        assert!((e - 41.2).abs() < 6.0, "bar 2: {e}");
        assert_eq!(m.band_events().text(Track::Bass), "A | E");
    }

    #[test]
    fn the_guitar_strums_the_chord_you_indicated() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        let mut p = Player { seq: 0 };
        b.configure(BandSettings { guitar: 1.0, bars: 1, guitar_style: 0, ..settings() });
        b.command(BandCmd::Record(Track::Guitar), &m);
        play(&mut b, &m, &mut p, 0.0, 2.02, &[]);
        play(&mut b, &m, &mut p, 2.02, 2.0, &[(2.0, 9)]);
        assert_eq!(b.events(Track::Guitar), &[(0.0, 9)]);
        let x = play(&mut b, &m, &mut p, 4.02, 2.0, &[]);
        // a whole-note strum of A major: the common period of its strings is A2 = 110 Hz
        let hz = period_hz(&x[(SR * 0.1) as usize..(SR * 0.5) as usize], 60.0, 400.0);
        assert!((hz - 110.0).abs() < 6.0, "{hz}");
        assert!(energy(&x[..(SR * 0.5) as usize]) > 1.0);
    }

    #[test]
    fn a_root_change_sounds_even_where_the_style_rests() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(BandSettings { bass: 1.0, bass_style: 0, ..settings() }); // whole notes
        b.bass_part.events = vec![(0.0, 9), (1.5, 4)];
        b.update_key();
        let x = render(&mut b, &m, 2.0);
        // beat 1.5 is 0.75 s in; the whole note from the bar start has faded, a new one begins there
        let before = energy(&x[(SR * 0.65) as usize..(SR * 0.72) as usize]);
        let after = energy(&x[(SR * 0.78) as usize..(SR * 0.85) as usize]);
        assert!(after > 1.5 * before, "{after} vs {before}");
    }

    #[test]
    fn recording_erases_the_old_take_and_an_empty_pass_leaves_it_empty() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(BandSettings { bars: 1, ..settings() });
        b.bass_part.events = vec![(0.0, 7)];
        b.command(BandCmd::Record(Track::Bass), &m);
        assert!(b.events(Track::Bass).is_empty(), "the old roots go as soon as you record");
        render(&mut b, &m, 4.5);
        assert!(b.events(Track::Bass).is_empty());
        assert_eq!(m.band_rec(), (0, 0));
        b.bass_part.events = vec![(0.0, 7)];
        b.command(BandCmd::CopyBassToGuitar, &m);
        assert_eq!(b.events(Track::Guitar), &[(0.0, 7)]);
        b.command(BandCmd::Clear(Track::Bass), &m);
        render(&mut b, &m, 0.1);
        assert!(m.band_events().bass.is_empty());
    }

    #[test]
    fn recording_counts_in_from_the_start_with_a_click() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(BandSettings { bars: 2, count_in: 2, ..settings() });
        render(&mut b, &m, 3.3); // somewhere in the loop
        b.command(BandCmd::Record(Track::Guitar), &m);
        let x = render(&mut b, &m, 1.0);
        assert!(m.band_pos() < 0.0 && m.band_pos() > -8.0, "the cursor is in the lead-in: {}", m.band_pos());
        assert_eq!(m.band_rec(), (2, 0), "counting in");
        assert!(energy(&x) > 0.5, "a click is heard with every other track silent");
        render(&mut b, &m, 3.05); // 2 bars of 2 s = 4 s of count-in in all
        assert_eq!(m.band_rec(), (2, 1), "recording");
        assert!(m.band_pos() > 0.0 && m.band_pos() < 1.0, "at the start of the loop: {}", m.band_pos());
        b.command(BandCmd::CancelRecord, &m);
        assert_eq!(m.band_rec().0, 2);
        render(&mut b, &m, 0.1);
        assert_eq!(m.band_rec(), (0, 0));
    }

    #[test]
    fn chord_quality_comes_from_the_key_of_the_roots() {
        let q = diatonic_qualities(&[0, 9, 5, 7]); // C Am F G
        assert_eq!([q(0), q(9), q(5), q(7)], [Quality::Major, Quality::Minor, Quality::Major, Quality::Major]);
        let q = diatonic_qualities(&[9, 2, 4]); // A D E
        assert_eq!([q(9), q(2), q(4)], [Quality::Major, Quality::Major, Quality::Major]);
        let q = diatonic_qualities(&[2, 7, 0, 9]); // Dm Gm C Am -> D minor
        assert_eq!(q(2), Quality::Minor);
    }

    #[test]
    fn a_recorded_take_loops_back_where_it_was_played() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        // 1 bar at 120 bpm = 2 s; the bass track plays what you record
        b.configure(BandSettings { bass: 1.0, bars: 1, bass_src: Source::Audio, ..settings() });
        b.command(BandCmd::Record(Track::Bass), &m);
        // wait for the top of the loop, with silence in
        render(&mut b, &m, 2.02);
        assert_eq!(m.band_rec(), (1, 1));
        // play a 220 Hz burst in the second half of the pass (1.0 s .. 1.5 s)
        let mut got = Vec::new();
        for k in 0..(2.0 * SR / 64.0) as usize {
            let mut blk = [0.0f32; 64];
            for (j, v) in blk.iter_mut().enumerate() {
                let t = (k * 64 + j) as f32 / SR;
                if (1.0..1.5).contains(&t) {
                    *v = 0.5 * (TAU * 220.0 * t).sin();
                }
            }
            b.process(&mut blk, &m);
            got.extend_from_slice(&blk);
        }
        assert_eq!(m.band_rec(), (0, 0));
        assert!(m.band_events().bass_wave_ok);
        // now silence in: the take comes back at the same place in the loop
        let x = render(&mut b, &m, 2.0);
        let first = energy(&x[..(SR * 0.9) as usize]);
        let burst = energy(&x[(SR * 1.1) as usize..(SR * 1.4) as usize]);
        assert!(first < 0.01 && burst > 100.0, "{first} {burst}");
        assert!((period_hz(&x[(SR * 1.1) as usize..(SR * 1.4) as usize], 100.0, 500.0) - 220.0).abs() < 5.0);
        // it does not play while the source is style, and clear removes it
        b.configure(BandSettings { bass: 1.0, bars: 1, bass_src: Source::Style, ..settings() });
        assert!(energy(&render(&mut b, &m, 2.0)) < 1e-6);
        b.configure(BandSettings { bass: 1.0, bars: 1, bass_src: Source::Audio, ..settings() });
        b.command(BandCmd::Clear(Track::Bass), &m);
        assert!(energy(&render(&mut b, &m, 2.0)) < 1e-6);
    }

    #[test]
    fn a_take_made_at_another_tempo_stays_silent() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(BandSettings { bass: 1.0, bars: 1, bass_src: Source::Audio, ..settings() });
        b.command(BandCmd::Record(Track::Bass), &m);
        let mut blk = [0.3f32; 64];
        for _ in 0..(5.0 * SR / 64.0) as usize {
            blk.fill(0.3);
            b.process(&mut blk, &m);
        }
        assert!(m.band_events().bass_wave_ok);
        b.configure(BandSettings { bass: 1.0, bars: 1, bpm: 90.0, bass_src: Source::Audio, ..settings() });
        render(&mut b, &m, 0.1);
        assert!(!m.band_events().bass_wave_ok);
        assert!(energy(&render(&mut b, &m, 2.0)) < 1e-6);
    }

    #[test]
    fn mute_and_solo_pick_what_is_heard() {
        let run = |mute: [bool; 3], solo: [bool; 3]| {
            let mut b = Band::new(SR);
            let m = Meters::default();
            b.configure(BandSettings { drums: 1.0, mute, solo, ..settings() });
            energy(&render(&mut b, &m, 2.0))
        };
        assert!(run([false; 3], [false; 3]) > 1.0);
        assert!(run([true, false, false], [false; 3]) < 1e-6);
        assert!(run([false; 3], [false, true, false]) < 1e-6);
        assert!(run([false; 3], [true, false, false]) > 1.0);
    }

    #[test]
    fn drum_steps_can_be_edited_and_roots_placed_by_hand() {
        let mut b = Band::new(SR);
        let m = Meters::default();
        b.configure(BandSettings { drums: 1.0, rhythm: 0, ..settings() });
        b.command(BandCmd::ClearDrums, &m);
        assert!(energy(&render(&mut b, &m, 1.0)) < 1e-6);
        b.command(BandCmd::SetDrumStep(0, 4, 4), &m); // a kick on beat 2
        let x = render(&mut b, &m, 2.0);
        assert!(energy(&x) > 1.0);
        render(&mut b, &m, 0.05);
        assert_eq!(m.band_events().drums.rows[0][4], 4);
        // roots by hand: rounded to the grid, replaced, removed
        b.configure(BandSettings { snap: Snap::Beat, bass: 1.0, ..settings() });
        b.command(BandCmd::SetRoot(Track::Bass, 0.1, 9), &m);
        b.command(BandCmd::SetRoot(Track::Bass, 4.9, 4), &m);
        assert_eq!(b.events(Track::Bass), &[(0.0, 9), (5.0, 4)]);
        b.command(BandCmd::SetRoot(Track::Bass, 5.2, 7), &m);
        assert_eq!(b.events(Track::Bass), &[(0.0, 9), (5.0, 7)]);
        b.command(BandCmd::RemoveRoot(Track::Bass, 5.1), &m);
        assert_eq!(b.events(Track::Bass), &[(0.0, 9)]);
        // changing the rhythm loads that rhythm's drum loop again
        b.configure(BandSettings { rhythm: 2, ..settings() });
        assert_eq!(b.grid.rows[0][4], 4);
        assert_eq!(b.grid.rows[0][1], 0);
    }

    #[test]
    fn changing_to_a_waltz_discards_roots_that_no_longer_fit() {
        let mut b = Band::new(SR);
        b.configure(settings());
        b.bass_part.events = vec![(0.0, 9), (6.0, 4)];
        b.configure(BandSettings { rhythm: 18, ..settings() });
        assert!(b.events(Track::Bass).is_empty());
    }
}
