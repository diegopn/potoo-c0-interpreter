# Security Policy

## Supported versions

The `develop` branch is the active development line. Reports affecting `develop` and the latest published release will be considered; older releases may no longer receive security updates.

## Report a vulnerability

Do not publish vulnerability details in issues, pull requests, discussions, or public comments. If GitHub provides the private **Report a vulnerability** form on the repository’s Security page, use it. Otherwise, use the [Potoo contact page](https://diegopn.github.io/#contato) only to request a private reporting channel. Do not include technical details or a proof of concept in that first message.

After arranging a private channel, include the affected version or commit, Rust version, operating system, reproduction steps, impact, and a minimal `C0` example. Remove credentials, personal information, and unrelated data.

For non-security issues, see [CONTRIBUTING.md](CONTRIBUTING.md). For conduct concerns, see [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Security boundaries

C0 source runs in the process that calls the interpreter. The crate does not create an operating-system sandbox or enforce general CPU or memory quotas. The standalone CLI and `RuntimeHost` may provide console, argument, file, image, and terminal services; those services use the permissions of the host process. The crate does not load arbitrary native C ABI libraries. Call-depth and other implementation guards do not isolate a program from its host. For untrusted source, expose only required host services and run it in a restricted process with operating-system access and resource limits.
