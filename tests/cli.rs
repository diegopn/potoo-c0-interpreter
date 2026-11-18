use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

const CLI: &str = env!("CARGO_BIN_EXE_potoo-c0");

struct ProgramDirectory(std::path::PathBuf);

impl ProgramDirectory {
    fn new() -> Self {
        let unique = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("potoo-c0-{}-{unique}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    fn write(&self, name: &str, source: &str) -> std::path::PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        path
    }
}

impl Drop for ProgramDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_lists_supported_cli_options() {
    let output = Command::new(CLI).arg("--help").output().unwrap();

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--no-purity-check"));
    assert!(help.contains("-l LIBRARY"));
    assert!(help.contains("FILE... [-- ARG...]"));
}

#[test]
fn cli_runs_a_source_file_and_prints_mains_return_value() {
    let directory = ProgramDirectory::new();
    let path = directory.write("main.c0", "int main() { return 42; }");

    let output = Command::new(CLI).arg(path).output().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"42\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn cli_resolves_source_imports_relative_to_the_importing_file() {
    let directory = ProgramDirectory::new();
    let entry = directory.write(
        "main.c0",
        "#use \"helpers.c0\"\nint main() { return answer(); }",
    );
    directory.write("helpers.c0", "int answer() { return 9; }");

    let output = Command::new(CLI).arg(entry).output().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"9\n");
}

#[test]
fn cli_forwards_arguments_after_the_separator_to_args_library() {
    let directory = ProgramDirectory::new();
    let path = directory.write(
        "main.c0",
        "#use <args>\nint main() { args_t parsed = args_parse(); return parsed->argc; }",
    );

    let output = Command::new(CLI)
        .arg(path)
        .args(["--", "primeiro", "segundo"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"2\n");
}

#[test]
fn dynamic_contract_failure_returns_a_diagnostic_with_source_location() {
    let directory = ProgramDirectory::new();
    let path = directory.write(
        "main.c0",
        "int positive(int value)\n//@requires value > 0;\n{\n    return value;\n}\nint main() { return positive(0); }",
    );

    let output = Command::new(CLI).arg("-d").arg(path).output().unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("main.c0:2:"), "{stderr}");
    assert!(stderr.contains("precondition failed"), "{stderr}");
}

#[test]
fn cli_requires_a_source_file() {
    let output = Command::new(CLI).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("FILE..."));
}
