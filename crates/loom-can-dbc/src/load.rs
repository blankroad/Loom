//! Loading: bytes → text → can-dbc AST (with recovery) → compiled database.

use std::collections::HashMap;

use can_dbc::{Dbc, DbcError, MessageId};
use sha2::{Digest, Sha256};

use crate::compile::{Block, CompileInput, Placeholder, compile};
use crate::error::{LimitKind, LoadError, WarningKind, Warnings};
use crate::model::Database;
use crate::options::LoadOptions;
use crate::text::{Statement, bo_header, decode_text, referenced_message, statements};

/// Sections that parse but that Loom does not use (reported once per keyword).
const IGNORED_SECTIONS: &[(&str, &str)] = &[
    ("EV_", "environment variables"),
    ("ENVVAR_DATA_", "environment variable data"),
    ("SIG_GROUP_", "signal groups"),
    ("BA_DEF_REL_", "relation attributes"),
    ("BA_REL_", "relation attributes"),
    ("BA_DEF_DEF_REL_", "relation attribute defaults"),
    ("VAL_TABLE_", "global value tables (not linked to signals)"),
];

impl Database {
    /// Loads a DBC from file bytes.
    ///
    /// Statements that can-dbc cannot parse are removed and reported as warnings with
    /// their line. If such a statement can change a message's layout, that message is
    /// kept but encode (and possibly decode) is blocked (fail closed).
    pub fn load(bytes: &[u8], options: &LoadOptions) -> Result<(Database, Vec<crate::Warning>), LoadError> {
        let limits = &options.limits;
        check_limit(LimitKind::FileBytes, bytes.len(), limits.max_file_bytes)?;
        let sha256: [u8; 32] = Sha256::digest(bytes).into();
        let (text, encoding, guessed) = decode_text(bytes, options.encoding)?;
        let mut warnings = Warnings::new(limits.max_warnings);
        if guessed {
            warnings.push(
                None,
                WarningKind::EncodingGuessed,
                format!("encoding not given; read as {encoding:?}"),
            );
        }

        let stats = scan(&text, limits.max_statements, limits.max_messages)?;
        report_ignored(&stats, &mut warnings);

        let parsed = match Dbc::try_from(text.as_ref()) {
            Ok(dbc) => Parsed {
                dbc,
                headers: stats.headers,
                blocks: HashMap::new(),
                placeholders: Vec::new(),
            },
            Err(_) => recover(&text, options, &mut warnings)?,
        };

        let db = compile(CompileInput {
            dbc: parsed.dbc,
            headers: parsed.headers,
            blocks: parsed.blocks,
            placeholders: parsed.placeholders,
            limits,
            sha256,
            encoding,
            encoding_guessed: guessed,
            warnings: &mut warnings,
        })?;
        Ok((db, warnings.finish()))
    }
}

pub(crate) fn check_limit(what: LimitKind, actual: usize, limit: usize) -> Result<(), LoadError> {
    if actual > limit {
        Err(LoadError::LimitExceeded {
            what,
            limit: limit as u64,
            actual: actual as u64,
        })
    } else {
        Ok(())
    }
}

/// `BO_` header as written in the text (can-dbc truncates some ids, so we keep our own).
#[derive(Debug, Clone)]
pub(crate) struct Header {
    pub(crate) raw_id: Option<u64>,
    pub(crate) line: u32,
}

struct Stats {
    headers: Vec<Header>,
    /// keyword → (count, first line)
    keywords: HashMap<String, (u32, u32)>,
}

fn scan(text: &str, max_statements: usize, max_messages: usize) -> Result<Stats, LoadError> {
    let mut headers = Vec::new();
    let mut keywords: HashMap<String, (u32, u32)> = HashMap::new();
    let mut count = 0usize;
    for st in statements(text) {
        count += 1;
        check_limit(LimitKind::Statements, count, max_statements)?;
        let kw = st.keyword();
        if kw == "BO_" {
            check_limit(LimitKind::Messages, headers.len() + 1, max_messages)?;
            headers.push(Header {
                raw_id: bo_header(st.text).map(|(id, _)| id),
                line: st.line,
            });
        } else if IGNORED_SECTIONS.iter().any(|(k, _)| *k == kw) {
            let e = keywords.entry(kw.to_string()).or_insert((0, st.line));
            e.0 += 1;
        }
    }
    Ok(Stats { headers, keywords })
}

fn report_ignored(stats: &Stats, warnings: &mut Warnings) {
    for (kw, what) in IGNORED_SECTIONS {
        if let Some((n, line)) = stats.keywords.get(*kw) {
            warnings.push(
                Some(*line),
                WarningKind::SectionIgnored,
                format!("{n} × {kw} ({what}) not used by Loom"),
            );
        }
    }
}

