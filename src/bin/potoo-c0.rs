#![forbid(unsafe_code)]

#[path = "potoo-c0/image.rs"]
mod image;

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use potoo_c0_interpreter::{
    CheckOptions, DiagnosticKind, Image, RunOptions, RuntimeHost, Source, SourceLibrary,
    SourceResolver, parse,
};

const USAGE: &str =
    "Usage: potoo-c0 [-d] [--no-purity-check] [-l LIBRARY] [-L DIRECTORY] FILE... [-- ARG...]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut options = RunOptions::default();
    let mut check = CheckOptions::default();
    let mut resolver = Files {
        library_paths: Vec::new(),
    };
    let mut libraries = Vec::new();
    let mut files = Vec::new();
    let mut program_arguments = Vec::new();
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("-h" | "--help") => {
                println!("{USAGE}");
                return Ok(());
            }
            Some("-d") => options.check_contracts = true,
            Some("--no-purity-check") => check.check_purity = false,
            Some("-l") => {
                let name = arguments
                    .next()
                    .and_then(|argument| argument.into_string().ok())
                    .ok_or("missing library name after -l")?;
                libraries.push(name);
            }
            Some("-L") => resolver.library_paths.push(PathBuf::from(
                arguments.next().ok_or("missing directory after -L")?,
            )),
            Some("--") => {
                program_arguments.extend(arguments.map(os_bytes));
                break;
            }
            Some(option) if option.starts_with("-l") => libraries.push(option[2..].to_owned()),
            Some(option) if option.starts_with("-L") => {
                resolver.library_paths.push(PathBuf::from(&option[2..]))
            }
            Some(option) if option.starts_with('-') => {
                return Err(format!("unknown option {option}\n{USAGE}"));
            }
            _ => files.push(PathBuf::from(argument)),
        }
    }
    if files.is_empty() {
        return Err(USAGE.to_owned());
    }
    let sources = files
        .iter()
        .map(|path| read_source(path).map_err(|error| format!("{}: {error}", path.display())))
        .collect::<Result<Vec<_>, _>>()?;
    let program = parse(sources, &libraries, &mut resolver).map_err(|error| error.to_string())?;
    let checked = program.check(check).map_err(|error| error.to_string())?;

    let mut host = Console {
        input: io::stdin().lock(),
        output: io::stdout().lock(),
        arguments: program_arguments,
    };
    match checked.run(&mut host, options) {
        Ok(result) => writeln!(host.output, "{result}")
            .and_then(|()| host.output.flush())
            .map_err(|error| format!("console output failed: {error}")),
        Err(error) => {
            let _ = host.flush();
            if error.kind == DiagnosticKind::ProgramError {
                return Err(error.message);
            }
            let mut message = error.to_string();
            for frame in error.call_stack {
                use std::fmt::Write;
                let source = frame.source.as_deref().unwrap_or("<source>");
                let _ = write!(
                    message,
                    "\n  {} ({source}:{}:{})",
                    frame.function, frame.location.line, frame.location.column
                );
            }
            Err(message)
        }
    }
}

fn read_source(path: &Path) -> io::Result<Source> {
    let path = path.canonicalize()?;
    let text = fs::read_to_string(&path)?;
    Ok(Source::from_path(path, text))
}

struct Files {
    library_paths: Vec<PathBuf>,
}

impl SourceResolver for Files {
    fn source_from(&mut self, from: &Source, path: &str) -> io::Result<Source> {
        let from = from
            .path()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        read_source(&from.join(path))
    }

    fn library(&mut self, name: &str) -> io::Result<SourceLibrary> {
        for directory in &self.library_paths {
            let header = match read_source(&directory.join(format!("{name}.h0"))) {
                Ok(header) => header,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let implementation = match read_source(&directory.join(format!("{name}.c0"))) {
                Ok(source) => Some(source),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            return Ok(SourceLibrary {
                header,
                implementation,
            });
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no source library named '{name}'"),
        ))
    }
}

struct Console<I, O> {
    input: I,
    output: O,
    arguments: Vec<Vec<u8>>,
}

impl<I: BufRead, O: Write> RuntimeHost for Console<I, O> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.output.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }

    fn read_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        if self.input.read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }

    fn input_eof(&mut self) -> io::Result<bool> {
        self.output.flush()?;
        Ok(self.input.fill_buf()?.is_empty())
    }

    fn open_file(&mut self, path: &[u8]) -> io::Result<Box<dyn BufRead>> {
        Ok(Box::new(BufReader::new(fs::File::open(os_path(path)?)?)))
    }

    fn load_image(&mut self, path: &[u8]) -> io::Result<Image> {
        image::load(os_path(path)?)
    }

    fn save_image(&mut self, path: &[u8], pixels: &Image) -> io::Result<()> {
        image::save(os_path(path)?, pixels)
    }

    fn arguments(&mut self) -> io::Result<Vec<Vec<u8>>> {
        Ok(self.arguments.clone())
    }
}

#[cfg(unix)]
fn os_path(bytes: &[u8]) -> io::Result<&Path> {
    use std::os::unix::ffi::OsStrExt;
    Ok(Path::new(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn os_path(bytes: &[u8]) -> io::Result<&Path> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(Path::new(OsStr::new(text)))
}

#[cfg(unix)]
fn os_bytes(string: OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStringExt;
    string.into_vec()
}

#[cfg(not(unix))]
fn os_bytes(string: OsString) -> Vec<u8> {
    string.to_string_lossy().into_owned().into_bytes()
}
