use super::*;
use crate::{
    EvaluationLimits, ExecutionAttempt, ExecutionOutcome, ProgramMode, SealedCompilation,
    SupportReason, Value, execute,
};
use eglur_sema::{CanonicalTarget, CompileCapability, InvocationTarget, TypecheckCapability};

const PACKET: &str = "std::fixture::Packet";

#[derive(Default)]
struct MemberBinders {
    owner: Vec<TrustedGenericParameter>,
    method: Vec<TrustedGenericParameter>,
}

fn member(
    id: &str,
    member_name: &str,
    pattern: TrustedTypeExpr,
    form: InherentMemberForm,
    binders: MemberBinders,
    arguments: Vec<TrustedTypeExpr>,
    result: TrustedTypeExpr,
) -> (TrustedDeclarationSignature, TrustedInherentMember) {
    let bang = member_name.ends_with('!');
    let owner_count = binders.owner.len();
    let mut generics = binders.owner;
    generics.extend(binders.method);
    let mut parameters = Vec::new();
    match form {
        InherentMemberForm::Associated => {}
        InherentMemberForm::ValueReceiver => parameters.push(parameter("self", pattern.clone())),
        InherentMemberForm::SharedReceiver | InherentMemberForm::MutableReceiver => parameters
            .push(parameter(
                "self",
                TrustedTypeExpr::Reference {
                    mutability: if form == InherentMemberForm::MutableReceiver {
                        ReferenceMutability::Mutable
                    } else {
                        ReferenceMutability::Shared
                    },
                    referent: Box::new(pattern.clone()),
                },
            )),
    }
    parameters.extend(
        arguments
            .into_iter()
            .enumerate()
            .map(|(index, ty)| parameter(&format!("arg{index}"), ty)),
    );
    let callable = external_id(id);
    (
        TrustedDeclarationSignature::new(
            callable.clone(),
            TrustedSignatureKind::Callable {
                bang,
                generic_parameters: generics.into_boxed_slice(),
                parameters: parameters.into_boxed_slice(),
                result,
                requirements: Box::new([]),
            },
        ),
        TrustedInherentMember {
            callable,
            name: name(member_name.trim_end_matches('!'), bang),
            owner_pattern: pattern,
            owner_parameter_count: owner_count,
            form,
            visibility: Visibility::Public,
            overload: false,
        },
    )
}

fn fixture_catalog() -> TrustedCatalog {
    fixture_catalog_for_root(
        TrustedInherentOwnerHead::Nominal(external_id(NUMERIC_ERROR)),
        nominal(external_id(NUMERIC_ERROR), []),
    )
}

