use eglur_driver::{CompilerSession, EvaluationToolLimit};
use eglur_driver::{
    EvaluationLimits, ExecutionAttempt, ExecutionCapability, ExecutionOutcome, FileInput,
    FileSetRequest, ProgramMode, SessionOptions, SupportProfile, SupportProfileId, SupportReason,
    SupportSchemaVersion, TrustedCatalog, Value, execute,
};
use eglur_sema::{CompileCapability, ResolveCapability, TypecheckCapability};

fn execution_profile() -> SupportProfile {
    SupportProfile::new(
        SupportSchemaVersion::V1,
        SupportProfileId::new("checked-execution-v1").unwrap(),
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
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            CompileCapability::Typecheck(TypecheckCapability::CheckOwnershipFlow),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence),
            CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries),
        ],
        [
            ExecutionCapability::CoreValues,
            ExecutionCapability::Calls,
            ExecutionCapability::Aggregates,
            ExecutionCapability::ReferencesAndMutation,
            ExecutionCapability::Branching,
            ExecutionCapability::FailureFlow,
            ExecutionCapability::CheckedIntegerArithmetic,
            ExecutionCapability::TrustedScalarHooks,
            ExecutionCapability::TestEffects,
        ],
    )
    .unwrap()
}

fn compile_single(source: &str) -> (eglur_driver::CheckedProgram, TrustedCatalog, SupportProfile) {
    compile_files([("main.eglur", source)], "main.eglur")
}

fn without_execution_capability(
    profile: &SupportProfile,
    removed: ExecutionCapability,
) -> SupportProfile {
    SupportProfile::new(
        profile.schema(),
        SupportProfileId::new("checked-execution-reduced-v1").unwrap(),
        profile.compile_capabilities(),
        profile
            .execution_capabilities()
            .filter(|&capability| capability != removed),
    )
    .unwrap()
}

fn compile_files<const N: usize>(
    files: [(&str, &str); N],
    entry: &str,
) -> (eglur_driver::CheckedProgram, TrustedCatalog, SupportProfile) {
    let request = FileSetRequest::new(
        files.map(|(path, source)| FileInput::new(path, source.as_bytes().to_vec())),
        entry,
        (N > 1).then_some("."),
    )
    .unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let profile = execution_profile();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    let analysis = session
        .resolve_for(ProgramMode::Executable, &profile, Some(&catalog))
        .unwrap();
    let missing = analysis
        .trace()
        .missing()
        .map(|entry| entry.capability().stable_key())
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "missing compile capabilities: {missing:?}"
    );
    let sealed = session.seal(None).unwrap();
    if sealed.checked_program().is_none() {
        panic!("source must check: {:?}", sealed.diagnostics());
    }
    let program = sealed
        .into_checked_program()
        .expect("checked program presence was verified");
    (program, catalog, profile)
}

fn completed(
    program: &eglur_driver::CheckedProgram,
    catalog: &TrustedCatalog,
    profile: &SupportProfile,
) -> eglur_driver::CompletedExecution {
    match execute(program, profile, catalog, EvaluationLimits::default()) {
        ExecutionAttempt::Executed(ExecutionOutcome::Completed(completed)) => completed,
        other => panic!("expected completion, found {other:?}"),
    }
}

fn ok_str(value: &Value) -> &str {
    let Value::Variant { payload, .. } = value else {
        panic!("expected result variant")
    };
    let [Value::Str(value)] = payload.as_ref() else {
        panic!("expected one string payload")
    };
    value
}

#[test]
fn entry_value_and_process_status_are_preserved_without_rendering() {
    let (program, catalog, profile) = compile_single("fn main!() -> Res[Str, Str] = Ok(\"ok\")\n");
    let result = completed(&program, &catalog, &profile);

    assert_eq!(ok_str(result.value()), "ok");
    assert_eq!(result.process_status(), 0);
    assert!(result.events().is_empty());
}

#[test]
fn unit_entry_maps_to_success_without_implicit_output() {
    let (program, catalog, profile) = compile_single("fn main!() -> Unit = ()\n");
    let result = completed(&program, &catalog, &profile);

    assert_eq!(result.value(), &Value::Unit);
    assert_eq!(result.process_status(), 0);
    assert!(result.events().is_empty());
}

