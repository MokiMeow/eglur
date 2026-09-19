use eglur_diagnostics::{DiagnosticDataValue, DiagnosticSnapshot, render_plain};
use eglur_driver::{
    CompilerSession, FileInput, FileSetRequest, SessionOptions, SupportProfile, SupportProfileId,
    SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{
    CanonicalTarget, CheckedNumericOperator, CompileCapability, ControlRegionKey,
    ControlTransferKind, ControlTransferSource, DeclarationSignature, ExternalDefId, FactState,
    Inhabitance, IntrinsicNominal, NominalHead, NumericFaultCause, PrimitiveType,
    ResolveCapability, TypeKind, TypecheckCapability,
};

#[path = "body_types/application.rs"]
mod application;

#[path = "body_types/invocation_traversal.rs"]
mod invocation_traversal;

#[path = "body_types/pattern_bindings.rs"]
mod pattern_bindings;

#[path = "body_types/statements.rs"]
mod statements;

#[path = "body_types/traversal.rs"]
mod traversal;

fn body_capabilities() -> [CompileCapability; 14] {
    [
        CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
        CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
        CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
        CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
        CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
        CompileCapability::Resolve(ResolveCapability::ResolvePatternScopes),
        CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
        CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
        CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
        CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
        CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
        CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
        CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
        CompileCapability::Typecheck(TypecheckCapability::CheckSelectedCheckedIntegerArithmetic),
    ]
}

fn branching_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("branching-v1").unwrap(),
        body_capabilities()
            .into_iter()
            .chain([CompileCapability::Typecheck(
                TypecheckCapability::CheckBranching,
            )]),
    )
    .unwrap()
}

fn overload_failure_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("overload-failure-v1").unwrap(),
        body_capabilities()
            .into_iter()
            .chain([CompileCapability::Typecheck(
                TypecheckCapability::ResolveOverloads,
            )]),
    )
    .unwrap()
}

#[test]
fn result_question_records_the_exact_nested_region_exit() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
        "fn choose(flag: Bool) -> Res[i64, Str] = result {\n",
        "  let value = if flag { yield source()? } else { yield 0 }\n",
        "  yield Ok(value)\n",
        "}\n",
    ));

    let analysis = session
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("failure typing is selected");
    let transfer = resolution
        .body_types()
        .transfers()
        .iter()
        .find(|transfer| transfer.kind() == ControlTransferKind::QuestionShortCircuit)
        .expect("question short-circuit edge is present");

    assert!(matches!(
        transfer.source(),
        ControlTransferSource::Expression(_)
    ));
    assert!(matches!(transfer.target(), ControlRegionKey::Result(_)));
    assert_eq!(transfer.exited_regions().len(), 2);
    assert_eq!(&transfer.exited_regions()[0], transfer.target());
    assert!(matches!(
        transfer.exited_regions()[1],
        ControlRegionKey::IfBranch { .. }
    ));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn expression_body_records_callable_normal_completion() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session("fn value() -> i64 = 1\n");

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    let value = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.spelling() == "value")
        })
        .expect("value declaration is collected");
    let region = ControlRegionKey::Callable(value.id());

    assert_eq!(
        resolution
            .body_types()
            .transfers()
            .iter()
            .filter(|transfer| {
                transfer.kind() == ControlTransferKind::NormalCompletion
                    && transfer.source() == &ControlTransferSource::RegionCompletion(region.clone())
                    && transfer.target() == &region
            })
            .count(),
        1
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn question_edges_follow_failure_carrier_inhabitance() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn optional() -> Opt[i64] = Some(1)\n",
        "fn infallible(value: i64) -> Res[i64, Never] = Ok(value)\n",
        "fn from_opt() -> Opt[i64] = result {\n",
        "  let value = optional()?\n",
        "  yield Some(value)\n",
        "}\n",
        "fn from_infallible() -> Res[i64, Never] = result {\n",
        "  let value = infallible(1)?\n",
        "  yield Ok(value)\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("failure typing is selected");
    let question_edges = resolution
        .body_types()
        .transfers()
        .iter()
        .filter(|transfer| transfer.kind() == ControlTransferKind::QuestionShortCircuit)
        .collect::<Vec<_>>();

    assert_eq!(question_edges.len(), 1);
    assert!(matches!(
        question_edges[0].target(),
        ControlRegionKey::Result(_)
    ));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn checked_integer_addition_records_operation_and_recoverable_fault_edge() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn double(value: i8) -> Res[i8, NumericError] = checked {\n",
        "  yield value + value\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("checked typing is selected");
    let transfer = resolution
        .body_types()
        .transfers()
        .iter()
        .find(|transfer| {
            transfer.kind() == ControlTransferKind::CheckedFault(NumericFaultCause::Overflow)
        })
        .expect("overflow edge is present");
    let ControlTransferSource::Expression(operation_key) = transfer.source() else {
        panic!("checked fault must be sourced from its operation")
    };
    let operation = match resolution.body_types().checked_numeric(*operation_key) {
        FactState::Known(operation) => operation,
        other => panic!("checked operation fact is not known: {other:?}"),
    };

    assert_eq!(operation.operator(), CheckedNumericOperator::Add);
    assert_eq!(operation.faults(), [NumericFaultCause::Overflow]);
    assert!(matches!(
        resolution
            .signatures()
            .types()
            .kind(operation.result())
            .unwrap(),
        TypeKind::Primitive(PrimitiveType::I8)
    ));
    assert!(matches!(transfer.target(), ControlRegionKey::Checked(_)));
    assert_eq!(transfer.exited_regions(), [transfer.target().clone()]);
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn checked_integer_operators_preserve_their_exact_fault_sets() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn signed_div(lhs: i8, rhs: i8) -> Res[i8, NumericError] = checked {\n",
        "  yield lhs / rhs\n",
        "}\n",
        "fn unsigned_div(lhs: u8, rhs: u8) -> Res[u8, NumericError] = checked {\n",
        "  yield lhs / rhs\n",
        "}\n",
        "fn remainder(lhs: i8, rhs: i8) -> Res[i8, NumericError] = checked {\n",
        "  yield lhs % rhs\n",
        "}\n",
        "fn negate(value: i8) -> Res[i8, NumericError] = checked {\n",
        "  yield -value\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("checked typing is selected");
    let body_types = resolution.body_types();
    let types = resolution.signatures().types();

    assert_eq!(body_types.checked_numeric_operations().count(), 4);
    assert!(
        body_types
            .checked_numeric_operations()
            .any(|(_, operation)| {
                operation.operator() == CheckedNumericOperator::Divide
                    && matches!(
                        types.kind(operation.result()).unwrap(),
                        TypeKind::Primitive(PrimitiveType::I8)
                    )
                    && operation.faults()
                        == [
                            NumericFaultCause::DivisionByZero,
                            NumericFaultCause::Overflow,
                        ]
            })
    );
    assert!(
        body_types
            .checked_numeric_operations()
            .any(|(_, operation)| {
                operation.operator() == CheckedNumericOperator::Divide
                    && matches!(
                        types.kind(operation.result()).unwrap(),
                        TypeKind::Primitive(PrimitiveType::U8)
                    )
                    && operation.faults() == [NumericFaultCause::DivisionByZero]
            })
    );
    assert!(
        body_types
            .checked_numeric_operations()
            .any(|(_, operation)| {
                operation.operator() == CheckedNumericOperator::Remainder
                    && operation.faults() == [NumericFaultCause::RemainderByZero]
            })
    );
    assert!(
        body_types
            .checked_numeric_operations()
            .any(|(_, operation)| {
                operation.operator() == CheckedNumericOperator::Negate
                    && operation.faults() == [NumericFaultCause::Overflow]
            })
    );
    assert_eq!(
        body_types
            .transfers()
            .iter()
            .filter(|transfer| matches!(transfer.kind(), ControlTransferKind::CheckedFault(_)))
            .count(),
        5
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn checked_body_resets_outer_question_authorization() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn source() -> Res[i64, Str] = Ok(1)\n",
        "fn invalid() -> Res[i64, NumericError] = result {\n",
        "  let value = checked { yield source()? }?\n",
        "  yield Ok(value)\n",
        "}\n",
    ));

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["Q_OUTSIDE_RESULT"]);
}

#[test]
fn return_and_callable_yield_keep_distinct_targets() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn choose(flag: Bool) -> i64 {\n",
        "  if flag { return 1 }\n",
        "  yield 2\n",
        "}\n",
    ));

    let analysis = session
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("control typing is selected");
    let return_edge = resolution
        .body_types()
        .transfers()
        .iter()
        .find(|transfer| transfer.kind() == ControlTransferKind::Return)
        .expect("return edge is present");
    let yield_edge = resolution
        .body_types()
        .transfers()
        .iter()
        .find(|transfer| transfer.kind() == ControlTransferKind::Yield)
        .expect("callable yield edge is present");

    assert!(matches!(
        return_edge.target(),
        ControlRegionKey::Callable(_)
    ));
    assert_eq!(return_edge.exited_regions().len(), 2);
    assert!(matches!(
        return_edge.exited_regions()[1],
        ControlRegionKey::IfBranch { .. }
    ));
    assert_eq!(yield_edge.exited_regions(), [yield_edge.target().clone()]);
    assert_eq!(return_edge.target(), yield_edge.target());
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn result_form_diagnostics_preserve_the_normative_partition() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected) in [
        (
            concat!(
                "fn source() -> Res[i64, Str] = Ok(1)\n",
                "fn invalid() -> Res[i64, Str] {\n",
                "  let value = source()?\n",
                "  yield Ok(value)\n",
                "}\n",
            ),
            "Q_OUTSIDE_RESULT",
        ),
        (
            concat!(
                "fn source() -> Res[i64, Str] = Ok(1)\n",
                "fn invalid() -> Res[i64, Str] = result {\n",
                "  let value = source()?\n",
                "  value\n",
                "}\n",
            ),
            "RESULT_MISSING_YIELD",
        ),
        (
            concat!(
                "fn from_res() -> Res[i64, Str] = Ok(1)\n",
                "fn from_opt() -> Opt[i64] = Some(2)\n",
                "fn invalid() -> Res[i64, Str] = result {\n",
                "  _ = from_res()?\n",
                "  _ = from_opt()?\n",
                "  yield Ok(0)\n",
                "}\n",
            ),
            "RESULT_BLOCK_MIXED_KINDS",
        ),
    ] {
        let mut rejected = session(source);
        rejected.resolve(&body_profile(), Some(&catalog)).unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            [expected],
            "source:\n{source}"
        );
    }
}

