use std::io;

use potoo_c0_interpreter::{
    CheckOptions, DiagnosticKind, Image, RunOptions, RuntimeHost, Source, parse,
};

#[derive(Default)]
struct Host {
    arguments: Vec<Vec<u8>>,
}

impl RuntimeHost for Host {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn open_file(&mut self, path: &[u8]) -> io::Result<Box<dyn io::BufRead>> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            String::from_utf8_lossy(path).into_owned(),
        ))
    }

    fn load_image(&mut self, path: &[u8]) -> io::Result<Image> {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            String::from_utf8_lossy(path).into_owned(),
        ))
    }

    fn arguments(&mut self) -> io::Result<Vec<Vec<u8>>> {
        Ok(self.arguments.clone())
    }
}

fn execute(source: &str, check_contracts: bool) -> Result<i32, potoo_c0_interpreter::Diagnostic> {
    execute_with_host(source, check_contracts, &mut Host::default())
}

fn execute_with_host(
    source: &str,
    check_contracts: bool,
    host: &mut Host,
) -> Result<i32, potoo_c0_interpreter::Diagnostic> {
    let parsed = parse([Source::new("conformance.c0", source)], &[], &mut ())?;
    let checked = parsed.check(CheckOptions::default())?;
    checked.run(host, RunOptions { check_contracts })
}

#[test]
fn zero_is_a_valid_random_generator_seed() {
    let result = execute(
        "#use <rand>\nint main() { rand_t generator = init_rand(0); assert(rand(generator) == 1013904223); assert(rand(generator) == 1196435762); return rand(generator); }",
        true,
    )
    .unwrap();

    assert_eq!(result, -775_096_599);
}

#[test]
fn opening_a_missing_file_aborts_instead_of_returning_null() {
    let diagnostic = execute(
        "#use <file>\nint main() { file_t file = file_read(\"missing\"); return file == NULL ? 0 : 1; }",
        false,
    )
    .unwrap_err();

    assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
    assert!(diagnostic.message.contains("file_read"));
}

#[test]
fn loading_a_missing_image_aborts_instead_of_returning_null() {
    let diagnostic = execute(
        "#use <img>\nint main() { image_t image = image_load(\"missing\"); return image == NULL ? 0 : 1; }",
        false,
    )
    .unwrap_err();

    assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
    assert!(diagnostic.message.contains("image_load"));
}

#[test]
fn invalid_integer_argument_leaves_the_destination_unchanged() {
    let source = r#"#use <args>
int main() {
    int* number = alloc(int);
    *number = 7;
    args_int("number", number);
    args_t parsed = args_parse();
    return parsed->argc + (*number == 7 ? 10 : 0);
}"#;
    let mut host = Host {
        arguments: vec![
            b"-number".to_vec(),
            b"invalid".to_vec(),
            b"positional".to_vec(),
        ],
    };

    let result = execute_with_host(source, false, &mut host).unwrap();

    assert_eq!(result, 11);
}

#[test]
fn declarations_are_accepted_as_single_statement_bodies() {
    let source = r#"int main() {
    if (true) int selected = 1;
    else char skipped;
    while (false) bool never;
    for (int i = 0; i < 0; i++) string unreachable;
    return 0;
}"#;

    assert_eq!(execute(source, false).unwrap(), 0);
}

#[test]
fn declarations_in_unbraced_branches_remain_in_scope_to_the_end_of_the_block() {
    let source = r#"int main() {
    bool condition = false;
    if (condition) int selected = 1;
    else selected = 2;
    return selected;
}"#;

    assert_eq!(execute(source, false).unwrap(), 2);
}

#[test]
fn declarations_in_unbraced_loop_bodies_remain_in_scope_after_the_loop() {
    for (loop_statement, expected) in [
        ("while (false) int selected = 1;", 2),
        ("for (; false; ) int selected = 1;", 3),
    ] {
        let source =
            format!("int main() {{ {loop_statement} selected = {expected}; return selected; }}");

        assert_eq!(execute(&source, false).unwrap(), expected);
    }
}

#[test]
fn declarations_in_unbraced_bodies_still_require_initialization_on_every_path() {
    for source in [
        "int main() { bool condition = false; if (condition) int selected = 1; return selected; }",
        "int main() { while (false) int selected = 1; return selected; }",
    ] {
        let parsed = parse([Source::new("uninitialized.c0", source)], &[], &mut ()).unwrap();
        let diagnostic = match parsed.check(CheckOptions::default()) {
            Ok(_) => panic!("a skipped declaration cannot initialize the variable"),
            Err(diagnostic) => diagnostic,
        };

        assert_eq!(diagnostic.kind, DiagnosticKind::Validation);
        assert!(diagnostic.message.contains("may be uninitialized"));
    }
}

#[test]
fn for_initializer_variables_do_not_escape_the_loop() {
    let source = "int main() { for (int index = 0; index < 0; index++) {} return index; }";
    let parsed = parse([Source::new("for-scope.c0", source)], &[], &mut ()).unwrap();
    let diagnostic = match parsed.check(CheckOptions::default()) {
        Ok(_) => panic!("for initializer declarations are scoped to the loop"),
        Err(diagnostic) => diagnostic,
    };

    assert_eq!(diagnostic.kind, DiagnosticKind::Validation);
    assert!(diagnostic.message.contains("unknown variable 'index'"));
}

#[test]
fn for_step_cannot_reference_a_declaration_in_its_body() {
    let source =
        "int main() { for (int index = 0; index < 0; body_local++) int body_local = 1; return 0; }";
    let parsed = parse([Source::new("for-step.c0", source)], &[], &mut ()).unwrap();
    let diagnostic = match parsed.check(CheckOptions::default()) {
        Ok(_) => panic!("for step cannot see declarations in the loop body"),
        Err(diagnostic) => diagnostic,
    };

    assert_eq!(diagnostic.kind, DiagnosticKind::Validation);
    assert!(diagnostic.message.contains("unknown variable 'body_local'"));
}

