//! Text decoding and splitting the DBC into top-level statements.
//!
//! can-dbc parses a whole file or fails. To keep going past statements it cannot read
//! (and report their line), we split the text ourselves: a statement starts at a line
//! whose first character is not whitespace and that is not inside a quoted string.
//! Indented lines (`SG_` under `BO_`, symbols under `NS_`) belong to the statement above.

use std::borrow::Cow;

use crate::error::LoadError;
use crate::options::TextEncoding;

/// Decodes file bytes. Returns the text, the encoding used and whether it was guessed.
pub(crate) fn decode_text(
    bytes: &[u8],
    encoding: TextEncoding,
) -> Result<(Cow<'_, str>, TextEncoding, bool), LoadError> {
    let utf8 = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match encoding {
        TextEncoding::Utf8 => match std::str::from_utf8(utf8) {
            Ok(s) => Ok((Cow::Borrowed(s), TextEncoding::Utf8, false)),
            Err(e) => Err(LoadError::Encoding {
                offset: e.valid_up_to() + (bytes.len() - utf8.len()),
                detail: "invalid UTF-8 (load with Windows-1252 or Auto for Latin-1 files)".into(),
            }),
        },
        TextEncoding::Windows1252 => Ok((cp1252(bytes), TextEncoding::Windows1252, false)),
        TextEncoding::Auto => match std::str::from_utf8(utf8) {
            Ok(s) => Ok((Cow::Borrowed(s), TextEncoding::Utf8, true)),
            Err(_) => Ok((cp1252(bytes), TextEncoding::Windows1252, true)),
        },
    }
}

fn cp1252(bytes: &[u8]) -> Cow<'_, str> {
    // Windows-1252 maps every byte, so this never fails.
    let (text, _had_errors) = can_dbc::encodings::WINDOWS_1252.decode_without_bom_handling(bytes);
    text
}

/// One top-level statement.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Statement<'a> {
    pub(crate) text: &'a str,
    /// 1-based line of the first line.
    pub(crate) line: u32,
}

impl<'a> Statement<'a> {
    /// Leading keyword (`BO_`, `CM_`, `VERSION`, `//` …).
    pub(crate) fn keyword(&self) -> &'a str {
        let t = self.text.trim_start();
        if t.starts_with("//") {
            return "//";
        }
        let end = t
            .find(|c: char| c.is_whitespace() || c == ':' || c == ';')
            .unwrap_or(t.len());
        &t[..end]
    }

    /// First line, shortened, for warnings.
    pub(crate) fn summary(&self) -> String {
        let first = self.text.lines().next().unwrap_or("").trim();
        let mut s: String = first.chars().take(80).collect();
        if first.chars().count() > 80 {
            s.push('…');
        }
        s
    }
}

/// Iterator over top-level statements. Holds no buffers (C2).
pub(crate) struct Statements<'a> {
    text: &'a str,
    pos: usize,
    line: u32,
}

pub(crate) fn statements(text: &str) -> Statements<'_> {
    Statements {
        text,
        pos: 0,
        line: 0,
    }
}

impl<'a> Iterator for Statements<'a> {
    type Item = Statement<'a>;

    fn next(&mut self) -> Option<Statement<'a>> {
        let mut start: Option<(usize, u32)> = None;
        let mut in_quote = false;
        while self.pos < self.text.len() {
            let rest = &self.text[self.pos..];
            let line_len = rest.find('\n').map_or(rest.len(), |i| i + 1);
            let line = &rest[..line_len];
            let begins = !in_quote && line.chars().next().is_some_and(|c| !c.is_whitespace());
            match start {
                Some((s, l)) if begins => {
                    return Some(Statement {
                        text: &self.text[s..self.pos],
                        line: l,
                    });
                }
                None if !line.trim().is_empty() => start = Some((self.pos, self.line + 1)),
                _ => {}
            }
            in_quote = scan_quotes(line, in_quote);
            self.pos += line_len;
            self.line += 1;
        }
        start.map(|(s, l)| Statement {
            text: &self.text[s..],
            line: l,
        })
    }
}