#[test]
fn guarded_match_preserves_false_and_nested_block_exit_paths() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn choose(input: Opt[i64]) -> i64 = match input {\n",
        "  Some(value) if true => {\n",
        "    let copied = value\n",
        "    yield copied\n",
        "  },\n",
        "  Some(value) => value,\n",
        "  None => 0,\n",
        "}\n",
    ));

    let analysis = session
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("branch typing is selected");
    let body_types = resolution.body_types();
    let block = body_types
        .regions()
        .find_map(|(key, region)| {
            matches!(key, ControlRegionKey::Block(_)).then(|| (key.clone(), region.clone()))
        })
        .expect("the guarded block arm has its own region");
    let arm = block
        .1
        .parent()
        .cloned()
        .expect("the block region has its match-arm parent");

    assert!(matches!(arm, ControlRegionKey::MatchArm { .. }));
    assert!(body_types.transfers().iter().any(|transfer| {
        transfer.kind() == ControlTransferKind::MatchGuardFalse
            && transfer.target() == &arm
            && transfer.exited_regions() == [arm.clone()]
    }));
    assert!(body_types.transfers().iter().any(|transfer| {
        transfer.kind() == ControlTransferKind::Yield
            && transfer.target() == &block.0
            && transfer.exited_regions() == [block.0.clone()]
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn unreachable_question_does_not_create_a_result_exit() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
        "fn choose() -> Res[i64, Str] = result {\n",
        "  stop()\n",
        "  let value = source()?\n",
        "  yield Ok(value)\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("failure typing is selected");

    assert!(
        !resolution
            .body_types()
            .transfers()
            .iter()
            .any(|transfer| transfer.kind() == ControlTransferKind::QuestionShortCircuit)
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn strict_expression_order_does_not_create_unreachable_failure_exits() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for source in [
        concat!(
            "fn stop() -> Never = stop()\n",
            "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
            "fn consume(first: Never, second: i64) -> Never = first\n",
            "fn choose() -> Res[i64, Str] = result {\n",
            "  consume(stop(), source()?)\n",
            "  yield Ok(0)\n",
            "}\n",
        ),
        concat!(
            "fn stop() -> Never = stop()\n",
            "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
            "fn consume(first: i64, second: Never) -> Never = second\n",
            "fn choose() -> Res[i64, Str] = result {\n",
            "  consume(second=stop(), first=source()?)\n",
            "  yield Ok(0)\n",
            "}\n",
        ),
        concat!(
            "fn stop() -> Never = stop()\n",
            "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
            "fn consume(value: (Never, i64)) -> Never = value.0\n",
            "fn choose() -> Res[i64, Str] = result {\n",
            "  consume((stop(), source()?))\n",
            "  yield Ok(0)\n",
            "}\n",
        ),
        concat!(
            "struct Pair {\n",
            "  field first: i64\n",
            "  field second: Never\n",
            "}\n",
            "fn stop() -> Never = stop()\n",
            "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
            "fn choose() -> Res[i64, Str] = result {\n",
            "  Pair { second: stop(), first: source()? }\n",
            "  yield Ok(0)\n",
            "}\n",
        ),
        concat!(
            "fn stop() -> Never = stop()\n",
            "fn fabricate[T]() -> T = stop()\n",
            "fn source() -> Res[Never, Str] = Err(\"boom\")\n",
            "fn consume[T](first: T, second: T) -> T = first\n",
            "fn choose() -> Res[i64, Str] = result {\n",
            "  consume(fabricate(), source()?)\n",
            "  yield Ok(0)\n",
            "}\n",
        ),
    ] {
        let mut session = session(source);
        let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().expect("failure typing is selected");
        let choose = resolution
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.spelling() == "choose")
            })
            .expect("choose declaration is collected");
        let choose_region = ControlRegionKey::Callable(choose.id());

        assert!(
            !resolution
                .body_types()
                .transfers()
                .iter()
                .any(|transfer| transfer.kind() == ControlTransferKind::QuestionShortCircuit),
            "source:\n{source}"
        );
        assert!(
            !resolution.body_types().transfers().iter().any(|transfer| {
                transfer.kind() == ControlTransferKind::NormalCompletion
                    && transfer.target() == &choose_region
            }),
            "source:\n{source}"
        );
        assert!(
            matches!(
                session.into_diagnostics(None),
                DiagnosticSnapshot::Accepted(_)
            ),
            "source:\n{source}"
        );
    }
}

#[test]
fn overload_selection_preserves_strict_argument_control_order() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn source() -> Res[i64, Str] = Err(\"boom\")\n",
        "overload fn consume(first: Never, second: i64) -> Never = first\n",
        "overload fn consume(first: Never, second: Str) -> Never = first\n",
        "fn choose() -> Res[i64, Str] = result {\n",
        "  consume(stop(), source()?)\n",
        "  yield Ok(0)\n",
        "}\n",
    ));

    let analysis = session
        .resolve(&overload_failure_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("overload typing is selected");

    assert!(
        !resolution
            .body_types()
            .transfers()
            .iter()
            .any(|transfer| { transfer.kind() == ControlTransferKind::QuestionShortCircuit })
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn deferred_application_prunes_only_unreachable_checked_exits() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn fabricate[T]() -> T = stop()\n",
        "fn finish(first: i8, second: Never) -> Never = second\n",
        "fn consume[T](first: T, second: T) -> T = first\n",
        "fn choose(lhs: i8, rhs: i8) -> Res[i8, NumericError] = checked {\n",
        "  consume(fabricate(), finish(lhs + rhs, stop()))\n",
        "  yield 0\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("checked typing is selected");
    let choose = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration
                .name()
                .is_some_and(|name| name.spelling() == "choose")
        })
        .expect("choose declaration is collected");
    let choose_region = ControlRegionKey::Callable(choose.id());

    assert_eq!(
        resolution.body_types().checked_numeric_operations().count(),
        1
    );
    assert!(
        !resolution
            .body_types()
            .transfers()
            .iter()
            .any(|transfer| { matches!(transfer.kind(), ControlTransferKind::CheckedFault(_)) })
    );
    assert!(!resolution.body_types().transfers().iter().any(|transfer| {
        transfer.kind() == ControlTransferKind::NormalCompletion
            && transfer.target() == &choose_region
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn no_success_checked_body_keeps_the_written_yield_type_without_a_fault_edge() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn inspect(value: i8) -> Unit {\n",
        "  let outcome = checked {\n",
        "    stop()\n",
        "    yield value + value\n",
        "  }\n",
        "  yield ()\n",
        "}\n",
    ));

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("checked typing is selected");
    let outcome = resolution
        .names()
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "outcome")
        .expect("checked result binding is resolved");
    let FactState::Known(outcome) = resolution.body_types().local(outcome.id()) else {
        panic!("checked result binding has no type")
    };
    let TypeKind::Nominal {
        head: NominalHead::Intrinsic(IntrinsicNominal::Res),
        substitution,
    } = resolution.signatures().types().kind(outcome.ty()).unwrap()
    else {
        panic!("checked result binding is not a Res")
    };
    let arguments = resolution
        .signatures()
        .types()
        .substitution(*substitution)
        .unwrap();

    assert!(matches!(
        resolution.signatures().types().kind(arguments[0]).unwrap(),
        TypeKind::Primitive(PrimitiveType::I8)
    ));
    assert_eq!(
        resolution.body_types().checked_numeric_operations().count(),
        1
    );
    assert!(
        !resolution
            .body_types()
            .transfers()
            .iter()
            .any(|transfer| matches!(transfer.kind(), ControlTransferKind::CheckedFault(_)))
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn unsupported_failure_inference_and_const_evaluation_are_capability_gaps() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, missing) in [
        (
            concat!(
                "fn source() -> Res[i64, Str] = Ok(1)\n",
                "fn infer() -> Unit {\n",
                "  let inferred = result {\n",
                "    let value = source()?\n",
                "    yield Ok(value)\n",
                "  }\n",
                "  yield ()\n",
                "}\n",
            ),
            TypecheckCapability::InferResultBlockType,
        ),
        (
            concat!(
                "fn add_one(value: i64) -> Res[i64, NumericError] = checked {\n",
                "  yield value + 1\n",
                "}\n",
            ),
            TypecheckCapability::EvaluateSelectedIntegerConstExpressions,
        ),
    ] {
        let mut unsupported = session(source);
        let analysis = unsupported
            .resolve(&body_profile(), Some(&catalog))
            .unwrap();

        assert!(
            analysis
                .trace()
                .missing()
                .any(|entry| { entry.capability() == CompileCapability::Typecheck(missing) })
        );
        assert!(matches!(
            unsupported.into_diagnostics(None),
            DiagnosticSnapshot::Accepted(_)
        ));
    }
}

#[test]
fn failure_diagnostic_plain_rendering_is_source_bound() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn source() -> Res[i64, Str] = Ok(1)\n",
        "fn invalid() -> Res[i64, Str] {\n",
        "  let value = source()?\n",
        "  yield Ok(value)\n",
        "}\n",
    ));

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(
        render_plain(&snapshot),
        concat!(
            "error[Q_OUTSIDE_RESULT] ? is only valid inside a result block\n",
            " --> main.eglur:3:14",
        )
    );
}

#[test]
fn bool_if_types_all_written_branches_and_preserves_never_paths() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn choose(flag: Bool) -> i64 = if flag { yield stop() } else { yield 2 }\n",
        "fn observe(flag: Bool) -> Unit {\n",
        "  if flag { 1 }\n",
        "  yield ()\n",
        "}\n",
    ));

    accepted
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    let mut mismatch =
        session("fn choose(flag: Bool) -> i64 = if flag { yield 1 } else { yield \"bad\" }\n");
    mismatch
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert_eq!(
        diagnostic_codes(&mismatch.into_diagnostics(None)),
        ["IF_BRANCH_TYPE_MISMATCH"]
    );
}