fn fixture_catalog_for_root(
    head: TrustedInherentOwnerHead,
    root: TrustedTypeExpr,
) -> TrustedCatalog {
    let TrustedCatalog {
        names,
        signatures,
        inhabitance,
        effects,
        language_items,
        runtime_hooks,
    } = TrustedCatalog::first_slice().unwrap();
    let scalar = TrustedTypeExpr::Primitive(PrimitiveType::I64);
    let unit = TrustedTypeExpr::Primitive(PrimitiveType::Unit);
    let packet = nominal(external_id(PACKET), [TrustedTypeExpr::GenericParameter(0)]);
    let mut members = vec![
        member(
            "fixture::add",
            "add",
            root.clone(),
            InherentMemberForm::Associated,
            MemberBinders::default(),
            vec![scalar.clone(), scalar.clone()],
            scalar.clone(),
        ),
        member(
            "fixture::zero",
            "zero",
            root.clone(),
            InherentMemberForm::Associated,
            MemberBinders::default(),
            vec![],
            scalar.clone(),
        ),
        member(
            "fixture::read",
            "read",
            root.clone(),
            InherentMemberForm::SharedReceiver,
            MemberBinders::default(),
            vec![],
            scalar.clone(),
        ),
        member(
            "fixture::take",
            "take",
            root.clone(),
            InherentMemberForm::ValueReceiver,
            MemberBinders::default(),
            vec![],
            scalar.clone(),
        ),
        member(
            "fixture::touch!",
            "touch!",
            root.clone(),
            InherentMemberForm::MutableReceiver,
            MemberBinders::default(),
            vec![],
            unit.clone(),
        ),
        member(
            "fixture::emit!",
            "emit!",
            root.clone(),
            InherentMemberForm::Associated,
            MemberBinders::default(),
            vec![TrustedTypeExpr::Primitive(PrimitiveType::Str)],
            unit.clone(),
        ),
        member(
            "fixture::private",
            "private",
            root.clone(),
            InherentMemberForm::Associated,
            MemberBinders::default(),
            vec![],
            scalar.clone(),
        ),
        member(
            "fixture::pick_int",
            "pick",
            root.clone(),
            InherentMemberForm::Associated,
            MemberBinders::default(),
            vec![scalar.clone()],
            scalar.clone(),
        ),
        member(
            "fixture::pick_bool",
            "pick",
            root,
            InherentMemberForm::Associated,
            MemberBinders::default(),
            vec![TrustedTypeExpr::Primitive(PrimitiveType::Bool)],
            scalar.clone(),
        ),
        member(
            "fixture::create",
            "create",
            packet.clone(),
            InherentMemberForm::Associated,
            MemberBinders {
                owner: vec![generic("T", [])],
                ..MemberBinders::default()
            },
            vec![],
            packet.clone(),
        ),
        member(
            "fixture::map",
            "map",
            packet.clone(),
            InherentMemberForm::SharedReceiver,
            MemberBinders {
                owner: vec![generic("T", [])],
                method: vec![generic("U", [])],
            },
            vec![TrustedTypeExpr::GenericParameter(1)],
            TrustedTypeExpr::GenericParameter(1),
        ),
        member(
            "fixture::bounded",
            "bounded",
            packet.clone(),
            InherentMemberForm::SharedReceiver,
            MemberBinders {
                owner: vec![generic(
                    "T",
                    [TrustedTraitExpr {
                        declaration: external_id(DISPLAY),
                        arguments: Box::new([]),
                    }],
                )],
                ..MemberBinders::default()
            },
            vec![],
            unit.clone(),
        ),
        member(
            "fixture::diagonal",
            "diagonal",
            nominal(
                external_id(PACKET),
                [TrustedTypeExpr::Tuple(Box::new([
                    TrustedTypeExpr::GenericParameter(0),
                    TrustedTypeExpr::GenericParameter(0),
                ]))],
            ),
            InherentMemberForm::SharedReceiver,
            MemberBinders {
                owner: vec![generic("T", [])],
                ..MemberBinders::default()
            },
            vec![],
            scalar,
        ),
    ];
    let union_owner = nominal(
        external_id(PACKET),
        [TrustedTypeExpr::Intrinsic {
            head: eglur_sema::IntrinsicNominal::Res,
            arguments: Box::new([
                TrustedTypeExpr::Primitive(PrimitiveType::I64),
                TrustedTypeExpr::ErrorUnion(Box::new([
                    TrustedTypeExpr::GenericParameter(0),
                    nominal(external_id(NUMERIC_ERROR), []),
                ])),
            ]),
        }],
    );
    for (id, method_name, form, arguments) in [
        (
            "fixture::union_owner",
            "union_owner",
            InherentMemberForm::SharedReceiver,
            vec![TrustedTypeExpr::GenericParameter(0)],
        ),
        (
            "fixture::union_only",
            "union_only",
            InherentMemberForm::SharedReceiver,
            vec![],
        ),
        (
            "fixture::union_create",
            "union_create",
            InherentMemberForm::Associated,
            vec![TrustedTypeExpr::GenericParameter(0)],
        ),
    ] {
        members.push(member(
            id,
            method_name,
            union_owner.clone(),
            form,
            MemberBinders {
                owner: vec![generic("E", [])],
                ..MemberBinders::default()
            },
            arguments,
            unit.clone(),
        ));
    }
    members[6].1.visibility = Visibility::Private;
    members[7].1.overload = true;
    members[8].1.overload = true;
    let owners = [
        TrustedInherentOwner {
            head: head.clone(),
            members: members[..9]
                .iter()
                .map(|(_, member)| member.clone())
                .collect(),
        },
        TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Nominal(external_id(PACKET)),
            members: members[9..]
                .iter()
                .map(|(_, member)| member.clone())
                .collect(),
        },
        TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Nominal(external_id(TEXT_ERROR)),
            members: Box::new([member(
                "fixture::enum_zero",
                "zero",
                nominal(external_id(TEXT_ERROR), []),
                InherentMemberForm::Associated,
                MemberBinders::default(),
                vec![],
                TrustedTypeExpr::Primitive(PrimitiveType::I64),
            )
            .1]),
        },
    ];
    let enum_callable = member(
        "fixture::enum_zero",
        "zero",
        nominal(external_id(TEXT_ERROR), []),
        InherentMemberForm::Associated,
        MemberBinders::default(),
        vec![],
        TrustedTypeExpr::Primitive(PrimitiveType::I64),
    )
    .0;
    let fixture_module = module(["std", "fixture"]);
    let names = ExternalNameEnvironment::partial_standard_library(
        names.modules().cloned().chain([fixture_module.clone()]),
        names.declarations().cloned().chain([declaration(
            external_id(PACKET),
            fixture_module,
            "Packet",
            false,
            DeclarationKind::OpaqueType,
        )
        .unwrap()]),
        names
            .prelude_bindings()
            .flat_map(|(_, _, ids)| ids.iter().cloned()),
    )
    .unwrap();
    let signatures = TrustedSignatureEnvironment::with_inherent_owners(
        signatures
            .declarations()
            .cloned()
            .chain([TrustedDeclarationSignature::new(
                external_id(PACKET),
                TrustedSignatureKind::Nominal {
                    generic_parameters: Box::new([generic("T", [])]),
                },
            )])
            .chain(members.iter().map(|(callable, _)| callable.clone()))
            .chain([enum_callable]),
        signatures
            .inherent_owners()
            .filter(|owner| owner.head != head)
            .cloned()
            .chain(owners),
    )
    .unwrap();
    let inhabitance = TrustedInhabitanceEnvironment::new(inhabitance.facts().cloned().chain([
        TrustedInhabitanceFact::new(external_id(PACKET), TrustedNominalInhabitance::Unknown),
    ]))
    .unwrap();
    let state = EffectSummary::Known(
        KnownEffectSummary::new([EffectCategory::State], StateScope::Nonlocal).unwrap(),
    );
    let effects = TrustedEffectEnvironment::new(
        effects
            .summaries()
            .cloned()
            .chain(members.iter().map(|(callable, _)| {
                TrustedEffectSummary::new(
                    callable.id().clone(),
                    if callable.id().as_str().ends_with('!') {
                        state.clone()
                    } else {
                        EffectSummary::pure()
                    },
                )
            }))
            .chain([TrustedEffectSummary::new(
                external_id("fixture::enum_zero"),
                EffectSummary::pure(),
            )]),
    )
    .unwrap();
    TrustedCatalog::new_with_language_items(
        names,
        signatures,
        inhabitance,
        effects,
        language_items,
        runtime_hooks.into_iter().chain([
            (external_id("fixture::add"), RuntimeHook::WrappingAdd),
            (external_id("fixture::emit!"), RuntimeHook::TestEmit),
        ]),
    )
    .unwrap()
}

