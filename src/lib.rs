#![forbid(unsafe_code)]

//! A parser, checker, and interpreter for the C0 programming language.

mod check;
mod eval;
mod heap;
mod host;
mod lexer;
mod libraries;
mod parser;
mod purity;
mod syntax;

use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::Write,
    sync::Arc,
};

pub use host::{
    DebugEvent, DebugEventKind, Image, RuntimeHost, SourceLibrary, SourceResolver,
    TerminalArgument, TerminalResult,
};

use lexer::{Import, TokenKind};
use syntax::{Declaration, Type};

/// A position in one source, with one-based line and column numbers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Location {
    pub line: usize,
    pub column: usize,
    source_id: usize,
}

impl Location {
    fn start(source_id: usize) -> Self {
        Self {
            line: 1,
            column: 1,
            source_id,
        }
    }

    fn error(self, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            location: self,
            source: None,
            message: message.into(),
            kind: DiagnosticKind::Validation,
            call_stack: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticKind {
    Syntax,
    Validation,
    Runtime,
    ProgramError,
    Cancelled,
}

/// A location and function name in the C0 runtime call stack.
#[derive(Clone, Debug)]
pub struct CallSite {
    pub function: String,
    pub source: Option<Arc<str>>,
    pub location: Location,
}

/// A syntax, semantic, or execution error.
#[derive(Debug)]
pub struct Diagnostic {
    pub location: Location,
    pub source: Option<Arc<str>>,
    pub message: String,
    pub kind: DiagnosticKind,
    pub call_stack: Vec<CallSite>,
}

impl Diagnostic {
    fn with_kind(mut self, kind: DiagnosticKind) -> Self {
        self.kind = kind;
        self
    }

    fn in_sources(mut self, sources: &[Arc<str>]) -> Self {
        self.source = sources.get(self.location.source_id).cloned();
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(source) = &self.source {
            write!(formatter, "{source}:")?;
        }
        write!(
            formatter,
            "{}:{}: {}",
            self.location.line, self.location.column, self.message
        )
    }
}

impl std::error::Error for Diagnostic {}

/// A source unit and its stable identity. Imports with the same identity are loaded once.
#[derive(Clone, Debug)]
pub struct Source {
    pub name: Arc<str>,
    pub text: String,
}

impl Source {
    pub fn new(name: impl Into<Arc<str>>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            text: text.into(),
        }
    }
}

/// Options used while checking a parsed program.
#[derive(Clone, Copy, Debug)]
pub struct CheckOptions {
    pub check_purity: bool,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self { check_purity: true }
    }
}

/// Options used while running a checked program.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunOptions {
    pub check_contracts: bool,
}

#[derive(Default)]
struct Loader {
    sources: Vec<Arc<str>>,
    loaded_sources: HashSet<Arc<str>>,
    loaded_libraries: HashSet<String>,
    aliases: HashMap<String, Type>,
    declarations: Vec<Declaration>,
}

/// Parsed C0 source units, in declaration order.
pub struct ParsedProgram {
    sources: Vec<Arc<str>>,
    declarations: Vec<Declaration>,
}

/// Parse named source units in order, resolving leading imports before declarations.
/// Command-line libraries, if any, load before the first source unit.
pub fn parse(
    sources: impl IntoIterator<Item = Source>,
    libraries: &[String],
    resolver: &mut impl SourceResolver,
) -> Result<ParsedProgram, Diagnostic> {
    let mut loader = Loader::default();
    let result = (|| {
        for library in libraries {
            loader.library(library, Location::start(0), resolver)?;
        }
        for source in sources {
            loader.source(source, false, resolver)?;
        }
        Ok(())
    })();
    result.map_err(|error: Diagnostic| error.in_sources(&loader.sources))?;
    Ok(ParsedProgram {
        sources: loader.sources,
        declarations: loader.declarations,
    })
}

impl Loader {
    fn source(
        &mut self,
        source: Source,
        native: bool,
        resolver: &mut impl SourceResolver,
    ) -> Result<(), Diagnostic> {
        if !self.loaded_sources.insert(Arc::clone(&source.name)) {
            return Ok(());
        }
        let source_id = self.sources.len();
        self.sources.push(Arc::clone(&source.name));
        let mut tokens = lexer::lex(&source.text, source_id)
            .map_err(|error| error.with_kind(DiagnosticKind::Syntax))?
            .into_iter()
            .peekable();
        while let Some(token) = tokens.next_if(|token| {
            matches!(
                token.kind,
                TokenKind::Import(_) | TokenKind::IgnoredDirective
            )
        }) {
            match token.kind {
                TokenKind::Import(Import::Source(path)) => {
                    let imported = resolver.source(&source.name, &path).map_err(|error| {
                        token
                            .location
                            .error(format!("could not load '{path}': {error}"))
                    })?;
                    self.source(imported, native, resolver)?;
                }
                TokenKind::Import(Import::Library(name)) => {
                    self.library(&name, token.location, resolver)?;
                }
                TokenKind::IgnoredDirective => {}
                _ => unreachable!("import token"),
            }
        }
        let declarations = parser::parse(tokens, &mut self.aliases, native)
            .map_err(|error| error.with_kind(DiagnosticKind::Syntax))?;
        self.declarations.extend(declarations);
        Ok(())
    }

    fn library(
        &mut self,
        name: &str,
        location: Location,
        resolver: &mut impl SourceResolver,
    ) -> Result<(), Diagnostic> {
        if !self.loaded_libraries.insert(name.to_owned()) {
            return Ok(());
        }
        if let Some(header) = libraries::header(name) {
            return self.source(Source::new(format!("<{name}>"), header), true, resolver);
        }
        let library = resolver
            .library(name)
            .map_err(|error| location.error(format!("could not load library '{name}': {error}")))?;
        self.source(library.header, false, resolver)?;
        if let Some(source) = library.implementation {
            self.source(source, false, resolver)?;
        }
        Ok(())
    }
}

impl ParsedProgram {
    /// Validate the sequenced declarations and produce an executable program.
    pub fn check(&self, options: CheckOptions) -> Result<CheckedProgram<'_>, Diagnostic> {
        let checked = check::validate(&self.declarations, options)
            .map_err(|error| error.in_sources(&self.sources))?;
        Ok(CheckedProgram {
            parsed: self,
            checked,
        })
    }
}

/// A type-checked C0 program, reusable across independent executions.
pub struct CheckedProgram<'a> {
    parsed: &'a ParsedProgram,
    checked: check::Checked<'a>,
}

impl CheckedProgram<'_> {
    /// Call C0's entry point. The host can present the returned integer separately.
    pub fn run(&self, host: &mut impl RuntimeHost, options: RunOptions) -> Result<i32, Diagnostic> {
        eval::run(&self.checked, &self.parsed.sources, host, options)
            .map_err(|error| error.in_sources(&self.parsed.sources))
    }
}

/// Validate and execute a single source string with a write-only console host.
pub fn run(source: &str, output: &mut impl Write) -> Result<i32, Diagnostic> {
    struct Writer<'a, W>(&'a mut W);
    impl<W: Write> RuntimeHost for Writer<'_, W> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<()> {
            self.0.write_all(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    let parsed = parse([Source::new("<source>", source)], &[], &mut ())?;
    parsed
        .check(CheckOptions::default())?
        .run(&mut Writer(output), RunOptions::default())
}
