//! A Raspberry Pi running Mycodo, driven over its REST API.
//!
//! Mycodo already does the hard, boring part: it owns the pins, the sensor drivers and the
//! database. This rig only maps each [`Quantity`] to a Mycodo measurement and each [`Switch`] to a
//! Mycodo output, and asks for every output with a duration so Mycodo turns it off by itself.
//!
//! Mycodo is GPL-3.0 software. Nothing of it is linked or vendored here; this is an HTTP client.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use ureq::Agent;
use ureq::tls::{TlsConfig, TlsProvider};

use crate::{Command, HalError, Quantity, Rig, Snapshot, Switch};

/// Where one quantity lives in Mycodo: an Input's unique id, its unit string and channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measurement {
    pub unique_id: String,
    /// Mycodo unit id, e.g. `C`, `percent`, `ppm`, `mS_cm`, `pH`.
    pub unit: String,
    #[serde(default)]
    pub channel: u8,
}

/// Where one switch lives in Mycodo: an Output's unique id and channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    pub unique_id: String,
    #[serde(default)]
    pub channel: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MycodoConfig {
    /// e.g. `https://192.168.0.42` (Mycodo's own self-signed certificate) or `http://pi.local:8080`.
    pub base_url: String,
    /// Created in Mycodo under Configure -> Users -> API key.
    pub api_key: String,
    /// Accept Mycodo's self-signed certificate. On a LAN this is the normal case.
    #[serde(default)]
    pub insecure_tls: bool,
    /// How far back a reading may be and still count, seconds.
    #[serde(default = "default_max_age")]
    pub max_age_secs: u32,
    pub measurements: BTreeMap<Quantity, Measurement>,
    pub outputs: BTreeMap<Switch, Output>,
}

fn default_max_age() -> u32 {
    600
}

pub struct MycodoRig {
    cfg: MycodoConfig,
    agent: Agent,
    /// When each switch was last commanded on, and for how long. Mycodo is the source of truth;
    /// this lets `read` report a state without a second round trip per switch.
    on_until: BTreeMap<Switch, Instant>,
}

#[derive(Deserialize)]
struct LastMeasurement {
    value: f64,
}

impl MycodoRig {
    pub fn new(cfg: MycodoConfig) -> Self {
        let tls = TlsConfig::builder().provider(TlsProvider::Rustls).disable_verification(cfg.insecure_tls).build();
        let agent: Agent = Agent::config_builder().tls_config(tls).timeout_global(Some(Duration::from_secs(10))).build().into();
        Self { cfg, agent, on_until: BTreeMap::new() }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/{}", self.cfg.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    fn last(&self, m: &Measurement) -> Result<f64, HalError> {
        let url = self.url(&format!("measurements/last/{}/{}/{}/{}", m.unique_id, m.unit, m.channel, self.cfg.max_age_secs));
        let body: LastMeasurement = self
            .agent
            .get(&url)
            .header("X-API-KEY", &self.cfg.api_key)
            .call()
            .map_err(|e| HalError::Transport(e.to_string()))?
            .body_mut()
            .read_json()
            .map_err(|e| HalError::BadReply(e.to_string()))?;
        Ok(body.value)
    }
}

impl Rig for MycodoRig {
    fn name(&self) -> &str {
        "mycodo"
    }

    fn switches(&self) -> Vec<Switch> {
        self.cfg.outputs.keys().copied().collect()
    }

    fn read(&mut self) -> Result<Snapshot, HalError> {
        let mut readings = BTreeMap::new();
        for (q, m) in &self.cfg.measurements {
            readings.insert(*q, self.last(m)?);
        }
        let now = Instant::now();
        let on = self.cfg.outputs.keys().map(|s| (*s, self.on_until.get(s).is_some_and(|t| *t > now))).collect();
        Ok(Snapshot { readings, on })
    }