fn analyze(
    files: &[(&str, &str)],
    catalog: &TrustedCatalog,
    mode: ProgramMode,
    profile: &SupportProfile,
) -> SealedCompilation {
    let request = FileSetRequest::new(
        files
            .iter()
            .map(|(path, source)| FileInput::new(*path, source.as_bytes().to_vec())),
        "main.eglur",
        (files.len() > 1).then_some("."),
    )
    .unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
    session.resolve_for(mode, profile, Some(catalog)).unwrap();
    session.seal(None).unwrap()
}

fn library(source: &str, catalog: &TrustedCatalog) -> SealedCompilation {
    analyze(
        &[("main.eglur", source)],
        catalog,
        ProgramMode::Library,
        &SupportProfile::semantic_core_v1(),
    )
}

fn require_checked(compilation: SealedCompilation) -> crate::CheckedProgram {
    assert!(
        !compilation.trace().has_unsupported(),
        "{:?}",
        compilation.trace()
    );
    assert!(
        compilation.diagnostics().diagnostics().is_empty(),
        "{:?}",
        compilation.diagnostics()
    );
    let program = compilation
        .into_checked_program()
        .expect("successful check seals a program");
    let resolution = program.resolution();
    let bodies = resolution.body_types();
    for instance in bodies
        .invocations()
        .filter_map(|(_, invocation)| match invocation.target() {
            InvocationTarget::Callable(id) => Some(*id),
            _ => None,
        })
        .chain(bodies.callable_values().map(|(_, id)| id))
    {
        let instance = bodies.callable_instance(instance).unwrap();
        let callable = resolution
            .signatures()
            .callable(instance.signature())
            .unwrap();
        assert_eq!(
            resolution
                .signatures()
                .types()
                .substitution(instance.substitution())
                .unwrap()
                .len(),
            callable.generic_parameters().len(),
            "{:?}",
            callable.declaration()
        );
    }
    program
}

