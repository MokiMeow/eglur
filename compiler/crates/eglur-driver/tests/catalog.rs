use std::collections::BTreeMap;

use eglur_diagnostics::DiagnosticSnapshot;
use eglur_driver::{
    CompilerSession, FileSetRequest, Phase, RuntimeHook, SessionOptions, SupportProfile,
    SupportProfileId, SupportSchemaVersion, TrustedCatalog,
};
use eglur_sema::{
    CallableRequirement, CanonicalTarget, CompileCapability, DeclarationKind, DeclarationSignature,
    DemandDisposition, EffectCategory, EffectSummary, ExternalDefId, ExternalSemanticOwner,
    FactState, IntrinsicNominal, KnownEffectSummary, LanguageItem, ResolveCapability, StateScope,
    TensorSourceConstructor, TrustedNominalInhabitance, TrustedSignatureKind, TypeKind,
    TypecheckCapability,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct EffectDocument {
    profiles: BTreeMap<Box<str>, EffectProfile>,
    default_own_summary: Box<str>,
    callables: BTreeMap<Box<str>, Vec<DocumentedCallable>>,
}

#[derive(Deserialize)]
struct EffectProfile {
    kind: Box<str>,
    #[serde(default)]
    categories: Vec<Box<str>>,
    state_scope: Option<Box<str>>,
}

#[derive(Deserialize)]
struct DocumentedCallable {
    declaration: Box<str>,
    own_summary: Box<str>,
}

fn external_id(value: &str) -> ExternalDefId {
    ExternalDefId::new(value).unwrap()
}

fn first_slice_profile() -> SupportProfile {
    SupportProfile::compile_only(
        SupportSchemaVersion::V1,
        SupportProfileId::new("core-v1").unwrap(),
        [
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
        ],
    )
    .unwrap()
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

fn documented_effect(document: &EffectDocument, module: &str, declaration: &str) -> EffectSummary {
    let profile = document
        .callables
        .get(module)
        .and_then(|callables| {
            callables
                .iter()
                .find(|callable| callable.declaration.as_ref() == declaration)
        })
        .map_or(document.default_own_summary.as_ref(), |callable| {
            callable.own_summary.as_ref()
        });
    let profile = &document.profiles[profile];
    match profile.kind.as_ref() {
        "Unknown" => EffectSummary::Unknown,
        "Known" => {
            let categories = profile
                .categories
                .iter()
                .map(|category| match category.as_ref() {
                    "IO" => EffectCategory::Io,
                    "STATE" => EffectCategory::State,
                    "NONDET" => EffectCategory::Nondet,
                    other => panic!("unknown effect category {other}"),
                });
            let state_scope = match profile.state_scope.as_deref() {
                Some("none") => StateScope::None,
                Some("localizable") => StateScope::Localizable,
                Some("nonlocal") => StateScope::Nonlocal,
                other => panic!("invalid known-effect state scope {other:?}"),
            };
            EffectSummary::Known(KnownEffectSummary::new(categories, state_scope).unwrap())
        }
        other => panic!("unknown effect profile kind {other}"),
    }
}

#[test]
fn first_slice_catalog_keeps_constructor_callable_effect_and_hook_facts_distinct() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let none = external_id("std::opt::None");
    let wrapping_add = external_id("std::prelude::wrapping_add");
    let emit = external_id("std::test::emit!");
    let print = external_id("std::io::print!");
    let numeric_error = external_id("std::prelude::NumericError");
    let text_error = external_id("std::io::TextError");

    assert_eq!(
        catalog.names().declaration(&none).unwrap().kind(),
        DeclarationKind::IntrinsicConstructor
    );
    assert!(matches!(
        catalog.signatures().declaration(&none).unwrap().kind(),
        TrustedSignatureKind::Constructor {
            payload: eglur_sema::TrustedConstructorPayload::Unit,
            ..
        }
    ));
    assert!(matches!(
        catalog
            .signatures()
            .declaration(&wrapping_add)
            .unwrap()
            .kind(),
        TrustedSignatureKind::Callable { requirements, .. }
            if matches!(
                requirements.as_ref(),
                [eglur_sema::TrustedCallableRequirement::FixedWidthInteger(
                    eglur_sema::TrustedTypeExpr::GenericParameter(0)
                )]
            )
    ));

    assert!(matches!(
        catalog.effects().summary(&emit),
        Some(EffectSummary::Known(summary))
            if summary.categories() == [EffectCategory::State]
                && summary.state_scope() == StateScope::Nonlocal
    ));
    assert!(matches!(
        catalog.inhabitance().rule(&numeric_error),
        Some(TrustedNominalInhabitance::Unknown)
    ));
    assert!(catalog.inhabitance().rule(&text_error).is_none());
    assert!(matches!(
        catalog.signatures().declaration(&text_error).unwrap().kind(),
        TrustedSignatureKind::Enum { variants, .. } if variants.len() == 2
    ));
    assert_eq!(catalog.runtime_hook(&none), Some(RuntimeHook::OptNone));
    assert_eq!(catalog.runtime_hook(&emit), Some(RuntimeHook::TestEmit));
    assert_eq!(catalog.runtime_hook(&print), None);
    assert_eq!(
        catalog
            .language_items()
            .declaration(LanguageItem::NumericError),
        Some(&numeric_error)
    );
    assert_eq!(
        catalog.language_items().declaration(LanguageItem::OptNone),
        Some(&none)
    );
}

#[test]
fn first_slice_catalog_routes_every_dtype_bracket_constructor_to_tensor_semantics() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    for (identity, constructor) in [
        ("std::tensor::zeros", TensorSourceConstructor::Zeros),
        ("std::tensor::ones", TensorSourceConstructor::Ones),
        ("std::tensor::nan", TensorSourceConstructor::Nan),
        ("std::tensor::rand!", TensorSourceConstructor::Rand),
    ] {
        let id = external_id(identity);
        let declaration = catalog.names().declaration(&id).unwrap();

        assert_eq!(
            declaration.semantic_owner(),
            ExternalSemanticOwner::TensorSource(constructor)
        );
        assert_eq!(declaration.kind(), DeclarationKind::Function);
        assert!(catalog.signatures().declaration(&id).is_none());
    }
}

