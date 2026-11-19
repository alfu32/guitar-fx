//! Where settings, saved chains and user impulse responses live.

use std::path::PathBuf;

fn base() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
    }
}

pub fn config_dir() -> PathBuf {
    let base = base();
    let dir = base.join("guitar-fx");
    // Earlier names of this program; carry the old settings over.
    for old in ["guitarfx", "fxchain"] {
        let old = base.join(old);
        if !dir.exists() && old.exists() {
            let _ = std::fs::rename(&old, &dir);
        }
    }
    dir
}

pub fn chains_dir() -> PathBuf {
    config_dir().join("chains")
}

/// Drop cabinet impulse responses (`.wav`) here; the Cabinet effect finds them by name.
pub fn ir_dir() -> PathBuf {
    config_dir().join("ir")
}

/// File names (without folder) of the impulse responses available in `ir_dir`.
pub fn ir_files() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(ir_dir())
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.to_lowercase().ends_with(".wav"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Drop Neural Amp Modeler captures (`.nam`) here, in folders if you like; the Neural Amp
/// effect finds them by their path inside this folder.
pub fn nam_dir() -> PathBuf {
    config_dir().join("nam")
}

/// Paths (relative to `nam_dir`, with `/`) of the `.nam` files below it.
pub fn nam_files() -> Vec<String> {
    fn walk(dir: &std::path::Path, rel: &str, depth: usize, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.filter_map(|e| e.ok()) {
            let Ok(name) = e.file_name().into_string() else { continue };
            let path = e.path();
            let here = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if path.is_dir() {
                if depth < 4 {
                    walk(&path, &here, depth + 1, out);
                }
            } else if name.to_lowercase().ends_with(".nam") {
                out.push(here);
            }
        }
    }
    let mut v = Vec::new();
    walk(&nam_dir(), "", 0, &mut v);
    v.sort();
    v
}
