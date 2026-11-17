use eglur_driver::{
    CompilerSession, FileSetRequest, ProgramMode, SealedCompilation, SessionOptions,
    SupportProfile, SupportProfileId, TrustedCatalog,
};
use eglur_sema::{CompileCapability, TypecheckCapability};

fn compile_with(source: &str, profile: &SupportProfile) -> SealedCompilation {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    session
        .resolve_for(
            ProgramMode::Executable,
            profile,
            Some(&TrustedCatalog::first_slice().unwrap()),
        )
        .unwrap();
    session.seal(None).unwrap()
}

fn compile(source: &str) -> SealedCompilation {
    compile_with(source, &SupportProfile::semantic_core_v1())
}

fn assert_operand_error(source: &str, expression: &str, operation: &str, types: &[(&str, &str)]) {
    let compiled = compile(source);
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(diagnostics.len(), 1, "{source}\n{diagnostics:?}");
    let diagnostic = &diagnostics[0];
    assert_eq!(
        diagnostic.code().as_str(),
        "NUMERIC_OPERAND_NOT_NUMERIC",
        "{source}\n{diagnostics:?}"
    );
    assert_eq!(diagnostic.data()["operation"], operation.into());
    assert_eq!(
        diagnostic.data()["reason"],
        if types.len() == 1 {
            "unary_operand".into()
        } else {
            "binary_operand".into()
        }
    );
    for (key, expected) in types {
        assert_eq!(diagnostic.data()[*key], (*expected).into(), "{source}");
        assert!(diagnostic.message().contains(expected));
    }
    assert!(diagnostic.message().contains("numeric"));
    assert!(diagnostic.message().contains("operand"));
    assert!(diagnostic.message().contains(operation));
    let range = diagnostic
        .primary_span()
        .as_source()
        .unwrap()
        .file_span()
        .range();
    assert_eq!(
        source[range.start().as_usize()..range.end().as_usize()].trim(),
        expression
    );
}

#[test]
fn binary_numeric_operators_reject_nonnumeric_types_before_checked_requirements() {
    for dtype in ["Str", "Unit", "Nat", "PositiveNat", "(i64, i64)", "&i64"] {
        for numeric in ["i8", "u64", "Bool", "f32", "complex128"] {
            for operator in ["+", "-", "*", "/", "/.", "%"] {
                for reversed in [false, true] {
                    let (left, right) = if reversed {
                        (numeric, dtype)
                    } else {
                        (dtype, numeric)
                    };
                    let expression = format!("a {operator} b");
                    let source = format!(
                        "fn invalid(a: {left}, b: {right}) -> Unit {{ let value = {expression}\nyield () }}\nfn main!() -> Unit = ()\n"
                    );
                    assert_operand_error(
                        &source,
                        &expression,
                        operator,
                        &[("left_type", left), ("right_type", right)],
                    );
                }
            }
        }
    }
}

#[test]
fn aliases_nominals_and_literals_report_canonical_operand_types() {
    for (prefix, params, expression, operation, left, right) in [
        ("type Label = Str\n", "a: Label", "a + 1", "+", "Str", "i64"),
        ("", "", "1 /. \"text\"", "/.", "i64", "Str"),
        ("", "", "\"a\" + \"b\"", "+", "Str", "Str"),
        (
            "struct Data { field n: i64 }\n",
            "a: Data, b: i8",
            "a % b",
            "%",
            "Data",
            "i8",
        ),
    ] {
        let source = format!(
            "{prefix}fn invalid({params}) -> Unit {{ let value = {expression}\nyield () }}\nfn main!() -> Unit = ()\n"
        );
        assert_operand_error(
            &source,
            expression,
            operation,
            &[("left_type", left), ("right_type", right)],
        );
    }
}

#[test]
fn unary_minus_rejects_nonnumeric_operands_inside_and_outside_checked() {
    for dtype in ["Str", "Unit", "Nat", "PositiveNat", "(i64, i64)", "&i64"] {
        for checked in [false, true] {
            let expression = if checked {
                "checked { yield -a }"
            } else {
                "-a"
            };
            let source = format!(
                "fn invalid(a: {dtype}) -> Unit {{ let value = {expression}\nyield () }}\nfn main!() -> Unit = ()\n"
            );
            assert_operand_error(&source, "-a", "-", &[("actual_type", dtype)]);
        }
    }
}

