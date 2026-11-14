//! Which levels of a fallback sequence a hazard runs, and which of their
//! rows are judged against which deadline (v0.3.18; rlm#4 and rlm#6).
//!
//! Both realizers walk a hazard's fallback sequence and compute one row per
//! level: detection + the dwelling levels waited out above it (each its
//! route plus its charged dwell) + the level's route + its settle. How a
//! route is computed is the realizer's (play_launch's reaction walk,
//! nano-ros's `ftti_budget`). What is stated once, here, is the part both
//! must agree on so they report the same verdict and the same binding row:
//!
//! - [`skip`]: a level the hazard does not run (it requires a function the
//!   fault removes, or it `requires_lost` functions the fault removes none
//!   of). A skipped level is neither judged nor charged.
//! - [`judgements`]: every (row, deadline) pair that is judged. A level
//!   without a dwell, the minimal risk condition included, is judged
//!   against the hazard's `ftti` and against its own `within`; a dwelling
//!   level only against its `within` (it is transitional: its time is
//!   charged to the levels below it, which carry the `ftti`).
//! - [`binding`]: the pair whose total takes the largest share of its
//!   deadline (total / deadline), above 1 being a miss. A share, not the
//!   slack in ms: one hazard's deadlines run from tens of milliseconds
//!   (taking authority) to minutes (reaching the minimal risk condition),
//!   and 18 ms of slack on 70 ms is a looser fit than 10 s on 90 s. Ties go
//!   to the earlier level, and on one level to `ftti` before `within`.

use std::collections::BTreeSet;

/// Why a hazard does not run a level ([`skip`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// The level requires these functions, which the fault removes.
    Removes(Vec<String>),
    /// The level `requires_lost` these functions, and the fault removes
    /// none of them: the level is never available after this fault.
    NotLost(Vec<String>),
}

/// Whether a hazard whose fault removes `removed` skips a level that
/// `requires` and `requires_lost` the given functions. `None`: the hazard
/// runs it.
pub fn skip(
    requires: &[String],
    requires_lost: &[String],
    removed: &BTreeSet<String>,
) -> Option<Skip> {
    let lost: Vec<String> = requires
        .iter()
        .filter(|r| removed.contains(*r))
        .cloned()
        .collect();
    if !lost.is_empty() {
        return Some(Skip::Removes(lost));
    }
    if !requires_lost.is_empty() && !requires_lost.iter().any(|r| removed.contains(r)) {
        return Some(Skip::NotLost(requires_lost.to_vec()));
    }
    None
}

/// What kind of level a row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A level without a dwell, above the minimal risk condition.
    Level,
    /// A level with a `min_dwell`: transitional.
    Dwell,
    /// The last level: the minimal risk condition.
    Mrc,
}

/// One row a realizer computed for a level the hazard runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub level: String,
    pub role: Role,
    /// Fault to the level entered, ms: detection + waited + route + settle.
    pub total_ms: f64,
    /// The level's own `within`, ms.
    pub within_ms: Option<f64>,
}

/// Which deadline a row is judged against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deadline {
    /// The hazard's `ftti`.
    Ftti,
    /// The level's own `within`.
    Within,
}

/// One judged (row, deadline) pair.
#[derive(Debug, Clone, PartialEq)]
pub struct Judgement {
    /// Index into the rows given.
    pub row: usize,
    pub deadline: Deadline,
    pub deadline_ms: f64,
    pub total_ms: f64,
}

impl Judgement {
    /// Deadline minus total: negative is a miss.
    pub fn slack_ms(&self) -> f64 {
        self.deadline_ms - self.total_ms
    }

    pub fn fits(&self) -> bool {
        self.total_ms <= self.deadline_ms
    }

    /// Total over deadline: above 1 is a miss.
    pub fn share(&self) -> f64 {
        if self.deadline_ms > 0.0 {
            self.total_ms / self.deadline_ms
        } else {
            f64::INFINITY
        }
    }
}

/// Every judged pair, in row order, `ftti` before `within` on one row.
pub fn judgements(rows: &[Row], ftti_ms: Option<f64>) -> Vec<Judgement> {
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if r.role != Role::Dwell
            && let Some(f) = ftti_ms
        {
            out.push(Judgement {
                row: i,
                deadline: Deadline::Ftti,
                deadline_ms: f,
                total_ms: r.total_ms,
            });
        }
        if let Some(w) = r.within_ms {
            out.push(Judgement {
                row: i,
                deadline: Deadline::Within,
                deadline_ms: w,
                total_ms: r.total_ms,
            });
        }
    }
    out
}

/// The binding pair: the largest share of its deadline, ties to the
/// earlier pair.
pub fn binding(rows: &[Row], ftti_ms: Option<f64>) -> Option<Judgement> {
    judgements(rows, ftti_ms)
        .into_iter()
        .fold(None, |best: Option<Judgement>, j| match best {
            Some(b) if b.share() >= j.share() => Some(b),
            _ => Some(j),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(level: &str, role: Role, total_ms: f64, within_ms: Option<f64>) -> Row {
        Row {
            level: level.into(),
            role,
            total_ms,
            within_ms,
        }
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    /// The L4 island's B2: available only while the HPC is lost.
    #[test]
    fn a_level_requiring_a_loss_is_skipped_unless_the_fault_removes_it() {
        let req = vec!["camera".to_string()];
        let lost = vec!["hpc_alive".to_string()];
        assert_eq!(
            skip(&req, &lost, &set(&["hpc_alive", "hpc_commands"])),
            None
        );
        assert_eq!(
            skip(&req, &lost, &set(&["odd_measurable"])),
            Some(Skip::NotLost(lost.clone()))
        );
        assert_eq!(
            skip(&req, &lost, &set(&["hpc_alive", "camera"])),
            Some(Skip::Removes(req.clone()))
        );
        assert_eq!(skip(&req, &[], &set(&["x"])), None);
    }

    /// HPC loss on the L4 draft: B2 must take authority within 70 ms and
    /// dwells 15 s; B3 settles at 80 s, B4 at 21 s, ftti 90 s. B3 binds
    /// against the ftti (89 % of it); B2's `within` is judged (74 %), its
    /// dwell is not judged against the ftti.
    #[test]
    fn every_level_is_judged_and_the_largest_share_binds() {
        let rows = [
            row("lane_keep_hold", Role::Dwell, 52.0, Some(70.0)),
            row("pull_over", Role::Level, 80_000.0, None),
            row("blind_stop", Role::Mrc, 21_240.0, None),
        ];
        let js = judgements(&rows, Some(90_000.0));
        assert_eq!(
            js.iter().map(|j| (j.row, j.deadline)).collect::<Vec<_>>(),
            [
                (0, Deadline::Within),
                (1, Deadline::Ftti),
                (2, Deadline::Ftti)
            ]
        );
        let b = binding(&rows, Some(90_000.0)).unwrap();
        assert_eq!((b.row, b.deadline), (1, Deadline::Ftti));
        assert!((b.slack_ms() - 10_000.0).abs() < 1e-9);
        // A missed `within` binds instead: a share above 1.
        let rows = [
            row("lane_keep_hold", Role::Dwell, 80.0, Some(70.0)),
            row("blind_stop", Role::Mrc, 21_240.0, Some(60_000.0)),
        ];
        let b = binding(&rows, Some(90_000.0)).unwrap();
        assert_eq!((b.row, b.deadline, b.fits()), (0, Deadline::Within, false));
        // No ftti and no within: nothing is judged.
        assert_eq!(binding(&[row("x", Role::Mrc, 1.0, None)], None), None);
    }
}