#[test]
fn branch_common_types_are_distinct_from_enclosing_return_expectations() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut inferred = session(concat!(
        "fn choose(flag: Bool, narrow: i8) -> i8 {\n",
        "  let selected = if flag { yield narrow } else { yield 1 }\n",
        "  yield selected\n",
        "}\n",
    ));
    inferred
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert!(matches!(
        inferred.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    for source in [
        concat!(
            "fn invalid(flag: Bool) -> i64 = ",
            "if flag { yield \"left\" } else { yield \"right\" }\n",
        ),
        concat!(
            "fn invalid(value: Opt[i64]) -> i64 = match value {\n",
            "  Some(_) => \"some\",\n",
            "  None => \"none\",\n",
            "}\n",
        ),
    ] {
        let mut rejected = session(source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            ["RETURN_TYPE_MISMATCH"],
            "source:\n{source}"
        );
    }
}

#[test]
fn branch_common_type_weakens_references_independently_of_source_order() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (then_value, else_value) in [("&mut value", "shared"), ("shared", "&mut value")] {
        let source = format!(
            concat!(
                "fn read(value: &i64) -> i64 = *value\n",
                "fn choose(flag: Bool) -> i64 {{\n",
                "  let mut value = 1\n",
                "  let shared = &value\n",
                "  let selected = if flag {{ yield {} }} else {{ yield {} }}\n",
                "  yield read(selected)\n",
                "}}\n",
            ),
            then_value, else_value
        );
        let mut accepted = session(&source);
        accepted
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert!(
            matches!(
                accepted.into_diagnostics(None),
                DiagnosticSnapshot::Accepted(_)
            ),
            "source:\n{source}"
        );
    }
}

#[test]
fn if_reports_condition_shape_and_value_context_errors() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected) in [
        (
            "fn invalid() -> i64 = if 1 { yield 1 } else { yield 2 }\n",
            "IF_CONDITION_NOT_BOOL",
        ),
        (
            "fn invalid(flag: Bool) -> i64 = if flag { yield 1 }\n",
            "IF_MISSING_ELSE_VALUE",
        ),
        (
            concat!(
                "fn invalid(flag: Bool) -> i64 = if flag { yield 1 } ",
                "else { yield 2 } unknown { yield 3 }\n",
            ),
            "IF_BOOL_UNKNOWN_BRANCH_FOR_BOOL",
        ),
    ] {
        let mut rejected = session(source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            [expected],
            "source:\n{source}"
        );
    }
}

#[test]
fn opt_match_types_payload_bindings_and_requires_complete_coverage() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(concat!(
        "fn value(input: Opt[i64]) -> i64 = match input {\n",
        "  Some(inner) => inner,\n",
        "  None => 0,\n",
        "}\n",
    ));

    let analysis = accepted
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    let inner = resolution
        .names()
        .locals()
        .iter()
        .find(|binding| binding.name().spelling() == "inner")
        .expect("Some payload binding is resolved");
    assert!(matches!(
        resolution.body_types().local(inner.id()),
        FactState::Known(local)
            if matches!(
                resolution.signatures().types().kind(local.ty()).unwrap(),
                TypeKind::Primitive(PrimitiveType::I64)
            ) && matches!(local.region(), ControlRegionKey::MatchArm { .. })
    ));
    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    let mut rejected = session(concat!(
        "fn value(input: Opt[i64]) -> i64 = match input {\n",
        "  Some(inner) => inner,\n",
        "}\n",
    ));
    rejected
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert_eq!(
        diagnostic_codes(&rejected.into_diagnostics(None)),
        ["MATCH_NON_EXHAUSTIVE"]
    );
}

#[test]
fn unsupported_match_coverage_is_a_capability_gap_not_a_known_fact() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = session(concat!(
        "fn value(input: Bool) -> i64 = match input {\n",
        "  true => 1,\n",
        "  false => 0,\n",
        "}\n",
    ));

    let analysis = session
        .resolve(&branching_profile(), Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::AnalyzePatternCoverage)
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn duplicate_pattern_bindings_are_language_diagnostics_not_fact_collisions() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (source, expected) in [
        (
            concat!(
                "fn invalid() -> i64 {\n",
                "  let (value, value) = (1, 2)\n",
                "  yield 0\n",
                "}\n",
            ),
            "LET_PATTERN_DUPLICATE_BINDING",
        ),
        (
            concat!(
                "fn invalid(input: Opt[(i64, i64)]) -> i64 = match input {\n",
                "  Some((value, value)) => 0,\n",
                "  None => 0,\n",
                "}\n",
            ),
            "PATTERN_DUPLICATE_BINDING",
        ),
    ] {
        let mut rejected = session(source);
        rejected
            .resolve(&branching_profile(), Some(&catalog))
            .unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            [expected],
            "source:\n{source}"
        );
    }
}

#[test]
fn bounded_inhabitance_accepts_only_proven_empty_match_scrutinees() {
    let mut accepted = session(concat!(
        "enum Empty {}\n",
        "struct Void { field impossible: Never }\n",
        "enum Loop { Next(Box[Loop]) }\n",
        "fn from_empty(value: Empty) -> Never = match value {}\n",
        "fn from_void(value: Void) -> Never = match value {}\n",
        "fn from_loop(value: Loop) -> Never = match value {}\n",
        "fn from_result(value: Res[Never, Never]) -> Never = match value {}\n",
        "fn from_tuple(value: (Never, i64)) -> Never = match value {}\n",
        "fn from_reference(value: &Never) -> Never = match value {}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = accepted.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    assert!(
        resolution
            .inhabitance()
            .facts()
            .any(|(_, fact)| fact == Inhabitance::Uninhabited)
    );
    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    for source in [
        concat!(
            "enum WithBase {\n  Stop\n  Next(Box[WithBase])\n}\n",
            "fn invalid(value: WithBase) -> Never = match value {}\n",
        ),
        "fn invalid(value: Opt[Never]) -> Never = match value {}\n",
        "fn invalid(value: Fn() -> Never) -> Never = match value {}\n",
        concat!(
            "enum Orbit[T] { Next(Box[Orbit[Opt[T]]]) }\n",
            "fn invalid(value: Orbit[i64]) -> Never = match value {}\n",
        ),
    ] {
        let mut rejected = session(source);
        rejected.resolve(&body_profile(), Some(&catalog)).unwrap();
        assert_eq!(
            diagnostic_codes(&rejected.into_diagnostics(None)),
            ["MATCH_NON_EXHAUSTIVE"],
            "source:\n{source}"
        );
    }
}

#[test]
fn strict_expressions_propagate_no_normal_continuation_without_skipping_source() {
    let mut accepted = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn consume(value: (Never, i64)) -> Str = \"unused\"\n",
        "fn main() -> Never {\n",
        "  consume((stop(), 1))\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    accepted.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    let mut rejected = session(concat!(
        "fn stop() -> Never = stop()\n",
        "fn need_i64(value: i64) -> Unit = ()\n",
        "fn main() -> Never {\n",
        "  stop()\n",
        "  need_i64(\"still checked\")\n",
        "}\n",
    ));
    rejected.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert_eq!(
        diagnostic_codes(&rejected.into_diagnostics(None)),
        ["CALL_ARG_TYPE_MISMATCH"]
    );
}

#[test]
fn exact_never_place_projections_have_no_normal_continuation() {
    let mut session = session(concat!(
        "struct Void { field impossible: Never }\n",
        "fn consume(value: Never) -> Str = \"unused\"\n",
        "fn from_field(value: Void) -> Never {\n",
        "  consume(value.impossible)\n",
        "}\n",
        "fn from_tuple(value: (Never, i64)) -> Never {\n",
        "  consume(value.0)\n",
        "}\n",
        "fn from_reference(value: &Never) -> Never {\n",
        "  consume(*value)\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn missing_inhabitance_support_is_observed_only_for_empty_matches() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut ordinary = session("fn main() -> i64 = 1\n");
    let ordinary_profile = body_profile_without(TypecheckCapability::AnalyzeInhabitance);
    let analysis = ordinary.resolve(&ordinary_profile, Some(&catalog)).unwrap();
    assert!(analysis.resolution().is_some());
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance)
    }));

    let mut empty_match = session(concat!(
        "enum Empty {}\n",
        "fn main(value: Empty) -> Never = match value {}\n",
    ));
    let analysis = empty_match
        .resolve(&ordinary_profile, Some(&catalog))
        .unwrap();
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance)
    }));
}

#[test]
fn poisoned_nominal_components_keep_inhabitance_analysis_conservative() {
    let mut session = session(concat!(
        "struct Broken { field value: Missing }\n",
        "enum BrokenChoice {\n  Base\n  Broken(Missing)\n}\n",
        "fn invalid(value: Broken) -> Never = match value {}\n",
        "fn invalid_choice(value: BrokenChoice) -> Never = match value {}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    assert!(
        resolution
            .inhabitance()
            .facts()
            .filter(|(_, fact)| *fact == Inhabitance::Unknown)
            .count()
            >= 2
    );
    assert!(
        diagnostic_codes(&session.into_diagnostics(None))
            .into_iter()
            .any(|code| code == "NAME_NOT_FOUND")
    );
}

fn body_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("body-types-v1").unwrap(),
        body_capabilities(),
    )
    .unwrap()
}

