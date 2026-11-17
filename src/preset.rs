//! Preset files: one `name [value]` per line, `#` starts a comment. Names are the effect
//! names plus `input-gain`, `output-gain` and `buffer`. Effects keep file order.

use crate::params::Spec;

#[derive(Default)]
pub struct Preset {
    pub specs: Vec<Spec>,
    /// Parallel to `specs`: true for effects saved switched off (`!name` in the file).
    pub bypass: Vec<bool>,
    pub input_db: f32,
    pub output_db: f32,
    pub buffer: Option<u32>,
}

/// Chains shipped with the program: (display name, preset text).
pub const BUILTIN: &[(&str, &str)] = &[
    ("Guitar clean", include_str!("../presets/builtin/guitar-clean.fx")),
    ("Guitar crunch", include_str!("../presets/builtin/guitar-crunch.fx")),
    ("Clean sparkle", include_str!("../presets/builtin/clean-sparkle.fx")),
    ("Crunch rhythm", include_str!("../presets/builtin/crunch-rhythm.fx")),
    ("Lead solo", include_str!("../presets/builtin/lead-solo.fx")),
    ("Ambient wash", include_str!("../presets/builtin/ambient-wash.fx")),
    ("Funk wah", include_str!("../presets/builtin/funk-wah.fx")),
    ("Surf tremolo", include_str!("../presets/builtin/surf-tremolo.fx")),
    ("Jet phase", include_str!("../presets/builtin/jet-phase.fx")),
    ("Octave thick", include_str!("../presets/builtin/octave-thick.fx")),
    ("Robot voice", include_str!("../presets/builtin/robot-voice.fx")),
    ("Autotune pop", include_str!("../presets/builtin/autotune-pop.fx")),
];

pub fn parse_db(v: &str, what: &str) -> Result<f32, String> {
    let n: f32 = v
        .trim()
        .trim_end_matches("dB")
        .trim_end_matches("db")
        .trim()
        .parse()
        .map_err(|_| format!("{what}: '{v}' is not a number (dB)"))?;
    if !(-60.0..=40.0).contains(&n) {
        return Err(format!("{what}: {n} dB is out of range (-60..40)"));
    }
    Ok(n)
}

impl Preset {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut p = Preset::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let (name, value) = (name.trim_start_matches("--"), value.trim());
            let (off, name) = match name.strip_prefix('!') {
                Some(n) => (true, n),
                None => (false, name),
            };
            let at = |e: String| format!("line {}: {e}", n + 1);
            match name {
                "input-gain" => p.input_db = parse_db(value, name).map_err(at)?,
                "output-gain" => p.output_db = parse_db(value, name).map_err(at)?,
                "buffer" => p.buffer = Some(value.parse().map_err(|_| at("buffer: not a number".into()))?),
                _ => {
                    p.specs.push(Spec::parse(name, value).map_err(at)?);
                    p.bypass.push(off);
                }
            }
        }
        Ok(p)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# fxchain preset — effects run top to bottom\n");
        if let Some(b) = self.buffer {
            out += &format!("buffer {b}\n");
        }
        if self.input_db != 0.0 {
            out += &format!("input-gain {}\n", self.input_db);
        }
        for (i, s) in self.specs.iter().enumerate() {
            let off = if self.bypass.get(i).copied().unwrap_or(false) { "!" } else { "" };
            out += &format!("{off}{} {}\n", s.def.name, s.raw()).replace(" \n", "\n");
        }
        if self.output_db != 0.0 {
            out += &format!("output-gain {}\n", self.output_db);
        }
        out
    }

    pub fn load(path: &str) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        Self::parse(&text).map_err(|e| format!("{path}: {e}"))
    }

    pub fn save(&self, path: &str) -> Result<(), String> {
        std::fs::write(path, self.to_text()).map_err(|e| format!("{path}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_chains_parse_and_build() {
        for (name, text) in BUILTIN {
            let p = Preset::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(!p.specs.is_empty(), "{name}");
            for s in &p.specs {
                crate::effects::build(s, 48000.0);
            }
        }
    }

    #[test]
    fn bypass_marker_round_trips() {
        let p = Preset::parse("compressor ratio=3\n!reverb mix=0.2\n").unwrap();
        assert_eq!(p.bypass, vec![false, true]);
        let q = Preset::parse(&p.to_text()).unwrap();
        assert_eq!(q.bypass, vec![false, true]);
        assert_eq!(q.specs.len(), 2);
    }
}
