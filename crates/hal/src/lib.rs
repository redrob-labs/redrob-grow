//! Hardware abstraction: everything above this crate talks to a [`Rig`], never to a pin.
//!
//! Two rigs ship today. [`mock::OfficeRig`] is a small physics model of a shelf in an
//! air-conditioned office, so the agent and the dashboard can be developed and tested with no
//! hardware at all. [`mycodo::MycodoRig`] drives a Raspberry Pi running Mycodo over its REST API.
//! [`tasmota::TasmotaRig`] drives Tasmota smart plugs and sensor nodes over their HTTP command
//! API, with no Pi in between. A direct I2C rig is planned (see the roadmap).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use grow_recipe::Quantity;

pub mod mock;
#[cfg(feature = "mycodo")]
pub mod mycodo;
#[cfg(feature = "tasmota")]
pub mod tasmota;

/// What a switch is wired to. The agent reasons about these, never about plug numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    /// Grow light.
    Light,
    /// Water or air heater in the reservoir.
    Heater,
    /// Air circulation fan.
    Fan,
    /// Circulation or air pump.
    Pump,
    /// Humidifier.
    Humidifier,
    /// Nutrient dosing pump (concentrate A+B).
    DoseNutrient,
    /// pH-down dosing pump.
    DosePhDown,
    /// Top-up valve from a fresh-water tank.
    TopUp,
}

impl Switch {
    pub const ALL: [Switch; 8] =
        [Switch::Light, Switch::Heater, Switch::Fan, Switch::Pump, Switch::Humidifier, Switch::DoseNutrient, Switch::DosePhDown, Switch::TopUp];

    /// Typical electrical draw, watts, used for the kWh tally when the plug cannot report power.
    pub fn nominal_watts(self) -> f64 {
        match self {
            Switch::Light => 120.0,
            Switch::Heater => 100.0,
            Switch::Fan => 10.0,
            Switch::Pump => 8.0,
            Switch::Humidifier => 25.0,
            Switch::DoseNutrient | Switch::DosePhDown | Switch::TopUp => 3.0,
        }
    }
}

/// One measurement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub quantity: Quantity,
    pub value: f64,
}

/// Everything read in one pass, plus which switches are currently on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub readings: BTreeMap<Quantity, f64>,
    pub on: BTreeMap<Switch, bool>,
    /// Cumulative energy the rig has measured, kWh, when it has a meter. The run summary turns
    /// the difference between first and last cycle into measured consumption.
    #[serde(default)]
    pub energy_kwh: Option<f64>,
}

impl Snapshot {
    pub fn get(&self, q: Quantity) -> Option<f64> {
        self.readings.get(&q).copied()
    }

    pub fn is_on(&self, s: Switch) -> bool {
        self.on.get(&s).copied().unwrap_or(false)
    }
}

/// A command to a switch. `for_secs` is mandatory: a rig turns the switch off again by itself
/// when it elapses, so a dead agent cannot leave a heater on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Command {
    pub switch: Switch,
    pub on: bool,
    pub for_secs: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum HalError {
    #[error("switch {0:?} is not wired on this rig")]
    NotWired(Switch),
    #[error("transport: {0}")]
    Transport(String),
    #[error("bad reply from rig: {0}")]
    BadReply(String),
}

/// A set of sensors and switches. Implementations must be safe to call every few minutes forever.
pub trait Rig {
    /// Human name shown in the dashboard.
    fn name(&self) -> &str;
    /// Which switches exist on this rig.
    fn switches(&self) -> Vec<Switch>;
    /// Read every sensor once.
    fn read(&mut self) -> Result<Snapshot, HalError>;
    /// Drive one switch for a bounded time.
    fn set(&mut self, cmd: Command) -> Result<(), HalError>;
    /// Advance simulated time, no-op on real hardware. Lets the agent loop be tested in seconds.
    fn tick(&mut self, _secs: u32) {}
    /// Simulated seconds since a Monday 00:00, for rigs that have their own clock. Real rigs
    /// return `None` and the node uses the wall clock.
    fn sim_time(&self) -> Option<u64> {
        None
    }
}

impl Rig for Box<dyn Rig> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn switches(&self) -> Vec<Switch> {
        (**self).switches()
    }
    fn read(&mut self) -> Result<Snapshot, HalError> {
        (**self).read()
    }
    fn set(&mut self, cmd: Command) -> Result<(), HalError> {
        (**self).set(cmd)
    }
    fn tick(&mut self, secs: u32) {
        (**self).tick(secs)
    }
    fn sim_time(&self) -> Option<u64> {
        (**self).sim_time()
    }
}
