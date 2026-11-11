//! Tokenizer. Every token carries its byte offset so errors point at the input.

use crate::FilterError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Tok {
    /// Unquoted run of `[A-Za-z0-9_.:/-]`: field names, keywords, numbers, addresses, byte strings.
    Word(String),
    /// Quoted string: decoded bytes, raw source text (for regex), first unknown escape offset.
    Str {
        bytes: Vec<u8>,
        raw: String,
        bad_escape: Option<usize>,
    },
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    Comma,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Tilde,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub tok: Tok,
    pub pos: usize,
}

fn word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-' | '/')
}

fn err(pos: usize, expected: &str, msg: impl Into<String>) -> FilterError {
    FilterError {
        pos,
        expected: expected.into(),
        msg: msg.into(),
    }
}

pub(crate) fn lex(src: &str) -> Result<Vec<Token>, FilterError> {
    let mut out = Vec::new();
    let mut it = src.char_indices().peekable();
    while let Some(&(pos, c)) = it.peek() {
        if c.is_whitespace() {
            it.next();
            continue;
        }
        let two = src.get(pos..pos + 2).unwrap_or("");
        let three = src.get(pos..pos + 3).unwrap_or("");
        let simple = |t: Tok| Token { tok: t, pos };
        if three == "===" || three == "!==" {
            return Err(err(
                pos,
                "== or !=",
                "`===` and `!==` are not supported; `!=` means not (==)",
            ));
        }
        let (tok, n) = match two {
            "==" => (Some(Tok::Eq), 2),
            "!=" => (Some(Tok::Ne), 2),
            "<=" => (Some(Tok::Le), 2),
            ">=" => (Some(Tok::Ge), 2),
            "&&" => (Some(Tok::AndAnd), 2),
            "||" => (Some(Tok::OrOr), 2),
            "${" => return Err(err(pos, "a field or literal", "macros are not supported")),
            _ => (None, 0),
        };
        if let Some(t) = tok {
            out.push(simple(t));
            for _ in 0..n {
                it.next();
            }
            continue;
        }
        let single = match c {
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            '{' => Some(Tok::LBrace),
            '}' => Some(Tok::RBrace),
            '[' => Some(Tok::LBrack),
            ']' => Some(Tok::RBrack),
            ',' => Some(Tok::Comma),
            '<' => Some(Tok::Lt),
            '>' => Some(Tok::Gt),
            '!' => Some(Tok::Bang),
            '~' => Some(Tok::Tilde),
            _ => None,
        };
        if let Some(t) = single {
            out.push(simple(t));
            it.next();
            continue;
        }
        match c {
            '"' => {
                it.next();
                let start = pos + 1;
                let mut bytes: Vec<u8> = Vec::new();
                let mut bad_escape = None;
                let mut end = None;
                while let Some((p, ch)) = it.next() {
                    match ch {
                        '"' => {
                            end = Some(p);
                            break;
                        }
                        '\\' => {
                            let Some((_, e)) = it.next() else { break };
                            match e {
                                '"' => bytes.push(b'"'),
                                '\\' => bytes.push(b'\\'),
                                'n' => bytes.push(b'\n'),
                                't' => bytes.push(b'\t'),
                                'r' => bytes.push(b'\r'),
                                '0' => bytes.push(0),
                                'x' => {
                                    let hi = it.next().and_then(|(_, h)| h.to_digit(16));
                                    let lo = it.next().and_then(|(_, l)| l.to_digit(16));
                                    match (hi, lo) {
                                        (Some(h), Some(l)) => {
                                            bytes.push(u8::try_from(h * 16 + l).unwrap_or(0))
                                        }
                                        _ => {
                                            bad_escape.get_or_insert(p);
                                        }
                                    }
                                }
                                other => {
                                    // Kept for regex use (`\d`, `\.`); an error for plain literals.
                                    bad_escape.get_or_insert(p);
                                    bytes.push(b'\\');
                                    let mut buf = [0u8; 4];
                                    bytes.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                                }
                            }
                        }
                        _ => {
                            let mut buf = [0u8; 4];
                            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                    }
                }
                let Some(end) = end else {
                    return Err(err(pos, "closing \"", "unterminated string"));
                };
                let raw = src.get(start..end).unwrap_or("").to_string();
                out.push(Token {
                    tok: Tok::Str {
                        bytes,
                        raw,
                        bad_escape,
                    },
                    pos,
                });
            }
            '#' => return Err(err(pos, "an operator", "layer operator `#` is not supported")),
            '&' | '|' => return Err(err(pos, "&& or ||", "bitwise operators are not supported")),
            '+' | '*' | '%' | '^' => return Err(err(pos, "an operator", "arithmetic is not supported")),
            '=' => return Err(err(pos, "==", "single `=`; use `==`")),
            '$' | '@' => {
                return Err(err(
                    pos,
                    "a field or literal",
                    "macros and raw field references are not supported",
                ));
            }
            '\'' => {
                return Err(err(
                    pos,
                    "a quoted string \"…\"",
                    "character literals are not supported",
                ));
            }
            ch if word_char(ch) => {
                let mut s = String::new();
                while let Some(&(_, ch)) = it.peek() {
                    if !word_char(ch) {
                        break;
                    }
                    s.push(ch);
                    it.next();
                }
                out.push(Token {
                    tok: Tok::Word(s),
                    pos,
                });
            }
            other => {
                return Err(err(
                    pos,
                    "a field, literal or operator",
                    format!("unexpected character `{other}`"),
                ));
            }
        }
    }
    out.push(Token {
        tok: Tok::Eof,
        pos: src.len(),
    });
    Ok(out)
}