fn body_profile_without(missing: TypecheckCapability) -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("body-types-gap-v1").unwrap(),
        body_capabilities()
            .into_iter()
            .filter(|capability| match capability {
                CompileCapability::Typecheck(TypecheckCapability::ResolveMembers)
                    if matches!(
                        missing,
                        TypecheckCapability::CheckBodies
                            | TypecheckCapability::ResolveTraitEnvironment
                    ) =>
                {
                    false
                }
                CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance)
                    if matches!(
                        missing,
                        TypecheckCapability::CheckBodies
                            | TypecheckCapability::ResolveTraitEnvironment
                    ) =>
                {
                    false
                }
                CompileCapability::Typecheck(
                    TypecheckCapability::CheckFailureFlow
                    | TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
                ) if matches!(
                    missing,
                    TypecheckCapability::CheckBodies | TypecheckCapability::ResolveTraitEnvironment
                ) =>
                {
                    false
                }
                CompileCapability::Typecheck(
                    TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
                ) if missing == TypecheckCapability::CheckFailureFlow => false,
                CompileCapability::Typecheck(TypecheckCapability::CheckBodies)
                    if missing == TypecheckCapability::ResolveTraitEnvironment =>
                {
                    false
                }
                CompileCapability::Typecheck(capability) => *capability != missing,
                CompileCapability::Resolve(_) => true,
            }),
    )
    .unwrap()
}

fn session(source: &str) -> CompilerSession {
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    CompilerSession::build(request, SessionOptions::default()).unwrap()
}

