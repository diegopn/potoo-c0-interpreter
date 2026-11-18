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
    path::{Path, PathBuf},
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

/// A source unit with a diagnostic name and stable identity for import deduplication.
#[derive(Clone, Debug)]
pub struct Source {
    pub name: Arc<str>,
    pub text: String,
    identity: SourceIdentity,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum SourceIdentity {
    Named(Arc<str>),
    File(PathBuf),
    Builtin(String),
}

impl Source {
    /// Create a virtual source whose name identifies it to the resolver.
    pub fn new(name: impl Into<Arc<str>>, text: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            identity: SourceIdentity::Named(Arc::clone(&name)),
            name,
            text: text.into(),
        }
    }

    /// Create a file source using a stable path, typically a canonical absolute path.
    pub fn from_path(path: PathBuf, text: impl Into<String>) -> Self {
        Self {
            name: Arc::from(path.to_string_lossy().as_ref()),
            text: text.into(),
            identity: SourceIdentity::File(path),
        }
    }

    /// Get the original path, including any bytes outside UTF-8.
    pub fn path(&self) -> Option<&Path> {
        match &self.identity {
            SourceIdentity::File(path) => Some(path),
            _ => None,
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceOrigin {
    Program,
    Builtin,
    LibraryHeader(Arc<str>),
    LibraryImplementation(Arc<str>),
}

impl SourceOrigin {
    fn library(&self) -> Option<&str> {
        match self {
            Self::LibraryHeader(name) | Self::LibraryImplementation(name) => Some(name),
            _ => None,
        }
    }
}

struct SourceOrigins {
    routes: Vec<SourceOrigin>,
    entries: Vec<SourceOrigin>,
    library_imports: Vec<String>,
}

impl SourceOrigins {
    fn declaring_roles(&self) -> &[SourceOrigin] {
        if self.entries.is_empty() {
            &self.routes
        } else {
            &self.entries
        }
    }

    fn has_header_route(&self) -> bool {
        self.routes.iter().any(|origin| {
            matches!(
                origin,
                SourceOrigin::Builtin | SourceOrigin::LibraryHeader(_)
            )
        })
    }

    fn belongs_to(&self, owner: &str) -> bool {
        let mut libraries = self
            .declaring_roles()
            .iter()
            .filter_map(SourceOrigin::library);
        libraries.next() == Some(owner) && libraries.all(|library| library == owner)
    }
}

#[derive(Default)]
struct Loader {
    sources: Vec<Arc<str>>,
    origins: Vec<SourceOrigins>,
    include_edges: Vec<Vec<usize>>,
    loaded_sources: HashMap<SourceIdentity, usize>,
    loaded_libraries: HashSet<String>,
    aliases: HashMap<String, Type>,
    declarations: Vec<Declaration>,
}

/// Parsed C0 source units, in declaration order.
pub struct ParsedProgram {
    sources: Vec<Arc<str>>,
    origins: Vec<SourceOrigins>,
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
            loader.source(source, SourceOrigin::Program, resolver)?;
        }
        Ok(())
    })();
    result.map_err(|error: Diagnostic| error.in_sources(&loader.sources))?;
    Ok(ParsedProgram {
        sources: loader.sources,
        origins: loader.origins,
        declarations: loader.declarations,
    })
}

impl Loader {
    fn source(
        &mut self,
        source: Source,
        origin: SourceOrigin,
        resolver: &mut impl SourceResolver,
    ) -> Result<usize, Diagnostic> {
        if let Some(&source_id) = self.loaded_sources.get(&source.identity) {
            self.assign_origin(source_id, &origin);
            return Ok(source_id);
        }
        let source_id = self.sources.len();
        self.loaded_sources
            .insert(source.identity.clone(), source_id);
        self.sources.push(Arc::clone(&source.name));
        self.origins.push(SourceOrigins {
            routes: vec![origin],
            entries: Vec::new(),
            library_imports: Vec::new(),
        });
        self.include_edges.push(Vec::new());
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
                    let imported = resolver.source_from(&source, &path).map_err(|error| {
                        token
                            .location
                            .error(format!("could not load '{path}': {error}"))
                    })?;
                    let target = self.source(
                        imported,
                        self.origins[source_id].routes[0].clone(),
                        resolver,
                    )?;
                    self.include_edges[source_id].push(target);
                    for origin in self.origins[source_id].routes.clone() {
                        self.assign_origin(target, &origin);
                    }
                }
                TokenKind::Import(Import::Library(name)) => {
                    self.library(&name, token.location, resolver)?;
                    self.origins[source_id].library_imports.push(name);
                }
                TokenKind::IgnoredDirective => {}
                _ => unreachable!("import token"),
            }
        }
        let native = self.origins[source_id]
            .routes
            .contains(&SourceOrigin::Builtin);
        let declarations = parser::parse(tokens, &mut self.aliases, native)
            .map_err(|error| error.with_kind(DiagnosticKind::Syntax))?;
        self.declarations.extend(declarations);
        Ok(source_id)
    }

    fn assign_origin(&mut self, source_id: usize, origin: &SourceOrigin) {
        // A file can already have been loaded through a root or a shared include.
        let mut pending = vec![source_id];
        while let Some(source_id) = pending.pop() {
            if !self.origins[source_id].routes.contains(origin) {
                self.origins[source_id].routes.push(origin.clone());
                pending.extend_from_slice(&self.include_edges[source_id]);
            }
        }
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
            let source = Source {
                name: Arc::from(format!("<{name}>")),
                text: header.to_owned(),
                identity: SourceIdentity::Builtin(name.to_owned()),
            };
            self.source(source, SourceOrigin::Builtin, resolver)?;
            return Ok(());
        }
        let library = resolver
            .library(name)
            .map_err(|error| location.error(format!("could not load library '{name}': {error}")))?;
        let name: Arc<str> = Arc::from(name);
        let origin = SourceOrigin::LibraryHeader(Arc::clone(&name));
        let header_id = self.source(library.header, origin.clone(), resolver)?;
        self.origins[header_id].entries.push(origin);
        if let Some(source) = library.implementation {
            let origin = SourceOrigin::LibraryImplementation(name);
            let implementation_id = self.source(source, origin.clone(), resolver)?;
            self.origins[implementation_id].entries.push(origin);
        }
        Ok(())
    }
}

impl ParsedProgram {
    /// Validate the sequenced declarations and produce an executable program.
    pub fn check(&self, options: CheckOptions) -> Result<CheckedProgram<'_>, Diagnostic> {
        let checked = check::validate(&self.declarations, &self.origins, options)
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
