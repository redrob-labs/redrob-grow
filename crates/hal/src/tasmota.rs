//! Tasmota smart plugs and sensor nodes, over their own HTTP command API.
//!
//! No Raspberry Pi, no soldering, no broker: every plug is a Wi-Fi device with a relay, usually a
//! power meter, and `GET /cm?cmnd=...`. A switch is one relay on one plug; a reading is one path
//! into a Tasmota sensor node's `Status 10` JSON (an ESP32 flashed with Tasmota and an I2C
//! SHT3x/SCD4x/DS18B20 wired to it). The plug's own `PulseTime` turns the relay off when the
//! command's duration elapses, so a dead agent cannot leave a heater on. Plugs that meter report
//! `ENERGY.Total` and the rig sums it into [`Snapshot::energy_kwh`], which is the measured kWh the
//! Stage 1 gate compares with the estimate.
//!
//! Tasmota is GPL-3.0 firmware. Nothing of it is linked or vendored here; this is an HTTP client.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ureq::Agent;

use crate::{Command, HalError, Quantity, Rig, Snapshot, Switch};

/// One relay on one plug.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plug {
    /// `192.168.0.51` or `plug-light.local`. Plain HTTP on the LAN.
    pub host: String,
    /// Relay number, 1 for a single-outlet plug.
    #[serde(default = "default_relay")]
    pub relay: u8,
    /// Whether this plug has a power meter (`Status 8` answers with `ENERGY`).
    #[serde(default)]
    pub meters: bool,
}

fn default_relay() -> u8 {
    1
}

/// One reading on one sensor node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sensor {
    pub host: String,
    /// Dotted path under `StatusSNS`, e.g. `SHT3X.Temperature`, `SCD40.CarbonDioxide`, `DS18B20.Temperature`.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TasmotaConfig {
    /// Web admin user and password if the plugs have one set. Empty means none.
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    pub plugs: BTreeMap<Switch, Plug>,
    #[serde(default)]
    pub sensors: BTreeMap<Quantity, Sensor>,
}

fn default_timeout() -> u64 {
    5
}

pub struct TasmotaRig {
    cfg: TasmotaConfig,
    agent: Agent,
    /// Last known relay state, refreshed on every read.
    on: BTreeMap<Switch, bool>,
}

impl TasmotaRig {
    pub fn new(cfg: TasmotaConfig) -> Self {
        let agent =
            Agent::config_builder().timeout_global(Some(Duration::from_secs(cfg.timeout_secs.max(1)))).http_status_as_error(false).build().into();
        Self { cfg, agent, on: BTreeMap::new() }
    }

    /// `PulseTime` encoding: 1..=111 are tenths of a second, 112..=64900 are seconds plus 100.
    pub fn pulse_time(secs: u32) -> u32 {
        if secs <= 11 { (secs * 10).max(1) } else { (secs + 100).min(64_900) }
    }

    fn url(&self, host: &str, cmnd: &str) -> String {
        let mut url = format!("http://{host}/cm?cmnd={}", urlencode(cmnd));
        if !self.cfg.user.is_empty() {
            url.push_str(&format!("&user={}&password={}", urlencode(&self.cfg.user), urlencode(&self.cfg.password)));
        }
        url
    }

    fn cm(&self, host: &str, cmnd: &str) -> Result<Value, HalError> {
        let url = self.url(host, cmnd);
        let mut resp = self.agent.get(&url).call().map_err(|e| HalError::Transport(format!("{host}: {e}")))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(|e| HalError::Transport(format!("{host}: {e}")))?;
        if status != 200 {
            return Err(HalError::Transport(format!("{host} answered {status}")));
        }
        serde_json::from_str(&text).map_err(|e| HalError::BadReply(format!("{host}: {e}")))
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Walk `a.b.c` into a JSON value.
fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, key| cur.get(key))
}

impl Rig for TasmotaRig {
    fn name(&self) -> &str {
        "tasmota"
    }

    fn switches(&self) -> Vec<Switch> {
        self.cfg.plugs.keys().copied().collect()
    }

