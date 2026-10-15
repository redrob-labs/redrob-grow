//! Talks to `grow-node`s: polls `GET /state` on a background thread per node, posts operator
//! answers, and listens for the nodes' UDP hellos so nobody has to type an address.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, UdpSocket};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use grow_agent::Report;
use serde::Deserialize;

/// The subset of the node's `/state` the dashboard shows.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeState {
    pub node: String,
    pub rig: String,
    #[serde(default)]
    pub planner: String,
    #[serde(default)]
    pub planner_status: String,
    pub cycles: usize,
    pub latest: Option<Report>,
    #[serde(default)]
    pub summary: Option<grow_run::Summary>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub questions: Vec<Question>,
    #[serde(default)]
    pub history: Vec<Report>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Note {
    pub cycle: usize,
    pub day: u32,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Question {
    pub day: u32,
    pub text: String,
    pub answer: Option<String>,
}

pub enum Event {
    State(Box<NodeState>),
    Error(String),
    Answered(Result<(), String>),
}

pub struct Remote {
    pub url: String,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    pub last: Option<NodeState>,
    pub last_error: Option<String>,
    /// What the operator is typing for this node.
    pub draft: String,
    pub answer_result: Option<Result<(), String>>,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into()
}

impl Remote {
    /// Start polling `url` (e.g. `http://127.0.0.1:7474`) every `every`.
    pub fn start(url: String, every: Duration) -> Self {
        let (tx, rx) = channel();
        let url = url.trim().trim_end_matches('/').to_owned();
        let url = if url.starts_with("http") { url } else { format!("http://{url}") };
        let (u2, t2) = (url.clone(), tx.clone());
        thread::spawn(move || poll_loop(&u2, every, &t2));
        Self { url, rx, tx, last: None, last_error: None, draft: String::new(), answer_result: None }
    }

    /// Drain events. Returns true if something changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            changed = true;
            match ev {
                Event::State(s) => {
                    self.last_error = None;
                    self.last = Some(*s);
                }
                Event::Error(e) => self.last_error = Some(e),
                Event::Answered(r) => self.answer_result = Some(r),
            }
        }
        changed
    }

    /// Send the draft as an operator message. The node answers every open question with it.
    pub fn send_answer(&mut self) {
        let text = std::mem::take(&mut self.draft).trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.answer_result = None;
        let (url, tx) = (self.url.clone(), self.tx.clone());
        thread::spawn(move || {
            let body = serde_json::json!({ "text": text });
            let r = agent(Duration::from_secs(5))
                .post(&format!("{url}/answer"))
                .send_json(&body)
                .map_err(|e| e.to_string())
                .and_then(|resp| if resp.status().as_u16() == 200 { Ok(()) } else { Err(format!("node answered {}", resp.status())) });
            let _ = tx.send(Event::Answered(r));
        });
    }
}

fn poll_loop(url: &str, every: Duration, tx: &Sender<Event>) {
    let agent = agent(Duration::from_secs(5));
    loop {
        let ev = match agent.get(&format!("{url}/state")).call() {
            Ok(mut resp) => match resp.body_mut().read_json::<NodeState>() {
                Ok(s) => Event::State(Box::new(s)),
                Err(e) => Event::Error(format!("bad /state: {e}")),
            },
            Err(e) => Event::Error(e.to_string()),
        };
        if tx.send(ev).is_err() {
            return;
        }
        thread::sleep(every);
    }
}

/// A node heard on the LAN.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Found {
    pub name: String,
    /// The node's own `listen`; loopback means it needs a tunnel from another machine.
    pub listen: String,
    /// Where the hello came from, so a loopback `listen` can be shown beside the real host.
    #[serde(default)]
    pub from: String,
}

/// Listens on the announce port for `window` and returns every distinct node heard.
pub fn discover(window: Duration) -> Result<Vec<Found>, String> {
    discover_on(7475, window)
}

pub fn discover_on(port: u16, window: Duration) -> Result<Vec<Found>, String> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).map_err(|e| format!("cannot listen on udp {port}: {e}"))?;
    sock.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
    let mut found: BTreeMap<String, Found> = BTreeMap::new();
    let mut buf = [0u8; 1024];
    let until = Instant::now() + window;
    while Instant::now() < until {
        if let Ok((n, from)) = sock.recv_from(&mut buf)
            && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&buf[..n])
            && v["grow"] == 1
            && let (Some(name), Some(listen)) = (v["name"].as_str(), v["listen"].as_str())
        {
            found.insert(format!("{name}@{listen}"), Found { name: name.into(), listen: listen.into(), from: from.ip().to_string() });
        }
    }
    Ok(found.into_values().collect())
}

/// Runs [`discover`] on a thread and hands the result back through `pump`.
pub struct Discovery {
    rx: Receiver<Result<Vec<Found>, String>>,
    pub result: Option<Result<Vec<Found>, String>>,
    pub running: bool,
}

impl Discovery {
    pub fn start(window: Duration) -> Self {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let _ = tx.send(discover(window));
        });
        Self { rx, result: None, running: true }
    }

    pub fn pump(&mut self) {
        if let Ok(r) = self.rx.try_recv() {
            self.result = Some(r);
            self.running = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_hears_a_hello_and_ignores_noise() {
        let port = {
            let probe = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            probe.local_addr().unwrap().port()
        };
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let t = thread::spawn(move || {
            for _ in 0..5 {
                thread::sleep(Duration::from_millis(60));
                let _ = sender.send_to(br#"{"grow":1,"name":"shelf-1","listen":"127.0.0.1:7474"}"#, (Ipv4Addr::LOCALHOST, port));
                let _ = sender.send_to(br#"{"grow":1,"name":"shelf-1","listen":"127.0.0.1:7474"}"#, (Ipv4Addr::LOCALHOST, port));
                let _ = sender.send_to(b"not json", (Ipv4Addr::LOCALHOST, port));
                let _ = sender.send_to(br#"{"other":true}"#, (Ipv4Addr::LOCALHOST, port));
            }
        });
        let found = discover_on(port, Duration::from_millis(600)).unwrap();
        t.join().unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "shelf-1");
        assert_eq!(found[0].listen, "127.0.0.1:7474");
        assert_eq!(found[0].from, "127.0.0.1");
    }

    #[test]
    fn node_state_tolerates_an_old_node_without_planner_fields() {
        let v = serde_json::json!({ "node": "a", "rig": "office-sim", "cycles": 0, "latest": null });
        let s: NodeState = serde_json::from_value(v).unwrap();
        assert!(s.planner.is_empty() && s.notes.is_empty() && s.questions.is_empty());
    }
}