#[test]
fn calls_and_named_values_select_the_exact_hidden_external_identity() {
    let catalog = fixture_catalog();
    for (source, id) in [
        (
            "fn read(v: &NumericError) -> i64 = NumericError.read(v)\n",
            "fixture::read",
        ),
        (
            "fn read(v: &NumericError) -> i64 = ((NumericError.read))(v)\n",
            "fixture::read",
        ),
        (
            "fn read(v: &NumericError) -> i64 = v.read()\n",
            "fixture::read",
        ),
        (
            "fn read(v: &NumericError) -> i64 = ((v.read))()\n",
            "fixture::read",
        ),
        (
            "fn take(v: NumericError) -> i64 = v.take()\n",
            "fixture::take",
        ),
        (
            "fn read(v: NumericError) -> i64 = v.read()\n",
            "fixture::read",
        ),
        ("fn zero() -> i64 = NumericError.zero()\n", "fixture::zero"),
        (
            "fn add() -> i64 = NumericError.add(arg1=3, arg0=2)\n",
            "fixture::add",
        ),
        (
            "fn keep() -> Unit { let f: Fn(&NumericError) -> i64 = ((NumericError.read))\n_ = f\nyield () }\n",
            "fixture::read",
        ),
        (
            "fn keep() -> Unit { let f: Fn() -> i64 = NumericError.zero\n_ = f\nyield () }\n",
            "fixture::zero",
        ),
        (
            "import std::io::TextError\nfn zero() -> i64 = TextError.zero()\n",
            "fixture::enum_zero",
        ),
        (
            "type Alias = NumericError\nfn read(v: &Alias) -> i64 = Alias.read(v)\n",
            "fixture::read",
        ),
        (
            "type Alias = NumericError\nfn keep() -> Unit { let f: Fn(&Alias) -> i64 = Alias.read\n_ = f\nyield () }\n",
            "fixture::read",
        ),
    ] {
        let program = require_checked(library(source, &catalog));
        let resolution = program.resolution();
        let bodies = resolution.body_types();
        let targets = bodies
            .invocations()
            .filter_map(|(_, invocation)| match invocation.target() {
                InvocationTarget::Callable(id) => Some(*id),
                _ => None,
            })
            .chain(bodies.callable_values().map(|(_, id)| id))
            .map(|instance| {
                resolution
                    .signatures()
                    .callable(bodies.callable_instance(instance).unwrap().signature())
                    .unwrap()
                    .declaration()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            vec![&CanonicalTarget::External(external_id(id))],
            "{source}"
        );
    }
}

#[test]
fn owner_prefix_and_method_tail_infer_independently_and_keep_bounds() {
    let catalog = fixture_catalog();
    for source in [
        "fn use(v: &Packet[i64]) -> Bool = Packet.map::[Bool](v, true)\n",
        "fn use(v: &Packet[i64]) -> Bool = v.map::[Bool](true)\n",
        "fn keep() -> Unit { let f: Fn(&Packet[i64], Bool) -> Bool = Packet.map\n_ = f\nyield () }\n",
        "type Fixed = Packet[i64]\nfn keep() -> Unit { let f: Fn(&Fixed, Bool) -> Bool = Fixed.map\n_ = f\nyield () }\n",
        "fn create() -> Packet[i64] = Packet.create()\n",
        "type Fixed = Packet[i64]\nfn create() -> Fixed = Fixed.create()\n",
        "fn keep() -> Unit { let f: Fn() -> Packet[i64] = Packet.create\n_ = f\nyield () }\n",
        "type Fixed = Packet[i64]\nfn keep() -> Unit { let f = Fixed.create\n_ = f\nyield () }\n",
        "fn diag(v: &Packet[(Bool, Bool)]) -> i64 = v.diagonal()\n",
        "fn bound(v: &Packet[i64]) -> Unit = v.bounded()\n",
    ] {
        require_checked(library(
            &format!("import std::fixture::Packet\n{source}"),
            &catalog,
        ));
    }
    for source in [
        "struct Bad {}\nfn bad(v: &Packet[Bad]) -> Unit = v.bounded()\n",
        "struct Bad {}\ntype Fixed = Packet[Bad]\nfn bad(v: &Fixed) -> Unit = Fixed.bounded(v)\n",
        "struct Bad {}\ntype Fixed = Packet[Bad]\nfn keep() -> Unit { let f: Fn(&Fixed) -> Unit = Fixed.bounded\n_ = f\nyield () }\n",
    ] {
        let compilation = library(&format!("import std::fixture::Packet\n{source}"), &catalog);
        assert!(!compilation.trace().has_unsupported());
        assert!(compilation.checked_program().is_none(), "{source}");
        assert!(
            compilation
                .diagnostics()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == "TRAIT_BOUND_NOT_SATISFIED"),
            "{source}\n{:?}",
            compilation.diagnostics()
        );
    }
}

