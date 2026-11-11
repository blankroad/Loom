//! Every bound of this crate (C2, contract 07 §7.1·§7.3-7). Over a bound: parse → error with
//! the path; evaluation → `Inconclusive{cause:"eval_budget"}` (or the noted behaviour).

/// AST nesting depth (`all`/`any`/`not`). Over: parse error.
pub const MAX_DEPTH: usize = 32;
/// AST nodes (comparisons + logic nodes). Over: parse error.
pub const MAX_NODES: usize = 512;
/// Items of an `in` list. Over: parse error.
pub const MAX_IN_ITEMS: usize = 256;
/// Regex pattern bytes. Over: parse error.
pub const MAX_REGEX_LEN: usize = 1024;
/// Compiled regex / DFA cache size (Rust `regex`, no backtracking). Over: parse error.
pub const REGEX_SIZE_LIMIT: usize = 256 * 1024;
/// String literal bytes (values, `contains`). Over: parse error.
pub const MAX_STRING_LEN: usize = 4096;
/// JSON Pointer bytes. Over: parse error.
pub const MAX_POINTER_LEN: usize = 512;
/// `bytes_eq` literal bytes. Over: parse error.
pub const MAX_BYTES_LEN: usize = 4096;
/// Events one check may evaluate (also enforced through `EvalBudget`). Over: inconclusive.
pub const MAX_EVAL_EVENTS: usize = 200_000;
/// Matched REFs reported per check (contract: ≤ 64 per verdict). More matches are counted.
pub const MAX_MATCHED_REFS: usize = 64;
/// `observed` JSON bytes per check (plan §3.1-8). Over: replaced by a short summary.
pub const MAX_OBSERVED_BYTES: usize = 2048;
/// Default largest sample interval of a hold (contract 07 §7.3-5).
pub const DEFAULT_MAX_GAP_MS: u64 = 150;
/// Longest hold / within / measure bound.
pub const MAX_DURATION_MS: u64 = 24 * 3600 * 1000;
/// Steps per TC (contract 07 §7.1). Over: aggregation error.
pub const MAX_STEPS: usize = 500;
/// Events charged to the budget at once (cooperative checks).
pub const BUDGET_CHUNK: usize = 256;