#[test]
fn nested_poison_does_not_create_derivative_numeric_diagnostics() {
    for expression in [
        "-(a + b)",
        "(a + b) * 2",
        "3 /. (a + b)",
        "checked { yield (a + b) / 0 }",
    ] {
        let source = format!(
            "fn invalid(a: Str, b: i64) -> Unit {{ let value = {expression}\nyield () }}\nfn main!() -> Unit = ()\n"
        );
        assert_operand_error(
            &source,
            "a + b",
            "+",
            &[("left_type", "Str"), ("right_type", "i64")],
        );
    }
    let compiled = compile("fn main!() -> Unit { let value = missing + \"text\"\nyield () }\n");
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code().as_str(), "NAME_NOT_FOUND");
}

#[test]
fn independent_operand_errors_are_retained_without_parent_cascades() {
    let compiled = compile(
        "fn invalid(a: Str, b: i64) -> Unit { let value = (a + b) /. (b - a)\nyield () }\nfn main!() -> Unit = ()\n",
    );
    let diagnostics = compiled.diagnostics().diagnostics();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    for (diagnostic, operation) in diagnostics.iter().zip(["+", "-"]) {
        assert_eq!(diagnostic.code().as_str(), "NUMERIC_OPERAND_NOT_NUMERIC");
        assert_eq!(diagnostic.data()["operation"], operation.into());
    }
}

#[test]
fn long_fractional_division_chains_retain_one_root_error_on_the_default_stack() {
    let source = format!(
        "fn invalid(a: Str) -> Unit {{ let value = a{}\nyield () }}\nfn main!() -> Unit = ()\n",
        " /. a".repeat(4000)
    );
    assert_operand_error(
        &source,
        "a /. a",
        "/.",
        &[("left_type", "Str"), ("right_type", "Str")],
    );
}

#[test]
fn valid_unimplemented_numeric_operations_are_not_reported_as_type_errors() {
    for (dtype, expression) in [
        ("Bool", "a + a"),
        ("f32", "a * a"),
        ("complex128", "-a"),
        ("i64", "a /. a"),
        ("Never", "-a"),
    ] {
        let source = format!(
            "fn pending(a: {dtype}) -> Unit {{ let value = {expression}\nyield () }}\nfn main!() -> Unit = ()\n"
        );
        let compiled = compile(&source);
        assert!(
            compiled.diagnostics().diagnostics().is_empty(),
            "{source}\n{:?}",
            compiled.diagnostics()
        );
        assert!(compiled.trace().missing().next().is_some(), "{source}");
    }
    let compiled = compile(
        "fn pending[T](a: T) -> Unit { let value = -a\nyield () }\nfn main!() -> Unit = ()\n",
    );
    assert!(
        compiled.diagnostics().diagnostics().is_empty(),
        "{:?}",
        compiled.diagnostics()
    );
    assert!(compiled.trace().missing().next().is_some());
}

#[test]
fn numeric_validation_respects_the_selected_profile_without_requiring_const_evaluation() {
    let core = SupportProfile::semantic_core_v1();
    for (excluded, expect_diagnostic) in [
        (
            TypecheckCapability::EvaluateSelectedIntegerConstExpressions,
            true,
        ),
        (
            TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            false,
        ),
    ] {
        let profile = SupportProfile::compile_only(
            core.schema(),
            SupportProfileId::new("numeric-operand-profile").unwrap(),
            core.compile_capabilities().filter(|capability| {
                !matches!(
                    capability,
                    CompileCapability::Typecheck(
                        TypecheckCapability::EvaluateSelectedIntegerConstExpressions
                            | TypecheckCapability::EvaluateSelectedIntegerConstDeclarations
                    )
                ) && *capability != CompileCapability::Typecheck(excluded)
            }),
        )
        .unwrap();
        let compiled = compile_with(
            "fn main!() -> Unit { let value = \"text\" + 1\nyield () }\n",
            &profile,
        );
        let diagnostics = compiled.diagnostics().diagnostics();
        if expect_diagnostic {
            assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
            assert_eq!(
                diagnostics[0].code().as_str(),
                "NUMERIC_OPERAND_NOT_NUMERIC"
            );
        } else {
            assert!(diagnostics.is_empty());
            assert!(compiled.trace().missing().next().is_some());
        }
    }
}