#[test]
fn structural_union_owners_use_only_independent_inference_evidence() {
    let catalog = fixture_catalog();
    for union in ["Union[Bool, NumericError]", "Union[NumericError, Bool]"] {
        let prefix =
            format!("import std::fixture::Packet\ntype Fixed = Packet[Res[i64, {union}]]\n");
        for source in [
            "fn use(v: &Fixed) -> Unit = v.union_owner(true)\n",
            "fn use(v: &Fixed) -> Unit = Fixed.union_owner(v, true)\n",
            "fn use() -> Unit = Fixed.union_create(true)\n",
            "fn use() -> Unit { let f: Fn(&Fixed, Bool) -> Unit = Fixed.union_owner\n_ = f\nyield () }\n",
            "fn use() -> Unit { let f: Fn(Bool) -> Unit = Fixed.union_create\n_ = f\nyield () }\n",
        ] {
            require_checked(library(&format!("{prefix}{source}"), &catalog));
        }
        for source in [
            "fn use(v: &Fixed) -> Unit = v.union_only()\n",
            "fn use(v: &Fixed) -> Unit = Fixed.union_only(v)\n",
            "fn use() -> Unit { let f: Fn(&Fixed) -> Unit = Fixed.union_only\n_ = f\nyield () }\n",
        ] {
            let compilation = library(&format!("{prefix}{source}"), &catalog);
            assert!(compilation.checked_program().is_none(), "{source}");
            assert!(
                compilation
                    .diagnostics()
                    .diagnostics()
                    .iter()
                    .any(|d| d.code().as_str() == "TYPE_ARG_INFERENCE_FAILED"),
                "{source}\n{:?}",
                compilation.diagnostics()
            );
        }
        for source in [
            "fn use() -> Unit = Fixed.union_create(\"wrong\")\n",
            "fn use() -> Unit { let f: Fn(Str) -> Unit = Fixed.union_create\n_ = f\nyield () }\n",
        ] {
            let compilation = library(&format!("{prefix}{source}"), &catalog);
            assert!(!compilation.trace().has_unsupported());
            assert!(compilation.checked_program().is_none(), "{source}");
        }
    }
}

#[test]
fn complete_families_enforce_visibility_binding_and_applicability() {
    let catalog = fixture_catalog();
    for (source, code) in [
        (
            "fn bad() -> i64 = NumericError.absent()\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "fn bad() -> i64 = NumericError.private()\n",
            "VISIBILITY_VIOLATION",
        ),
        (
            "fn bad(v: &NumericError) -> i64 = v.private()\n",
            "VISIBILITY_VIOLATION",
        ),
        (
            "fn bad(v: &NumericError) -> i64 = v.zero()\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "fn bad() -> i64 = NumericError.read()\n",
            "CALL_MISSING_ARG",
        ),
        (
            "fn bad() -> i64 = NumericError.add(arg2=2, arg1=3)\n",
            "CALL_UNKNOWN_NAMED_ARG",
        ),
        (
            "fn bad(v: &NumericError) -> Unit = v.touch!()\n",
            "METHOD_NOT_FOUND",
        ),
        (
            "import std::fixture::Packet\nfn bad(v: &Packet[(Bool, i64)]) -> i64 = v.diagonal()\n",
            "METHOD_NOT_FOUND",
        ),
    ] {
        let compilation = library(source, &catalog);
        assert!(!compilation.trace().has_unsupported(), "{source}");
        assert!(compilation.checked_program().is_none());
        assert!(
            compilation
                .diagnostics()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == code),
            "{source}\n{:?}",
            compilation.diagnostics()
        );
    }
}

