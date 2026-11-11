//! `loom-eth-filter` — the `loom-pktfilter` display filter: a Wireshark display-filter subset
//! evaluated over `loom-eth-decode` fields (plan §4.1, decision X4).
//!
//! One implementation for three users: the eth worker (push-down), `eth.query`, and the TEST
//! evidence verifier. Pipeline: lexer → parser (AST) → type check against the decode field
//! registry → evaluator over [`loom_eth_decode::Decoded::field_values`].
//!
//! ```
//! use loom_eth_filter::Filter;
//! let f = Filter::compile("doip.type == 0x8001 && ip.addr == 192.0.2.0/24").expect("valid");
//! assert_eq!(f.fields_used(), &["doip.type", "ip.addr"]);
//! let e = Filter::compile("doip.type ==").expect_err("incomplete");
//! assert_eq!(e.pos, 12);
//! ```
//!
//! Semantics defined by Loom (not version-dependent Wireshark behaviour):
//! - multi-value fields: a test is true if any value satisfies it;
//! - `a != b` is `not (a == b)` (true when the field is absent);
//! - a bare field or protocol name tests presence.
//!
//! Not supported (explicit error with position): functions, arithmetic and bitwise operators,
//! field-to-field comparison, the `#` layer operator, macros, `===`/`!==`, `any`/`all`.

mod compile;
mod eval;
mod lexer;
mod parser;

use compile::Node;

/// Longest filter text (bytes). Over: rejected at position 4096.
pub const MAX_FILTER_LEN: usize = 4096;
/// Deepest nesting of parentheses and `not`. Over: rejected at the offending token.
pub const MAX_DEPTH: usize = 64;
/// AST nodes (tests, logical operators, set items). Over: rejected.
pub const MAX_NODES: usize = 1024;
/// Items in one `in { … }` set. Over: rejected.
pub const MAX_SET_ITEMS: usize = 256;
/// Longest `matches` pattern (bytes). Over: rejected.
pub const MAX_REGEX_LEN: usize = 1024;
/// Compiled regex size and lazy-DFA cache limit (bytes) for each pattern. Over: rejected.
pub const REGEX_SIZE_LIMIT: usize = 256 * 1024;

/// Compile error with the byte offset in the filter text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{msg} at offset {pos} (expected {expected})")]
pub struct FilterError {
    /// Byte offset of the offending token.
    pub pos: usize,
    /// What would have been accepted there.
    pub expected: String,
    pub msg: String,
}

/// Time references for `frame.time_relative` and `frame.time_delta`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EvalCtx {
    /// `frame.time_relative = ts_ns - time_ref_ns` (seconds in literals).
    pub time_ref_ns: u64,
    /// Timestamp of the previously displayed frame; None → `frame.time_delta` = 0.
    pub prev_ts_ns: Option<u64>,
}

/// A compiled display filter.
#[derive(Debug, Clone)]
pub struct Filter {
    src: String,
    root: Node,
    used: Vec<&'static str>,
}

impl Filter {
    /// Parses and type-checks a filter.
    pub fn compile(src: &str) -> Result<Filter, FilterError> {
        if src.len() > MAX_FILTER_LEN {
            return Err(FilterError {
                pos: MAX_FILTER_LEN,
                expected: format!("at most {MAX_FILTER_LEN} bytes"),
                msg: format!("filter is {} bytes long", src.len()),
            });
        }
        let toks = lexer::lex(src)?;
        let ast = parser::Parser::new(toks).parse()?;
        let mut used = Vec::new();
        let root = compile::compile(ast, &mut used)?;
        Ok(Filter {
            src: src.to_string(),
            root,
            used,
        })
    }

    /// Evaluates with a zero time reference (`frame.time_relative` = packet timestamp).
    pub fn matches(&self, d: &loom_eth_decode::Decoded) -> bool {
        eval::eval(&self.root, d, &EvalCtx::default())
    }

    /// Evaluates with explicit time references.
    pub fn matches_with(&self, d: &loom_eth_decode::Decoded, cx: &EvalCtx) -> bool {
        eval::eval(&self.root, d, cx)
    }

    /// Canonical field names the filter reads, in first-use order (for push-down).
    pub fn fields_used(&self) -> &[&'static str] {
        &self.used
    }

    /// The original filter text.
    pub fn as_str(&self) -> &str {
        &self.src
    }
}
