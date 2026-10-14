//! A shelf in an air-conditioned office, in about a hundred lines of physics.
//!
//! Not a plant model. It reproduces the four things that make an office hard for a crop: the air
//! conditioner follows office hours, not the plant; its air is dry; CO2 climbs with people in the
//! room and falls at night; and the only reliable light is the LED. Every effect is a first-order
//! pull toward a target, which is enough for the agent loop and the safety layer to be exercised.

use std::collections::BTreeMap;

use crate::{Command, HalError, Quantity, Reading, Rig, Snapshot, Switch};

/// Room behaviour the rig sits in.
#[derive(Debug, Clone, PartialEq)]
pub struct Office {
    /// Air temperature the air conditioner holds during office hours.
    pub ac_set_c: f64,
    /// Where the room drifts when the air conditioner is off (hot in summer, cold in winter).
    pub idle_c: f64,
    /// First and last office hour (local), air conditioner on in between on weekdays.
    pub office_hours: (u32, u32),
    /// Relative humidity the air conditioner drives the room to.
    pub ac_rh_pct: f64,
    /// CO2 with people in the room and with nobody there.
    pub co2_busy_ppm: f64,
    pub co2_empty_ppm: f64,
}

impl Office {
    /// A Seoul office in July: cold and dry inside by day, warm and still at night and all weekend.
    pub fn summer() -> Self {
        Self { ac_set_c: 23.0, idle_c: 29.0, office_hours: (9, 18), ac_rh_pct: 45.0, co2_busy_ppm: 950.0, co2_empty_ppm: 430.0 }
    }

    /// The same office in January: heated by day, cold at night.
    pub fn winter() -> Self {
        Self { ac_set_c: 22.0, idle_c: 14.0, office_hours: (9, 18), ac_rh_pct: 30.0, co2_busy_ppm: 1100.0, co2_empty_ppm: 430.0 }
    }

    fn is_office_time(&self, t: SimTime) -> bool {
        t.is_weekday() && (self.office_hours.0..self.office_hours.1).contains(&t.hour())
    }
}

/// Seconds since a Monday 00:00. Keeps the model free of real clocks and time zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SimTime(pub u64);

impl SimTime {
    pub const DAY: u64 = 86_400;

    pub fn hour(self) -> u32 {
        ((self.0 % Self::DAY) / 3600) as u32
    }

    pub fn day(self) -> u32 {
        (self.0 / Self::DAY) as u32
    }

    pub fn is_weekday(self) -> bool {
        self.day() % 7 < 5
    }

    pub fn hour_f(self) -> f64 {
        (self.0 % Self::DAY) as f64 / 3600.0
    }
}

#[derive(Debug, Clone)]
pub struct OfficeRig {
    pub office: Office,
    pub now: SimTime,
    state: BTreeMap<Quantity, f64>,
    /// Remaining on-time per switch, seconds. Absent or 0 means off.
    timers: BTreeMap<Switch, u32>,
    wired: Vec<Switch>,
    /// Watt-seconds consumed since start.
    pub joules: f64,
}

impl OfficeRig {
    pub fn new(office: Office) -> Self {
        let mut state = BTreeMap::new();
        state.insert(Quantity::AirTempC, office.idle_c);
        state.insert(Quantity::WaterTempC, office.idle_c - 1.0);
        state.insert(Quantity::HumidityPct, 55.0);
        state.insert(Quantity::Co2Ppm, office.co2_empty_ppm);
        state.insert(Quantity::EcMs, 1.3);
        state.insert(Quantity::Ph, 6.0);
        state.insert(Quantity::WaterLevelPct, 90.0);
        state.insert(Quantity::PpfdUmol, 0.0);
        Self { office, now: SimTime(0), state, timers: BTreeMap::new(), wired: Switch::ALL.to_vec(), joules: 0.0 }
    }

    /// Restrict the rig to a few plugs, as a real shelf would be.
    pub fn with_switches(mut self, wired: &[Switch]) -> Self {
        self.wired = wired.to_vec();
        self
    }

    pub fn kwh(&self) -> f64 {
        self.joules / 3_600_000.0
    }

    fn is_on(&self, s: Switch) -> bool {
        self.timers.get(&s).copied().unwrap_or(0) > 0
    }

    fn pull(&mut self, q: Quantity, target: f64, rate_per_min: f64, minutes: f64) {
        let v = self.state[&q];
        let k = (rate_per_min * minutes).min(1.0);
        self.state.insert(q, v + (target - v) * k);
    }

