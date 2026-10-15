//! The part of the system that does not take suggestions.
//!
//! The agent proposes commands. [`Guard::check`] turns the proposal into what actually runs:
//! every command is clipped to a maximum on-time, a switch in cooldown is refused, a switch whose
//! quantity is already past a hard limit is refused, and if any quantity is past a hard limit the
//! guard adds its own command to turn the offending switch off. The guard is deterministic and
//! has no model behind it; that is the point.

use std::collections::BTreeMap;

use grow_hal::{Command, Quantity, Snapshot, Switch};
use grow_recipe::Band;
use serde::{Deserialize, Serialize};

/// Limits for one switch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SwitchLimit {
    /// Longest a single command may keep the switch on.
    pub max_on_secs: u32,
    /// Shortest gap between two on-commands.
    pub cooldown_secs: u32,
}

/// Everything the guard knows. Ships with [`Limits::office_shelf`]; a node may tighten it in
/// its config, never loosen it below the built-in floor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    /// Readings outside these bands are an emergency, whatever the recipe says.
    pub hard: BTreeMap<Quantity, Band>,
    pub switches: BTreeMap<Switch, SwitchLimit>,
    /// A switch that pushes a quantity up. When that quantity is past its hard `max`, the switch
    /// is forced off and may not be turned on.
    pub raises: BTreeMap<Switch, Quantity>,
}

impl Limits {
    /// Limits for a shelf in a room people work in. Water never above 34 C (root death, and a
    /// heater fault), air never above 38 C, EC never above 3.0, pH kept where nothing is
    /// corrosive. Dosing pumps run seconds, not minutes.
    pub fn office_shelf() -> Self {
        let mut hard = BTreeMap::new();
        hard.insert(Quantity::AirTempC, Band::new(5.0, 38.0));
        hard.insert(Quantity::WaterTempC, Band::new(5.0, 34.0));
        hard.insert(Quantity::HumidityPct, Band::new(15.0, 98.0));
        hard.insert(Quantity::Co2Ppm, Band::new(0.0, 2500.0));
        hard.insert(Quantity::EcMs, Band::new(0.0, 3.0));
        hard.insert(Quantity::Ph, Band::new(4.5, 8.0));
        hard.insert(Quantity::WaterLevelPct, Band::new(5.0, 100.0));

        let mut switches = BTreeMap::new();
        switches.insert(Switch::Light, SwitchLimit { max_on_secs: 3600, cooldown_secs: 0 });
        switches.insert(Switch::Heater, SwitchLimit { max_on_secs: 1800, cooldown_secs: 300 });
        switches.insert(Switch::Fan, SwitchLimit { max_on_secs: 3600, cooldown_secs: 0 });
        switches.insert(Switch::Pump, SwitchLimit { max_on_secs: 3600, cooldown_secs: 0 });
        switches.insert(Switch::Humidifier, SwitchLimit { max_on_secs: 1800, cooldown_secs: 300 });
        switches.insert(Switch::DoseNutrient, SwitchLimit { max_on_secs: 20, cooldown_secs: 1800 });
        switches.insert(Switch::DosePhDown, SwitchLimit { max_on_secs: 10, cooldown_secs: 1800 });
        switches.insert(Switch::TopUp, SwitchLimit { max_on_secs: 120, cooldown_secs: 900 });

        let mut raises = BTreeMap::new();
        raises.insert(Switch::Heater, Quantity::WaterTempC);
        raises.insert(Switch::Humidifier, Quantity::HumidityPct);
        raises.insert(Switch::DoseNutrient, Quantity::EcMs);
        raises.insert(Switch::TopUp, Quantity::WaterLevelPct);

        Self { hard, switches, raises }
    }
}

/// Why a proposed command did not run as proposed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Rejection {
    /// The switch is not in the limits table, so nothing is known about it.
    UnknownSwitch(Switch),
    /// Shortened to the switch's `max_on_secs`.
    Clipped { switch: Switch, from_secs: u32, to_secs: u32 },
    /// Turned on too recently.
    Cooldown { switch: Switch, remaining_secs: u32 },
    /// The quantity this switch raises is already past its hard maximum.
    PastHardLimit { switch: Switch, quantity: Quantity, value: f64, max: f64 },
}

