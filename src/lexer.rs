use crate::{
    Diagnostic, Location,
    syntax::{Literal, Type},
};

#[derive(Debug)]
pub(crate) enum TokenKind {
    Type(Type),
    Literal(Literal),
    Identifier(String),
    Symbol(&'static str),
    Import(Import),
    IgnoredDirective,
    Annotation(Vec<Token>),
    End,
}

#[derive(Debug)]
pub(crate) struct Token {
    pub(crate) location: Location,
    pub(crate) kind: TokenKind,
}

#[derive(Debug)]
pub(crate) enum Import {
    Source(String),
    Library(String),
}

pub(crate) fn lex(source: &str, source_id: usize) -> Result<Vec<Token>, Diagnostic> {
    Lexer {
        remaining: source,
        location: Location::start(source_id),
    }
    .tokens(None)
}

struct Lexer<'a> {
    remaining: &'a str,
    location: Location,
}

impl Lexer<'_> {
    fn peek(&self) -> Option<char> {
        self.remaining.chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.remaining = &self.remaining[ch.len_utf8()..];
        if ch == '\n' {
            self.location.line += 1;
            self.location.column = 1;
        } else {
            self.location.column += 1;
        }
        Some(ch)
    }

    fn consume(&mut self, text: &str) -> bool {
        if !self.remaining.starts_with(text) {
            return false;
        }
        for _ in text.chars() {
            self.advance();
        }
        true
    }

    fn take_while(&mut self, predicate: impl Fn(char) -> bool) -> &str {
        let start = self.remaining;
        while self.peek().is_some_and(&predicate) {
            self.advance();
        }
        &start[..start.len() - self.remaining.len()]
    }

    fn tokens(&mut self, end: Option<&str>) -> Result<Vec<Token>, Diagnostic> {
        let mut tokens = Vec::new();

        while let Some(ch) = self.peek() {
            if end.is_some_and(|end| self.remaining.starts_with(end)) {
                break;
            }
            let location = self.location;
            let kind = match ch {
                '@' if end.is_some() => {
                    self.advance();
                    continue;
                }
                ' ' | '\t' | '\r' | '\n' | '\x0b' | '\x0c' => {
                    self.advance();
                    continue;
                }
                '/' if end.is_none() && self.consume("//@") => {
                    TokenKind::Annotation(self.tokens(Some("\n"))?)
                }
                '/' if end.is_none() && self.consume("/*@") => {
                    let tokens = self.tokens(Some("@*/"))?;
                    if !self.consume("@*/") {
                        return Err(location.error("unterminated contract annotation"));
                    }
                    TokenKind::Annotation(tokens)
                }
                '/' if end.is_none() && self.consume("//") => {
                    self.take_while(|ch| ch != '\n');
                    continue;
                }
                '/' if end.is_none() && self.consume("/*") => {
                    self.block_comment(location)?;
                    continue;
                }
                '#' if end.is_none() => self.directive(location)?,
                '\\' if end.is_some() => self.contract_word(location)?,
                '0'..='9' => self.number(location)?,
                '"' | '\'' => self.literal(ch, location)?,
                'a'..='z' | 'A'..='Z' | '_' => self.word(end.is_some()),
                _ => self.symbol(location)?,
            };
            tokens.push(Token { location, kind });
        }

        tokens.push(Token {
            location: self.location,
            kind: TokenKind::End,
        });
        Ok(tokens)
    }

    fn block_comment(&mut self, location: Location) -> Result<(), Diagnostic> {
        let mut depth = 1;
        while depth > 0 {
            if self.consume("/*") {
                depth += 1;
            } else if self.consume("*/") {
                depth -= 1;
            } else if self.advance().is_none() {
                return Err(location.error("unterminated block comment"));
            }
        }
        Ok(())
    }

    fn directive(&mut self, location: Location) -> Result<TokenKind, Diagnostic> {
        let line = self.take_while(|ch| ch != '\n');
        if line[1..]
            .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .next()
            != Some("use")
        {
            return Ok(TokenKind::IgnoredDirective);
        }
        let argument = line
            .strip_prefix("#use")
            .filter(|rest| rest.starts_with([' ', '\t', '\x0b', '\x0c']))
            .unwrap_or_default()
            .trim_start_matches([' ', '\t', '\x0b', '\x0c']);
        let import = match argument.as_bytes().first() {
            Some(b'<') => argument[1..]
                .split_once('>')
                .filter(|(name, _)| name.bytes().all(|byte| (b' '..=b'~').contains(&byte)))
                .map(|(name, tail)| (Import::Library(name.to_owned()), tail)),
            Some(b'"') => {
                let mut filename = Lexer {
                    remaining: argument,
                    location,
                };
                match filename.literal('"', location)? {
                    TokenKind::Literal(Literal::String(bytes)) => Some((
                        Import::Source(String::from_utf8_lossy(&bytes).into_owned()),
                        filename.remaining,
                    )),
                    _ => unreachable!("string literal"),
                }
            }
            _ => None,
        };
        if let Some((import, tail)) = import
            && (tail.trim().is_empty() || tail.trim_start().starts_with("//"))
        {
            return Ok(TokenKind::Import(import));
        }
        Err(location.error("expected '#use <library>' or '#use \"filename\"'"))
    }

    fn contract_word(&mut self, location: Location) -> Result<TokenKind, Diagnostic> {
        self.advance();
        match self.take_while(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            "length" => Ok(TokenKind::Symbol("\\length")),
            "result" => Ok(TokenKind::Symbol("\\result")),
            word => Err(location.error(format!("unknown contract form '\\{word}'"))),
        }
    }

    fn number(&mut self, location: Location) -> Result<TokenKind, Diagnostic> {
        let (number, bound) = if self.consume("0x") || self.consume("0X") {
            (
                u32::from_str_radix(self.take_while(|ch| ch.is_ascii_hexdigit()), 16),
                u32::MAX,
            )
        } else {
            let digits = self.take_while(|ch| ch.is_ascii_digit());
            if digits.len() > 1 && digits.starts_with('0') {
                return Err(location.error("decimal literals cannot begin with zero"));
            }
            (digits.parse::<u32>(), 1 << 31)
        };

        match number {
            Ok(number) if number <= bound => {
                Ok(TokenKind::Literal(Literal::Int(number.cast_signed())))
            }
            _ => Err(location.error("invalid or out-of-range integer literal")),
        }
    }

    fn literal(&mut self, quote: char, location: Location) -> Result<TokenKind, Diagnostic> {
        self.advance();
        let mut value = String::new();
        loop {
            let element_location = self.location;
            match self.advance() {
                Some(ch) if ch == quote => break,
                Some('\\') => value.push(self.escape(quote, element_location)?),
                Some(ch @ ' '..='~') => value.push(ch),
                _ => {
                    return Err(location.error("unterminated literal or invalid literal character"));
                }
            }
        }

        if quote == '"' {
            Ok(TokenKind::Literal(Literal::String(
                value.into_bytes().into(),
            )))
        } else if value.len() == 1 {
            Ok(TokenKind::Literal(Literal::Char(value.as_bytes()[0])))
        } else {
            Err(location.error("expected one ASCII character"))
        }
    }

    fn escape(&mut self, quote: char, location: Location) -> Result<char, Diagnostic> {
        Ok(match self.advance() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some('v') => '\x0b',
            Some('b') => '\x08',
            Some('r') => '\r',
            Some('f') => '\x0c',
            Some('a') => '\x07',
            Some(ch @ ('\\' | '\'' | '"')) => ch,
            Some('0') if quote == '\'' => '\0',
            _ => return Err(location.error("invalid escape sequence")),
        })
    }

    fn word(&mut self, annotation: bool) -> TokenKind {
        let word = self.take_while(|ch| ch.is_ascii_alphanumeric() || ch == '_');
        match word {
            "int" => TokenKind::Type(Type::Int),
            "bool" => TokenKind::Type(Type::Bool),
            "char" => TokenKind::Type(Type::Char),
            "string" => TokenKind::Type(Type::String),
            "void" => TokenKind::Type(Type::Void),
            "true" => TokenKind::Literal(Literal::Bool(true)),
            "false" => TokenKind::Literal(Literal::Bool(false)),
            "if" => TokenKind::Symbol("if"),
            "else" => TokenKind::Symbol("else"),
            "while" => TokenKind::Symbol("while"),
            "for" => TokenKind::Symbol("for"),
            "return" => TokenKind::Symbol("return"),
            "assert" => TokenKind::Symbol("assert"),
            "requires" if annotation => TokenKind::Symbol("requires"),
            "ensures" if annotation => TokenKind::Symbol("ensures"),
            "loop_invariant" if annotation => TokenKind::Symbol("loop_invariant"),
            "error" => TokenKind::Symbol("error"),
            "break" => TokenKind::Symbol("break"),
            "continue" => TokenKind::Symbol("continue"),
            "struct" => TokenKind::Symbol("struct"),
            "typedef" => TokenKind::Symbol("typedef"),
            "NULL" => TokenKind::Symbol("NULL"),
            "alloc" => TokenKind::Symbol("alloc"),
            "alloc_array" => TokenKind::Symbol("alloc_array"),
            _ => TokenKind::Identifier(word.to_owned()),
        }
    }

    fn symbol(&mut self, location: Location) -> Result<TokenKind, Diagnostic> {
        const SYMBOLS: &[&str] = &[
            "<<=", ">>=", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "++", "--", "+=", "-=",
            "*=", "/=", "%=", "&=", "^=", "|=", "->", "(", ")", "[", "]", "{", "}", ",", ";", ".",
            "!", "~", "+", "-", "*", "/", "%", "&", "^", "|", "?", ":", "=", "<", ">",
        ];
        for &symbol in SYMBOLS {
            if self.consume(symbol) {
                return Ok(TokenKind::Symbol(symbol));
            }
        }
        Err(location.error(format!(
            "unexpected character {:?}",
            self.peek().unwrap_or_default()
        )))
    }
}
