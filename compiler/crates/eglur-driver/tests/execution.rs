use eglur_driver::{CompilerSession, EvaluationToolLimit};
use eglur_driver::{
    EvaluationLimits, ExecutionAttempt, ExecutionCapability, ExecutionOutcome, FileInput,
    FileSetRequest, ProgramMode, SessionOptions, SupportProfile, SupportProfileId, SupportReason,
    SupportSchemaVersion, TrustedCatalog, Value, execute,
};
use eglur_sema::{CompileCapability, ResolveCapability, TypecheckCapability};

#[path = "execution/grouped_calls.rs"]
mod grouped_calls;
#[path = "execution/receiver_adjustment.rs"]
mod receiver_adjustment;

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
                TypecheckCapability::EvaluateSelectedIntegerConstExpressions,
            ),
            CompileCapability::Typecheck(
                TypecheckCapability::EvaluateSelectedIntegerConstDeclarations,
            ),
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
fn residual_patterns_select_arms_independently_of_same_spelled_locals() {
    for (input, pattern, expected) in [
        ("None", "None", "7"),
        ("None", "(None)", "7"),
        ("None", "whole @ None", "7"),
        ("Some(3)", "None", "3"),
        ("Some(3)", "(None)", "3"),
        ("Some(3)", "whole @ None", "3"),
    ] {
        let source = format!(
            "fn choose(None: i64, Some: i64, input: Opt[i64]) -> i64 = match input {{ {pattern} => None, Some(value) => value }}\n\
             fn main!() -> Res[Str, Str] = Ok(to_str(choose(7, 9, {input})))\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            expected,
            "{source}"
        );
    }

    for (input, expected) in [("Ok(3)", "7"), ("Err(4)", "9")] {
        let source = format!(
            "fn choose(Ok: i64, Err: i64, input: Res[i64, i64]) -> i64 = match input {{ Ok(value) => Ok, Err(error) => Err }}\n\
             fn main!() -> Res[Str, Str] = Ok(to_str(choose(7, 9, {input})))\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            ok_str(completed(&program, &catalog, &profile).value()),
            expected,
            "{source}"
        );
    }
}

#[test]
fn unit_constructor_values_keep_canonical_identity_across_spellings() {
    for expression in [
        "None",
        "(None)",
        "std::opt::None",
        "Absent",
        "bridge::Absent",
        "take(None, 7)",
    ] {
        let source = format!(
            "import std::opt::None as Absent\n\
             type Maybe = Opt[i64]\n\
             fn take[T](left: Opt[T], right: T) -> Opt[T] = left\n\
             fn make() -> Maybe = {expression}\n\
             fn main!() -> Res[Opt[i64], Str] = Ok(make())\n"
        );
        let (program, catalog, profile) = compile_files(
            [
                ("bridge.eglur", "pub import std::opt::None as Absent\n"),
                ("main.eglur", &source),
            ],
            "main.eglur",
        );
        let result = completed(&program, &catalog, &profile);
        let payload = result.value().variant_payload().unwrap();
        assert_eq!(
            payload,
            [Value::Variant {
                declaration: eglur_sema::CanonicalTarget::External(
                    eglur_sema::ExternalDefId::new("std::opt::None").unwrap(),
                ),
                payload: Box::new([]),
            }],
            "{expression}"
        );
        assert_eq!(result.process_status(), 0);
        assert!(result.events().is_empty());
    }
}

#[test]
fn unit_constructor_values_short_circuit_optional_result_blocks() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn empty() -> Opt[i64] = result {\n",
        "  let absent: Opt[i64] = None\n",
        "  let value = absent?\n",
        "  yield Some(value)\n",
        "}\n",
        "fn main!() -> Res[Str, Str] = Ok(match empty() { None => \"empty\", Some(_) => \"full\" })\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "empty"
    );
}

