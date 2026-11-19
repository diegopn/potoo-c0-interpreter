# C0 reference coverage

This crate targets C0.2 at compiler revision 707 and uses the corresponding C0 reference and library documentation. Coverage refers to the documented C0 language and selected libraries; CC0/C1 additions are not part of the profile.

| Reference area | Implemented profile | Boundary or validation status |
| --- | --- | --- |
| Core syntax and types | C0 declarations, functions, statements, expressions, primitive values, arrays, structs, pointers, and the documented C0 type rules | C1 loop-control features, generic pointers, and function pointers remain rejected. |
| Memory-safe operations | Checked C0 pointer and array operations, runtime bounds and arithmetic diagnostics, source locations, and call-stack context | Host files and image decoding are services, not language-level memory operations. |
| Contracts | Assertions always execute; function contracts and loop invariants can be enabled with -d | Contract checks follow the selected reference ordering and purity restrictions. |
| Libraries and imports | Built-in C0 libraries, source imports, external .h0/optional .c0 libraries, console I/O, and command-line arguments | The interpreter resolves C0 source; it does not load arbitrary native C ABI libraries. This implementation forbids unsafe Rust and does not define a native symbol loader. |
| CLI and host services | int main() result, source diagnostics, command-line arguments, and host interfaces for files, images, and terminal operations | Image decoding in the CLI depends on FFmpeg. Host services use the permissions of the embedding process. |

Existing coverage is in tests/conformance.rs, tests/runtime.rs, and tests/cli.rs. Those tests exercise representative reference behavior but do not compare every program against the upstream compiler.

## Sources

- [C0 Reference, compiler revision 707](https://www.cs.cmu.edu/~c0/docs/c0-reference.pdf)
- [CC0 User's Guide, compiler revision 712](https://c0.cs.cmu.edu/docs/cc0-users-guide.pdf)
- [C0 Library Documentation, 12 January 2011](https://www.cs.cmu.edu/~wlovas/15122-r11/misc/c0-libraries.pdf)