#[test]
fn only_the_selected_if_branch_executes() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::abort!\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let x = if true { yield 1 } else {\n",
        "    abort!(\"RUNTIME_ABORT\", \"dead else\")\n",
        "    yield 0\n",
        "  }\n",
        "  yield Ok(to_str(x))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "1");
}

#[test]
fn match_and_question_short_circuit_preserve_carriers() {
    let cases = [
        (
            concat!(
                "fn main!() -> Res[Str, Str] {\n",
                "  let x = Some(3)\n",
                "  let y = match x { Some(v) => v, None => 0 }\n",
                "  yield Ok(to_str(y))\n",
                "}\n",
            ),
            "3",
            true,
        ),
        (
            concat!(
                "fn f() -> Res[i64, Str] = Err(\"boom\")\n",
                "fn main!() -> Res[Str, Str] = result {\n",
                "  _ = f()?\n",
                "  yield Ok(\"unreachable\")\n",
                "}\n",
            ),
            "boom",
            false,
        ),
    ];

    for (source, expected, is_ok) in cases {
        let (program, catalog, profile) = compile_single(source);
        let result = completed(&program, &catalog, &profile);
        let Value::Variant { payload, .. } = result.value() else {
            panic!("expected result variant")
        };
        assert_eq!(payload.as_ref(), [Value::Str(expected.into())]);
        assert_eq!(result.process_status(), u8::from(!is_ok));
    }
}

#[test]
fn checked_overflow_is_an_err_value_with_a_value_diagnostic() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn double(x: i8) -> Res[i8, NumericError] = checked { yield x + x }\n",
        "fn main!() -> Res[Str, NumericError] = result {\n",
        "  _ = double(64)?\n",
        "  yield Ok(\"unreachable\")\n",
        "}\n",
    ));
    let result = completed(&program, &catalog, &profile);

    assert_eq!(result.process_status(), 1);
    assert_eq!(result.value_diagnostics().len(), 1);
    assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
    assert_eq!(
        result.value_diagnostics()[0].data()["operation"].as_ref(),
        "add"
    );
    assert_eq!(result.value_diagnostics()[0].data()["dtype"].as_ref(), "i8");

    let reduced =
        without_execution_capability(&profile, ExecutionCapability::ReferencesAndMutation);
    assert!(matches!(
        execute(&program, &reduced, &catalog, EvaluationLimits::default()),
        ExecutionAttempt::Executed(ExecutionOutcome::Completed(_))
    ));
}

#[test]
fn generic_and_bound_calls_execute_shared_checked_bodies() {
    let (program, catalog, profile) = compile_single(concat!(
        "closed trait Readable {\n",
        "  type Elem\n",
        "  fn read(self: &Self) -> Self::Elem\n",
        "}\n",
        "struct Count {\n",
        "  field value: i64\n",
        "}\n",
        "impl Readable for Count {\n",
        "  type Elem = i64\n",
        "  fn read(self: &Self) -> i64 = self.value\n",
        "}\n",
        "fn project[C: Readable](x: C) -> C::Elem = x.read()\n",
        "fn identity[C: Readable](x: C) -> C = x\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let same = identity(Count { value: 7 })\n",
        "  let inferred = project(same)\n",
        "  let explicit = project::[Count](Count { value: 8 })\n",
        "  yield Ok(to_str(wrapping_add(inferred, explicit)))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "15"
    );
}

#[test]
fn cross_module_method_dispatch_and_mutable_reference_updates_work() {
    let (program, catalog, profile) = compile_files(
        [
            (
                "app/state.eglur",
                concat!(
                    "module app::state\n",
                    "pub struct Counter { field mut n: i64 }\n",
                    "pub fn counter() -> Counter = Counter { n: 0 }\n",
                    "pub fn increment!(counter: &mut Counter) -> Unit {\n",
                    "  counter.n = wrapping_add(counter.n, 1)\n",
                    "}\n",
                    "pub fn read(counter: &Counter) -> i64 = counter.n\n",
                ),
            ),
            (
                "app/main.eglur",
                concat!(
                    "module app::main\n",
                    "import app::state::{counter, increment!, read}\n",
                    "fn run() -> i64 {\n",
                    "  let mut value = counter()\n",
                    "  increment!(&mut value)\n",
                    "  yield read(&value)\n",
                    "}\n",
                    "fn main!() -> Res[Str, Str] { yield Ok(to_str(run())) }\n",
                ),
            ),
        ],
        "app/main.eglur",
    );

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "1");
}