#[test]
fn unit_constructor_source_owners_preserve_identity_and_payload_form() {
    for (expression, variant) in [
        ("dep::Token::Stop", "Stop"),
        ("bridge::Renamed::Stop", "Stop"),
        ("dep::Token::Empty()", "Empty"),
    ] {
        let source = format!(
            "fn make() -> dep::Token[i64] = {expression}\n\
             fn main!() -> Res[dep::Token[i64], Str] = Ok(make())\n"
        );
        let (program, catalog, profile) = compile_files(
            [
                (
                    "dep.eglur",
                    "pub enum Token[T] {\n Stop\n Empty()\n Item(T)\n}\n",
                ),
                ("bridge.eglur", "pub import dep::Token as Renamed\n"),
                ("main.eglur", &source),
            ],
            "main.eglur",
        );
        let declaration = program
            .resolution()
            .declarations()
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.spelling() == variant)
            })
            .unwrap()
            .id();
        let result = completed(&program, &catalog, &profile);
        assert_eq!(
            result.value().variant_payload().unwrap(),
            [Value::Variant {
                declaration: eglur_sema::CanonicalTarget::Source(declaration),
                payload: Box::new([]),
            }],
            "{expression}"
        );
    }
}

#[test]
fn unit_constructor_preflight_requires_the_selected_execution_capability() {
    for (source, capability) in [
        (
            "fn main!() -> Unit { let value: Opt[i64] = None\n _ = value }\n",
            ExecutionCapability::FailureFlow,
        ),
        (
            "enum Token { Stop }\nfn main!() -> Unit { let value = Token::Stop\n _ = value }\n",
            ExecutionCapability::Aggregates,
        ),
    ] {
        let (program, catalog, profile) = compile_single(source);
        let reduced = without_execution_capability(&profile, capability);
        assert_eq!(
            execute(&program, &reduced, &catalog, EvaluationLimits::default()),
            ExecutionAttempt::Unsupported(SupportReason::MissingCapability(capability)),
            "{source}"
        );
    }
}

#[test]
fn unit_constructor_missing_hook_is_refused_before_any_body_executes() {
    struct WithoutNoneHook<'a>(&'a TrustedCatalog);

    impl eglur_eval::RuntimeCatalog for WithoutNoneHook<'_> {
        fn runtime_hook(
            &self,
            declaration: &eglur_sema::ExternalDefId,
        ) -> Option<eglur_driver::RuntimeHook> {
            self.0
                .runtime_hook(declaration)
                .filter(|hook| *hook != eglur_driver::RuntimeHook::OptNone)
        }
    }

    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::abort!\n",
        "fn main!() -> Unit {\n",
        "  abort!(\"RUNTIME_ABORT\", \"must not execute\")\n",
        "  let value: Opt[i64] = None\n",
        "  _ = value\n",
        "}\n",
    ));
    assert_eq!(
        execute(
            &program,
            &profile,
            &WithoutNoneHook(&catalog),
            EvaluationLimits::default(),
        ),
        ExecutionAttempt::Unsupported(SupportReason::MissingRuntimeHook(
            eglur_sema::ExternalDefId::new("std::opt::None").unwrap(),
        ))
    );
}

