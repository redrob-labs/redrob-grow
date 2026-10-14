//! The egui screens. Data lives in `sim`, `remote` and `wizard`; this file only draws it.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Ui};
use grow_agent::Report;
use grow_hal::{Quantity, Switch};
use grow_recipe::{Band, builtin};

use crate::remote::Remote;
use crate::sim::{Season, Sim};
use crate::wizard::{RigKind, STEPS, Wizard};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Shelf,
    Setup,
    Recipes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Sim,
    Remote,
}

pub struct App {
    tab: Tab,
    source: Source,
    sim: Sim,
    sim_recipe: String,
    remote: Option<Remote>,
    remote_url: String,
    wizard: Wizard,
    last_frame: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            tab: Tab::Shelf,
            source: Source::Sim,
            sim: Sim::new(builtin::by_id("lettuce-butterhead").expect("builtin"), Season::Summer),
            sim_recipe: "lettuce-butterhead".into(),
            remote: None,
            remote_url: "http://127.0.0.1:7474".into(),
            wizard: Wizard::default(),
            last_frame: Instant::now(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f64().min(0.5);
        self.last_frame = now;
        if self.source == Source::Sim {
            self.sim.advance(dt);
        }
        if let Some(r) = &mut self.remote {
            r.pump();
        }
        ctx.request_repaint_after(Duration::from_millis(200));

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Redrob Grow").strong().size(18.0));
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Shelf, "Shelf");
                ui.selectable_value(&mut self.tab, Tab::Setup, "Setup");
                ui.selectable_value(&mut self.tab, Tab::Recipes, "Recipes");
            });
        });

        match self.tab {
            Tab::Shelf => self.shelf(ui),
            Tab::Setup => self.setup(ui),
            Tab::Recipes => self.recipes(ui),
        }
    }
}

fn band_color(v: f64, b: &Band) -> Color32 {
    if b.contains(v) {
        Color32::from_rgb(76, 175, 80)
    } else if (b.error(v)).abs() < (b.max - b.min).max(0.5) * 0.5 {
        Color32::from_rgb(255, 179, 0)
    } else {
        Color32::from_rgb(229, 57, 53)
    }
}

/// Where `v` sits on a bar that spans one band width on either side of the band.
fn band_fraction(v: f64, b: &Band) -> f32 {
    let w = (b.max - b.min).max(0.5);
    (((v - (b.min - w)) / (3.0 * w)).clamp(0.0, 1.0)) as f32
}

fn fmt_q(q: Quantity, v: f64) -> String {
    match q {
        Quantity::Co2Ppm | Quantity::PpfdUmol => format!("{v:.0} {}", q.unit()),
        Quantity::EcMs | Quantity::Ph => format!("{v:.2} {}", q.unit()),
        _ => format!("{v:.1} {}", q.unit()),
    }
}

fn label_q(q: Quantity) -> &'static str {
    match q {
        Quantity::AirTempC => "Air",
        Quantity::WaterTempC => "Water",
        Quantity::HumidityPct => "Humidity",
        Quantity::Co2Ppm => "CO2",
        Quantity::EcMs => "EC",
        Quantity::Ph => "pH",
        Quantity::WaterLevelPct => "Level",
        Quantity::PpfdUmol => "Light",
    }
}

fn label_s(s: Switch) -> &'static str {
    match s {
        Switch::Light => "Light",
        Switch::Heater => "Heater",
        Switch::Fan => "Fan",
        Switch::Pump => "Pump",
        Switch::Humidifier => "Humidifier",
        Switch::DoseNutrient => "Nutrient",
        Switch::DosePhDown => "pH down",
        Switch::TopUp => "Top-up",
    }
}

