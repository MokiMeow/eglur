use super::{body_profile, branching_profile, diagnostic_codes, session};
use eglur_diagnostics::DiagnosticDataValue;
use eglur_driver::{SupportProfile, SupportProfileId, SupportSchemaVersion, TrustedCatalog};
use eglur_sema::{CompileCapability, FactState, PrimitiveType, TypeKind, TypecheckCapability};

#[test]
fn duplicate_aliases_report_the_later_binding_not_an_internal_collision() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (ty, pattern, later) in [
        ("i64", "x @ x", "x"),
        ("i64", "x @ (x)", "x)"),
        ("i64", "x @ x @ x", "x @ x"),
        ("(i64, i64)", "x @ (x, _)", "x, _"),
        ("(i64, i64)", "(x @ x, _)", "x, _"),
        ("Opt[i64]", "x @ Some(x)", "x)"),
        ("Opt[i64]", "Some(x @ x)", "x)"),
    ] {
        let source = format!("fn f(v: {ty}) -> i64 = match v {{ {pattern} => 0, _ => 0 }}\n");
        let mut rejected = session(&source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        let snapshot = rejected.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&snapshot),
            ["PATTERN_DUPLICATE_BINDING"],
            "{pattern}"
        );
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(
            diagnostic.data().get("name"),
            Some(&DiagnosticDataValue::from("x"))
        );
        let range = diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        let pattern_start = source.find(pattern).unwrap();
        let expected = pattern_start + pattern.rfind(later).unwrap();
        assert_eq!(range.start().as_usize(), expected, "{pattern}");
        assert_eq!(
            &source[range.start().as_usize()..range.end().as_usize()],
            "x"
        );
    }
}

#[test]
fn alias_canonicalization_preserves_pattern_diagnostic_precedence() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected) in [
        (
            "fn f(v: i64) -> i64 = match v { x @ (x, _) => 0 }",
            "PATTERN_TYPE_MISMATCH",
        ),
        (
            "fn f(v: (i64, i64)) -> i64 = match v { x @ (x, _, _) => 0 }",
            "PATTERN_ARITY_MISMATCH",
        ),
        (
            "fn f(v: Opt[i64]) -> i64 = match v { x @ Some(x, x) => 0 }",
            "PATTERN_ARITY_MISMATCH",
        ),
        (
            "fn f(v: i64) -> i64 = match v { x @ Missing(x) => 0 }",
            "NAME_NOT_FOUND",
        ),
        (
            "fn f() -> i64 { let x @ x = 1\nyield 0 }",
            "LET_PATTERN_DUPLICATE_BINDING",
        ),
        (
            "fn f() -> i64 { let x @ y = 1\nyield 0 }",
            "LET_PATTERN_UNSUPPORTED",
        ),
        ("fn f(x @ x: i64) -> i64 = 0", "PARAM_PATTERN_UNSUPPORTED"),
        (
            "fn f() -> i64 { let x = 0\nlet x @ x = 1\nyield 0 }",
            "LOCAL_BINDING_REDECLARATION",
        ),
    ] {
        let mut rejected = session(source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn distinct_aliases_remain_valid_bindings() {
    let source = "fn f(v: i64) -> i64 = match v { outer @ inner @ x => outer }\n";
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(source);
    let analysis = accepted
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    assert_eq!(resolution.names().locals().len(), 4);
    for binding in resolution.names().locals() {
        let FactState::Known(ty) = resolution.body_types().local(binding.id()) else {
            panic!("alias binding has no known type");
        };
        assert!(matches!(
            resolution.signatures().types().kind(ty.ty()).unwrap(),
            TypeKind::Primitive(PrimitiveType::I64)
        ));
    }
    assert!(diagnostic_codes(&accepted.into_diagnostics(None)).is_empty());
}

#[test]
fn parser_budgeted_match_patterns_typecheck_on_the_default_stack() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let grouped = format!("{}value{}", "(".repeat(254), ")".repeat(254));
    let aliases = (0..254)
        .map(|index| format!("alias_{index} @ "))
        .collect::<String>();

    for pattern in [grouped, format!("{aliases}value")] {
        let source = format!("fn f(input: i64) -> i64 = match input {{ {pattern} => value }}\n");
        let mut accepted = session(&source);
        let analysis = accepted
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert!(analysis.trace().missing().next().is_none(), "{pattern}");
        assert!(
            diagnostic_codes(&accepted.into_diagnostics(None)).is_empty(),
            "{pattern}"
        );
    }
}

#[test]
fn parser_budgeted_pattern_failure_keeps_one_diagnostic() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let pattern = format!("{}(value, _){}", "(".repeat(253), ")".repeat(253));
    let source = format!("fn f(input: i64) -> i64 = match input {{ {pattern} => 0 }}\n");
    let mut rejected = session(&source);
    rejected
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert_eq!(
        diagnostic_codes(&rejected.into_diagnostics(None)),
        ["PATTERN_TYPE_MISMATCH"]
    );
}

#[test]
fn parser_budgeted_let_groups_bind_on_the_default_stack() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let pattern = format!("{}value{}", "(".repeat(254), ")".repeat(254));
    let source = format!("fn f(input: i64) -> i64 {{ let {pattern} = input\n yield value }}\n");
    let mut accepted = session(&source);
    let analysis = accepted
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().next().is_none());
    let resolution = analysis.resolution().unwrap();
    let binding = resolution
        .names()
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "value")
        .unwrap();
    assert!(matches!(
        resolution.body_types().local(binding.id()),
        FactState::Known(_)
    ));
    assert!(diagnostic_codes(&accepted.into_diagnostics(None)).is_empty());
}

