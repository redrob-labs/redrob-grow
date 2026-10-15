//! The egui screens, in the PhotoCraft shape: a menu bar, a thin tool strip on the left, the
//! canvas in the middle, a dock on the right that holds the **Agent** chat and the **Nodes** panel,
//! and a status bar. Data lives in `sim`, `remote` and `wizard`; this file only draws it.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Ui};
use grow_agent::Report;
use grow_hal::{Quantity, Switch};
use grow_recipe::{Band, builtin};

use crate::remote::{Discovery, Remote};
use crate::sim::{Season, Sim};
use crate::theme;
use crate::wizard::{PlannerKind, RigKind, STEPS, Wizard};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Shelf,
    Setup,
    Recipes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Sim,
    Nodes,
}

pub struct App {
    tab: Tab,
    source: Source,
    sim: Sim,
    sim_recipe: String,
    remotes: Vec<Remote>,
    /// Which node the Agent panel talks to.
    chat_node: usize,
    remote_url: String,
    discovery: Option<Discovery>,
    show_dock: bool,
    wizard: Wizard,
    last_frame: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, nodes: &[String]) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self {
            tab: Tab::Shelf,
            source: Source::Sim,
            sim: Sim::new(builtin::by_id("lettuce-butterhead").expect("builtin"), Season::Summer),
            sim_recipe: "lettuce-butterhead".into(),
            remotes: Vec::new(),
            chat_node: 0,
            remote_url: "http://127.0.0.1:7474".into(),
            discovery: None,
            show_dock: true,
            wizard: Wizard::default(),
            last_frame: Instant::now(),
        };
        for n in nodes {
            app.connect(n);
        }
        app
    }

    fn connect(&mut self, url: &str) {
        let url = url.trim().trim_end_matches('/');
        let full = if url.starts_with("http") { url.to_owned() } else { format!("http://{url}") };
        if self.remotes.iter().any(|r| r.url == full) {
            return;
        }
        self.remotes.push(Remote::start(full, Duration::from_secs(5)));
        self.chat_node = self.remotes.len() - 1;
        self.source = Source::Nodes;
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
        for r in &mut self.remotes {
            r.pump();
        }
        if let Some(d) = &mut self.discovery {
            d.pump();
        }
        if self.chat_node >= self.remotes.len() {
            self.chat_node = 0;
        }
        ctx.request_repaint_after(Duration::from_millis(200));

        self.menu_bar(ui);
        self.status_bar(ui);
        self.tool_strip(ui);
        if self.show_dock {
            self.dock(ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::CANVAS).inner_margin(egui::Margin::same(12))).show(ui, |ui| {
            match self.tab {
                Tab::Shelf => self.shelf(ui),
                Tab::Setup => self.setup(ui),
                Tab::Recipes => self.recipes(ui),
            }
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Chrome: menu bar, tool strip, status bar, dock

impl App {
    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::Panel::top("menu").frame(egui::Frame::new().fill(theme::CHROME).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label(RichText::new("Redrob Grow").strong());
                ui.add_space(8.0);
                ui.menu_button("Shelf", |ui| {
                    if ui.button("Simulated office shelf").clicked() {
                        self.source = Source::Sim;
                        self.tab = Tab::Shelf;
                        ui.close();
                    }
                    if ui.add_enabled(!self.remotes.is_empty(), egui::Button::new("Connected nodes")).clicked() {
                        self.source = Source::Nodes;
                        self.tab = Tab::Shelf;
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Find nodes on LAN").clicked() {
                        self.discovery = Some(Discovery::start(Duration::from_secs(6)));
                        self.show_dock = true;
                        ui.close();
                    }
                    if ui.button("Set up a new shelf...").clicked() {
                        self.tab = Tab::Setup;
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.selectable_label(self.tab == Tab::Shelf, "Shelf").clicked() {
                        self.tab = Tab::Shelf;
                        ui.close();
                    }
                    if ui.selectable_label(self.tab == Tab::Setup, "Setup").clicked() {
                        self.tab = Tab::Setup;
                        ui.close();
                    }
                    if ui.selectable_label(self.tab == Tab::Recipes, "Recipes").clicked() {
                        self.tab = Tab::Recipes;
                        ui.close();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_dock, "Agent and Nodes dock");
                });
                ui.menu_button("Help", |ui| {
                    ui.label(format!("Redrob Grow {}", env!("CARGO_PKG_VERSION")));
                    ui.hyperlink_to("Repository", "https://github.com/redrob-labs/redrob-grow");
                    ui.hyperlink_to("Roadmap", "https://github.com/redrob-labs/redrob-grow/blob/develop/docs/ROADMAP.ko.md");
                });
            });
        });
    }

    fn tool_strip(&mut self, ui: &mut Ui) {
        egui::Panel::left("tools")
            .exact_size(theme::TOOL_STRIP_WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::TAB_STRIP).inner_margin(egui::Margin::symmetric(6, 8)))
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    for (tab, glyph, tip) in [(Tab::Shelf, "▦", "Shelf"), (Tab::Setup, "⚙", "Setup"), (Tab::Recipes, "☰", "Recipes")] {
                        let selected = self.tab == tab;
                        let btn = egui::Button::new(RichText::new(glyph).size(16.0).color(if selected { Color32::WHITE } else { theme::TEXT_DIM }))
                            .fill(if selected { theme::ROW_SELECTED } else { Color32::TRANSPARENT })
                            .min_size(egui::vec2(32.0, 32.0));
                        if ui.add(btn).on_hover_text(tip).clicked() {
                            self.tab = tab;
                        }
                    }
                });
            });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let line = match self.source {
            Source::Sim => {
                let t = self.sim.now();
                format!(
                    "Simulated shelf  |  day {} {}  {:02}:{:02}  |  {:.2} kWh  |  rules planner",
                    t.day(),
                    if t.is_weekday() { "weekday" } else { "weekend" },
                    t.hour(),
                    (t.0 % 3600) / 60,
                    self.sim.kwh()
                )
            }
            Source::Nodes => match self.remotes.get(self.chat_node).and_then(|r| r.last.as_ref()) {
                Some(s) => format!(
                    "{}  |  {} cycles  |  {}{}",
                    s.node,
                    s.cycles,
                    s.planner,
                    if s.planner_status.is_empty() { String::new() } else { format!("  |  {}", s.planner_status) }
                ),
                None => format!("{} node(s), waiting...", self.remotes.len()),
            },
        };
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(theme::CHROME).inner_margin(egui::Margin::symmetric(10, 3))).show(ui, |ui| {
            ui.label(RichText::new(line).small().color(theme::TEXT_FAINT));
        });
    }

    fn dock(&mut self, ui: &mut Ui) {
        egui::Panel::right("dock").default_size(320.0).size_range(theme::DOCK_WIDTH).frame(egui::Frame::new().fill(theme::DOCK)).show(ui, |ui| {
            let total = ui.available_height();
            let nodes_h = (total * 0.36).clamp(150.0, 320.0);
            ui.vertical(|ui| {
                ui.set_min_height(total - nodes_h);
                ui.set_max_height(total - nodes_h);
                self.agent_panel(ui);
            });
            self.nodes_panel(ui);
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Dock panels

#[derive(Clone)]
struct Line {
    cycle: usize,
    who: Who,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Who {
    Agent,
    Question,
    Answered,
    User,
}

impl App {
    fn agent_panel(&mut self, ui: &mut Ui) {
        theme::panel_title(ui, "AGENT");
        let inner = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6));
        inner.show(ui, |ui| {
            if self.source == Source::Sim || self.remotes.is_empty() {
                // The simulator has no model behind it: show the rules' reasoning read-only.
                ui.label(RichText::new("Simulated shelf: the rule planner. Its reasons, newest first.").small().color(theme::TEXT_FAINT));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt("sim-chat").auto_shrink([false, false]).show(ui, |ui| {
                    for r in self.sim.history.iter().rev().take(12) {
                        let text = if r.proposal.commands.is_empty() {
                            "nothing to do".to_owned()
                        } else {
                            r.proposal
                                .commands
                                .iter()
                                .zip(&r.proposal.reasons)
                                .map(|(c, w)| format!("{} {}: {w}", if c.on { "on" } else { "off" }, label_s(c.switch)))
                                .collect::<Vec<_>>()
                                .join("\n")
                        };
                        bubble(
                            ui,
                            Who::Agent,
                            &format!(
                                "day {} {:02}:{:02}",
                                r.observation.clock.day,
                                r.observation.clock.hour as u32,
                                ((r.observation.clock.hour % 1.0) * 60.0) as u32
                            ),
                            &text,
                        );
                    }
                });
                ui.add_space(4.0);
                ui.label(RichText::new("Start a grow-node with [planner] kind = \"engine\" to talk to the agent.").small().color(theme::TEXT_FAINT));
                return;
            }

            if self.remotes.len() > 1 {
                let names: Vec<String> =
                    self.remotes.iter().map(|r| r.last.as_ref().map(|s| s.node.clone()).unwrap_or_else(|| r.url.clone())).collect();
                egui::ComboBox::from_id_salt("chat-node").selected_text(&names[self.chat_node]).width(ui.available_width() - 16.0).show_ui(
                    ui,
                    |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut self.chat_node, i, n);
                        }
                    },
                );
                ui.add_space(4.0);
            }
            let remote = &mut self.remotes[self.chat_node];
            let Some(state) = remote.last.clone() else {
                ui.label(RichText::new(remote.last_error.clone().unwrap_or_else(|| "waiting for the node...".into())).color(theme::TEXT_FAINT));
                return;
            };
            ui.horizontal(|ui| {
                planner_badge(ui, &state.planner);
                if !state.planner_status.is_empty() {
                    ui.label(RichText::new(&state.planner_status).small().color(theme::TEXT_FAINT));
                }
            });

            let mut lines: Vec<Line> = Vec::new();
            for n in &state.notes {
                lines.push(Line { cycle: n.cycle, who: Who::Agent, text: format!("day {}: {}", n.day, n.text) });
            }
            for q in &state.questions {
                lines.push(Line {
                    cycle: q.cycle,
                    who: if q.answer.is_some() { Who::Answered } else { Who::Question },
                    text: format!("day {}: {}", q.day, q.text),
                });
            }
            for m in &state.operator {
                lines.push(Line { cycle: m.cycle, who: Who::User, text: m.text.clone() });
            }
            lines.sort_by_key(|l| (l.cycle, matches!(l.who, Who::User) as u8));

            let input_h = 64.0;
            let h = (ui.available_height() - input_h).max(60.0);
            egui::ScrollArea::vertical().id_salt(("chat", &remote.url)).auto_shrink([false, false]).max_height(h).stick_to_bottom(true).show(
                ui,
                |ui| {
                    if lines.is_empty() {
                        ui.label(
                            RichText::new("No notes yet. The agent writes here when it has something to say; you can write first.")
                                .color(theme::TEXT_FAINT),
                        );
                    }
                    for l in &lines {
                        bubble(ui, l.who, &format!("#{}", l.cycle), &l.text);
                    }
                },
            );
            ui.add_space(4.0);
            let open = state.questions.iter().filter(|q| q.answer.is_none()).count();
            ui.horizontal(|ui| {
                let hint = if open > 0 { format!("Answer the agent ({open} open)") } else { "Tell the agent something".to_owned() };
                let edit = egui::TextEdit::singleline(&mut remote.draft).hint_text(hint).desired_width(ui.available_width() - 58.0);
                let resp = ui.add(edit);
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let send = egui::Button::new(RichText::new("Send").color(Color32::WHITE)).fill(theme::ACCENT);
                if ui.add(send).clicked() || enter {
                    remote.send_answer();
                }
            });
            match &remote.answer_result {
                Some(Ok(())) => ui.label(RichText::new("delivered; read next cycle").small().color(theme::GOOD)),
                Some(Err(e)) => ui.label(RichText::new(e).small().color(theme::DANGER)),
                None => ui.label(RichText::new(" ").small()),
            };
        });
    }

    fn nodes_panel(&mut self, ui: &mut Ui) {
        let mut to_connect: Option<String> = None;
        let mut to_remove: Option<usize> = None;
        theme::panel_title(ui, "NODES");
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt("nodes").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.source, Source::Sim, "Simulated");
                    ui.add_enabled_ui(!self.remotes.is_empty(), |ui| {
                        ui.selectable_value(&mut self.source, Source::Nodes, "Nodes");
                    });
                });
                if self.source == Source::Sim {
                    let mut changed = false;
                    egui::ComboBox::from_id_salt("sim-recipe").selected_text(&self.sim_recipe).width(ui.available_width() - 16.0).show_ui(ui, |ui| {
                        for id in Wizard::recipe_ids() {
                            changed |= ui.selectable_value(&mut self.sim_recipe, id.clone(), id).changed();
                        }
                    });
                    let mut season = self.sim.season;
                    ui.horizontal(|ui| {
                        changed |= ui.selectable_value(&mut season, Season::Summer, "Summer").changed();
                        changed |= ui.selectable_value(&mut season, Season::Winter, "Winter").changed();
                        if ui.small_button("Restart").clicked() {
                            changed = true;
                        }
                    });
                    ui.add(egui::Slider::new(&mut self.sim.speed, 60..=86_400).logarithmic(true).text("x"));
                    if changed {
                        self.sim.reset(&self.sim_recipe, season);
                    }
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.remote_url).desired_width(ui.available_width() - 100.0));
                    if ui.button("Add").clicked() {
                        to_connect = Some(self.remote_url.clone());
                    }
                    let running = self.discovery.as_ref().is_some_and(|d| d.running);
                    if ui.add_enabled(!running, egui::Button::new("Find")).on_hover_text("Listen 6 s for nodes on the LAN").clicked() {
                        self.discovery = Some(Discovery::start(Duration::from_secs(6)));
                    }
                });
                if let Some(d) = &self.discovery {
                    if d.running {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("listening...").small().color(theme::TEXT_FAINT));
                        });
                    }
                    match &d.result {
                        Some(Ok(found)) if found.is_empty() => {
                            ui.label(RichText::new("No node heard.").small().color(theme::TEXT_FAINT));
                        }
                        Some(Ok(found)) => {
                            for f in found {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("{}  {}", f.name, f.listen)).small());
                                    if f.listen.starts_with("127.") && f.from != "127.0.0.1" {
                                        ui.label(RichText::new(format!("on {} (tunnel)", f.from)).small().color(theme::TEXT_FAINT));
                                    }
                                    if ui.small_button("Add").clicked() {
                                        to_connect = Some(f.listen.clone());
                                    }
                                });
                            }
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).small().color(theme::DANGER));
                        }
                        None => {}
                    }
                }
                for (i, r) in self.remotes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (dot, text) = match (&r.last_error, &r.last) {
                            (Some(e), _) => (theme::DANGER, format!("{}  {e}", r.url)),
                            (None, Some(s)) => (theme::GOOD, format!("{}  {}  {} cycles", s.node, s.rig, s.cycles)),
                            (None, None) => (theme::TEXT_FAINT, format!("{}  waiting", r.url)),
                        };
                        ui.colored_label(dot, "●");
                        let selected = self.source == Source::Nodes && self.chat_node == i;
                        if ui.selectable_label(selected, RichText::new(text).small()).clicked() {
                            self.chat_node = i;
                            self.source = Source::Nodes;
                        }
                        if ui.small_button("x").clicked() {
                            to_remove = Some(i);
                        }
                    });
                }
            });
        });
        if let Some(i) = to_remove {
            self.remotes.remove(i);
        }
        if let Some(u) = to_connect {
            self.connect(&u);
        }
    }
}

