//! The model-backed planner.
//!
//! [`EnginePlanner`] sends one cycle's [`Observation`], the stage's targets and notes, the rule
//! planner's own proposal for the same cycle, the latest photo (if there is one) and whatever the
//! operator typed since the last cycle to an OpenAI-compatible chat endpoint, with the tool set in
//! [`crate::tools`]. The tool calls that come back become the [`Proposal`]: `switch_on` and
//! `switch_off` are commands for the guard, `note` and `ask_human` are text for the operator.
//!
//! The endpoint is normally the shared Redrob Code daemon (the node resolves its address and
//! credential), but any `/v1/chat/completions` works. When the call fails for any reason the rule
//! planner's proposal is used and the failure is recorded as a note, so a shelf is never left
//! without a decision because a model was slow.

use std::path::PathBuf;
use std::time::Duration;

use grow_hal::{Command, Switch};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tools::{self, ToolCall};
use crate::{Observation, Planner, Proposal, RulePlanner};

/// Where and how to reach the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    /// OpenAI-compatible base, ending in `/v1`.
    pub base_url: String,
    /// Extra headers on every call (authorization, app and session labels). Never logged.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    /// Model name the endpoint expects. `redrob/auto` lets the Redrob engine route through the Console.
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Directory of photos; the newest `*.jpg` is attached to the prompt when present.
    #[serde(default)]
    pub photo_dir: Option<PathBuf>,
}

fn default_model() -> String {
    "redrob/auto".into()
}
fn default_timeout() -> u64 {
    90
}

pub struct EnginePlanner {
    cfg: EngineConfig,
    agent: ureq::Agent,
    fallback: RulePlanner,
    /// Operator messages waiting to be shown to the model on the next cycle.
    pub inbox: Vec<String>,
    /// The last transport or parse failure, cleared on the next success.
    pub last_error: Option<String>,
    /// Cycles answered by the model and cycles that fell back to rules.
    pub model_cycles: u64,
    pub fallback_cycles: u64,
}

