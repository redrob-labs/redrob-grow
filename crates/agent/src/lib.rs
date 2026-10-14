//! The decision loop.
//!
//! One [`Cycle::run`] is: read the rig, work out which recipe stage today is, update the room
//! model, ask the planner for commands, pass them through the guard, send what survives, and
//! return a [`Report`] of all of it. The node calls it every `period_secs`; tests call it with a
//! simulated rig thousands of times in a second.
//!
//! Two planners exist. [`RulePlanner`] is deterministic deadband control plus the office tricks
//! (pre-heat before the air conditioner comes on, move the light into the hours when people are
//! breathing CO2 into the room). The model-backed planner that reasons over the same
//! [`Observation`] and emits the same [`Proposal`] arrives in Stage 2, through the tool schemas in
//! [`tools`]; nothing in the loop changes when it does.

use grow_hal::{Command, HalError, Quantity, Rig, Snapshot, Switch};
use grow_recipe::{Band, Recipe, Stage};
use grow_safety::{Guard, Verdict};
use serde::{Deserialize, Serialize};

pub mod office;
pub mod tools;

pub use office::OfficeModel;

/// Wall-clock facts the planner needs, supplied by the caller so the loop never reads a clock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Clock {
    /// Monotonic seconds, any epoch. Used for cooldowns and the room model.
    pub now_secs: u64,
    /// Local hour of day, 0.0..24.0.
    pub hour: f64,
    pub weekday: bool,
    /// Days since sowing.
    pub day: u32,
}

/// What the planner sees. Serialisable on purpose: this is also the model-planner's prompt input
/// and what the run log stores.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub clock: Clock,
    pub snapshot: Snapshot,
    pub stage: Stage,
    pub recipe_id: String,
    pub recipe_done: bool,
    /// Forecast air temperature one hour ahead, if the room model has seen enough days.
    pub air_temp_forecast_c: Option<f64>,
    pub switches: Vec<Switch>,
}

/// What a planner wants done, with one line of why per command. The reason is kept in the run
/// log so a human can audit a grow afterwards.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub commands: Vec<Command>,
    pub reasons: Vec<String>,
}

impl Proposal {
    pub fn push(&mut self, cmd: Command, why: impl Into<String>) {
        self.commands.push(cmd);
        self.reasons.push(why.into());
    }
}

pub trait Planner {
    fn name(&self) -> &str;
    fn plan(&mut self, obs: &Observation) -> Proposal;
}

/// Everything one cycle did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub observation: Observation,
    pub proposal: Proposal,
    pub verdict: Verdict,
    /// Commands the rig refused (unwired, transport error), with the error text.
    pub failed: Vec<(Command, String)>,
}

pub struct Cycle<R: Rig, P: Planner> {
    pub rig: R,
    pub planner: P,
    pub guard: Guard,
    pub recipe: Recipe,
    pub office: OfficeModel,
}

impl<R: Rig, P: Planner> Cycle<R, P> {
    pub fn new(rig: R, planner: P, guard: Guard, recipe: Recipe) -> Self {
        Self { rig, planner, guard, recipe, office: OfficeModel::default() }
    }

    pub fn observe(&mut self, clock: Clock) -> Result<Observation, HalError> {
        let snapshot = self.rig.read()?;
        if let Some(t) = snapshot.get(Quantity::AirTempC) {
            self.office.record(clock.hour, clock.weekday, t);
        }
        Ok(Observation {
            clock,
            stage: self.recipe.stage_at(clock.day).clone(),
            recipe_id: self.recipe.id.clone(),
            recipe_done: self.recipe.is_done(clock.day),
            air_temp_forecast_c: self.office.forecast(clock.hour + 1.0, clock.weekday),
            switches: self.rig.switches(),
            snapshot,
        })
    }

    pub fn run(&mut self, clock: Clock) -> Result<Report, HalError> {
        let observation = self.observe(clock)?;
        let proposal = self.planner.plan(&observation);
        let verdict = self.guard.check(clock.now_secs, &observation.snapshot, &proposal.commands);
        let mut failed = Vec::new();
        for cmd in &verdict.run {
            if let Err(e) = self.rig.set(*cmd) {
                failed.push((*cmd, e.to_string()));
            }
        }
        Ok(Report { observation, proposal, verdict, failed })
    }
}

/// Deadband control with two office habits. Holds each quantity inside its stage band using the
/// switch that moves it; schedules the light; pre-heats water when the room is about to cool.
#[derive(Debug, Clone, Default)]
pub struct RulePlanner {
    /// Shift the light window to start when people arrive, so the crop gets their CO2. Hours.
    pub light_shift_to_hour: Option<f64>,
}

impl RulePlanner {
    fn light_should_be_on(stage: &Stage, hour: f64, shift: Option<f64>) -> bool {
        let on = shift.unwrap_or(stage.light.on_hour);
        let off = on + stage.light.hours;
        let h = if hour < on { hour + 24.0 } else { hour };
        stage.light.hours > 0.0 && h >= on && h < off
    }
}

