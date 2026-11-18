use std::io;

use potoo_c0_interpreter::{
    CheckOptions, DiagnosticKind, RunOptions, RuntimeHost, Source, parse, run,
};

#[test]
fn run_returns_the_integer_from_main() {
    let result = run("int main() { return 42; }", &mut Vec::new()).unwrap();

    assert_eq!(result, 42);
}

#[test]
fn function_calls_and_while_loops_compute_the_expected_result() {
    let source = r#"int square(int value) {
    return value * value;
}

int main() {
    int i = 0;
    int total = 0;
    while (i < 3) {
        total = total + square(i);
        i = i + 1;
    }
    return total;
}"#;

    let result = run(source, &mut Vec::new()).unwrap();

    assert_eq!(result, 5);
}

#[test]
fn conio_output_is_written_to_the_supplied_buffer() {
    let source = r#"#use <conio>
int main() {
    println("Bonjour");
    return 0;
}"#;
    let mut output = Vec::new();

    let result = run(source, &mut output).unwrap();

    assert_eq!(result, 0);
    assert_eq!(output, b"Bonjour\n");
}

#[test]
fn array_access_outside_its_length_returns_a_runtime_diagnostic() {
    let source = r#"int main() {
    int[] values = alloc_array(int, 1);
    return values[1];
}"#;

    let diagnostic = run(source, &mut Vec::new()).unwrap_err();

    assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
    assert_eq!(diagnostic.location.line, 3);
}

#[test]
fn dynamic_contract_checks_run_when_enabled() {
    struct BufferHost;

    impl RuntimeHost for BufferHost {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<()> {
            Ok(())
        }
    }

    let source = r#"int positive(int value)
//@requires value > 0;
{
    return value;
}

int main() {
    return positive(0);
}"#;
    let parsed = parse([Source::new("contract.c0", source)], &[], &mut ()).unwrap();
    let checked = parsed.check(CheckOptions::default()).unwrap();
    let diagnostic = checked
        .run(
            &mut BufferHost,
            RunOptions {
                check_contracts: true,
            },
        )
        .unwrap_err();

    assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
    assert!(diagnostic.message.contains("precondition failed"));
}
