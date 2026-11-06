//! Parser for OP.GG's "class notation" replies (SPEC §5.4).
//!
//! ```text
//! class Data: summary,damage_type
//! class Position: name,stats
//!
//! Data(Summary([Position("MID",Stats(0.95))]),"AP")
//! ```
//! Header lines map each class to its field names. The body is `Name(args)`, `[list]`, numbers,
//! `"strings"` and `null|true|false`. Each argument maps to its field by position. The result is
//! a `serde_json::Value` object tree.

use std::collections::HashMap;

use serde_json::{Map, Number, Value};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum NotationError {
    #[error("unexpected end of input")]
    Eof,
    #[error("unexpected character {0:?} at {1}")]
    Unexpected(char, usize),
    #[error("unknown class {0}")]
    UnknownClass(String),
    #[error("class {class} has {expected} fields but got {got} arguments")]
    Arity { class: String, expected: usize, got: usize },
    #[error("invalid number {0:?}")]
    Number(String),
    #[error("trailing input at {0}")]
    Trailing(usize),
}

pub fn parse(text: &str) -> Result<Value, NotationError> {
    let mut classes: HashMap<String, Vec<String>> = HashMap::new();
    let mut body = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("class ") {
            if let Some((name, fields)) = rest.split_once(':') {
                let fields = fields.split(',').map(|f| f.trim().to_owned()).filter(|f| !f.is_empty()).collect();
                classes.insert(name.trim().to_owned(), fields);
            }
        } else if !line.trim().is_empty() {
            body.push_str(line.trim());
        }
    }
    let mut p = Parser { s: body.as_bytes(), pos: 0, classes: &classes };
    let value = p.value()?;
    p.skip_ws();
    if p.pos != p.s.len() {
        return Err(NotationError::Trailing(p.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    classes: &'a HashMap<String, Vec<String>>,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.s.get(self.pos).is_some_and(|c| c.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Result<u8, NotationError> {
        self.skip_ws();
        self.s.get(self.pos).copied().ok_or(NotationError::Eof)
    }

    fn expect(&mut self, c: u8) -> Result<(), NotationError> {
        match self.peek()? {
            got if got == c => {
                self.pos += 1;
                Ok(())
            }
            got => Err(NotationError::Unexpected(got as char, self.pos)),
        }
    }

    fn value(&mut self) -> Result<Value, NotationError> {
        match self.peek()? {
            b'"' => self.string().map(Value::String),
            b'[' => self.list(),
            b'-' | b'0'..=b'9' => self.number(),
            c if c.is_ascii_alphabetic() || c == b'_' => self.word(),
            c => Err(NotationError::Unexpected(c as char, self.pos)),
        }
    }

    /// Comma-separated values up to `close`. The opening bracket is already consumed.
    fn items(&mut self, close: u8) -> Result<Vec<Value>, NotationError> {
        let mut out = Vec::new();
        if self.peek()? == close {
            self.pos += 1;
            return Ok(out);
        }
        loop {
            out.push(self.value()?);
            match self.peek()? {
                b',' => self.pos += 1,
                c if c == close => {
                    self.pos += 1;
                    return Ok(out);
                }
                c => return Err(NotationError::Unexpected(c as char, self.pos)),
            }
        }
    }

    fn list(&mut self) -> Result<Value, NotationError> {
        self.expect(b'[')?;
        Ok(Value::Array(self.items(b']')?))
    }

    fn word(&mut self) -> Result<Value, NotationError> {
        let start = self.pos;
        while self.s.get(self.pos).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            self.pos += 1;
        }
        let word = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or_default().to_owned();
        if self.peek().ok() != Some(b'(') {
            return match word.as_str() {
                "null" | "None" => Ok(Value::Null),
                "true" | "True" => Ok(Value::Bool(true)),
                "false" | "False" => Ok(Value::Bool(false)),
                _ => Err(NotationError::Unexpected(word.chars().next().unwrap_or('?'), start)),
            };
        }
        self.pos += 1; // '('
        let args = self.items(b')')?;
        let fields = self.classes.get(&word).ok_or_else(|| NotationError::UnknownClass(word.clone()))?;
        if fields.len() != args.len() {
            return Err(NotationError::Arity { class: word, expected: fields.len(), got: args.len() });
        }
        let mut obj = Map::new();
        for (field, arg) in fields.iter().zip(args) {
            obj.insert(field.clone(), arg);
        }
        Ok(Value::Object(obj))
    }

    fn number(&mut self) -> Result<Value, NotationError> {
        let start = self.pos;
        while self.s.get(self.pos).is_some_and(|c| c.is_ascii_digit() || b"-+.eE".contains(c)) {
            self.pos += 1;
        }
        let raw = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or_default();
        if let Ok(i) = raw.parse::<i64>() {
            return Ok(Value::Number(i.into()));
        }
        raw.parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| NotationError::Number(raw.to_owned()))
    }

    fn string(&mut self) -> Result<String, NotationError> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = *self.s.get(self.pos).ok_or(NotationError::Eof)?;
            self.pos += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = *self.s.get(self.pos).ok_or(NotationError::Eof)?;
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = self.s.get(self.pos..self.pos + 4).ok_or(NotationError::Eof)?;
                            let code = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or(""), 16)
                                .map_err(|_| NotationError::Unexpected('u', self.pos))?;
                            self.pos += 4;
                            let ch = char::from_u32(code).unwrap_or('\u{fffd}');
                            out.extend_from_slice(ch.to_string().as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                _ => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_nested_classes_lists_and_scalars() {
        let text = "class Data: summary,damage_type,flag,missing\nclass Summary: positions\nclass Position: name,stats\nclass Stats: role_rate\n\n\
                    Data(Summary([Position(\"MID\",Stats(0.95)),Position(\"TOP\",Stats(1))]),\"AP\",true,null)";
        assert_eq!(
            parse(text).unwrap(),
            json!({"summary": {"positions": [
                {"name": "MID", "stats": {"role_rate": 0.95}},
                {"name": "TOP", "stats": {"role_rate": 1}}
            ]}, "damage_type": "AP", "flag": true, "missing": null})
        );
    }

    #[test]
    fn strings_with_escapes_and_negative_numbers() {
        let text = "class A: s,n,e\nA(\"Kai'Sa \\\"x\\\" \\u00e9\",-3,1.5e2)";
        assert_eq!(parse(text).unwrap(), json!({"s": "Kai'Sa \"x\" é", "n": -3, "e": 150.0}));
    }

    #[test]
    fn empty_lists_and_whitespace() {
        assert_eq!(parse("class A: xs, ys\n A( [ ] , [1, 2] )").unwrap(), json!({"xs": [], "ys": [1, 2]}));
    }

    #[test]
    fn errors() {
        assert_eq!(parse("A(1)"), Err(NotationError::UnknownClass("A".into())));
        assert_eq!(
            parse("class A: x,y\nA(1)"),
            Err(NotationError::Arity { class: "A".into(), expected: 2, got: 1 })
        );
        assert_eq!(parse("class A: x\nA(1"), Err(NotationError::Eof));
        assert!(matches!(parse("class A: x\nA(1))"), Err(NotationError::Trailing(_))));
        assert!(matches!(parse("class A: x\nA(bogus)"), Err(NotationError::Unexpected(..))));
    }
}
