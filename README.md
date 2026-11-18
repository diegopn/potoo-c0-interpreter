<h1 align="center">C0 Interpreter</h1>

<p align="center">A Rust library and command-line interpreter for the C0 programming language.</p>

<p align="center">
  <a href="https://diegopn.github.io/potoo-site/">Website</a> ·
  <a href="https://diegopn.github.io/potoo-site/en/manual/c0/">Potoo IDE manual</a>
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white"></a>
  <a href="https://doc.rust-lang.org/cargo/"><img alt="Cargo" src="https://img.shields.io/badge/Cargo-DEA584?style=for-the-badge&logo=cargo&logoColor=white"></a>
  <img alt="Rust library" src="https://img.shields.io/badge/Rust_library-4A86CF?style=for-the-badge">
  <img alt="CLI" src="https://img.shields.io/badge/CLI-6A4C93?style=for-the-badge">
</p>

C0 is a programming language and a safe subset of C, not a pseudolanguage. It is designed for learning imperative programming. This interpreter targets the C0.2 language described in the reference dated 18 February 2025, compiler revision 707. The language includes checked memory access, 32-bit modular arithmetic, and contracts. [C0 Reference](https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf)

## Quick start

Save this program as `hello.c0`:

```c
int main() {
    return 42;
}
```

Run it with:

```sh
potoo-c0 hello.c0
```

The command prints the integer returned by `main`:

```text
42
```

## Installation

### Install from source

Install Rust 1.93 or later, then install the CLI from the `develop` branch:

```sh
cargo install --git https://github.com/diegopn/potoo-c0-interpreter.git --branch develop --locked --bin potoo-c0
```

Alternatively, clone the repository and install the checked-out source:

```sh
git clone --branch develop https://github.com/diegopn/potoo-c0-interpreter.git
cd potoo-c0-interpreter
cargo install --path . --locked --bin potoo-c0
```

Cargo places the installed executable in its configured binary directory. Include that directory in your `PATH` to run `potoo-c0` from any folder. The CLI's image file support uses FFmpeg; the remaining language features do not require image decoding or encoding.

## Command line

```text
potoo-c0 [-d] [--no-purity-check] [-l LIBRARY] [-L DIRECTORY] FILE... [-- ARG...]
```

Source files share one compilation unit in the order supplied. A leading `#use "file.c0"` resolves relative to the importing file. A leading `#use <library>` loads a built-in library or searches the supplied `-L` directories for a C0 header and implementation. A library header can publish a concrete struct layout. Private layouts remain visible to their owning library even when other libraries share the same source files. The CLI uses canonical filesystem paths to identify files, including paths that contain bytes outside UTF-8.

The executable invokes `int main()`, prints the returned integer, and exits successfully on a normal return. Runtime diagnostics include C0 locations and the call stack. The `-d` option enables dynamic contract checks. Ordinary `assert` statements run in either mode.

- `-d` checks function contracts and loop invariants during execution.
- `--no-purity-check` disables the interpreter's conservative purity analysis.
- `-l LIBRARY` loads a library before the source files. Use `-L DIRECTORY` to add a directory containing external `.h0` and optional `.c0` library files.
- `-- ARG...` passes the remaining values to the program; use `#use <args>` to read them.
- `--help` prints the command syntax and available options.

For example, a program can use the built-in `conio` library for console output:

```c
#use <conio>

int main() {
    println("Hello, C0!");
    return 0;
}
```

Run it with `potoo-c0 hello.c0`. It writes `Hello, C0!` and then prints `0`, the value returned by `main`. Source and runtime failures are written to standard error with source locations when available, and the CLI exits unsuccessfully.

## Rust library

Add the crate to a Rust application's `Cargo.toml`:

```toml
[dependencies]
potoo-c0-interpreter = { git = "https://github.com/diegopn/potoo-c0-interpreter.git", branch = "develop" }
```

Use `run` for a source program that only needs console output:

```rust
use potoo_c0_interpreter::run;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = run("int main() { return 42; }", &mut Vec::new())?;
    assert_eq!(result, 42);
    Ok(())
}
```

For imports, custom console input, file access, images, or terminal services, use `parse` with named sources and a `SourceResolver`, then call `ParsedProgram::check` and `CheckedProgram::run`. The run method returns the integer from `main`. Use `Source::from_path` to preserve file identity and `SourceResolver::source_from` to resolve relative includes against the original path. Implement `RuntimeHost` to supply console streams, program arguments, file reads, image codecs, and terminal operations. Source-position events let the host pause or cancel execution.

The built-in interfaces cover `conio`, `string`, `parse`, `file`, `args`, `img`, `rand`, `util`, `curses`, `fpt`, and `dub`. Loading the format forms `printf` and `format` reserves those names against ordinary function declarations in the compilation unit. The integer parsers require at least one valid digit after any sign and hexadecimal prefix. [C0 Library Documentation](https://www.cs.cmu.edu/~wlovas/15122-r11/misc/c0-libraries.pdf)

## Runtime limits

- The evaluator uses the Rust call stack and limits execution to 128 active C0 calls.
- The CLI uses FFmpeg for image file decoding and encoding. The supported file formats depend on the installed build. Embedding applications provide their own image codecs.
- Curses programs require a host that implements the terminal interface. The bundled CLI supplies line-oriented console I/O.
- Console lines, file lines, and program arguments must be ASCII. The first zero byte ends the text. Bytes outside the ASCII range in the resulting text produce a runtime diagnostic.
- Third-party libraries that use the original compiler's native C ABI need a C0 implementation to load here.
- The interpreter checks purity with its own conservative analysis of writes and function calls. Use `--no-purity-check` to disable that analysis while keeping ordinary type and initialization checks.

## References and license

- [C0 Reference, compiler revision 707](https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf)
- [CC0 User’s Guide, compiler revision 712](https://c0.cs.cmu.edu/docs/cc0-users-guide.pdf)
- [CC0 compiler interface and program execution](https://www.cs.cmu.edu/afs/cs/academic/class/15122-n14/www/recitations/rec1/getting_started.html)
- [C0 Library Documentation, 12 January 2011](https://www.cs.cmu.edu/~wlovas/15122-r11/misc/c0-libraries.pdf)
- [Formatted output and runtime updates](https://ishantheperson.github.io/posts/c0-2/)

This independent implementation is licensed under [GPL-3.0-or-later](LICENSE). The [original implementation](https://bitbucket.org/c0-lang/c0) has a separate distribution.

## Support

If this interpreter is useful in your work, you can support its continued development:

<p align="left">
  <a href="https://github.com/sponsors/diegopn"><img alt="Become a sponsor" src="https://img.shields.io/badge/Become_a_sponsor-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white"></a>
</p>

## Development

```sh
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
```

Integration tests under `tests/` cover language execution, runtime checks, and the standalone command-line runner.

## Project policies

- [Code of conduct](CODE_OF_CONDUCT.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [GPL-3.0-or-later license](LICENSE)