impl App {
    fn shelf(&mut self, ui: &mut Ui) {
        egui::Panel::left("source").default_size(260.0).show(ui, |ui| {
            ui.heading("Source");
            ui.radio_value(&mut self.source, Source::Sim, "Simulated office shelf");
            ui.radio_value(&mut self.source, Source::Remote, "grow-node over HTTP");
            ui.separator();
            match self.source {
                Source::Sim => {
                    let mut changed = false;
                    egui::ComboBox::from_label("Recipe").selected_text(&self.sim_recipe).show_ui(ui, |ui| {
                        for id in Wizard::recipe_ids() {
                            changed |= ui.selectable_value(&mut self.sim_recipe, id.clone(), id).changed();
                        }
                    });
                    let mut season = self.sim.season;
                    ui.horizontal(|ui| {
                        changed |= ui.selectable_value(&mut season, Season::Summer, "Summer").changed();
                        changed |= ui.selectable_value(&mut season, Season::Winter, "Winter").changed();
                    });
                    ui.add(egui::Slider::new(&mut self.sim.speed, 60..=86_400).logarithmic(true).text("sim s / real s"));
                    if changed || ui.button("Restart grow").clicked() {
                        self.sim.reset(&self.sim_recipe, season);
                    }
                    let t = self.sim.now();
                    ui.label(format!(
                        "Sim clock: day {} {}  {:02}:{:02}",
                        t.day(),
                        if t.is_weekday() { "weekday" } else { "weekend" },
                        t.hour(),
                        (t.0 % 3600) / 60
                    ));
                    ui.label(format!("Energy so far: {:.2} kWh", self.sim.kwh()));
                }
                Source::Remote => {
                    ui.text_edit_singleline(&mut self.remote_url);
                    if ui.button("Connect").clicked() {
                        self.remote = Some(Remote::start(self.remote_url.clone(), Duration::from_secs(5)));
                    }
                    match &self.remote {
                        None => ui.label("Not connected."),
                        Some(r) => match (&r.last_error, &r.last) {
                            (Some(e), _) => ui.colored_label(Color32::from_rgb(229, 57, 53), e),
                            (None, Some(s)) => ui.label(format!("{} on {} ({} cycles)", s.node, s.rig, s.cycles)),
                            (None, None) => ui.label("Waiting for first /state..."),
                        },
                    };
                }
            }
        });

        let (report, history): (Option<Report>, Vec<Report>) = match self.source {
            Source::Sim => (self.sim.latest().cloned(), self.sim.history.clone()),
            Source::Remote => match self.remote.as_ref().and_then(|r| r.last.as_ref()) {
                Some(s) => (s.latest.clone(), s.history.clone()),
                None => (None, Vec::new()),
            },
        };

        egui::CentralPanel::default().show(ui, |ui| {
            let Some(r) = report else {
                ui.label("No data yet.");
                return;
            };
            let obs = &r.observation;
            ui.horizontal(|ui| {
                ui.label(RichText::new(&obs.recipe_id).strong().size(16.0));
                ui.label(format!("day {}", obs.clock.day));
                ui.label(RichText::new(format!("stage: {}", obs.stage.name)).italics());
                if obs.recipe_done {
                    ui.colored_label(Color32::from_rgb(76, 175, 80), "ready to harvest");
                }
            });
            if !obs.stage.notes.is_empty() {
                ui.label(RichText::new(&obs.stage.notes).weak());
            }
            ui.separator();

            egui::Grid::new("gauges").num_columns(4).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
                for (q, band) in &obs.stage.targets {
                    ui.label(label_q(*q));
                    match obs.snapshot.get(*q) {
                        Some(v) => {
                            ui.label(RichText::new(fmt_q(*q, v)).strong());
                            ui.add(egui::ProgressBar::new(band_fraction(v, band)).fill(band_color(v, band)).desired_width(220.0));
                        }
                        None => {
                            ui.label("--");
                            ui.label("no sensor");
                        }
                    }
                    ui.label(RichText::new(format!("{:.1} .. {:.1}", band.min, band.max)).weak());
                    ui.end_row();
                }
            });
            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                ui.label("Switches:");
                for s in &obs.switches {
                    let on = obs.snapshot.is_on(*s);
                    let col = if on { Color32::from_rgb(76, 175, 80) } else { Color32::GRAY };
                    ui.colored_label(col, format!("{} {}", if on { "●" } else { "○" }, label_s(*s)));
                }
            });
            ui.separator();

            ui.columns(2, |cols| {
                cols[0].heading("Agent decided");
                for (cmd, why) in r.proposal.commands.iter().zip(&r.proposal.reasons) {
                    cols[0].label(format!(
                        "{} {} {}  -- {}",
                        if cmd.on { "on " } else { "off" },
                        label_s(cmd.switch),
                        if cmd.on { format!("{}s", cmd.for_secs) } else { String::new() },
                        why
                    ));
                }
                if r.proposal.commands.is_empty() {
                    cols[0].label(RichText::new("nothing to do").weak());
                }
                cols[1].heading("Guard");
                for a in &r.verdict.alarms {
                    cols[1].colored_label(
                        Color32::from_rgb(229, 57, 53),
                        format!("ALARM {} {} outside {:.1}..{:.1}", label_q(a.quantity), fmt_q(a.quantity, a.value), a.band.min, a.band.max),
                    );
                }
                for rej in &r.verdict.rejected {
                    cols[1].colored_label(Color32::from_rgb(255, 179, 0), format!("{rej:?}"));
                }
                for (cmd, e) in &r.failed {
                    cols[1].colored_label(Color32::from_rgb(229, 57, 53), format!("{} failed: {e}", label_s(cmd.switch)));
                }
                if r.verdict.alarms.is_empty() && r.verdict.rejected.is_empty() && r.failed.is_empty() {
                    cols[1].label(RichText::new("all proposals passed").weak());
                }
            });
            ui.separator();
            ui.heading("Last 48 h");
            chart(ui, &history, &obs.stage.targets);
        });
    }

    fn setup(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Set up a shelf");
            ui.horizontal(|ui| {
                for (i, name) in STEPS.iter().enumerate() {
                    let txt = format!("{}. {name}", i + 1);
                    if i == self.wizard.step {
                        ui.label(RichText::new(txt).strong());
                    } else {
                        ui.label(RichText::new(txt).weak());
                    }
                    if i + 1 < STEPS.len() {
                        ui.label("›");
                    }
                }
            });
            ui.separator();
            let w = &mut self.wizard;
            match w.step {
                0 => {
                    egui::Grid::new("w0").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label("Shelf name");
                        ui.text_edit_singleline(&mut w.name);
                        ui.end_row();
                        ui.label("Recipe");
                        egui::ComboBox::from_id_salt("w0r").selected_text(&w.recipe_id).show_ui(ui, |ui| {
                            for id in Wizard::recipe_ids() {
                                ui.selectable_value(&mut w.recipe_id, id.clone(), id);
                            }
                        });
                        ui.end_row();
                        ui.label("Sown on (YYYY-MM-DD)");
                        ui.text_edit_singleline(&mut w.sown_on);
                        ui.end_row();
                    });
                }
                1 => {
                    ui.radio_value(&mut w.rig, RigKind::Sim, "No hardware yet: simulated office shelf");
                    ui.radio_value(&mut w.rig, RigKind::Mycodo, "Raspberry Pi running Mycodo");
                    ui.add_space(8.0);
                    match w.rig {
                        RigKind::Sim => {
                            ui.checkbox(&mut w.season_winter, "Winter office (cold at night) instead of summer (hot at night)");
                        }
                        RigKind::Mycodo => {
                            egui::Grid::new("w1").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                ui.label("Mycodo address");
                                ui.text_edit_singleline(&mut w.mycodo_url);
                                ui.end_row();
                                ui.label("API key");
                                ui.add(egui::TextEdit::singleline(&mut w.mycodo_key).password(true));
                                ui.end_row();
                                ui.label("Self-signed certificate");
                                ui.checkbox(&mut w.mycodo_insecure, "accept (normal on a LAN)");
                                ui.end_row();
                            });
                        }
                    }
                }
                2 => {
                    if w.rig == RigKind::Sim {
                        ui.label("The simulated shelf has every switch and sensor. Nothing to wire.");
                    } else {
                        ui.label("Paste each Mycodo unique id. Leave blank for anything the shelf does not have.");
                        ui.add_space(6.0);
                        ui.strong("Sensors (Mycodo Inputs)");
                        egui::Grid::new("w2i").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                            for (q, id, unit) in &mut w.inputs {
                                ui.label(label_q(*q));
                                ui.text_edit_singleline(id);
                                ui.label(RichText::new(format!("unit {unit}")).weak());
                                ui.end_row();
                            }
                        });
                        ui.add_space(6.0);
                        ui.strong("Switches (Mycodo Outputs)");
                        egui::Grid::new("w2o").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                            for (s, id) in &mut w.outputs {
                                ui.label(label_s(*s));
                                ui.text_edit_singleline(id);
                                ui.end_row();
                            }
                        });
                    }
                }
                _ => {
                    ui.label("This is the file grow-node will read:");
                    let text = w.to_toml();
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut text.as_str()).code_editor().desired_width(f32::INFINITY));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Save as");
                        ui.text_edit_singleline(&mut w.save_path);
                        if ui.button("Save").clicked() {
                            w.save();
                        }
                    });
                    match &w.saved {
                        Some(Ok(p)) => {
                            ui.colored_label(Color32::from_rgb(76, 175, 80), format!("Saved {p}. Now run:  grow-node run {p}"));
                        }
                        Some(Err(e)) => {
                            ui.colored_label(Color32::from_rgb(229, 57, 53), e);
                        }
                        None => {}
                    }
                }
            }
            ui.add_space(12.0);
            let problems = w.problems();
            for p in &problems {
                ui.colored_label(Color32::from_rgb(255, 179, 0), p);
            }
            ui.horizontal(|ui| {
                if w.step > 0 && ui.button("Back").clicked() {
                    w.step -= 1;
                }
                if w.step + 1 < STEPS.len() && ui.add_enabled(problems.is_empty(), egui::Button::new("Next")).clicked() {
                    w.step += 1;
                }
            });
        });
    }

    fn recipes(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for r in builtin::all() {
                    ui.heading(format!("{}  ({} {}, {} days)", r.id, r.crop, r.variety, r.days));
                    egui::Grid::new(format!("r-{}", r.id)).num_columns(4).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                        ui.strong("stage");
                        ui.strong("from day");
                        ui.strong("light");
                        ui.strong("targets");
                        ui.end_row();
                        for s in &r.stages {
                            ui.label(&s.name);
                            ui.label(s.from_day.to_string());
                            ui.label(format!("{}h from {:02}:00", s.light.hours, s.light.on_hour as u32));
                            ui.label(
                                s.targets.iter().map(|(q, b)| format!("{} {:.1}..{:.1}", label_q(*q), b.min, b.max)).collect::<Vec<_>>().join("   "),
                            );
                            ui.end_row();
                        }
                    });
                    for src in &r.sources {
                        ui.label(RichText::new(src).weak().small());
                    }
                    ui.add_space(12.0);
                }
            });
        });
    }
}

