//! What the node remembers in memory and what the local API serves.
//!
//! ```text
//! GET  /health            {"ok":true}
//! GET  /state             everything below, as JSON
//! POST /answer  {"text"}  a message from the operator: answers the open questions, reaches the planner next cycle
//! ```

use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, Mutex};

use grow_agent::Report;
use grow_run::{RunMeta, Summary};
use serde::{Deserialize, Serialize};

pub type Shared = Arc<Mutex<State>>;

/// Keep the last two days at ten-minute cycles.
const HISTORY: usize = 288;
/// Notes kept for the dashboard.
const NOTES: usize = 50;

/// Something the planner wanted the operator to read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub cycle: usize,
    pub day: u32,
    pub text: String,
}

/// A question the planner asked. Open until the operator answers anything.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub cycle: usize,
    pub day: u32,
    pub text: String,
    pub answer: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct State {
    pub node: String,
    pub recipe: String,
    pub rig: String,
    pub planner: String,
    pub planner_status: String,
    pub cycles: usize,
    pub latest: Option<Report>,
    pub summary: Option<Summary>,
    pub last_error: Option<String>,
    /// Newest last.
    pub notes: VecDeque<Note>,
    /// Newest last; open ones have `answer: None`.
    pub questions: Vec<Question>,
    /// Operator messages not yet handed to the planner.
    #[serde(skip)]
    pub inbox: Vec<String>,
    /// Oldest first.
    pub history: VecDeque<Report>,
}

impl State {
    pub fn new(node: &str, recipe: &str, rig: &str, planner: &str) -> Self {
        Self {
            node: node.into(),
            recipe: recipe.into(),
            rig: rig.into(),
            planner: planner.into(),
            planner_status: String::new(),
            cycles: 0,
            latest: None,
            summary: None,
            last_error: None,
            notes: VecDeque::with_capacity(NOTES),
            questions: Vec::new(),
            inbox: Vec::new(),
            history: VecDeque::with_capacity(HISTORY),
        }
    }

    pub fn push(&mut self, report: Report, period_secs: u32) {
        self.cycles += 1;
        self.last_error = None;
        let day = report.observation.clock.day;
        for text in &report.proposal.notes {
            if self.notes.len() == NOTES {
                self.notes.pop_front();
            }
            self.notes.push_back(Note { cycle: self.cycles, day, text: text.clone() });
        }
        for text in &report.proposal.questions {
            self.questions.push(Question { cycle: self.cycles, day, text: text.clone(), answer: None });
        }
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(report.clone());
        let meta = RunMeta {
            id: self.node.clone(),
            recipe_id: self.recipe.clone(),
            rig: self.rig.clone(),
            sown_on: String::new(),
            operator: String::new(),
            notes: String::new(),
        };
        let reports: Vec<Report> = self.history.iter().cloned().collect();
        self.summary = Some(Summary::from_reports(&meta, &reports, period_secs, None));
        self.latest = Some(report);
    }

    /// The operator said something: every open question takes it as its answer, and the planner
    /// hears it on the next cycle.
    pub fn answer(&mut self, text: &str) -> usize {
        let text = text.trim();
        if text.is_empty() {
            return 0;
        }
        let mut closed = 0;
        for q in self.questions.iter_mut().filter(|q| q.answer.is_none()) {
            q.answer = Some(text.to_owned());
            closed += 1;
        }
        self.inbox.push(text.to_owned());
        closed
    }

    /// Open questions, for the log line and tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn open_questions(&self) -> usize {
        self.questions.iter().filter(|q| q.answer.is_none()).count()
    }
}

#[derive(Deserialize)]
struct Answer {
    text: String,
}