impl EnginePlanner {
    pub fn new(cfg: EngineConfig, fallback: RulePlanner) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(cfg.timeout_secs.max(5))))
            .http_status_as_error(false)
            .build()
            .into();
        Self { cfg, agent, fallback, inbox: Vec::new(), last_error: None, model_cycles: 0, fallback_cycles: 0 }
    }

    pub const SYSTEM: &str = "You run one hydroponic shelf in an office built for people, not plants. Every cycle you get \
        the sensor snapshot, which switches exist and are on, the recipe stage with its target bands and notes, a one-hour \
        air temperature forecast learned from this room, what the deterministic rule planner would do, and any message from \
        the operator. Decide with the tools only. Rules: (1) keep every reading inside its band with the fewest switch changes; \
        (2) the pump stays on while the crop is alive; (3) a switch_on needs a duration, the guard clips or refuses what is \
        unsafe and you will see that next cycle; (4) prefer the rule planner's proposal unless you can say in one line why \
        yours is better for this room (lamp heat, air conditioning schedule, weekend, a trend in the history); (5) use note for \
        anything the human should know about the crop or the recipe, and ask_human when a reading makes no physical sense or \
        the photo shows a sick plant; (6) never explain outside tool calls.";

    fn newest_photo(&self) -> Option<PathBuf> {
        let dir = self.cfg.photo_dir.as_ref()?;
        let mut jpgs: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg") || x.eq_ignore_ascii_case("jpeg")))
            .filter_map(|e| e.metadata().and_then(|m| m.modified()).ok().map(|t| (t, e.path())))
            .collect();
        jpgs.sort();
        jpgs.pop().map(|(_, p)| p)
    }

    /// The request body for one cycle. Public so a test can assert the shape without a server.
    pub fn request(&self, obs: &Observation, baseline: &Proposal) -> Value {
        let baseline_lines: Vec<String> = baseline
            .commands
            .iter()
            .zip(&baseline.reasons)
            .map(|(c, why)| format!("{} {:?} {}s -- {why}", if c.on { "on" } else { "off" }, c.switch, c.for_secs))
            .collect();
        let mut text = format!(
            "Observation (JSON):\n{}\n\nRule planner would do:\n{}\n",
            serde_json::to_string_pretty(obs).unwrap_or_default(),
            if baseline_lines.is_empty() { "nothing".to_owned() } else { baseline_lines.join("\n") }
        );
        if !self.inbox.is_empty() {
            text.push_str("\nOperator says:\n");
            for m in &self.inbox {
                text.push_str("- ");
                text.push_str(m);
                text.push('\n');
            }
        }
        let mut content = vec![json!({ "type": "text", "text": text })];
        if let Some(p) = self.newest_photo()
            && let Ok(bytes) = std::fs::read(&p)
        {
            content.push(json!({ "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{}", base64(&bytes)) } }));
        }
        let tools: Vec<Value> = tools::specs()
            .into_iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
            .collect();
        json!({
            "model": self.cfg.model,
            "temperature": 0.2,
            "messages": [
                { "role": "system", "content": Self::SYSTEM },
                { "role": "user", "content": content }
            ],
            "tools": tools,
            "tool_choice": "required"
        })
    }

    fn call(&self, body: &Value) -> Result<Vec<ToolCall>, String> {
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let mut req = self.agent.post(&url);
        for (k, v) in &self.cfg.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let mut resp = req.send_json(body).map_err(|e| format!("engine: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().map_err(|e| format!("engine body: {e}"))?;
        if status != 200 {
            return Err(format!("engine answered {status}: {}", text.chars().take(200).collect::<String>()));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| format!("engine json: {e}"))?;
        parse_tool_calls(&v)
    }

    /// Turn tool calls into a proposal. Unknown switches for this rig are dropped with a note.
    pub fn apply(calls: Vec<ToolCall>, obs: &Observation, baseline: &Proposal) -> Proposal {
        let mut p = Proposal { planner: "engine".into(), ..Default::default() };
        let wired = |s: Switch| obs.switches.contains(&s);
        for c in calls {
            match c {
                ToolCall::SwitchOn { switch, for_secs, reason } if wired(switch) => {
                    p.push(Command { switch, on: true, for_secs: for_secs.max(1) }, reason);
                }
                ToolCall::SwitchOff { switch, reason } if wired(switch) => {
                    p.push(Command { switch, on: false, for_secs: 0 }, reason);
                }
                ToolCall::SwitchOn { switch, .. } | ToolCall::SwitchOff { switch, .. } => {
                    p.notes.push(format!("model asked for {switch:?}, which this rig does not have"));
                }
                ToolCall::Note { text } => p.notes.push(text),
                ToolCall::AskHuman { question } => p.questions.push(question),
            }
        }
        // Rule 2 is not negotiable: if the model forgot the pump, keep the rule planner's pump command.
        if let Some(i) = baseline.commands.iter().position(|c| c.switch == Switch::Pump && c.on)
            && !p.commands.iter().any(|c| c.switch == Switch::Pump)
        {
            p.push(baseline.commands[i], "circulation (kept from rules)");
        }
        p
    }
}

/// `choices[0].message.tool_calls[*].function.{name,arguments}` to typed calls.
pub fn parse_tool_calls(v: &Value) -> Result<Vec<ToolCall>, String> {
    let calls = v["choices"][0]["message"]["tool_calls"].as_array().ok_or_else(|| "no tool_calls in reply".to_owned())?;
    let mut out = Vec::new();
    for c in calls {
        let name = c["function"]["name"].as_str().ok_or("tool call without name")?;
        let args_raw = &c["function"]["arguments"];
        let mut args: Value = match args_raw {
            Value::String(s) => serde_json::from_str(s).map_err(|e| format!("arguments of {name}: {e}"))?,
            other => other.clone(),
        };
        args["tool"] = json!(name);
        let call: ToolCall = serde_json::from_value(args).map_err(|e| format!("tool {name}: {e}"))?;
        out.push(call);
    }
    Ok(out)
}

impl Planner for EnginePlanner {
    fn name(&self) -> &str {
        "engine"
    }

    fn hear(&mut self, message: &str) {
        let m = message.trim();
        if !m.is_empty() {
            self.inbox.push(m.chars().take(1000).collect());
        }
    }

    fn status(&self) -> String {
        match &self.last_error {
            Some(e) => format!("model {} / rules {} -- last error: {e}", self.model_cycles, self.fallback_cycles),
            None => format!("model {} / rules {}", self.model_cycles, self.fallback_cycles),
        }
    }

    fn plan(&mut self, obs: &Observation) -> Proposal {
        let baseline = self.fallback.plan(obs);
        let body = self.request(obs, &baseline);
        match self.call(&body) {
            Ok(calls) => {
                self.last_error = None;
                self.model_cycles += 1;
                self.inbox.clear();
                Self::apply(calls, obs, &baseline)
            }
            Err(e) => {
                self.last_error = Some(e.clone());
                self.fallback_cycles += 1;
                let mut p = baseline;
                p.planner = "rules (engine unavailable)".into();
                p.notes.push(format!("engine unavailable, rules used: {e}"));
                p
            }
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use grow_hal::mock::{Office, OfficeRig};
    use grow_hal::{Quantity, Rig};
    use grow_recipe::builtin;
    use grow_safety::{Guard, Limits};

    use super::*;
    use crate::{Clock, Cycle};

    fn observation() -> Observation {
        let recipe = builtin::by_id("lettuce-butterhead").unwrap();
        let mut cycle = Cycle::new(OfficeRig::new(Office::winter()), RulePlanner::default(), Guard::new(Limits::office_shelf()), recipe);
        cycle.observe(Clock { now_secs: 0, hour: 10.0, weekday: true, day: 10 }).unwrap()
    }

    /// A fake engine: answers every POST with `reply`, records the bodies it saw.
    fn fake_engine(reply: String, status: u16) -> (String, Arc<Mutex<Vec<Value>>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut body);
                if let Ok(v) = serde_json::from_str::<Value>(&body) {
                    seen2.lock().unwrap().push(v);
                }
                let _ = req.respond(tiny_http::Response::from_string(reply.clone()).with_status_code(status));
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn reply_with(calls: &[(&str, Value)]) -> String {
        let tc: Vec<Value> = calls
            .iter()
            .map(|(name, args)| json!({ "id": "x", "type": "function", "function": { "name": name, "arguments": args.to_string() } }))
            .collect();
        json!({ "choices": [{ "message": { "role": "assistant", "tool_calls": tc } }] }).to_string()
    }

    #[test]
    fn request_carries_observation_tools_and_baseline() {
        let obs = observation();
        let cfg = EngineConfig { base_url: "http://x/v1".into(), headers: vec![], model: "redrob/auto".into(), timeout_secs: 5, photo_dir: None };
        let mut p = EnginePlanner::new(cfg, RulePlanner::default());
        p.inbox.push("leaves look pale".into());
        let baseline = RulePlanner::default().plan(&obs);
        let body = p.request(&obs, &baseline);
        assert_eq!(body["tools"].as_array().unwrap().len(), 4);
        assert_eq!(body["tool_choice"], "required");
        let text = body["messages"][1]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"recipe_id\": \"lettuce-butterhead\""));
        assert!(text.contains("Rule planner would do"));
        assert!(text.contains("Operator says"));
        assert!(text.contains("leaves look pale"));
    }

    #[test]
    fn tool_calls_become_commands_notes_and_questions() {
        let (base, seen) = fake_engine(
            reply_with(&[
                ("switch_on", json!({ "switch": "heater", "for_secs": 900, "reason": "water 1 C under band, room cooling" })),
                ("switch_off", json!({ "switch": "fan", "reason": "air inside band" })),
                ("note", json!({ "text": "EC drifting up slowly" })),
                ("ask_human", json!({ "question": "tips brown?" })),
            ]),
            200,
        );
        let cfg = EngineConfig {
            base_url: base,
            headers: vec![("x-redrob-app".into(), "grow".into())],
            model: "redrob/auto".into(),
            timeout_secs: 5,
            photo_dir: None,
        };
        let mut planner = EnginePlanner::new(cfg, RulePlanner::default());
        planner.inbox.push("hello".into());
        let obs = observation();
        let p = planner.plan(&obs);
        assert_eq!(p.planner, "engine");
        assert!(p.commands.iter().any(|c| c.switch == Switch::Heater && c.on && c.for_secs == 900));
        assert!(p.commands.iter().any(|c| c.switch == Switch::Fan && !c.on));
        // The pump the model forgot is kept from the rules.
        assert!(p.commands.iter().any(|c| c.switch == Switch::Pump && c.on));
        assert_eq!(p.notes, vec!["EC drifting up slowly"]);
        assert_eq!(p.questions, vec!["tips brown?"]);
        assert!(planner.inbox.is_empty(), "inbox is delivered once");
        assert_eq!(planner.model_cycles, 1);
        assert!(planner.last_error.is_none());
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0]["model"], "redrob/auto");
    }

    #[test]
    fn engine_failure_falls_back_to_rules_and_says_so() {
        let (base, _) = fake_engine(r#"{"error":"busy"}"#.into(), 503);
        let cfg = EngineConfig { base_url: base, headers: vec![], model: "redrob/auto".into(), timeout_secs: 5, photo_dir: None };
        let mut planner = EnginePlanner::new(cfg, RulePlanner::default());
        let obs = observation();
        let p = planner.plan(&obs);
        let rules = RulePlanner::default().plan(&obs);
        assert_eq!(p.commands, rules.commands);
        assert!(p.planner.starts_with("rules"));
        assert!(p.notes[0].contains("503"));
        assert_eq!(planner.fallback_cycles, 1);
        assert!(planner.last_error.is_some());
    }

    #[test]
    fn full_cycle_with_engine_goes_through_the_guard() {
        // The model asks for a 3-hour heater pulse; the guard clips it to the limit.
        let (base, _) = fake_engine(reply_with(&[("switch_on", json!({ "switch": "heater", "for_secs": 3600, "reason": "cold" }))]), 200);
        let cfg = EngineConfig { base_url: base, headers: vec![], model: "redrob/auto".into(), timeout_secs: 5, photo_dir: None };
        let planner = EnginePlanner::new(cfg, RulePlanner::default());
        let mut cycle =
            Cycle::new(OfficeRig::new(Office::winter()), planner, Guard::new(Limits::office_shelf()), builtin::by_id("lettuce-butterhead").unwrap());
        let r = cycle.run(Clock { now_secs: 0, hour: 10.0, weekday: true, day: 10 }).unwrap();
        assert!(r.verdict.rejected.iter().any(|x| matches!(x, grow_safety::Rejection::Clipped { to_secs: 1800, .. })));
        assert!(r.verdict.run.iter().any(|c| c.switch == Switch::Heater && c.for_secs == 1800));
        assert!(r.failed.is_empty());
        assert!(cycle.rig.read().unwrap().get(Quantity::WaterTempC).is_some());
    }

    #[test]
    fn unknown_switch_and_bad_arguments_are_handled() {
        let v: Value = serde_json::from_str(&reply_with(&[("switch_on", json!({ "switch": "laser", "for_secs": 1, "reason": "x" }))])).unwrap();
        assert!(parse_tool_calls(&v).unwrap_err().contains("switch_on"));
        let v: Value = serde_json::from_str(&reply_with(&[("note", json!({ "text": "fine" }))])).unwrap();
        assert_eq!(parse_tool_calls(&v).unwrap(), vec![ToolCall::Note { text: "fine".into() }]);
        let v: Value = json!({ "choices": [{ "message": { "content": "I think the heater should go on." } }] });
        assert!(parse_tool_calls(&v).unwrap_err().contains("no tool_calls"));
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
