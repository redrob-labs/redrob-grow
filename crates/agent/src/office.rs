//! Learns what the room does by hour of day, weekdays and weekends apart.
//!
//! Forty-eight bins (24 hours x weekday/weekend), each an exponential moving average of the air
//! temperature seen at that hour. After two or three days the model knows when the air conditioner
//! comes on and off, and the planner can act an hour early instead of chasing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OfficeModel {
    /// `[weekday][hour]`, `None` until first seen.
    bins: [[Option<f64>; 24]; 2],
    /// Weight of a new sample. 0.3 settles in about three days of one sample per hour.
    pub alpha: f64,
    pub samples: u64,
}

impl Default for OfficeModel {
    fn default() -> Self {
        Self { bins: [[None; 24]; 2], alpha: 0.3, samples: 0 }
    }
}

impl OfficeModel {
    fn idx(weekday: bool) -> usize {
        usize::from(weekday)
    }

    fn hour_bin(hour: f64) -> usize {
        (hour.rem_euclid(24.0).floor() as usize).min(23)
    }

    pub fn record(&mut self, hour: f64, weekday: bool, air_temp_c: f64) {
        let bin = &mut self.bins[Self::idx(weekday)][Self::hour_bin(hour)];
        *bin = Some(match *bin {
            None => air_temp_c,
            Some(prev) => prev + self.alpha * (air_temp_c - prev),
        });
        self.samples += 1;
    }

    /// Expected air temperature at `hour` (may exceed 24, wraps) on a weekday or weekend day.
    pub fn forecast(&self, hour: f64, weekday: bool) -> Option<f64> {
        self.bins[Self::idx(weekday)][Self::hour_bin(hour)]
    }

    /// Hour of the steepest drop in the weekday profile: when the air conditioner comes on.
    /// `None` until every weekday hour has a value.
    pub fn ac_on_hour(&self) -> Option<usize> {
        let w = &self.bins[1];
        if w.iter().any(Option::is_none) {
            return None;
        }
        (0..24).min_by(|&a, &b| {
            let da = w[(a + 1) % 24].unwrap_or(0.0) - w[a].unwrap_or(0.0);
            let db = w[(b + 1) % 24].unwrap_or(0.0) - w[b].unwrap_or(0.0);
            da.total_cmp(&db)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_the_air_conditioner_schedule() {
        let mut m = OfficeModel::default();
        for day in 0..4 {
            for h in 0..24 {
                let t = if (9..18).contains(&h) { 23.0 } else { 29.0 };
                m.record(f64::from(h) + 0.5, true, t + f64::from(day % 2) * 0.2);
            }
        }
        assert_eq!(m.ac_on_hour(), Some(8), "drop happens between 8 and 9");
        let noon = m.forecast(12.0, true).unwrap();
        assert!((noon - 23.1).abs() < 0.2, "{noon}");
        assert!(m.forecast(3.0, true).unwrap() > 28.5);
        assert_eq!(m.forecast(12.0, false), None, "weekend unseen");
    }

    #[test]
    fn hours_wrap() {
        let mut m = OfficeModel::default();
        m.record(1.0, false, 20.0);
        assert_eq!(m.forecast(25.0, false), Some(20.0));
        assert_eq!(m.forecast(-23.0, false), Some(20.0));
    }
}
