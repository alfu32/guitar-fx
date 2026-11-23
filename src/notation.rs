//! Music notation for the band's score view.
//!
//! A track's notes (see `band::NoteHit`) are written as ABC text with the key signature, the time
//! signature and the tempo, and with `ui` enabled drawn by the `acorde` engraver (pure Rust) into
//! an image. The notation is drawn from vector shapes, so it needs no music font.
//!
//! Times are rounded to sixteenth notes, so a swung or shuffled part is shown straight.

use crate::band::NoteHit;
use std::collections::HashMap;

/// What to notate.
pub struct ScoreSpec<'a> {
    pub hits: &'a [NoteHit],
    /// Sharps (positive) or flats (negative).
    pub key_sig: i8,
    pub minor: bool,
    pub bpm: f32,
    pub bars: u32,
    pub beats_per_bar: u32,
    /// A bass part: written an octave above the sound, in the bass clef only.
    pub bass: bool,
}

const MAJOR_KEYS: [&str; 15] = ["Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#"];
const MINOR_KEYS: [&str; 15] = ["Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#"];
const LETTERS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];
/// Lengths (in sixteenths) that are written as one note or rest.
const WRITABLE: [u32; 8] = [16, 12, 8, 6, 4, 3, 2, 1];

/// Letter (0 = C) and accidental of a pitch class, spelled with sharps or flats.
pub fn spell(pc: usize, flats: bool) -> (usize, i8) {
    const SHARP: [(usize, i8); 12] = [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1), (4, 0), (4, 1), (5, 0), (5, 1), (6, 0)];
    const FLAT: [(usize, i8); 12] = [(0, 0), (1, -1), (1, 0), (2, -1), (2, 0), (3, 0), (4, -1), (4, 0), (5, -1), (5, 0), (6, -1), (6, 0)];
    if flats { FLAT[pc % 12] } else { SHARP[pc % 12] }
}

/// The accidental a key signature puts on a letter (0 = C).
pub fn implied(letter: usize, sig: i8) -> i8 {
    const SHARPS: [usize; 7] = [3, 0, 4, 1, 5, 2, 6];
    const FLATS: [usize; 7] = [6, 2, 5, 1, 4, 0, 3];
    if sig > 0 && SHARPS[..sig.min(7) as usize].contains(&letter) {
        1
    } else if sig < 0 && FLATS[..(-sig).min(7) as usize].contains(&letter) {
        -1
    } else {
        0
    }
}

/// An ABC note for a MIDI pitch, with an accidental only where the bar's running state needs one.
fn abc_note(midi: i32, sig: i8, state: &mut HashMap<(usize, i32), i8>) -> String {
    let (letter, acc) = spell(midi.rem_euclid(12) as usize, sig < 0);
    let oct = midi.div_euclid(12) - 1;
    let cur = *state.get(&(letter, oct)).unwrap_or(&implied(letter, sig));
    let mut s = String::new();
    if acc != cur {
        s.push(match acc {
            1 => '^',
            -1 => '_',
            _ => '=',
        });
    }
    state.insert((letter, oct), acc);
    if oct >= 5 {
        s.push(LETTERS[letter].to_ascii_lowercase());
        s.extend(std::iter::repeat_n('\'', (oct - 5) as usize));
    } else {
        s.push(LETTERS[letter]);
        s.extend(std::iter::repeat_n(',', (4 - oct) as usize));
    }
    s
}

fn push_rests(out: &mut String, mut n: u32) {
    for w in WRITABLE {
        while n >= w {
            out.push_str(&format!("z{w} "));
            n -= w;
        }
    }
}

struct Ev {
    t: u32,
    dur: u32,
    notes: Vec<i32>,
}

fn voice(spec: &ScoreSpec, treble: bool) -> Option<String> {
    let shift = if spec.bass { 12 } else { 0 };
    let mut evs: Vec<Ev> = Vec::new();
    for h in spec.hits {
        let mut notes: Vec<i32> = h.notes.iter().map(|&m| m as i32 + shift).filter(|&m| spec.bass && !treble || !spec.bass && (m >= 60) == treble).collect();
        notes.sort_unstable();
        notes.dedup();
        if notes.is_empty() {
            continue;
        }
        evs.push(Ev { t: (h.t * 4.0).round() as u32, dur: ((h.dur * 4.0).round() as u32).max(1), notes });
    }
    if evs.is_empty() {
        return None;
    }
    evs.sort_by_key(|e| e.t);
    let bar16 = spec.beats_per_bar * 4;
    let mut out = String::new();
    for bar in 0..spec.bars {
        let (base, end) = (bar * bar16, (bar + 1) * bar16);
        let mut here: Vec<&Ev> = evs.iter().filter(|e| e.t >= base && e.t < end).collect();
        here.dedup_by_key(|e| e.t);
        let mut state: HashMap<(usize, i32), i8> = HashMap::new();
        let mut cur = base;
        for (i, e) in here.iter().enumerate() {
            if e.t > cur {
                push_rests(&mut out, e.t - cur);
                cur = e.t;
            }
            let next = here.get(i + 1).map_or(end, |n| n.t);
            let len = e.dur.min(next - e.t).max(1);
            let w = WRITABLE.iter().copied().find(|&w| w <= len).unwrap_or(1);
            let names: Vec<String> = e.notes.iter().map(|&m| abc_note(m, spec.key_sig, &mut state)).collect();
            if names.len() == 1 {
                out.push_str(&format!("{}{w} ", names[0]));
            } else {
                out.push_str(&format!("[{}]{w} ", names.concat()));
            }
            cur += w;
        }
        if cur < end {
            push_rests(&mut out, end - cur);
        }
        out.push_str(if bar + 1 == spec.bars { "|]" } else { "| " });
    }
    Some(out)
}

