<h1 align="center">C0 Interpreter</h1>

<p align="center">An initial Rust library and CLI scaffold for the C0 programming language.</p>

<p align="center">
  <a href="https://diegopn.github.io/potoo-site/">Website</a> ·
  <a href="https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf">Official C0 language reference</a>
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white"></a>
  <a href="https://doc.rust-lang.org/cargo/"><img alt="Cargo" src="https://img.shields.io/badge/Cargo-DEA584?style=for-the-badge&logo=rust&logoColor=white"></a>
  <img alt="Rust library" src="https://img.shields.io/badge/Rust_library-4A86CF?style=for-the-badge">
  <img alt="CLI" src="https://img.shields.io/badge/CLI-6A4C93?style=for-the-badge">
</p>

C0 is a programming language: a carefully defined, safe subset of C designed for learning imperative programming. It is not a pseudolanguage. This scaffold uses Carnegie Mellon University's C0 reference as its compatibility source; the exact language version and libraries will be recorded before implementation.

This repository is only a starting scaffold. It does not yet parse or execute C0 source, and no interpreter features are currently implemented. The Rust implementation will be developed here independently from the original language project.

## Original language references

- [C0 language reference](https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf)
- [C0 at Carnegie Mellon University](https://c0.cs.cmu.edu/)
- [C0 course materials](https://www.cs.cmu.edu/~15122/)

These references define the compatibility work. This repository will list its supported syntax, runtime behavior, and limitations after implementation begins.

## Development

Run these commands from the repository root:

```sh
cargo fmt --all -- --check
cargo check
cargo test
```

## Project policies

- [Code of conduct](CODE_OF_CONDUCT.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)
- [GPL-3.0-or-later license](LICENSE)
