//! Polls a remote `grow-node` for `GET /state` on a background thread.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use grow_agent::Report;
use serde::Deserialize;

/// The subset of the node's `/state` the dashboard shows.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeState {
    pub node: String,
    pub rig: String,
    pub cycles: usize,
    pub latest: Option<Report>,
    #[serde(default)]
    pub history: Vec<Report>,
}

pub enum Event {
    State(Box<NodeState>),
    Error(String),
}

pub struct Remote {
    rx: Receiver<Event>,
    pub last: Option<NodeState>,
    pub last_error: Option<String>,
}

impl Remote {
    /// Start polling `url` (e.g. `http://127.0.0.1:7474`) every `every`.
    pub fn start(url: String, every: Duration) -> Self {
        let (tx, rx) = channel();
        thread::spawn(move || poll_loop(&url, every, &tx));
        Self { rx, last: None, last_error: None }
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
            }
        }
        changed
    }
}

fn poll_loop(url: &str, every: Duration, tx: &Sender<Event>) {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
    loop {
        let ev = match agent.get(&format!("{}/state", url.trim_end_matches('/'))).call() {
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