fn bubble(ui: &mut Ui, who: Who, stamp: &str, text: &str) {
    let (fill, align, label) = match who {
        Who::Agent => (theme::BUBBLE_AGENT, egui::Align::Min, "agent"),
        Who::Question => (theme::BUBBLE_AGENT, egui::Align::Min, "agent asks"),
        Who::Answered => (theme::BUBBLE_AGENT, egui::Align::Min, "agent asked"),
        Who::User => (theme::BUBBLE_USER, egui::Align::Max, "you"),
    };
    let w = ui.available_width() * 0.86;
    ui.with_layout(egui::Layout::top_down(align), |ui| {
        egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_max_width(w);
            ui.horizontal(|ui| {
                let col = if who == Who::Question { theme::WARNING } else { theme::TEXT_FAINT };
                ui.label(RichText::new(label).small().color(col));
                ui.label(RichText::new(stamp).small().color(theme::TEXT_FAINT));
            });
            ui.label(text);
        });
    });
    ui.add_space(2.0);
}

// ---------------------------------------------------------------------------------------------
// Helpers shared by the canvas views

fn band_color(v: f64, b: &Band) -> Color32 {
    if b.contains(v) {
        theme::GOOD
    } else if (b.error(v)).abs() < (b.max - b.min).max(0.5) * 0.5 {
        theme::WARNING
    } else {
        theme::DANGER
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

fn planner_badge(ui: &mut Ui, planner: &str) {
    let (text, col) = if planner.starts_with("engine") {
        ("agent", theme::ACCENT)
    } else if planner.contains("unavailable") {
        ("rules (fallback)", theme::WARNING)
    } else {
        ("rules", theme::TEXT_FAINT)
    };
    ui.colored_label(col, RichText::new(format!("[{text}]")).strong());
}

/// One cycle's readings, switches, decisions and guard verdict. `compact` is the side-by-side form.
fn report_panel(ui: &mut Ui, r: &Report, history: &[Report], compact: bool) {
    let obs = &r.observation;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(&obs.recipe_id).strong().size(16.0));
        ui.label(RichText::new(format!("day {}", obs.clock.day)).color(theme::TEXT_DIM));
        ui.label(RichText::new(format!("stage: {}", obs.stage.name)).italics().color(theme::TEXT_DIM));
        planner_badge(ui, &r.proposal.planner);
        if obs.recipe_done {
            ui.colored_label(theme::GOOD, "ready to harvest");
        }
    });
    if !compact && !obs.stage.notes.is_empty() {
        ui.label(RichText::new(&obs.stage.notes).color(theme::TEXT_FAINT));
    }
    ui.separator();

    let bar_w = if compact { 120.0 } else { 220.0 };
    egui::Grid::new(("gauges", compact)).num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
        for (q, band) in &obs.stage.targets {
            ui.label(RichText::new(label_q(*q)).color(theme::TEXT_DIM));
            match obs.snapshot.get(*q) {
                Some(v) => {
                    ui.label(RichText::new(fmt_q(*q, v)).strong());
                    ui.add(egui::ProgressBar::new(band_fraction(v, band)).fill(band_color(v, band)).desired_width(bar_w));
                }
                None => {
                    ui.label("--");
                    ui.label(RichText::new("no sensor").color(theme::TEXT_FAINT));
                }
            }
            ui.label(RichText::new(format!("{:.1} .. {:.1}", band.min, band.max)).color(theme::TEXT_FAINT));
            ui.end_row();
        }
    });
    ui.add_space(6.0);

    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Switches").color(theme::TEXT_DIM));
        for s in &obs.switches {
            let on = obs.snapshot.is_on(*s);
            let col = if on { theme::GOOD } else { theme::TEXT_FAINT };
            ui.colored_label(col, format!("{} {}", if on { "●" } else { "○" }, label_s(*s)));
        }
        if let Some(k) = obs.snapshot.energy_kwh {
            ui.label(RichText::new(format!("meter {k:.2} kWh")).color(theme::TEXT_FAINT));
        }
    });
    ui.separator();

    let decided = |ui: &mut Ui| {
        ui.label(RichText::new("Decided").strong());
        for (cmd, why) in r.proposal.commands.iter().zip(&r.proposal.reasons) {
            ui.label(format!(
                "{} {} {}  -- {}",
                if cmd.on { "on " } else { "off" },
                label_s(cmd.switch),
                if cmd.on { format!("{}s", cmd.for_secs) } else { String::new() },
                why
            ));
        }
        if r.proposal.commands.is_empty() {
            ui.label(RichText::new("nothing to do").color(theme::TEXT_FAINT));
        }
    };
    let guard = |ui: &mut Ui| {
        ui.label(RichText::new("Guard").strong());
        for a in &r.verdict.alarms {
            ui.colored_label(
                theme::DANGER,
                format!("ALARM {} {} outside {:.1}..{:.1}", label_q(a.quantity), fmt_q(a.quantity, a.value), a.band.min, a.band.max),
            );
        }
        for rej in &r.verdict.rejected {
            ui.colored_label(theme::WARNING, format!("{rej:?}"));
        }
        for (cmd, e) in &r.failed {
            ui.colored_label(theme::DANGER, format!("{} failed: {e}", label_s(cmd.switch)));
        }
        if r.verdict.alarms.is_empty() && r.verdict.rejected.is_empty() && r.failed.is_empty() {
            ui.label(RichText::new("all proposals passed").color(theme::TEXT_FAINT));
        }
    };
    if compact {
        decided(ui);
        ui.add_space(4.0);
        guard(ui);
    } else {
        ui.columns(2, |cols| {
            decided(&mut cols[0]);
            guard(&mut cols[1]);
        });
    }
    ui.separator();
    ui.label(RichText::new(if compact { "48 h" } else { "Last 48 h" }).strong());
    chart(ui, history, &obs.stage.targets, if compact { 110.0 } else { 160.0 });
}

