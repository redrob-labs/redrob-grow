//! Node configuration: one TOML file the dashboard's setup wizard writes and this binary reads.

use std::path::PathBuf;

use grow_hal::mycodo::MycodoConfig;
use grow_hal::tasmota::TasmotaConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node: Node,
    pub rig: RigConfig,
    #[serde(default)]
    pub planner: PlannerConfig,
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
    /// Say "I am here" on the LAN every few seconds (UDP broadcast, port 7475) so the dashboard's
    /// Find button lists this node. Carries only the name and the listen address.
    #[serde(default = "default_announce")]
    pub announce: bool,
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
fn default_announce() -> bool {
    true
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
    Tasmota(TasmotaConfig),
}

fn default_season() -> String {
    "summer".into()
}
fn default_speed() -> u32 {
    60
}

/// Who decides. `rules` needs nothing. `engine` asks a model through an OpenAI-compatible
/// endpoint and falls back to the rules when it does not answer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlannerConfig {
    #[default]
    Rules,
    Engine {
        /// Empty: the shared Redrob Code daemon on this machine (found and started through its
        /// run record). Otherwise an OpenAI-compatible base ending in `/v1`.
        #[serde(default)]
        endpoint: String,
        /// Environment variable holding a bearer token for a non-Redrob endpoint. The value never
        /// goes in this file.
        #[serde(default)]
        api_key_env: String,
        #[serde(default = "default_model")]
        model: String,
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
        /// Newest `*.jpg` here is attached to every prompt (`scripts/snap.sh` fills it).
        #[serde(default)]
        photo_dir: Option<PathBuf>,
    },
}

fn default_model() -> String {
    "redrob/auto".into()
}
fn default_timeout() -> u64 {
    90
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
                announce: true,
            },
            rig: RigConfig::Sim { season: default_season(), speed: default_speed() },
            planner: PlannerConfig::Engine {
                endpoint: String::new(),
                api_key_env: String::new(),
                model: default_model(),
                timeout_secs: default_timeout(),
                photo_dir: Some(PathBuf::from("photos")),
            },
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
        assert!(text.contains("kind = \"engine\""));
    }

    #[test]
    fn minimal_file_gets_defaults() {
        let text = "[node]\nname = \"a\"\nrecipe = \"radish-sprout\"\nsown_on = \"2026-01-01\"\n[rig]\nkind = \"sim\"\n";
        let c: NodeConfig = toml::from_str(text).unwrap();
        assert_eq!(c.node.period_secs, 600);
        assert_eq!(c.node.listen, "127.0.0.1:7474");
        assert!(c.node.announce);
        assert!(matches!(c.rig, RigConfig::Sim { speed: 60, .. }));
        assert_eq!(c.planner, PlannerConfig::Rules);
    }

    #[test]
    fn tasmota_and_engine_sections_parse() {
        let text = r#"
            [node]
            name = "a"
            recipe = "radish-sprout"
            sown_on = "2026-01-01"
            [rig]
            kind = "tasmota"
            [rig.plugs.light]
            host = "192.168.0.51"
            meters = true
            [planner]
            kind = "engine"
            endpoint = "https://api.example.com/v1"
            api_key_env = "GROW_API_KEY"
            model = "gpt-4.1-mini"
        "#;
        let c: NodeConfig = toml::from_str(text).unwrap();
        assert!(matches!(c.rig, RigConfig::Tasmota(_)));
        match c.planner {
            PlannerConfig::Engine { endpoint, api_key_env, model, timeout_secs, photo_dir } => {
                assert_eq!(endpoint, "https://api.example.com/v1");
                assert_eq!(api_key_env, "GROW_API_KEY");
                assert_eq!(model, "gpt-4.1-mini");
                assert_eq!(timeout_secs, 90);
                assert!(photo_dir.is_none());
            }
            PlannerConfig::Rules => panic!("engine expected"),
        }
    }
}