#[test]
fn unit_constructor_spelling_does_not_change_local_or_constant_values() {
    let (program, catalog, profile) = compile_single(concat!(
        "const number: i64 = 4\n",
        "fn take(None: i64) -> i64 = None\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let None = take(number)\n",
        "  yield Ok(to_str(None))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "4");
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
fn reordered_generic_bounds_forward_witnesses_to_the_right_methods() {
    for callee_bounds in ["Zulu + Alpha", "Alpha + Zulu"] {
        for caller_bounds in ["Zulu + Alpha", "Alpha + Zulu"] {
            let source = format!(
                concat!(
                    "trait Alpha {{ fn alpha(self: &Self) -> i64 }}\n",
                    "trait Zulu {{ fn zulu(self: &Self) -> i64 = 5 }}\n",
                    "struct Both {{}}\n",
                    "impl Alpha for Both {{ fn alpha(self: &Self) -> i64 = 2 }}\n",
                    "impl Zulu for Both {{}}\n",
                    "fn combine[T: {}](value: T) -> i64 = wrapping_add(value.alpha(), value.zulu())\n",
                    "fn forward[T: {}](value: T) -> i64 = combine(value)\n",
                    "fn main!() -> Res[Str, Str] = Ok(to_str(forward(Both {{}})))\n",
                ),
                callee_bounds, caller_bounds,
            );
            let (program, catalog, profile) = compile_single(&source);
            assert_eq!(
                ok_str(completed(&program, &catalog, &profile).value()),
                "7",
                "{source}"
            );
        }
    }
}

#[test]
fn dependent_generic_bounds_forward_parameterized_witnesses() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Link[A] { fn linked(self: &Self, value: A) -> A }\n",
        "impl Link[i64] for Bool { fn linked(self: &Self, value: i64) -> i64 = value }\n",
        "fn project[A: Link[B], B](value: A, argument: B) -> B = value.linked(argument)\n",
        "fn forward[A: Link[B], B](value: A, argument: B) -> B = project(value, argument)\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(forward(true, 7)))\n",
    ));
    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn normalized_owner_bounds_forward_witnesses_to_checked_methods() {
    for caller_argument in ["i64", "Item[Carrier]"] {
        for callee_argument in ["i64", "Item[Carrier]"] {
            let source = format!(
                "trait Carries {{ type Item }}\nstruct Carrier {{}}\n\
                 impl Carries for Carrier {{ type Item = i64 }}\n\
                 type Item[T: Carries] = T::Item\n\
                 trait Link[A] {{\nfn linked(self: &Self, value: A) -> A\n\
                 fn forward(self: &Self, value: A) -> A = self.linked(value)\n}}\n\
                 impl Link[i64] for Bool {{ fn linked(self: &Self, value: i64) -> i64 = value }}\n\
                 fn project[T: Link[{callee_argument}]](value: T) -> i64 = value.linked(7)\n\
                 fn forward[T: Link[{caller_argument}]](value: T) -> i64 = project(value)\n\
                 fn main!() -> Res[Str, Str] = Ok(to_str(forward(true)))\n"
            );
            let (program, catalog, profile) = compile_single(&source);
            assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
        }
    }
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
fn grouped_postfix_applications_preserve_values_and_effect_order() {
    for (expression, marks) in [
        ("(mark!(2), mark!(3)).apply(join!)", vec!["2", "3", "join"]),
        (
            "((mark!(2), mark!(3))).apply(join!)",
            vec!["2", "3", "join"],
        ),
        (
            "(((mark!(2), mark!(3))).apply)(join!)",
            vec!["2", "3", "join"],
        ),
        ("((mark!(5)).then)(pass!)", vec!["5", "join"]),
    ] {
        let source = format!(
            "import std::test::emit!\n\
             fn mark!(value: i64) -> i64 {{ emit!(to_str(value))\nyield value }}\n\
             fn join!(first: i64, second: i64) -> i64 {{ emit!(\"join\")\nyield wrapping_add(first, second) }}\n\
             fn pass!(value: i64) -> i64 {{ emit!(\"join\")\nyield value }}\n\
             fn main!() -> Res[Str, Str] = Ok(to_str({expression}))\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        let result = completed(&program, &catalog, &profile);
        assert_eq!(ok_str(result.value()), "5", "{expression}");
        assert_eq!(
            result.events(),
            marks
                .into_iter()
                .map(|mark| eglur_driver::RuntimeEvent::Emitted(mark.into()))
                .collect::<Vec<_>>(),
            "{expression}"
        );
    }
}

#[test]
fn grouped_apply_keeps_a_tuple_argument_inside_a_unary_tuple() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn first(pair: (i64, i64)) -> i64 = pair.0\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str((((2, 3),)).apply(first)))\n",
    ));
    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "2");
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
fn default_trait_method_uses_its_current_trait_witness() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Link[A] {\n",
        "  fn linked(self: &Self, value: A) -> A\n",
        "  fn forward(self: &Self, value: A) -> A = self.linked(value)\n",
        "}\n",
        "impl Link[i64] for Bool { fn linked(self: &Self, value: i64) -> i64 = value }\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(true.forward(7)))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn normalized_bound_dispatches_into_the_default_method_frame() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Carries { type Item }\n",
        "struct Carrier {}\n",
        "impl Carries for Carrier { type Item = i64 }\n",
        "type Item[T: Carries] = T::Item\n",
        "trait Link[A] {\n",
        "  fn linked(self: &Self, value: A) -> A\n",
        "  fn forward(self: &Self, value: A) -> A = self.linked(value)\n",
        "}\n",
        "impl Link[i64] for Bool { fn linked(self: &Self, value: i64) -> i64 = value }\n",
        "fn forward[T: Link[Item[Carrier]]](value: T) -> i64 = value.forward(7)\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(forward(true)))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn generic_impl_method_receives_its_prerequisite_witness() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Marker {}\n",
        "impl Marker for Bool {}\n",
        "trait Show { fn number(self: &Self) -> i64 }\n",
        "fn marker[T: Marker]() -> i64 = 7\n",
        "impl[T: Marker] Show for T {\n",
        "  fn number(self: &Self) -> i64 = marker::[T]()\n",
        "}\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(true.number()))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn default_trait_method_receives_trait_generic_prerequisite() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Marker {}\n",
        "impl Marker for Bool {}\n",
        "fn marked[T: Marker]() -> i64 = 7\n",
        "trait Echo[A: Marker] {\n",
        "  fn echo(self: &Self, value: A) -> i64 = marked::[A]()\n",
        "}\n",
        "impl Echo[Bool] for i64 {}\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(1.echo(true)))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn override_method_generic_bounds_route_by_contract_not_written_order() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Left {}\n",
        "trait Right {}\n",
        "impl Left for Bool {}\n",
        "impl Right for Bool {}\n",
        "fn both[T: Left + Right]() -> i64 = 7\n",
        "trait Combo { fn combine[T: Left + Right](self: &Self, value: T) -> i64 }\n",
        "impl Combo for Bool {\n",
        "  fn combine[U: Right + Left](self: &Self, value: U) -> i64 = both::[U]()\n",
        "}\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(true.combine(true)))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn selected_impl_prerequisite_survives_multiple_generic_calls() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Marker {}\n",
        "impl Marker for Bool {}\n",
        "trait Show { fn number(self: &Self) -> i64 }\n",
        "fn marker[T: Marker]() -> i64 = 7\n",
        "impl[T: Marker] Show for T {\n",
        "  fn number(self: &Self) -> i64 = marker::[T]()\n",
        "}\n",
        "fn inner[T: Marker](value: T) -> i64 = value.number()\n",
        "fn outer[T: Marker](value: T) -> i64 = inner::[T](value)\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(outer(true)))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn no_bound_default_method_remains_executable() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Readable { fn read(self: &Self) -> i64 = 7 }\n",
        "impl Readable for Bool {}\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(true.read()))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn method_generic_bound_can_refer_to_a_trait_parameter() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Link[A] {}\n",
        "impl Link[i64] for Bool {}\n",
        "fn linked[X: Link[Y], Y]() -> i64 = 7\n",
        "trait Use[A] {\n",
        "  fn use[B: Link[A]](self: &Self, value: B) -> i64 = linked::[B, A]()\n",
        "}\n",
        "impl Use[i64] for Bool {}\n",
        "fn main!() -> Res[Str, Str] = Ok(to_str(true.use(true)))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn trusted_adapter_routes_source_impl_prerequisites() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Marker {}\n",
        "impl Marker for Bool {}\n",
        "fn marker[T: Marker]() -> i64 = 7\n",
        "struct Wrap[T] { field value: T }\n",
        "impl[T: Marker] Display for Wrap[T] {\n",
        "  fn to_str(self: &Self) -> Str = to_str(marker::[T]())\n",
        "}\n",
        "fn stringify[T: Display](value: T) -> Str = to_str(value)\n",
        "fn main!() -> Res[Str, Str] = Ok(stringify(Wrap { value: true }))\n",
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
fn named_function_values_execute_through_local_bindings() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn identity(value: i64) -> i64 = value\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let f: Fn(i64) -> i64 = identity\n",
        "  yield Ok(to_str(f(7)))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn selected_overload_function_value_executes_the_selected_target() {
    let (program, catalog, profile) = compile_single(concat!(
        "overload fn identity(value: i64) -> i64 = value\n",
        "overload fn identity(value: Str) -> Str = \"wrong\"\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let f: Fn(i64) -> i64 = identity\n",
        "  yield Ok(to_str(f(7)))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn bounded_generic_function_value_carries_witness_into_indirect_call() {
    let (program, catalog, profile) = compile_single(concat!(
        "trait Readable {\n  type Elem\n  fn read(self: &Self) -> Self::Elem\n}\n",
        "struct Good {}\n",
        "impl Readable for Good {\n  type Elem = i64\n  fn read(self: &Self) -> Self::Elem = 8\n}\n",
        "fn bounded[T: Readable](value: T) -> T::Elem = value.read()\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let f: Fn(Good) -> i64 = bounded\n",
        "  yield Ok(to_str(f(Good{})))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "8");
}

#[test]
fn trusted_wrapping_hook_executes_through_a_function_value() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Str, Str] {\n",
        "  let add: Fn(i8, i8) -> i8 = wrapping_add\n",
        "  let maximum: i8 = 127\n",
        "  yield Ok(to_str(add(maximum, 1)))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "-128"
    );
}

#[test]
fn trusted_display_adapter_value_executes_scalar_hook() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Str, Str] {\n",
        "  let stringify: Fn(i64) -> Str = to_str\n",
        "  yield Ok(stringify(7))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn trusted_display_adapter_value_captures_source_impl() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Label { field value: i64 }\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.value)\n",
        "}\n",
        "fn make!() -> Label { emit!(\"make\")\nyield Label { value: 8 } }\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let stringify: Fn(Label) -> Str = to_str\n",
        "  yield Ok(stringify(make!()))\n",
        "}\n",
    ));

    let result = completed(&program, &catalog, &profile);
    assert_eq!(ok_str(result.value()), "8");
    assert_eq!(
        result.events(),
        [eglur_driver::RuntimeEvent::Emitted("make".into())]
    );
}

