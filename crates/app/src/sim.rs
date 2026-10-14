//! The built-in simulated shelf: a `Cycle` over `OfficeRig`, fast-forwarded in-process.

use grow_agent::{Clock, Cycle, Report, RulePlanner};
use grow_hal::Rig;
use grow_hal::mock::{Office, OfficeRig, SimTime};
use grow_recipe::{Recipe, builtin};
use grow_safety::{Guard, Limits};

pub const PERIOD_SECS: u32 = 600;

pub struct Sim {
    cycle: Cycle<OfficeRig, RulePlanner>,
    sown: SimTime,
    /// Simulated seconds advanced per real second.
    pub speed: u32,
    carry: f64,
    pub history: Vec<Report>,
    pub season: Season,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Season {
    Summer,
    Winter,
}

impl Sim {
    pub fn new(recipe: Recipe, season: Season) -> Self {
        let office = match season {
            Season::Summer => Office::summer(),
            Season::Winter => Office::winter(),
        };
        let rig = OfficeRig::new(office);
        let sown = rig.now;
        let mut sim = Self {
            cycle: Cycle::new(rig, RulePlanner::default(), Guard::new(Limits::office_shelf()), recipe),
            sown,
            speed: 3600,
            carry: 0.0,
            history: Vec::new(),
            season,
        };
        sim.step_cycle();
        sim
    }

    pub fn kwh(&self) -> f64 {
        self.cycle.rig.kwh()
    }

    pub fn now(&self) -> SimTime {
        self.cycle.rig.now
    }

    pub fn latest(&self) -> Option<&Report> {
        self.history.last()
    }

    fn clock(&self) -> Clock {
        let now = self.cycle.rig.now;
        Clock { now_secs: now.0, hour: now.hour_f(), weekday: now.is_weekday(), day: ((now.0 - self.sown.0) / SimTime::DAY) as u32 }
    }

    fn step_cycle(&mut self) {
        let c = self.clock();
        if let Ok(r) = self.cycle.run(c) {
            if self.history.len() >= 2016 {
                self.history.remove(0);
            }
            self.history.push(r);
        }
        self.cycle.rig.tick(PERIOD_SECS);
    }

    /// Advance by `dt` real seconds at the current speed.
    pub fn advance(&mut self, dt: f64) {
        self.carry += dt * f64::from(self.speed);
        let mut budget = 50; // never stall a frame
        while self.carry >= f64::from(PERIOD_SECS) && budget > 0 {
            self.carry -= f64::from(PERIOD_SECS);
            self.step_cycle();
            budget -= 1;
        }
        if budget == 0 {
            self.carry = 0.0;
        }
    }

    pub fn reset(&mut self, recipe_id: &str, season: Season) {
        if let Some(r) = builtin::by_id(recipe_id) {
            *self = Self::new(r, season);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advancing_produces_cycles_and_energy() {
        let mut sim = Sim::new(builtin::by_id("lettuce-butterhead").unwrap(), Season::Winter);
        assert_eq!(sim.history.len(), 1);
        sim.speed = 36_000;
        sim.advance(1.0); // 10 simulated hours
        assert!(sim.history.len() >= 50, "{}", sim.history.len());
        assert!(sim.kwh() > 0.0);
        assert_eq!(sim.latest().unwrap().observation.recipe_id, "lettuce-butterhead");
    }

    #[test]
    fn a_frame_never_runs_more_than_fifty_cycles() {
        let mut sim = Sim::new(builtin::by_id("radish-sprout").unwrap(), Season::Summer);
        sim.speed = 1_000_000;
        sim.advance(10.0);
        assert!(sim.history.len() <= 51);
    }
}