    fn read(&mut self) -> Result<Snapshot, HalError> {
        let mut snap = Snapshot::default();
        // Group sensor paths by host so each node is asked once.
        let mut by_host: BTreeMap<&str, Vec<(Quantity, &str)>> = BTreeMap::new();
        for (q, s) in &self.cfg.sensors {
            by_host.entry(s.host.as_str()).or_default().push((*q, s.path.as_str()));
        }
        for (host, wants) in by_host {
            let v = self.cm(host, "Status 10")?;
            let sns = v.get("StatusSNS").ok_or_else(|| HalError::BadReply(format!("{host}: no StatusSNS")))?;
            for (q, path) in wants {
                let val = lookup(sns, path).and_then(Value::as_f64).ok_or_else(|| HalError::BadReply(format!("{host}: no number at {path}")))?;
                snap.readings.insert(q, val);
            }
        }
        let mut kwh = 0.0;
        let mut metered = false;
        for (sw, plug) in &self.cfg.plugs {
            let v = self.cm(&plug.host, &format!("Power{}", plug.relay))?;
            let key = if plug.relay == 1 && v.get("POWER").is_some() { "POWER".to_owned() } else { format!("POWER{}", plug.relay) };
            let on = v.get(&key).and_then(Value::as_str).is_some_and(|s| s.eq_ignore_ascii_case("ON"));
            self.on.insert(*sw, on);
            snap.on.insert(*sw, on);
            if plug.meters {
                let e = self.cm(&plug.host, "Status 8")?;
                if let Some(total) = lookup(&e, "StatusSNS.ENERGY.Total").and_then(Value::as_f64) {
                    kwh += total;
                    metered = true;
                }
            }
        }
        if metered {
            snap.energy_kwh = Some(kwh);
        }
        Ok(snap)
    }

    fn set(&mut self, cmd: Command) -> Result<(), HalError> {
        let plug = self.cfg.plugs.get(&cmd.switch).ok_or(HalError::NotWired(cmd.switch))?;
        let cmnd = if cmd.on {
            format!("Backlog PulseTime{r} {}; Power{r} ON", Self::pulse_time(cmd.for_secs), r = plug.relay)
        } else {
            format!("Power{} OFF", plug.relay)
        };
        self.cm(&plug.host, &cmnd)?;
        self.on.insert(cmd.switch, cmd.on);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A fake Tasmota device: answers by command prefix and records every command it saw.
    fn fake_device(answers: Vec<(&'static str, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap().to_string();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let url = req.url().to_owned();
                let cmnd = url.split("cmnd=").nth(1).unwrap_or("").split('&').next().unwrap_or("").replace("%20", " ").replace("%3B", ";");
                seen2.lock().unwrap().push(cmnd.clone());
                let body =
                    answers.iter().find(|(p, _)| cmnd.starts_with(p)).map(|(_, b)| b.clone()).unwrap_or_else(|| r#"{"Command":"Unknown"}"#.into());
                let _ = req.respond(tiny_http::Response::from_string(body));
            }
        });
        (addr, seen)
    }

    #[test]
    fn pulse_time_encoding() {
        assert_eq!(TasmotaRig::pulse_time(0), 1);
        assert_eq!(TasmotaRig::pulse_time(5), 50);
        assert_eq!(TasmotaRig::pulse_time(11), 110);
        assert_eq!(TasmotaRig::pulse_time(12), 112);
        assert_eq!(TasmotaRig::pulse_time(600), 700);
        assert_eq!(TasmotaRig::pulse_time(99_999), 64_900);
    }

