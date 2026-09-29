//! Valve KeyValues (VDF/ACF) text format, as used by Steam's `libraryfolders.vdf` and `appmanifest_*.acf`:
//! `"key" "value"` pairs and `"key" { ... }` blocks, with `\\` and `\"` escapes and `//` comments.

use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Block(Block),
}

/// Keys are compared case-insensitively (Steam's files aren't consistent); they're stored lower-cased.
/// Duplicate keys keep the first value, like Steam does.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Block {
    entries: BTreeMap<String, Value>,
    order: Vec<String>,
}

impl Block {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.get(&key.to_lowercase())
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Text(t) => Some(t),
            Value::Block(_) => None,
        }
    }

    pub fn block(&self, key: &str) -> Option<&Block> {
        match self.get(key)? {
            Value::Block(b) => Some(b),
            Value::Text(_) => None,
        }
    }

    /// Entries in file order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.order
            .iter()
            .filter_map(|k| self.entries.get(k).map(|v| (k.as_str(), v)))
    }

    fn insert(&mut self, key: String, value: Value) {
        let key = key.to_lowercase();
        if !self.entries.contains_key(&key) {
            self.order.push(key.clone());
            self.entries.insert(key, value);
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("unexpected end of file")]
    UnexpectedEnd,
    #[error("unexpected character {0:?} at byte {1}")]
    Unexpected(char, usize),
    #[error("blocks are nested too deeply")]
    TooDeep,
}

const MAX_DEPTH: usize = 32;

/// Parses a whole KeyValues document into its top-level block.
pub fn parse(text: &str) -> Result<Block, ParseError> {
    let mut parser = Parser {
        chars: text.char_indices().peekable(),
    };
    let block = parser.block(0, false)?;
    Ok(block)
}

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
}

impl Parser<'_> {
    fn block(&mut self, depth: usize, nested: bool) -> Result<Block, ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::TooDeep);
        }
        let mut block = Block::default();
        loop {
            self.skip_space_and_comments();
            match self.chars.peek().copied() {
                None if nested => return Err(ParseError::UnexpectedEnd),
                None => return Ok(block),
                Some((_, '}')) if nested => {
                    self.chars.next();
                    return Ok(block);
                }
                Some((i, '}')) => return Err(ParseError::Unexpected('}', i)),
                Some(_) => {
                    let key = self.token()?;
                    self.skip_space_and_comments();
                    match self.chars.peek().copied() {
                        Some((_, '{')) => {
                            self.chars.next();
                            let child = self.block(depth + 1, true)?;
                            block.insert(key, Value::Block(child));
                        }
                        Some(_) => {
                            let value = self.token()?;
                            block.insert(key, Value::Text(value));
                        }
                        None => return Err(ParseError::UnexpectedEnd),
                    }
                }
            }
        }
    }

    /// A quoted string (with escapes) or a bare word.
    fn token(&mut self) -> Result<String, ParseError> {
        let Some((start, first)) = self.chars.next() else {
            return Err(ParseError::UnexpectedEnd);
        };
        let mut out = String::new();
        if first == '"' {
            loop {
                match self.chars.next() {
                    None => return Err(ParseError::UnexpectedEnd),
                    Some((_, '"')) => return Ok(out),
                    Some((_, '\\')) => match self.chars.next() {
                        Some((_, 'n')) => out.push('\n'),
                        Some((_, 't')) => out.push('\t'),
                        Some((_, c)) => out.push(c),
                        None => return Err(ParseError::UnexpectedEnd),
                    },
                    Some((_, c)) => out.push(c),
                }
            }
        }
        if first == '{' || first == '}' {
            return Err(ParseError::Unexpected(first, start));
        }
        out.push(first);
        while let Some((_, c)) = self.chars.peek().copied() {
            if c.is_whitespace() || c == '"' || c == '{' || c == '}' {
                break;
            }
            out.push(c);
            self.chars.next();
        }
        Ok(out)
    }

    fn skip_space_and_comments(&mut self) {
        loop {
            while self.chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                self.chars.next();
            }
            let mut ahead = self.chars.clone();
            if matches!(
                (ahead.next(), ahead.next()),
                (Some((_, '/')), Some((_, '/')))
            ) {
                while self.chars.peek().is_some_and(|(_, c)| *c != '\n') {
                    self.chars.next();
                }
                continue;
            }
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_blocks_and_escapes() {
        let doc = parse(
            r#"// comment
            "libraryfolders"
            {
                "0" { "path" "C:\\Program Files (x86)\\Steam" "label" "" }
                "1" { "path"  "D:\\SteamLibrary" }
            }"#,
        )
        .expect("parses");
        let folders = doc.block("LibraryFolders").expect("case-insensitive");
        assert_eq!(
            folders.block("1").and_then(|b| b.text("path")),
            Some(r"D:\SteamLibrary")
        );
        assert_eq!(folders.iter().count(), 2);
    }

    #[test]
    fn malformed_input_is_an_error() {
        assert_eq!(parse(r#""a" { "b" "c""#), Err(ParseError::UnexpectedEnd));
        assert!(parse(r#""a" "unterminated"#).is_err());
        assert!(parse("}").is_err());
        let deep = "\"a\" {".repeat(40);
        assert_eq!(parse(&deep), Err(ParseError::TooDeep));
    }
}