#[test]
fn explicit_dereference_assignment_updates_the_referent() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn increment!(value: &mut i64) -> Unit {\n",
        "  *value = wrapping_add(*value, 1)\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let mut value = 4\n",
        "  increment!(&mut value)\n",
        "  yield Ok(to_str(value))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "5");

    let reduced =
        without_execution_capability(&profile, ExecutionCapability::ReferencesAndMutation);
    assert_eq!(
        execute(&program, &reduced, &catalog, EvaluationLimits::default()),
        ExecutionAttempt::Unsupported(SupportReason::MissingCapability(
            ExecutionCapability::ReferencesAndMutation,
        ))
    );
}

#[test]
fn reference_rebinding_and_nested_references_preserve_place_identity() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Str, Str] {\n",
        "  let mut first = 1\n",
        "  let mut second = 2\n",
        "  let mut reference = &mut first\n",
        "  reference = &mut second\n",
        "  *reference = 9\n",
        "  let nested = &reference\n",
        "  yield Ok(to_str(wrapping_add(first, **nested)))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "10"
    );
}

#[test]
fn reference_bearing_local_tuples_preserve_aliases_across_all_transports() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn update!(pair: (&mut i64, &mut i64)) -> Unit {\n",
        "  *pair.1 = 8\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let mut first = 1\n",
        "  let mut second = 2\n",
        "  let mut pair = (&mut first, &mut second)\n",
        "  let original = pair\n",
        "  pair.0 = &mut second\n",
        "  *pair.0 = 9\n",
        "  *original.0 = 7\n",
        "  let (left, right) = original\n",
        "  update!((left, right))\n",
        "  let nested = ((left,), (right,))\n",
        "  let selected = match nested { ((found,), _) => found }\n",
        "  *selected = wrapping_add(*selected, 1)\n",
        "  yield Ok(to_str(wrapping_add(first, second)))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "16"
    );
}

#[test]
fn public_tuple_boundaries_round_trip_without_internal_runtime_tokens() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Holder { field mut pair: (i64, i64) }\n",
        "fn pair() -> Res[(i64, i64), Str] = Ok((3, 4))\n",
        "fn main!() -> Res[Str, Str] = result {\n",
        "  let (left, right) = pair()?\n",
        "  let mut holder = Holder { pair: (left, right) }\n",
        "  holder.pair.0 = 5\n",
        "  yield Ok(to_str(wrapping_add(holder.pair.0, holder.pair.1)))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "9");

    let (program, catalog, profile) =
        compile_single("fn main!() -> Res[(i64, Bool), Str] = Ok((3, true))\n");
    let result = completed(&program, &catalog, &profile);
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected result variant")
    };
    let [Value::Tuple(values)] = payload.as_ref() else {
        panic!("expected one public tuple payload")
    };
    let [Value::Integer(integer), Value::Bool(true)] = values.as_ref() else {
        panic!("expected integer and bool tuple elements")
    };
    assert_eq!(integer.value(), 3);
}

#[test]
fn match_can_bind_and_return_a_reference_value() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Str, Str] {\n",
        "  let value = 7\n",
        "  let reference = &value\n",
        "  let selected = match reference { found => found }\n",
        "  yield Ok(to_str(*selected))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn source_order_is_preserved_when_calls_and_fields_bind_by_declaration_slot() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Marks {\n",
        "  field first: Unit\n",
        "  field second: Unit\n",
        "}\n",
        "fn consume(first: Unit, second: Unit) -> Unit = ()\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let _marks = Marks { second: emit!(\"field-second\"), first: emit!(\"field-first\") }\n",
        "  consume(second=emit!(\"call-second\"), first=emit!(\"call-first\"))\n",
        "  yield Ok(\"ok\")\n",
        "}\n",
    ));
    let result = completed(&program, &catalog, &profile);

    assert_eq!(
        result.events(),
        [
            eglur_driver::RuntimeEvent::Emitted("field-second".into()),
            eglur_driver::RuntimeEvent::Emitted("field-first".into()),
            eglur_driver::RuntimeEvent::Emitted("call-second".into()),
            eglur_driver::RuntimeEvent::Emitted("call-first".into()),
        ]
    );
}

