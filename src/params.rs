//! Parsing of `key=value,key=value` effect parameter lists.
//!
//! A bare number (`--overdrive 0.7`) sets the effect's primary parameter.

use std::collections::HashMap;

pub struct Params {
    effect: &'static str,
    map: HashMap<String, String>,
}

impl Params {
    pub fn parse(
        effect: &'static str,
        raw: &str,
        primary: &str,
        allowed: &[&str],
    ) -> Result<Self, String> {
        let mut map = HashMap::new();
        for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = match part.split_once('=') {
                Some((k, v)) => (k.trim().to_lowercase().replace('_', "-"), v.trim().to_string()),
                None => (primary.to_string(), part.to_string()),
            };
            if !allowed.contains(&key.as_str()) {
                return Err(format!(
                    "--{effect}: unknown parameter '{key}' (valid: {})",
                    allowed.join(", ")
                ));
            }
            map.insert(key, value);
        }
        Ok(Self { effect, map })
    }

    /// Numeric parameter, falling back to `default`; errors when outside `min..=max`.
    pub fn num(&self, key: &str, default: f32, min: f32, max: f32) -> Result<f32, String> {
        match self.map.get(key) {
            None => Ok(default),
            Some(v) => {
                let n: f32 = v
                    .parse()
                    .map_err(|_| format!("--{}: {key}='{v}' is not a number", self.effect))?;
                if !(min..=max).contains(&n) {
                    return Err(format!(
                        "--{}: {key}={n} is out of range ({min}..{max})",
                        self.effect
                    ));
                }
                Ok(n)
            }
        }
    }

    pub fn text(&self, key: &str, default: &str) -> String {
        self.map.get(key).cloned().unwrap_or_else(|| default.to_string())
    }
}
