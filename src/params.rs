//! Typed effect parameters: one static schema drives the CLI parser, presets and the UI.

/// A parameter value. `Auto` is only valid for `Kind::AutoNum` (e.g. compressor makeup).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Num(f32),
    Choice(usize),
    Auto,
}

pub enum Kind {
    Num { min: f32, max: f32, default: f32, unit: &'static str },
    Choice { options: &'static [&'static str], default: usize },
    /// A number, or `auto` (the default).
    AutoNum { min: f32, max: f32, unit: &'static str },
}

pub struct ParamDef {
    pub key: &'static str,
    pub kind: Kind,
    /// Can be changed on a running effect without rebuilding it (keeps its state).
    pub live: bool,
}

pub struct EffectDef {
    pub name: &'static str,
    pub title: &'static str,
    /// The parameter a bare number sets (`--overdrive 0.7`).
    pub primary: &'static str,
    pub params: &'static [ParamDef],
}

const fn num(key: &'static str, default: f32, min: f32, max: f32, unit: &'static str, live: bool) -> ParamDef {
    ParamDef { key, kind: Kind::Num { min, max, default, unit }, live }
}

const fn choice(key: &'static str, options: &'static [&'static str], default: usize, live: bool) -> ParamDef {
    ParamDef { key, kind: Kind::Choice { options, default }, live }
}

pub const NOTES: &[&str] = &["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub const SCALES: &[&str] =
    &["chromatic", "major", "minor", "pentatonic", "minor-pentatonic", "blues"];
const MODES: &[&str] = &["high", "fast"];
const NOISE_MODES: &[&str] = &["burst", "follow"];
const SHAPES: &[&str] = &["sine", "square"];

pub const EFFECTS: &[EffectDef] = &[
    EffectDef {
        name: "overdrive",
        title: "Overdrive",
        primary: "drive",
        params: &[
            num("drive", 0.5, 0.0, 1.0, "", true),
            num("tone", 0.6, 0.0, 1.0, "", true),
            num("level", 1.0, 0.0, 2.0, "", true),
        ],
    },
    EffectDef {
        name: "compressor",
        title: "Compressor",
        primary: "ratio",
        params: &[
            num("ratio", 4.0, 1.0, 20.0, ":1", true),
            num("threshold", -18.0, -60.0, 0.0, "dB", true),
            num("attack", 10.0, 0.1, 200.0, "ms", true),
            num("release", 120.0, 5.0, 2000.0, "ms", true),
            ParamDef { key: "makeup", kind: Kind::AutoNum { min: -12.0, max: 24.0, unit: "dB" }, live: true },
        ],
    },
    EffectDef {
        name: "echo",
        title: "Echo",
        primary: "mix",
        params: &[
            num("mix", 0.4, 0.0, 1.0, "", true),
            num("time", 350.0, 1.0, 2000.0, "ms", true),
            num("feedback", 0.45, 0.0, 0.95, "", true),
            num("tone", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "delay",
        title: "Delay",
        primary: "time",
        params: &[
            num("time", 300.0, 1.0, 2000.0, "ms", true),
            num("feedback", 0.3, 0.0, 0.95, "", true),
            num("mix", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "chorus",
        title: "Chorus",
        primary: "depth",
        params: &[
            num("depth", 0.5, 0.0, 1.0, "", true),
            num("rate", 0.6, 0.05, 10.0, "Hz", true),
            num("mix", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "flanger",
        title: "Flanger",
        primary: "depth",
        params: &[
            num("depth", 0.7, 0.0, 1.0, "", true),
            num("rate", 0.3, 0.02, 5.0, "Hz", true),
            num("feedback", 0.5, -0.95, 0.95, "", true),
            num("mix", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "phaser",
        title: "Phaser",
        primary: "depth",
        params: &[
            num("depth", 0.7, 0.0, 1.0, "", true),
            num("rate", 0.5, 0.02, 5.0, "Hz", true),
            num("feedback", 0.5, -0.95, 0.95, "", true),
            num("mix", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "tremolo",
        title: "Tremolo",
        primary: "depth",
        params: &[num("depth", 0.5, 0.0, 1.0, "", true), num("rate", 5.0, 0.1, 20.0, "Hz", true)],
    },
    EffectDef {
        name: "wah",
        title: "Auto-wah",
        primary: "sensitivity",
        params: &[
            num("sensitivity", 0.6, 0.0, 1.0, "", true),
            num("rate", 0.0, 0.0, 10.0, "Hz", true),
            num("q", 5.0, 1.0, 15.0, "", true),
            num("min", 350.0, 100.0, 2000.0, "Hz", true),
            num("max", 2200.0, 500.0, 6000.0, "Hz", true),
            num("mix", 1.0, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "harmonics",
        title: "Harmonics",
        primary: "mix",
        params: &[
            num("mix", 0.3, 0.0, 1.0, "", true),
            num("drive", 0.5, 0.0, 1.0, "", true),
            num("freq", 2000.0, 200.0, 12000.0, "Hz", true),
        ],
    },
    EffectDef {
        name: "reverb",
        title: "Reverb",
        primary: "mix",
        params: &[
            num("mix", 0.3, 0.0, 1.0, "", true),
            num("time", 2.0, 0.2, 10.0, "s", false),
            num("size", 10.0, 3.0, 30.0, "m", false),
            num("damping", 0.5, 0.0, 1.0, "", false),
        ],
    },
    EffectDef {
        name: "tone-shift",
        title: "Pitch shift",
        primary: "semitones",
        params: &[
            num("semitones", 0.0, -24.0, 24.0, "st", true),
            num("cents", 0.0, -100.0, 100.0, "ct", true),
            num("mix", 1.0, 0.0, 1.0, "", true),
            choice("mode", MODES, 0, false),
            num("window", 10.0, 4.0, 40.0, "ms", false),
        ],
    },
    EffectDef {
        name: "autotune",
        title: "Autotune",
        primary: "amount",
        params: &[
            choice("key", NOTES, 0, true),
            choice("scale", SCALES, 0, true),
            num("speed", 40.0, 0.0, 500.0, "ms", true),
            num("amount", 1.0, 0.0, 1.0, "", true),
            choice("mode", MODES, 0, false),
            num("window", 10.0, 4.0, 40.0, "ms", false),
        ],
    },
    EffectDef {
        name: "vocoder",
        title: "Vocoder",
        primary: "mix",
        params: &[
            num("mix", 1.0, 0.0, 1.0, "", true),
            num("bands", 16.0, 4.0, 32.0, "", false),
            num("carrier", 110.0, 30.0, 880.0, "Hz", false),
            num("release", 15.0, 1.0, 200.0, "ms", true),
        ],
    },
    EffectDef {
        name: "transient",
        title: "Transient shaper",
        primary: "attack",
        params: &[
            num("attack", -0.6, -1.0, 1.0, "", true),
            num("time", 15.0, 2.0, 60.0, "ms", true),
            num("mix", 1.0, 0.0, 1.0, "", true),
        ],
    },
    // ---- experimental
    EffectDef {
        name: "freeze",
        title: "Freeze",
        primary: "mix",
        params: &[
            num("mix", 0.7, 0.0, 1.0, "", true),
            num("sustain", 8.0, 0.2, 60.0, "s", true),
            num("length", 50.0, 10.0, 200.0, "ms", true),
            num("settle", 60.0, 20.0, 400.0, "ms", true),
            num("sensitivity", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "pick-noise",
        title: "Pick noise",
        primary: "amount",
        params: &[
            num("amount", 0.3, 0.0, 1.0, "", true),
            num("decay", 60.0, 5.0, 800.0, "ms", true),
            num("tone", 1.0, 0.0, 1.0, "", true),
            choice("mode", NOISE_MODES, 0, true),
            num("sensitivity", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "bitcrusher",
        title: "Bitcrusher",
        primary: "bits",
        params: &[
            num("bits", 8.0, 1.0, 16.0, "", true),
            num("rate", 12000.0, 500.0, 48000.0, "Hz", true),
            num("mix", 1.0, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "ringmod",
        title: "Ring mod",
        primary: "freq",
        params: &[
            num("freq", 150.0, 20.0, 2000.0, "Hz", true),
            num("mix", 0.5, 0.0, 1.0, "", true),
            choice("shape", SHAPES, 0, true),
        ],
    },
    EffectDef {
        name: "reverse",
        title: "Reverse",
        primary: "mix",
        params: &[num("mix", 0.7, 0.0, 1.0, "", true), num("time", 400.0, 50.0, 2000.0, "ms", true)],
    },
    // ---- zen and ethereal
    EffectDef {
        name: "kalimba",
        title: "Kalimba",
        primary: "mix",
        params: &[
            num("mix", 0.6, 0.0, 1.0, "", true),
            num("octave", 1.0, 0.0, 2.0, "", true),
            num("decay", 900.0, 100.0, 4000.0, "ms", true),
            num("dry", 1.0, 0.0, 1.0, "", true),
            num("sensitivity", 0.5, 0.0, 1.0, "", true),
        ],
    },
    EffectDef {
        name: "wind",
        title: "Wind",
        primary: "mix",
        params: &[
            num("mix", 0.5, 0.0, 1.0, "", true),
            num("level", 0.3, 0.0, 1.0, "", true),
            num("follow", 0.7, 0.0, 1.0, "", true),
            num("rate", 0.15, 0.02, 2.0, "Hz", true),
            num("q", 5.0, 1.0, 20.0, "", true),
        ],
    },
    EffectDef {
        name: "swell",
        title: "Swell",
        primary: "rise",
        params: &[num("rise", 400.0, 50.0, 3000.0, "ms", true), num("sensitivity", 0.5, 0.0, 1.0, "", true)],
    },
    EffectDef {
        name: "shimmer",
        title: "Shimmer",
        primary: "mix",
        params: &[
            num("mix", 0.4, 0.0, 1.0, "", true),
            num("shimmer", 0.5, 0.0, 1.0, "", true),
            num("pitch", 12.0, -12.0, 24.0, "st", true),
            num("time", 4.0, 0.5, 10.0, "s", false),
            num("size", 15.0, 3.0, 30.0, "m", false),
        ],
    },
];

/// Effects shown under "Experimental" in the UI.
pub const EXPERIMENTAL: &[&str] = &["freeze", "pick-noise", "bitcrusher", "ringmod", "reverse"];
/// Effects shown under "Zen & ethereal" in the UI.
pub const ZEN: &[&str] = &["kalimba", "wind", "swell", "shimmer"];

pub fn effect_def(name: &str) -> Option<&'static EffectDef> {
    EFFECTS.iter().find(|e| e.name == name)
}

impl ParamDef {
    pub fn default_value(&self) -> Value {
        match self.kind {
            Kind::Num { default, .. } => Value::Num(default),
            Kind::Choice { default, .. } => Value::Choice(default),
            Kind::AutoNum { .. } => Value::Auto,
        }
    }
}

/// One effect instance's settings (typed, validated against the schema).
#[derive(Clone)]
pub struct Spec {
    pub def: &'static EffectDef,
    pub values: Vec<Value>,
}

impl Spec {
    pub fn new(def: &'static EffectDef) -> Self {
        Self { def, values: def.params.iter().map(ParamDef::default_value).collect() }
    }

    fn index(&self, key: &str) -> usize {
        self.def
            .params
            .iter()
            .position(|p| p.key == key)
            .unwrap_or_else(|| panic!("effect '{}' has no parameter '{key}'", self.def.name))
    }

    pub fn get(&self, key: &str) -> Value {
        self.values[self.index(key)]
    }

    pub fn num(&self, key: &str) -> f32 {
        match self.get(key) {
            Value::Num(n) => n,
            other => panic!("parameter '{key}' is {other:?}, not a number"),
        }
    }

    pub fn choice(&self, key: &str) -> usize {
        match self.get(key) {
            Value::Choice(i) => i,
            other => panic!("parameter '{key}' is {other:?}, not a choice"),
        }
    }

    pub fn option(&self, key: &str) -> &'static str {
        match &self.def.params[self.index(key)].kind {
            Kind::Choice { options, .. } => options[self.choice(key)],
            _ => panic!("parameter '{key}' is not a choice"),
        }
    }

    pub fn set(&mut self, key: &str, value: Value) {
        let i = self.index(key);
        self.values[i] = value;
    }

    /// Parse `name=value,name=value`; a bare value sets the primary parameter.
    pub fn parse(name: &str, raw: &str) -> Result<Self, String> {
        let def = effect_def(name).ok_or_else(|| format!("unknown effect '{name}'"))?;
        let mut spec = Spec::new(def);
        let valid = || def.params.iter().map(|p| p.key).collect::<Vec<_>>().join(", ");
        for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, text) = match part.split_once('=') {
                Some((k, v)) => (k.trim().to_lowercase().replace('_', "-"), v.trim()),
                None => (def.primary.to_string(), part),
            };
            let p = def
                .params
                .iter()
                .find(|p| p.key == key)
                .ok_or_else(|| format!("--{name}: unknown parameter '{key}' (valid: {})", valid()))?;
            let number = |min: f32, max: f32| -> Result<f32, String> {
                let n: f32 = text.parse().map_err(|_| format!("--{name}: {key}='{text}' is not a number"))?;
                if (min..=max).contains(&n) {
                    Ok(n)
                } else {
                    Err(format!("--{name}: {key}={n} is out of range ({min}..{max})"))
                }
            };
            let value = match &p.kind {
                Kind::Num { min, max, .. } => Value::Num(number(*min, *max)?),
                Kind::AutoNum { min, max, .. } => {
                    if text.eq_ignore_ascii_case("auto") {
                        Value::Auto
                    } else {
                        Value::Num(number(*min, *max)?)
                    }
                }
                Kind::Choice { options, .. } => {
                    let wanted = normalize_choice(&key, text);
                    let i = options.iter().position(|o| o.eq_ignore_ascii_case(&wanted)).ok_or_else(|| {
                        format!("--{name}: {key}='{text}' is not valid (use {})", options.join(", "))
                    })?;
                    Value::Choice(i)
                }
            };
            spec.set(&key, value);
        }
        Ok(spec)
    }

    /// The non-default settings as `key=value,...` (empty when everything is default).
    pub fn raw(&self) -> String {
        let mut parts = Vec::new();
        for (p, v) in self.def.params.iter().zip(&self.values) {
            if *v == p.default_value() {
                continue;
            }
            let text = match (&p.kind, v) {
                (_, Value::Num(n)) => format!("{n}"),
                (Kind::Choice { options, .. }, Value::Choice(i)) => options[*i].to_string(),
                _ => "auto".to_string(),
            };
            parts.push(format!("{}={}", p.key, text));
        }
        parts.join(",")
    }
}

/// Accept flats and Unicode accidentals for note names (Db -> C#).
fn normalize_choice(key: &str, text: &str) -> String {
    if key != "key" {
        return text.to_string();
    }
    let t = text.trim().replace('♯', "#").replace('♭', "b");
    let lower = t.to_lowercase();
    let sharp = match lower.as_str() {
        "db" => "C#",
        "eb" => "D#",
        "gb" => "F#",
        "ab" => "G#",
        "bb" => "A#",
        _ => return t,
    };
    sharp.to_string()
}
