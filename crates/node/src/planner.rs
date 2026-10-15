//! Builds the planner the config asks for.
//!
//! `engine` with an empty endpoint means the shared Redrob Code daemon on this machine: the
//! engine-client crate reads the daemon's run record, starts the daemon if none answers, and
//! gives back the OpenAI-compatible base and the headers every Redrob app puts on its calls (the
//! daemon's credential and the app name `grow`). With an endpoint, any `/v1` works and the bearer
//! token comes from the named environment variable, never from the config file.

use grow_agent::{EngineConfig, EnginePlanner, Planner, RulePlanner};

use crate::config::PlannerConfig;

/// Minutes the daemon stays up with nobody attached. A shelf is attached for weeks, so this only
/// matters after the node stops.
const IDLE_EXIT_MINUTES: u32 = 30;

pub fn build(cfg: &PlannerConfig) -> Result<Box<dyn Planner>, String> {
    match cfg {
        PlannerConfig::Rules => Ok(Box::new(RulePlanner::default())),
        PlannerConfig::Engine { endpoint, api_key_env, model, timeout_secs, photo_dir } => {
            let (base_url, headers) = if endpoint.trim().is_empty() {
                let record = redrob_engine_client::require_as("grow", IDLE_EXIT_MINUTES).map_err(|e| e.to_string())?;
                let headers = redrob_engine_client::app_headers(&record, "grow", None).into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
                (redrob_engine_client::openai_base(&record), headers)
            } else {
                let mut headers = Vec::new();
                if !api_key_env.is_empty() {
                    let key = std::env::var(api_key_env).map_err(|_| format!("environment variable {api_key_env} is not set"))?;
                    headers.push(("Authorization".to_owned(), format!("Bearer {key}")));
                }
                (endpoint.trim_end_matches('/').to_owned(), headers)
            };
            let engine = EngineConfig { base_url, headers, model: model.clone(), timeout_secs: *timeout_secs, photo_dir: photo_dir.clone() };
            Ok(Box::new(EnginePlanner::new(engine, RulePlanner::default())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_and_explicit_endpoint_build_without_a_daemon() {
        assert_eq!(build(&PlannerConfig::Rules).unwrap().name(), "rules");
        let cfg = PlannerConfig::Engine {
            endpoint: "http://127.0.0.1:9/v1".into(),
            api_key_env: String::new(),
            model: "redrob/auto".into(),
            timeout_secs: 5,
            photo_dir: None,
        };
        assert_eq!(build(&cfg).unwrap().name(), "engine");
        let cfg = PlannerConfig::Engine {
            endpoint: "http://127.0.0.1:9/v1".into(),
            api_key_env: "GROW_TEST_KEY_THAT_IS_NOT_SET".into(),
            model: "redrob/auto".into(),
            timeout_secs: 5,
            photo_dir: None,
        };
        match build(&cfg) {
            Err(e) => assert!(e.contains("GROW_TEST_KEY_THAT_IS_NOT_SET")),
            Ok(_) => panic!("missing key must refuse"),
        }
    }
}