/// The track as ABC text, or `None` when it plays no notes.
pub fn abc(spec: &ScoreSpec) -> Option<String> {
    let (hi, lo) = if spec.bass { (None, voice(spec, false)) } else { (voice(spec, true), voice(spec, false)) };
    if hi.is_none() && lo.is_none() {
        return None;
    }
    let idx = (spec.key_sig.clamp(-7, 7) + 7) as usize;
    let key = if spec.minor { format!("{}m", MINOR_KEYS[idx]) } else { MAJOR_KEYS[idx].to_string() };
    let mut s = format!("X:1\nT:\nM:{}/4\nL:1/16\nQ:1/4={}\nK:{key}\n", spec.beats_per_bar, spec.bpm.round() as i32);
    if hi.is_some() && lo.is_some() {
        s.push_str("%%score {1 2}\n");
    }
    let mut n = 0;
    for (v, clef) in [(hi, "treble"), (lo, "bass")] {
        if let Some(v) = v {
            n += 1;
            s.push_str(&format!("V:{n} clef={clef}\n{v}\n"));
        }
    }
    Some(s)
}

/// A rendered score: premultiplied RGBA pixels (twice the size in points) and where the measures
/// are, in points from the left edge of the image.
#[cfg(feature = "ui")]
pub struct Rendered {
    pub rgba: Vec<u8>,
    pub px_w: usize,
    pub px_h: usize,
    /// Start of the first measure, then the end of every measure.
    pub bar_x: Vec<f32>,
}