/// Air and water temperature over the stored history, with the current stage band shaded.
fn chart(ui: &mut Ui, history: &[Report], targets: &std::collections::BTreeMap<Quantity, Band>) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), 160.0), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, Color32::from_gray(28));
    let series = [(Quantity::AirTempC, Color32::from_rgb(100, 181, 246)), (Quantity::WaterTempC, Color32::from_rgb(255, 138, 101))];
    let n = history.len().min(288);
    let slice = &history[history.len() - n..];
    let (mut lo, mut hi) = (f64::MAX, f64::MIN);
    for r in slice {
        for (q, _) in &series {
            if let Some(v) = r.observation.snapshot.get(*q) {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    if let Some(b) = targets.get(&Quantity::WaterTempC) {
        lo = lo.min(b.min);
        hi = hi.max(b.max);
    }
    if lo >= hi || !lo.is_finite() || !hi.is_finite() {
        return;
    }
    let pad = (hi - lo) * 0.1;
    let (lo, hi) = (lo - pad, hi + pad);
    let y = |v: f64| rect.bottom() - ((v - lo) / (hi - lo)) as f32 * rect.height();
    if let Some(b) = targets.get(&Quantity::WaterTempC) {
        let band = egui::Rect::from_min_max(egui::pos2(rect.left(), y(b.max)), egui::pos2(rect.right(), y(b.min)));
        painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(76, 175, 80, 40));
    }
    for (q, col) in series {
        let pts: Vec<egui::Pos2> = slice
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.observation.snapshot.get(q).map(|v| egui::pos2(rect.left() + (i as f32 / (n.max(2) - 1) as f32) * rect.width(), y(v)))
            })
            .collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, egui::Stroke::new(1.5, col)));
        }
    }
    painter.text(rect.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, format!("{hi:.1}"), egui::FontId::proportional(11.0), Color32::GRAY);
    painter.text(
        rect.left_bottom() + egui::vec2(6.0, -4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{lo:.1}"),
        egui::FontId::proportional(11.0),
        Color32::GRAY,
    );
    painter.text(rect.right_top() + egui::vec2(-6.0, 4.0), egui::Align2::RIGHT_TOP, "air  water", egui::FontId::proportional(11.0), Color32::GRAY);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_colours_and_positions() {
        let b = Band::new(18.0, 22.0);
        assert_eq!(band_color(20.0, &b), Color32::from_rgb(76, 175, 80));
        assert_eq!(band_color(23.0, &b), Color32::from_rgb(255, 179, 0));
        assert_eq!(band_color(30.0, &b), Color32::from_rgb(229, 57, 53));
        assert!((band_fraction(18.0, &b) - 1.0 / 3.0).abs() < 1e-6);
        assert!((band_fraction(22.0, &b) - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!(band_fraction(-100.0, &b), 0.0);
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_q(Quantity::Co2Ppm, 812.6), "813 ppm");
        assert_eq!(fmt_q(Quantity::Ph, 6.0), "6.00 pH");
        assert_eq!(fmt_q(Quantity::AirTempC, 21.26), "21.3 C");
    }
}
