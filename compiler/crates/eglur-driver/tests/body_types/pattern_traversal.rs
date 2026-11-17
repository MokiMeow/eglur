use super::{branching_profile, diagnostic_codes, session};
use eglur_driver::TrustedCatalog;
use eglur_sema::{
    CapabilityTrace, CompileCapability, FactState, LocalTypeFact, PrimitiveType,
    SemanticResolution, TypeKind, TypecheckCapability,
};

fn check_pattern(
    source: &str,
    expected_diagnostics: &[&str],
    inspect: impl FnOnce(&SemanticResolution, &CapabilityTrace),
) {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut checked = session(source);
    let analysis = checked
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    inspect(analysis.resolution().unwrap(), analysis.trace());

    let diagnostics = checked.into_diagnostics(None);
    assert_eq!(
        diagnostic_codes(&diagnostics),
        expected_diagnostics,
        "{:?}",
        diagnostics.diagnostics()
    );
}

fn local<'a>(resolution: &'a SemanticResolution, name: &str) -> FactState<'a, LocalTypeFact> {
    let binding = resolution
        .names()
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == name)
        .unwrap();
    resolution.body_types().local(binding.id())
}

fn assert_i64_binding(resolution: &SemanticResolution, name: &str) {
    let FactState::Known(fact) = local(resolution, name) else {
        panic!("expected a known binding for {name}");
    };
    assert_eq!(
        resolution.signatures().types().kind(fact.ty()).unwrap(),
        &TypeKind::Primitive(PrimitiveType::I64)
    );
}

fn option_match(depth: usize, leaf_type: &str, leaf_pattern: &str, value: &str) -> String {
    let ty = format!("{}{leaf_type}{}", "Opt[".repeat(depth), "]".repeat(depth));
    let pattern = format!(
        "{}{leaf_pattern}{}",
        "Some(".repeat(depth),
        ")".repeat(depth)
    );
    format!("fn f(input: {ty}) -> i64 = match input {{ {pattern} => {value}, _ => 0 }}\n")
}

#[test]
fn parser_budgeted_tuple_patterns_retain_the_payload_type() {
    let depth = 254;
    let ty = format!("{}i64{}", "(".repeat(depth), ",)".repeat(depth));
    let pattern = format!("{}value{}", "(".repeat(depth), ",)".repeat(depth));
    let source = format!("fn f(input: {ty}) -> i64 = match input {{ {pattern} => value }}\n");

    check_pattern(&source, &[], |resolution, trace| {
        assert!(!trace.has_missing());
        assert_i64_binding(resolution, "value");
    });
}

#[test]
fn mixed_alias_tuple_and_variant_patterns_preserve_every_binding() {
    let depth = 70;
    let mut ty = "i64".to_owned();
    let mut pattern = "value".to_owned();
    for index in 0..depth {
        ty = format!("(Opt[{ty}],)");
        pattern = format!("alias_{index} @ (Some({pattern}),)");
    }
    let source =
        format!("fn f(input: {ty}) -> i64 = match input {{ {pattern} => value, _ => 0 }}\n");

    check_pattern(&source, &[], |resolution, trace| {
        assert!(!trace.has_missing());
        assert_i64_binding(resolution, "value");
        assert_eq!(resolution.names().locals().len(), depth + 2);
        for index in 0..depth {
            let FactState::Known(fact) = local(resolution, &format!("alias_{index}")) else {
                panic!("alias {index} lost its type");
            };
            assert!(matches!(
                resolution.signatures().types().kind(fact.ty()).unwrap(),
                TypeKind::Tuple(_)
            ));
        }
    });
}

#[test]
fn parser_budgeted_user_variant_patterns_instantiate_generic_payloads() {
    let depth = 254;
    let ty = format!("{}i64{}", "Link[".repeat(depth), "]".repeat(depth));
    let pattern = format!("{}value{}", "Link::Next(".repeat(depth), ")".repeat(depth));
    let source = format!(
        "enum Link[T] {{ Next(T)\n End }}\nfn f(input: {ty}) -> i64 = match input {{ {pattern} => value, _ => 0 }}\n"
    );

    check_pattern(&source, &[], |resolution, trace| {
        assert!(!trace.has_missing());
        assert_i64_binding(resolution, "value");
    });
}

#[test]
fn deep_variant_and_tuple_rest_patterns_skip_unbound_payloads() {
    let depth = 240;
    let source = option_match(depth, "(i64, Bool, Str)", "(value, ..)", "value");

    check_pattern(&source, &[], |resolution, trace| {
        assert!(!trace.has_missing());
        assert_i64_binding(resolution, "value");
        assert_eq!(resolution.names().locals().len(), 2);
    });

    let source = format!(
        "enum Pair[A, B] {{ Both(A, B) }}\n{}",
        option_match(depth, "Pair[i64, Bool]", "Pair::Both(value, ..)", "value")
    );
    check_pattern(&source, &[], |resolution, trace| {
        assert!(!trace.has_missing());
        assert_i64_binding(resolution, "value");
    });
}

