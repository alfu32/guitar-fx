//! Graphical front end. Every control talks to the running audio chain through
//! `fxcore::chain::Control`, so changes are heard within one audio buffer.

#[path = "fxchain-ui/theme.rs"]
mod theme;

use eframe::egui;
use fxcore::chain::{Cmd, Slot};
use fxcore::effects::build;
use fxcore::live::{self, LiveOpts, Running, DEFAULT_BUFFER};
use fxcore::params::{EffectDef, Kind, Spec, Value, EFFECTS};
use fxcore::preset::Preset;
use std::time::Duration;

const BUFFERS: [u32; 6] = [32, 64, 128, 256, 512, 1024];

struct Fx {
    id: u64,
    spec: Spec,
    bypass: bool,
}

enum Act {
    Param(usize, usize),
    Bypass(usize),
    Remove(usize),
    Move(usize, isize),
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
    preset_name: String,
    presets: Vec<String>,
    status: String,
    shown_in: f32,
    shown_out: f32,
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
            preset_name: "my-preset".into(),
            presets: Vec::new(),
            status: String::new(),
            shown_in: 0.0,
            shown_out: 0.0,
        };
        app.scan_presets();
        for name in ["compressor", "overdrive", "reverb"] {
            app.add_local(fxcore::params::effect_def(name).unwrap());
        }
        app
    }

    fn scan_presets(&mut self) {
        self.presets = std::fs::read_dir("presets")
            .map(|d| {
                let mut v: Vec<String> = d
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n.ends_with(".fx"))
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default();
    }

    fn sample_rate(&self) -> f32 {
        self.running.as_ref().map_or(48000.0, |r| r.control.sample_rate)
    }

    fn add_local(&mut self, def: &'static EffectDef) -> usize {
        self.chain.push(Fx { id: self.next_id, spec: Spec::new(def), bypass: false });
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
            Act::Move(i, d) => {
                let to = (i as isize + d).clamp(0, self.chain.len() as isize - 1) as usize;
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

    fn load_preset(&mut self, name: &str) {
        match Preset::load(&format!("presets/{name}")) {
            Ok(p) => {
                for fx in std::mem::take(&mut self.chain) {
                    self.send(Cmd::Remove { id: fx.id });
                }
                for spec in p.specs {
                    let sr = self.sample_rate();
                    let id = self.next_id;
                    self.next_id += 1;
                    let slot = Slot::new(id, &spec, sr);
                    self.send(Cmd::Insert { index: self.chain.len(), slot });
                    self.chain.push(Fx { id, spec, bypass: false });
                }
                self.input_db = p.input_db;
                self.output_db = p.output_db;
                self.send(Cmd::Gains { input_db: p.input_db, output_db: p.output_db });
                self.preset_name = name.trim_end_matches(".fx").to_string();
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn save_preset(&mut self) {
        let _ = std::fs::create_dir_all("presets");
        let preset = Preset {
            specs: self.chain.iter().filter(|f| !f.bypass).map(|f| f.spec.clone()).collect(),
            input_db: self.input_db,
            output_db: self.output_db,
            buffer: None,
        };
        let path = format!("presets/{}.fx", self.preset_name.trim());
        match preset.save(&path) {
            Ok(()) => {
                self.error = None;
                self.status = format!("saved {path}");
                self.scan_presets();
            }
            Err(e) => self.error = Some(e),
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

        egui::Panel::left("side").resizable(false).default_size(190.0).show(root, |ui| {
            ui.heading("Add effect");
            for def in EFFECTS {
                let color = theme::accent(def.name);
                ui.horizontal(|ui| {
                    theme::effect_icon(ui, def.name, color, 22.0);
                    let label = egui::RichText::new(def.title).color(color).strong();
                    let btn = egui::Button::new(label).fill(theme::tint(color, 28)).stroke(egui::Stroke::new(1.0, theme::tint(color, 120)));
                    if ui.add_sized([138.0, 22.0], btn).clicked() {
                        acts.push(Act::Add(def));
                    }
                });
            }
            ui.separator();
            ui.heading("Presets");
            let mut to_load = None;
            for name in &self.presets {
                if ui.add_sized([170.0, 22.0], egui::Button::new(name.trim_end_matches(".fx"))).clicked() {
                    to_load = Some(name.clone());
                }
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.preset_name).desired_width(110.0));
                if ui.button("Save").clicked() {
                    self.save_preset();
                }
            });
            if let Some(name) = to_load {
                self.load_preset(&name);
            }
        });

        egui::CentralPanel::default().show(root, |ui| {
            if self.chain.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label("The chain is empty — add an effect on the left.");
                });
                return;
            }
            let n = self.chain.len();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in 0..n {
                    let (id, title) = (self.chain[i].id, self.chain[i].spec.def.title);
                    let color = theme::accent(self.chain[i].spec.def.name);
                    let dim = self.chain[i].bypass;
                    let shown = if dim { theme::tint(color, 90) } else { color };
                    let card = egui::Frame::new()
                        .fill(theme::tint(color, if dim { 10 } else { 26 }))
                        .stroke(egui::Stroke::new(1.0, theme::tint(color, 110)))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin { left: 16, right: 10, top: 8, bottom: 8 })
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
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
                                    if ui.add_enabled(i + 1 < n, egui::Button::new("▼").small()).clicked() {
                                        acts.push(Act::Move(i, 1));
                                    }
                                    if ui.add_enabled(i > 0, egui::Button::new("▲").small()).clicked() {
                                        acts.push(Act::Move(i, -1));
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
                    ui.add_space(4.0);
                }
            });
        });

        if gains_changed {
            self.send(Cmd::Gains { input_db: self.input_db, output_db: self.output_db });
        }
        for act in acts {
            self.apply(act);
        }
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
        viewport: egui::ViewportBuilder::default().with_inner_size([980.0, 720.0]).with_title("fxchain"),
        ..Default::default()
    };
    eframe::run_native("fxchain", options, Box::new(|_cc| Ok(Box::new(App::new()))))
}
