//! Node configuration: one TOML file the dashboard's setup wizard writes and this binary reads.

use std::path::PathBuf;

use grow_hal::mycodo::MycodoConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node: Node,
    pub rig: RigConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Shown in the dashboard, used in run ids.
    pub name: String,
    /// A built-in recipe id or a path to a recipe TOML file.
    pub recipe: String,
    /// Local date of sowing, `YYYY-MM-DD`.
    pub sown_on: String,
    /// Seconds between cycles. 600 is a sensible floor for a real shelf.
    #[serde(default = "default_period")]
    pub period_secs: u32,
    /// Local time offset from UTC in hours. Korea is 9.
    #[serde(default = "default_tz")]
    pub tz_offset_hours: f64,
    /// Loopback only by default. The dashboard reaches a remote node through an SSH tunnel or a
    /// LAN address you choose to put here on purpose.
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_runs_dir")]
    pub runs_dir: PathBuf,
}

fn default_period() -> u32 {
    600
}
fn default_tz() -> f64 {
    9.0
}
fn default_listen() -> String {
    "127.0.0.1:7474".into()
}
fn default_runs_dir() -> PathBuf {
    PathBuf::from("runs")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RigConfig {
    /// The simulated office. `season` is `summer` or `winter`; `speed` is simulated seconds per
    /// real second (60 means one simulated hour per real minute).
    Sim {
        #[serde(default = "default_season")]
        season: String,
        #[serde(default = "default_speed")]
        speed: u32,
    },
    Mycodo(MycodoConfig),
}

fn default_season() -> String {
    "summer".into()
}
fn default_speed() -> u32 {
    60
}

impl NodeConfig {
    pub fn example() -> Self {
        Self {
            node: Node {
                name: "shelf-1".into(),
                recipe: "lettuce-butterhead".into(),
                sown_on: "2026-10-20".into(),
                period_secs: default_period(),
                tz_offset_hours: default_tz(),
                listen: default_listen(),
                runs_dir: default_runs_dir(),
            },
            rig: RigConfig::Sim { season: default_season(), speed: default_speed() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_round_trips() {
        let c = NodeConfig::example();
        let text = toml::to_string_pretty(&c).unwrap();
        let back: NodeConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
        assert!(text.contains("kind = \"sim\""));
    }

    #[test]
    fn minimal_file_gets_defaults() {
        let text = "[node]\nname = \"a\"\nrecipe = \"radish-sprout\"\nsown_on = \"2026-01-01\"\n[rig]\nkind = \"sim\"\n";
        let c: NodeConfig = toml::from_str(text).unwrap();
        assert_eq!(c.node.period_secs, 600);
        assert_eq!(c.node.listen, "127.0.0.1:7474");
        assert!(matches!(c.rig, RigConfig::Sim { speed: 60, .. }));
    }
}