/// A reading outside a hard band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Alarm {
    pub quantity: Quantity,
    pub value: f64,
    pub band: Band,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Verdict {
    /// Commands that will actually be sent, in order.
    pub run: Vec<Command>,
    pub rejected: Vec<Rejection>,
    pub alarms: Vec<Alarm>,
}

/// Stateful guard: remembers when each switch was last turned on, for cooldowns.
#[derive(Debug, Clone)]
pub struct Guard {
    /// Limits in force now: the floor, tightened by the current recipe stage.
    pub limits: Limits,
    floor: Limits,
    last_on_at: BTreeMap<Switch, u64>,
}

impl Guard {
    pub fn new(limits: Limits) -> Self {
        Self { floor: limits.clone(), limits, last_on_at: BTreeMap::new() }
    }

    /// Apply a stage's extra hard bands. Each band may only narrow the floor: a looser value is
    /// ignored. Call once per cycle with the stage in force; a stage with no `hard` table
    /// restores the floor.
    pub fn tighten(&mut self, stage_hard: &BTreeMap<Quantity, Band>) {
        let mut hard = self.floor.hard.clone();
        for (q, b) in stage_hard {
            let merged = match hard.get(q) {
                Some(base) => Band::new(base.min.max(b.min), base.max.min(b.max)),
                None => *b,
            };
            hard.insert(*q, merged);
        }
        self.limits.hard = hard;
    }

    /// Alarms for the current readings, independent of any proposal.
    pub fn alarms(&self, snap: &Snapshot) -> Vec<Alarm> {
        let mut out = Vec::new();
        for (q, band) in &self.limits.hard {
            if let Some(v) = snap.get(*q)
                && !band.contains(v)
            {
                out.push(Alarm { quantity: *q, value: v, band: *band });
            }
        }
        out
    }