#[test]
fn generic_display_adapter_value_preserves_bound_witness() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Label { field value: i64 }\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.value)\n",
        "}\n",
        "fn stringify[T: Display](value: T) -> Str {\n",
        "  let f: Fn(T) -> Str = to_str\n",
        "  yield f(value)\n",
        "}\n",
        "fn main!() -> Res[Str, Str] = Ok(stringify(Label { value: 9 }))\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "9");
}

#[test]
fn generic_display_adapter_value_resolves_builtin_impl() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn stringify[T: Display](value: T) -> Str {\n",
        "  let f: Fn(T) -> Str = to_str\n",
        "  yield f(value)\n",
        "}\n",
        "fn main!() -> Res[Str, Str] = Ok(stringify(11))\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "11"
    );
}

#[test]
fn display_adapter_value_keeps_its_route_when_passed_to_a_function() {
    let (program, catalog, profile) = compile_single(concat!(
        "struct Label { field value: i64 }\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.value)\n",
        "}\n",
        "fn invoke(f: Fn(Label) -> Str) -> Str = f(Label { value: 12 })\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let stringify: Fn(Label) -> Str = to_str\n",
        "  yield Ok(invoke(stringify))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "12"
    );
}

#[test]
fn display_adapter_value_rejects_unsupported_impl_before_effects() {
    let (program, catalog, profile) = compile_single(concat!(
        "import std::test::emit!\n",
        "struct Label {\n",
        "  field value: i64\n",
        "  prop count: i64 { fn get_count(self: &Self) -> i64 = self.value }\n",
        "}\n",
        "impl Display for Label {\n",
        "  fn to_str(self: &Self) -> Str = to_str(self.count)\n",
        "}\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  emit!(\"before\")\n",
        "  let stringify: Fn(Label) -> Str = to_str\n",
        "  yield Ok(stringify(Label { value: 5 }))\n",
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
fn display_adapter_value_rejects_unsupported_scalar_hook() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn main!() -> Res[Str, Str] {\n",
        "  let stringify: Fn(f32) -> Str = to_str\n",
        "  yield Ok(\"unused\")\n",
        "}\n",
    ));

    assert_eq!(
        execute(&program, &profile, &catalog, EvaluationLimits::default()),
        ExecutionAttempt::Unsupported(SupportReason::Feature(
            eglur_driver::UnsupportedFeature::HookArgumentType,
        ))
    );
}

#[test]
fn named_function_values_execute_through_locals_and_parameters() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn identity(value: Str) -> Str = value\n",
        "fn invoke(f: Fn(Str) -> Str) -> Str = f(\"passed\")\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let f: Fn(Str) -> Str = identity\n",
        "  yield Ok(invoke(f))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "passed"
    );
}

