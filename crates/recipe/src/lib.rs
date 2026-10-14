//! A recipe is what a crop wants, day by day, written as data.
//!
//! The agent never invents a target. It reads the stage the crop is in today and tries to hold
//! the room inside that stage's ranges. A recipe is a TOML file; the three the repository ships
//! with are embedded so a node can start with no network.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Every quantity the system can measure. One enum shared by recipes, sensors and safety limits
/// so a target and a reading can never disagree about what they name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    /// Air temperature at canopy height, degrees Celsius.
    AirTempC,
    /// Water (or nutrient solution) temperature, degrees Celsius.
    WaterTempC,
    /// Relative humidity, percent.
    HumidityPct,
    /// Carbon dioxide, parts per million.
    Co2Ppm,
    /// Electrical conductivity of the nutrient solution, mS/cm.
    EcMs,
    /// pH of the nutrient solution.
    Ph,
    /// Water level in the reservoir, percent of full.
    WaterLevelPct,
    /// Photosynthetic photon flux density at canopy, umol/m2/s.
    PpfdUmol,
}

impl Quantity {
    pub const ALL: [Quantity; 8] = [
        Quantity::AirTempC,
        Quantity::WaterTempC,
        Quantity::HumidityPct,
        Quantity::Co2Ppm,
        Quantity::EcMs,
        Quantity::Ph,
        Quantity::WaterLevelPct,
        Quantity::PpfdUmol,
    ];

    pub fn unit(self) -> &'static str {
        match self {
            Quantity::AirTempC | Quantity::WaterTempC => "C",
            Quantity::HumidityPct | Quantity::WaterLevelPct => "%",
            Quantity::Co2Ppm => "ppm",
            Quantity::EcMs => "mS/cm",
            Quantity::Ph => "pH",
            Quantity::PpfdUmol => "umol/m2/s",
        }
    }
}

/// An inclusive target band. `min <= max` is checked by [`Recipe::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Band {
    pub min: f64,
    pub max: f64,
}

impl Band {
    pub fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    pub fn mid(&self) -> f64 {
        (self.min + self.max) / 2.0
    }

    pub fn contains(&self, v: f64) -> bool {
        v >= self.min && v <= self.max
    }

    /// Signed distance outside the band: negative below `min`, positive above `max`, 0 inside.
    pub fn error(&self, v: f64) -> f64 {
        if v < self.min {
            v - self.min
        } else if v > self.max {
            v - self.max
        } else {
            0.0
        }
    }
}

/// Light schedule for a stage. Hours are local wall-clock hours, 0..24.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Light {
    /// Hours of light per day.
    pub hours: f64,
    /// Hour the lights come on. The agent may shift this to follow the room (see `grow-agent`).
    pub on_hour: f64,
}

/// One growth stage: a start day and the bands to hold while it lasts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    /// First day of this stage, counting from 0 at sowing.
    pub from_day: u32,
    pub light: Light,
    #[serde(default)]
    pub targets: BTreeMap<Quantity, Band>,
    /// Free text for the human and for the agent's prompt. Never parsed.
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    /// Stable id, `kebab-case`. Doubles as the file name.
    pub id: String,
    pub crop: String,
    #[serde(default)]
    pub variety: String,
    /// Days from sowing to harvest under this recipe.
    pub days: u32,
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub sources: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RecipeError {
    #[error("recipe has no stages")]
    NoStages,
    #[error("first stage must start at day 0, starts at {0}")]
    FirstStageNotZero(u32),
    #[error("stage `{0}` starts at day {1}, not after the previous stage")]
    StagesOutOfOrder(String, u32),
    #[error("stage `{0}` starts at day {1} but the recipe ends at day {2}")]
    StagePastEnd(String, u32, u32),
    #[error("stage `{0}` target {1:?} has min {2} > max {3}")]
    BandInverted(String, Quantity, f64, f64),
    #[error("stage `{0}` light hours {1} outside 0..=24")]
    LightHours(String, f64),
    #[error("stage `{0}` light on_hour {1} outside 0..24")]
    LightOnHour(String, f64),
    #[error("parse: {0}")]
    Parse(String),
}

impl Recipe {
    pub fn from_toml(text: &str) -> Result<Self, RecipeError> {
        let r: Recipe = toml::from_str(text).map_err(|e| RecipeError::Parse(e.to_string()))?;
        r.validate()?;
        Ok(r)
    }