#[test]
fn c0_integer_arithmetic_uses_signed_division_and_modular_words() {
    let source = r#"int main() {
    assert(2147483647 + 1 == -2147483648);
    assert(-7 / 3 == -2);
    assert(-7 % 3 == -1);
    assert((-5 >> 1) == -3);
    assert((1 << 31) == -2147483648);
    return 0;
}"#;

    assert_eq!(execute(source, false).unwrap(), 0);
}

#[test]
fn c0_arithmetic_exceptions_cover_division_overflow_and_shift_range() {
    for expression in [
        "1 / 0",
        "-2147483648 / -1",
        "-2147483648 % -1",
        "1 << 32",
        "1 >> -1",
    ] {
        let source = format!("int main() {{ return {expression}; }}");
        let diagnostic = execute(&source, false).unwrap_err();

        assert_eq!(diagnostic.kind, DiagnosticKind::Runtime, "{expression}");
    }
}

#[test]
fn ordinary_assertions_run_when_contract_checks_are_disabled() {
    let diagnostic = execute("int main() { assert(false); return 0; }", false).unwrap_err();

    assert_eq!(diagnostic.kind, DiagnosticKind::Runtime);
    assert!(diagnostic.message.contains("assertion failed"));
}

#[test]
fn string_and_integer_parsing_libraries_follow_their_documented_edges() {
    let source = r#"#use <parse>
#use <string>
#use <util>
int main() {
    int* minimum = parse_int("0x80000000", 16);
    assert(minimum != NULL && *minimum == -2147483648);
    assert(string_equal(int2hex(-1), "FFFFFFFF"));
    assert(string_equal(int2hex(-2147483648), "80000000"));
    assert(string_compare("a", "b") == -1);
    assert(string_compare("b", "a") == 1);
    assert(string_compare("same", "same") == 0);
    char[] characters = string_to_chararray("A\n");
    assert(string_terminated(characters, 3));
    assert(string_equal(string_from_chararray(characters), "A\n"));
    return 0;
}"#;

    assert_eq!(execute(source, false).unwrap(), 0);
}

#[test]
fn pointers_structs_and_arrays_preserve_c0_reference_identity() {
    let source = r#"struct node {
    int value;
    struct node* next;
    int[] empty;
};

int main() {
    struct node* first = alloc(struct node);
    struct node* second = alloc(struct node);
    assert(first->value == 0 && first->next == NULL);
    first->value = 7;
    first->next = second;
    second->value = 9;

    int[] values = alloc_array(int, 2);
    values[0] = first->value;
    values[1] = first->next->value;
    int[] alias = values;
    int[] allocated_empty = alloc_array(int, 0);
    assert(alias == values);
    assert(first->empty == second->empty);
    assert(first->empty != allocated_empty);
    return values[0] + values[1];
}"#;

    assert_eq!(execute(source, false).unwrap(), 16);
}

#[test]
fn contracts_and_loop_invariants_are_checked_in_reference_order() {
    let source = r#"int triangular(int n)
//@requires n >= 0;
//@ensures \result == n * (n + 1) / 2;
{
    int sum = 0;
    for (int i = 0; i <= n; i++)
    //@loop_invariant 0 <= i && i <= n + 1;
    {
        sum += i;
    }
    return sum;
}

int main() { return triangular(5); }"#;
    assert_eq!(execute(source, false).unwrap(), 15);
    let result = execute(source, true).unwrap();

    assert_eq!(result, 15);
}

#[test]
fn functions_that_mutate_heap_memory_are_rejected_in_contracts() {
    let source = r#"#use <rand>
int sample(rand_t generator)
//@ensures rand(generator) == \result;
{
    return 0;
}

int main() {
    rand_t generator = init_rand(0);
    return sample(generator);
}"#;
    let parsed = parse([Source::new("purity.c0", source)], &[], &mut ()).unwrap();
    let diagnostic = match parsed.check(CheckOptions::default()) {
        Ok(_) => panic!("purity validation should reject rand in a contract"),
        Err(diagnostic) => diagnostic,
    };

    assert_eq!(diagnostic.kind, DiagnosticKind::Validation);
    assert!(
        diagnostic
            .message
            .contains("may modify previously allocated memory")
    );
}

#[test]
fn c1_loop_control_remains_rejected_by_c0() {
    for statement in ["break;", "continue;"] {
        let source = format!("int main() {{ while (true) {{ {statement} }} return 0; }}");
        let diagnostic = execute(&source, false).unwrap_err();

        assert_eq!(diagnostic.kind, DiagnosticKind::Syntax);
        assert!(diagnostic.message.contains("require C1"));
    }
}

#[test]
fn c1_only_generic_and_function_pointers_remain_rejected_by_c0() {
    let generic_pointer = "void* identity(void* value) { return value; } int main() { return 0; }";
    let parsed = parse([Source::new("generic.c0", generic_pointer)], &[], &mut ()).unwrap();
    let diagnostic = match parsed.check(CheckOptions::default()) {
        Ok(_) => panic!("generic pointers require C1"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(diagnostic.kind, DiagnosticKind::Validation);

    let function_pointer = "int helper() { return 1; } int main() { return (*&helper)(); }";
    let diagnostic = match parse(
        [Source::new("function-pointer.c0", function_pointer)],
        &[],
        &mut (),
    ) {
        Ok(_) => panic!("function pointers require C1"),
        Err(diagnostic) => diagnostic,
    };
    assert_eq!(diagnostic.kind, DiagnosticKind::Syntax);
}
