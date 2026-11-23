//! Graphical front end. Every control talks to the running audio chain through
//! `guitar_fx_core::chain::Control`, so changes are heard within one audio buffer.

#[path = "guitar-fx-ui/theme.rs"]
mod theme;

use eframe::egui;
use guitar_fx_core::notation;
use guitar_fx_core::band::{self, BandCmd, BandEvents, BandSetup, BandSettings, DrumGrid, Snap, Source, Track, CHORD_TYPES, RHYTHMS, TRACKS};
use guitar_fx_core::chain::{Cmd, Slot};
use guitar_fx_core::tools::{LoopCmd, LoopState};
use guitar_fx_core::effects::build;
use guitar_fx_core::listen;
use guitar_fx_core::live::{self, LiveOpts, Running, DEFAULT_BUFFER};
use guitar_fx_core::params::{EffectDef, Kind, Spec, Value, EFFECTS, EXPERIMENTAL, ZEN};
use guitar_fx_core::preset::{Preset, BUILTIN};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const BUFFERS: [u32; 6] = [32, 64, 128, 256, 512, 1024];

const HISTORY: usize = 72;
/// Height of the always-present drop slot between cards; constant so nothing shifts while dragging.
const GAP: f32 = 18.0;

use guitar_fx_core::paths::{self, config_dir};

struct Fx {
    id: u64,
    spec: Spec,
    bypass: bool,
    /// Recent output levels of this effect, newest last (0..1 on a dB scale).
    hist: VecDeque<f32>,
}

impl Fx {
    fn new(id: u64, spec: Spec, bypass: bool) -> Self {
        Fx { id, spec, bypass, hist: VecDeque::new() }
    }
}

