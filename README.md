<p align="center">
  <img src="https://diegopn.github.io/potoo-site/src/assets/images/language-interpreter-icon.svg?v=1c651ca7" alt="Potoo IDE language interpreter icon" width="120" />
</p>

<h1 align="center">C0 Interpreter</h1>

<p align="center">An independent Rust interpreter library and CLI for the C0 programming language.</p>

<p align="center">
  <a href="https://github.com/diegopn/potoo-c0-interpreter/releases/latest">Download</a> ·
  <a href="https://diegopn.github.io/potoo-site/">Website</a> ·
  <a href="https://diegopn.github.io/potoo-site/manual/c0/">User manual</a>
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white"></a>
  <a href="https://doc.rust-lang.org/cargo/"><img alt="Cargo" src="https://img.shields.io/badge/Cargo-DEA584?style=for-the-badge&logo=rust&logoColor=white"></a>
  <img alt="Rust library" src="https://img.shields.io/badge/Rust_library-4A86CF?style=for-the-badge">
  <img alt="CLI" src="https://img.shields.io/badge/CLI-6A4C93?style=for-the-badge">
</p>

Originally developed as part of the Potoo IDE project, this interpreter is an independent Rust software package intended to be distributed separately from the IDE. It can be embedded as a library in other Rust projects or run as a standalone command-line tool on Linux, macOS, and Windows.

C0 is a programming language and a safe subset of C, not a pseudolanguage. This interpreter targets C0.2, compiler revision 707.

## Original language references

- [C0 Reference, compiler revision 707](https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf)
- [CC0 User’s Guide, compiler revision 712](https://c0.cs.cmu.edu/docs/cc0-users-guide.pdf)
- [CC0 compiler interface and program execution](https://www.cs.cmu.edu/afs/cs/academic/class/15122-n14/www/recitations/rec1/getting_started.html)
- [C0 Library Documentation, 12 January 2011](https://www.cs.cmu.edu/~wlovas/15122-r11/misc/c0-libraries.pdf)

This independent implementation is licensed under [GPL-3.0-or-later](LICENSE). The [original implementation](https://bitbucket.org/c0-lang/c0) has separate distribution terms.

## Source example

Save this program as `hello.c0`:

```c
int main() {
    return 42;
}
```

## Standalone command-line runner

### Install from source

Install Rust 1.93 or later, then install the CLI from the stable `main` branch:

```sh
cargo install --git https://github.com/diegopn/potoo-c0-interpreter.git --branch main --locked --bin potoo-c0
```

Alternatively, clone the repository and install the checked-out source:

```sh
git clone --branch main https://github.com/diegopn/potoo-c0-interpreter.git
cd potoo-c0-interpreter
cargo install --path . --locked --bin potoo-c0
```

Cargo places the installed executable in its configured binary directory. Include that directory in your `PATH` to run `potoo-c0` from any folder. The CLI's image file support uses FFmpeg; the remaining language features do not require image decoding or encoding.

### Command-line usage

```text
potoo-c0 [-d] [--no-purity-check] [-l LIBRARY] [-L DIRECTORY] FILE... [-- ARG...]
```

For example, `potoo-c0 hello.c0` runs the sample program and prints `42`.

Source files in one invocation share a compilation unit. `#use "file.c0"` loads source relative to the importing file; `#use <library>` loads a built-in or external C0 library.

The executable invokes `int main()`, prints the returned integer, and exits successfully on a normal return. Runtime diagnostics include C0 locations and the call stack. The `-d` option enables dynamic contract checks. Ordinary `assert` statements run in either mode.

- `-d` checks function contracts and loop invariants during execution.
- `--no-purity-check` disables the interpreter's conservative purity analysis.
- `-l LIBRARY` loads a library before the source files. Use `-L DIRECTORY` to add a directory containing external `.h0` and optional `.c0` library files.
- `-- ARG...` passes the remaining values to the program; use `#use <args>` to read them.
- `--help` prints the command syntax and available options.

Source and runtime failures are written to standard error with source locations when available, and the CLI exits unsuccessfully.

### Prebuilt binaries

The [latest release](https://github.com/diegopn/potoo-c0-interpreter/releases/latest) provides these CLI archives:

- Linux x86_64 (GNU): `potoo-c0-interpreter-v<version>-x86_64-unknown-linux-gnu.tar.gz`
- Windows x86_64 (MSVC): `potoo-c0-interpreter-v<version>-x86_64-pc-windows-msvc.zip`
- macOS Intel x86_64: `potoo-c0-interpreter-v<version>-x86_64-apple-darwin.tar.gz`

Each archive contains the `potoo-c0` executable (named `potoo-c0.exe` on Windows). `SHA256SUMS` is published with the archives. The macOS build targets Intel; Apple silicon is not included.

## Use as a Rust library

Add the crate to a Rust application's `Cargo.toml`:

```toml
[dependencies]
potoo-c0-interpreter = { git = "https://github.com/diegopn/potoo-c0-interpreter.git", branch = "main" }
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

For imports, use `parse` with a `SourceResolver`. Implement `RuntimeHost` to provide custom input, files, images, or terminal services. See the [C0 library documentation](https://www.cs.cmu.edu/~wlovas/15122-r11/misc/c0-libraries.pdf) for the standard interfaces.

The source resolver supports built-in libraries and external C0 source libraries supplied as an `.h0` header with an optional `.c0` implementation. This interpreter does not load arbitrary libraries through the native C ABI. That is a boundary of this crate: it forbids unsafe Rust and has no native-library loader. Rust itself supports C ABI calls through `unsafe extern` blocks ([Rust Reference](https://doc.rust-lang.org/reference/items/external-blocks.html)). `RuntimeHost` supplies the host-facing console, command-line argument, file, image, and terminal services used by built-in library functions. These services are distinct from C0 language semantics and are not a general native-function dispatcher.

## Development

```sh
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
```

## Support

If this interpreter is useful in your work, you can support its continued development:

<p align="left">
  <a href="https://github.com/sponsors/diegopn"><img alt="Become a sponsor" src="https://img.shields.io/badge/Become_a_sponsor-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white"></a>
</p>

## Project policies

- [Code of conduct](CODE_OF_CONDUCT.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [GPL-3.0-or-later license](LICENSE)