#[test]
fn reexported_named_function_value_keeps_its_source_target() {
    let (program, catalog, profile) = compile_files(
        [
            ("dep.eglur", "pub fn identity(value: Str) -> Str = value\n"),
            ("bridge.eglur", "pub import dep::identity as routed\n"),
            (
                "main.eglur",
                concat!(
                    "import bridge::routed\n",
                    "fn main!() -> Res[Str, Str] {\n",
                    "  let f: Fn(Str) -> Str = routed\n",
                    "  yield Ok(f(\"reexported\"))\n",
                    "}\n",
                ),
            ),
        ],
        "main.eglur",
    );

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "reexported"
    );
}

#[test]
fn reexported_overload_function_value_selects_the_expected_fn_target() {
    let (program, catalog, profile) = compile_files(
        [
            (
                "dep.eglur",
                concat!(
                    "pub overload fn identity(value: i64) -> i64 = value\n",
                    "pub overload fn identity(value: Str) -> Str = \"wrong\"\n",
                ),
            ),
            ("bridge.eglur", "pub import dep::identity as routed\n"),
            (
                "main.eglur",
                concat!(
                    "import bridge::routed\n",
                    "fn main!() -> Res[Str, Str] {\n",
                    "  let f: Fn(i64) -> i64 = routed\n",
                    "  yield Ok(to_str(f(7)))\n",
                    "}\n",
                ),
            ),
        ],
        "main.eglur",
    );

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn callable_value_variance_preserves_reference_argument_passing() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn read(value: &i64) -> i64 = *value\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let mut value = 7\n",
        "  let f: Fn(&mut i64) -> i64 = read\n",
        "  yield Ok(to_str(f(&mut value)))\n",
        "}\n",
    ));

    assert_eq!(ok_str(completed(&program, &catalog, &profile).value()), "7");
}