#[test]
fn overloads_and_reexports_preserve_ordinary_candidate_precedence() {
    let catalog = fixture_catalog();
    for source in [
        "fn pick() -> i64 = NumericError.pick(2)\n",
        "fn pick() -> i64 = NumericError.pick(true)\n",
        "fn keep() -> Unit { let f: Fn(Bool) -> i64 = NumericError.pick\n_ = f\nyield () }\n",
    ] {
        require_checked(library(source, &catalog));
    }
    require_checked(analyze(
        &[
            (
                "main.eglur",
                "import bridge::Error\nfn read(v: &Error) -> i64 = Error.read(v)\n",
            ),
            (
                "bridge.eglur",
                "pub import std::prelude::NumericError as Error\n",
            ),
        ],
        &catalog,
        ProgramMode::Library,
        &SupportProfile::semantic_core_v1(),
    ));
}

#[test]
fn effectful_members_keep_their_summary_and_cannot_become_pure_fn_values() {
    let catalog = fixture_catalog();
    let compilation = library(
        "fn bad() -> Unit = NumericError.emit!(\"event\")\n",
        &catalog,
    );
    assert!(!compilation.trace().has_unsupported());
    assert!(
        compilation
            .diagnostics()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code().as_str() == "EFFECT_VIOLATION"),
        "{:?}",
        compilation.diagnostics()
    );
    require_checked(library(
        "fn good!() -> Unit = NumericError.emit!(\"event\")\n",
        &catalog,
    ));
    require_checked(library(
        "fn good!(v: &mut NumericError) -> Unit = v.touch!()\n",
        &catalog,
    ));
    let compilation = library(
        "fn keep() -> Unit { let f: Fn(Str) -> Unit = NumericError.emit!\n_ = f\nyield () }\n",
        &catalog,
    );
    assert!(compilation.checked_program().is_none());
    assert!(compilation.trace().missing().any(|entry| entry.capability()
        == CompileCapability::Typecheck(TypecheckCapability::CheckClosures)));
}

#[test]
fn primitive_fixture_families_preserve_identity_overloads_visibility_and_effects() {
    let catalog = fixture_catalog_for_root(
        TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
        TrustedTypeExpr::Primitive(PrimitiveType::I64),
    );
    for (source, identity) in [
        ("fn pick() -> i64 = i64.pick(2)\n", "fixture::pick_int"),
        ("fn pick() -> i64 = i64.pick(true)\n", "fixture::pick_bool"),
        ("fn read(v: i64) -> i64 = v.read()\n", "fixture::read"),
        (
            "type Count = i64\nfn read(v: &Count) -> i64 = Count.read(v)\n",
            "fixture::read",
        ),
        (
            "fn keep() -> Unit { let f: Fn(Bool) -> i64 = i64.pick\n_ = f\nyield () }\n",
            "fixture::pick_bool",
        ),
        (
            "fn good!() -> Unit = i64.emit!(\"event\")\n",
            "fixture::emit!",
        ),
    ] {
        let program = require_checked(library(source, &catalog));
        let resolution = program.resolution();
        let bodies = resolution.body_types();
        let targets = bodies
            .invocations()
            .filter_map(|(_, call)| match call.target() {
                InvocationTarget::Callable(id) => Some(*id),
                _ => None,
            })
            .chain(bodies.callable_values().map(|(_, id)| id))
            .map(|instance| {
                resolution
                    .signatures()
                    .callable(bodies.callable_instance(instance).unwrap().signature())
                    .unwrap()
                    .declaration()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            targets,
            vec![&CanonicalTarget::External(external_id(identity))],
            "{source}"
        );
    }
    for (source, code) in [
        ("fn bad() -> i64 = i64.private()\n", "VISIBILITY_VIOLATION"),
        (
            "fn bad(v: &i64) -> i64 = v.private()\n",
            "VISIBILITY_VIOLATION",
        ),
        (
            "fn bad() -> Unit = i64.emit!(\"event\")\n",
            "EFFECT_VIOLATION",
        ),
    ] {
        let compilation = library(source, &catalog);
        assert!(!compilation.trace().has_unsupported(), "{source}");
        assert!(compilation.checked_program().is_none());
        assert!(
            compilation
                .diagnostics()
                .diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.code().as_str() == code),
            "{source}\n{:?}",
            compilation.diagnostics()
        );
    }
}