    /// Reports the first problem found. Recipes are small, so one at a time is fine here; the
    /// node refuses to start on any error.
    pub fn validate(&self) -> Result<(), RecipeError> {
        let first = self.stages.first().ok_or(RecipeError::NoStages)?;
        if first.from_day != 0 {
            return Err(RecipeError::FirstStageNotZero(first.from_day));
        }
        let mut last = None;
        for s in &self.stages {
            if let Some(prev) = last
                && s.from_day <= prev
            {
                return Err(RecipeError::StagesOutOfOrder(s.name.clone(), s.from_day));
            }
            if s.from_day >= self.days {
                return Err(RecipeError::StagePastEnd(s.name.clone(), s.from_day, self.days));
            }
            for (q, b) in &s.targets {
                if b.min > b.max {
                    return Err(RecipeError::BandInverted(s.name.clone(), *q, b.min, b.max));
                }
            }
            if !(0.0..=24.0).contains(&s.light.hours) {
                return Err(RecipeError::LightHours(s.name.clone(), s.light.hours));
            }
            if !(0.0..24.0).contains(&s.light.on_hour) {
                return Err(RecipeError::LightOnHour(s.name.clone(), s.light.on_hour));
            }
            last = Some(s.from_day);
        }
        Ok(())
    }

    /// The stage in force on `day` (0 = sowing). Past the last day the last stage still applies,
    /// so a crop left one day too long is held, not abandoned.
    pub fn stage_at(&self, day: u32) -> &Stage {
        self.stages.iter().rev().find(|s| s.from_day <= day).unwrap_or(&self.stages[0])
    }

    pub fn is_done(&self, day: u32) -> bool {
        day >= self.days
    }
}

/// Recipes shipped in the repository, embedded at build time.
pub mod builtin {
    use super::Recipe;

    pub const RADISH_SPROUT: &str = include_str!("../../../recipes/radish-sprout.toml");
    pub const LETTUCE_BUTTERHEAD: &str = include_str!("../../../recipes/lettuce-butterhead.toml");
    pub const RICE_SINDONGJIN: &str = include_str!("../../../recipes/rice-sindongjin.toml");

    pub fn all() -> Vec<Recipe> {
        [RADISH_SPROUT, LETTUCE_BUTTERHEAD, RICE_SINDONGJIN]
            .iter()
            .map(|t| Recipe::from_toml(t).expect("built-in recipe is valid; tested in CI"))
            .collect()
    }

    pub fn by_id(id: &str) -> Option<Recipe> {
        all().into_iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_recipes_parse_and_validate() {
        let all = builtin::all();
        assert_eq!(all.len(), 3);
        let ids: Vec<_> = all.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["radish-sprout", "lettuce-butterhead", "rice-sindongjin"]);
    }

    #[test]
    fn stage_lookup_follows_days() {
        let r = builtin::by_id("lettuce-butterhead").unwrap();
        assert_eq!(r.stage_at(0).name, "germination");
        assert_eq!(r.stage_at(r.days + 5).name, r.stages.last().unwrap().name);
        assert!(r.is_done(r.days));
        assert!(!r.is_done(r.days - 1));
    }

    #[test]
    fn band_error_is_signed() {
        let b = Band::new(18.0, 22.0);
        assert_eq!(b.error(20.0), 0.0);
        assert_eq!(b.error(16.0), -2.0);
        assert_eq!(b.error(25.0), 3.0);
        assert!(b.contains(22.0));
    }

    #[test]
    fn validation_catches_bad_recipes() {
        let base = builtin::by_id("radish-sprout").unwrap();

        let mut r = base.clone();
        r.stages.clear();
        assert_eq!(r.validate(), Err(RecipeError::NoStages));

        let mut r = base.clone();
        r.stages[0].from_day = 1;
        assert_eq!(r.validate(), Err(RecipeError::FirstStageNotZero(1)));

        let mut r = base.clone();
        r.stages[1].from_day = 0;
        assert!(matches!(r.validate(), Err(RecipeError::StagesOutOfOrder(..))));

        let mut r = base.clone();
        r.stages[0].targets.insert(Quantity::AirTempC, Band::new(30.0, 20.0));
        assert!(matches!(r.validate(), Err(RecipeError::BandInverted(_, Quantity::AirTempC, _, _))));

        let mut r = base.clone();
        r.stages[0].light.hours = 25.0;
        assert!(matches!(r.validate(), Err(RecipeError::LightHours(..))));

        let mut r = base;
        r.days = 1;
        assert!(matches!(r.validate(), Err(RecipeError::StagePastEnd(..))));
    }

    #[test]
    fn parse_error_is_reported() {
        assert!(matches!(Recipe::from_toml("id = 3"), Err(RecipeError::Parse(_))));
    }
}
