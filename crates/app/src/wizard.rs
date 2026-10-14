//! The setup wizard's data: four steps that end in a `node.toml` for `grow-node`.
//!
//! Kept free of egui so the TOML it produces is tested here and the screen only draws it.

use std::collections::BTreeMap;

use grow_hal::Switch;
use grow_recipe::builtin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RigKind {
    Sim,
    Mycodo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Wizard {
    pub step: usize,
    pub name: String,
    pub recipe_id: String,
    pub sown_on: String,
    pub rig: RigKind,
    pub season_winter: bool,
    pub mycodo_url: String,
    pub mycodo_key: String,
    pub mycodo_insecure: bool,
    /// Switch -> Mycodo output unique id. Empty string means not wired.
    pub outputs: BTreeMap<Switch, String>,
    /// Quantity name -> (input unique id, unit). Only the five a shelf usually has.
    pub inputs: Vec<(grow_hal::Quantity, String, String)>,
    pub save_path: String,
    pub saved: Option<Result<String, String>>,
}

pub const STEPS: [&str; 4] = ["Crop", "Hardware", "Wiring", "Save"];

impl Default for Wizard {
    fn default() -> Self {
        let mut outputs = BTreeMap::new();
        for s in [Switch::Light, Switch::Heater, Switch::Fan, Switch::Pump] {
            outputs.insert(s, String::new());
        }
        use grow_hal::Quantity as Q;
        let inputs = vec![
            (Q::AirTempC, String::new(), "C".to_owned()),
            (Q::HumidityPct, String::new(), "percent".to_owned()),
            (Q::WaterTempC, String::new(), "C".to_owned()),
            (Q::Co2Ppm, String::new(), "ppm".to_owned()),
            (Q::EcMs, String::new(), "mS_cm".to_owned()),
            (Q::Ph, String::new(), "pH".to_owned()),
        ];
        Self {
            step: 0,
            name: "shelf-1".into(),
            recipe_id: "radish-sprout".into(),
            sown_on: String::new(),
            rig: RigKind::Sim,
            season_winter: false,
            mycodo_url: "https://192.168.0.42".into(),
            mycodo_key: String::new(),
            mycodo_insecure: true,
            outputs,
            inputs,
            save_path: "node.toml".into(),
            saved: None,
        }
    }
}

impl Wizard {
    pub fn recipe_ids() -> Vec<String> {
        builtin::all().into_iter().map(|r| r.id).collect()
    }

    /// Problems that block moving past the current step, in plain words.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.step {
            0 => {
                if self.name.trim().is_empty() {
                    out.push("Give the shelf a name.".into());
                }
                if self.sown_on.len() != 10 || self.sown_on.split('-').count() != 3 {
                    out.push("Sowing date as YYYY-MM-DD.".into());
                }
            }
            1 if self.rig == RigKind::Mycodo => {
                if !self.mycodo_url.starts_with("http") {
                    out.push("Mycodo address must start with http:// or https://.".into());
                }
                if self.mycodo_key.trim().is_empty() {
                    out.push("Paste the Mycodo API key (Configure > Users).".into());
                }
            }
            2 if self.rig == RigKind::Mycodo => {
                if self.outputs.values().all(|v| v.trim().is_empty()) {
                    out.push("Wire at least one switch to a Mycodo output.".into());
                }
                if self.inputs.iter().all(|(_, id, _)| id.trim().is_empty()) {
                    out.push("Wire at least one sensor to a Mycodo input.".into());
                }
            }
            _ => {}
        }
        out
    }

    /// The `node.toml` this wizard describes.
    pub fn to_toml(&self) -> String {
        let mut t = String::new();
        t.push_str("# Written by the Redrob Grow setup wizard. Run with: grow-node run node.toml\n\n");
        t.push_str("[node]\n");
        t.push_str(&format!("name = {:?}\nrecipe = {:?}\nsown_on = {:?}\n", self.name.trim(), self.recipe_id, self.sown_on.trim()));
        t.push_str("period_secs = 600\ntz_offset_hours = 9.0\nlisten = \"127.0.0.1:7474\"\nruns_dir = \"runs\"\n\n");
        match self.rig {
            RigKind::Sim => {
                t.push_str("[rig]\nkind = \"sim\"\n");
                t.push_str(&format!("season = {:?}\nspeed = 60\n", if self.season_winter { "winter" } else { "summer" }));
            }
            RigKind::Mycodo => {
                t.push_str("[rig]\nkind = \"mycodo\"\n");
                t.push_str(&format!(
                    "base_url = {:?}\napi_key = {:?}\ninsecure_tls = {}\nmax_age_secs = 600\n\n",
                    self.mycodo_url.trim(),
                    self.mycodo_key.trim(),
                    self.mycodo_insecure
                ));
                for (q, id, unit) in &self.inputs {
                    if !id.trim().is_empty() {
                        t.push_str(&format!("[rig.measurements.{}]\nunique_id = {:?}\nunit = {:?}\nchannel = 0\n\n", snake(q), id.trim(), unit));
                    }
                }
                for (s, id) in &self.outputs {
                    if !id.trim().is_empty() {
                        t.push_str(&format!("[rig.outputs.{}]\nunique_id = {:?}\nchannel = 0\n\n", snake(s), id.trim()));
                    }
                }
            }
        }
        t
    }

    pub fn save(&mut self) {
        let text = self.to_toml();
        self.saved = Some(std::fs::write(&self.save_path, text).map(|()| self.save_path.clone()).map_err(|e| e.to_string()));
    }
}

fn snake<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_toml_is_a_valid_node_config_shape() {
        let mut w = Wizard { sown_on: "2026-10-20".into(), ..Default::default() };
        w.recipe_id = "lettuce-butterhead".into();
        let text = w.to_toml();
        let v: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(v["node"]["recipe"].as_str(), Some("lettuce-butterhead"));
        assert_eq!(v["rig"]["kind"].as_str(), Some("sim"));
        assert_eq!(v["rig"]["season"].as_str(), Some("summer"));
        assert!(w.problems().is_empty());
    }

    #[test]
    fn mycodo_toml_only_lists_wired_things() {
        let mut w = Wizard { rig: RigKind::Mycodo, sown_on: "2026-10-20".into(), ..Default::default() };
        w.mycodo_key = "k".into();
        w.outputs.insert(Switch::Heater, "out-1".into());
        w.inputs[2].1 = "in-water".into();
        let v: toml::Value = toml::from_str(&w.to_toml()).unwrap();
        assert_eq!(v["rig"]["kind"].as_str(), Some("mycodo"));
        assert_eq!(v["rig"]["outputs"]["heater"]["unique_id"].as_str(), Some("out-1"));
        assert!(v["rig"]["outputs"].get("light").is_none());
        assert_eq!(v["rig"]["measurements"]["water_temp_c"]["unit"].as_str(), Some("C"));
        assert!(v["rig"]["measurements"].get("air_temp_c").is_none());
    }

    #[test]
    fn problems_block_each_step() {
        let mut w = Wizard::default();
        assert!(w.problems().iter().any(|p| p.contains("YYYY")));
        w.sown_on = "2026-10-20".into();
        assert!(w.problems().is_empty());
        w.step = 1;
        w.rig = RigKind::Mycodo;
        assert!(w.problems().iter().any(|p| p.contains("API key")));
        w.mycodo_key = "k".into();
        w.step = 2;
        assert_eq!(w.problems().len(), 2);
        assert_eq!(Wizard::recipe_ids().len(), 3);
    }
}