/// Tracks whether we are inside a quoted string at the end of `line`.
/// `\"` is an escaped quote; `//` outside quotes ends the line.
fn scan_quotes(line: &str, mut in_quote: bool) -> bool {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if in_quote {
            match b[i] {
                b'\\' => i += 1,
                b'"' => in_quote = false,
                _ => {}
            }
        } else {
            match b[i] {
                b'"' => in_quote = true,
                b'/' if b.get(i + 1) == Some(&b'/') => return false,
                _ => {}
            }
        }
        i += 1;
    }
    in_quote
}

/// Whitespace tokens of a statement, with a trailing `:`/`;` split off the token.
fn tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace()
        .map(|t| t.trim_end_matches([':', ';']))
        .filter(|t| !t.is_empty())
}

/// `BO_ <id> <name>:` → (raw id as written, name).
pub(crate) fn bo_header(text: &str) -> Option<(u64, &str)> {
    let mut t = tokens(text);
    if t.next()? != "BO_" {
        return None;
    }
    let id = t.next()?.parse().ok()?;
    let name = t.next()?;
    Some((id, name))
}

/// Message id that a statement refers to, and the attribute name for `BA_`.
/// Used only to judge the effect of a statement that could not be parsed.
pub(crate) fn referenced_message(text: &str) -> Option<(u64, Option<&str>)> {
    let mut t = tokens(text);
    match t.next()? {
        "BO_" | "SG_MUL_VAL_" | "SIG_VALTYPE_" | "BO_TX_BU_" | "VAL_" => {
            Some((t.next()?.parse().ok()?, None))
        }
        "BA_" => {
            let name = t.next()?.trim_matches('"');
            match t.next()? {
                "BO_" | "SG_" => Some((t.next()?.parse().ok()?, Some(name))),
                _ => None,
            }
        }
        "CM_" => match t.next()? {
            "BO_" | "SG_" => Some((t.next()?.parse().ok()?, None)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn splits_blocks_and_multiline_comments() {
        let text = "VERSION \"\"\n\nNS_ :\n\tCM_\n\tBA_\n\nBO_ 1 A: 8 X\n SG_ s : 0|8@1+ (1,0) [0|0] \"\" X\n\nCM_ BO_ 1 \"two\nBO_ lines\";\nBA_ \"x\" BO_ 1 2;\n";
        let st: Vec<_> = statements(text).collect();
        let kw: Vec<_> = st.iter().map(|s| s.keyword()).collect();
        assert_eq!(kw, ["VERSION", "NS_", "BO_", "CM_", "BA_"]);
        assert_eq!(st[2].line, 7);
        assert!(st[2].text.contains("SG_ s"));
        assert!(st[3].text.contains("BO_ lines"));
        assert_eq!(st[4].line, 12);
    }

    #[test]
    fn escaped_quote_and_line_comment() {
        let text = "CM_ \"a \\\" b\";\n// \"not a string\nBO_ 1 A: 1 X\n";
        let kw: Vec<_> = statements(text).map(|s| s.keyword()).collect();
        assert_eq!(kw, ["CM_", "//", "BO_"]);
    }

    #[test]
    fn headers() {
        assert_eq!(bo_header("BO_ 2147483905 Ext: 8 X"), Some((2147483905, "Ext")));
        assert_eq!(bo_header("BO_ 256 Engine : 8 X"), Some((256, "Engine")));
        assert_eq!(
            referenced_message("BA_ \"GenSigStartValue\" SG_ 256 Sig 3;"),
            Some((256, Some("GenSigStartValue")))
        );
        assert_eq!(referenced_message("SG_MUL_VAL_ 100 S M 1-2;"), Some((100, None)));
    }

    #[test]
    fn encodings() {
        let bytes = b"BO_ 1 A: 1 X\n SG_ T : 0|8@1+ (1,0) [0|0] \"\xB0C\" X\n";
        assert!(decode_text(bytes, TextEncoding::Utf8).is_err());
        let (t, used, guessed) = decode_text(bytes, TextEncoding::Auto).unwrap();
        assert!(t.contains("°C"));
        assert_eq!(used, TextEncoding::Windows1252);
        assert!(guessed);
        let (t, _, guessed) = decode_text(bytes, TextEncoding::Windows1252).unwrap();
        assert!(t.contains("°C") && !guessed);
        let bom = b"\xEF\xBB\xBFVERSION \"\"\n";
        assert!(
            decode_text(bom, TextEncoding::Utf8)
                .unwrap()
                .0
                .starts_with("VERSION")
        );
    }
}