    /// Check a proposal against the snapshot at time `now` (seconds, any monotonic clock).
    /// Records on-times of the commands it lets through, so call it once per cycle.
    pub fn check(&mut self, now: u64, snap: &Snapshot, proposed: &[Command]) -> Verdict {
        let mut v = Verdict { alarms: self.alarms(snap), ..Default::default() };

        // Emergencies first: force off every switch that raises an alarmed quantity.
        for alarm in &v.alarms {
            for (sw, q) in &self.limits.raises {
                if *q == alarm.quantity && alarm.value > alarm.band.max && snap.is_on(*sw) {
                    v.run.push(Command { switch: *sw, on: false, for_secs: 0 });
                }
            }
        }

        for cmd in proposed {
            let Some(lim) = self.limits.switches.get(&cmd.switch) else {
                v.rejected.push(Rejection::UnknownSwitch(cmd.switch));
                continue;
            };
            if !cmd.on {
                v.run.push(Command { for_secs: 0, ..*cmd });
                continue;
            }
            if let Some(q) = self.limits.raises.get(&cmd.switch)
                && let (Some(val), Some(band)) = (snap.get(*q), self.limits.hard.get(q))
                && val > band.max
            {
                v.rejected.push(Rejection::PastHardLimit { switch: cmd.switch, quantity: *q, value: val, max: band.max });
                continue;
            }
            if let Some(last) = self.last_on_at.get(&cmd.switch) {
                let since = now.saturating_sub(*last);
                if since < u64::from(lim.cooldown_secs) {
                    v.rejected.push(Rejection::Cooldown { switch: cmd.switch, remaining_secs: (u64::from(lim.cooldown_secs) - since) as u32 });
                    continue;
                }
            }
            let mut run = *cmd;
            if run.for_secs > lim.max_on_secs {
                v.rejected.push(Rejection::Clipped { switch: cmd.switch, from_secs: cmd.for_secs, to_secs: lim.max_on_secs });
                run.for_secs = lim.max_on_secs;
            }
            if run.for_secs == 0 {
                run.for_secs = lim.max_on_secs.min(60);
            }
            self.last_on_at.insert(cmd.switch, now);
            v.run.push(run);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(pairs: &[(Quantity, f64)], on: &[Switch]) -> Snapshot {
        let mut s = Snapshot::default();
        for (q, v) in pairs {
            s.readings.insert(*q, *v);
        }
        for sw in on {
            s.on.insert(*sw, true);
        }
        s
    }

    #[test]
    fn long_commands_are_clipped_and_zero_gets_a_floor() {
        let mut g = Guard::new(Limits::office_shelf());
        let v = g.check(
            0,
            &snap(&[], &[]),
            &[Command { switch: Switch::Heater, on: true, for_secs: 99_999 }, Command { switch: Switch::Fan, on: true, for_secs: 0 }],
        );
        assert_eq!(v.run[0].for_secs, 1800);
        assert_eq!(v.run[1].for_secs, 60);
        assert!(matches!(v.rejected[0], Rejection::Clipped { switch: Switch::Heater, from_secs: 99_999, to_secs: 1800 }));
    }

    #[test]
    fn cooldown_blocks_a_second_dose() {
        let mut g = Guard::new(Limits::office_shelf());
        let dose = Command { switch: Switch::DoseNutrient, on: true, for_secs: 10 };
        assert_eq!(g.check(0, &snap(&[], &[]), &[dose]).run.len(), 1);
        let v = g.check(600, &snap(&[], &[]), &[dose]);
        assert!(v.run.is_empty());
        assert!(matches!(v.rejected[0], Rejection::Cooldown { switch: Switch::DoseNutrient, remaining_secs: 1200 }));
        assert_eq!(g.check(1800, &snap(&[], &[]), &[dose]).run.len(), 1);
    }

    #[test]
    fn hot_water_forces_heater_off_and_refuses_more_heat() {
        let mut g = Guard::new(Limits::office_shelf());
        let s = snap(&[(Quantity::WaterTempC, 35.5)], &[Switch::Heater]);
        let v = g.check(0, &s, &[Command { switch: Switch::Heater, on: true, for_secs: 600 }]);
        assert_eq!(v.alarms.len(), 1);
        assert_eq!(v.run, vec![Command { switch: Switch::Heater, on: false, for_secs: 0 }]);
        assert!(matches!(v.rejected[0], Rejection::PastHardLimit { switch: Switch::Heater, quantity: Quantity::WaterTempC, .. }));
    }

    #[test]
    fn off_commands_always_pass_and_unknown_switches_do_not() {
        let mut limits = Limits::office_shelf();
        limits.switches.remove(&Switch::Pump);
        let mut g = Guard::new(limits);
        let v = g.check(
            0,
            &snap(&[], &[]),
            &[Command { switch: Switch::Light, on: false, for_secs: 5 }, Command { switch: Switch::Pump, on: true, for_secs: 5 }],
        );
        assert_eq!(v.run, vec![Command { switch: Switch::Light, on: false, for_secs: 0 }]);
        assert_eq!(v.rejected, vec![Rejection::UnknownSwitch(Switch::Pump)]);
    }

    #[test]
    fn a_stage_can_tighten_but_never_loosen() {
        let mut g = Guard::new(Limits::office_shelf());
        let mut hard = BTreeMap::new();
        hard.insert(Quantity::AirTempC, Band::new(0.0, 35.0)); // min looser, max tighter
        hard.insert(Quantity::PpfdUmol, Band::new(0.0, 900.0)); // new quantity
        g.tighten(&hard);
        assert_eq!(g.limits.hard[&Quantity::AirTempC], Band::new(5.0, 35.0));
        assert_eq!(g.limits.hard[&Quantity::PpfdUmol], Band::new(0.0, 900.0));
        let s = snap(&[(Quantity::AirTempC, 36.0)], &[Switch::Heater]);
        assert_eq!(g.alarms(&s).len(), 1);
        g.tighten(&BTreeMap::new());
        assert_eq!(g.limits.hard[&Quantity::AirTempC], Band::new(5.0, 38.0));
        assert!(g.alarms(&s).is_empty());
    }

    #[test]
    fn no_alarm_inside_bands() {
        let g = Guard::new(Limits::office_shelf());
        assert!(g.alarms(&snap(&[(Quantity::AirTempC, 22.0), (Quantity::Ph, 6.0)], &[])).is_empty());
    }
}