enum Act {
    Param(usize, usize),
    Bypass(usize),
    Remove(usize),
    MoveTo(usize, usize),
    Add(&'static EffectDef),
}

#[derive(Clone, Copy, PartialEq)]
enum BandView {
    Wave,
    Score,
    Letters,
}

/// An engraved score of one track, as a texture.
struct ScoreTex {
    key: u64,
    tex: egui::TextureHandle,
    /// Size in points, as engraved, and the width it was engraved for.
    size: egui::Vec2,
    width: f32,
    bar_x: Vec<f32>,
}

struct App {
    chain: Vec<Fx>,
    next_id: u64,
    running: Option<Running>,
    error: Option<String>,
    in_devices: Vec<String>,
    out_devices: Vec<String>,
    in_sel: Option<usize>,
    out_sel: Option<usize>,
    buffer: u32,
    input_db: f32,
    output_db: f32,
    chain_name: String,
    user_chains: Vec<String>,
    user_bands: Vec<String>,
    band_name: String,
    band_view: BandView,
    score_cache: std::collections::HashMap<u32, ScoreTex>,
    /// The drum loop and roots last seen in the running band (kept across Stop/Start and saved).
    band_cache: Option<(DrumGrid, [Vec<(f32, u8)>; 5])>,
    status: String,
    shown_in: f32,
    shown_out: f32,
    /// The reserved drop slots (one before every card and one after the last), from the last frame.
    gap_rects: Vec<egui::Rect>,
    dragging: Option<usize>,
    last_hist: Instant,
    metro_on: bool,
    bpm: f32,
    beats: u32,
    metro_vol: f32,
    taps: Vec<Instant>,
    last_beat: (u32, Instant, bool),
    tuner_on: bool,
    listen_on: bool,
    band_open: bool,
    band_pick: Option<(Track, f32, egui::Pos2)>,
    band: BandSettings,
    tuner_mute: bool,
    tuner_hz: f32,
    tuner_seen: Instant,
    loop_vol: f32,
    saved_session: String,
    last_save: Instant,
}

fn level_frac(peak: f32) -> f32 {
    ((20.0 * peak.max(1e-6).log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

impl App {
    fn new() -> Self {
        let (in_devices, out_devices) = live::device_names().unwrap_or_default();
        let mut app = App {
            chain: Vec::new(),
            next_id: 0,
            running: None,
            error: None,
            in_devices,
            out_devices,
            in_sel: None,
            out_sel: None,
            buffer: DEFAULT_BUFFER,
            input_db: 0.0,
            output_db: 0.0,
            chain_name: "my-chain".into(),
            user_chains: Vec::new(),
            user_bands: Vec::new(),
            band_name: "my-band".into(),
            band_view: BandView::Wave,
            score_cache: Default::default(),
            band_cache: None,
            status: String::new(),
            shown_in: 0.0,
            shown_out: 0.0,
            gap_rects: Vec::new(),
            dragging: None,
            last_hist: Instant::now(),
            metro_on: false,
            bpm: 100.0,
            beats: 4,
            metro_vol: 0.5,
            taps: Vec::new(),
            last_beat: (0, Instant::now(), false),
            tuner_on: false,
            listen_on: false,
            band_open: false,
            band_pick: None,
            band: BandSettings::default(),
            tuner_mute: true,
            tuner_hz: 0.0,
            tuner_seen: Instant::now(),
            loop_vol: 1.0,
            saved_session: String::new(),
            last_save: Instant::now(),
        };
        app.scan_chains();
        app.scan_bands();
        if !app.restore_session() {
            for name in ["compressor", "overdrive", "reverb"] {
                app.add_local(guitar_fx_core::params::effect_def(name).unwrap());
            }
        }
        app.saved_session = app.session_text();
        app
    }

    fn chains_dir() -> PathBuf {
        paths::chains_dir()
    }

    fn bands_dir() -> PathBuf {
        paths::bands_dir()
    }

    fn scan_bands(&mut self) {
        self.user_bands = std::fs::read_dir(Self::bands_dir())
            .map(|d| {
                let mut v: Vec<String> = d
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter_map(|n| n.strip_suffix(".band").map(str::to_string))
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default();
    }

    /// The band as it is now: mixer, drum loop and roots.
    fn band_setup(&self) -> BandSetup {
        let (drums, roots) = self.band_cache.clone().unwrap_or_else(|| (band::drum_grid_of(self.band.rhythm), Default::default()));
        BandSetup { settings: BandSettings { on: false, ..self.band }, drums, roots }
    }

    /// Make the band match a setup (a running band at once; otherwise at the next Start).
    fn apply_band_setup(&mut self, mut st: BandSetup) {
        st.settings.on = self.band.on;
        self.band = st.settings;
        self.band_cache = Some((st.drums.clone(), st.roots.clone()));
        self.send_band_setup(&st);
        self.sync_listening();
    }

    fn send_band_setup(&self, st: &BandSetup) {
        for c in st.commands() {
            self.send(Cmd::Band(c));
        }
    }

    fn load_band(&mut self, name: &str) {
        match std::fs::read_to_string(Self::bands_dir().join(format!("{name}.band"))).map_err(|e| e.to_string()).and_then(|t| BandSetup::from_text(&t)) {
            Ok(st) => {
                self.apply_band_setup(st);
                self.band_name = name.to_string();
                self.error = None;
                self.status = format!("band setup {name} loaded");
            }
            Err(e) => self.error = Some(format!("{name}: {e}")),
        }
    }

    fn save_band(&mut self) {
        let dir = Self::bands_dir();
        let _ = std::fs::create_dir_all(&dir);
        let name: String = self.band_name.trim().chars().map(|c| if c.is_alphanumeric() || "-_ ".contains(c) { c } else { '-' }).collect();
        if name.is_empty() {
            self.error = Some("give the band a name first".into());
            return;
        }
        let path = dir.join(format!("{name}.band"));
        match std::fs::write(&path, self.band_setup().to_text()) {
            Ok(()) => {
                self.error = None;
                self.status = format!("saved {}", path.display());
                self.scan_bands();
            }
            Err(e) => self.error = Some(format!("{}: {e}", path.display())),
        }
    }

    fn new_band(&mut self) {
        let d = BandSettings::default();
        let st = BandSetup { settings: d, drums: band::drum_grid_of(d.rhythm), roots: Default::default() };
        self.apply_band_setup(st);
        self.band_name = "new-band".into();
    }

    fn scan_chains(&mut self) {
        self.user_chains = std::fs::read_dir(Self::chains_dir())
            .map(|d| {
                let mut v: Vec<String> = d
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter_map(|n| n.strip_suffix(".fx").map(str::to_string))
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default();
    }

    fn preset(&self) -> Preset {
        Preset {
            specs: self.chain.iter().map(|f| f.spec.clone()).collect(),
            bypass: self.chain.iter().map(|f| f.bypass).collect(),
            input_db: self.input_db,
            output_db: self.output_db,
            buffer: Some(self.buffer),
        }
    }

    /// Everything that is restored on the next launch, as text.
    fn session_text(&self) -> String {
        let name = |sel: Option<usize>, list: &[String]| sel.map(|i| list[i].clone()).unwrap_or_default();
        format!(
            "{}\n@in {}\n@out {}\n",
            self.preset().to_text(),
            name(self.in_sel, &self.in_devices),
            name(self.out_sel, &self.out_devices)
        )
    }

    fn save_session(&mut self) {
        let text = self.session_text();
        if text == self.saved_session {
            return;
        }
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        let (chain, devices) = text.split_once("\n@in ").unwrap_or((&text, ""));
        let _ = std::fs::write(dir.join("session.fx"), chain);
        let _ = std::fs::write(dir.join("devices.txt"), format!("@in {devices}"));
        self.saved_session = text;
        self.last_save = Instant::now();
    }

    fn restore_session(&mut self) -> bool {
        let dir = config_dir();
        let Ok(text) = std::fs::read_to_string(dir.join("session.fx")) else { return false };
        let Ok(p) = Preset::parse(&text) else { return false };
        if let Some(b) = p.buffer {
            self.buffer = b;
        }
        self.input_db = p.input_db;
        self.output_db = p.output_db;
        for (i, spec) in p.specs.into_iter().enumerate() {
            let id = self.next_id;
            self.next_id += 1;
            self.chain.push(Fx::new(id, spec, p.bypass.get(i).copied().unwrap_or(false)));
        }
        if let Ok(d) = std::fs::read_to_string(dir.join("devices.txt")) {
            for line in d.lines() {
                if let Some(n) = line.strip_prefix("@in ") {
                    self.in_sel = self.in_devices.iter().position(|d| d == n);
                } else if let Some(n) = line.strip_prefix("@out ") {
                    self.out_sel = self.out_devices.iter().position(|d| d == n);
                }
            }
        }
        true
    }

    fn sample_rate(&self) -> f32 {
        self.running.as_ref().map_or(48000.0, |r| r.control.sample_rate)
    }

    fn add_local(&mut self, def: &'static EffectDef) -> usize {
        self.chain.push(Fx::new(self.next_id, Spec::new(def), false));
        self.next_id += 1;
        self.chain.len() - 1
    }

    fn start(&mut self) {
        let opts = LiveOpts {
            input_device: self.in_sel.map(|i| self.in_devices[i].clone()),
            output_device: self.out_sel.map(|i| self.out_devices[i].clone()),
            buffer: Some(self.buffer),
            channel: 0,
            input_db: self.input_db,
            output_db: self.output_db,
        };
        let specs: Vec<Spec> = self.chain.iter().map(|f| f.spec.clone()).collect();
        match live::start(&opts, &specs) {
            Ok(running) => {
                // The audio chain numbers its slots by position; mirror that.
                for (i, fx) in self.chain.iter_mut().enumerate() {
                    fx.id = i as u64;
                }
                self.next_id = self.chain.len() as u64;
                for fx in &self.chain {
                    if fx.bypass {
                        running.control.send(Cmd::Bypass { id: fx.id, on: true });
                    }
                }
                running.control.meters.set_tuner(self.tuner_on, self.tuner_mute);
                running.control.meters.set_listening(self.listen_on || self.band.on);
                running.control.send(Cmd::Band(BandCmd::Settings(self.band)));
                if self.band_cache.is_some() {
                    for c in self.band_setup().commands() {
                        running.control.send(Cmd::Band(c));
                    }
                    running.control.send(Cmd::Band(BandCmd::Settings(self.band)));
                }
                running.control.send(Cmd::Metronome { on: self.metro_on, bpm: self.bpm, beats: self.beats, volume: self.metro_vol });
                running.control.send(Cmd::Looper(LoopCmd::Level(self.loop_vol)));
                self.status = format!("{} → {}", running.input_name, running.output_name);
                self.error = None;
                self.running = Some(running);
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn send_metronome(&self) {
        self.send(Cmd::Metronome { on: self.metro_on, bpm: self.bpm, beats: self.beats, volume: self.metro_vol });
    }

    /// Start from scratch: no effects, gains at zero.
    fn new_chain(&mut self) {
        for fx in std::mem::take(&mut self.chain) {
            self.send(Cmd::Remove { id: fx.id });
        }
        self.input_db = 0.0;
        self.output_db = 0.0;
        self.send(Cmd::Gains { input_db: 0.0, output_db: 0.0 });
        self.chain_name = "new-chain".into();
        self.error = None;
    }

    /// The Listener runs while its window is open or while the band is following the guitar.
    fn sync_listening(&self) {
        if let Some(r) = &self.running {
            r.control.meters.set_listening(self.listen_on || self.band.on);
        }
    }

    fn send(&self, cmd: Cmd) {
        if let Some(r) = &self.running {
            r.control.send(cmd);
        }
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::Param(i, p) => {
                let fx = &self.chain[i];
                let def = &fx.spec.def.params[p];
                if let Some(r) = &self.running {
                    if def.live {
                        r.control.send(Cmd::Set { id: fx.id, key: def.key, value: fx.spec.values[p] });
                    } else {
                        // Structural change: build a fresh effect here, off the audio thread.
                        r.control.send(Cmd::Replace { id: fx.id, fx: build(&fx.spec, r.control.sample_rate) });
                    }
                }
            }
            Act::Bypass(i) => {
                let fx = &mut self.chain[i];
                fx.bypass = !fx.bypass;
                let (id, on) = (fx.id, fx.bypass);
                self.send(Cmd::Bypass { id, on });
            }
            Act::Remove(i) => {
                let fx = self.chain.remove(i);
                self.send(Cmd::Remove { id: fx.id });
            }
            Act::MoveTo(i, to) => {
                let to = to.min(self.chain.len() - 1);
                if to != i {
                    let fx = self.chain.remove(i);
                    let id = fx.id;
                    self.chain.insert(to, fx);
                    self.send(Cmd::Move { id, to });
                }
            }
            Act::Add(def) => {
                let i = self.add_local(def);
                if let Some(r) = &self.running {
                    let slot = Slot::new(self.chain[i].id, &self.chain[i].spec, r.control.sample_rate);
                    r.control.send(Cmd::Insert { index: i, slot });
                }
            }
        }
    }

    /// Replace the whole chain with the one described by `text`.
    fn load_chain(&mut self, text: &str, name: &str) {
        match Preset::parse(text) {
            Ok(p) => {
                for fx in std::mem::take(&mut self.chain) {
                    self.send(Cmd::Remove { id: fx.id });
                }
                for (i, spec) in p.specs.into_iter().enumerate() {
                    let sr = self.sample_rate();
                    let id = self.next_id;
                    self.next_id += 1;
                    let off = p.bypass.get(i).copied().unwrap_or(false);
                    let slot = Slot::new(id, &spec, sr);
                    self.send(Cmd::Insert { index: self.chain.len(), slot });
                    if off {
                        self.send(Cmd::Bypass { id, on: true });
                    }
                    self.chain.push(Fx::new(id, spec, off));
                }
                self.input_db = p.input_db;
                self.output_db = p.output_db;
                self.send(Cmd::Gains { input_db: p.input_db, output_db: p.output_db });
                self.chain_name = name.to_string();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{name}: {e}")),
        }
    }

    fn load_user_chain(&mut self, name: &str) {
        match std::fs::read_to_string(Self::chains_dir().join(format!("{name}.fx"))) {
            Ok(text) => self.load_chain(&text, name),
            Err(e) => self.error = Some(format!("{name}: {e}")),
        }
    }

    fn save_user_chain(&mut self) {
        let dir = Self::chains_dir();
        let _ = std::fs::create_dir_all(&dir);
        let name: String = self.chain_name.trim().chars().map(|c| if c.is_alphanumeric() || "-_ ".contains(c) { c } else { '-' }).collect();
        if name.is_empty() {
            self.error = Some("give the chain a name first".into());
            return;
        }
        let path = dir.join(format!("{name}.fx"));
        let mut p = self.preset();
        p.buffer = None;
        match p.save(&path.to_string_lossy()) {
            Ok(()) => {
                self.error = None;
                self.status = format!("saved {}", path.display());
                self.scan_chains();
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn record_levels(&mut self) {
        let Some(r) = &self.running else {
            self.chain.iter_mut().for_each(|f| f.hist.clear());
            return;
        };
        if self.last_hist.elapsed() < Duration::from_millis(30) {
            return;
        }
        self.last_hist = Instant::now();
        for (i, fx) in self.chain.iter_mut().enumerate() {
            fx.hist.push_back(level_frac(r.control.meters.slot_peak(i)));
            while fx.hist.len() > HISTORY {
                fx.hist.pop_front();
            }
        }
    }
}

/// Meter bar on a -60..0 dB scale.
fn meter(ui: &mut egui::Ui, label: &str, peak: f32) {
    let db = 20.0 * peak.max(1e-6).log10();
    let frac = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let color = if db > -1.0 {
        egui::Color32::from_rgb(220, 60, 60)
    } else if db > -12.0 {
        egui::Color32::from_rgb(220, 180, 60)
    } else {
        egui::Color32::from_rgb(70, 190, 110)
    };
    ui.label(label);
    ui.add(egui::ProgressBar::new(frac).desired_width(140.0).fill(color).text(format!("{db:.0} dB")));
}


/// Output level of one effect over the last couple of seconds.
fn level_graph(ui: &mut egui::Ui, hist: &VecDeque<f32>, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(HISTORY as f32 * 1.5, 24.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(90));
    let n = hist.len();
    for (k, v) in hist.iter().enumerate() {
        let x = rect.right() - (n - k) as f32 * 1.5;
        let top = rect.bottom() - v * (rect.height() - 2.0) - 1.0;
        let hot = *v > 0.98;
        let c = if hot { egui::Color32::from_rgb(235, 70, 70) } else { theme::tint(color, 70 + (150.0 * v) as u8) };
        p.line_segment([egui::pos2(x, rect.bottom() - 1.0), egui::pos2(x, top)], egui::Stroke::new(1.5, c));
    }
    if let Some(v) = hist.back() {
        let y = rect.bottom() - v * (rect.height() - 2.0) - 1.0;
        p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], egui::Stroke::new(1.0, theme::tint(color, 140)));
    }
}

/// Grip the user drags to reorder a card.
fn grip(ui: &mut egui::Ui, color: egui::Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(16.0, 30.0), egui::Sense::drag());
    let c = if resp.hovered() || resp.dragged() { color } else { theme::tint(color, 130) };
    for row in 0..3 {
        for col in 0..2 {
            let pos = rect.center() + egui::vec2(col as f32 * 6.0 - 3.0, row as f32 * 6.0 - 6.0);
            ui.painter().circle_filled(pos, 1.7, c);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::Grab).on_hover_text("Drag to reorder")
}

/// Draws one parameter; returns true when the user changed it. Right-click resets it.
fn param_widget(ui: &mut egui::Ui, salt: &str, spec: &mut Spec, pi: usize) -> bool {
    let def = &spec.def.params[pi];
    let mut changed = false;
    match &def.kind {
        Kind::Num { min, max, default, unit } => {
            let mut v = match spec.values[pi] {
                Value::Num(n) => n,
                _ => *default,
            };
            let mut slider = egui::Slider::new(&mut v, *min..=*max).max_decimals(3);
            if *min > 0.0 && *max / *min >= 50.0 {
                slider = slider.logarithmic(true);
            }
            if !unit.is_empty() {
                slider = slider.suffix(format!(" {unit}"));
            }
            let r = ui.add(slider);
            if r.secondary_clicked() {
                v = *default;
                changed = true;
            }
            if r.changed() || changed {
                spec.values[pi] = Value::Num(v);
                changed = true;
            }
        }
        Kind::AutoNum { min, max, unit } => {
            let mut auto = matches!(spec.values[pi], Value::Auto);
            let mut v = match spec.values[pi] {
                Value::Num(n) => n,
                _ => 0.0,
            };
            ui.horizontal(|ui| {
                if ui.checkbox(&mut auto, "auto").changed() {
                    changed = true;
                }
                if !auto {
                    let r = ui.add(egui::Slider::new(&mut v, *min..=*max).suffix(format!(" {unit}")));
                    changed |= r.changed();
                }
            });
            if changed {
                spec.values[pi] = if auto { Value::Auto } else { Value::Num(v) };
            }
        }
        Kind::Text { .. } => {
            // The UI supplies a file picker for the cabinet; elsewhere text is shown read-only.
            let t = match spec.values[pi] {
                Value::Text(t) => t,
                _ => "",
            };
            if spec.def.name == "cabinet" {
                let files = paths::ir_files();
                let mut chosen = t;
                egui::ComboBox::from_id_salt(format!("{salt}-ir"))
                    .width(170.0)
                    .selected_text(if t.is_empty() { "built-in cabinet" } else { t })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut chosen, "", "built-in cabinet");
                        for f in &files {
                            let name = guitar_fx_core::params::intern(f);
                            ui.selectable_value(&mut chosen, name, f.as_str());
                        }
                        ui.separator();
                        ui.weak(format!("Put .wav files in {}", paths::ir_dir().display()));
                    });
                if chosen != t {
                    spec.values[pi] = Value::Text(chosen);
                    changed = true;
                }
            } else if spec.def.name == "nam" {
                let files = paths::nam_files();
                let mut chosen = t;
                let shown = if t.is_empty() { "(none)" } else { t.rsplit(['/', '\\']).next().unwrap_or(t) };
                egui::ComboBox::from_id_salt(format!("{salt}-nam"))
                    .width(190.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut chosen, "", "(none)");
                        for f in &files {
                            let name = guitar_fx_core::params::intern(f);
                            ui.selectable_value(&mut chosen, name, f.as_str());
                        }
                        ui.separator();
                        ui.weak(format!("Put .nam files in {}", paths::nam_dir().display()));
                    });
                if chosen != t {
                    spec.values[pi] = Value::Text(chosen);
                    changed = true;
                }
            } else {
                ui.label(if t.is_empty() { "(none)" } else { t });
            }
        }
        Kind::Choice { options, .. } => {
            let mut i = match spec.values[pi] {
                Value::Choice(i) => i,
                _ => 0,
            };
            egui::ComboBox::from_id_salt(format!("{salt}-{}", def.key))
                .selected_text(options[i])
                .show_ui(ui, |ui| {
                    for (n, o) in options.iter().enumerate() {
                        if ui.selectable_value(&mut i, n, *o).changed() {
                            changed = true;
                        }
                    }
                });
            if changed {
                spec.values[pi] = Value::Choice(i);
            }
        }
    }
    changed
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let mut acts: Vec<Act> = Vec::new();
        let mut gains_changed = false;

        if let Some(r) = &self.running {
            let m = &r.control.meters;
            self.shown_in = m.in_peak();
            self.shown_out = m.out_peak();
            ctx.request_repaint_after(Duration::from_millis(33));
        }
        self.record_levels();

        egui::Panel::top("top").show(root, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                if self.running.is_none() {
                    if ui.button("▶  Start").clicked() {
                        self.start();
                    }
                } else if ui.button("■  Stop").clicked() {
                    self.running = None;
                    self.status.clear();
                }
                ui.separator();

                let enabled = self.running.is_none();
                ui.add_enabled_ui(enabled, |ui| {
                    ui.label("In");
                    device_combo(ui, "in-dev", &self.in_devices, &mut self.in_sel);
                    ui.label("Out");
                    device_combo(ui, "out-dev", &self.out_devices, &mut self.out_sel);
                    ui.label("Buffer");
                    egui::ComboBox::from_id_salt("buffer")
                        .selected_text(format!("{} fr", self.buffer))
                        .show_ui(ui, |ui| {
                            for b in BUFFERS {
                                ui.selectable_value(&mut self.buffer, b, format!("{b} frames"));
                            }
                        });
                    if ui.small_button("⟳").on_hover_text("Refresh device list").clicked() {
                        let (i, o) = live::device_names().unwrap_or_default();
                        self.in_devices = i;
                        self.out_devices = o;
                        self.in_sel = None;
                        self.out_sel = None;
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                gains_changed |= ui.add(egui::Slider::new(&mut self.input_db, -24.0..=24.0).suffix(" dB").text("Input gain")).changed();
                gains_changed |= ui.add(egui::Slider::new(&mut self.output_db, -24.0..=12.0).suffix(" dB").text("Output gain")).changed();
                ui.separator();
                meter(ui, "In", self.shown_in);
                meter(ui, "Out", self.shown_out);
                if let Some(r) = &self.running {
                    let x = r.control.meters.xruns();
                    let color = if x == 0 { egui::Color32::GRAY } else { egui::Color32::from_rgb(220, 120, 60) };
                    ui.colored_label(color, format!("dropouts: {x}"))
                        .on_hover_text("Buffer underruns. If this climbs while you play, pick a larger buffer.");
                    let m = &r.control.meters;
                    let ms = m.buffered() as f32 / r.control.sample_rate * 1000.0;
                    ui.weak(format!("queue {ms:.1} ms")).on_hover_text(format!(
                        "Audio waiting between input and output (part of the latency). It is kept from growing; {} samples have been dropped so far to do that.",
                        m.trimmed()
                    ));
                }
            });
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), e);
            } else if !self.status.is_empty() {
                ui.label(&self.status);
            }
            ui.add_space(4.0);
        });

        // ---- practice tools: metronome, looper, tuner
        let mut metro_changed = false;
        let mut loop_cmd: Option<LoopCmd> = None;
        let mut tuner_changed = false;
        egui::Panel::top("tools-bar").show(root, |ui| {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                // metronome
                ui.label(egui::RichText::new("Metronome").strong());
                let (clicks, at, accent) = self.last_beat;
                if let Some(r) = &self.running {
                    let (c, a) = r.control.meters.beat();
                    if c != clicks {
                        self.last_beat = (c, Instant::now(), a);
                    }
                }
                let lit = self.metro_on && self.last_beat.1.elapsed() < Duration::from_millis(110);
                let (dot, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                let dot_color = if !lit { egui::Color32::from_gray(70) } else if accent { egui::Color32::from_rgb(235, 80, 70) } else { egui::Color32::from_rgb(90, 200, 120) };
                ui.painter().circle_filled(dot.center(), 6.0, dot_color);
                let _ = at;
                if ui.selectable_label(self.metro_on, if self.metro_on { "■ On" } else { "▶ Off" }).clicked() {
                    self.metro_on = !self.metro_on;
                    metro_changed = true;
                }
                metro_changed |= ui.add(egui::DragValue::new(&mut self.bpm).range(20.0..=300.0).suffix(" bpm")).changed();
                metro_changed |= ui.add(egui::DragValue::new(&mut self.beats).range(1..=12).suffix(" / bar")).changed();
                metro_changed |= ui.add(egui::Slider::new(&mut self.metro_vol, 0.0..=1.0).show_value(false)).on_hover_text("Click volume").changed();
                if ui.button("Tap").on_hover_text("Tap the tempo").clicked() {
                    let now = Instant::now();
                    self.taps.retain(|t| now.duration_since(*t) < Duration::from_millis(2500));
                    self.taps.push(now);
                    if self.taps.len() >= 2 {
                        let span = now.duration_since(self.taps[0]).as_secs_f32();
                        let bpm = 60.0 * (self.taps.len() - 1) as f32 / span;
                        self.bpm = bpm.clamp(20.0, 300.0).round();
                        metro_changed = true;
                    }
                }
                ui.separator();

                // looper
                ui.label(egui::RichText::new("Looper").strong());
                let st = self.running.as_ref().map_or(LoopState::Empty, |r| r.control.meters.loop_state());
                let live = self.running.is_some();
                ui.add_enabled_ui(live, |ui| {
                    let (label, cmd) = match st {
                        LoopState::Empty => ("● Record", LoopCmd::Record),
                        LoopState::Recording => ("▶ Close loop", LoopCmd::Play),
                        LoopState::Playing => ("◉ Overdub", LoopCmd::Overdub),
                        LoopState::Overdubbing => ("▶ Back to play", LoopCmd::Play),
                        LoopState::Stopped => ("▶ Play", LoopCmd::Play),
                    };
                    let color = match st {
                        LoopState::Recording => egui::Color32::from_rgb(235, 70, 70),
                        LoopState::Overdubbing => egui::Color32::from_rgb(235, 170, 60),
                        LoopState::Playing => egui::Color32::from_rgb(90, 200, 120),
                        _ => ui.visuals().text_color(),
                    };
                    if ui.button(egui::RichText::new(label).color(color)).clicked() {
                        loop_cmd = Some(cmd);
                    }
                    if ui.add_enabled(matches!(st, LoopState::Playing | LoopState::Overdubbing | LoopState::Recording), egui::Button::new("■ Stop")).clicked() {
                        loop_cmd = Some(LoopCmd::Stop);
                    }
                    if ui.add_enabled(st != LoopState::Empty, egui::Button::new("✕ Clear")).clicked() {
                        loop_cmd = Some(LoopCmd::Clear);
                    }
                    if ui.add(egui::Slider::new(&mut self.loop_vol, 0.0..=1.5).show_value(false)).on_hover_text("Loop volume").changed() {
                        loop_cmd = Some(LoopCmd::Level(self.loop_vol));
                    }
                });
                if let Some(r) = &self.running {
                    let (len, pos) = r.control.meters.loop_seconds(r.control.sample_rate);
                    if st != LoopState::Empty {
                        let frac = if st == LoopState::Recording { 1.0 } else if len > 0.0 { pos / len } else { 0.0 };
                        ui.add(egui::ProgressBar::new(frac).desired_width(90.0).text(format!("{len:.1} s")));
                    }
                }
                ui.separator();

                // tuner
                if ui.selectable_label(self.tuner_on, "Tuner").on_hover_text("Open the tuner").clicked() {
                    self.tuner_on = !self.tuner_on;
                    tuner_changed = true;
                }
                if ui.selectable_label(self.listen_on, "Listener").on_hover_text("Show the tempo, key and chord of what you play").clicked() {
                    self.listen_on = !self.listen_on;
                    self.sync_listening();
                }
                if ui.selectable_label(self.band.on, "Band").on_hover_text("A drummer and a bassist that follow your playing").clicked() {
                    self.band_open = !self.band_open;
                }
            });
            ui.add_space(2.0);
        });
        if metro_changed {
            self.send_metronome();
        }
        if let Some(c) = loop_cmd {
            self.send(Cmd::Looper(c));
        }
        if tuner_changed {
            if let Some(r) = &self.running {
                r.control.meters.set_tuner(self.tuner_on, self.tuner_mute);
            }
        }
        if self.tuner_on {
            let mut open = true;
            let mut mute_changed = false;
            let hz = self.running.as_ref().map_or(0.0, |r| r.control.meters.pitch());
            if hz > 0.0 {
                self.tuner_hz = hz;
                self.tuner_seen = Instant::now();
            }
            let live_note = self.tuner_seen.elapsed() < Duration::from_millis(400) && self.tuner_hz > 0.0;
            let shown_hz = self.tuner_hz;
            let running = self.running.is_some();
            let mut mute = self.tuner_mute;
            egui::Window::new("Tuner").open(&mut open).collapsible(false).resizable(false).show(&ctx, |ui| {
                ui.set_min_width(300.0);
                if !running {
                    ui.label("Press Start so the tuner can hear the guitar.");
                }
                let (name, cents) = if live_note {
                    let midi = 69.0 + 12.0 * (shown_hz / 440.0).log2();
                    let n = midi.round();
                    let names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
                    (format!("{}{}", names[(n as i32).rem_euclid(12) as usize], (n as i32).div_euclid(12) - 1), (midi - n) * 100.0)
                } else {
                    ("—".to_string(), 0.0)
                };
                let color = if !live_note {
                    egui::Color32::GRAY
                } else if cents.abs() < 5.0 {
                    egui::Color32::from_rgb(80, 210, 120)
                } else if cents.abs() < 15.0 {
                    egui::Color32::from_rgb(235, 190, 60)
                } else {
                    egui::Color32::from_rgb(235, 90, 80)
                };
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(name).size(54.0).strong().color(color));
                    ui.label(if live_note { format!("{shown_hz:.1} Hz   {cents:+.0} cents") } else { "play a string".into() });
                });
                let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 36.0), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(rect, 6.0, egui::Color32::from_black_alpha(90));
                let x_of = |c: f32| rect.center().x + c / 50.0 * (rect.width() / 2.0 - 10.0);
                for t in [-50.0, -25.0, 0.0, 25.0, 50.0] {
                    let h = if t == 0.0 { 14.0 } else { 7.0 };
                    p.line_segment([egui::pos2(x_of(t), rect.center().y - h), egui::pos2(x_of(t), rect.center().y + h)], egui::Stroke::new(1.5, egui::Color32::from_gray(120)));
                }
                if live_note {
                    p.circle_filled(egui::pos2(x_of(cents.clamp(-50.0, 50.0)), rect.center().y), 9.0, color);
                }
                ui.add_space(4.0);
                mute_changed = ui.checkbox(&mut mute, "Mute the output while tuning").changed();
            });
            self.tuner_mute = mute;
            if !open {
                self.tuner_on = false;
                mute_changed = true;
            }
            if mute_changed {
                if let Some(r) = &self.running {
                    r.control.meters.set_tuner(self.tuner_on, self.tuner_mute);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        if self.listen_on {
            let mut open = true;
            let a = self.running.as_ref().map(|r| r.control.meters.analysis()).unwrap_or_default();
            let running = self.running.is_some();
            egui::Window::new("Listener").open(&mut open).collapsible(false).resizable(false).show(&ctx, |ui| {
                ui.set_min_width(300.0);
                if !running {
                    ui.label("Press Start so the listener can hear the guitar.");
                }
                let dim = egui::Color32::GRAY;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.weak("Chord");
                        let (txt, col) = match a.chord {
                            Some(c) => (c.name(), egui::Color32::from_rgb(120, 200, 255)),
                            None => ("—".to_string(), dim),
                        };
                        ui.label(egui::RichText::new(txt).size(40.0).strong().color(col));
                    });
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.weak("Key");
                        match a.key {
                            Some(k) => {
                                ui.label(egui::RichText::new(k.name()).size(24.0).strong());
                                let sure = if k.conf > 0.6 && k.tonic_conf > 0.5 { "clear" } else if k.conf > 0.3 { "fairly sure" } else { "unsure" };
                                ui.weak(format!("{sure}  ·  notes of {} major", listen::NOTE_NAMES[k.scale_root() as usize]));
                            }
                            None => {
                                ui.label(egui::RichText::new("—").size(24.0).color(dim));
                                ui.weak("needs a few seconds of playing");
                            }
                        }
                    });
                });
                ui.add_space(6.0);
                if ui.button("Forget what I played").on_hover_text("Start listening afresh: key, tempo and chord").clicked() {
                    if let Some(r) = &self.running {
                        r.control.meters.reset_listener();
                    }
                }
                ui.horizontal(|ui| {
                    let known = a.tempo > 0.0 && a.tempo_conf >= 0.15;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                    let pulse = if known { (1.0 - a.beat_phase * 3.0).clamp(0.0, 1.0) } else { 0.0 };
                    let c = egui::Color32::from_rgb(80 + (175.0 * pulse) as u8, 80 + (110.0 * pulse) as u8, 80);
                    ui.painter().circle_filled(rect.center(), 6.0 + 5.0 * pulse, c);
                    if known {
                        ui.label(egui::RichText::new(format!("{:.0} BPM", a.tempo)).size(24.0).strong());
                    } else if a.tempo > 0.0 {
                        ui.label(egui::RichText::new(format!("~{:.0} BPM", a.tempo)).size(24.0).color(dim));
                        ui.weak("(not steady yet)");
                    } else {
                        ui.label(egui::RichText::new("—").size(24.0).color(dim));
                        ui.weak("play a steady rhythm");
                    }
                });
                ui.add_space(6.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(300.0, 54.0), egui::Sense::hover());
                let p = ui.painter();
                p.rect_filled(rect, 6.0, egui::Color32::from_black_alpha(90));
                let w = rect.width() / 12.0;
                for i in 0..12 {
                    let h = (a.chroma[i].clamp(0.0, 1.0) * (rect.height() - 16.0)).max(1.0);
                    let x = rect.left() + i as f32 * w;
                    let in_key = a.key.is_some_and(|k| {
                        let rel = (i + 12 - k.scale_root() as usize) % 12;
                        [0, 2, 4, 5, 7, 9, 11].contains(&rel)
                    });
                    let col = if in_key { egui::Color32::from_rgb(120, 200, 255) } else { egui::Color32::from_gray(110) };
                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(x + 3.0, rect.bottom() - 14.0 - h), egui::pos2(x + w - 3.0, rect.bottom() - 14.0)), 2.0, col);
                    p.text(egui::pos2(x + w / 2.0, rect.bottom() - 7.0), egui::Align2::CENTER_CENTER, listen::NOTE_NAMES[i], egui::FontId::proportional(9.0), egui::Color32::from_gray(170));
                }
            });
            if !open {
                self.listen_on = false;
                self.sync_listening();
            }
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        if let (Some(r), true) = (&self.running, self.band_open || self.band.on) {
            let ev: BandEvents = r.control.meters.band_events();
            if ev.beats_per_bar != 0 {
                self.band_cache = Some((ev.drums, ev.roots));
            }
        }
        let mut load_band: Option<String> = None;
        let mut delete_band: Option<String> = None;
        let mut new_band = false;
        let mut load_builtin: Option<usize> = None;
        let mut load_user: Option<String> = None;
        let mut new_chain = false;
        let mut delete_user: Option<String> = None;
        egui::Panel::left("mine").resizable(true).default_size(230.0).size_range(170.0..=450.0).show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("My chains");
                if ui.add_sized([200.0, 24.0], egui::Button::new("＋  New chain")).on_hover_text("Remove every effect and start again").clicked() {
                    new_chain = true;
                }
                if self.user_chains.is_empty() {
                    ui.weak("Nothing saved yet.");
                }
                for name in &self.user_chains {
                    ui.horizontal(|ui| {
                        if ui.add_sized([170.0, 22.0], egui::Button::new(name)).clicked() {
                            load_user = Some(name.clone());
                        }
                        if ui.small_button("✕").on_hover_text("Delete this saved chain").clicked() {
                            delete_user = Some(name.clone());
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.chain_name).desired_width(100.0));
                    if ui.button("Save chain").clicked() {
                        self.save_user_chain();
                    }
                });
                ui.weak("Stored in your config folder.").on_hover_text(Self::chains_dir().display().to_string());

                ui.add_space(10.0);
                ui.separator();
                ui.heading("My bands");
                if ui.add_sized([200.0, 24.0], egui::Button::new("＋  New band")).on_hover_text("Default mixer, the first drum loop, no roots").clicked() {
                    new_band = true;
                }
                if self.user_bands.is_empty() {
                    ui.weak("Nothing saved yet.");
                }
                for name in &self.user_bands {
                    ui.horizontal(|ui| {
                        if ui.add_sized([170.0, 22.0], egui::Button::new(name)).on_hover_text("Load this band: mixer, styles, drum loop and roots").clicked() {
                            load_band = Some(name.clone());
                        }
                        if ui.small_button("✕").on_hover_text("Delete this saved band").clicked() {
                            delete_band = Some(name.clone());
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.band_name).desired_width(100.0));
                    if ui.button("Save band").clicked() {
                        self.save_band();
                    }
                });
                ui.weak("Recorded takes are not saved, only roots, styles, drums and the mixer.").on_hover_text(Self::bands_dir().display().to_string());
            });
        });
        egui::Panel::right("tools").resizable(true).default_size(270.0).size_range(200.0..=520.0).show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let groups: [(&str, Vec<&'static EffectDef>); 3] = [
                    ("Add effect", EFFECTS.iter().filter(|d| !EXPERIMENTAL.contains(&d.name) && !ZEN.contains(&d.name)).collect()),
                    ("Zen & ethereal", EFFECTS.iter().filter(|d| ZEN.contains(&d.name)).collect()),
                    ("Experimental", EFFECTS.iter().filter(|d| EXPERIMENTAL.contains(&d.name)).collect()),
                ];
                for (g, (title, defs)) in groups.iter().enumerate() {
                    if g > 0 {
                        ui.add_space(4.0);
                    }
                    ui.heading(*title);
                    egui::Grid::new(("add-grid", g)).num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                        for (k, def) in defs.iter().enumerate() {
                            let color = theme::accent(def.name);
                            ui.horizontal(|ui| {
                                theme::effect_icon(ui, def.name, color, 22.0);
                                let btn = egui::Button::new(egui::RichText::new(def.title).color(color))
                                    .fill(theme::tint(color, 28))
                                    .stroke(egui::Stroke::new(1.0, theme::tint(color, 120)));
                                if ui.add_sized([86.0, 22.0], btn).clicked() {
                                    acts.push(Act::Add(def));
                                }
                            });
                            if k % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                ui.separator();
                ui.heading("Built-in chains");
                for (k, (name, text)) in BUILTIN.iter().enumerate() {
                    let tip = text.lines().next().unwrap_or("").trim_start_matches('#').trim();
                    if ui.add_sized([250.0, 22.0], egui::Button::new(*name)).on_hover_text(tip).clicked() {
                        load_builtin = Some(k);
                    }
                }
            });
        });
        if let Some(k) = load_builtin {
            let (name, text) = BUILTIN[k];
            self.load_chain(text, name);
        }
        if new_chain {
            self.new_chain();
        }
        if let Some(name) = load_user {
            self.load_user_chain(&name);
        }
        if new_band {
            self.new_band();
        }
        if let Some(name) = load_band {
            self.load_band(&name);
        }
        if let Some(name) = delete_band {
            let _ = std::fs::remove_file(Self::bands_dir().join(format!("{name}.band")));
            self.scan_bands();
        }
        if let Some(name) = delete_user {
            let _ = std::fs::remove_file(Self::chains_dir().join(format!("{name}.fx")));
            self.scan_chains();
        }

        // Which slot would receive the dragged card (never one that leaves it where it is).
        let drop_slot: Option<usize> = match (self.dragging, ctx.pointer_interact_pos()) {
            (Some(i), Some(p)) => self
                .gap_rects
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != i && *k != i + 1)
                .min_by(|a, b| (a.1.center().y - p.y).abs().total_cmp(&(b.1.center().y - p.y).abs()))
                .map(|(k, _)| k),
            _ => None,
        };
        let mut new_gaps: Vec<egui::Rect> = Vec::new();
        let mut drop_at: Option<(usize, usize)> = None;
        let mut drag_now: Option<usize> = None;
        if self.band_open {
            let mut open = true;
            let mut b = self.band;
            let mut cmds: Vec<BandCmd> = Vec::new();
            let (pos, (rec_track, rec_stage), ev, levels) = match &self.running {
                Some(r) => (r.control.meters.band_pos(), r.control.meters.band_rec(), r.control.meters.band_events(), r.control.meters.band_levels()),
                None => (0.0, (0, 0), Default::default(), [0.0; 6]),
            };
            let running = self.running.is_some();
            let bpb = RHYTHMS[b.rhythm.min(RHYTHMS.len() - 1)].beats.max(1) as f32;
            let loop_beats = (b.bars as f32 * bpb).max(1.0);
            let mut pick = self.band_pick;
            let mut view = self.band_view;
            let score_cache = &mut self.score_cache;
            let wave_live: Vec<f32> = self.running.as_ref().map(|r| r.control.meters.band_wave()).unwrap_or_default();
            let count_text = format!("count-in: {}", (-pos).ceil().max(1.0) as i32);
            const HEAD_W: f32 = 236.0;
            const GUTTER: f32 = 58.0;
            const ROW_H: f32 = 15.0;
            let amber = egui::Color32::from_rgb(235, 170, 60);
            let blue = egui::Color32::from_rgb(90, 160, 235);
            let green = egui::Color32::from_rgb(110, 200, 130);
            let red = egui::Color32::from_rgb(235, 90, 80);
            let note_name = |pc: u8| listen::NOTE_NAMES[pc as usize % 12];
            egui::Panel::bottom("band").resizable(true).default_size(380.0).size_range(150.0..=720.0).show(root, |ui| {
              egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Band");
                    if !running {
                        ui.label("Press Start first.");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✕").on_hover_text("Close the mixer").clicked() {
                            open = false;
                        }
                    });
                });
                // ---- transport
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(running, egui::Button::new(if b.on { "■ Stop" } else { "▶ Start" }).selected(b.on)).clicked() {
                        b.on = !b.on;
                    }
                    if ui.add_enabled(b.on, egui::Button::new("↺")).on_hover_text("Back to the start of the loop").clicked() {
                        cmds.push(BandCmd::Restart);
                    }
                    ui.add(egui::DragValue::new(&mut b.bpm).range(40.0..=240.0).suffix(" bpm"));
                    ui.add(egui::DragValue::new(&mut b.bars).range(1..=8).suffix(" bars"));
                    ui.add(egui::DragValue::new(&mut b.count_in).range(0..=4).suffix(" bar count-in")).on_hover_text("Bars of click before a recording starts");
                    ui.label("Drum loop");
                    egui::ComboBox::from_id_salt("rhythm").width(170.0).selected_text(RHYTHMS[b.rhythm.min(RHYTHMS.len() - 1)].name).show_ui(ui, |ui| {
                        for (i, r) in RHYTHMS.iter().enumerate() {
                            ui.selectable_value(&mut b.rhythm, i, r.name);
                        }
                    });
                    ui.label("View");
                    ui.selectable_value(&mut view, BandView::Wave, "Waveform").on_hover_text("Roots and what each track plays, as a waveform");
                    ui.selectable_value(&mut view, BandView::Score, "Score").on_hover_text("Music notation with key, tempo and time signature");
                    ui.selectable_value(&mut view, BandView::Letters, "Letters").on_hover_text("Note names C D E F G A B against time, with bars and beats");
                    ui.label("Chords");
                    egui::ComboBox::from_id_salt("chord").width(150.0).selected_text(CHORD_TYPES[b.chord.min(CHORD_TYPES.len() - 1)]).show_ui(ui, |ui| {
                        for (i, n) in CHORD_TYPES.iter().enumerate() {
                            ui.selectable_value(&mut b.chord, i, *n);
                        }
                    });
                    ui.label("Snap");
                    egui::ComboBox::from_id_salt("snap").width(80.0).selected_text(match b.snap { Snap::Beat => "beat", Snap::Eighth => "eighth", Snap::Sixteenth => "sixteenth" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut b.snap, Snap::Beat, "beat");
                        ui.selectable_value(&mut b.snap, Snap::Eighth, "eighth");
                        ui.selectable_value(&mut b.snap, Snap::Sixteenth, "sixteenth");
                    });
                });
                ui.add_space(4.0);

                let lane_w = (ui.available_width() - HEAD_W - 12.0).max(360.0);
                let grid_w = lane_w - GUTTER;
                let x_of = |lane: egui::Rect, beat: f32| lane.left() + GUTTER + beat / loop_beats * grid_w;

                // ---- the ruler
                ui.horizontal_top(|ui| {
                    ui.allocate_space(egui::vec2(HEAD_W, 14.0));
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(lane_w, 14.0), egui::Sense::hover());
                    let p = ui.painter_at(rect);
                    for bar in 0..b.bars {
                        let x = x_of(rect, bar as f32 * bpb);
                        p.text(egui::pos2(x + 3.0, rect.center().y), egui::Align2::LEFT_CENTER, format!("{}", bar + 1), egui::FontId::proportional(11.0), egui::Color32::from_gray(170));
                    }
                });

                // ---- one row: header on the left, lane on the right
                // drums
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(egui::vec2(HEAD_W, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_width(HEAD_W - 8.0);
                        ui.horizontal(|ui| {
                            ui.colored_label(amber, egui::RichText::new("Drums").strong());
                            ui.toggle_value(&mut b.mute[0], "M").on_hover_text("Mute");
                            ui.toggle_value(&mut b.solo[0], "S").on_hover_text("Solo");
                        });
                        ui.add(egui::Slider::new(&mut b.vol[0], 0.0..=1.0).text("vol"));
                        ui.add(egui::ProgressBar::new(levels[0].sqrt().min(1.0)).desired_height(4.0));
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(RHYTHMS[b.rhythm.min(RHYTHMS.len() - 1)].name).weak());
                            if ui.small_button("✕ clear").clicked() {
                                cmds.push(BandCmd::ClearDrums);
                            }
                        });
                        ui.weak("click: hit · again: accent\nright-click: ghost/soft");
                    });
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(lane_w, ROW_H * 6.0), egui::Sense::click());
                    let p = ui.painter_at(rect);
                    p.rect_filled(rect, 3.0, egui::Color32::from_gray(26));
                    let steps = ev.drums.steps.max(1);
                    let total = (b.bars as usize * steps) as f32;
                    let cell_w = grid_w / total;
                    for (row, name) in band::DRUM_ROWS.iter().enumerate() {
                        let y = rect.top() + row as f32 * ROW_H;
                        p.text(egui::pos2(rect.left() + 4.0, y + ROW_H / 2.0), egui::Align2::LEFT_CENTER, *name, egui::FontId::proportional(10.0), egui::Color32::from_gray(150));
                        for k in 0..(b.bars as usize * steps) {
                            let i = k % steps;
                            let cell = egui::Rect::from_min_size(egui::pos2(rect.left() + GUTTER + k as f32 * cell_w, y + 1.0), egui::vec2((cell_w - 1.0).max(1.0), ROW_H - 2.0));
                            let lv = ev.drums.rows[row][i];
                            let on_beat = (i * bpb as usize) % steps == 0;
                            let col = match lv {
                                0 => if on_beat { egui::Color32::from_gray(52) } else { egui::Color32::from_gray(38) },
                                1 => amber.gamma_multiply(0.30),
                                2 => amber.gamma_multiply(0.55),
                                3 => amber.gamma_multiply(0.8),
                                _ => amber,
                            };
                            p.rect_filled(cell, 2.0, col);
                        }
                    }
                    if b.on {
                        let x = x_of(rect, pos.clamp(0.0, loop_beats));
                        p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(2.0, egui::Color32::from_rgb(250, 250, 250)));
                    }
                    if resp.clicked() || resp.secondary_clicked() {
                        if let Some(pt) = resp.interact_pointer_pos() {
                            let row = ((pt.y - rect.top()) / ROW_H) as i32;
                            let k = ((pt.x - rect.left() - GUTTER) / cell_w).floor() as i32;
                            if (0..6).contains(&row) && k >= 0 && (k as usize) < b.bars as usize * steps {
                                let i = k as usize % steps;
                                let lv = ev.drums.rows[row as usize][i];
                                let next = if resp.secondary_clicked() {
                                    match lv { 1 => 2, 2 => 0, _ => 1 }
                                } else {
                                    match lv { 0 | 1 | 2 => 3, 3 => 4, _ => 0 }
                                };
                                cmds.push(BandCmd::SetDrumStep(row as u8, i as u8, next));
                            }
                        }
                    }
                });
                ui.add_space(6.0);

                // bass and guitar
                for track in TRACKS {
                    let ti = track as usize; // 1..5: index into vol/style/src/mute/solo/levels
                    let title = track.name();
                    let styles = track.styles();
                    let col = match track {
                        Track::Bass => blue,
                        Track::Guitar => green,
                        Track::Lead => egui::Color32::from_rgb(190, 130, 235),
                        Track::Solo => egui::Color32::from_rgb(240, 120, 160),
                        Track::Organ => egui::Color32::from_rgb(80, 205, 205),
                    };
                    let ((wave, wave_ok), roots) = (ev.wave_of(track), ev.roots_of(track));
                    let (style, volume, source) = (&mut b.style[ti], &mut b.vol[ti], &mut b.src[ti]);
                    let mine = rec_track == track as u32;
                    let audio = *source == Source::Audio;
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(egui::vec2(HEAD_W, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(HEAD_W - 8.0);
                            ui.horizontal(|ui| {
                                ui.colored_label(col, egui::RichText::new(title).strong());
                                ui.toggle_value(&mut b.mute[ti], "M").on_hover_text("Mute");
                                ui.toggle_value(&mut b.solo[ti], "S").on_hover_text("Solo");
                                ui.selectable_value(source, Source::Style, "Style").on_hover_text("The band plays a style on the root notes you indicate");
                                ui.selectable_value(source, Source::Audio, "Rec").on_hover_text("Your own playing, recorded for one pass and looped");
                            });
                            ui.add(egui::Slider::new(volume, 0.0..=1.0).text("vol"));
                            ui.add(egui::ProgressBar::new(levels[ti].sqrt().min(1.0)).desired_height(4.0));
                            if !audio {
                                egui::ComboBox::from_id_salt(title).width(HEAD_W - 20.0).selected_text(styles[(*style).min(styles.len() - 1)].0).show_ui(ui, |ui| {
                                    for (i, (n, _)) in styles.iter().enumerate() {
                                        ui.selectable_value(style, i, *n);
                                    }
                                });
                            }
                            ui.horizontal(|ui| {
                                if mine {
                                    if ui.button("■ Cancel").clicked() {
                                        cmds.push(BandCmd::CancelRecord);
                                    }
                                } else if ui
                                    .add_enabled(b.on, egui::Button::new(if audio { "● Record take" } else { "● Record roots" }))
                                    .on_hover_text(if audio { "Starts at the top of the loop: play your part once through" } else { "Starts at the top of the loop: play each chord's root note where it changes (or click the lane to place them)" })
                                    .clicked()
                                {
                                    cmds.push(BandCmd::Record(track));
                                }
                                if track != Track::Bass && !audio && ui.add_enabled(!ev.roots_of(Track::Bass).is_empty(), egui::Button::new("= bass")).on_hover_text("Use the bass roots").clicked() {
                                    cmds.push(BandCmd::CopyRoots(Track::Bass, track));
                                }
                                if ui.small_button("✕").on_hover_text("Clear this track").clicked() {
                                    cmds.push(BandCmd::Clear(track));
                                }
                            });
                        });
                        // ---- what to draw in this view
                        let notes = &ev.notes[ti - 1];
                        let mut score: Option<(u32, f32)> = None; // (cache key, displayed height)
                        let mut score_err: Option<String> = None;
                        if view == BandView::Score && !audio && !notes.is_empty() {
                            let spec = notation::ScoreSpec { hits: notes, key_sig: ev.key_sig, minor: ev.key.1, bpm: b.bpm, bars: b.bars, beats_per_bar: bpb as u32, bass: track == Track::Bass };
                            if let Some(abc) = notation::abc(&spec) {
                                use std::hash::{Hash, Hasher};
                                let wq = ((lane_w / 32.0).floor() * 32.0).max(320.0);
                                let dark = ui.visuals().dark_mode;
                                let mut h = std::collections::hash_map::DefaultHasher::new();
                                (&abc, wq as u32, dark).hash(&mut h);
                                let key = h.finish();
                                if !score_cache.get(&(ti as u32)).is_some_and(|e| e.key == key) {
                                    match notation::render(&abc, wq, 5.0, if dark { "#dcdcdc" } else { "#222222" }, b.bars) {
                                        Ok(r) => {
                                            let img = egui::ColorImage::from_rgba_premultiplied([r.px_w, r.px_h], &r.rgba);
                                            let tex = ctx.load_texture(format!("score{ti}"), img, egui::TextureOptions::LINEAR);
                                            score_cache.insert(ti as u32, ScoreTex { key, tex, size: egui::vec2(r.px_w as f32 / 2.0, r.px_h as f32 / 2.0), width: wq, bar_x: r.bar_x });
                                        }
                                        Err(e) => {
                                            score_cache.remove(&(ti as u32));
                                            score_err = Some(e);
                                        }
                                    }
                                }
                                if let Some(e) = score_cache.get(&(ti as u32)) {
                                    score = Some((ti as u32, e.size.y * lane_w / e.width));
                                }
                            }
                        }
                        let letters = view == BandView::Letters && !audio;
                        let lane_h = match (&score, letters) {
                            (Some((_, h)), _) => (h + 22.0).max(70.0),
                            (None, true) => 90.0,
                            _ => 62.0,
                        };
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(lane_w, lane_h), egui::Sense::click());
                        let p = ui.painter_at(rect);
                        p.rect_filled(rect, 3.0, egui::Color32::from_gray(26));
                        let grid = egui::Rect::from_min_max(egui::pos2(rect.left() + GUTTER, rect.top()), rect.right_bottom());
                        // beat position -> x, and back, for this lane
                        let score_map = score.as_ref().and_then(|(k, _)| score_cache.get(k)).map(|e| (e.bar_x.clone(), lane_w / e.width));
                        let beat_x = |beat: f32| -> f32 {
                            match &score_map {
                                Some((bx, k)) => {
                                    let bar = ((beat / bpb).floor() as usize).min(bx.len().saturating_sub(2));
                                    let f = (beat - bar as f32 * bpb) / bpb;
                                    rect.left() + (bx[bar] + (bx[bar + 1] - bx[bar]) * f.clamp(0.0, 1.0)) * k
                                }
                                None => x_of(rect, beat),
                            }
                        };
                        let x_beat = |x: f32| -> f32 {
                            match &score_map {
                                Some((bx, k)) => {
                                    let xs = (x - rect.left()) / k;
                                    let bar = (0..bx.len() - 1).find(|&i| xs < bx[i + 1]).unwrap_or(bx.len() - 2);
                                    let f = ((xs - bx[bar]) / (bx[bar + 1] - bx[bar]).max(1.0)).clamp(0.0, 0.999);
                                    (bar as f32 + f) * bpb
                                }
                                None => ((x - grid.left()) / grid_w * loop_beats).clamp(0.0, loop_beats - 0.01),
                            }
                        };
                        if let Some((k, h)) = &score {
                            if let Some(e) = score_cache.get(k) {
                                let img = egui::Rect::from_min_size(egui::pos2(rect.left(), rect.top() + 16.0), egui::vec2(lane_w, *h));
                                // the tempo marking: a quarter note = bpm
                                let (nx, ny) = (rect.left() + 9.0, rect.top() + 9.0);
                                let ink = ui.visuals().text_color();
                                p.circle_filled(egui::pos2(nx, ny), 2.6, ink);
                                p.line_segment([egui::pos2(nx + 2.4, ny), egui::pos2(nx + 2.4, ny - 8.0)], egui::Stroke::new(1.0, ink));
                                p.text(egui::pos2(nx + 7.0, ny - 1.0), egui::Align2::LEFT_CENTER, format!("= {:.0}   {}/4   {} {}", b.bpm, bpb as u32, note_name(ev.key.0), if ev.key.1 { "minor" } else { "major" }), egui::FontId::proportional(11.0), ink);
                                p.image(e.tex.id(), img, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                            }
                        } else {
                            for bar in 0..b.bars {
                                let x = x_of(rect, bar as f32 * bpb);
                                p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(1.0, egui::Color32::from_gray(48)));
                            }
                        }
                        let faint = egui::Color32::from_gray(150);
                        if audio {
                            p.text(egui::pos2(rect.left() + 4.0, rect.center().y), egui::Align2::LEFT_CENTER, "take", egui::FontId::proportional(10.0), faint);
                            if wave.is_empty() {
                                p.text(grid.center(), egui::Align2::CENTER_CENTER, "no take yet: ● Record take, then play once through", egui::FontId::proportional(12.0), egui::Color32::from_gray(120));
                            } else {
                                let c = if wave_ok { col } else { egui::Color32::from_gray(90) };
                                let mid = grid.center().y;
                                for (i, v) in wave.iter().enumerate() {
                                    let x = grid.left() + (i as f32 + 0.5) / wave.len() as f32 * grid_w;
                                    let h = (v.sqrt().min(1.0)) * (lane_h / 2.0 - 3.0);
                                    p.line_segment([egui::pos2(x, mid - h), egui::pos2(x, mid + h)], egui::Stroke::new((grid_w / wave.len() as f32).max(1.0), c));
                                }
                                if !wave_ok {
                                    p.text(grid.center(), egui::Align2::CENTER_CENTER, "recorded at another tempo or length: silent", egui::FontId::proportional(12.0), egui::Color32::from_rgb(235, 190, 60));
                                }
                            }
                        } else if score.is_some() {
                            // the engraved score is the picture
                        } else if letters {
                            // rows C..B, time across, bars and beats numbered, chords named
                            let dim = egui::Color32::from_gray(120);
                            let (top_strip, rh) = (26.0, 8.0);
                            let flats = ev.key_sig < 0;
                            let row_y = |l: usize| rect.top() + top_strip + (6 - l) as f32 * rh;
                            for l in 0..7 {
                                let y = row_y(l);
                                if l % 2 == 0 {
                                    p.rect_filled(egui::Rect::from_min_max(egui::pos2(grid.left(), y), egui::pos2(rect.right(), y + rh)), 0.0, egui::Color32::from_white_alpha(6));
                                }
                                p.text(egui::pos2(rect.left() + GUTTER - 12.0, y + rh / 2.0), egui::Align2::CENTER_CENTER, ['C', 'D', 'E', 'F', 'G', 'A', 'B'][l].to_string(), egui::FontId::monospace(9.0), faint);
                            }
                            let key_name = format!("{} {}", note_name(ev.key.0), if ev.key.1 { "minor" } else { "major" });
                            p.text(egui::pos2(rect.left() + 4.0, rect.top() + 7.0), egui::Align2::LEFT_CENTER, format!("{}/{}  {:.0} bpm", bpb as u32, 4, b.bpm), egui::FontId::proportional(9.0), faint);
                            p.text(egui::pos2(rect.left() + 4.0, rect.top() + 20.0), egui::Align2::LEFT_CENTER, key_name, egui::FontId::proportional(9.0), faint);
                            for beat in 0..loop_beats as u32 {
                                let x = x_of(rect, beat as f32);
                                let first = beat % bpb as u32 == 0;
                                p.line_segment([egui::pos2(x, rect.top() + if first { 0.0 } else { 14.0 }), egui::pos2(x, rect.bottom() - 2.0)], egui::Stroke::new(1.0, egui::Color32::from_gray(if first { 70 } else { 40 })));
                                let label = if first { format!("{}", beat / bpb as u32 + 1) } else { format!("{}", beat % bpb as u32 + 1) };
                                p.text(egui::pos2(x + 3.0, rect.top() + 7.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(if first { 11.0 } else { 9.0 }), if first { amber } else { dim });
                            }
                            let mut last_chord: Option<(u8, u8)> = None;
                            for h in notes.iter() {
                                if let Some(c) = h.chord {
                                    if last_chord != Some(c) || h.t % bpb == 0.0 {
                                        let sym = format!("{}{}", note_name(c.0), ["", "m", "7", "m7", "5"][c.1 as usize % 5]);
                                        p.text(egui::pos2(x_of(rect, h.t) + 2.0, rect.top() + 20.0), egui::Align2::LEFT_CENTER, sym, egui::FontId::proportional(10.0), col);
                                    }
                                    last_chord = Some(c);
                                }
                                for &m in &h.notes {
                                    let (l, acc) = notation::spell(m as usize % 12, flats);
                                    let oct = (m / 12) as i32 - 1;
                                    let (x0, x1) = (x_of(rect, h.t), x_of(rect, (h.t + h.dur).min(loop_beats)) - 1.0);
                                    let blk = egui::Rect::from_min_max(egui::pos2(x0, row_y(l) + 0.5), egui::pos2(x1.max(x0 + 3.0), row_y(l) + rh - 0.5));
                                    p.rect_filled(blk, 1.5, col.gamma_multiply((0.35 + 0.1 * (oct - 1).clamp(0, 6) as f32).min(1.0)));
                                    if blk.width() >= 13.0 {
                                        let name = format!("{}{}{}", ['C', 'D', 'E', 'F', 'G', 'A', 'B'][l], match acc { 1 => "#", -1 => "b", _ => "" }, oct);
                                        p.text(blk.left_center() + egui::vec2(1.5, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::monospace(7.0), egui::Color32::WHITE);
                                    }
                                }
                            }
                            if notes.is_empty() {
                                p.text(grid.center(), egui::Align2::CENTER_CENTER, "no roots yet: ● Record roots and play them, or click here to place one", egui::FontId::proportional(12.0), egui::Color32::from_gray(120));
                            }
                        } else {
                            p.text(egui::pos2(rect.left() + 4.0, rect.center().y), egui::Align2::LEFT_CENTER, "roots", egui::FontId::proportional(10.0), faint);
                            // root blocks: each lasts until the next root; the last one carries across the loop
                            let n = roots.len();
                            for (k, &(t, pc)) in roots.iter().enumerate() {
                                let end = if k + 1 < n { roots[k + 1].0 } else { loop_beats };
                                let blk = egui::Rect::from_min_max(egui::pos2(beat_x(t), rect.top() + 3.0), egui::pos2(beat_x(end) - 1.0, rect.bottom() - 3.0));
                                p.rect_filled(blk, 3.0, col.gamma_multiply(0.35));
                                p.rect_stroke(blk, 3.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Inside);
                                p.text(blk.left_top() + egui::vec2(5.0, 3.0), egui::Align2::LEFT_TOP, note_name(pc), egui::FontId::proportional(15.0), egui::Color32::WHITE);
                            }
                            if let Some(&(t0, pc)) = roots.first() {
                                if t0 > 0.0 {
                                    let last = roots.last().map(|r| r.1).unwrap_or(pc);
                                    let blk = egui::Rect::from_min_max(egui::pos2(x_of(rect, 0.0), rect.top() + 3.0), egui::pos2(x_of(rect, t0) - 1.0, rect.bottom() - 3.0));
                                    p.rect_filled(blk, 3.0, col.gamma_multiply(0.2));
                                    p.text(blk.left_top() + egui::vec2(5.0, 3.0), egui::Align2::LEFT_TOP, note_name(last), egui::FontId::proportional(13.0), egui::Color32::from_gray(190));
                                }
                            }
                            // what the track actually played over the loop (live: the last pass stays until it is overwritten)
                            if wave_live.len() >= (ti + 1) * band::WAVE_BINS {
                                let w = &wave_live[ti * band::WAVE_BINS..(ti + 1) * band::WAVE_BINS];
                                let mid = rect.center().y;
                                for (i, v) in w.iter().enumerate() {
                                    let x = grid.left() + (i as f32 + 0.5) / w.len() as f32 * grid_w;
                                    let h = v.sqrt().min(1.0) * (lane_h / 2.0 - 4.0);
                                    if h > 0.4 {
                                        p.line_segment([egui::pos2(x, mid - h), egui::pos2(x, mid + h)], egui::Stroke::new((grid_w / w.len() as f32).max(1.0), col.gamma_multiply(1.4)));
                                    }
                                }
                            }
                            if roots.is_empty() {
                                p.text(grid.center(), egui::Align2::CENTER_CENTER, "no roots yet: ● Record roots and play them, or click here to place one", egui::FontId::proportional(12.0), egui::Color32::from_gray(120));
                            }
                        }
                        if let Some(e) = &score_err {
                            p.text(grid.center(), egui::Align2::CENTER_CENTER, format!("cannot engrave: {e}"), egui::FontId::proportional(11.0), red);
                        }
                        if !audio && view == BandView::Score && score.is_none() && score_err.is_none() {
                            p.text(grid.center(), egui::Align2::CENTER_CENTER, "no notes yet: record or place roots, and the score appears", egui::FontId::proportional(12.0), egui::Color32::from_gray(120));
                        }
                        if mine {
                            p.rect_stroke(rect, 3.0, egui::Stroke::new(2.0, if rec_stage == 0 { amber } else { red }), egui::StrokeKind::Inside);
                            p.text(egui::pos2(rect.right() - 8.0, rect.top() + 9.0), egui::Align2::RIGHT_CENTER, if rec_stage == 0 { count_text.as_str() } else if audio { "● recording: play your part" } else { "● play the root notes, one at a time" }, egui::FontId::proportional(12.0), if rec_stage == 0 { amber } else { red });
                        }
                        if b.on {
                            let x = beat_x(pos.clamp(0.0, loop_beats));
                            p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(2.0, egui::Color32::from_rgb(250, 250, 250)));
                        }
                        if !audio && resp.clicked() {
                            if let Some(pt) = resp.interact_pointer_pos() {
                                if score.is_some() || pt.x >= grid.left() {
                                    pick = Some((track, x_beat(pt.x), pt));
                                }
                            }
                        }
                    });
                    ui.add_space(6.0);
                }

                ui.weak("Pick a drum loop (click the grid to edit it), then give the bass and the guitar a source: Style (play each chord's root once, or click the lane to place roots; the band plays the chosen style on them) or Rec (record your own playing for one pass).");
              });
            });

            // the note chooser for a clicked spot of a roots lane
            if let Some((track, beat, at)) = pick {
                let mut done = false;
                let existing = ev
                    .roots_of(track)
                    .iter()
                .any(|r| (r.0 - beat).abs() < 0.3);
                egui::Area::new(egui::Id::unique("root_pick")).fixed_pos(at).order(egui::Order::Foreground).show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(format!("Root at bar {} beat {:.1}", (beat / bpb) as i32 + 1, beat % bpb + 1.0));
                        egui::Grid::new("notes").show(ui, |ui| {
                            for pc in 0..12u8 {
                                if ui.button(note_name(pc)).clicked() {
                                    cmds.push(BandCmd::SetRoot(track, beat, pc));
                                    done = true;
                                }
                                if pc % 4 == 3 {
                                    ui.end_row();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            if existing && ui.button("Remove").clicked() {
                                cmds.push(BandCmd::RemoveRoot(track, beat));
                                done = true;
                            }
                            if ui.button("Close").clicked() {
                                done = true;
                            }
                        });
                    });
                });
                if done || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                    pick = None;
                }
            }
            self.band_pick = pick;
            self.band_view = view;
            if !open {
                self.band_open = false;
            }
            if b != self.band {
                self.band = b;
                self.send(Cmd::Band(BandCmd::Settings(b)));
                self.sync_listening();
            }
            for c in cmds {
                self.send(Cmd::Band(c));
            }
            ctx.request_repaint_after(Duration::from_millis(40));
        }

        egui::CentralPanel::default().show(root, |ui| {
            if self.chain.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("The chain is empty — add an effect from the panel on the right.");
                });
                return;
            }
            let n = self.chain.len();
            egui::ScrollArea::vertical().show(ui, |ui| {
                if self.dragging.is_some() {
                    // Scroll when the card is held near the top or bottom edge.
                    if let Some(p) = ctx.pointer_interact_pos() {
                        let view = ui.clip_rect();
                        let speed = |d: f32| ((40.0 - d) / 40.0).clamp(0.0, 1.0) * 14.0;
                        let dy = speed(p.y - view.top()) - speed(view.bottom() - p.y);
                        if dy != 0.0 {
                            ui.scroll_with_delta(egui::vec2(0.0, dy));
                        }
                    }
                    ctx.request_repaint();
                }
                let dragged_color = self.dragging.map(|d| theme::accent(self.chain[d.min(n - 1)].spec.def.name));
                for i in 0..=n {
                    // The slot before card i: always the same height, lit up when it is the target.
                    let (slot, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), GAP), egui::Sense::hover());
                    new_gaps.push(slot);
                    if let (Some(c), true) = (dragged_color, drop_slot == Some(i)) {
                        let inner = slot.shrink2(egui::vec2(0.0, 1.0));
                        ui.painter().rect_filled(inner, 6.0, theme::tint(c, 90));
                        ui.painter().rect_stroke(inner, 6.0, egui::Stroke::new(2.0, c), egui::StrokeKind::Inside);
                    } else if dragged_color.is_some() && self.dragging != Some(i) && self.dragging.map(|d| d + 1) != Some(i) {
                        let y = slot.center().y;
                        ui.painter().line_segment(
                            [egui::pos2(slot.left() + 12.0, y), egui::pos2(slot.right() - 12.0, y)],
                            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40)),
                        );
                    }
                    if i == n {
                        break;
                    }
                    let (id, title) = (self.chain[i].id, self.chain[i].spec.def.title);
                    let color = theme::accent(self.chain[i].spec.def.name);
                    let dim = self.chain[i].bypass;
                    let shown = if dim { theme::tint(color, 90) } else { color };
                    let lifted = self.dragging == Some(i);
                    let card = egui::Frame::new()
                        .fill(theme::tint(color, if dim { 10 } else { 26 }))
                        .stroke(egui::Stroke::new(1.0, theme::tint(color, 110)))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin { left: 12, right: 10, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            if lifted {
                                // The card stays where it is, faded: its spot is not given away.
                                ui.set_opacity(0.4);
                            }
                            ui.horizontal(|ui| {
                                let g = grip(ui, shown);
                                if g.dragged() {
                                    drag_now = Some(i);
                                }
                                if g.drag_stopped() {
                                    if let Some(k) = drop_slot {
                                        // Slot k sits before card k; after removing card i the index shifts.
                                        drop_at = Some((i, if k > i { k - 1 } else { k }));
                                    }
                                }
                                let mut on = !self.chain[i].bypass;
                                if ui.checkbox(&mut on, "").on_hover_text("Enable / bypass").changed() {
                                    acts.push(Act::Bypass(i));
                                }
                                theme::effect_icon(ui, self.chain[i].spec.def.name, shown, 28.0);
                                ui.label(egui::RichText::new(format!("{}. {title}", i + 1)).strong().size(16.0).color(shown));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("✕").on_hover_text("Remove").clicked() {
                                        acts.push(Act::Remove(i));
                                    }
                                    if self.running.is_some() {
                                        level_graph(ui, &self.chain[i].hist, shown);
                                    }
                                });
                            });
                            let np = self.chain[i].spec.def.params.len();
                            ui.add_enabled_ui(!self.chain[i].bypass, |ui| {
                                egui::Grid::new(format!("grid-{id}")).num_columns(2).show(ui, |ui| {
                                    for p in 0..np {
                                        let key = self.chain[i].spec.def.params[p].key;
                                        let cc = theme::control_accent(color, p);
                                        ui.horizontal(|ui| {
                                            theme::control_icon(ui, key, cc);
                                            ui.label(egui::RichText::new(key).color(cc));
                                        });
                                        let changed = ui
                                            .scope(|ui| {
                                                let v = ui.visuals_mut();
                                                v.selection.bg_fill = cc;
                                                v.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, cc);
                                                v.widgets.active.bg_stroke = egui::Stroke::new(1.5, cc);
                                                v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, theme::tint(cc, 120));
                                                param_widget(ui, &format!("{id}"), &mut self.chain[i].spec, p)
                                            })
                                            .inner;
                                        if changed {
                                            acts.push(Act::Param(i, p));
                                        }
                                        ui.end_row();
                                    }
                                });
                            });
                        });
                    let r = card.response.rect;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(r.left_top(), egui::pos2(r.left() + 6.0, r.bottom())),
                        egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
                        shown,
                    );
                }

            });
        });
        self.gap_rects = new_gaps;
        self.dragging = drag_now;
        if let (Some(i), Some(p)) = (self.dragging, ctx.pointer_interact_pos()) {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some(fx) = self.chain.get(i) {
                let color = theme::accent(fx.spec.def.name);
                egui::Area::new(egui::Id::unique("drag-ghost"))
                    .order(egui::Order::Tooltip)
                    .fixed_pos(p + egui::vec2(14.0, -16.0))
                    .interactable(false)
                    .show(&ctx, |ui| {
                        egui::Frame::new()
                            .fill(theme::tint(color, 235))
                            .corner_radius(8.0)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .shadow(egui::Shadow { offset: [0, 3], blur: 12, spread: 0, color: egui::Color32::from_black_alpha(120) })
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    theme::effect_icon(ui, fx.spec.def.name, egui::Color32::WHITE, 22.0);
                                    ui.label(egui::RichText::new(fx.spec.def.title).strong().color(egui::Color32::WHITE));
                                });
                            });
                    });
            }
        }
        if let Some((from, to)) = drop_at {
            acts.push(Act::MoveTo(from, to));
        }

        if gains_changed {
            self.send(Cmd::Gains { input_db: self.input_db, output_db: self.output_db });
        }
        for act in acts {
            self.apply(act);
        }

        if self.last_save.elapsed() > Duration::from_millis(700) {
            self.save_session();
        }
    }

    fn on_exit(&mut self) {
        self.save_session();
    }
}

fn device_combo(ui: &mut egui::Ui, salt: &str, devices: &[String], sel: &mut Option<usize>) {
    let text = sel.map_or("System default".to_string(), |i| devices[i].clone());
    egui::ComboBox::from_id_salt(salt).width(170.0).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(sel, None, "System default");
        for (i, d) in devices.iter().enumerate() {
            ui.selectable_value(sel, Some(i), d);
        }
    });
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 720.0]).with_title("guitar-fx"),
        ..Default::default()
    };
    eframe::run_native("guitar-fx", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}