    fn step_minute(&mut self) {
        let o = self.office.clone();
        let office_time = o.is_office_time(self.now);
        let light = self.is_on(Switch::Light);
        let heater = self.is_on(Switch::Heater);
        let fan = self.is_on(Switch::Fan);
        let humidifier = self.is_on(Switch::Humidifier);

        // Room air: the air conditioner is the strongest actor in the building.
        let room_c = if office_time { o.ac_set_c } else { o.idle_c };
        // The lamp warms the canopy; a fan carries most of that heat off the shelf.
        let lamp_heat = match (light, fan) {
            (true, false) => 1.5,
            (true, true) => 0.4,
            _ => 0.0,
        };
        let air_rate = if fan { 0.12 } else { 0.04 };
        self.pull(Quantity::AirTempC, room_c + lamp_heat, air_rate, 1.0);

        // Water follows air slowly; the heater pulls it toward 35 C.
        let water_target = if heater { 35.0 } else { self.state[&Quantity::AirTempC] - 0.5 };
        // A 100 W heater in a 20 L tub is about 0.07 C per minute.
        self.pull(Quantity::WaterTempC, water_target, if heater { 0.005 } else { 0.01 }, 1.0);

        // Humidity: the air conditioner dries, the reservoir and a humidifier wet.
        let rh_target = if humidifier {
            80.0
        } else if office_time {
            o.ac_rh_pct
        } else {
            60.0
        };
        self.pull(Quantity::HumidityPct, rh_target, if fan { 0.08 } else { 0.03 }, 1.0);

        // CO2 is people.
        let co2_target = if office_time { o.co2_busy_ppm } else { o.co2_empty_ppm };
        self.pull(Quantity::Co2Ppm, co2_target, 0.02, 1.0);

        // Light is binary at the canopy.
        self.state.insert(Quantity::PpfdUmol, if light { 220.0 } else { 0.0 });

        // Reservoir: evaporation, dosing, top-up.
        let mut level = self.state[&Quantity::WaterLevelPct] - if light { 0.004 } else { 0.002 };
        let mut ec = self.state[&Quantity::EcMs] + 0.00002; // plants drink water, salts stay
        let mut ph = self.state[&Quantity::Ph] + 0.0001; // drifts up as nutrients are taken
        if self.is_on(Switch::TopUp) {
            level += 1.0;
            ec -= 0.02; // fresh water dilutes
        }
        if self.is_on(Switch::DoseNutrient) {
            ec += 0.02;
        }
        if self.is_on(Switch::DosePhDown) {
            ph -= 0.03;
        }
        self.state.insert(Quantity::WaterLevelPct, level.clamp(0.0, 100.0));
        self.state.insert(Quantity::EcMs, ec.clamp(0.0, 5.0));
        self.state.insert(Quantity::Ph, ph.clamp(3.0, 9.0));

        // Energy and timers.
        for s in Switch::ALL {
            if self.is_on(s) {
                self.joules += s.nominal_watts() * 60.0;
            }
        }
        for t in self.timers.values_mut() {
            *t = t.saturating_sub(60);
        }
        self.now = SimTime(self.now.0 + 60);
    }

    pub fn readings(&self) -> Vec<Reading> {
        self.state.iter().map(|(q, v)| Reading { quantity: *q, value: *v }).collect()
    }
}

impl Rig for OfficeRig {
    fn name(&self) -> &str {
        "office-sim"
    }

    fn switches(&self) -> Vec<Switch> {
        self.wired.clone()
    }

    fn read(&mut self) -> Result<Snapshot, HalError> {
        let on = self.wired.iter().map(|s| (*s, self.is_on(*s))).collect();
        Ok(Snapshot { readings: self.state.clone(), on })
    }

    fn set(&mut self, cmd: Command) -> Result<(), HalError> {
        if !self.wired.contains(&cmd.switch) {
            return Err(HalError::NotWired(cmd.switch));
        }
        self.timers.insert(cmd.switch, if cmd.on { cmd.for_secs } else { 0 });
        Ok(())
    }

    fn tick(&mut self, secs: u32) {
        for _ in 0..secs.div_ceil(60) {
            self.step_minute();
        }
    }

    fn sim_time(&self) -> Option<u64> {
        Some(self.now.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_conditioner_follows_office_hours() {
        let mut rig = OfficeRig::new(Office::summer());
        // Monday 00:00 -> run to 12:00: office is cold.
        rig.tick(12 * 3600);
        let noon = rig.read().unwrap().get(Quantity::AirTempC).unwrap();
        assert!((noon - 23.0).abs() < 0.5, "noon {noon}");
        // Run to 02:00 Tuesday: warm again.
        rig.tick(14 * 3600);
        let night = rig.read().unwrap().get(Quantity::AirTempC).unwrap();
        assert!((night - 29.0).abs() < 0.5, "night {night}");
    }

    #[test]
    fn weekend_is_never_conditioned() {
        let mut rig = OfficeRig::new(Office::winter());
        rig.now = SimTime(5 * SimTime::DAY); // Saturday
        rig.tick(13 * 3600);
        let sat = rig.read().unwrap().get(Quantity::AirTempC).unwrap();
        assert!((sat - 14.0).abs() < 0.5, "saturday noon {sat}");
    }

    #[test]
    fn heater_turns_itself_off_and_counts_energy() {
        let mut rig = OfficeRig::new(Office::winter()).with_switches(&[Switch::Heater]);
        let before = rig.read().unwrap().get(Quantity::WaterTempC).unwrap();
        rig.set(Command { switch: Switch::Heater, on: true, for_secs: 600 }).unwrap();
        rig.tick(600);
        let after = rig.read().unwrap().get(Quantity::WaterTempC).unwrap();
        assert!(after > before + 0.5, "{before} -> {after}");
        assert!(!rig.read().unwrap().is_on(Switch::Heater), "timer expired");
        let expected_kwh = 100.0 * 600.0 / 3_600_000.0;
        assert!((rig.kwh() - expected_kwh).abs() < 1e-9);
    }

    #[test]
    fn unwired_switch_is_refused() {
        let mut rig = OfficeRig::new(Office::summer()).with_switches(&[Switch::Light]);
        let err = rig.set(Command { switch: Switch::Heater, on: true, for_secs: 60 }).unwrap_err();
        assert!(matches!(err, HalError::NotWired(Switch::Heater)));
    }

    #[test]
    fn light_gives_ppfd_and_some_heat() {
        let mut rig = OfficeRig::new(Office::summer());
        rig.set(Command { switch: Switch::Light, on: true, for_secs: 3600 }).unwrap();
        rig.tick(60);
        assert_eq!(rig.read().unwrap().get(Quantity::PpfdUmol), Some(220.0));
        assert_eq!(rig.readings().len(), 8);
    }
}