#[test]
fn first_slice_catalog_exposes_stably_ordered_selected_facts() {
    let catalog = TrustedCatalog::first_slice().unwrap();
    let names = catalog
        .names()
        .declarations()
        .map(|declaration| declaration.id().as_str())
        .collect::<Vec<_>>();
    let signatures = catalog
        .signatures()
        .declarations()
        .map(|declaration| declaration.id().as_str())
        .collect::<Vec<_>>();
    let effects = catalog
        .effects()
        .summaries()
        .map(|summary| summary.declaration().as_str())
        .collect::<Vec<_>>();
    let inhabitance = catalog
        .inhabitance()
        .facts()
        .map(|fact| fact.declaration().as_str())
        .collect::<Vec<_>>();
    let hooks = catalog
        .runtime_hooks()
        .map(|(declaration, _)| declaration.as_str())
        .collect::<Vec<_>>();

    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(signatures.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(effects.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(inhabitance.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(hooks.windows(2).all(|pair| pair[0] < pair[1]));
    for primitive in [
        "Bool",
        "i8",
        "i16",
        "i32",
        "i64",
        "u8",
        "u16",
        "u32",
        "u64",
        "Nat",
        "PositiveNat",
        "f32",
        "f64",
        "Str",
    ] {
        let implementation = external_id(&format!("std::prelude::impl::Display::{primitive}"));
        assert!(matches!(
            catalog
                .signatures()
                .declaration(&implementation)
                .map(|declaration| declaration.kind()),
            Some(TrustedSignatureKind::Impl { .. })
        ));
    }
}

#[test]
fn first_slice_effects_match_the_normative_effect_summary_profiles() {
    let document: EffectDocument = serde_yaml::from_str(include_str!(
        "../../../../docs/spec/stdlib/effect-summaries.yaml"
    ))
    .unwrap();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let selected = [
        (
            "std::tensor::zeros",
            "std::tensor",
            "fn zeros[T](dims: Nat...) -> Tensor[T]",
        ),
        (
            "std::tensor::ones",
            "std::tensor",
            "fn ones[T](dims: Nat...) -> Tensor[T]",
        ),
        (
            "std::tensor::nan",
            "std::tensor",
            "fn nan[f32](dims: Nat...) -> Tensor[f32]",
        ),
        (
            "std::tensor::rand!",
            "std::tensor",
            "fn rand![f32](dims: Nat...) -> Tensor[f32]",
        ),
        (
            "std::prelude::to_str",
            "std::prelude",
            "fn to_str[T](value: T) -> Str requires Display[T]",
        ),
        (
            "std::prelude::wrapping_add",
            "std::prelude",
            "fn wrapping_add[T](lhs: T, rhs: T) -> T requires FixedWidthInteger[T]",
        ),
        (
            "std::prelude::wrapping_sub",
            "std::prelude",
            "fn wrapping_sub[T](lhs: T, rhs: T) -> T requires FixedWidthInteger[T]",
        ),
        (
            "std::prelude::wrapping_mul",
            "std::prelude",
            "fn wrapping_mul[T](lhs: T, rhs: T) -> T requires FixedWidthInteger[T]",
        ),
        (
            "std::io::print!",
            "std::io",
            "fn print!(s: Str) -> Res[Unit, TextError]",
        ),
        ("std::test::emit!", "std::test", "fn emit!(s: Str) -> Unit"),
        (
            "std::test::abort!",
            "std::test",
            "fn abort!(code: Str, message: Str) -> Never",
        ),
    ];

    for (identity, module, declaration) in selected {
        assert_eq!(
            catalog.effects().summary(&external_id(identity)),
            Some(&documented_effect(&document, module, declaration)),
            "effect drift for {identity}"
        );
    }
}

#[test]
fn selected_catalog_records_resolve_into_one_canonical_signature_graph() {
    let source = concat!(
        "fn maybe() -> Opt[i64] = None\n",
        "fn some() -> Opt[i64] = Some(1)\n",
        "fn wrap(x: i64) -> i64 = wrapping_add(x, x)\n",
        "fn main!() -> Res[Str, NumericError] = Ok(to_str(wrap(1)))\n",
    );
    let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
    let profile = first_slice_profile();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::new(true)).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    let resolution = analysis.resolution().unwrap();
    let signatures = resolution.signatures();
    let none = CanonicalTarget::External(external_id("std::opt::None"));
    let wrapping_add = CanonicalTarget::External(external_id("std::prelude::wrapping_add"));

    let FactState::Known(DeclarationSignature::Constructor(constructor)) =
        signatures.declaration(&none)
    else {
        panic!("None did not resolve as a canonical constructor");
    };
    assert!(matches!(
        constructor.payload(),
        eglur_sema::ConstructorPayloadSignature::Unit
    ));
    assert_eq!(constructor.generic_parameters().len(), 1);
    assert!(matches!(
        signatures.types().kind(constructor.result()).unwrap(),
        TypeKind::Nominal { head, .. }
            if head == &eglur_sema::NominalHead::Intrinsic(IntrinsicNominal::Opt)
    ));

    let FactState::Known(DeclarationSignature::Callable(callable_id)) =
        signatures.declaration(&wrapping_add)
    else {
        panic!("wrapping_add did not resolve as a canonical callable");
    };
    assert!(matches!(
        signatures.callable(*callable_id).unwrap().requirements(),
        [CallableRequirement::FixedWidthInteger(_)]
    ));
    assert!(
        session
            .observations()
            .iter()
            .any(|observation| observation.phase() == Phase::DeclarationSignatures)
    );
    assert!(matches!(
        session.into_diagnostics(None),
        DiagnosticSnapshot::Accepted(_)
    ));
}

#[test]
fn partial_catalog_makes_missing_explicit_std_surface_unsupported_not_a_language_error() {
    let request = FileSetRequest::single(
        "main.eglur",
        b"import std::io::read_text!\nfn main!() -> Unit = ()\n".to_vec(),
    )
    .unwrap();
    let profile = first_slice_profile();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(analysis.trace().entries().iter().any(|entry| {
        entry.capability()
            == CompileCapability::Resolve(ResolveCapability::ResolveCompleteStandardLibraryNames)
            && entry.disposition() == DemandDisposition::Missing
    }));
    assert!(!diagnostic_codes(&session.into_diagnostics(None)).contains(&"IMPORT_NOT_FOUND"));
}

#[test]
fn partial_catalog_covers_unknown_std_modules_and_qualified_values() {
    for source in [
        "import std::future::thing\nfn main() -> Unit = ()\n",
        "fn main() -> i64 = std::io::missing\n",
    ] {
        let request = FileSetRequest::single("main.eglur", source.as_bytes().to_vec()).unwrap();
        let profile = first_slice_profile();
        let catalog = TrustedCatalog::first_slice().unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

        let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
        assert!(
            analysis.trace().entries().iter().any(|entry| {
                entry.capability()
                    == CompileCapability::Resolve(
                        ResolveCapability::ResolveCompleteStandardLibraryNames,
                    )
                    && entry.disposition() == DemandDisposition::Missing
            }),
            "missing complete-stdlib demand for {source:?}"
        );
        let snapshot = session.into_diagnostics(None);
        let codes = diagnostic_codes(&snapshot);
        assert!(!codes.contains(&"IMPORT_NOT_FOUND"));
        assert!(!codes.contains(&"NAME_NOT_FOUND"));
    }
}

#[test]
fn partial_catalog_does_not_turn_an_ordinary_missing_name_into_unsupported() {
    let request =
        FileSetRequest::single("main.eglur", b"fn main() -> i64 = missing\n".to_vec()).unwrap();
    let profile = first_slice_profile();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(!analysis.trace().has_missing());
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["NAME_NOT_FOUND"]
    );
}

#[test]
fn partial_catalog_preserves_namespace_errors_for_represented_std_names() {
    let request = FileSetRequest::single(
        "main.eglur",
        b"fn main() -> i64 = std::io::TextError\n".to_vec(),
    )
    .unwrap();
    let profile = first_slice_profile();
    let catalog = TrustedCatalog::first_slice().unwrap();
    let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();

    let analysis = session.resolve(&profile, Some(&catalog)).unwrap();
    assert!(!analysis.trace().entries().iter().any(|entry| {
        entry.capability()
            == CompileCapability::Resolve(ResolveCapability::ResolveCompleteStandardLibraryNames)
    }));
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        ["NAME_NOT_FOUND"]
    );
}
