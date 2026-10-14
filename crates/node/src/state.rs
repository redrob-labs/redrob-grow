//! What the node remembers in memory and what `GET /state` returns.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use grow_agent::Report;
use grow_run::{RunMeta, Summary};
use serde::Serialize;

pub type Shared = Arc<Mutex<State>>;

/// Keep the last two days at ten-minute cycles.
const HISTORY: usize = 288;

#[derive(Debug, Serialize)]
pub struct State {
    pub node: String,
    pub recipe: String,
    pub rig: String,
    pub cycles: usize,
    pub latest: Option<Report>,
    pub summary: Option<Summary>,
    pub last_error: Option<String>,
    /// Oldest first.
    pub history: VecDeque<Report>,
}

impl State {
    pub fn new(node: &str, recipe: &str, rig: &str) -> Self {
        Self {
            node: node.into(),
            recipe: recipe.into(),
            rig: rig.into(),
            cycles: 0,
            latest: None,
            summary: None,
            last_error: None,
            history: VecDeque::with_capacity(HISTORY),
        }
    }

    pub fn push(&mut self, report: Report, period_secs: u32) {
        self.cycles += 1;
        self.last_error = None;
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
    for req in server.incoming_requests() {
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
    fn state_serves_latest_and_summary() {
        let shared: Shared = Arc::new(Mutex::new(State::new("t", "radish-sprout", "office-sim")));
        let mut cycle = Cycle::new(
            OfficeRig::new(Office::summer()),
            RulePlanner::default(),
            Guard::new(Limits::office_shelf()),
            builtin::by_id("radish-sprout").unwrap(),
        );
        for i in 0..3u64 {
            let r = cycle.run(Clock { now_secs: i * 600, hour: 10.0, weekday: true, day: 2 }).unwrap();
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

        let body = ureq_get(&format!("http://{listen}/state"));
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["cycles"], 3);
        assert_eq!(v["summary"]["cycles"], 3);
        assert_eq!(v["latest"]["observation"]["stage"]["name"], "greening");
        assert!(ureq_get(&format!("http://{listen}/nope")).contains("not found"));
    }

    /// Minimal GET with std only, so the node has no HTTP client dependency.
    fn ureq_get(url: &str) -> String {
        use std::io::{Read, Write};
        let rest = url.trim_start_matches("http://");
        let (host, path) = rest.split_once('/').unwrap();
        let mut s = std::net::TcpStream::connect(host).unwrap();
        write!(s, "GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out.split("\r\n\r\n").nth(1).unwrap_or("").to_owned()
    }
}
