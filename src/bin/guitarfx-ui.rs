//! Graphical front end. Every control talks to the running audio chain through
//! `guitarfx_core::chain::Control`, so changes are heard within one audio buffer.

#[path = "guitarfx-ui/theme.rs"]
mod theme;

use eframe::egui;
use guitarfx_core::chain::{Cmd, Slot};
use guitarfx_core::effects::build;
use guitarfx_core::live::{self, LiveOpts, Running, DEFAULT_BUFFER};
use guitarfx_core::params::{EffectDef, Kind, Spec, Value, EFFECTS};
use guitarfx_core::preset::{Preset, BUILTIN};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const BUFFERS: [u32; 6] = [32, 64, 128, 256, 512, 1024];

const HISTORY: usize = 72;
/// Height of the always-present drop slot between cards; constant so nothing shifts while dragging.
const GAP: f32 = 18.0;

/// Where the session and the user's own chains live.
fn config_dir() -> PathBuf {
    let dir = config_base().join("guitarfx");
    // The program was called fxchain before it was renamed; carry the old settings over.
    let old = config_base().join("fxchain");
    if !dir.exists() && old.exists() {
        let _ = std::fs::rename(&old, &dir);
    }
    dir
}

fn config_base() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home)
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
    };
    base
}

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
    status: String,
    shown_in: f32,
    shown_out: f32,
    /// The reserved drop slots (one before every card and one after the last), from the last frame.
    gap_rects: Vec<egui::Rect>,
    dragging: Option<usize>,
    last_hist: Instant,
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
            status: String::new(),
            shown_in: 0.0,
            shown_out: 0.0,
            gap_rects: Vec::new(),
            dragging: None,
            last_hist: Instant::now(),
            saved_session: String::new(),
            last_save: Instant::now(),
        };
        app.scan_chains();
        if !app.restore_session() {
            for name in ["compressor", "overdrive", "reverb"] {
                app.add_local(guitarfx_core::params::effect_def(name).unwrap());
            }
        }
        app.saved_session = app.session_text();
        app
    }

    fn chains_dir() -> PathBuf {
        config_dir().join("chains")
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
                self.status = format!("{} → {}", running.input_name, running.output_name);
                self.error = None;
                self.running = Some(running);
            }
            Err(e) => self.error = Some(e),
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
                }
            });
            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), e);
            } else if !self.status.is_empty() {
                ui.label(&self.status);
            }
            ui.add_space(4.0);
        });

        let mut load_builtin: Option<usize> = None;
        let mut load_user: Option<String> = None;
        let mut delete_user: Option<String> = None;
        egui::Panel::left("mine").resizable(false).default_size(230.0).show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("My chains");
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
            });
        });
        egui::Panel::right("tools").resizable(false).default_size(270.0).show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Add effect");
                egui::Grid::new("add-grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                    for (k, def) in EFFECTS.iter().enumerate() {
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
        if let Some(name) = load_user {
            self.load_user_chain(&name);
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
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 720.0]).with_title("guitarfx"),
        ..Default::default()
    };
    eframe::run_native("guitarfx", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