    #[test]
    fn reads_sensors_relays_and_energy_then_drives_with_pulse_time() {
        let (addr, seen) = fake_device(vec![
            ("Status 10", r#"{"StatusSNS":{"Time":"x","SHT3X":{"Temperature":23.4,"Humidity":41.0},"DS18B20":{"Temperature":19.8}}}"#.into()),
            ("Status 8", r#"{"StatusSNS":{"ENERGY":{"Total":1.234,"Power":118}}}"#.into()),
            ("Power1", r#"{"POWER":"ON"}"#.into()),
            ("Power2", r#"{"POWER2":"OFF"}"#.into()),
            ("Backlog", r#"{"PulseTime1":{"Set":700,"Remaining":0},"POWER":"ON"}"#.into()),
        ]);
        let mut plugs = BTreeMap::new();
        plugs.insert(Switch::Light, Plug { host: addr.clone(), relay: 1, meters: true });
        plugs.insert(Switch::Heater, Plug { host: addr.clone(), relay: 2, meters: false });
        let mut sensors = BTreeMap::new();
        sensors.insert(Quantity::AirTempC, Sensor { host: addr.clone(), path: "SHT3X.Temperature".into() });
        sensors.insert(Quantity::HumidityPct, Sensor { host: addr.clone(), path: "SHT3X.Humidity".into() });
        sensors.insert(Quantity::WaterTempC, Sensor { host: addr.clone(), path: "DS18B20.Temperature".into() });
        let mut rig = TasmotaRig::new(TasmotaConfig { user: "admin".into(), password: "p w".into(), timeout_secs: 5, plugs, sensors });

        let snap = rig.read().unwrap();
        assert_eq!(snap.get(Quantity::AirTempC), Some(23.4));
        assert_eq!(snap.get(Quantity::WaterTempC), Some(19.8));
        assert!(snap.is_on(Switch::Light));
        assert!(!snap.is_on(Switch::Heater));
        assert_eq!(snap.energy_kwh, Some(1.234));
        assert_eq!(rig.switches(), vec![Switch::Light, Switch::Heater]);

        rig.set(Command { switch: Switch::Light, on: true, for_secs: 600 }).unwrap();
        rig.set(Command { switch: Switch::Heater, on: false, for_secs: 0 }).unwrap();
        assert!(matches!(rig.set(Command { switch: Switch::Fan, on: true, for_secs: 1 }), Err(HalError::NotWired(Switch::Fan))));

        let seen = seen.lock().unwrap();
        assert_eq!(seen.iter().filter(|c| c.starts_with("Status 10")).count(), 1, "one sensor node asked once");
        assert!(seen.contains(&"Backlog PulseTime1 700; Power1 ON".to_owned()), "{seen:?}");
        assert!(seen.contains(&"Power2 OFF".to_owned()));
    }

    #[test]
    fn missing_sensor_path_is_a_bad_reply_and_dead_host_is_transport() {
        let (addr, _) = fake_device(vec![("Status 10", r#"{"StatusSNS":{"SHT3X":{"Temperature":1.0}}}"#.into())]);
        let mut sensors = BTreeMap::new();
        sensors.insert(Quantity::Co2Ppm, Sensor { host: addr, path: "SCD40.CarbonDioxide".into() });
        let mut rig =
            TasmotaRig::new(TasmotaConfig { user: String::new(), password: String::new(), timeout_secs: 2, plugs: BTreeMap::new(), sensors });
        assert!(matches!(rig.read(), Err(HalError::BadReply(_))));

        let mut plugs = BTreeMap::new();
        plugs.insert(Switch::Fan, Plug { host: "127.0.0.1:9".into(), relay: 1, meters: false });
        let mut rig =
            TasmotaRig::new(TasmotaConfig { user: String::new(), password: String::new(), timeout_secs: 1, plugs, sensors: BTreeMap::new() });
        assert!(matches!(rig.read(), Err(HalError::Transport(_))));
    }

    #[test]
    fn config_parses_from_toml() {
        let text = r#"
            user = "admin"
            password = "secret"
            [plugs.light]
            host = "192.168.0.51"
            meters = true
            [plugs.heater]
            host = "192.168.0.52"
            relay = 2
            [sensors.air_temp_c]
            host = "192.168.0.60"
            path = "SHT3X.Temperature"
        "#;
        let cfg: TasmotaConfig = toml::from_str(text).unwrap();
        assert_eq!(cfg.plugs[&Switch::Light].relay, 1);
        assert!(cfg.plugs[&Switch::Light].meters);
        assert_eq!(cfg.plugs[&Switch::Heater].relay, 2);
        assert_eq!(cfg.sensors[&Quantity::AirTempC].path, "SHT3X.Temperature");
        assert_eq!(cfg.timeout_secs, 5);
    }
}
