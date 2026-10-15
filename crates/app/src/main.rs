//! Redrob Grow desktop dashboard.
//!
//! Three tabs. **Shelf** shows one node: readings against the stage bands, which switches are on,
//! what the agent decided and why, and the guard's alarms. **Setup** is the one-pass wizard that
//! writes a `node.toml` for `grow-node`. **Recipes** lists the built-in recipes stage by stage.
//!
//! The shelf can be the built-in simulated office (no hardware, runs in-process, fast-forwarded)
//! or one or more remote `grow-node`s polled over HTTP. `redrob-grow --node http://host:7474
//! --node ...` opens on those nodes straight away.

mod remote;
mod sim;
mod theme;
mod ui;
mod wizard;

use eframe::egui;

fn main() -> eframe::Result {
    let nodes: Vec<String> = {
        let args: Vec<String> = std::env::args().skip(1).collect();
        args.windows(2).filter(|w| w[0] == "--node").map(|w| w[1].clone()).collect()
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 760.0]).with_min_inner_size([900.0, 600.0]).with_title("Redrob Grow"),
        ..Default::default()
    };
    eframe::run_native("Redrob Grow", options, Box::new(move |cc| Ok(Box::new(ui::App::new(cc, &nodes)))))
}
