//! Pure, bounded TC Markdown and plan/scenario YAML. No filesystem IO or execution.
mod canonical;
mod check;
mod markdown;
mod plan;
mod yaml;
pub use canonical::{canonical_json, canonical_sha256, sha256};
pub use check::{
    CommandSpec, CommandTable, Report, Risk, RiskLevel, RiskProvider, RiskUse, StructuralReport, check_plan,
    inspect_plan,
};
pub use markdown::{TcDoc, TcMeta, TcStep, parse_tc, parse_tc_bytes};
pub use plan::{
    Action, Anchor, Expectation, FailureAction, Filter, Manual, Plan, PlanFormat, Step, When, parse_plan,
    parse_plan_bytes, parse_plan_with_tc,
};
use serde::{Deserialize, Serialize};
use std::fmt;
/// Input byte ceiling; reject before parsing, allocating AST nodes or hashing.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Total steps including finally; reject the first excess step with its location.
pub const MAX_STEPS: usize = 500;
/// Markdown source bytes per cell; reject rather than truncate original sentences.
pub const MAX_CELL_BYTES: usize = 4096;
/// YAML/JSON nesting ceiling, including opaque args/pred; reject before descending.
pub const MAX_DEPTH: usize = 32;
/// YAML/JSON value and key node ceiling, including alias expansion; reject before allocation.
pub const MAX_NODES: usize = 65_536;
/// Expanded AST retained-byte budget (128 bytes/node plus scalar bytes); reject before cloning aliases.
pub const MAX_AST_BYTES: usize = 8 * 1024 * 1024;
/// Conservative retained allocation charge per AST value/key, including collection overhead.
pub const CAP_AST_NODE_BYTES: usize = 128;
/// Entries per YAML/JSON collection; reject before adding an excess entry.
pub const MAX_COLLECTION_ITEMS: usize = 8192;
/// Single YAML/JSON scalar byte ceiling; reject before retaining it.
pub const MAX_SCALAR_BYTES: usize = 64 * 1024;
/// RPC invocations per step; reject excess entries rather than dropping commands.
pub const MAX_ACTIONS: usize = 64;
/// Expectations/kinds/origins per step or filter; reject excess entries.
pub const MAX_EXPECTATIONS: usize = 64;
/// Identifier/method length ceiling; reject rather than truncate identities.
pub const MAX_ID_BYTES: usize = 128;
/// Additional attempts, not total attempts; reject count >3.
pub const MAX_RETRY: u32 = 3;
/// Bounded repeat iterations; reject count outside 1..=100.
pub const MAX_REPEAT: u32 = 100;
/// Individual timeout/delay ceiling; reject nonpositive or larger durations.
pub const MAX_TIMEOUT_MS: u64 = 3_600_000;
/// Diagnostic count ceiling; stop accumulating after this many errors/warnings.
pub const MAX_DIAGNOSTICS: usize = 1024;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}
impl Location {
    pub(crate) fn at(text: &str, offset: usize) -> Self {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let before = &text[..offset];
        let start = before.rfind('\n').map_or(0, |n| n + 1);
        Self {
            line: before.bytes().filter(|b| *b == b'\n').count() + 1,
            column: text[start..offset].chars().count() + 1,
            offset,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub path: String,
    pub location: Location,
}
pub type FormatError = Diagnostic;
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {} ({} at {})",
            self.location.line, self.location.column, self.message, self.code, self.path
        )
    }
}
impl std::error::Error for Diagnostic {}
pub(crate) fn error(
    text: &str,
    offset: usize,
    path: &str,
    code: &str,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        path: path.into(),
        location: Location::at(text, offset),
    }
}
pub(crate) fn input(bytes: &[u8]) -> Result<&str, FormatError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(error("", 0, "/", "file_limit", "input exceeds 1 MiB"));
    }
    std::str::from_utf8(bytes).map_err(|e| {
        error(
            std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or(""),
            e.valid_up_to(),
            "/",
            "utf8",
            "input is not UTF-8",
        )
    })
}
