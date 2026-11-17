use std::{
    io::{self, BufRead},
    ops::ControlFlow,
};

use crate::{Location, Source};

/// A library supplied as C0 source. Its interface is processed first.
pub struct SourceLibrary {
    pub header: Source,
    pub implementation: Option<Source>,
}

/// Resolves source and library imports. Source identities need to be stable,
/// including when a file was already supplied as a top-level compilation unit.
pub trait SourceResolver {
    fn source(&mut self, _from: &str, path: &str) -> io::Result<Source> {
        Err(io::Error::new(io::ErrorKind::NotFound, path.to_owned()))
    }

    fn library(&mut self, name: &str) -> io::Result<SourceLibrary> {
        Err(io::Error::new(io::ErrorKind::NotFound, name.to_owned()))
    }
}

impl SourceResolver for () {}

/// Raw image pixels in row-major order, encoded as 0xAARRGGBB.
#[derive(Clone, Debug)]
pub struct Image {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugEventKind {
    EnterFunction,
    Statement,
    LoopCondition,
    LeaveFunction,
}

#[derive(Clone, Copy, Debug)]
pub struct DebugEvent<'a> {
    pub kind: DebugEventKind,
    pub source: Option<&'a str>,
    pub location: Location,
    pub function: &'a str,
    pub depth: usize,
}

/// Supplies OS and editor services during execution. A host may pause in
/// `event`; returning `ControlFlow::Break` cancels the current execution.
pub trait RuntimeHost {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()>;

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    /// Get the next line of console input without a line ending.
    fn read_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        Err(unsupported("console input"))
    }

    /// Test whether console input has reached EOF without consuming a line.
    fn input_eof(&mut self) -> io::Result<bool> {
        Err(unsupported("console input"))
    }

    /// Open a file for buffered byte reads. The interpreter owns the handle.
    fn open_file(&mut self, _path: &[u8]) -> io::Result<Box<dyn BufRead>> {
        Err(unsupported("file input"))
    }

    /// Program arguments, excluding the executable name.
    fn arguments(&mut self) -> io::Result<Vec<Vec<u8>>> {
        Ok(Vec::new())
    }

    /// Decode a file into ARGB pixels.
    fn load_image(&mut self, _path: &[u8]) -> io::Result<Image> {
        Err(unsupported("image decoding"))
    }

    /// Encode ARGB pixels to the format selected by the host.
    fn save_image(&mut self, _path: &[u8], _image: &Image) -> io::Result<()> {
        Err(unsupported("image encoding"))
    }

    /// Perform a curses operation using the exact C0 function name and argument order.
    fn terminal(
        &mut self,
        _operation: &str,
        _arguments: &[TerminalArgument<'_>],
    ) -> io::Result<TerminalResult> {
        Err(unsupported("curses terminal"))
    }

    fn event(&mut self, _event: DebugEvent<'_>) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }
}

fn unsupported(capability: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{capability} is unavailable"),
    )
}

/// Argument passed to a curses-style terminal operation.
#[derive(Clone, Copy, Debug)]
pub enum TerminalArgument<'a> {
    Int(i32),
    Bool(bool),
    Text(&'a [u8]),
    Window(Option<u64>),
}

/// Result of a terminal operation. Window identifiers refer to host-owned windows.
#[derive(Clone, Copy, Debug)]
pub enum TerminalResult {
    Unit,
    Int(i32),
    Bool(bool),
    Window(Option<u64>),
}