#[test]
fn deep_pattern_type_failure_keeps_one_diagnostic_and_poisoned_bindings() {
    let source = option_match(240, "i64", "(value, _)", "value");
    check_pattern(&source, &["PATTERN_TYPE_MISMATCH"], |resolution, trace| {
        assert!(!trace.has_missing());
        assert!(matches!(local(resolution, "value"), FactState::Poisoned(_)));
    });
}

#[test]
fn deep_pattern_duplicate_check_follows_type_compatibility() {
    for (ty, expected) in [
        ("(i64, i64)", "PATTERN_DUPLICATE_BINDING"),
        ("i64", "PATTERN_TYPE_MISMATCH"),
    ] {
        let source = option_match(240, ty, "(value, value)", "value");
        check_pattern(&source, &[expected], |resolution, trace| {
            assert!(!trace.has_missing());
            for binding in resolution
                .names()
                .locals()
                .iter()
                .filter(|binding| binding.name().spelling() == "value")
            {
                assert!(matches!(
                    resolution.body_types().local(binding.id()),
                    FactState::Poisoned(_)
                ));
            }
        });
    }
}

#[test]
fn deep_unsupported_patterns_leave_bindings_unperformed() {
    for (declarations, ty, pattern) in [
        ("", "i64", "value | value"),
        (
            "enum Named[T] { Value { item: T } }\n",
            "Named[i64]",
            "Named::Value { item: value }",
        ),
    ] {
        let source = format!("{declarations}{}", option_match(240, ty, pattern, "value"));
        check_pattern(&source, &[], |resolution, trace| {
            assert!(matches!(local(resolution, "value"), FactState::Unperformed));
            assert_eq!(
                trace
                    .missing()
                    .map(|entry| entry.capability())
                    .collect::<Vec<_>>(),
                [CompileCapability::Typecheck(
                    TypecheckCapability::CheckAdvancedPatterns
                )]
            );
        });
    }
}

#[test]
fn first_payload_failure_does_not_request_later_pattern_capabilities() {
    let source = format!(
        "enum Pair[A, B] {{ Both(A, B) }}\n{}",
        option_match(
            240,
            "Pair[i64, i64]",
            "Pair::Both((first, _), later | later)",
            "0",
        )
    );
    check_pattern(&source, &["PATTERN_TYPE_MISMATCH"], |resolution, trace| {
        assert!(!trace.has_missing());
        for name in ["first", "later"] {
            assert!(matches!(local(resolution, name), FactState::Poisoned(_)));
        }
    });
}

#[test]
fn variant_payload_types_are_instantiated_only_when_visited() {
    for (first, expected_diagnostics, visits_second) in [
        ("(first, _)", vec!["PATTERN_TYPE_MISMATCH"], false),
        ("first", vec![], true),
    ] {
        let pattern = format!("Pair::Both({first}, (later,))");
        let source = format!(
            "enum Pair[A, B] {{ Both(A, (B,)) }}\n{}",
            option_match(240, "Pair[i64, Bool]", &pattern, "0")
        );
        check_pattern(&source, &expected_diagnostics, |resolution, trace| {
            assert!(!trace.has_missing());
            for name in ["first", "later"] {
                assert!(if visits_second {
                    matches!(local(resolution, name), FactState::Known(_))
                } else {
                    matches!(local(resolution, name), FactState::Poisoned(_))
                });
            }

            let types = resolution.signatures().types();
            let has_bool_tuple = types.types().any(|(_, kind)| match kind {
                TypeKind::Tuple(elements) if elements.len() == 1 => matches!(
                    types.kind(elements[0]).unwrap(),
                    TypeKind::Primitive(PrimitiveType::Bool)
                ),
                _ => false,
            });
            assert_eq!(has_bool_tuple, visits_second);
        });
    }
}

#[test]
fn alias_and_tuple_payloads_preserve_variant_exhaustiveness() {
    let source = concat!(
        "fn f(input: Opt[(i64, Bool)]) -> i64 = match input {\n",
        "  outer @ Some((value, _)) => value,\n",
        "  None => 0,\n",
        "}\n",
    );
    check_pattern(source, &[], |resolution, trace| {
        assert!(!trace.has_missing());
        assert_i64_binding(resolution, "value");
        assert!(matches!(local(resolution, "outer"), FactState::Known(_)));
    });
}