struct Parsed {
    dbc: Dbc,
    headers: Vec<Header>,
    blocks: HashMap<MessageId, Vec<Block>>,
    placeholders: Vec<Placeholder>,
}

/// Slow path: parse each statement alone, drop the ones that fail, parse the rest.
fn recover(text: &str, options: &LoadOptions, warnings: &mut Warnings) -> Result<Parsed, LoadError> {
    let limits = &options.limits;
    let mut kept = String::with_capacity(text.len() + 64);
    let mut headers = Vec::new();
    let mut blocks: HashMap<MessageId, Vec<Block>> = HashMap::new();
    let mut placeholders = Vec::new();
    let mut parsed_ok = 0usize;
    let mut dropped = 0usize;
    let mut first_error: Option<(u32, String)> = None;
    let mut one = String::new();

    for st in statements(text) {
        one.clear();
        one.push_str(st.text);
        if !one.ends_with('\n') {
            one.push('\n');
        }
        match Dbc::try_from(one.as_str()) {
            Ok(_) => {
                parsed_ok += 1;
                if st.keyword() == "BO_" {
                    headers.push(Header {
                        raw_id: bo_header(st.text).map(|(id, _)| id),
                        line: st.line,
                    });
                }
                kept.push_str(&one);
            }
            Err(e) => {
                dropped += 1;
                check_limit(
                    LimitKind::DroppedStatements,
                    dropped,
                    limits.max_dropped_statements,
                )?;
                let line = error_line(&e, &st);
                let reason = short_error(&e);
                if first_error.is_none() {
                    first_error = Some((line, reason.clone()));
                }
                warnings.push(
                    Some(line),
                    WarningKind::StatementDropped,
                    format!("{} — can-dbc: {reason}", st.summary()),
                );
                register_effect(&st, line, &mut blocks, &mut placeholders);
            }
        }
    }

    if parsed_ok == 0 {
        let (line, reason) = first_error.unwrap_or((0, "empty file".into()));
        return Err(LoadError::InvalidDbc {
            line: (line > 0).then_some(line),
            reason: format!("no statement could be parsed ({reason})"),
        });
    }
    let dbc = Dbc::try_from(kept.as_str()).map_err(|e| LoadError::InvalidDbc {
        line: None,
        reason: format!("cleaned text still failed to parse: {}", short_error(&e)),
    })?;
    Ok(Parsed {
        dbc,
        headers,
        blocks,
        placeholders,
    })
}

/// What a dropped statement means for the message it names.
fn register_effect(
    st: &Statement<'_>,
    line: u32,
    blocks: &mut HashMap<MessageId, Vec<Block>>,
    placeholders: &mut Vec<Placeholder>,
) {
    let kw = st.keyword();
    if kw == "BO_" {
        if let Some((raw, name)) = bo_header(st.text) {
            // Header line, like `Message::line()` of parsed messages.
            placeholders.push(Placeholder {
                raw_id: raw,
                name: name.to_string(),
                line: st.line,
            });
        }
        return;
    }
    let Some((raw, attr)) = referenced_message(st.text) else {
        return;
    };
    let Some(id) = u32::try_from(raw).ok().and_then(|r| MessageId::try_from(r).ok()) else {
        return;
    };
    let block = match (kw, attr) {
        // Mux ranges or float type unknown: neither direction is safe.
        ("SG_MUL_VAL_" | "SIG_VALTYPE_", _) => Block {
            line,
            reason: format!("line {line}: unparsable {kw} changes the signal layout"),
            decode: true,
        },
        // Start values or frame format unknown: the bytes we would send are unknown.
        ("BA_", Some("GenSigStartValue" | "VFrameFormat")) => Block {
            line,
            reason: format!("line {line}: unparsable {kw} for {}", attr.unwrap_or("")),
            decode: false,
        },
        _ => return,
    };
    blocks.entry(id).or_default().push(block);
}

fn error_line(e: &DbcError, st: &Statement<'_>) -> u32 {
    if let DbcError::Pest(p) = e {
        let line = match p.line_col {
            pest::error::LineColLocation::Pos((l, _)) => l,
            pest::error::LineColLocation::Span((l, _), _) => l,
        };
        return st.line + line.saturating_sub(1) as u32;
    }
    st.line
}

fn short_error(e: &DbcError) -> String {
    match e {
        DbcError::Pest(p) => {
            let msg = match &p.variant {
                pest::error::ErrorVariant::ParsingError { positives, .. } => {
                    let names: Vec<String> = positives.iter().take(4).map(|r| format!("{r:?}")).collect();
                    format!("syntax error, expected {}", names.join(" | "))
                }
                pest::error::ErrorVariant::CustomError { message } => message.clone(),
            };
            msg.chars().take(160).collect()
        }
        other => other.to_string().chars().take(160).collect(),
    }
}