    fn set(&mut self, cmd: Command) -> Result<(), HalError> {
        let out = self.cfg.outputs.get(&cmd.switch).ok_or(HalError::NotWired(cmd.switch))?;
        let url = self.url(&format!("outputs/{}", out.unique_id));
        let body = serde_json::json!({
            "channel": out.channel,
            "state": cmd.on,
            "duration": if cmd.on { cmd.for_secs } else { 0 },
        });
        self.agent.post(&url).header("X-API-KEY", &self.cfg.api_key).send_json(&body).map_err(|e| HalError::Transport(e.to_string()))?;
        if cmd.on {
            self.on_until.insert(cmd.switch, Instant::now() + Duration::from_secs(u64::from(cmd.for_secs)));
        } else {
            self.on_until.remove(&cmd.switch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    type Posts = Arc<Mutex<Vec<(String, String)>>>;

    /// A tiny fake Mycodo: answers one measurement and records every output POST.
    fn fake_server() -> (String, Posts) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let posts = Arc::new(Mutex::new(Vec::new()));
        let seen = posts.clone();
        thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let key = req.headers().iter().find(|h| h.field.equiv("X-API-KEY")).map(|h| h.value.to_string());
                if key.as_deref() != Some("secret") {
                    let _ = req.respond(tiny_http::Response::from_string("{}").with_status_code(401));
                    continue;
                }
                let url = req.url().to_string();
                if url.starts_with("/api/measurements/last/") {
                    let _ = req.respond(tiny_http::Response::from_string(r#"{"time":"t","value":21.5}"#));
                } else if url.starts_with("/api/outputs/") {
                    let mut body = String::new();
                    req.as_reader().read_to_string(&mut body).unwrap();
                    seen.lock().unwrap().push((url, body));
                    let _ = req.respond(tiny_http::Response::from_string(r#"{"message":"ok"}"#));
                } else {
                    let _ = req.respond(tiny_http::Response::from_string("{}").with_status_code(404));
                }
            }
        });
        (format!("http://{addr}"), posts)
    }

    fn cfg(base_url: String, key: &str) -> MycodoConfig {
        let mut measurements = BTreeMap::new();
        measurements.insert(Quantity::WaterTempC, Measurement { unique_id: "abc".into(), unit: "C".into(), channel: 0 });
        let mut outputs = BTreeMap::new();
        outputs.insert(Switch::Heater, Output { unique_id: "out1".into(), channel: 0 });
        MycodoConfig { base_url, api_key: key.into(), insecure_tls: false, max_age_secs: 600, measurements, outputs }
    }

    #[test]
    fn reads_and_switches_through_the_api() {
        let (base, posts) = fake_server();
        let mut rig = MycodoRig::new(cfg(base, "secret"));
        assert_eq!(rig.switches(), vec![Switch::Heater]);

        let snap = rig.read().unwrap();
        assert_eq!(snap.get(Quantity::WaterTempC), Some(21.5));
        assert!(!snap.is_on(Switch::Heater));

        rig.set(Command { switch: Switch::Heater, on: true, for_secs: 600 }).unwrap();
        assert!(rig.read().unwrap().is_on(Switch::Heater));
        let posts = posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].0, "/api/outputs/out1");
        let body: serde_json::Value = serde_json::from_str(&posts[0].1).unwrap();
        assert_eq!(body["state"], true);
        assert_eq!(body["duration"], 600);
    }

    #[test]
    fn wrong_key_is_a_transport_error_and_unwired_is_refused() {
        let (base, _) = fake_server();
        let mut rig = MycodoRig::new(cfg(base, "wrong"));
        assert!(matches!(rig.read(), Err(HalError::Transport(_))));
        assert!(matches!(rig.set(Command { switch: Switch::Light, on: true, for_secs: 1 }), Err(HalError::NotWired(Switch::Light))));
    }

    #[test]
    fn config_round_trips_through_toml() {
        let c = cfg("https://pi.local".into(), "k");
        let text = toml::to_string(&c).unwrap();
        let back: MycodoConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, c);
    }
}
