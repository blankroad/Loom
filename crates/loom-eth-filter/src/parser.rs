//! Recursive-descent parser → untyped AST. Grammar (plan §4.1 subset):
//!
//! ```text
//! expr   := and ( ("or" | "||") and )*
//! and    := not ( ("and" | "&&") not )*
//! not    := ("not" | "!") not | "(" expr ")" | test
//! test   := FIELD [ "[" SLICE "]" ] [ relop VALUE | "contains" VALUE | ("matches" | "~") STRING
//!                                    | "in" "{" item ( [","] item )* "}" ]
//! item   := VALUE | VALUE ".." VALUE | WORD-with-".."
//! relop  := == != < <= > >= | eq ne lt le gt ge
//! ```

use crate::lexer::{Tok, Token};
use crate::{FilterError, MAX_DEPTH, MAX_NODES, MAX_SET_ITEMS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Value {
    pub text: String,
    pub quoted: bool,
    pub pos: usize,
    /// Decoded bytes of a quoted string (UTF-8 of `text` for words).
    pub bytes: Vec<u8>,
    /// Raw source between the quotes (regex patterns use this).
    pub raw: String,
    /// Offset of the first unknown escape in a quoted string.
    pub bad_escape: Option<usize>,
}

impl Value {
    pub(crate) fn word(text: String, pos: usize) -> Value {
        Value {
            bytes: text.as_bytes().to_vec(),
            raw: text.clone(),
            text,
            quoted: false,
            pos,
            bad_escape: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ItemAst {
    One(Value),
    Range(Value, Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rel {
    Cmp(Op, Value, usize),
    Contains(Value, usize),
    Matches(Value, usize),
    In(Vec<ItemAst>, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FieldRef {
    pub name: String,
    pub pos: usize,
    pub slice: Option<(String, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ast {
    Or(Vec<Ast>),
    And(Vec<Ast>),
    Not(Box<Ast>),
    Test(FieldRef, Option<Rel>),
}

pub(crate) struct Parser {
    toks: Vec<Token>,
    i: usize,
    depth: usize,
    nodes: usize,
}

fn err(pos: usize, expected: &str, msg: impl Into<String>) -> FilterError {
    FilterError {
        pos,
        expected: expected.into(),
        msg: msg.into(),
    }
}

fn is_kw(w: &str, kw: &str) -> bool {
    w.eq_ignore_ascii_case(kw)
}

const KEYWORDS: &[&str] = &[
    "and", "or", "not", "eq", "ne", "lt", "le", "gt", "ge", "contains", "matches", "in",
];

impl Parser {
    pub(crate) fn new(toks: Vec<Token>) -> Parser {
        Parser {
            toks,
            i: 0,
            depth: 0,
            nodes: 0,
        }
    }

    fn peek(&self) -> &Token {
        // lex() always appends Eof, so the last token exists.
        self.toks.get(self.i).or_else(|| self.toks.last()).unwrap_or(&EOF)
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if self.i < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn node(&mut self, pos: usize) -> Result<(), FilterError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(err(
                pos,
                "a smaller expression",
                format!("more than {MAX_NODES} nodes"),
            ));
        }
        Ok(())
    }

    fn enter(&mut self, pos: usize) -> Result<(), FilterError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(err(
                pos,
                "less nesting",
                format!("nesting deeper than {MAX_DEPTH}"),
            ));
        }
        Ok(())
    }

    pub(crate) fn parse(mut self) -> Result<Ast, FilterError> {
        let first = self.peek().clone();
        if first.tok == Tok::Eof {
            return Err(err(first.pos, "a field name", "empty filter"));
        }
        let ast = self.expr()?;
        let t = self.peek().clone();
        if t.tok != Tok::Eof {
            return Err(err(t.pos, "`and`, `or` or end of filter", "unexpected token"));
        }
        Ok(ast)
    }

    fn expr(&mut self) -> Result<Ast, FilterError> {
        let pos = self.peek().pos;
        self.enter(pos)?;
        let mut parts = vec![self.and()?];
        loop {
            let t = self.peek().clone();
            let is_or = matches!(&t.tok, Tok::OrOr) || matches!(&t.tok, Tok::Word(w) if is_kw(w, "or"));
            if !is_or {
                break;
            }
            self.bump();
            parts.push(self.and()?);
        }
        self.depth -= 1;
        if parts.len() == 1 {
            return Ok(parts.remove(0));
        }
        self.node(pos)?;
        Ok(Ast::Or(parts))
    }

    fn and(&mut self) -> Result<Ast, FilterError> {
        let pos = self.peek().pos;
        let mut parts = vec![self.not()?];
        loop {
            let t = self.peek().clone();
            let is_and = matches!(&t.tok, Tok::AndAnd) || matches!(&t.tok, Tok::Word(w) if is_kw(w, "and"));
            if !is_and {
                break;
            }
            self.bump();
            parts.push(self.not()?);
        }
        if parts.len() == 1 {
            return Ok(parts.remove(0));
        }
        self.node(pos)?;
        Ok(Ast::And(parts))
    }

    fn not(&mut self) -> Result<Ast, FilterError> {
        let t = self.peek().clone();
        let is_not = matches!(&t.tok, Tok::Bang) || matches!(&t.tok, Tok::Word(w) if is_kw(w, "not"));
        if is_not {
            self.bump();
            self.enter(t.pos)?;
            let inner = self.not()?;
            self.depth -= 1;
            self.node(t.pos)?;
            return Ok(Ast::Not(Box::new(inner)));
        }
        if t.tok == Tok::LParen {
            self.bump();
            let inner = self.expr()?;
            let close = self.bump();
            if close.tok != Tok::RParen {
                return Err(err(close.pos, ")", "missing closing parenthesis"));
            }
            return Ok(inner);
        }
        self.test()
    }

    fn test(&mut self) -> Result<Ast, FilterError> {
        let t = self.bump();
        let name = match t.tok {
            Tok::Word(w) => w,
            Tok::Eof => return Err(err(t.pos, "a field name", "filter ends early")),
            Tok::Str { .. } => return Err(err(t.pos, "a field name", "a literal cannot start a test")),
            _ => return Err(err(t.pos, "a field name", "unexpected token")),
        };
        if KEYWORDS.iter().any(|k| is_kw(&name, k)) {
            return Err(err(
                t.pos,
                "a field name",
                format!("keyword `{name}` where a field was expected"),
            ));
        }
        if is_kw(&name, "any") || is_kw(&name, "all") {
            return Err(err(
                t.pos,
                "a field name",
                "`any`/`all` quantifiers are not supported",
            ));
        }
        self.node(t.pos)?;
        if self.peek().tok == Tok::LParen {
            return Err(err(self.peek().pos, "an operator", "functions are not supported"));
        }
        let mut field = FieldRef {
            name,
            pos: t.pos,
            slice: None,
        };
        if self.peek().tok == Tok::LBrack {
            let open = self.bump();
            let spec = self.bump();
            let text = match spec.tok {
                Tok::Word(w) => w,
                _ => return Err(err(spec.pos, "a slice like 0:3, 2-4 or 5", "bad slice")),
            };
            let close = self.bump();
            if close.tok != Tok::RBrack {
                return Err(err(close.pos, "]", "missing closing bracket"));
            }
            field.slice = Some((text, open.pos));
        }
        let rt = self.peek().clone();
        let op = match &rt.tok {
            Tok::Eq => Some(Op::Eq),
            Tok::Ne => Some(Op::Ne),
            Tok::Lt => Some(Op::Lt),
            Tok::Le => Some(Op::Le),
            Tok::Gt => Some(Op::Gt),
            Tok::Ge => Some(Op::Ge),
            Tok::Word(w) if is_kw(w, "eq") => Some(Op::Eq),
            Tok::Word(w) if is_kw(w, "ne") => Some(Op::Ne),
            Tok::Word(w) if is_kw(w, "lt") => Some(Op::Lt),
            Tok::Word(w) if is_kw(w, "le") => Some(Op::Le),
            Tok::Word(w) if is_kw(w, "gt") => Some(Op::Gt),
            Tok::Word(w) if is_kw(w, "ge") => Some(Op::Ge),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            let v = self.value()?;
            return Ok(Ast::Test(field, Some(Rel::Cmp(op, v, rt.pos))));
        }
        match &rt.tok {
            Tok::Word(w) if is_kw(w, "contains") => {
                self.bump();
                let v = self.value()?;
                Ok(Ast::Test(field, Some(Rel::Contains(v, rt.pos))))
            }
            Tok::Word(w) if is_kw(w, "matches") => {
                self.bump();
                let v = self.value()?;
                Ok(Ast::Test(field, Some(Rel::Matches(v, rt.pos))))
            }
            Tok::Tilde => {
                self.bump();
                let v = self.value()?;
                Ok(Ast::Test(field, Some(Rel::Matches(v, rt.pos))))
            }
            Tok::Word(w) if is_kw(w, "in") => {
                self.bump();
                let items = self.set()?;
                Ok(Ast::Test(field, Some(Rel::In(items, rt.pos))))
            }
            Tok::LBrace => Err(err(rt.pos, "an operator", "`{` without `in`")),
            _ => Ok(Ast::Test(field, None)),
        }
    }

    fn value(&mut self) -> Result<Value, FilterError> {
        let t = self.bump();
        match t.tok {
            Tok::Word(w) => {
                if self.peek().tok == Tok::LParen {
                    return Err(err(self.peek().pos, "a literal", "functions are not supported"));
                }
                if self.peek().tok == Tok::LBrack {
                    return Err(err(t.pos, "a literal", "comparing two fields is not supported"));
                }
                Ok(Value::word(w, t.pos))
            }
            Tok::Str {
                bytes,
                raw,
                bad_escape,
            } => Ok(Value {
                text: String::from_utf8_lossy(&bytes).into_owned(),
                quoted: true,
                pos: t.pos,
                bytes,
                raw,
                bad_escape,
            }),
            Tok::Eof => Err(err(t.pos, "a value", "filter ends after operator")),
            _ => Err(err(t.pos, "a value", "unexpected token")),
        }
    }

    fn set(&mut self) -> Result<Vec<ItemAst>, FilterError> {
        let open = self.bump();
        if open.tok != Tok::LBrace {
            return Err(err(open.pos, "{", "`in` needs a set"));
        }
        let mut items = Vec::new();
        let mut after_comma = false;
        loop {
            let t = self.peek().clone();
            match t.tok {
                Tok::RBrace if !after_comma => {
                    self.bump();
                    break;
                }
                Tok::Comma if !items.is_empty() && !after_comma => {
                    self.bump();
                    after_comma = true;
                    continue;
                }
                Tok::Eof => return Err(err(t.pos, "}", "unterminated set")),
                _ => {}
            }
            if items.len() >= MAX_SET_ITEMS {
                return Err(err(t.pos, "}", format!("more than {MAX_SET_ITEMS} set items")));
            }
            let v = self.value()?;
            after_comma = false;
            self.node(v.pos)?;
            // `a .. b` with spaces, or `a..b` as one word.
            if matches!(&self.peek().tok, Tok::Word(w) if w == "..") {
                self.bump();
                let hi = self.value()?;
                items.push(ItemAst::Range(v, hi));
                continue;
            }
            if !v.quoted
                && let Some((a, b)) = v.text.split_once("..")
                && !a.is_empty()
                && !b.is_empty()
            {
                let lo = Value::word(a.to_string(), v.pos);
                let hi = Value::word(b.to_string(), v.pos + a.len() + 2);
                items.push(ItemAst::Range(lo, hi));
                continue;
            }
            items.push(ItemAst::One(v));
        }
        if items.is_empty() {
            return Err(err(open.pos, "at least one item", "empty set"));
        }
        Ok(items)
    }
}

static EOF: Token = Token {
    tok: Tok::Eof,
    pos: 0,
};
