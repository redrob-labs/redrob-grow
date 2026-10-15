//! A grow is a folder.
//!
//! ```text
//! runs/2026-10-20-lettuce-butterhead-01/
//!   run.json        who, what, when, which rig, which recipe (frozen copy)
//!   cycles.jsonl    one `grow_agent::Report` per line, append-only
//!   photos/         `day-012-0900.jpg` from the shelf camera
//!   harvest.json    weight, count, notes, written by a human at the end
//! ```
//!
//! This is the data the repository exists to collect. Everything here is plain files so a run
//! can be read with `jq` and committed to git.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use grow_agent::Report;
use grow_hal::Switch;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMeta {
    pub id: String,
    pub recipe_id: String,
    pub rig: String,
    /// ISO-8601 local date of sowing, e.g. `2026-10-20`.
    pub sown_on: String,
    #[serde(default)]
    pub operator: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Harvest {
    pub harvested_on: String,
    pub fresh_weight_g: f64,
    pub plant_count: u32,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct Run {
    pub dir: PathBuf,
    pub meta: RunMeta,
}

impl Run {
    /// Create the folder and write `run.json`. Fails if the folder exists.
    pub fn create(parent: &Path, meta: RunMeta) -> Result<Self, RunError> {
        let dir = parent.join(&meta.id);
        fs::create_dir(&dir)?;
        fs::create_dir(dir.join("photos"))?;
        fs::write(dir.join("run.json"), serde_json::to_vec_pretty(&meta)?)?;
        Ok(Self { dir, meta })
    }

    pub fn open(dir: &Path) -> Result<Self, RunError> {
        let meta: RunMeta = serde_json::from_slice(&fs::read(dir.join("run.json"))?)?;
        Ok(Self { dir: dir.to_path_buf(), meta })
    }

    pub fn append(&self, report: &Report) -> Result<(), RunError> {
        let mut f = OpenOptions::new().create(true).append(true).open(self.dir.join("cycles.jsonl"))?;
        serde_json::to_writer(&mut f, report)?;
        f.write_all(b"\n")?;
        Ok(())
    }

    /// What the operator told the planner, one JSON line per message, in `operator.jsonl`.
    pub fn append_operator(&self, day: u32, text: &str) -> Result<(), RunError> {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.dir.join("operator.jsonl"))?;
        let line = serde_json::to_string(&serde_json::json!({ "day": day, "text": text }))?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    pub fn cycles(&self) -> Result<Vec<Report>, RunError> {
        let path = self.dir.join("cycles.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for line in BufReader::new(File::open(path)?).lines() {
            let line = line?;
            if !line.trim().is_empty() {
                out.push(serde_json::from_str(&line)?);
            }
        }
        Ok(out)
    }

    pub fn write_harvest(&self, h: &Harvest) -> Result<(), RunError> {
        fs::write(self.dir.join("harvest.json"), serde_json::to_vec_pretty(h)?)?;
        Ok(())
    }

    pub fn harvest(&self) -> Result<Option<Harvest>, RunError> {
        let p = self.dir.join("harvest.json");
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_slice(&fs::read(p)?)?))
    }

    /// The one-page summary a run is judged by.
    pub fn summary(&self, period_secs: u32) -> Result<Summary, RunError> {
        let cycles = self.cycles()?;
        Ok(Summary::from_reports(&self.meta, &cycles, period_secs, self.harvest()?))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub run_id: String,
    pub recipe_id: String,
    pub cycles: usize,
    pub days: u32,
    /// Share of cycles with every stage target inside its band.
    pub in_band_ratio: f64,
    pub alarms: usize,
    pub rejected: usize,
    /// Estimated from nominal watts and on-time.
    pub kwh_estimate: f64,
    /// Measured by the rig's meter (last minus first cumulative reading), when it has one.
    #[serde(default)]
    pub kwh_measured: Option<f64>,
    pub harvest: Option<Harvest>,
    /// kWh per gram of harvest, the number that decides whether indoor rice makes sense.
    pub kwh_per_g: Option<f64>,
}

impl Summary {
    pub fn from_reports(meta: &RunMeta, reports: &[Report], period_secs: u32, harvest: Option<Harvest>) -> Self {
        let mut in_band = 0usize;
        let mut alarms = 0usize;
        let mut rejected = 0usize;
        let mut joules = 0.0;
        let mut days = 0;
        for r in reports {
            let snap = &r.observation.snapshot;
            let all_ok = r.observation.stage.targets.iter().all(|(q, b)| snap.get(*q).is_none_or(|v| b.contains(v)));
            if all_ok {
                in_band += 1;
            }
            alarms += r.verdict.alarms.len();
            rejected += r.verdict.rejected.len();
            for sw in Switch::ALL {
                if snap.is_on(sw) {
                    joules += sw.nominal_watts() * f64::from(period_secs);
                }
            }
            days = days.max(r.observation.clock.day + 1);
        }
        let kwh_estimate = joules / 3_600_000.0;
        let first = reports.iter().find_map(|r| r.observation.snapshot.energy_kwh);
        let last = reports.iter().rev().find_map(|r| r.observation.snapshot.energy_kwh);
        let kwh_measured = first.zip(last).map(|(a, b)| (b - a).max(0.0));
        // The measured number wins when there is one: it is the point of the meter.
        let kwh = kwh_measured.unwrap_or(kwh_estimate);
        let kwh_per_g = harvest.as_ref().filter(|h| h.fresh_weight_g > 0.0).map(|h| kwh / h.fresh_weight_g);
        Self {
            run_id: meta.id.clone(),
            recipe_id: meta.recipe_id.clone(),
            cycles: reports.len(),
            days,
            in_band_ratio: if reports.is_empty() { 0.0 } else { in_band as f64 / reports.len() as f64 },
            alarms,
            rejected,
            kwh_estimate,
            kwh_measured,
            harvest,
            kwh_per_g,
        }
    }
}

#[cfg(test)]
mod tests {
    use grow_agent::{Clock, Cycle, RulePlanner};
    use grow_hal::Rig;
    use grow_hal::mock::{Office, OfficeRig};
    use grow_recipe::builtin;
    use grow_safety::{Guard, Limits};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("KIROCREW_SCRATCH").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join("grow-run-tests").join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_run_is_a_folder_of_plain_files() {
        let parent = scratch("folder");
        let meta = RunMeta {
            id: "2026-10-20-radish-sprout-01".into(),
            recipe_id: "radish-sprout".into(),
            rig: "office-sim".into(),
            sown_on: "2026-10-20".into(),
            operator: "test".into(),
            notes: String::new(),
        };
        let run = Run::create(&parent, meta.clone()).unwrap();
        assert!(run.dir.join("photos").is_dir());
        assert!(Run::create(&parent, meta.clone()).is_err(), "no silent overwrite");

        let mut cycle = Cycle::new(
            OfficeRig::new(Office::summer()),
            RulePlanner::default(),
            Guard::new(Limits::office_shelf()),
            builtin::by_id("radish-sprout").unwrap(),
        );
        for i in 0..6u64 {
            let report = cycle.run(Clock { now_secs: i * 600, hour: 9.0 + i as f64 / 6.0, weekday: true, day: 3 }).unwrap();
            run.append(&report).unwrap();
            cycle.rig.tick(600);
        }
        let again = Run::open(&run.dir).unwrap();
        assert_eq!(again.meta, meta);
        assert_eq!(again.cycles().unwrap().len(), 6);

        let s = again.summary(600).unwrap();
        assert_eq!(s.cycles, 6);
        assert_eq!(s.days, 4);
        assert!(s.kwh_estimate > 0.0);
        assert!(s.harvest.is_none() && s.kwh_per_g.is_none());

        again.write_harvest(&Harvest { harvested_on: "2026-10-27".into(), fresh_weight_g: 80.0, plant_count: 200, notes: String::new() }).unwrap();
        let s = again.summary(600).unwrap();
        // The simulated rig meters, so the measured number is used and it tracks the estimate.
        let measured = s.kwh_measured.unwrap();
        assert!(measured > 0.0 && (measured - s.kwh_estimate).abs() / s.kwh_estimate < 0.5, "measured {measured} estimate {}", s.kwh_estimate);
        assert!((s.kwh_per_g.unwrap() - measured / 80.0).abs() < 1e-12);
        let _ = fs::remove_dir_all(parent);
    }

    #[test]
    fn empty_run_summarises_to_zero() {
        let meta =
            RunMeta { id: "x".into(), recipe_id: "y".into(), rig: "z".into(), sown_on: String::new(), operator: String::new(), notes: String::new() };
        let s = Summary::from_reports(&meta, &[], 600, None);
        assert_eq!(s.in_band_ratio, 0.0);
        assert_eq!(s.cycles, 0);
    }
}
