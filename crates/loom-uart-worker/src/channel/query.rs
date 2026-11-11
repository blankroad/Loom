//! Request shapes for [`super::ChannelCore`]: read / lines queries, expect start and patterns,
//! plus parsers for the RPC params (`crates/loom-uart/src/commands.rs` schemas).

use loom_console::cursor::{ReadFrom, ReadMode};
use loom_console::matcher::Pattern;
use loom_console::text::{AnsiMode, Encoding};
use loom_proto::event::Dir;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ChannelError;

/// Largest `limit` of one `uart.lines` call (UI contract `limit ≤ 2000`).
pub const LINES_MAX_LIMIT: u32 = 2000;
/// Default `limit` of `uart.lines`.
pub const LINES_DEFAULT_LIMIT: u32 = 500;

/// A parsed position spec (UART plan §3.3 "위치 지정자", the subset the worker can resolve).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PosSpec {
    /// `@last` — the client's cursor.
    Last,
    /// `now` / `-0s` — the current RX high-water mark.
    Now,
    /// `off:N` — RX offset.
    RxOffset(u64),
    /// `src:N` — first RX byte at/after this worker `source_seq` (the gateway maps `seq:N` here).
    SourceSeq(u64),
}

impl PosSpec {
    /// Parse `@last`, `now`, `-0s`, `off:N`, `src:N` / `source_seq:N`. Stream `seq:N`, marks
    /// and times need the gateway (it knows seq ↔ source_seq) → `E_USAGE` here.
    pub fn parse(s: &str) -> Result<PosSpec, ChannelError> {
        let s = s.trim();
        let num = |v: &str| {
            v.parse::<u64>()
                .map_err(|_| ChannelError::Usage(format!("bad position '{s}': expected a decimal u64")))
        };
        if s == "@last" || s.is_empty() {
            Ok(PosSpec::Last)
        } else if s == "now" || s == "-0s" {
            Ok(PosSpec::Now)
        } else if let Some(v) = s.strip_prefix("off:") {
            num(v).map(PosSpec::RxOffset)
        } else if let Some(v) = s.strip_prefix("src:").or_else(|| s.strip_prefix("source_seq:")) {
            num(v).map(PosSpec::SourceSeq)
        } else {
            Err(ChannelError::Usage(format!(
                "position '{s}' is not resolvable by the worker (use @last, now, off:N or src:N; \
                 seq:N / mark: / times are mapped by the gateway)"
            )))
        }
    }
}

/// `uart.read` request (plan §3.6, §4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadQuery {
    /// Start (default: the client's cursor).
    pub from: ReadFrom,
    /// Stop before this RX offset (exclusive); `None` = up to the newest byte.
    pub until: Option<u64>,
    /// Summary (default) / page / peek.
    pub mode: ReadMode,
    /// `--max-lines` (default 100, hard 1000).
    pub max_lines: Option<u32>,
    /// `--max-bytes` (default 16 KiB, hard 64 KiB; counted on the text).
    pub max_bytes: Option<u32>,
    /// `--ansi` (default strip).
    pub ansi: AnsiMode,
    /// `--keep-cr`.
    pub keep_cr: bool,
    /// Channel encoding.
    pub encoding: Encoding,
}

impl Default for ReadQuery {
    fn default() -> Self {
        Self {
            from: ReadFrom::Last,
            until: None,
            mode: ReadMode::Summary,
            max_lines: None,
            max_bytes: None,
            ansi: AnsiMode::Strip,
            keep_cr: false,
            encoding: Encoding::Utf8,
        }
    }
}

fn get_str<'a>(p: &'a Value, k: &str) -> Result<Option<&'a str>, ChannelError> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ChannelError::Usage(format!("'{k}' must be a string"))),
    }
}

fn get_u32(p: &Value, k: &str) -> Result<Option<u32>, ChannelError> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| ChannelError::Usage(format!("'{k}' must be a non-negative integer"))),
    }
}

fn get_bool(p: &Value, k: &str) -> Result<bool, ChannelError> {
    match p.get(k) {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(ChannelError::Usage(format!("'{k}' must be a boolean"))),
    }
}