fn file(path: &str, source: &str) -> FileInput {
    FileInput::new(path, source.as_bytes().to_vec())
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

#[test]
fn local_generic_calls_produce_complete_substitutions_and_pending_bounds() {
    let mut session = session(concat!(
        "fn id[T](x: T) -> T = x\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let y = id(7)\n",
        "  yield Ok(to_str(y))\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().expect("body typing is selected");
    let to_str = CanonicalTarget::External(ExternalDefId::new("std::prelude::to_str").unwrap());
    let FactState::Known(DeclarationSignature::Callable(to_str_callable)) =
        resolution.signatures().declaration(&to_str)
    else {
        panic!("to_str callable signature is missing");
    };
    let to_str_generic = &resolution
        .signatures()
        .callable(*to_str_callable)
        .unwrap()
        .generic_parameters()[0];
    let FactState::Known(to_str_generic) =
        resolution.signatures().generic_parameter(to_str_generic)
    else {
        panic!("to_str generic signature is missing");
    };
    assert_eq!(to_str_generic.bounds().len(), 1);
    let obligation_count = resolution.body_types().obligations().len();
    let obligation_subject_is_i64 =
        resolution
            .body_types()
            .obligations()
            .first()
            .is_some_and(|obligation| {
                matches!(
                    resolution
                        .signatures()
                        .types()
                        .kind(obligation.subject())
                        .unwrap(),
                    TypeKind::Primitive(PrimitiveType::I64)
                )
            });
    let obligation_target = resolution
        .body_types()
        .obligations()
        .first()
        .map(|obligation| obligation.trait_ref().declaration().clone());
    let fact_count = resolution.body_types().fact_count();
    let snapshot = session.into_diagnostics(None);

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
    assert_eq!(obligation_count, 1, "expected to_str Display obligation");
    assert!(obligation_subject_is_i64);
    assert!(matches!(
        obligation_target.as_ref().unwrap(),
        CanonicalTarget::External(declaration)
            if declaration.as_str() == "std::prelude::Display"
    ));
    assert!(fact_count > 0);
}

#[test]
fn underconstrained_generic_call_reports_the_expression_specific_error() {
    let mut session = session(concat!(
        "fn none_of[T]() -> Opt[T] = None\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let x = none_of()\n",
        "  yield Ok(to_str(x))\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["TYPE_ARG_INFERENCE_FAILED"]
    );
}

#[test]
fn call_argument_mismatch_uses_the_parameter_type_contract() {
    let mut session = session(concat!(
        "fn need_i64(x: i64) -> i64 { yield x }\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let x = need_i64(\"not an int\")\n",
        "  yield Ok(\"x\")\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "CALL_ARG_TYPE_MISMATCH");
    assert_eq!(
        diagnostics[0].data(),
        &[
            ("actual_type", DiagnosticDataValue::from("Str")),
            ("expected_type", DiagnosticDataValue::from("i64")),
            ("parameter", DiagnosticDataValue::from("x")),
            ("reason", DiagnosticDataValue::from("parameter_type")),
        ]
        .into_iter()
        .map(|(key, value)| (Box::<str>::from(key), value))
        .collect()
    );
    assert_eq!(diagnostics[0].related().len(), 1);
}

#[test]
fn expression_body_return_mismatch_has_expected_and_actual_types() {
    let mut session = session("fn bad() -> i64 = true\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "RETURN_TYPE_MISMATCH");
    assert_eq!(
        diagnostics[0].data().get("expected_type"),
        Some(&DiagnosticDataValue::from("i64"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Bool"))
    );
}

#[test]
fn diagnostic_types_distinguish_same_named_module_declarations() {
    let request = FileSetRequest::new(
        [
            file("alpha.eglur", "pub struct Item {}\n"),
            file("beta.eglur", "pub struct Item {}\n"),
            file(
                "main.eglur",
                "fn bad(value: alpha::Item) -> beta::Item = value\n",
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    assert_eq!(
        diagnostics[0].data().get("expected_type"),
        Some(&DiagnosticDataValue::from("beta::Item"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("alpha::Item"))
    );
}

#[test]
fn diagnostic_types_prefer_use_site_import_aliases() {
    let request = FileSetRequest::new(
        [
            file("alpha.eglur", "pub struct Item {}\n"),
            file(
                "main.eglur",
                concat!(
                    "import alpha::Item as Renamed\n",
                    "fn bad(value: Renamed) -> Bool = value\n",
                ),
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Renamed"))
    );
}

#[test]
fn diagnostic_types_break_equal_length_alias_ties_lexically() {
    let request = FileSetRequest::new(
        [
            file("alpha.eglur", "pub struct Item {}\n"),
            file(
                "main.eglur",
                concat!(
                    "import alpha::Item as Zed\n",
                    "import alpha::Item as Aye\n",
                    "fn bad(value: Zed) -> Bool = value\n",
                ),
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Aye"))
    );
}

#[test]
fn diagnostic_types_expand_transparent_type_aliases() {
    let request = FileSetRequest::new(
        [
            file("alpha.eglur", "pub struct Item {}\n"),
            file(
                "main.eglur",
                concat!(
                    "type Alias = alpha::Item\n",
                    "fn bad(value: Alias) -> Bool = value\n",
                ),
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("alpha::Item"))
    );
}

#[test]
fn diagnostic_types_qualify_nominals_shadowed_by_generics() {
    let mut session = session(concat!(
        "struct Item {}\n",
        "fn bad[Item](value: Item) -> main::Item = value\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    assert_eq!(
        diagnostics[0].data().get("expected_type"),
        Some(&DiagnosticDataValue::from("main::Item"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Item"))
    );
}

#[test]
fn diagnostic_types_sort_nested_union_members_by_rendered_spelling() {
    fn actual_type(source: &str) -> DiagnosticDataValue {
        let mut session = session(source);
        let catalog = TrustedCatalog::first_slice().unwrap();

        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        let diagnostics = snapshot.diagnostics();

        assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
        diagnostics[0].data()["actual_type"].clone()
    }

    let forward = actual_type(concat!(
        "struct Zulu {}\n",
        "struct Alpha {}\n",
        "struct Wrap[T] {}\n",
        "fn bad(value: Wrap[Res[i64, Union[Zulu, Alpha]]]) -> Bool = value\n",
    ));
    let reverse = actual_type(concat!(
        "struct Alpha {}\n",
        "struct Zulu {}\n",
        "struct Wrap[T] {}\n",
        "fn bad(value: Wrap[Res[i64, Union[Alpha, Zulu]]]) -> Bool = value\n",
    ));

    assert_eq!(
        forward,
        DiagnosticDataValue::from("Wrap[Res[i64, Union[Alpha, Zulu]]]")
    );
    assert_eq!(reverse, forward);
}

#[test]
fn trait_diagnostic_types_include_resolved_type_arguments() {
    let mut session = session(concat!(
        "trait Rel[T] {\n",
        "  type Item\n",
        "}\n",
        "struct Value {}\n",
        "impl Rel[i64] for Value {}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(
        diagnostic_codes(&snapshot),
        ["TRAIT_IMPL_ASSOC_TYPE_MISMATCH"]
    );
    assert_eq!(
        diagnostics[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Rel[i64]"))
    );
}

#[test]
fn concrete_projection_contextually_types_an_integer_literal_before_defaulting() {
    let mut session = session(concat!(
        "trait HasItem { type Item }\n",
        "struct Count {}\n",
        "impl HasItem for Count { type Item = u8 }\n",
        "fn consume[T: HasItem](value: T::Item, carrier: T) -> T::Item = value\n",
        "fn main() -> u8 = consume(255, Count{})\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    assert!(resolution.trait_environment().normalization_cache_len() > 0);

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn generic_projection_remains_symbolic_while_a_nominal_substitution_normalizes_forward() {
    let mut session = session(concat!(
        "trait HasItem { type Item }\n",
        "struct Wrap[T] {}\n",
        "impl[T: HasItem] HasItem for Wrap[T] { type Item = T::Item }\n",
        "fn consume[C: HasItem](carrier: C, value: C::Item) -> C::Item = value\n",
        "fn bridge[T: HasItem](carrier: Wrap[T], value: T::Item) -> T::Item = ",
        "consume(carrier, value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn current_generic_impl_evidence_normalizes_self_projection_in_method_bodies() {
    let mut session = session(concat!(
        "trait Identity {\n",
        "  type Item\n",
        "  fn identity(self: &Self, value: Self::Item) -> Self::Item\n",
        "}\n",
        "impl[T: Identity] Identity for T {\n",
        "  type Item = T\n",
        "  fn identity(self: &Self, value: T) -> Self::Item = value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn projected_local_annotations_normalize_before_contextual_literal_typing() {
    let mut session = session(concat!(
        "trait Factory {\n",
        "  type Item\n",
        "  fn make(self: &Self) -> Self::Item\n",
        "}\n",
        "struct Count {}\n",
        "impl Factory for Count {\n",
        "  type Item = u8\n",
        "  fn make(self: &Self) -> Self::Item {\n",
        "    let value: Self::Item = 255\n",
        "    yield value\n",
        "  }\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn projection_lookup_reports_the_missing_bound_instead_of_a_literal_mismatch() {
    let mut session = session(concat!(
        "trait HasItem { type Item }\n",
        "struct Missing {}\n",
        "fn consume[T: HasItem](carrier: T, value: T::Item) -> T::Item = value\n",
        "fn main() -> i64 = consume(Missing{}, 1)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(diagnostic_codes(&snapshot), ["TRAIT_BOUND_NOT_SATISFIED"]);
    assert!(!diagnostic_codes(&snapshot).contains(&"CALL_ARG_TYPE_MISMATCH"));
}

#[test]
fn projection_lookup_validates_the_selected_impl_bounds() {
    let mut session = session(concat!(
        "trait Marker {}\n",
        "trait HasItem { type Item }\n",
        "struct Wrap[T] {}\n",
        "impl[T: Marker] HasItem for Wrap[T] { type Item = u8 }\n",
        "fn consume[C: HasItem](value: C::Item, carrier: C) -> C::Item = value\n",
        "fn bridge[T](carrier: Wrap[T]) -> u8 = consume(1, carrier)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(diagnostic_codes(&snapshot), ["TRAIT_BOUND_NOT_SATISFIED"]);
    assert_eq!(
        snapshot.diagnostics()[0].data().get("trait"),
        Some(&DiagnosticDataValue::from("Marker"))
    );
    assert_eq!(
        snapshot.diagnostics()[0].data().get("type"),
        Some(&DiagnosticDataValue::from("T"))
    );
}

#[test]
fn poisoned_associated_binding_suppresses_a_derivative_projection_error() {
    let mut session = session(concat!(
        "trait HasItem { type Item }\n",
        "struct Broken {}\n",
        "impl HasItem for Broken {}\n",
        "fn consume[T: HasItem](carrier: T, value: T::Item) -> T::Item = value\n",
        "fn main() -> i64 = consume(Broken{}, 1)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert_eq!(
        diagnostic_codes(&snapshot),
        ["TRAIT_IMPL_ASSOC_TYPE_MISMATCH"]
    );
    assert!(!diagnostic_codes(&snapshot).contains(&"TRAIT_BOUND_NOT_SATISFIED"));
}

#[test]
fn rejected_impl_header_does_not_participate_in_projection_lookup() {
    let mut session = session(concat!(
        "trait HasItem { type Item }\n",
        "struct Value {}\n",
        "impl HasItem for Value { type Item = u8 }\n",
        "impl HasItem for Value {}\n",
        "fn consume[T: HasItem](carrier: T, value: T::Item) -> T::Item = value\n",
        "fn main() -> u8 = consume(Value{}, 255)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let main = resolution
        .declarations()
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|name| name.text() == "main"))
        .expect("main declaration exists")
        .id();
    assert!(matches!(
        resolution.body_types().body(main),
        FactState::Known(_)
    ));

    let snapshot = session.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["TRAIT_IMPL_DUPLICATE"]);
}

#[test]
fn impl_source_order_does_not_change_projection_normalization() {
    fn analyze_source(first_impl_first: bool) -> (usize, usize, Vec<String>) {
        let implementations = if first_impl_first {
            concat!(
                "impl HasItem for First { type Item = i8 }\n",
                "impl HasItem for Second { type Item = u8 }\n",
            )
        } else {
            concat!(
                "impl HasItem for Second { type Item = u8 }\n",
                "impl HasItem for First { type Item = i8 }\n",
            )
        };
        let source = format!(
            "trait HasItem {{ type Item }}\n\
             struct First {{}}\n\
             struct Second {{}}\n\
             {implementations}\
             fn consume[T: HasItem](carrier: T, value: T::Item) -> T::Item = value\n\
             fn first() -> i8 = consume(First{{}}, 1)\n\
             fn second() -> u8 = consume(Second{{}}, 1)\n"
        );
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();

        let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let resolution = analysis.resolution().unwrap();
        let metrics = (
            resolution.trait_environment().fact_count(),
            resolution.trait_environment().normalization_cache_len(),
        );
        let codes = diagnostic_codes(&session.into_diagnostics(None))
            .into_iter()
            .map(str::to_owned)
            .collect();
        (metrics.0, metrics.1, codes)
    }

    assert_eq!(analyze_source(true), analyze_source(false));
}

#[test]
fn diagnostic_types_render_bound_self_projections_source_like() {
    let mut session = session(concat!(
        "trait Carrier {\n",
        "  type Item\n",
        "  fn bad(self: &Self, value: Self::Item) -> i64 = value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostic_codes(&snapshot), ["RETURN_TYPE_MISMATCH"]);
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Self::Item"))
    );
}

#[test]
fn block_body_fallthrough_is_checked_against_the_declared_result() {
    let mut session = session(concat!("fn bad() -> i64 {\n", "  let value = 1\n", "}\n",));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "RETURN_TYPE_MISMATCH");
    assert_eq!(
        diagnostics[0].data().get("expected_type"),
        Some(&DiagnosticDataValue::from("i64"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Unit"))
    );
}

#[test]
fn nested_block_yield_uses_the_block_context_not_the_function_result() {
    let mut session = session(concat!(
        "fn valid() -> i64 {\n",
        "  let flag: Bool = { yield true }\n",
        "  yield 1\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn nested_block_mismatch_is_owned_by_the_enclosing_call_slot() {
    let mut session = session(concat!(
        "fn need_i64(value: i64) -> i64 = value\n",
        "fn invalid() -> i64 = need_i64({ yield true })\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["CALL_ARG_TYPE_MISMATCH"]
    );
}

#[test]
fn return_in_a_const_initializer_is_outside_a_function() {
    let mut session = session(concat!(
        "const VALUE: i64 = {\n",
        "  return 1\n",
        "  yield 0\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["RETURN_OUTSIDE_FUNCTION"]
    );
}

#[test]
fn reserved_constructors_use_expected_nominal_context_and_record_invocations() {
    let mut session = session(concat!(
        "fn maybe() -> Opt[i64] = None\n",
        "fn make_result() -> Res[i64, Str] = Ok(1)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();

    assert!(
        resolution
            .signatures()
            .types()
            .types()
            .any(|(_, kind)| matches!(
                kind,
                TypeKind::Nominal {
                    head: NominalHead::Intrinsic(IntrinsicNominal::Opt | IntrinsicNominal::Res),
                    ..
                }
            ))
    );
    assert_eq!(resolution.body_types().invocations().len(), 2);
    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn expected_results_and_explicit_arguments_complete_local_generic_calls() {
    let mut session = session(concat!(
        "fn id[T](value: T) -> T = value\n",
        "fn none_of[T]() -> Opt[T] = None\n",
        "fn inferred() -> Opt[i64] = none_of()\n",
        "fn explicit() -> Str = id::[Str](\"value\")\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn error_union_inference_binds_only_complete_union_positions() {
    let mut session = session(concat!(
        "struct IoError {}\n",
        "struct ParseError {}\n",
        "fn pass[E](value: Res[i64, E]) -> Res[i64, E] = value\n",
        "fn nested[E](value: Res[i64, Union[E, ParseError]]) -> ",
        "Res[i64, Union[E, ParseError]] = value\n",
        "fn whole(value: Res[i64, Union[IoError, ParseError]]) -> ",
        "Res[i64, Union[IoError, ParseError]] = pass(value)\n",
        "fn explicit(value: Res[i64, Union[IoError, ParseError]]) -> ",
        "Res[i64, Union[IoError, ParseError]] = nested::[IoError](value)\n",
        "fn independent(error: IoError, value: Res[i64, Union[IoError, ParseError]]) -> ",
        "Res[i64, Union[IoError, ParseError]] = choose(error, value)\n",
        "fn choose[E](_error: E, value: Res[i64, Union[E, ParseError]]) -> ",
        "Res[i64, Union[E, ParseError]] = value\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);

    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn nested_error_union_parameters_are_underconstrained_in_any_member_order() {
    fn diagnostics(actual_union: &str) -> Vec<String> {
        let source = format!(
            concat!(
                "struct IoError {{}}\n",
                "struct ParseError {{}}\n",
                "fn nested[E](value: Res[i64, Union[E, ParseError]]) -> ",
                "Res[i64, Union[E, ParseError]] = value\n",
                "fn bad(value: Res[i64, {0}]) -> Res[i64, {0}] = nested(value)\n",
            ),
            actual_union,
        );
        let mut session = session(&source);
        let catalog = TrustedCatalog::first_slice().unwrap();

        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        diagnostic_codes(&session.into_diagnostics(None))
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    let forward = diagnostics("Union[IoError, ParseError]");
    let reverse = diagnostics("Union[ParseError, IoError]");

    assert_eq!(forward, ["TYPE_ARG_INFERENCE_FAILED"]);
    assert_eq!(reverse, forward);
}

#[test]
fn call_shape_precedes_argument_typing() {
    let mut session = session(concat!(
        "fn choose(first: i64, second: i64) -> i64 = first\n",
        "fn bad() -> i64 = choose(second=1, \"not checked\")\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["CALL_POSITIONAL_AFTER_NAMED"]
    );
}

#[test]
fn mutable_struct_fields_and_explicit_references_keep_place_identity() {
    let mut session = session(concat!(
        "struct Cell[T] { field mut value: T }\n",
        "fn main() -> i64 {\n",
        "  let mut cell = Cell { value: 1 }\n",
        "  cell.value = 2\n",
        "  let reference = &mut cell.value\n",
        "  *reference = 3\n",
        "  yield cell.value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let fact_count = resolution.body_types().fact_count();
    let snapshot = session.into_diagnostics(None);

    assert!(fact_count > 10);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn mutable_references_weaken_to_shared_call_parameters() {
    let mut session = session(concat!(
        "fn observe(_value: &i64) -> Unit = ()\n",
        "fn bridge!(value: &mut i64) -> Unit = observe(value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn generic_inference_preserves_mutable_to_shared_reference_weakening() {
    let mut session = session(concat!(
        "fn observe[T](_value: &T) -> Unit = ()\n",
        "fn bridge![T](value: &mut T) -> Unit = observe(value)\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn never_compatibility_does_not_lift_through_result_success_types() {
    let mut session = session("fn invalid(value: Res[Never, Str]) -> Res[i64, Str] = value\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "RETURN_TYPE_MISMATCH");
    assert_eq!(
        diagnostics[0].data().get("expected_type"),
        Some(&DiagnosticDataValue::from("Res[i64, Str]"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Res[Never, Str]"))
    );
}

#[test]
fn compatible_local_reassignment_stays_in_the_body_typing_slice() {
    let mut session = session(concat!(
        "fn main() -> i64 {\n",
        "  let mut value = 1\n",
        "  value = 2\n",
        "  yield value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(analysis.trace().missing().next().is_none());
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn missing_body_support_keeps_the_resolution_product_unperformed() {
    let mut session = session("fn main() -> i64 = 1\n");
    let catalog = TrustedCatalog::first_slice().unwrap();
    let profile = body_profile_without(TypecheckCapability::CheckBodies);

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();

    assert!(analysis.resolution().is_none());
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckBodies)
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn missing_trait_environment_support_stops_before_body_typing() {
    let mut session = session("fn main() -> i64 = 1\n");
    let catalog = TrustedCatalog::first_slice().unwrap();
    let profile = body_profile_without(TypecheckCapability::ResolveTraitEnvironment);

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();

    assert!(analysis.resolution().is_none());
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability()
            == CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment)
    }));
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckBodies)
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn unavailable_impl_binding_keeps_the_trait_environment_unperformed() {
    let mut session = session(concat!(
        "import std::future::Missing\n",
        "trait HasItem { type Item }\n",
        "struct Value {}\n",
        "impl HasItem for Value { type Item = Missing }\n",
        "fn main() -> Unit = ()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert!(
        analysis.resolution().is_none(),
        "unexpected completed resolution; trace: {:?}",
        analysis.trace().entries()
    );
    assert!(analysis.trace().entries().iter().any(|entry| {
        entry.capability()
            == CompileCapability::Resolve(ResolveCapability::ResolveCompleteStandardLibraryNames)
    }));
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn missing_member_support_does_not_invent_a_type_error() {
    let mut session = session(concat!(
        "struct Cell { field value: i64 }\n",
        "fn main() -> i64 {\n",
        "  let cell = Cell { value: 1 }\n",
        "  yield cell.value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();
    let profile = body_profile_without(TypecheckCapability::ResolveMembers);

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();

    assert!(analysis.resolution().is_some());
    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::ResolveMembers)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn incompatible_assignment_reports_expected_and_actual_types() {
    let mut session = session(concat!(
        "fn main() -> i64 {\n",
        "  let mut value = 1\n",
        "  value = true\n",
        "  yield value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    assert_eq!(diagnostic_codes(&snapshot), ["ASSIGNMENT_TYPE_MISMATCH"]);
    let diagnostic = &snapshot.diagnostics()[0];
    assert_eq!(
        diagnostic.data().get("expected_type"),
        Some(&DiagnosticDataValue::from("i64"))
    );
    assert_eq!(
        diagnostic.data().get("actual_type"),
        Some(&DiagnosticDataValue::from("Bool"))
    );
}

#[test]
fn assignment_mismatch_contexts_preserve_payload_span_and_poison() {
    let cases = [
        (
            "annotated-local",
            "fn invalid() -> Unit { let value: i64 = true\n _ = value }\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "mutable-local",
            "fn invalid() -> Unit { let mut value = 0\n value = true }\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "mutable-field",
            "struct Cell { field mut value: i64 }\nfn invalid!(cell: &mut Cell) -> Unit { cell.value = true }\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "explicit-dereference",
            "fn invalid!(value: &mut i64) -> Unit { *value = true }\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "const-initializer",
            "const VALUE: i64 = true\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "alias-annotation",
            "type Count = i64\nfn invalid() -> Unit { let value: Count = true\n _ = value }\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "tuple-annotation",
            "fn invalid() -> Unit { let (left, right): (i64, i64) = (1, true)\n _ = left\n _ = right }\n",
            "(i64, i64)",
            "(i64, Bool)",
            "(1, true)",
        ),
        (
            "wildcard-annotation",
            "fn invalid() -> Unit { let _: i64 = true }\n",
            "i64",
            "Bool",
            "true",
        ),
        (
            "reference-binding",
            "fn invalid!(value: &mut i64) -> Unit { let other: &mut Bool = value\n _ = other }\n",
            "&mut Bool",
            "&mut i64",
            "value",
        ),
    ];
    for (name, source, expected, actual, blame) in cases {
        let mut session = session(source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        assert_eq!(
            diagnostic_codes(&snapshot),
            ["ASSIGNMENT_TYPE_MISMATCH"],
            "{name}"
        );
        let diagnostic = &snapshot.diagnostics()[0];
        assert_eq!(
            diagnostic.data().get("expected_type"),
            Some(&DiagnosticDataValue::from(expected)),
            "{name}"
        );
        assert_eq!(
            diagnostic.data().get("actual_type"),
            Some(&DiagnosticDataValue::from(actual)),
            "{name}"
        );
        let range = diagnostic
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            source[range.start().as_usize()..range.end().as_usize()].trim(),
            blame,
            "{name}"
        );
    }
}

#[test]
fn assignment_mismatch_retains_validation_precedence() {
    for (source, expected) in [
        (
            "fn invalid() -> Unit { let value = 1\n value = true }",
            "IMMUTABLE_BINDING_ASSIGN",
        ),
        (
            "fn value() -> i64 = 1\nfn invalid() -> Unit { value() = true }",
            "ASSIGN_TARGET_NOT_ASSIGNABLE",
        ),
        (
            "fn value(x: i64) -> i64 = x\nfn invalid() -> Unit { let value: i64 = value(true) }",
            "CALL_ARG_TYPE_MISMATCH",
        ),
        (
            "fn invalid() -> Unit { let value: i64 = absent }",
            "NAME_NOT_FOUND",
        ),
        (
            "fn invalid() -> Unit { let (left, right): i64 = true }",
            "ASSIGNMENT_TYPE_MISMATCH",
        ),
        (
            "fn invalid() -> Unit { let (left, right): i64 = 1 }",
            "LET_PATTERN_TYPE_MISMATCH",
        ),
        (
            "fn invalid() -> Unit { let value = 1\n let value: i64 = true }",
            "LOCAL_BINDING_REDECLARATION",
        ),
        (
            "fn invalid() -> Unit { let (value, value): i64 = true }",
            "LET_PATTERN_DUPLICATE_BINDING",
        ),
        (
            "fn invalid() -> Unit { let value: i8 = 128 }",
            "INT_OVERFLOW",
        ),
        (
            "fn invalid() -> Unit { let (head, ..): i64 = true\n _ = head }",
            "LET_PATTERN_UNSUPPORTED",
        ),
        (
            "fn invalid() -> Unit { let (head, (tail, ..)): i64 = true\n _ = tail }",
            "LET_PATTERN_UNSUPPORTED",
        ),
    ] {
        let mut session = session(source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        session
            .resolve(&body_profile(), Some(&catalog))
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn assignment_compatibility_preserves_aliases_reference_weakening_and_never() {
    let source = concat!(
        "type Count = i64\n",
        "fn local() -> i64 { let mut value: Count = 1\n value = 2\n _ = true\n yield value }\n",
        "fn reference!(value: &mut i64) -> Unit { let read: &i64 = value\n _ = read }\n",
        "fn diverge(value: Never) -> Unit { let slot: i64 = value\n _ = slot }\n",
    );
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn assignment_checks_precede_ownership_and_preserve_target_rejections() {
    for (source, expected) in [
        (
            "fn invalid() -> Unit { let value = 1\n let mut root = own value\n root = true }",
            "ASSIGNMENT_TYPE_MISMATCH",
        ),
        (
            "fn invalid() -> Unit { let value = 1\n let mut root = own value\n let other = 2\n root = own other }",
            "OWNERSHIP_DISCARD_FORBIDDEN",
        ),
        (
            "struct Cell { field value: i64 }\nfn invalid!(cell: &mut Cell) -> Unit { cell.value = true }",
            "IMMUTABLE_BINDING_ASSIGN",
        ),
        (
            "struct Cell { prop value: i64 { fn get_value(self: &Self) -> i64 = 0 } }\nfn invalid!(cell: &mut Cell) -> Unit { cell.value = true }",
            "PROPERTY_ASSIGN_FORBIDDEN",
        ),
        (
            "fn invalid(value: i64) -> Unit { let slot: Never = value }",
            "ASSIGNMENT_TYPE_MISMATCH",
        ),
    ] {
        let mut session = session(source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap_or_else(|error| panic!("{source}: {error:?}"));
        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn direct_call_of_a_non_callable_value_is_an_explicit_support_gap() {
    let mut session = session(concat!(
        "const VALUE: i64 = 1\n",
        "fn invalid() -> i64 = VALUE()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn member_calls_are_classified_as_method_resolution_gaps() {
    let mut session = session(concat!(
        "struct Cell { field value: i64 }\n",
        "fn invalid(cell: Cell) -> i64 = cell.read()\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert!(analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::ResolveMethods)
    }));
    assert!(!analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)
    }));
    assert!(diagnostic_codes(&session.into_diagnostics(None)).is_empty());
}

#[test]
fn generic_enum_variants_infer_from_payloads_and_expected_results() {
    let mut session = session(concat!(
        "enum Choice[T] {\n",
        "  One(T)\n",
        "  Named { value: T }\n",
        "  Empty\n",
        "}\n",
        "fn one() -> Choice[i64] = Choice::One(1)\n",
        "fn named() -> Choice[Str] = Choice::Named { value: \"x\" }\n",
        "fn empty() -> Choice[i64] = Choice::Empty\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn variant_constructor_payload_forms_are_distinct() {
    fn source(expression: &str) -> String {
        format!(
            "enum Token {{\n  End\n  Empty()\n  Pair(i64)\n  Named {{ value: i64 }}\n}}\nfn make() -> Token = {expression}\n"
        )
    }

    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut accepted = session(&source("Token::End"));
    accepted.resolve(&body_profile(), Some(&catalog)).unwrap();
    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));

    for (expression, constructor, expected_form, actual_form) in [
        ("Token::End()", "Token::End", "unit", "positional"),
        ("Token::End {}", "Token::End", "unit", "named"),
        ("Token::Empty", "Token::Empty", "positional", "unit"),
        (
            "Token::Pair { value: 1 }",
            "Token::Pair",
            "positional",
            "named",
        ),
        ("Token::Named", "Token::Named", "named", "unit"),
        ("Token::Named(1)", "Token::Named", "named", "positional"),
    ] {
        let mut rejected = session(&source(expression));
        rejected.resolve(&body_profile(), Some(&catalog)).unwrap();
        let snapshot = rejected.into_diagnostics(None);
        let diagnostics = snapshot.diagnostics();

        assert_eq!(diagnostics.len(), 1, "constructor expression: {expression}");
        assert_eq!(
            diagnostics[0].code().as_str(),
            "CONSTRUCTOR_PAYLOAD_FORM_MISMATCH",
            "constructor expression: {expression}"
        );
        assert_eq!(
            diagnostics[0].data(),
            &[
                ("actual_form", DiagnosticDataValue::from(actual_form)),
                ("constructor", DiagnosticDataValue::from(constructor)),
                ("expected_form", DiagnosticDataValue::from(expected_form)),
            ]
            .into_iter()
            .map(|(key, value)| (Box::<str>::from(key), value))
            .collect(),
            "constructor expression: {expression}"
        );
    }
}

#[test]
fn valid_variant_constructor_payload_forms_remain_accepted() {
    let mut session = session(concat!(
        "enum Token {\n",
        "  End\n",
        "  Empty()\n",
        "  Pair(i64)\n",
        "  Named { value: i64 }\n",
        "}\n",
        "fn unit() -> Token = Token::End\n",
        "fn empty_positional() -> Token = Token::Empty()\n",
        "fn positional() -> Token = Token::Pair(1)\n",
        "fn named() -> Token = Token::Named { value: 1 }\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn qualified_braced_forms_share_struct_and_variant_typing() {
    let request = FileSetRequest::new(
        [
            file(
                "dep.eglur",
                concat!(
                    "pub struct Cell[T] { pub field value: T }\n",
                    "pub enum Choice[T] { Named { value: T } }\n",
                ),
            ),
            file("bridge.eglur", "pub import dep::Cell\n"),
            file(
                "main.eglur",
                concat!(
                    "fn cell() -> bridge::Cell[i64] = bridge::Cell { value: 1 }\n",
                    "fn choice() -> dep::Choice[i64] = dep::Choice::Named { value: 1 }\n",
                ),
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn qualified_struct_form_uses_canonical_field_diagnostics() {
    let request = FileSetRequest::new(
        [
            file("dep.eglur", "pub struct Cell { pub field value: i64 }\n"),
            file(
                "main.eglur",
                "fn bad() -> dep::Cell = dep::Cell { missing: 1 }\n",
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["STRUCT_INIT_UNKNOWN_FIELD"]
    );
}

#[test]
fn qualified_braced_direct_types_preserve_lookup_and_kind_precedence() {
    for (dependency, expression, expected_code, expected_kind) in [
        (
            "pub enum Token { End }\n",
            "dep::Token {}",
            "CONSTRUCTOR_TARGET_KIND_MISMATCH",
            Some(("struct", "enum")),
        ),
        (
            concat!(
                "pub struct Cell { pub field value: i64 }\n",
                "pub type Alias = Cell\n",
            ),
            "dep::Alias { value: 1 }",
            "CONSTRUCTOR_TARGET_KIND_MISMATCH",
            Some(("struct", "type_alias")),
        ),
        (
            "pub fn helper() -> i64 = 1\n",
            "dep::helper {}",
            "NAME_NOT_FOUND",
            None,
        ),
        (
            "struct Cell { field value: i64 }\n",
            "dep::Cell { value: 1 }",
            "VISIBILITY_VIOLATION",
            None,
        ),
    ] {
        let request = FileSetRequest::new(
            [
                file("dep.eglur", dependency),
                file("main.eglur", &format!("fn bad() -> Unit = {expression}\n")),
            ],
            "main.eglur",
            Some("."),
        )
        .unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
        let catalog = TrustedCatalog::first_slice().unwrap();

        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        let diagnostics = snapshot.diagnostics();

        assert_eq!(diagnostic_codes(&snapshot), [expected_code], "{expression}");
        if let Some((expected, actual)) = expected_kind {
            assert_eq!(
                diagnostics[0].data().get("expected_kind"),
                Some(&DiagnosticDataValue::from(expected)),
                "{expression}"
            );
            assert_eq!(
                diagnostics[0].data().get("actual_kind"),
                Some(&DiagnosticDataValue::from(actual)),
                "{expression}"
            );
        }
    }
}

#[test]
fn qualified_struct_form_preserves_type_reexport_ambiguity() {
    let request = FileSetRequest::new(
        [
            file("left.eglur", "pub struct Value {}\n"),
            file("right.eglur", "pub struct Value {}\n"),
            file(
                "bridge.eglur",
                "pub import left::Value\npub import right::Value\n",
            ),
            file("main.eglur", "fn bad() -> Unit = bridge::Value {}\n"),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["NAME_AMBIGUOUS"]
    );
}

#[test]
fn resolved_non_constructor_targets_report_semantic_kinds() {
    for (source, head, target, expected_kind, actual_kind) in [
        (
            concat!(
                "struct Cell { field value: i64 }\n",
                "type IntCell = Cell\n",
                "fn bad() -> Cell = IntCell { value: 1 }\n",
            ),
            "IntCell",
            "IntCell",
            "struct",
            "type_alias",
        ),
        (
            concat!("enum Token { End }\n", "fn bad() -> Token = Token {}\n",),
            "Token",
            "Token",
            "struct",
            "enum",
        ),
        (
            concat!(
                "fn helper() -> i64 = 1\n",
                "fn bad() -> i64 = main::helper\n",
            ),
            "main::helper",
            "helper",
            "variant_constructor",
            "function",
        ),
    ] {
        let mut session = session(source);
        let catalog = TrustedCatalog::first_slice().unwrap();

        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        let snapshot = session.into_diagnostics(None);
        let diagnostics = snapshot.diagnostics();

        assert_eq!(diagnostics.len(), 1, "source:\n{source}");
        assert_eq!(
            diagnostics[0].code().as_str(),
            "CONSTRUCTOR_TARGET_KIND_MISMATCH",
            "source:\n{source}"
        );
        assert_eq!(
            diagnostics[0].data(),
            &[
                ("actual_kind", DiagnosticDataValue::from(actual_kind)),
                ("expected_kind", DiagnosticDataValue::from(expected_kind)),
                ("target", DiagnosticDataValue::from(target)),
            ]
            .into_iter()
            .map(|(key, value)| (Box::<str>::from(key), value))
            .collect(),
            "source:\n{source}"
        );
        assert_eq!(diagnostics[0].related().len(), 1, "source:\n{source}");
        assert_eq!(diagnostics[0].related()[0].label(), "target declared here");
        let related_range = diagnostics[0].related()[0].span().file_span().range();
        assert!(
            source[related_range.start().as_usize()..related_range.end().as_usize()]
                .contains(target),
            "source:\n{source}"
        );
        let range = diagnostics[0]
            .primary_span()
            .as_source()
            .expect("constructor diagnostics are source-bound")
            .file_span()
            .range();
        assert_eq!(
            &source[range.start().as_usize()..range.end().as_usize()],
            head,
            "source:\n{source}"
        );
        if target == "IntCell" {
            assert_eq!(
                render_plain(&snapshot),
                concat!(
                    "error[CONSTRUCTOR_TARGET_KIND_MISMATCH] constructor target kind mismatch ",
                    "for `IntCell`: expected struct, found type_alias\n",
                    " --> main.eglur:3:20\n",
                    " = actual_kind: type_alias\n",
                    " = expected_kind: struct\n",
                    " = target: IntCell\n",
                    " = related: target declared here\n",
                    "   --> main.eglur:2:1-3:1",
                )
            );
        }
    }
}

#[test]
fn constructor_target_kind_validation_precedes_member_capability() {
    let mut session = session(concat!(
        "enum Token { End }\n",
        "fn bad() -> Token = Token {}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    let analysis = session
        .resolve(
            &body_profile_without(TypecheckCapability::ResolveMembers),
            Some(&catalog),
        )
        .unwrap();

    assert!(!analysis.trace().missing().any(|entry| {
        entry.capability() == CompileCapability::Typecheck(TypecheckCapability::ResolveMembers)
    }));
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["CONSTRUCTOR_TARGET_KIND_MISMATCH"]
    );
}

#[test]
fn unresolved_constructor_target_retains_name_lookup_diagnostic() {
    let mut session = session("fn bad() -> Unit = Missing {}\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["NAME_NOT_FOUND"]
    );
}

#[test]
fn trusted_constructor_payload_forms_are_distinct() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut rejected = session("fn bad() -> Opt[i64] = None()\n");
    rejected.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = rejected.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code().as_str(),
        "CONSTRUCTOR_PAYLOAD_FORM_MISMATCH"
    );
    assert_eq!(
        diagnostics[0].data().get("constructor"),
        Some(&DiagnosticDataValue::from("std::opt::None"))
    );
    assert_eq!(
        diagnostics[0].data().get("expected_form"),
        Some(&DiagnosticDataValue::from("unit"))
    );
    assert_eq!(
        diagnostics[0].data().get("actual_form"),
        Some(&DiagnosticDataValue::from("positional"))
    );
}

#[test]
fn named_variant_with_missing_field_reports_the_first_missing_field() {
    let mut session = session(concat!(
        "enum Token {\n",
        "  Number { value: i64 }\n",
        "}\n",
        "fn bad() -> Token = Token::Number {}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();
    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code().as_str(), "STRUCT_INIT_MISSING_FIELD");
    assert!(diagnostics[0].message().contains("struct"));
    assert!(diagnostics[0].message().contains("field"));
    assert!(diagnostics[0].message().contains("missing"));
    assert_eq!(
        diagnostics[0].data().get("struct"),
        Some(&DiagnosticDataValue::from("Number"))
    );
    assert_eq!(
        diagnostics[0].data().get("field"),
        Some(&DiagnosticDataValue::from("value"))
    );
}

#[test]
fn let_pattern_failures_preserve_their_semantic_partition_and_data() {
    fn first_diagnostic(source: &str) -> eglur_diagnostics::Diagnostic {
        let mut session = session(source);
        let catalog = TrustedCatalog::first_slice().unwrap();
        session.resolve(&body_profile(), Some(&catalog)).unwrap();
        session
            .into_diagnostics(None)
            .diagnostics()
            .first()
            .cloned()
            .expect("fixture emits one diagnostic")
    }

    let rest = first_diagnostic(concat!(
        "fn invalid() -> i64 {\n",
        "  let (head, ..) = (1, 2, 3)\n",
        "  yield head\n",
        "}\n",
    ));
    assert_eq!(rest.code().as_str(), "LET_PATTERN_UNSUPPORTED");

    let non_tuple = first_diagnostic(concat!(
        "fn invalid() -> i64 {\n",
        "  let (head, tail) = 1\n",
        "  yield head\n",
        "}\n",
    ));
    assert_eq!(non_tuple.code().as_str(), "LET_PATTERN_TYPE_MISMATCH");
    assert_eq!(
        non_tuple.data(),
        &[
            ("actual_type", DiagnosticDataValue::from("i64")),
            ("pattern_kind", DiagnosticDataValue::from("tuple")),
            ("reason", DiagnosticDataValue::from("tuple_required")),
        ]
        .into_iter()
        .map(|(key, value)| (Box::<str>::from(key), value))
        .collect()
    );

    let arity = first_diagnostic(concat!(
        "fn invalid() -> i64 {\n",
        "  let (first, second, third) = (1, 2)\n",
        "  yield first\n",
        "}\n",
    ));
    assert_eq!(arity.code().as_str(), "LET_PATTERN_TYPE_MISMATCH");
    assert_eq!(
        arity.data().get("reason"),
        Some(&DiagnosticDataValue::from("tuple_arity"))
    );
    assert_eq!(
        arity.data().get("expected_count"),
        Some(&DiagnosticDataValue::from(3_i64))
    );
    assert_eq!(
        arity.data().get("actual_count"),
        Some(&DiagnosticDataValue::from(2_i64))
    );

    let unit = first_diagnostic(concat!(
        "fn invalid() -> Unit {\n",
        "  let () = 1\n",
        "  yield ()\n",
        "}\n",
    ));
    assert_eq!(unit.code().as_str(), "LET_PATTERN_TYPE_MISMATCH");
    assert_eq!(
        unit.data().get("reason"),
        Some(&DiagnosticDataValue::from("unit_required"))
    );
    assert_eq!(
        unit.data().get("expected_type"),
        Some(&DiagnosticDataValue::from("Unit"))
    );
}

#[test]
fn inferred_reference_arguments_are_rejected_before_bound_solving() {
    let mut session = session(concat!(
        "fn id[T](value: T) -> T = value\n",
        "fn main() -> i64 {\n",
        "  let value = 1\n",
        "  let reference = id(&value)\n",
        "  yield value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["REFERENCE_TYPE_POSITION_FORBIDDEN"]
    );
}

#[test]
fn shuffled_file_inputs_preserve_body_typing_products() {
    fn request(reverse: bool) -> FileSetRequest {
        let dependency = file("dep.eglur", "pub fn id[T](value: T) -> T = value\n");
        let main = file("main.eglur", "import dep::id\nfn main() -> i64 = id(1)\n");
        let files = if reverse {
            vec![main, dependency]
        } else {
            vec![dependency, main]
        };
        FileSetRequest::new(files, "main.eglur", Some(".")).unwrap()
    }

    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut forward = CompilerSession::build(request(false), SessionOptions::default()).unwrap();
    let mut reverse = CompilerSession::build(request(true), SessionOptions::default()).unwrap();

    let forward_analysis = forward.resolve(&body_profile(), Some(&catalog)).unwrap();
    let reverse_analysis = reverse.resolve(&body_profile(), Some(&catalog)).unwrap();
    let forward_resolution = forward_analysis.resolution().unwrap();
    let reverse_resolution = reverse_analysis.resolution().unwrap();

    assert_eq!(
        forward_resolution.body_types().fact_count(),
        reverse_resolution.body_types().fact_count()
    );
    assert_eq!(
        forward_resolution.body_types().obligations(),
        reverse_resolution.body_types().obligations()
    );
    assert_eq!(
        forward_resolution.body_types().transfers(),
        reverse_resolution.body_types().transfers()
    );
    assert_eq!(
        forward_resolution
            .body_types()
            .regions()
            .map(|(key, region)| (key.clone(), region.clone()))
            .collect::<Vec<_>>(),
        reverse_resolution
            .body_types()
            .regions()
            .map(|(key, region)| (key.clone(), region.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(forward_analysis.trace(), reverse_analysis.trace());
    assert_eq!(
        diagnostic_codes(&forward.into_diagnostics(None)),
        diagnostic_codes(&reverse.into_diagnostics(None))
    );
}

#[test]
fn qualified_type_paths_observe_public_source_reexports() {
    let request = FileSetRequest::new(
        [
            file("origin.eglur", "pub struct Value {}\n"),
            file("bridge.eglur", "pub import origin::Value\n"),
            file(
                "main.eglur",
                "fn identity(value: bridge::Value) -> bridge::Value = value\n",
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn known_module_prefix_precedes_same_spelled_generic_projection() {
    let request = FileSetRequest::new(
        [
            file("types.eglur", "pub struct Value {}\n"),
            file(
                "main.eglur",
                "fn identity[types](value: types::Value) -> types::Value = value\n",
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    assert!(
        matches!(snapshot, DiagnosticSnapshot::Accepted(_)),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&snapshot)
    );
}

#[test]
fn qualified_type_paths_enforce_cross_module_visibility() {
    let request = FileSetRequest::new(
        [
            file("private.eglur", "struct Value {}\n"),
            file(
                "main.eglur",
                "fn invalid(value: private::Value) -> i64 = 0\n",
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["VISIBILITY_VIOLATION"]
    );
}

#[test]
fn qualified_type_reexports_preserve_import_ambiguity() {
    let request = FileSetRequest::new(
        [
            file("left.eglur", "pub struct Value {}\n"),
            file("right.eglur", "pub struct Value {}\n"),
            file(
                "bridge.eglur",
                "pub import left::Value\npub import right::Value\n",
            ),
            file(
                "main.eglur",
                "fn invalid(value: bridge::Value) -> i64 = 0\n",
            ),
        ],
        "main.eglur",
        Some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["NAME_AMBIGUOUS"]
    );
}

#[test]
fn contextual_integer_literals_enforce_exact_fixed_width_ranges() {
    let mut accepted = session(concat!(
        "fn signed() -> i8 = 127\n",
        "fn unsigned() -> u64 = 18_446_744_073_709_551_615\n",
    ));
    let mut rejected = session("fn overflow() -> i8 = 128\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    accepted.resolve(&body_profile(), Some(&catalog)).unwrap();
    rejected.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
    let snapshot = rejected.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();
    assert_eq!(diagnostic_codes(&snapshot), ["INT_OVERFLOW"]);
    assert_eq!(
        diagnostics[0].data().get("operation"),
        Some(&DiagnosticDataValue::from("literal"))
    );
    assert_eq!(
        diagnostics[0].data().get("dtype"),
        Some(&DiagnosticDataValue::from("i8"))
    );
}

#[test]
fn default_integer_literals_do_not_search_for_a_wider_type() {
    let mut session = session(concat!(
        "fn main() -> i64 {\n",
        "  let value = 9_223_372_036_854_775_808\n",
        "  yield value\n",
        "}\n",
    ));
    let catalog = TrustedCatalog::first_slice().unwrap();

    session.resolve(&body_profile(), Some(&catalog)).unwrap();

    let snapshot = session.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();
    assert_eq!(diagnostic_codes(&snapshot), ["INT_OVERFLOW"]);
    assert_eq!(
        diagnostics[0].data().get("dtype"),
        Some(&DiagnosticDataValue::from("i64"))
    );
}

#[test]
fn domain_integer_literals_use_exact_expected_domain_types() {
    let mut accepted = session(concat!(
        "fn nat() -> Nat = 0\n",
        "fn positive() -> PositiveNat = 1\n",
        "fn take(value: Nat) -> Nat = value\n",
        "fn contextual_call() -> Nat = take(2)\n",
    ));
    let mut rejected = session("fn zero() -> PositiveNat = 0\n");
    let catalog = TrustedCatalog::first_slice().unwrap();

    accepted.resolve(&body_profile(), Some(&catalog)).unwrap();
    rejected.resolve(&body_profile(), Some(&catalog)).unwrap();

    assert!(matches!(
        accepted.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
    let snapshot = rejected.into_diagnostics(None);
    let diagnostics = snapshot.diagnostics();
    assert_eq!(diagnostic_codes(&snapshot), ["DOMAIN_INVALID_VALUE"]);
    assert_eq!(
        diagnostics[0].data().get("domain"),
        Some(&DiagnosticDataValue::from("PositiveNat"))
    );
    assert_eq!(
        diagnostics[0].data().get("value"),
        Some(&DiagnosticDataValue::from("0"))
    );
}

#[test]
fn every_no_binding_discard_rejects_ownership_values() {
    fn source(discard: &str) -> String {
        format!(
            "fn make(value: i64) -> Own[i64] = own value\nfn bad() -> Unit {{\n  {discard}\n}}\n"
        )
    }

    let catalog = TrustedCatalog::first_slice().unwrap();
    for discard in ["_ = make(1)", "let _ = make(1)", "make(1)"] {
        let mut session = session(&source(discard));
        session.resolve(&body_profile(), Some(&catalog)).unwrap();

        assert_eq!(
            diagnostic_codes(&session.into_diagnostics(None)),
            ["OWNERSHIP_DISCARD_FORBIDDEN"],
            "discard surface: {discard}"
        );
    }
}
