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