#[test]
fn source_display_impl_executes_through_the_free_adapter() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Label { field value: i64 }\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.value)\n",
        "}\n",
        "fn make!() -> Label {\n",
        "  emit!(\"make\")\n",
        "  yield Label { value: 7 }\n",
        "}\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(make!()))\n",
    ));

    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "7");
    assert_eq!(
        result.events(),
        [eglur_driver::RuntimeEvent::Emitted("make".into())]
    );
}

#[test]
fn display_method_syntax_executes_scalar_and_source_targets() {
    let (scalar, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Str, Str] {\n",
        "  let value = 7\n",
        "  yield Ok(value.to_str())\n",
        "}\n",
    ));
    assert_eq!(ok_str(completed(&scalar, &catalog, &profile).value()), "7");

    let (source, catalog, profile) = compile_single(concat!(
        "struct Label { field value: i64 }\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.value)\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let value = Label { value: 8 }\n",
        "  yield Ok(value.to_str())\n",
        "}\n",
    ));
    assert_eq!(ok_str(completed(&source, &catalog, &profile).value()), "8");
}

#[test]
fn generic_display_adapter_follows_a_source_impl_witness() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Label { field value: i64 }\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.value)\n",
        "}\n",
        "fn stringify[T: Display](value: T) -> Str = to_str(value)\n",
        "fn main!() -> Res[Str, Str] = Ok(stringify(Label { value: 9 }))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "9");
}

#[test]
fn scalar_hook_support_follows_checked_witnesses_through_shared_generic_bodies() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn stringify[T: Display](value: T) -> Str = to_str(value)\n",
        "fn main!() -> Res[Str, Str] = Ok(stringify(7))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn property_reads_are_rejected_before_runtime_until_accessor_dispatch_is_lowered() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Counter {\n",
        "  field n: i64\n",
        "  prop count: i64 {\n",
        "    fn get_count(self: &Self) -> i64 = self.n\n",
        "  }\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let counter = Counter { n: 7 }\n",
        "  yield Ok(to_str(counter.count))\n",
        "}\n",
    ));

    assert_eq!(
        execute(&program, &profile, &catalog, EvaluationLimits::default()),
        ExecutionAttempt::Unsupported(SupportReason::Feature(
            eglur_driver::UnsupportedFeature::PropertyRead,
        ))
    );
}

#[test]
fn runtime_abort_and_call_depth_limit_remain_distinct_outcomes() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::abort!\n",
        "fn main!() -> Never = abort!(\"TEST_ABORT\", \"stopped\")\n",
    ));
    assert!(matches!(
        execute(&program, &profile, &catalog, EvaluationLimits::default()),
        ExecutionAttempt::Executed(ExecutionOutcome::RuntimeAbort(ref diagnostic))
            if diagnostic.code() == "TEST_ABORT"
    ));

    let (program, catalog, profile) = compile_single(concat!(
        "fn recurse() -> i64 = recurse()\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(recurse()))\n",
    ));
    assert_eq!(
        execute(&program, &profile, &catalog, EvaluationLimits::new(100, 2)),
        ExecutionAttempt::ToolLimitExceeded(EvaluationToolLimit::CallDepth { limit: 2 })
    );

    let (program, catalog, profile) = compile_single(concat!(
        "fn leaf() -> i64 = 1\n",
        "fn middle() -> i64 = leaf()\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(middle()))\n",
    ));
    assert_eq!(
        execute(&program, &profile, &catalog, EvaluationLimits::new(100, 2)),
        ExecutionAttempt::ToolLimitExceeded(EvaluationToolLimit::CallDepth { limit: 2 })
    );
}