/// Blocking HTTP server. Loopback by default; see `NodeConfig::listen`.
pub fn serve(listen: &str, state: Shared) {
    let server = match tiny_http::Server::http(listen) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot listen on {listen}: {e}");
            return;
        }
    };
    for mut req in server.incoming_requests() {
        let path = req.url().split('?').next().unwrap_or("/").to_owned();
        let json_header = tiny_http::Header::from_bytes("Content-Type", "application/json").expect("static header");
        let resp = match (req.method(), path.as_str()) {
            (tiny_http::Method::Get, "/health") => tiny_http::Response::from_string(r#"{"ok":true}"#).with_header(json_header),
            (tiny_http::Method::Get, "/state") => {
                let body = state
                    .lock()
                    .map(|s| serde_json::to_string(&*s).unwrap_or_else(|e| format!(r#"{{"error":"{e}"}}"#)))
                    .unwrap_or_else(|_| r#"{"error":"poisoned"}"#.into());
                tiny_http::Response::from_string(body).with_header(json_header)
            }
            (tiny_http::Method::Post, "/answer") => {
                let mut body = String::new();
                let _ = req.as_reader().take(16_384).read_to_string(&mut body);
                match serde_json::from_str::<Answer>(&body) {
                    Ok(a) => {
                        let closed = state.lock().map(|mut s| s.answer(&a.text)).unwrap_or(0);
                        tiny_http::Response::from_string(format!(r#"{{"ok":true,"closed":{closed}}}"#)).with_header(json_header)
                    }
                    Err(e) => tiny_http::Response::from_string(format!(r#"{{"error":"{e}"}}"#)).with_status_code(400).with_header(json_header),
                }
            }
            _ => tiny_http::Response::from_string(r#"{"error":"not found"}"#).with_status_code(404).with_header(json_header),
        };
        let _ = req.respond(resp);
    }
}

#[cfg(test)]
mod tests {
    use grow_agent::{Clock, Cycle, RulePlanner};
    use grow_hal::mock::{Office, OfficeRig};
    use grow_recipe::builtin;
    use grow_safety::{Guard, Limits};

    use super::*;

    #[test]
    fn state_serves_latest_summary_and_takes_answers() {
        let shared: Shared = Arc::new(Mutex::new(State::new("t", "radish-sprout", "office-sim", "rules")));
        let mut cycle = Cycle::new(
            OfficeRig::new(Office::summer()),
            RulePlanner::default(),
            Guard::new(Limits::office_shelf()),
            builtin::by_id("radish-sprout").unwrap(),
        );
        for i in 0..3u64 {
            let mut r = cycle.run(Clock { now_secs: i * 600, hour: 10.0, weekday: true, day: 2 }).unwrap();
            if i == 1 {
                r.proposal.notes.push("humidity trending down".into());
                r.proposal.questions.push("tray dry?".into());
            }
            shared.lock().unwrap().push(r, 600);
        }
        let listen = {
            let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            probe.local_addr().unwrap().to_string()
        };
        let s2 = shared.clone();
        let l2 = listen.clone();
        std::thread::spawn(move || serve(&l2, s2));
        std::thread::sleep(std::time::Duration::from_millis(150));

        let body = http(&listen, "GET", "/state", "");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["cycles"], 3);
        assert_eq!(v["summary"]["cycles"], 3);
        assert_eq!(v["planner"], "rules");
        assert_eq!(v["latest"]["observation"]["stage"]["name"], "greening");
        assert_eq!(v["notes"][0]["text"], "humidity trending down");
        assert_eq!(v["notes"][0]["cycle"], 2);
        assert_eq!(v["questions"][0]["answer"], serde_json::Value::Null);
        assert!(http(&listen, "GET", "/nope", "").contains("not found"));

        let body = http(&listen, "POST", "/answer", r#"{"text":"yes, I watered it"}"#);
        assert!(body.contains(r#""closed":1"#), "{body}");
        assert!(http(&listen, "POST", "/answer", "nonsense").contains("error"));
        let s = shared.lock().unwrap();
        assert_eq!(s.open_questions(), 0);
        assert_eq!(s.inbox, vec!["yes, I watered it"]);
        assert_eq!(s.questions[0].answer.as_deref(), Some("yes, I watered it"));
    }

    /// Minimal HTTP/1.0 with std only, so the node has no HTTP client dependency.
    fn http(host: &str, method: &str, path: &str, body: &str) -> String {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(host).unwrap();
        write!(s, "{method} {path} HTTP/1.0\r\nHost: {host}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out.split("\r\n\r\n").nth(1).unwrap_or("").to_owned()
    }
}