impl Planner for RulePlanner {
    fn name(&self) -> &str {
        "rules"
    }

    fn plan(&mut self, obs: &Observation) -> Proposal {
        let mut p = Proposal::default();
        let s = &obs.snapshot;
        let has = |sw: Switch| obs.switches.contains(&sw);
        let stage = &obs.stage;

        // Light: a schedule, not a measurement.
        if has(Switch::Light) {
            let want = Self::light_should_be_on(stage, obs.clock.hour, self.light_shift_to_hour);
            if want && !s.is_on(Switch::Light) {
                p.push(
                    Command { switch: Switch::Light, on: true, for_secs: 3600 },
                    format!("light window {}h from {}", stage.light.hours, self.light_shift_to_hour.unwrap_or(stage.light.on_hour)),
                );
            } else if want {
                p.push(Command { switch: Switch::Light, on: true, for_secs: 3600 }, "renew light timer");
            } else if s.is_on(Switch::Light) {
                p.push(Command { switch: Switch::Light, on: false, for_secs: 0 }, "outside light window");
            }
        }

        // Water temperature: heater up, nothing down (an office shelf has no chiller).
        if has(Switch::Heater)
            && let (Some(band), Some(t)) = (stage.targets.get(&Quantity::WaterTempC), s.get(Quantity::WaterTempC))
        {
            let cooling_ahead = obs.air_temp_forecast_c.is_some_and(|f| f < band.min);
            let width = (band.max - band.min).max(0.5);
            if t < band.min {
                // Pulse length follows the deficit; the guard clips the ceiling.
                let secs = ((band.min - t) * 600.0).clamp(300.0, 1800.0) as u32;
                p.push(Command { switch: Switch::Heater, on: true, for_secs: secs }, format!("water {t:.1} C below {:.1}", band.min));
            } else if cooling_ahead && t < band.min + 0.3 * width {
                p.push(Command { switch: Switch::Heater, on: true, for_secs: 600 }, "room about to cool, pre-heat toward band middle");
            } else if s.is_on(Switch::Heater) && t >= band.mid() {
                p.push(Command { switch: Switch::Heater, on: false, for_secs: 0 }, "water at band middle");
            }
        }

        // Air: a fan can only pull the shelf toward the room. Use it when the shelf is hotter than
        // the band and the room is cooler (lamp heat), or to dry a too-wet shelf.
        if has(Switch::Fan) {
            let above = |q: Quantity| stage.targets.get(&q).zip(s.get(q)).is_some_and(|(b, t)| t > b.max);
            // Warm water on a shelf with no chiller is lamp heat; the fan is the only lever.
            let hot = above(Quantity::AirTempC) || above(Quantity::WaterTempC);
            let wet = stage.targets.get(&Quantity::HumidityPct).zip(s.get(Quantity::HumidityPct)).is_some_and(|(b, rh)| rh > b.max);
            if hot || wet {
                p.push(Command { switch: Switch::Fan, on: true, for_secs: 1800 }, if hot { "shelf air above band" } else { "humidity above band" });
            } else if s.is_on(Switch::Fan) {
                p.push(Command { switch: Switch::Fan, on: false, for_secs: 0 }, "air inside band");
            }
        }

        if has(Switch::Humidifier)
            && let (Some(band), Some(rh)) = (stage.targets.get(&Quantity::HumidityPct), s.get(Quantity::HumidityPct))
        {
            if rh < band.min {
                p.push(Command { switch: Switch::Humidifier, on: true, for_secs: 900 }, format!("humidity {rh:.0}% below {:.0}%", band.min));
            } else if s.is_on(Switch::Humidifier) && rh >= band.mid() {
                p.push(Command { switch: Switch::Humidifier, on: false, for_secs: 0 }, "humidity at band middle");
            }
        }

        // Reservoir: short pulses, the guard enforces the cooldown. A stage with no reservoir
        // target still has water in the tub, so a sane default band is held instead.
        let band_or = |q: Quantity, fallback: Band| stage.targets.get(&q).copied().unwrap_or(fallback);
        if has(Switch::TopUp)
            && let Some(lvl) = s.get(Quantity::WaterLevelPct)
        {
            let level_band = band_or(Quantity::WaterLevelPct, Band::new(40.0, 100.0));
            let ec_high = s.get(Quantity::EcMs).is_some_and(|ec| ec > band_or(Quantity::EcMs, Band::new(0.8, 1.6)).max);
            if lvl < level_band.min {
                p.push(Command { switch: Switch::TopUp, on: true, for_secs: 60 }, format!("level {lvl:.0}% below band"));
            } else if ec_high && lvl < level_band.max {
                p.push(Command { switch: Switch::TopUp, on: true, for_secs: 60 }, "EC above band, dilute with fresh water");
            }
        }
        if has(Switch::DoseNutrient)
            && let Some(ec) = s.get(Quantity::EcMs)
            && ec < band_or(Quantity::EcMs, Band::new(0.8, 1.6)).min
        {
            p.push(Command { switch: Switch::DoseNutrient, on: true, for_secs: 10 }, format!("EC {ec:.2} below band"));
        }
        if has(Switch::DosePhDown)
            && let Some(ph) = s.get(Quantity::Ph)
            && ph > band_or(Quantity::Ph, Band::new(5.5, 6.5)).max
        {
            p.push(Command { switch: Switch::DosePhDown, on: true, for_secs: 5 }, format!("pH {ph:.2} above band"));
        }

        // Pump: always on while the crop is alive. Roots need oxygen more than anything above.
        if has(Switch::Pump) && !obs.recipe_done {
            p.push(Command { switch: Switch::Pump, on: true, for_secs: 3600 }, "circulation");
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use grow_hal::mock::{Office, OfficeRig, SimTime};
    use grow_recipe::builtin;
    use grow_safety::Limits;

    use super::*;

    fn clock(rig: &OfficeRig, sow: SimTime) -> Clock {
        Clock { now_secs: rig.now.0, hour: rig.now.hour_f(), weekday: rig.now.is_weekday(), day: (rig.now.0 - sow.0) as u32 / SimTime::DAY as u32 }
    }

    /// Run a whole recipe in the simulated office and measure how often each target was held.
    fn simulate(office: Office, recipe_id: &str, period_secs: u32) -> (f64, f64, f64) {
        let recipe = builtin::by_id(recipe_id).unwrap();
        let rig = OfficeRig::new(office);
        let sow = rig.now;
        let mut cycle = Cycle::new(rig, RulePlanner::default(), Guard::new(Limits::office_shelf()), recipe.clone());
        let (mut water_ok, mut light_ok, mut n) = (0u32, 0u32, 0u32);
        while !cycle.recipe.is_done(clock(&cycle.rig, sow).day) {
            let c = clock(&cycle.rig, sow);
            let report = cycle.run(c).unwrap();
            assert!(report.failed.is_empty(), "{:?}", report.failed);
            assert!(report.verdict.alarms.is_empty(), "alarm {:?} on day {}", report.verdict.alarms, c.day);
            let snap = &report.observation.snapshot;
            let stage = &report.observation.stage;
            if let (Some(b), Some(t)) = (stage.targets.get(&Quantity::WaterTempC), snap.get(Quantity::WaterTempC))
                && b.contains(t)
            {
                water_ok += 1;
            }
            let want = RulePlanner::light_should_be_on(stage, c.hour, None);
            if want == snap.is_on(Switch::Light) {
                light_ok += 1;
            }
            n += 1;
            cycle.rig.tick(period_secs);
        }
        (f64::from(water_ok) / f64::from(n), f64::from(light_ok) / f64::from(n), cycle.rig.kwh())
    }

    #[test]
    fn lettuce_in_a_winter_office_holds_water_temperature_most_of_the_time() {
        let (water, light, kwh) = simulate(Office::winter(), "lettuce-butterhead", 600);
        // The remainder is physics, not planning: with the lamp on in a 22 C office the water sits
        // just above the 22 C ceiling and the shelf has no chiller, only a fan.
        assert!(water > 0.75, "water in band {water:.2}");
        assert!(light > 0.95, "light schedule kept {light:.2}");
        assert!(kwh > 0.0 && kwh < 150.0, "kwh {kwh:.1}");
    }

    #[test]
    fn rice_in_a_summer_office_never_trips_an_alarm() {
        let (water, _, _) = simulate(Office::summer(), "rice-sindongjin", 1800);
        assert!(water > 0.5, "water in band {water:.2}");
    }

    #[test]
    fn sprouts_need_almost_nothing() {
        let (_, light, kwh) = simulate(Office::summer(), "radish-sprout", 600);
        assert!(light > 0.95);
        assert!(kwh < 15.0, "kwh {kwh:.1}");
    }

    #[test]
    fn light_window_wraps_midnight() {
        let mut stage = builtin::by_id("lettuce-butterhead").unwrap().stages[1].clone();
        stage.light.on_hour = 20.0;
        stage.light.hours = 10.0;
        assert!(RulePlanner::light_should_be_on(&stage, 23.0, None));
        assert!(RulePlanner::light_should_be_on(&stage, 3.0, None));
        assert!(!RulePlanner::light_should_be_on(&stage, 7.0, None));
        assert!(RulePlanner::light_should_be_on(&stage, 9.5, Some(9.0)));
    }

    #[test]
    fn report_serialises_for_the_run_log() {
        let rig = OfficeRig::new(Office::summer());
        let mut cycle = Cycle::new(rig, RulePlanner::default(), Guard::new(Limits::office_shelf()), builtin::by_id("radish-sprout").unwrap());
        let report = cycle.run(Clock { now_secs: 0, hour: 9.0, weekday: true, day: 3 }).unwrap();
        let json = serde_json::to_string(&report).unwrap();
        let back: Report = serde_json::from_str(&json).unwrap();
        assert_eq!(back, report);
        assert_eq!(report.observation.stage.name, "greening");
    }
}