fn parse_ansi(p: &Value) -> Result<AnsiMode, ChannelError> {
    match get_str(p, "ansi")? {
        None | Some("strip") => Ok(AnsiMode::Strip),
        Some("keep") => Ok(AnsiMode::Keep),
        Some("escape") => Ok(AnsiMode::Escape),
        Some(o) => Err(ChannelError::Usage(format!("bad ansi mode '{o}'"))),
    }
}

fn parse_encoding(p: &Value) -> Result<Encoding, ChannelError> {
    match get_str(p, "encoding")? {
        None | Some("utf-8" | "utf8") => Ok(Encoding::Utf8),
        Some("latin-1" | "latin1") => Ok(Encoding::Latin1),
        Some(o) => Err(ChannelError::Usage(format!("unsupported encoding '{o}'"))),
    }
}

fn parse_until(p: &Value) -> Result<Option<u64>, ChannelError> {
    match get_str(p, "until")? {
        None => Ok(None),
        Some(s) => match PosSpec::parse(s)? {
            PosSpec::RxOffset(o) => Ok(Some(o)),
            PosSpec::Now | PosSpec::Last => Ok(None),
            PosSpec::SourceSeq(_) => Err(ChannelError::Usage("'until' takes off:N".into())),
        },
    }
}

impl ReadQuery {
    /// Parse `uart.read` params: `from` (`@last`|`off:N`|`src:N`), `cursor_token`, `until`
    /// (`off:N`), `mode`, `max_lines`, `max_bytes`, `ansi`, `keep_cr`, `encoding`.
    /// `from: "now"` reads nothing old: it starts at the current end.
    pub fn from_params(p: &Value) -> Result<ReadQuery, ChannelError> {
        let mut q = ReadQuery::default();
        if let Some(t) = get_str(p, "cursor_token")? {
            q.from = ReadFrom::Token(t.to_owned());
        } else if let Some(f) = get_str(p, "from")? {
            q.from = match PosSpec::parse(f)? {
                PosSpec::Last => ReadFrom::Last,
                PosSpec::Now => ReadFrom::RxOffset(u64::MAX),
                PosSpec::RxOffset(o) => ReadFrom::RxOffset(o),
                PosSpec::SourceSeq(s) => ReadFrom::SourceSeq(s),
            };
        }
        q.until = parse_until(p)?;
        q.mode = match get_str(p, "mode")? {
            None | Some("summary") => ReadMode::Summary,
            Some("page") => ReadMode::Page,
            Some("peek") => ReadMode::Peek,
            Some(o) => return Err(ChannelError::Usage(format!("bad read mode '{o}'"))),
        };
        q.max_lines = get_u32(p, "max_lines")?;
        q.max_bytes = get_u32(p, "max_bytes")?;
        q.ansi = parse_ansi(p)?;
        q.keep_cr = get_bool(p, "keep_cr")?;
        q.encoding = parse_encoding(p)?;
        Ok(q)
    }
}

/// `uart.lines` request (panel trace history; never moves a cursor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinesQuery {
    /// Forward from this RX offset. `None` = the **last** `limit` rows before `until`.
    pub from: Option<u64>,
    /// Exclusive RX end; `None` = the newest byte.
    pub until: Option<u64>,
    /// Rows wanted (clamped to `1..=2000`).
    pub limit: u32,
    /// `None` = RX lines and TX rows interleaved; `Some(Rx)` / `Some(Tx)` = only that direction.
    pub dir: Option<Dir>,
    /// ANSI handling of RX lines (default strip, like `read`).
    pub ansi: AnsiMode,
    /// `--keep-cr`.
    pub keep_cr: bool,
}

impl Default for LinesQuery {
    fn default() -> Self {
        Self {
            from: None,
            until: None,
            limit: LINES_DEFAULT_LIMIT,
            dir: None,
            ansi: AnsiMode::Strip,
            keep_cr: false,
        }
    }
}