#[test]
fn associated_hook_executes_direct_grouped_and_named_fn_forms() {
    let profile = SupportProfile::semantic_core_v1();
    for (catalog, owner) in [
        (fixture_catalog(), "NumericError"),
        (
            fixture_catalog_for_root(
                TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
                TrustedTypeExpr::Primitive(PrimitiveType::I64),
            ),
            "i64",
        ),
    ] {
        for body in [
            format!("yield Ok(to_str({owner}.add(2, 3)))"),
            format!("yield Ok(to_str((({owner}.add))(2, 3)))"),
            format!("let f: Fn(i64, i64) -> i64 = {owner}.add\nyield Ok(to_str(f(2, 3)))"),
        ] {
            let source = format!("fn main!() -> Res[Str, Str] {{ {body} }}\n");
            let program = require_checked(analyze(
                &[("main.eglur", &source)],
                &catalog,
                ProgramMode::Executable,
                &profile,
            ));
            let ExecutionAttempt::Executed(ExecutionOutcome::Completed(result)) =
                execute(&program, &profile, &catalog, EvaluationLimits::default())
            else {
                panic!("associated hook did not execute for {owner}");
            };
            let Value::Variant { payload, .. } = result.value() else {
                panic!("expected result");
            };
            assert_eq!(payload.as_ref(), &[Value::Str("5".into())]);
            assert!(result.events().is_empty());
        }
    }
}

#[test]
fn missing_hooks_refuse_before_effects_even_for_unused_named_values() {
    let catalog = fixture_catalog();
    let profile = SupportProfile::semantic_core_v1();
    for expression in [
        "_ = NumericError.zero()",
        "let f: Fn() -> i64 = NumericError.zero\n_ = f",
    ] {
        let source = format!(
            "fn main!() -> Unit {{ NumericError.emit!(\"must not execute\")\n{expression}\nyield () }}\n"
        );
        let program = require_checked(analyze(
            &[("main.eglur", &source)],
            &catalog,
            ProgramMode::Executable,
            &profile,
        ));
        assert_eq!(
            execute(&program, &profile, &catalog, EvaluationLimits::default()),
            ExecutionAttempt::Unsupported(SupportReason::MissingRuntimeHook(external_id(
                "fixture::zero"
            )))
        );
    }
}

#[test]
fn explicit_method_arguments_never_include_owner_prefix_binders() {
    let catalog = fixture_catalog();
    for source in [
        "import std::fixture::Packet\nfn bad(v: &Packet[i64]) -> Bool = Packet.map::[i64, Bool](v, true)\n",
        "import std::fixture::Packet\nfn bad(v: &Packet[i64]) -> Bool = v.map::[i64, Bool](true)\n",
        "import std::fixture::Packet\nfn bad(v: &Packet[i64]) -> Bool = ((Packet.map))::[i64, Bool](v, true)\n",
        "struct Item[T] { fn map[U](self: &Self, v: U) -> U = v }\nfn bad(v: &Item[i64]) -> Bool = Item.map::[i64, Bool](v, true)\n",
        "struct Item[T] { fn map[U](self: &Self, v: U) -> U = v }\nfn bad(v: &Item[i64]) -> Bool = v.map::[i64, Bool](true)\n",
        "trait Echo { fn echo[T](self: &Self, v: T) -> T }\nimpl Echo for Bool { fn echo[T](self: &Self, v: T) -> T = v }\nfn bad(v: &Bool) -> i64 = Echo.echo::[i64, Bool](v, 2)\n",
    ] {
        let compilation = library(source, &catalog);
        assert!(!compilation.trace().has_unsupported(), "{source}");
        assert_eq!(
            compilation
                .diagnostics()
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str())
                .collect::<Vec<_>>(),
            ["TYPE_ARG_INFERENCE_FAILED"],
            "{source}"
        );
    }
    let source = "import std::fixture::Packet\nfn bad(v: &Packet[i64]) -> Bool = Packet.map[Bool](v, true)\n";
    let compilation = library(source, &catalog);
    assert_eq!(
        compilation.diagnostics().diagnostics()[0].code().as_str(),
        "TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN"
    );
}

#[test]
fn absent_inherent_families_allow_only_receiver_trait_fallback() {
    let catalog = fixture_catalog();
    let prefix = "trait Rank { fn rank(self: &Self) -> i64 }\nimpl Rank for NumericError { fn rank(self: &Self) -> i64 = 9 }\n";
    require_checked(library(
        &format!("{prefix}fn good(v: &NumericError) -> i64 = v.rank()\n"),
        &catalog,
    ));
    for source in [
        format!("{prefix}fn bad(v: &NumericError) -> i64 = NumericError.rank(v)\n"),
        "trait Zero { fn zero(self: &Self) -> i64 }\nimpl Zero for NumericError { fn zero(self: &Self) -> i64 = 9 }\nfn bad(v: &NumericError) -> i64 = v.zero()\n".into(),
        "import std::fixture::Packet\ntrait Diag { fn diagonal(self: &Self) -> i64 }\nimpl Diag for Packet[(Bool, i64)] { fn diagonal(self: &Self) -> i64 = 9 }\nfn bad(v: &Packet[(Bool, i64)]) -> i64 = v.diagonal()\n".into(),
    ] {
        let compilation = library(&source, &catalog);
        assert!(!compilation.trace().has_unsupported());
        assert_eq!(compilation.diagnostics().diagnostics().iter().map(|diagnostic| diagnostic.code().as_str()).collect::<Vec<_>>(), ["METHOD_NOT_FOUND"], "{source}");
    }
}

