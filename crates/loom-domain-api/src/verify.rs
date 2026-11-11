//! Evidence verification (contract v1 §7, plan §10.9). The core checks windows, coverage and gaps;
//! the domain re-evaluates meaning predicates on the original events.

use std::time::{Duration, Instant};

use loom_proto::event::{PublishedRecord, SeqRange, SourceRef};
use loom_proto::num::U64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A predicate to evaluate (`uart.text_contains`, `can.signal_range`, `uds.dtc_absent` …).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Predicate {
    /// Predicate name `<domain>.<name>`.
    pub name: String,
    /// Arguments (TEST comparison AST / domain args).
    pub args: Value,
    /// Registered plan expectation id this predicate answers.
    pub expect_id: Option<String>,
}

/// How much coverage a predicate needs to be decidable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageNeed {
    /// exists / count ≥ N: a partial window can prove it.
    Positive,
    /// absent / exact / max / hold / order: needs the whole window without loss.
    Complete,
    /// within_ms / measure: both end points + clock quality.
    Measure,
}

/// Coverage of the window handed to the verifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    /// Stream window.
    pub window: SeqRange,
    /// No record/source gap, retention loss, `--no-record` or scan limit inside the window.
    pub complete: bool,
    /// Causes of incompleteness, if any.
    pub causes: Vec<String>,
}

/// Reference to one event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventRef {
    /// Stream epoch.
    pub epoch: U64,
    /// Stream seq.
    pub seq: U64,
    /// Source identity.
    pub source: SourceRef,
}

/// Result of a domain verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// Predicate holds; witnesses listed.
    Match {
        /// Matching events.
        matched: Vec<EventRef>,
        /// Observed value summary.
        observed: Value,
    },
    /// Predicate is false.
    Mismatch {
        /// Observed value summary.
        observed: Value,
        /// Why.
        reason: String,
    },
    /// Cannot decide (coverage, clock quality, budget …).
    Inconclusive {
        /// Why.
        reason: String,
    },
}

/// Cooperative evaluation budget exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BudgetExceeded {
    /// Event count limit.
    #[error("event budget exceeded")]
    Events,
    /// CPU time limit.
    #[error("time budget exceeded")]
    Time,
}

/// Cooperative budget (TC-C9: ≤ 200k events, ≤ 200 ms CPU per check). Verifiers call
/// [`EvalBudget::spend`] as they go and return `Inconclusive{eval_budget}` when it fails.
#[derive(Debug, Clone)]
pub struct EvalBudget {
    events_left: usize,
    deadline: Instant,
}

impl EvalBudget {
    /// Default per-check event limit.
    pub const DEFAULT_EVENTS: usize = 200_000;
    /// Default per-check time limit.
    pub const DEFAULT_TIME: Duration = Duration::from_millis(200);

    /// New budget.
    pub fn new(max_events: usize, max_time: Duration) -> Self {
        EvalBudget {
            events_left: max_events,
            deadline: Instant::now() + max_time,
        }
    }

    /// Account for `events` more evaluated events; fails once either limit is exhausted.
    pub fn spend(&mut self, events: usize) -> Result<(), BudgetExceeded> {
        if events > self.events_left {
            self.events_left = 0;
            return Err(BudgetExceeded::Events);
        }
        self.events_left -= events;
        if Instant::now() > self.deadline {
            return Err(BudgetExceeded::Time);
        }
        Ok(())
    }

    /// Events still allowed.
    pub fn events_left(&self) -> usize {
        self.events_left
    }
}

impl Default for EvalBudget {
    fn default() -> Self {
        EvalBudget::new(Self::DEFAULT_EVENTS, Self::DEFAULT_TIME)
    }
}

/// Domain evidence verifier (uart #9, can §5, uds C-6).
pub trait EvidenceVerifier: Send + Sync {
    /// Predicate names this verifier handles (`<domain>.<name>`).
    fn predicates(&self) -> &[&'static str];
    /// Coverage requirement of a predicate.
    fn coverage_need(&self, pred: &Predicate) -> CoverageNeed;
    /// Evaluate on the original events of the window (the core re-queried the whole window).
    fn verify(
        &self,
        pred: &Predicate,
        events: &[PublishedRecord],
        cov: &Coverage,
        budget: &mut EvalBudget,
    ) -> Verification;
}