#[test]
fn callable_value_target_is_selected_by_runtime_control_flow() {
    let (program, catalog, profile) = compile_single(concat!(
        "fn left(value: Str) -> Str = value\n",
        "fn right(value: Str) -> Str = \"right\"\n",
        "fn main!() -> Res[Str, Str] {\n",
        "  let f: Fn(Str) -> Str = if false { yield left } else { yield right }\n",
        "  yield Ok(f(\"left\"))\n",
        "}\n",
    ));

    assert_eq!(
        ok_str(completed(&program, &catalog, &profile).value()),
        "right"
    );
}

#[test]
fn callable_values_in_public_carriers_are_refused_before_execution() {
    for (declaration, expression) in [
        ("", "Some(identity)"),
        ("", "Some((identity,))"),
        (
            "struct Holder { field callback: Fn(Str) -> Str }\n",
            "Holder { callback: identity }",
        ),
    ] {
        let source = format!(
            "import std::test::emit!\n\
             fn identity(value: Str) -> Str = value\n\
             {declaration}\
             fn main!() -> Res[Str, Str] {{\n\
               emit!(\"before\")\n\
               let wrapped = {expression}\n\
               yield Ok(\"ready\")\n\
             }}\n"
        );
        let (program, catalog, profile) = compile_single(&source);
        assert_eq!(
            execute(&program, &profile, &catalog, EvaluationLimits::default()),
            ExecutionAttempt::Unsupported(SupportReason::Feature(
                eglur_driver::UnsupportedFeature::FunctionValue,
            )),
            "{expression}"
        );
    }
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
