//! Preset files: one `name [value]` per line, `#` starts a comment. Names are the effect
//! names plus `input-gain`, `output-gain` and `buffer`. Effects keep file order.

use crate::params::Spec;

#[derive(Default)]
pub struct Preset {
    pub specs: Vec<Spec>,
    pub input_db: f32,
    pub output_db: f32,
    pub buffer: Option<u32>,
}

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
            let at = |e: String| format!("line {}: {e}", n + 1);
            match name {
                "input-gain" => p.input_db = parse_db(value, name).map_err(at)?,
                "output-gain" => p.output_db = parse_db(value, name).map_err(at)?,
                "buffer" => p.buffer = Some(value.parse().map_err(|_| at("buffer: not a number".into()))?),
                _ => p.specs.push(Spec::parse(name, value).map_err(at)?),
            }
        }
        Ok(p)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::from("# fxchain preset — effects run top to bottom\n");
        if self.input_db != 0.0 {
            out += &format!("input-gain {}\n", self.input_db);
        }
        for s in &self.specs {
            out += &format!("{} {}\n", s.def.name, s.raw()).replace(" \n", "\n");
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
