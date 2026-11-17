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

C0 is a safe subset of C designed for learning imperative programming. This interpreter targets the C0.2 language described in the reference dated 18 February 2025, compiler revision 707. The language includes checked memory access, 32-bit modular arithmetic, and contracts. [C0 Reference](https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf)

## Command line

```text
potoo-c0 [-d] [--no-purity-check] [-l LIBRARY] [-L DIRECTORY] FILE... [-- ARG...]
```

Source files share one compilation unit in the order supplied. A leading `#use "file.c0"` resolves relative to the importing file. A leading `#use <library>` loads a built-in library or searches the supplied `-L` directories for a C0 header and implementation. A library header can publish a concrete struct layout. Private layouts remain visible to their owning library even when other libraries share the same source files. The CLI uses canonical filesystem paths to identify files, including paths that contain bytes outside UTF-8.

The executable invokes `int main()`, prints the returned integer, and exits successfully on a normal return. Runtime diagnostics include C0 locations and the call stack. The `-d` option enables dynamic contract checks. Ordinary `assert` statements run in either mode.

## Rust library

Use `parse` with named sources and a `SourceResolver` to load imports, then call `ParsedProgram::check` and `CheckedProgram::run`. The run method returns the integer from `main`. Use `Source::from_path` to preserve file identity and `SourceResolver::source_from` to resolve relative includes against the original path. The host provides console streams, program arguments, file reads, image codecs, and terminal operations through `RuntimeHost`. Source-position events let the host pause or cancel execution.

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

## Development

```sh
cargo fmt --all -- --check
cargo check --locked
```

## Project policies

- [Code of conduct](CODE_OF_CONDUCT.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [GPL-3.0-or-later license](LICENSE)