#[test]
fn inapplicable_owner_patterns_and_callable_overloads_do_not_become_closure_gaps() {
    let catalog = fixture_catalog();
    for (source, code) in [
        (
            "import std::fixture::Packet\ntype Fixed = Packet[(Bool, i64)]\nfn keep() -> Unit { let f: Fn(&Fixed) -> i64 = Fixed.diagonal\n_ = f\nyield () }\n",
            "TYPE_ARG_INFERENCE_FAILED",
        ),
        (
            "fn keep() -> Unit { let f: Fn(Str) -> i64 = NumericError.pick\n_ = f\nyield () }\n",
            "CALLABLE_VALUE_NO_MATCH",
        ),
        (
            "fn keep() -> Unit { let f: Fn(Never) -> i64 = NumericError.pick\n_ = f\nyield () }\n",
            "OVERLOAD_AMBIGUOUS",
        ),
        (
            "fn keep() -> Unit { let f = NumericError.pick\n_ = f\nyield () }\n",
            "TYPE_ARG_INFERENCE_FAILED",
        ),
        (
            "fn bad() -> i64 = NumericError.pick(\"wrong\")\n",
            "CALL_ARG_TYPE_MISMATCH",
        ),
        (
            "import std::fixture::Packet\ntype Fixed = Packet[i64]\nfn bad(v: &Packet[Bool]) -> Bool = Fixed.map(v, true)\n",
            "CALL_ARG_TYPE_MISMATCH",
        ),
        (
            "import std::fixture::Packet\ntype Fixed = Packet[i64]\nfn keep() -> Unit { let f: Fn(&Packet[Bool], Bool) -> Bool = Fixed.map\n_ = f\nyield () }\n",
            "TYPE_ARG_INFERENCE_FAILED",
        ),
    ] {
        let compilation = library(source, &catalog);
        assert!(!compilation.trace().has_unsupported(), "{source}");
        assert_eq!(
            compilation
                .diagnostics()
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.code().as_str())
                .collect::<Vec<_>>(),
            [code],
            "{source}"
        );
    }
}

#[test]
fn inventory_coverage_and_profile_selection_refuse_independently() {
    let catalog = fixture_catalog();
    let core = SupportProfile::semantic_core_v1();
    let reduced = SupportProfile::new(
        core.schema(),
        crate::SupportProfileId::new("without-trusted-inherent").unwrap(),
        core.compile_capabilities().filter(|capability| {
            *capability
                != CompileCapability::Typecheck(TypecheckCapability::ResolveTrustedInherentMethods)
        }),
        core.execution_capabilities(),
    )
    .unwrap();
    for source in [
        "fn use(v: &NumericError) -> i64 = v.read()\n",
        "fn use(v: &NumericError) -> i64 = NumericError.read(v)\n",
        "fn keep() -> Unit { let f: Fn(&NumericError) -> i64 = NumericError.read\n_ = f\nyield () }\n",
    ] {
        let compilation = analyze(
            &[("main.eglur", source)],
            &catalog,
            ProgramMode::Library,
            &reduced,
        );
        assert!(compilation.checked_program().is_none());
        assert!(compilation.diagnostics().diagnostics().is_empty());
        let entry = compilation.trace().unsupported().next().unwrap();
        assert_eq!(
            entry.capability(),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTrustedInherentMethods)
        );
        assert!(entry.input_refusals().is_empty());
    }
    let source = "import std::io::IoError\ntrait Read { fn read(self: &Self) -> i64 }\nimpl Read for IoError { fn read(self: &Self) -> i64 = 9 }\nfn use(v: &IoError) -> i64 = v.read()\n";
    let compilation = library(source, &catalog);
    assert!(compilation.trace().has_unsupported());
    assert!(compilation.trace().missing().next().is_none());
    assert!(compilation.diagnostics().diagnostics().is_empty());
    assert!(compilation.checked_program().is_none());
}
