use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot};
use eglur_driver::{
    CompilerSession, FileSetRequest, SessionOptions, SupportProfile, SupportProfileId,
    SupportSchemaVersion, TrustedCatalog,
};
use eglur_hir::ExprKind;
use eglur_sema::{
    CompileCapability, ExpressionKey, FactState, InvocationTarget, PrimitiveType,
    ResolveCapability, TypeKind, TypecheckCapability,
};

fn overload_profile() -> SupportProfile {
    overload_profile_with_selection(true)
}

fn overload_profile_with_selection(include_overloads: bool) -> SupportProfile {
    let mut capabilities = vec![
        CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
        CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
        CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
        CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
        CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
        CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
        CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
        CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
        CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
        CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
        CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
    ];
    if include_overloads {
        capabilities.push(CompileCapability::Typecheck(
            TypecheckCapability::ResolveOverloads,
        ));
    }
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new(if include_overloads {
            "overloads-v1"
        } else {
            "without-overloads-v1"
        })
        .unwrap(),
        capabilities,
    )
    .unwrap()
}

fn session(source: &str) -> CompilerSession {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    CompilerSession::build(request, SessionOptions::default()).unwrap()
}

fn diagnostics(source: &str) -> DiagnosticSnapshot {
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    session
        .resolve(&overload_profile(), Some(&catalog))
        .unwrap();
    session.into_diagnostics(None)
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

#[test]
fn overload_family_validation_uses_normative_precedence() {
    let cases = [
        (
            concat!(
                "fn parse(value: i64) -> i64 = value\n",
                "pub overload fn parse(value: f32) -> i64 = 0\n",
            ),
            "OVERLOAD_MARKER_REQUIRED",
        ),
        (
            concat!(
                "overload fn parse(value: i64) -> i64 = value\n",
                "pub overload fn parse(value: f32) -> i64 = 0\n",
            ),
            "OVERLOAD_VISIBILITY_MISMATCH",
        ),
        (
            concat!(
                "overload fn parse(value: i64) -> i64 = value\n",
                "overload fn parse(value: i64) -> f32 = 0.0\n",
            ),
            "OVERLOAD_RETURN_ONLY_DIFFERENCE",
        ),
        (
            concat!(
                "overload fn parse[T](value: T) -> i64 = 0\n",
                "overload fn parse(value: i64) -> i64 = value\n",
            ),
            "OVERLOAD_OVERLAP",
        ),
    ];

    for (source, expected) in cases {
        let snapshot = diagnostics(source);
        assert_eq!(diagnostic_codes(&snapshot), [expected], "source:\n{source}");
    }
}

#[test]
fn invalid_overload_families_suppress_derivative_body_type_errors() {
    let snapshot = diagnostics(concat!(
        "fn parse(value: i64) -> i64 {\n",
        "  let ignored: Missing = value\n",
        "  yield value\n",
        "}\n",
        "overload fn parse(value: Str) -> Str = value\n",
    ));

    assert_eq!(diagnostic_codes(&snapshot), ["OVERLOAD_MARKER_REQUIRED"]);
}

#[test]
fn overload_calls_remain_unperformed_without_the_selection_capability() {
    let mut session = session(concat!(
        "overload fn choose(value: i64) -> i64 = value\n",
        "overload fn choose(value: Str) -> Str = value\n",
        "fn main(value: i64) -> i64 = choose(value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session
        .resolve(&overload_profile_with_selection(false), Some(&catalog))
        .unwrap();

    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn overload_overlap_ignores_bounds_and_projection_negative_facts() {
    let bounded = diagnostics(concat!(
        "trait Left {}\n",
        "trait Right {}\n",
        "overload fn choose[T: Left](value: T) -> i64 = 1\n",
        "overload fn choose[U: Right](value: U) -> i64 = 2\n",
    ));
    assert_eq!(diagnostic_codes(&bounded), ["OVERLOAD_OVERLAP"]);

    let projected = diagnostics(concat!(
        "trait HasItem { type Item }\n",
        "struct Only {}\n",
        "impl HasItem for Only { type Item = i64 }\n",
        "type ItemOf[T: HasItem] = T::Item\n",
        "overload fn choose(value: ItemOf[Only]) -> i64 = 1\n",
        "overload fn choose(value: Str) -> i64 = 2\n",
    ));
    assert_eq!(diagnostic_codes(&projected), ["OVERLOAD_OVERLAP"]);

    let nested_projection = diagnostics(concat!(
        "trait HasItem { type Item }\n",
        "struct Wrap[T] {}\n",
        "overload fn choose[T: HasItem](value: Wrap[T::Item]) -> i64 = 1\n",
        "overload fn choose(value: Wrap[Str]) -> i64 = 2\n",
    ));
    assert_eq!(diagnostic_codes(&nested_projection), ["OVERLOAD_OVERLAP"]);

    let rigid_heads = diagnostics(concat!(
        "trait HasItem { type Item }\n",
        "struct Left[T] {}\n",
        "struct Right[T] {}\n",
        "overload fn choose[T: HasItem](value: Left[T::Item]) -> i64 = 1\n",
        "overload fn choose[U: HasItem](value: Right[U::Item]) -> i64 = 2\n",
    ));
    assert!(
        matches!(rigid_heads, DiagnosticSnapshot::Accepted(_)),
        "diagnostics: {:#?}",
        rigid_heads.diagnostics()
    );
}

#[test]
fn generic_return_only_differences_are_alpha_normalized() {
    let snapshot = diagnostics(concat!(
        "overload fn choose[T](value: T) -> T = value\n",
        "overload fn choose[U](value: U) -> Str = \"value\"\n",
    ));

    assert_eq!(
        diagnostic_codes(&snapshot),
        ["OVERLOAD_RETURN_ONLY_DIFFERENCE"]
    );
}

#[test]
fn exact_overload_calls_use_independently_typed_arguments() {
    let snapshot = diagnostics(concat!(
        "enum Empty {}\n",
        "struct Wrap[T] {}\n",
        "fn absurd(value: Empty) -> Never = match value {}\n",
        "overload fn classify(value: Never) -> Bool = true\n",
        "overload fn classify(value: i64) -> i64 = value\n",
        "overload fn select[T](value: Wrap[T]) -> Wrap[T] = value\n",
        "overload fn select(value: Str) -> Str = value\n",
        "fn from_default() -> i64 = classify(1)\n",
        "fn from_qualified() -> i64 = main::classify(1)\n",
        "fn from_never(value: Empty) -> Bool = classify(absurd(value))\n",
        "fn from_generic(value: Wrap[i64]) -> Wrap[i64] = select(value)\n",
    ));

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "diagnostics: {:#?}",
        snapshot.diagnostics()
    );
}

#[test]
fn inherent_overload_families_include_the_receiver_shape() {
    let snapshot = diagnostics(concat!(
        "struct Parser {\n",
        "  overload fn parse(self: Self, value: i64) -> i64 = value\n",
        "  overload fn parse(self: &Self, value: i64) -> i64 = value\n",
        "}\n",
    ));

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "diagnostics: {:#?}",
        snapshot.diagnostics()
    );
}

#[test]
fn overload_no_match_renders_alpha_normalized_generic_candidates() {
    let snapshot = diagnostics(concat!(
        "trait Zebra {}\n",
        "trait Alpha {}\n",
        "struct Wrap[T] {}\n",
        "overload fn select[T: Zebra + Alpha](value: Wrap[T]) -> Wrap[T] = value\n",
        "overload fn select(value: Str) -> Str = value\n",
        "fn invalid(value: Bool) -> Bool = select(value)\n",
    ));
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "CALL_ARG_TYPE_MISMATCH");
    assert_eq!(
        diagnostics[0].data().get("candidates"),
        Some(&DiagnosticDataValue::from(concat!(
            "fn select[T0: Alpha + Zebra](value: Wrap[T0]) -> Wrap[T0]; ",
            "fn select(value: Str) -> Str"
        )))
    );
}

#[test]
fn explicit_type_arguments_do_not_reclassify_an_exact_no_match() {
    let snapshot = diagnostics(concat!(
        "struct Wrap[T] {}\n",
        "overload fn select[T](value: Wrap[T]) -> Wrap[T] = value\n",
        "overload fn select(value: Str) -> Str = value\n",
        "fn invalid(value: Bool) -> Bool = select::[i64](value)\n",
    ));

    assert_eq!(diagnostic_codes(&snapshot), ["CALL_ARG_TYPE_MISMATCH"]);
    assert_eq!(
        snapshot.diagnostics()[0].data().get("reason"),
        Some(&DiagnosticDataValue::from("overload_no_match"))
    );
}

#[test]
fn overload_binding_errors_precede_argument_type_errors() {
    let snapshot = diagnostics(concat!(
        "fn need_i64(value: i64) -> i64 = value\n",
        "overload fn select(value: i64) -> i64 = value\n",
        "overload fn select(value: Str) -> Str = value\n",
        "fn invalid() -> i64 = select(need_i64(true), 2)\n",
    ));

    assert_eq!(diagnostic_codes(&snapshot), ["CALL_TOO_MANY_ARGS"]);
}

#[test]
fn exact_never_does_not_infer_an_unresolved_overload_generic() {
    let snapshot = diagnostics(concat!(
        "enum Empty {}\n",
        "fn absurd(value: Empty) -> Never = match value {}\n",
        "overload fn select[T](value: T) -> T = value\n",
        "overload fn select(lhs: i64, rhs: i64) -> i64 = lhs\n",
        "fn invalid(value: Empty) -> Never = select(absurd(value))\n",
    ));

    assert_eq!(diagnostic_codes(&snapshot), ["CALL_ARG_TYPE_MISMATCH"]);
    assert_eq!(
        snapshot.diagnostics()[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("(Never,)"))
    );

    let nested = diagnostics(concat!(
        "struct Wrap[T] {}\n",
        "overload fn select[T](value: Wrap[T]) -> Wrap[T] = value\n",
        "overload fn select(value: Str) -> Str = value\n",
        "fn accepted(value: Wrap[Never]) -> Wrap[Never] = select(value)\n",
    ));
    assert!(
        matches!(nested, DiagnosticSnapshot::Accepted(_)),
        "diagnostics: {:#?}",
        nested.diagnostics()
    );
}

#[test]
fn selected_overloads_materialize_direct_call_fact_shapes() {
    let mut session = session(concat!(
        "struct Wrap[T] {}\n",
        "fn direct[T](value: T) -> T = value\n",
        "overload fn select[T](value: Wrap[T]) -> Wrap[T] = value\n",
        "overload fn select(value: Str) -> Str = value\n",
        "fn main(value: Wrap[i64]) -> (Wrap[i64], i64) = (select(value), direct(1))\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();
    let source = &session.sources()[0];
    let file = source.file();
    let hir = source.hir().expect("valid source has HIR");
    let mut calls = std::collections::BTreeMap::new();
    for (_, body) in hir.bodies() {
        for (expression_id, expression) in body.expressions() {
            let ExprKind::Call(call) = expression.kind() else {
                continue;
            };
            let Some(ExprKind::Name(path)) = body.expression(call.callee).map(|expr| expr.kind())
            else {
                continue;
            };
            let Some(name) = path.last().and_then(|name| name.text()) else {
                continue;
            };
            if matches!(name, "select" | "direct") {
                calls.insert(name.to_owned(), (expression_id, call.callee));
            }
        }
    }
    let analysis = session
        .resolve(&overload_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    let module = resolution
        .declarations()
        .module_for_file(file)
        .expect("source module is collected");

    for name in ["select", "direct"] {
        let &(expression, callee) = calls.get(name).expect("expected call remains in HIR");
        let FactState::Known(invocation) = resolution
            .body_types()
            .invocation(ExpressionKey::new(module, expression))
        else {
            panic!("{name} call has no checked invocation fact");
        };
        let InvocationTarget::Callable(instance) = invocation.target() else {
            panic!("{name} call did not select a callable instance");
        };
        let instance = resolution
            .body_types()
            .callable_instance(*instance)
            .expect("invocation references an interned callable instance");
        let substitution = resolution
            .signatures()
            .types()
            .substitution(instance.substitution())
            .unwrap();

        assert_eq!(invocation.arguments().len(), 1);
        assert_eq!(substitution.len(), 1);
        assert!(matches!(
            resolution
                .signatures()
                .types()
                .kind(substitution[0])
                .unwrap(),
            TypeKind::Primitive(PrimitiveType::I64)
        ));
        assert!(matches!(
            resolution
                .body_types()
                .expression(ExpressionKey::new(module, callee)),
            FactState::Known(_)
        ));
    }
}

#[test]
fn overload_no_match_reports_the_independent_argument_vector_and_candidates() {
    let snapshot = diagnostics(concat!(
        "overload fn width(value: i8) -> i64 = 8\n",
        "overload fn width(value: i16) -> i64 = 16\n",
        "fn invalid() -> i64 = width(1)\n",
    ));
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "CALL_ARG_TYPE_MISMATCH");
    assert_eq!(
        diagnostics[0].data().get("reason"),
        Some(&DiagnosticDataValue::from("overload_no_match"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("(i64,)"))
    );
    assert_eq!(
        diagnostics[0].data().get("candidates"),
        Some(&DiagnosticDataValue::from(
            "fn width(value: i8) -> i64; fn width(value: i16) -> i64"
        ))
    );
    assert_eq!(diagnostics[0].related().len(), 2);
}

#[test]
fn named_argument_reordering_participates_in_overlap_validation() {
    let snapshot = diagnostics(concat!(
        "overload fn choose(x: i64, y: Str) -> i64 = x\n",
        "overload fn choose(y: Str, x: i64) -> i64 = x\n",
    ));

    assert_eq!(diagnostic_codes(&snapshot), ["OVERLOAD_OVERLAP"]);
}