#[cfg(feature = "ui")]
pub fn render(abc: &str, width: f32, staff_size: f32, color: &str, bars: u32) -> Result<Rendered, String> {
    use acorde_render_svg::{render_svg_with_config, SvgRenderConfig, SvgRenderOptions};
    let score = acorde_io::parse_abc(abc).map_err(|e| e.to_string())?;
    let opts = SvgRenderOptions { width, staff_size, measures_per_system: bars.max(1) as usize, interactive: false };
    let cfg = SvgRenderConfig::new(opts).with_staff_gap(1.5);
    let svg = render_svg_with_config(&score, &cfg).map_err(|e| e.to_string())?.replace("\"black\"", &format!("\"{color}\""));

    // measure edges, read from the drawing
    let mut bars_x: Vec<f32> = Vec::new();
    for part in svg.split("class=\"acorde-barline\"").skip(1) {
        if let Some(x) = part.split("x1=\"").nth(1).and_then(|r| r.split('"').next()).and_then(|v| v.parse::<f32>().ok()) {
            if !bars_x.iter().any(|b| (b - x).abs() < 0.5) {
                bars_x.push(x);
            }
        }
    }
    bars_x.sort_by(|a, b| a.total_cmp(b));
    let header_end = svg
        .split("class=\"acorde-time-sig\"")
        .nth(1)
        .and_then(|g| g.split("</g>").next())
        .map(|g| {
            let mut max = 0.0f32;
            for d in g.split(" d=\"").skip(1) {
                let d = d.split('"').next().unwrap_or("");
                let nums: Vec<f32> = d.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).filter_map(|t| t.parse().ok()).collect();
                for x in nums.iter().step_by(2) {
                    max = max.max(*x);
                }
            }
            max + staff_size * 1.2
        })
        .unwrap_or(0.0);

    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    let (w, h) = (size.width() * 2, size.height() * 2);
    let mut pm = resvg::tiny_skia::Pixmap::new(w, h).ok_or("empty image")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(2.0, 2.0), &mut pm.as_mut());
    // trim the empty rows above and below the staves
    let data = pm.data();
    let row_has = |y: usize| data[y * w as usize * 4..(y + 1) * w as usize * 4].chunks(4).any(|p| p[3] > 8);
    let first = (0..h as usize).find(|&y| row_has(y)).unwrap_or(0).saturating_sub(6);
    let last = (0..h as usize).rev().find(|&y| row_has(y)).map_or(h as usize, |y| (y + 7).min(h as usize));
    let rgba = data[first * w as usize * 4..last * w as usize * 4].to_vec();

    let mut bar_x = vec![header_end];
    bar_x.extend(bars_x.iter().copied().filter(|&x| x > header_end + 1.0));
    if bar_x.len() != bars as usize + 1 {
        // could not read the measures: spread them evenly
        bar_x = (0..=bars).map(|k| header_end + (width - 8.0 - header_end) * k as f32 / bars as f32).collect();
    }
    Ok(Rendered { rgba, px_w: w as usize, px_h: last - first, bar_x })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(t: f32, dur: f32, notes: &[u8]) -> NoteHit {
        NoteHit { t, dur, notes: notes.to_vec(), chord: None }
    }

    fn spec(hits: &[NoteHit], key_sig: i8, minor: bool, bass: bool) -> ScoreSpec<'_> {
        ScoreSpec { hits, key_sig, minor, bpm: 100.0, bars: 2, beats_per_bar: 4, bass }
    }

    #[test]
    fn writes_notes_chords_rests_key_meter_and_tempo() {
        let hits = [hit(0.0, 1.0, &[57, 61, 64]), hit(1.0, 0.5, &[33]), hit(4.0, 4.0, &[45])];
        let s = abc(&spec(&hits, 3, false, false)).unwrap();
        assert!(s.contains("M:4/4") && s.contains("L:1/16") && s.contains("Q:1/4=100") && s.contains("K:A\n"), "{s}");
        assert!(s.contains("%%score {1 2}"), "treble and bass staves: {s}");
        // A3 C#4 E4 -> the C#4 and E4 on the treble staff, A3 on the bass one; the key signature gives the C#
        assert!(s.contains("V:1 clef=treble\n[CE]4 "), "{s}");
        assert!(s.contains("V:2 clef=bass\n"), "{s}");
        assert!(s.contains("A,,,2"), "an eighth A1 on the bass staff: {s}");
        let minor = abc(&spec(&hits, 0, true, false)).unwrap();
        assert!(minor.contains("K:Am"), "{minor}");
        let flats = abc(&spec(&hits, -2, false, false)).unwrap();
        assert!(flats.contains("K:Bb"), "{flats}");
        assert!(abc(&spec(&[], 0, false, false)).is_none());
    }

    #[test]
    fn accidentals_follow_the_key_and_the_bar() {
        // in C major F# is marked, and a following F natural in the same bar needs its natural
        let hits = [hit(0.0, 1.0, &[66]), hit(1.0, 1.0, &[65]), hit(2.0, 1.0, &[65])];
        let s = abc(&spec(&hits, 0, false, false)).unwrap();
        assert!(s.contains("^F4 =F4 F4"), "{s}");
        // in G major (F#) the same F# needs nothing and an F natural is marked
        let g = abc(&spec(&hits, 1, false, false)).unwrap();
        assert!(g.contains("F4 =F4 F4"), "{g}");
        // a flat key spells with flats
        let hits = [hit(0.0, 1.0, &[70])];
        assert!(abc(&spec(&hits, -1, false, false)).unwrap().contains("B4"), "Bb is in the key of F");
    }

    #[test]
    fn lengths_are_writable_and_bars_are_filled() {
        let hits = [hit(0.0, 1.25, &[60]), hit(2.0, 3.0, &[62])];
        let s = abc(&spec(&hits, 0, false, false)).unwrap();
        // 5 sixteenths cannot be one note: a quarter, then the left-over sixteenth joins the gap to the next note
        assert!(s.contains("C4 z4 D8 |"), "{s}");
        // the second note is cut at the bar line (2.0 + 3.0 beats runs to beat 5, bar 1 ends at 4)
        let bars: Vec<&str> = s.lines().last().unwrap().split('|').collect();
        assert!(bars.len() >= 2, "{s}");
        // a bass part is written an octave up, in the bass clef only
        let b = abc(&spec(&[hit(0.0, 1.0, &[33])], 0, false, true)).unwrap();
        assert!(b.contains("V:1 clef=bass") && !b.contains("treble") && b.contains("A,,4"), "A1 sounds, A2 is written: {b}");
    }

    #[cfg(feature = "ui")]
    #[test]
    fn it_engraves_an_image_with_measure_edges() {
        let hits = [hit(0.0, 1.0, &[57, 61, 64]), hit(4.0, 2.0, &[50])];
        let s = abc(&spec(&hits, 3, false, false)).unwrap();
        let r = render(&s, 800.0, 5.0, "#222222", 2).unwrap();
        assert!(r.px_w == 1600 && r.px_h > 100 && r.px_h < 600, "{}x{}", r.px_w, r.px_h);
        assert!(r.rgba.chunks(4).filter(|p| p[3] > 128).count() > 2000, "something is drawn");
        assert_eq!(r.bar_x.len(), 3);
        assert!(r.bar_x[0] > 20.0 && r.bar_x[0] < r.bar_x[1] && r.bar_x[1] < r.bar_x[2], "{:?}", r.bar_x);
    }
}