/// Summary line under a node's report: in-band, alarms, refused, energy.
fn summary_line(ui: &mut Ui, remote: &Remote) {
    if let Some(s) = remote.last.as_ref().and_then(|s| s.summary.as_ref()) {
        let energy = match s.kwh_measured {
            Some(m) => format!("{m:.2} kWh measured ({:.2} est.)", s.kwh_estimate),
            None => format!("{:.2} kWh est.", s.kwh_estimate),
        };
        ui.label(
            RichText::new(format!("in-band {:.0}%   alarms {}   refused {}   {energy}", s.in_band_ratio * 100.0, s.alarms, s.rejected))
                .small()
                .color(theme::TEXT_FAINT),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Canvas views

impl App {
    fn shelf(&mut self, ui: &mut Ui) {
        match self.source {
            Source::Sim => match self.sim.latest().cloned() {
                Some(r) => {
                    theme::card(ui, |ui| report_panel(ui, &r, &self.sim.history, false));
                }
                None => {
                    ui.label("No data yet.");
                }
            },
            Source::Nodes => {
                if self.remotes.is_empty() {
                    ui.label(RichText::new("Add a grow-node in the Nodes panel, or press Find.").color(theme::TEXT_FAINT));
                    return;
                }
                if self.remotes.len() == 1 {
                    let remote = &self.remotes[0];
                    egui::ScrollArea::vertical().show(ui, |ui| match remote.last.as_ref().and_then(|s| s.latest.clone()) {
                        Some(r) => {
                            let history = remote.last.as_ref().map(|s| s.history.clone()).unwrap_or_default();
                            theme::card(ui, |ui| {
                                report_panel(ui, &r, &history, false);
                                summary_line(ui, remote);
                            });
                        }
                        None => {
                            ui.label(remote.last_error.clone().unwrap_or_else(|| "Waiting for first /state...".into()));
                        }
                    });
                    return;
                }
                let n = self.remotes.len().min(3);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(n, |cols| {
                        for (col, remote) in cols.iter_mut().zip(self.remotes.iter()) {
                            theme::card(col, |col| {
                                let title = remote.last.as_ref().map(|s| s.node.clone()).unwrap_or_else(|| remote.url.clone());
                                col.heading(title);
                                match remote.last.as_ref().and_then(|s| s.latest.clone()) {
                                    Some(r) => {
                                        let history = remote.last.as_ref().map(|s| s.history.clone()).unwrap_or_default();
                                        report_panel(col, &r, &history, true);
                                        summary_line(col, remote);
                                    }
                                    None => {
                                        col.label(remote.last_error.clone().unwrap_or_else(|| "waiting...".into()));
                                    }
                                }
                            });
                        }
                    });
                    if self.remotes.len() > 3 {
                        ui.label(RichText::new("Showing the first three nodes.").color(theme::TEXT_FAINT));
                    }
                });
            }
        }
    }

    fn setup(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            theme::card(ui, |ui| {
                ui.heading("Set up a shelf");
                ui.horizontal(|ui| {
                    for (i, name) in STEPS.iter().enumerate() {
                        let txt = format!("{}. {name}", i + 1);
                        if i == self.wizard.step {
                            ui.label(RichText::new(txt).strong().color(theme::ACCENT));
                        } else {
                            ui.label(RichText::new(txt).color(theme::TEXT_FAINT));
                        }
                        if i + 1 < STEPS.len() {
                            ui.label(RichText::new("›").color(theme::TEXT_FAINT));
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
                        ui.radio_value(&mut w.rig, RigKind::Tasmota, "Tasmota smart plugs (and an optional Tasmota sensor node)");
                        ui.radio_value(&mut w.rig, RigKind::Mycodo, "Raspberry Pi running Mycodo");
                        ui.add_space(8.0);
                        match w.rig {
                            RigKind::Sim => {
                                ui.checkbox(&mut w.season_winter, "Winter office (cold at night) instead of summer (hot at night)");
                            }
                            RigKind::Tasmota => {
                                ui.label(RichText::new("Cheapest path: one metering plug per switch, no soldering. Plugs turn themselves off when the agent's timer runs out.").color(theme::TEXT_DIM));
                                egui::Grid::new("w1t").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label("Web user (if set)");
                                    ui.text_edit_singleline(&mut w.tasmota_user);
                                    ui.end_row();
                                    ui.label("Web password");
                                    ui.add(egui::TextEdit::singleline(&mut w.tasmota_password).password(true));
                                    ui.end_row();
                                });
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
                    2 => match w.rig {
                        RigKind::Sim => {
                            ui.label("The simulated shelf has every switch and sensor. Nothing to wire.");
                        }
                        RigKind::Tasmota => {
                            ui.label("One plug per switch. Leave the host blank for anything the shelf does not have.");
                            ui.add_space(6.0);
                            ui.strong("Plugs");
                            egui::Grid::new("w2p").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
                                ui.label("");
                                ui.label(RichText::new("host").color(theme::TEXT_FAINT));
                                ui.label(RichText::new("relay").color(theme::TEXT_FAINT));
                                ui.label(RichText::new("meters").color(theme::TEXT_FAINT));
                                ui.end_row();
                                for (s, (host, relay, meters)) in &mut w.plugs {
                                    ui.label(label_s(*s));
                                    ui.text_edit_singleline(host);
                                    ui.add(egui::DragValue::new(relay).range(1..=8));
                                    ui.checkbox(meters, "");
                                    ui.end_row();
                                }
                            });
                            ui.add_space(6.0);
                            ui.strong("Sensors (a Tasmota node's Status 10 path)");
                            egui::Grid::new("w2s").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                                for (q, host, path) in &mut w.tasmota_sensors {
                                    ui.label(label_q(*q));
                                    ui.text_edit_singleline(host);
                                    ui.text_edit_singleline(path);
                                    ui.end_row();
                                }
                            });
                        }
                        RigKind::Mycodo => {
                            ui.label("Paste each Mycodo unique id. Leave blank for anything the shelf does not have.");
                            ui.add_space(6.0);
                            ui.strong("Sensors (Mycodo Inputs)");
                            egui::Grid::new("w2i").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                                for (q, id, unit) in &mut w.inputs {
                                    ui.label(label_q(*q));
                                    ui.text_edit_singleline(id);
                                    ui.label(RichText::new(format!("unit {unit}")).color(theme::TEXT_FAINT));
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
                    },
                    3 => {
                        ui.radio_value(&mut w.planner, PlannerKind::Redrob, "Agent through the Redrob Code engine on the node's machine (recommended)");
                        ui.radio_value(&mut w.planner, PlannerKind::Custom, "Agent through another OpenAI-compatible endpoint");
                        ui.radio_value(&mut w.planner, PlannerKind::Rules, "Rules only, no model");
                        ui.add_space(8.0);
                        match w.planner {
                            PlannerKind::Redrob => {
                                ui.label(RichText::new("The node finds the daemon's record, starts it if needed, and labels its calls as the Grow app. Falls back to rules when the engine is away.").color(theme::TEXT_DIM));
                            }
                            PlannerKind::Custom => {
                                egui::Grid::new("w3c").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label("Endpoint (/v1)");
                                    ui.text_edit_singleline(&mut w.endpoint);
                                    ui.end_row();
                                    ui.label("API key environment variable");
                                    ui.text_edit_singleline(&mut w.api_key_env);
                                    ui.end_row();
                                    ui.label("Model");
                                    ui.text_edit_singleline(&mut w.model);
                                    ui.end_row();
                                });
                                ui.label(RichText::new("The key itself never goes into node.toml.").color(theme::TEXT_FAINT));
                            }
                            PlannerKind::Rules => {
                                ui.label(RichText::new("Deadband control plus the office habits. The baseline the agent is measured against.").color(theme::TEXT_DIM));
                            }
                        }
                        if w.planner != PlannerKind::Rules {
                            ui.add_space(6.0);
                            ui.checkbox(&mut w.photos, "Attach the newest photo from photos/ to every decision (scripts/snap.sh)");
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new("Whatever decides, the guard in grow-safety has the last word.").color(theme::TEXT_FAINT));
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
                                ui.colored_label(theme::GOOD, format!("Saved {p}. Now run:  grow-node run {p}"));
                            }
                            Some(Err(e)) => {
                                ui.colored_label(theme::DANGER, e);
                            }
                            None => {}
                        }
                    }
                }
                ui.add_space(12.0);
                let problems = w.problems();
                for p in &problems {
                    ui.colored_label(theme::WARNING, p);
                }
                ui.horizontal(|ui| {
                    if w.step > 0 && ui.button("Back").clicked() {
                        w.step -= 1;
                    }
                    if w.step + 1 < STEPS.len() {
                        let next = egui::Button::new(RichText::new("Next").color(Color32::WHITE)).fill(theme::ACCENT);
                        if ui.add_enabled(problems.is_empty(), next).clicked() {
                            w.step += 1;
                        }
                    }
                });
            });
        });
    }

    fn recipes(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for r in builtin::all() {
                theme::card(ui, |ui| {
                    ui.heading(format!("{}  ({} {}, {} days)", r.id, r.crop, r.variety, r.days));
                    egui::Grid::new(format!("r-{}", r.id)).num_columns(5).spacing([12.0, 4.0]).show(ui, |ui| {
                        for h in ["stage", "from day", "light", "targets", "hard ceiling"] {
                            ui.label(RichText::new(h).color(theme::TEXT_FAINT));
                        }
                        ui.end_row();
                        for s in &r.stages {
                            ui.label(&s.name);
                            ui.label(s.from_day.to_string());
                            ui.label(format!("{}h from {:02}:00", s.light.hours, s.light.on_hour as u32));
                            ui.label(
                                s.targets.iter().map(|(q, b)| format!("{} {:.1}..{:.1}", label_q(*q), b.min, b.max)).collect::<Vec<_>>().join("   "),
                            );
                            if s.hard.is_empty() {
                                ui.label(RichText::new("--").color(theme::TEXT_FAINT));
                            } else {
                                ui.colored_label(
                                    theme::WARNING,
                                    s.hard.iter().map(|(q, b)| format!("{} <= {:.0}", label_q(*q), b.max)).collect::<Vec<_>>().join("  "),
                                );
                            }
                            ui.end_row();
                        }
                    });
                    for src in &r.sources {
                        ui.label(RichText::new(src).small().color(theme::TEXT_FAINT));
                    }
                });
                ui.add_space(10.0);
            }
        });
    }
}

/// Air and water temperature over the stored history, with the current stage band shaded.
fn chart(ui: &mut Ui, history: &[Report], targets: &std::collections::BTreeMap<Quantity, Band>, height: f32) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 4.0, theme::FIELD);
    let series = [(Quantity::AirTempC, theme::ACCENT), (Quantity::WaterTempC, Color32::from_rgb(255, 138, 101))];
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
        painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(92, 184, 92, 40));
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
    let font = egui::FontId::proportional(11.0);
    painter.text(rect.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, format!("{hi:.1}"), font.clone(), theme::TEXT_FAINT);
    painter.text(rect.left_bottom() + egui::vec2(6.0, -4.0), egui::Align2::LEFT_BOTTOM, format!("{lo:.1}"), font.clone(), theme::TEXT_FAINT);
    painter.text(rect.right_top() + egui::vec2(-6.0, 4.0), egui::Align2::RIGHT_TOP, "air  water", font, theme::TEXT_FAINT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauge_colours_and_positions() {
        let b = Band::new(18.0, 22.0);
        assert_eq!(band_color(20.0, &b), theme::GOOD);
        assert_eq!(band_color(23.0, &b), theme::WARNING);
        assert_eq!(band_color(30.0, &b), theme::DANGER);
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
