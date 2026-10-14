//! `grow-node`: the program that runs on the shelf.
//!
//! ```text
//! grow-node init [node.toml]     write an example config (simulated office, lettuce)
//! grow-node run  [node.toml]     run the loop and serve GET /state on node.listen
//! grow-node recipes              list built-in recipes
//! ```
//!
//! The HTTP API is loopback-only unless `listen` says otherwise, and has no authentication: it is
//! meant for the dashboard on the same machine or over an SSH tunnel. Do not expose it.

mod config;
mod state;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grow_agent::{Clock, Cycle, RulePlanner};
use grow_hal::Rig;
use grow_hal::mock::{Office, OfficeRig};
use grow_hal::mycodo::MycodoRig;
use grow_recipe::{Recipe, builtin};
use grow_run::{Run, RunMeta};
use grow_safety::{Guard, Limits};

use config::{NodeConfig, RigConfig};
use state::{Shared, serve};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("init") => cmd_init(args.get(1).map(PathBuf::from).unwrap_or_else(|| "node.toml".into())),
        Some("run") => cmd_run(args.get(1).map(PathBuf::from).unwrap_or_else(|| "node.toml".into())),
        Some("recipes") => {
            for r in builtin::all() {
                println!("{:<22} {:<10} {:>4} days  {} stages", r.id, r.crop, r.days, r.stages.len());
            }
            0
        }
        _ => {
            eprintln!("usage: grow-node init [node.toml] | run [node.toml] | recipes");
            2
        }
    };
    std::process::exit(code);
}

fn cmd_init(path: PathBuf) -> i32 {
    if path.exists() {
        eprintln!("{} exists, not overwriting", path.display());
        return 1;
    }
    let text = toml::to_string_pretty(&NodeConfig::example()).expect("example serialises");
    match std::fs::write(&path, text) {
        Ok(()) => {
            println!("wrote {}", path.display());
            0
        }
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            1
        }
    }
}

fn load_recipe(spec: &str) -> Result<Recipe, String> {
    if let Some(r) = builtin::by_id(spec) {
        return Ok(r);
    }
    let text = std::fs::read_to_string(spec).map_err(|e| format!("recipe `{spec}`: {e}"))?;
    Recipe::from_toml(&text).map_err(|e| format!("recipe `{spec}`: {e}"))
}

fn cmd_run(path: PathBuf) -> i32 {
    let cfg: NodeConfig = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| toml::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return 1;
        }
    };
    let recipe = match load_recipe(&cfg.node.recipe) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let Some(sown_day) = parse_civil(&cfg.node.sown_on) else {
        eprintln!("sown_on `{}` is not YYYY-MM-DD", cfg.node.sown_on);
        return 1;
    };

    let (rig, speed): (Box<dyn Rig>, u32) = match &cfg.rig {
        RigConfig::Sim { season, speed } => {
            let office = if season == "winter" { Office::winter() } else { Office::summer() };
            (Box::new(OfficeRig::new(office)), *speed)
        }
        RigConfig::Mycodo(m) => (Box::new(MycodoRig::new(m.clone())), 1),
    };

    let shared: Shared = Arc::new(Mutex::new(state::State::new(&cfg.node.name, &recipe.id, rig.name())));
    let listen = cfg.node.listen.clone();
    let server_state = shared.clone();
    std::thread::spawn(move || serve(&listen, server_state));
    println!("grow-node `{}` recipe {} rig {} listening on http://{}", cfg.node.name, recipe.id, rig.name(), cfg.node.listen);

    let run = match open_or_create_run(&cfg, &recipe, rig.name()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runs dir: {e}");
            return 1;
        }
    };

    let mut cycle = Cycle::new(rig, RulePlanner::default(), Guard::new(Limits::office_shelf()), recipe);
    let period = cfg.node.period_secs;
    loop {
        let clock = match cycle.rig.sim_time() {
            Some(sim) => Clock { now_secs: sim, hour: (sim % 86_400) as f64 / 3600.0, weekday: (sim / 86_400) % 7 < 5, day: (sim / 86_400) as u32 },
            None => wall_clock(cfg.node.tz_offset_hours, sown_day),
        };
        match cycle.run(clock) {
            Ok(report) => {
                if let Err(e) = run.append(&report) {
                    eprintln!("run log: {e}");
                }
                if let Ok(mut s) = shared.lock() {
                    s.push(report, period);
                }
            }
            Err(e) => {
                eprintln!("cycle failed: {e}");
                if let Ok(mut s) = shared.lock() {
                    s.last_error = Some(e.to_string());
                }
            }
        }
        if speed > 1 {
            cycle.rig.tick(period);
            std::thread::sleep(Duration::from_millis(u64::from(period) * 1000 / u64::from(speed)));
        } else {
            std::thread::sleep(Duration::from_secs(u64::from(period)));
        }
    }
}

fn open_or_create_run(cfg: &NodeConfig, recipe: &Recipe, rig: &str) -> Result<Run, grow_run::RunError> {
    std::fs::create_dir_all(&cfg.node.runs_dir)?;
    let id = format!("{}-{}-{}", cfg.node.sown_on, recipe.id, cfg.node.name);
    let dir = cfg.node.runs_dir.join(&id);
    if dir.exists() {
        return Run::open(&dir);
    }
    Run::create(
        &cfg.node.runs_dir,
        RunMeta {
            id,
            recipe_id: recipe.id.clone(),
            rig: rig.to_owned(),
            sown_on: cfg.node.sown_on.clone(),
            operator: String::new(),
            notes: String::new(),
        },
    )
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm). `None` if not `YYYY-MM-DD`.
fn parse_civil(s: &str) -> Option<i64> {
    let mut it = s.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn wall_clock(tz_offset_hours: f64, sown_day: i64) -> Clock {
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let local = unix + (tz_offset_hours * 3600.0) as i64;
    let day_index = local.div_euclid(86_400);
    let secs_today = local.rem_euclid(86_400);
    // 1970-01-01 was a Thursday: weekday index 0 = Monday is (day_index + 3) % 7.
    let weekday = (day_index + 3).rem_euclid(7) < 5;
    Clock { now_secs: unix as u64, hour: secs_today as f64 / 3600.0, weekday, day: (day_index - sown_day).max(0) as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(parse_civil("1970-01-01"), Some(0));
        assert_eq!(parse_civil("2000-03-01"), Some(11_017));
        assert_eq!(parse_civil("2026-10-20"), Some(20_746));
        assert_eq!(parse_civil("2026-13-01"), None);
        assert_eq!(parse_civil("yesterday"), None);
    }

    #[test]
    fn builtin_recipe_resolves_and_missing_file_is_an_error() {
        assert!(load_recipe("rice-sindongjin").is_ok());
        assert!(load_recipe("/definitely/not/here.toml").unwrap_err().contains("not/here"));
    }
}
