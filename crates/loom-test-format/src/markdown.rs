use crate::*;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TcMeta {
    #[serde(default)]
    pub loom_tc: Option<u32>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub env: Option<String>,
    #[serde(default)]
    pub equipment: Vec<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TcStep {
    pub n: u32,
    pub action: String,
    pub expected: String,
    pub criteria: String,
    pub source_line: usize,
    pub span: SourceSpan,
    pub cell_spans: [SourceSpan; 4],
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TcDoc {
    pub meta: TcMeta,
    /// Exact Markdown body, including the original table: never rewrite a user's sentences.
    pub description: String,
    pub steps: Vec<TcStep>,
    pub source_sha256: String,
    pub body_offset: usize,
}
pub fn parse_tc_bytes(bytes: &[u8]) -> Result<TcDoc, FormatError> {
    parse_tc(input(bytes)?)
}
pub fn parse_tc(source: &str) -> Result<TcDoc, FormatError> {
    input(source.as_bytes())?;
    let mut meta = TcMeta::default();
    let mut body_offset = 0;
    let mut lines = source.split_inclusive('\n');
    if lines
        .next()
        .is_some_and(|l| l.trim_end_matches(['\r', '\n']) == "---")
    {
        let start = source.find('\n').map_or(source.len(), |n| n + 1);
        let mut end = start;
        let mut closing = None;
        for l in lines {
            if l.trim_end_matches(['\r', '\n']) == "---" {
                closing = Some((end, end + l.len()));
                break;
            }
            end += l.len();
        }
        let Some((end, body)) = closing else {
            return Err(error(
                source,
                0,
                "/meta",
                "frontmatter",
                "unclosed YAML frontmatter",
            ));
        };
        let value = yaml::parse(&source[start..end], source, start)?;
        if !value.is_null() {
            meta = serde_json::from_value(value)
                .map_err(|e| error(source, start, "/meta", "metadata", e.to_string()))?;
        }
        if meta.loom_tc.is_some_and(|v| v != 1) {
            return Err(error(
                source,
                start,
                "/meta/loom_tc",
                "version",
                "loom_tc must be 1",
            ));
        }
        if meta
            .env
            .as_ref()
            .is_some_and(|s| !matches!(s.as_str(), "sim" | "bench" | "vehicle"))
        {
            return Err(error(
                source,
                start,
                "/meta/env",
                "env",
                "env must be sim, bench or vehicle",
            ));
        }
        body_offset = body;
    }
    let body = &source[body_offset..];
    let parser = Parser::new_ext(body, Options::ENABLE_TABLES).into_offset_iter();
    let mut heading = None::<(HeadingLevel, String)>;
    let mut section = None::<HeadingLevel>;
    let mut table = false;
    let mut header = false;
    let mut cells = Vec::<SourceSpan>::new();
    let mut columns = None::<[usize; 4]>;
    let mut row_start = 0;
    let mut steps = Vec::new();
    for (event, range) in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if section.is_some_and(|active| level <= active) {
                    section = None;
                }
                heading = Some((level, String::new()));
            }
            Event::Text(t) | Event::Code(t) if heading.is_some() => {
                if let Some((_, s)) = &mut heading {
                    s.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((level, text)) = heading.take()
                    && (text.trim() == "단계" || text.trim().eq_ignore_ascii_case("steps"))
                {
                    section = Some(level);
                }
            }
            Event::Start(Tag::Table(_)) if section.is_some() => {
                table = true;
                columns = None;
            }
            Event::Start(Tag::TableHead) if table => {
                header = true;
                cells.clear();
                row_start = range.start;
            }
            Event::Start(Tag::TableRow) if table => {
                header = false;
                cells.clear();
                row_start = range.start;
            }
            Event::Start(Tag::TableCell) if table => {
                if cells.len() >= MAX_COLLECTION_ITEMS {
                    return Err(error(
                        source,
                        body_offset + range.start,
                        "/steps",
                        "column_limit",
                        "too many table columns",
                    ));
                }
                let raw = &body[range.clone()];
                let trim = raw.trim();
                let left = raw.len() - raw.trim_start().len();
                let start = body_offset + range.start + left;
                if trim.len() > MAX_CELL_BYTES {
                    return Err(error(
                        source,
                        start,
                        "/steps",
                        "cell_limit",
                        "table cell exceeds 4 KiB",
                    ));
                }
                cells.push(SourceSpan {
                    start,
                    end: start + trim.len(),
                });
            }
            Event::End(TagEnd::TableHead) if table => {
                let mut indices = [None; 4];
                for (i, span) in cells.iter().enumerate() {
                    let label = source[span.start..span.end].to_ascii_lowercase();
                    let col = match label.as_str() {
                        "#" | "번호" | "step" => Some(0),
                        "동작" | "action" => Some(1),
                        "기대 결과" | "expected" => Some(2),
                        "합격 기준" | "pass criteria" => Some(3),
                        _ => None,
                    };
                    if let Some(c) = col
                        && indices[c].replace(i).is_some()
                    {
                        return Err(error(
                            source,
                            span.start,
                            "/steps",
                            "table_columns",
                            "duplicate step table column",
                        ));
                    }
                }
                let [Some(a), Some(b), Some(c), Some(d)] = indices else {
                    return Err(error(
                        source,
                        body_offset + row_start,
                        "/steps",
                        "table_columns",
                        "step table needs #/Step, Action, Expected and Pass criteria columns",
                    ));
                };
                columns = Some([a, b, c, d]);
            }
            Event::End(TagEnd::TableRow) if table && !header => {
                let Some(indices) = columns else {
                    return Err(error(
                        source,
                        body_offset + row_start,
                        "/steps",
                        "table_columns",
                        "missing table header",
                    ));
                };
                if steps.len() >= MAX_STEPS {
                    return Err(error(
                        source,
                        body_offset + row_start,
                        "/steps",
                        "step_limit",
                        "more than 500 TC steps",
                    ));
                }
                let mut spans = [SourceSpan { start: 0, end: 0 }; 4];
                for (i, column) in indices.iter().enumerate() {
                    spans[i] = *cells.get(*column).ok_or_else(|| {
                        error(
                            source,
                            body_offset + row_start,
                            "/steps",
                            "table_cells",
                            "step table row is missing cells",
                        )
                    })?;
                }
                let text = |i: usize| source[spans[i].start..spans[i].end].to_owned();
                let n = text(0).parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(|| {
                    error(
                        source,
                        spans[0].start,
                        "/steps/n",
                        "step_number",
                        "step number must be a positive integer",
                    )
                })?;
                steps.push(TcStep {
                    n,
                    action: text(1),
                    expected: text(2),
                    criteria: text(3),
                    source_line: Location::at(source, body_offset + row_start).line,
                    span: SourceSpan {
                        start: body_offset + row_start,
                        end: body_offset + range.end,
                    },
                    cell_spans: spans,
                });
            }
            Event::End(TagEnd::Table) if table => {
                table = false;
                header = false;
            }
            _ => (),
        }
    }
    Ok(TcDoc {
        meta,
        description: body.into(),
        steps,
        source_sha256: sha256(source.as_bytes()),
        body_offset,
    })
}
