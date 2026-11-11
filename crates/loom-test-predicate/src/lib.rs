//! `loom-test-predicate` — TEST predicate AST, common evidence predicates and verdict rules.
//! Pure library (no I/O): the CORE session engine re-reads the window from the journal and
//! hands `&[PublishedRecord]` + `Coverage` here (contract 07 §7.3–7.4, TEST plan §3.1, §6.3–6.4).
//!
//! - [`ast`]: comparison AST — parse (path-tagged errors), type check against domain
//!   `PointerSpec`s, evaluate (missing/wrong type = error, never false).
//! - [`pred`]: common predicates `exists absent count order within_ms hold measure` as
//!   [`pred::Citation`]s, coverage gating, role checks, digests; [`pred::CommonVerifier`] is the
//!   CORE `EvidenceVerifier`.
//! - [`render`]: Korean sentences for the approval screen.
//! - [`verdict`]: step judgment (R-1..R-9) and session aggregation (exit 0/11/8/6).
//! - [`canonical`]: hash convention shared with `loom-test-format`.

pub mod ast;
pub mod canonical;
pub mod limits;
pub mod pred;
pub mod render;
pub mod units;
pub mod verdict;

pub use ast::{EvalError, Expr, PredError};
pub use pred::{Check, Citation, CommonVerifier, Status};