impl LinesQuery {
    /// Parse `uart.lines` params: `from` / `until` (`off:N`), `limit` (≤ 2000; `max_lines` is
    /// accepted as an alias), `dir` (`rx`|`tx`), `ansi`, `keep_cr`.
    pub fn from_params(p: &Value) -> Result<LinesQuery, ChannelError> {
        let mut q = LinesQuery::default();
        if let Some(f) = get_str(p, "from")? {
            q.from = match PosSpec::parse(f)? {
                PosSpec::RxOffset(o) => Some(o),
                PosSpec::Last | PosSpec::Now => None,
                PosSpec::SourceSeq(_) => {
                    return Err(ChannelError::Usage("uart.lines 'from' takes off:N".into()));
                }
            };
        }
        q.until = parse_until(p)?;
        if let Some(l) = get_u32(p, "limit")?.or(get_u32(p, "max_lines")?) {
            if l == 0 || l > LINES_MAX_LIMIT {
                return Err(ChannelError::Usage(format!(
                    "limit must be 1..={LINES_MAX_LIMIT}"
                )));
            }
            q.limit = l;
        }
        q.dir = match get_str(p, "dir")? {
            None | Some("both") => None,
            Some("rx") => Some(Dir::Rx),
            Some("tx") => Some(Dir::Tx),
            Some(o) => return Err(ChannelError::Usage(format!("bad dir '{o}'"))),
        };
        q.ansi = parse_ansi(p)?;
        q.keep_cr = get_bool(p, "keep_cr")?;
        Ok(q)
    }
}

/// Where an expect starts matching (plan §4.3, §4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectFrom {
    /// The RX high-water mark at registration (`send --expect`: taken before the TX intent).
    Now,
    /// An RX offset (`--from off:N`, `resume_from`).
    RxOffset(u64),
    /// The client's read cursor (`expect` default `@last`); success moves it to the match end.
    Cursor(String),
}

impl ExpectFrom {
    /// From an optional `from` param of client `client`: missing / `@last` → `Cursor(client)`,
    /// `now` → `Now`, `off:N` → `RxOffset(N)`.
    pub fn from_spec(spec: Option<&str>, client: &str) -> Result<ExpectFrom, ChannelError> {
        match spec.map(PosSpec::parse).transpose()? {
            None | Some(PosSpec::Last) => Ok(ExpectFrom::Cursor(client.to_owned())),
            Some(PosSpec::Now) => Ok(ExpectFrom::Now),
            Some(PosSpec::RxOffset(o)) => Ok(ExpectFrom::RxOffset(o)),
            Some(PosSpec::SourceSeq(_)) => Err(ChannelError::Usage(
                "expect 'from' takes @last, now or off:N".into(),
            )),
        }
    }
}

/// Pattern kind (`{"kind": "literal"|"regex"|"prompt"}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    /// Exact text (default; no regex engine).
    #[default]
    Literal,
    /// Rust `regex` syntax.
    Regex,
    /// Prompt regex anchored at the end of the data; empty `pattern` = the profile prompt.
    Prompt,
}

/// One expect pattern as sent over RPC: `{"kind": "...", "pattern": "..."}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternSpec {
    /// Kind (default literal).
    #[serde(default)]
    pub kind: PatternKind,
    /// Pattern text (UTF-8; literal bytes are its UTF-8 encoding).
    #[serde(default)]
    pub pattern: String,
}

impl PatternSpec {
    /// Literal pattern.
    pub fn literal(s: &str) -> Self {
        Self {
            kind: PatternKind::Literal,
            pattern: s.to_owned(),
        }
    }

    /// Regex pattern.
    pub fn regex(s: &str) -> Self {
        Self {
            kind: PatternKind::Regex,
            pattern: s.to_owned(),
        }
    }

    /// The profile prompt.
    pub fn prompt() -> Self {
        Self {
            kind: PatternKind::Prompt,
            pattern: String::new(),
        }
    }

    /// Matcher pattern; an empty prompt pattern resolves to `profile_prompt`.
    pub(crate) fn to_pattern(&self, profile_prompt: Option<&str>) -> Result<Pattern, ChannelError> {
        Ok(match self.kind {
            PatternKind::Literal => Pattern::Literal(self.pattern.as_bytes().to_vec()),
            PatternKind::Regex => Pattern::Regex(self.pattern.clone()),
            PatternKind::Prompt if self.pattern.is_empty() => Pattern::Prompt(
                profile_prompt
                    .ok_or_else(|| ChannelError::Usage("the channel profile has no prompt".into()))?
                    .to_owned(),
            ),
            PatternKind::Prompt => Pattern::Prompt(self.pattern.clone()),
        })
    }
}