#[test]
fn parser_budgeted_let_groups_preserve_duplicate_and_initializer_errors() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let groups = "(".repeat(245);
    let closes = ")".repeat(245);
    let cases = [
        (
            format!(
                "fn f(input: (i64, i64)) -> i64 {{ let {groups}(value, value){closes} = input\n yield 0 }}\n"
            ),
            "LET_PATTERN_DUPLICATE_BINDING",
        ),
        (
            format!("fn f() -> i64 {{ let {groups}value{closes} = missing\n yield 0 }}\n"),
            "NAME_NOT_FOUND",
        ),
    ];
    for (source, expected) in cases {
        let mut rejected = session(&source);
        let analysis = rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        let resolution = analysis.resolution().unwrap();
        let bindings = resolution
            .names()
            .locals()
            .iter()
            .filter(|binding| binding.name().spelling() == "value")
            .collect::<Vec<_>>();
        assert!(!bindings.is_empty(), "{expected}");
        for binding in bindings {
            assert!(
                matches!(
                    resolution.body_types().local(binding.id()),
                    FactState::Poisoned(_)
                ),
                "{expected}"
            );
        }
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            [expected],
            "{expected}"
        );
    }
}

#[test]
fn bare_enum_variant_spellings_are_not_whole_value_bindings() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (declarations, ty, pattern, name) in [
        ("enum Color { Red\n Blue }\n", "Color", "Red", "Red"),
        ("enum Color { Red\n Blue }\n", "Color", "(Red)", "Red"),
        (
            "enum Color { Red\n Blue }\ntype Shade = Color\n",
            "Shade",
            "Red",
            "Red",
        ),
        (
            "enum Color { Red\n Blue }\n",
            "(Color, i64)",
            "(Red, _)",
            "Red",
        ),
        (
            "enum Color { Red\n Blue }\n",
            "Opt[Color]",
            "Some(Red)",
            "Red",
        ),
        (
            "enum Boxed[T] { Value(T)\n Empty }\n",
            "Boxed[i64]",
            "Value",
            "Value",
        ),
        (
            "import std::io::TextError\n",
            "TextError",
            "InvalidUtf8",
            "InvalidUtf8",
        ),
        ("import std::io::TextError\n", "TextError", "Io", "Io"),
        (
            "import std::io::TextError\n",
            "Opt[TextError]",
            "Some(InvalidUtf8)",
            "InvalidUtf8",
        ),
    ] {
        let source = format!(
            "{declarations}fn f(value: {ty}) -> i64 = match value {{ {pattern} => 0, _ => 1 }}\n"
        );
        let mut rejected = session(&source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        let snapshot = rejected.into_diagnostics(None);
        assert_eq!(diagnostic_codes(&snapshot), ["NAME_AMBIGUOUS"], "{source}");
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(
            diagnostic.data().get("name"),
            Some(&DiagnosticDataValue::from(name))
        );
        let range = diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            &source[range.start().as_usize()..range.end().as_usize()],
            name
        );
        assert!(range.start().as_usize() > source.find("match value").unwrap());
    }
}

#[test]
fn enum_variant_names_only_conflict_in_bare_match_binding_roles() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        "enum Color { Red\n Blue }\nfn f(value: Color) -> i64 = match value { other => 0 }\n",
        "enum Color { Red\n Blue }\nfn f(value: Color) -> i64 = match value { Red @ _ => 0 }\n",
        "enum Color { Red\n Blue }\nfn f(Red: Color) -> Color = Red\n",
        "enum Color { Red\n Blue }\nfn f(value: Color) -> Color { let Red = value\nyield Red }\n",
        "enum Color { Red\n Blue }\nfn f(value: i64) -> i64 = match value { Red => Red }\n",
        "enum Color { Red\n Blue }\nenum Other { Green }\nfn f(value: Other) -> i64 = match value { Red => 0 }\n",
        "import std::io::TextError\nfn f(value: TextError) -> i64 = match value { InvalidUtf8 @ _ => 0 }\n",
    ] {
        let mut accepted = session(source);
        accepted
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert!(
            diagnostic_codes(&accepted.into_diagnostics(None)).is_empty(),
            "{source}"
        );
    }
}

#[test]
fn poisoned_enum_members_do_not_create_derivative_pattern_diagnostics() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for pattern in ["Red", "other"] {
        let source = format!(
            "enum Color {{ Red(Missing) }}\nfn f(value: Color) -> i64 = match value {{ {pattern} => 0, _ => 1 }}\n"
        );
        let mut rejected = session(&source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            ["NAME_NOT_FOUND"]
        );
    }
}

#[test]
fn enum_metadata_does_not_enable_unsupported_pattern_or_constructor_paths() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let without_members = SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("without-members").unwrap(),
        branching_profile()
            .compile_capabilities()
            .filter(|capability| {
                *capability != CompileCapability::Typecheck(TypecheckCapability::ResolveMembers)
            }),
    )
    .unwrap();
    for source in [
        "import std::io::TextError\nfn f(value: TextError) -> i64 = match value { TextError::InvalidUtf8 => 0, _ => 1 }\n",
        "import std::io::TextError\nfn f() -> TextError = TextError::InvalidUtf8\n",
    ] {
        let mut unsupported = session(source);
        let analysis = unsupported
            .resolve(&without_members, Some(&catalog))
            .unwrap();
        assert!(analysis.trace().missing().any(|entry| entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::ResolveMembers)));
        assert!(diagnostic_codes(&unsupported.into_diagnostics(None)).is_empty());
    }

    let mut unsupported =
        session("enum Color { Red }\nfn f(value: Color) -> i64 = match value { Red => 0 }\n");
    let analysis = unsupported
        .resolve(&body_profile(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| entry.capability()
        == CompileCapability::Typecheck(TypecheckCapability::CheckBranching)));
    assert!(diagnostic_codes(&unsupported.into_diagnostics(None)).is_empty());
}