#[test]
fn unsupported_hooks_are_preflighted_and_limits_are_out_of_band() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::io::print!\n",
        "fn main!() -> Unit { print!(\"x\") }\n",
    ));
    assert!(matches!(
        execute(&program, &profile, &catalog, EvaluationLimits::default()),
        ExecutionAttempt::Unsupported(SupportReason::MissingRuntimeHook(_))
    ));

    let (program, catalog, profile) = compile_single("fn main!() -> Res[Str, Str] = Ok(\"ok\")\n");
    assert_eq!(
        execute(&program, &profile, &catalog, EvaluationLimits::new(0, 10)),
        ExecutionAttempt::ToolLimitExceeded(EvaluationToolLimit::Steps { limit: 0 })
    );
}

#[test]
fn checked_unsigned_negation_preserves_dtype_and_reports_overflow() {
    for (dtype, maximum) in [
        ("u8", "255"),
        ("u16", "65535"),
        ("u32", "4294967295"),
        ("u64", "18446744073709551615"),
    ] {
        for argument in ["0", "1", maximum] {
            let source = format!(
                "type Unsigned = {dtype}\n\
                 fn id[T](x: T) -> T = x\n\
                 fn negate(x: Unsigned) -> Res[Unsigned, NumericError] = checked {{ yield -x }}\n\
                 fn main!() -> Res[Str, NumericError] = result {{\n\
                   let value = negate(id::[Unsigned]({argument}))?\n\
                   yield Ok(to_str(value))\n\
                 }}\n"
            );
            let (program, catalog, profile) = compile_single(&source);
            let result = completed(&program, &catalog, &profile);
            if argument == "0" {
                assert_eq!(result.process_status(), 0);
                assert_eq!(ok_str(result.value()), "0");
                assert!(result.value_diagnostics().is_empty());
            } else {
                assert_eq!(result.process_status(), 1);
                assert_eq!(result.value_diagnostics().len(), 1);
                let diagnostic = &result.value_diagnostics()[0];
                assert_eq!(diagnostic.code(), "INT_OVERFLOW");
                assert_eq!(diagnostic.data()["operation"].as_ref(), "negate");
                assert_eq!(diagnostic.data()["dtype"].as_ref(), dtype);
            }
            let reduced = without_execution_capability(
                &profile,
                ExecutionCapability::CheckedIntegerArithmetic,
            );
            assert!(!matches!(
                execute(&program, &reduced, &catalog, EvaluationLimits::default()),
                ExecutionAttempt::Executed(_)
            ));
        }
    }
}

#[test]
fn checked_signed_negation_keeps_signed_minimum_overflow() {
    for (dtype, minimum) in [
        ("i8", "-128"),
        ("i16", "-32768"),
        ("i32", "-2147483648"),
        ("i64", "-9223372036854775808"),
    ] {
        for (argument, expected) in [("0", Some("0")), ("-1", Some("1")), (minimum, None)] {
            let source = format!(
                "fn negate(x: {dtype}) -> Res[{dtype}, NumericError] = checked {{ yield -x }}\n\
                 fn main!() -> Res[Str, NumericError] = result {{\n\
                   let value = negate({argument})?\n\
                   yield Ok(to_str(value))\n\
                 }}\n"
            );
            let (program, catalog, profile) = compile_single(&source);
            let result = completed(&program, &catalog, &profile);
            if let Some(expected) = expected {
                assert_eq!(result.process_status(), 0);
                assert_eq!(ok_str(result.value()), expected);
            } else {
                assert_eq!(result.process_status(), 1);
                assert_eq!(result.value_diagnostics()[0].code(), "INT_OVERFLOW");
            }
        }
    }
}

#[test]
fn checked_unsigned_negation_fault_stays_in_the_inner_region() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Res[u8, NumericError], NumericError] {\n",
        "  let x: u8 = 1\n",
        "  yield checked { yield checked { yield -x } }\n",
        "}\n",
    ));
    let result = completed(&program, &catalog, &profile);
    assert_eq!(result.process_status(), 0);
    let Value::Variant { payload, .. } = result.value() else {
        panic!("expected outer Ok");
    };
    let [Value::Variant { payload, .. }] = payload.as_ref() else {
        panic!("expected inner Err");
    };
    let [Value::NumericError(diagnostic)] = payload.as_ref() else {
        panic!("expected numeric fault");
    };
    assert_eq!(diagnostic.code(), "INT_OVERFLOW");
    assert_eq!(diagnostic.data()["dtype"].as_ref(), "u8");
}
