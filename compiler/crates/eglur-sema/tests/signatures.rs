use std::collections::BTreeSet;

use eglur_diagnostics::{DiagnosticSnapshot, DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::{HirFile, lower};
use eglur_sema::{
    AssociatedProjection, CanonicalTarget, CapabilityDemand, CompileCapability, CompileSupport,
    DeclarationCollection, DeclarationKind, DeclarationSignature, DemandDisposition,
    EnumVariantPayloadSignature, ExternalDeclaration, ExternalDefId, ExternalNameEnvironment,
    FactState, GenericParameterId, ImportAnalysisUnit, Inhabitance, InhabitanceFacts,
    IntrinsicNominal, LanguageItem, ModuleAddress, NameKey, NominalHead, PhaseProduct,
    RawPointerMutability, ReferenceMutability, ResolveCapability, SemanticSource,
    SignatureResolutionError, SignatureResolutionInputs, TrustedAssociatedBinding,
    TrustedAssociatedType, TrustedCallableParameter, TrustedConstructorPayload,
    TrustedDeclarationInputs, TrustedDeclarationSignature, TrustedGenericParameter,
    TrustedImplMethod, TrustedInhabitanceEnvironment, TrustedInhabitanceFact,
    TrustedInhabitanceProduct, TrustedLanguageItemEnvironment, TrustedNominalInhabitance,
    TrustedSignatureEnvironment, TrustedSignatureError, TrustedSignatureKind, TrustedTraitAdapter,
    TrustedTraitExpr, TrustedTraitMethod, TrustedTypeExpr, TypeKind, TypecheckCapability,
    collect_declarations, plan_imports, resolve_declaration_signatures, resolve_imports,
};
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

struct PreparedSource {
    file: FileId,
    module: ModuleAddress,
    hir: HirFile,
    spans: Vec<(eglur_hir::OriginId, SourceSpan)>,
}

struct CapabilitySet(BTreeSet<CompileCapability>);

impl CapabilitySet {
    fn signatures() -> Self {
        Self(
            [
                CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
                CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
                CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
                CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
            ]
            .into_iter()
            .collect(),
        )
    }

    fn without_signatures() -> Self {
        Self(
            [
                CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
                CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
                CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
            ]
            .into_iter()
            .collect(),
        )
    }
}

impl CompileSupport for CapabilitySet {
    fn supports(&self, capability: CompileCapability) -> bool {
        self.0.contains(&capability)
    }
}

struct Analysis {
    declarations: DeclarationCollection,
    signatures: Option<eglur_sema::DeclarationSignatures>,
    inhabitance: Option<InhabitanceFacts>,
    diagnostics: DiagnosticSnapshot,
    demands: Vec<CapabilityDemand>,
}

fn prepare(path: &str, text: &str) -> PreparedSource {
    let file = FileId::new(0);
    let source_file = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = source_file.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parse = parse_validated(validated);
    assert!(
        parse.diagnostics().is_empty(),
        "test source did not parse: {:?}",
        parse.diagnostics()
    );
    let hir = lower(&parse);
    assert!(
        hir.invalid_occurrences().is_empty(),
        "test source lowered invalid HIR"
    );
    let label = SourceLabel::new(path).unwrap();
    let spans = hir
        .origins()
        .map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        })
        .collect();
    PreparedSource {
        file,
        module: ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        hir,
        spans,
    }
}

fn analyze(
    text: &str,
    support: &CapabilitySet,
    external: Option<&ExternalNameEnvironment>,
    trusted: Option<&TrustedSignatureEnvironment>,
) -> Analysis {
    try_analyze(text, support, external, trusted).unwrap()
}

fn try_analyze(
    text: &str,
    support: &CapabilitySet,
    external: Option<&ExternalNameEnvironment>,
    trusted: Option<&TrustedSignatureEnvironment>,
) -> Result<Analysis, SignatureResolutionError> {
    try_analyze_with_inhabitance(text, support, external, trusted, None, None)
}

fn try_analyze_with_inhabitance(
    text: &str,
    support: &CapabilitySet,
    external: Option<&ExternalNameEnvironment>,
    trusted: Option<&TrustedSignatureEnvironment>,
    trusted_inhabitance: Option<&TrustedInhabitanceEnvironment>,
    trusted_language_items: Option<&TrustedLanguageItemEnvironment>,
) -> Result<Analysis, SignatureResolutionError> {
    let prepared = prepare("main.eglur", text);
    let sources = [SemanticSource::new(
        prepared.file,
        prepared.module.clone(),
        Some(&prepared.hir),
        prepared.spans.clone(),
        [],
        None,
    )
    .unwrap()];
    let PhaseProduct::Performed(declarations) = collect_declarations(&sources, support)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declaration collection must be supported")
    };
    let mut diagnostics = DiagnosticStore::new();
    let PhaseProduct::Performed(plan) =
        plan_imports(&declarations, &sources, support, external, &mut diagnostics)
            .unwrap()
            .into_parts()
            .0
    else {
        panic!("import planning returns a partial product")
    };
    let unit = ImportAnalysisUnit::new([prepared.file]).unwrap();
    let PhaseProduct::Performed(imports) = resolve_imports(
        &declarations,
        &plan,
        &[unit],
        &sources,
        support,
        external,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import resolution returns a partial product")
    };
    let inputs = SignatureResolutionInputs::new(
        external,
        trusted.map(|signatures| {
            let inputs = TrustedDeclarationInputs::new(signatures, trusted_inhabitance);
            trusted_language_items.map_or(inputs, |items| inputs.with_language_items(items))
        }),
    );
    let (product, demands) = resolve_declaration_signatures(
        &declarations,
        &imports,
        &sources,
        support,
        inputs,
        &mut diagnostics,
    )?
    .into_parts();
    let (signatures, inhabitance) = match product {
        PhaseProduct::Performed(resolved) => {
            let (signatures, inhabitance) = resolved.into_parts();
            (Some(signatures), Some(inhabitance))
        }
        PhaseProduct::Unperformed => (None, None),
    };
    Ok(Analysis {
        declarations,
        signatures,
        inhabitance,
        diagnostics: diagnostics.finalize(None),
        demands: demands.into_vec(),
    })
}

fn empty_external() -> ExternalNameEnvironment {
    ExternalNameEnvironment::new([], [], []).unwrap()
}

fn source_def(
    declarations: &DeclarationCollection,
    name: &str,
    kind: DeclarationKind,
) -> eglur_sema::DefId {
    declarations
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| {
            declaration.kind() == kind
                && declaration
                    .name()
                    .is_some_and(|candidate| candidate.text() == name)
        })
        .unwrap()
        .id()
}

fn diagnostic_codes(snapshot: &DiagnosticSnapshot) -> Vec<&str> {
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str())
        .collect()
}

#[test]
fn type_lookup_diagnostics_select_the_name_path_without_its_arguments() {
    for (prefix, spelling, name) in [
        ("", "Missing[i64]", "Missing"),
        ("", "pkg::Missing[i64]", "pkg::Missing"),
        (
            "",
            "pkg /* path */ :: Missing[i64]",
            "pkg /* path */ :: Missing",
        ),
        ("struct Parcel[T] {}\n", "Parcel[Missing[i64]]", "Missing"),
    ] {
        let text = format!("{prefix}type Target = {spelling}\n");
        let external = empty_external();
        let analysis = analyze(&text, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), ["NAME_NOT_FOUND"]);
        let range = analysis.diagnostics.diagnostics()[0]
            .primary_span()
            .as_source()
            .unwrap()
            .file_span()
            .range();
        assert_eq!(
            &text[range.start().as_usize()..range.end().as_usize()],
            name
        );
    }
}

#[test]
fn applied_type_diagnostics_keep_the_complete_type_occurrence() {
    let arity = "TYPE_ARG_ARITY_MISMATCH";
    let cases = [
        ("type Target = Never[i64]\n", "Never[i64]", arity),
        ("type Target = i64[i64]\n", "i64[i64]", arity),
        ("type Target = Opt[i64, i64]\n", "Opt[i64, i64]", arity),
        (
            "struct Parcel[T] {}\ntype Target = Parcel[i64, i64]\n",
            "Parcel[i64, i64]",
            arity,
        ),
        (
            "type Id[T] = T\ntype Target = Id[i64, i64]\n",
            "Id[i64, i64]",
            arity,
        ),
        (
            "trait Read[T] {}\ntrait Target[X: Read[i64, i64]] {}\n",
            "Read[i64, i64]",
            arity,
        ),
        ("fn value[T]() -> T[i64] = 0\n", "T[i64]", arity),
        (
            "struct S { fn value() -> Self[i64] = 0\n}\n",
            "Self[i64]",
            arity,
        ),
        (
            "trait Read { type Item\n}\nfn value[T: Read](x: T::Item[i64]) -> Unit {}\n",
            "T::Item[i64]",
            arity,
        ),
        (
            "type Target = Union[i64]\n",
            "Union[i64]",
            "UNION_TYPE_ARITY_MISMATCH",
        ),
        (
            "type Target = Union[i64, Bool]\n",
            "Union[i64, Bool]",
            "UNION_TYPE_UNSUPPORTED",
        ),
        (
            "trait Read[T] {}\ntype Target = Read[i64]\n",
            "Read[i64]",
            "DYN_MUST_BE_EXPLICIT",
        ),
        (
            "fn value[T](x: T::Item[i64]) -> Unit {}\n",
            "T::Item[i64]",
            "ASSOCIATED_TYPE_PROJECTION_INVALID",
        ),
    ];
    for (text, spelling, code) in cases {
        let prepared = prepare("main.eglur", text);
        let expected = prepared
            .hir
            .type_refs()
            .find_map(|(_, ty)| {
                let span = prepared.hir.origin(ty.origin()).source();
                let range = span.range();
                (text[range.start().as_usize()..range.end().as_usize()].trim() == spelling)
                    .then_some(span)
            })
            .expect("the source contains the tested type occurrence");

        let external = empty_external();
        let analysis = analyze(text, &CapabilitySet::signatures(), Some(&external), None);
        assert_eq!(diagnostic_codes(&analysis.diagnostics), [code], "{text}");
        assert_eq!(
            analysis.diagnostics.diagnostics()[0]
                .primary_span()
                .as_source()
                .unwrap()
                .file_span(),
            expected,
            "{text}"
        );
    }
}

#[test]
fn source_signatures_expand_aliases_and_preserve_symbolic_projection_identity() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait Readable {
  type Elem
  fn read(self: &Self) -> Self::Elem
}

struct Source {
  field value: i64
}

impl Readable for Source {
  type Elem = i64
  fn read(self: &Self) -> Self::Elem = self.value
}

type Maybe[T] = Opt[T]

fn first[C: Readable](value: &C) -> C::Elem = value.read()
fn same(value: Maybe[i64]) -> Opt[i64] = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));
    let signatures = analysis.signatures.unwrap();

    let first = source_def(&analysis.declarations, "first", DeclarationKind::Function);
    let FactState::Known(DeclarationSignature::Callable(first_signature)) =
        signatures.source_declaration(first)
    else {
        panic!("generic function signature must resolve")
    };
    let first_signature = signatures.callable(*first_signature).unwrap();
    let TypeKind::AssociatedProjection(projection) =
        signatures.types().kind(first_signature.result()).unwrap()
    else {
        panic!("generic result must remain a symbolic projection")
    };
    let readable = source_def(&analysis.declarations, "Readable", DeclarationKind::Trait);
    assert_eq!(
        projection.trait_ref().declaration(),
        &CanonicalTarget::Source(readable)
    );
    assert!(matches!(
        signatures.types().kind(projection.base()).unwrap(),
        TypeKind::GenericParameter(_)
    ));

    let same = source_def(&analysis.declarations, "same", DeclarationKind::Function);
    let FactState::Known(DeclarationSignature::Callable(same_signature)) =
        signatures.source_declaration(same)
    else {
        panic!("alias-consuming function signature must resolve")
    };
    let same_signature = signatures.callable(*same_signature).unwrap();
    assert_eq!(same_signature.parameters()[0].ty(), same_signature.result());
    assert!(matches!(
        signatures.types().kind(same_signature.result()).unwrap(),
        TypeKind::Nominal {
            head: NominalHead::Intrinsic(IntrinsicNominal::Opt),
            ..
        }
    ));

    assert!(signatures.occurrence_fact_count() >= 12);
}

#[test]
fn source_and_trusted_never_types_share_one_canonical_identity() {
    let (external, trusted) = trusted_callable_environment(
        DeclarationKind::Function,
        Box::new([TrustedCallableParameter {
            name: Some("value".into()),
            ty: TrustedTypeExpr::Never,
        }]),
        TrustedTypeExpr::Never,
    );
    let analysis = analyze(
        r#"
type Bottom = Never
fn source(value: Never) -> Bottom = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    );
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));
    let signatures = analysis.signatures.unwrap();
    let never = signatures.types().never();
    assert!(matches!(
        signatures.types().kind(never),
        Ok(TypeKind::Never)
    ));

    let alias = source_def(&analysis.declarations, "Bottom", DeclarationKind::TypeAlias);
    assert!(matches!(
        signatures.source_declaration(alias),
        FactState::Known(DeclarationSignature::TypeAlias { value, .. }) if *value == never
    ));

    let source = source_def(&analysis.declarations, "source", DeclarationKind::Function);
    let FactState::Known(DeclarationSignature::Callable(source_signature)) =
        signatures.source_declaration(source)
    else {
        panic!("source Never signature must resolve")
    };
    let source_signature = signatures.callable(*source_signature).unwrap();
    assert_eq!(source_signature.parameters()[0].ty(), never);
    assert_eq!(source_signature.result(), never);

    let trusted = CanonicalTarget::External(
        ExternalDefId::new("std::ffi::callable").expect("trusted helper uses a stable identity"),
    );
    let FactState::Known(DeclarationSignature::Callable(trusted_signature)) =
        signatures.declaration(&trusted)
    else {
        panic!("trusted Never signature must resolve")
    };
    let trusted_signature = signatures.callable(*trusted_signature).unwrap();
    assert_eq!(trusted_signature.parameters()[0].ty(), never);
    assert_eq!(trusted_signature.result(), never);
}

#[test]
fn trusted_language_items_require_exact_constructor_semantics() {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("opt")]).unwrap();
    let some = ExternalDefId::new("std::opt::Some").unwrap();
    let declaration = ExternalDeclaration::new(
        some.clone(),
        module.clone(),
        NameKey::new("Some", false).unwrap(),
        DeclarationKind::IntrinsicConstructor,
    )
    .unwrap();
    let external = ExternalNameEnvironment::new([module], [declaration], [some.clone()]).unwrap();
    let generic = |name: &str| TrustedGenericParameter {
        name: name.into(),
        bounds: Box::new([]),
    };
    let trusted = TrustedSignatureEnvironment::new([TrustedDeclarationSignature::new(
        some.clone(),
        TrustedSignatureKind::Constructor {
            generic_parameters: Box::new([generic("T"), generic("Wrong")]),
            payload: TrustedConstructorPayload::Positional(Box::new([
                TrustedTypeExpr::GenericParameter(1),
            ])),
            result: TrustedTypeExpr::Intrinsic {
                head: IntrinsicNominal::Opt,
                arguments: Box::new([TrustedTypeExpr::GenericParameter(0)]),
            },
        },
    )])
    .unwrap();
    let language_items =
        TrustedLanguageItemEnvironment::new([(LanguageItem::OptSome, some.clone())]).unwrap();

    let error = try_analyze_with_inhabitance(
        "fn local() -> i64 = 0\n",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
        None,
        Some(&language_items),
    )
    .err()
    .expect("malformed language-item constructor must be rejected");
    assert_eq!(
        error,
        SignatureResolutionError::InvalidLanguageItemSignature {
            item: LanguageItem::OptSome,
            declaration: some,
        }
    );
}

#[test]
fn trusted_product_and_sum_inhabitance_rules_bind_to_canonical_types() {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("shape")]).unwrap();
    let wrapper = ExternalDefId::new("std::shape::Wrapper").unwrap();
    let choice = ExternalDefId::new("std::shape::Choice").unwrap();
    let empty_seq = ExternalDefId::new("std::shape::EmptySeq").unwrap();
    let declarations = [
        ExternalDeclaration::new(
            wrapper.clone(),
            module.clone(),
            NameKey::new("Wrapper", false).unwrap(),
            DeclarationKind::Struct,
        )
        .unwrap(),
        ExternalDeclaration::new(
            choice.clone(),
            module.clone(),
            NameKey::new("Choice", false).unwrap(),
            DeclarationKind::Enum,
        )
        .unwrap(),
        ExternalDeclaration::new(
            empty_seq.clone(),
            module.clone(),
            NameKey::new("EmptySeq", false).unwrap(),
            DeclarationKind::Struct,
        )
        .unwrap(),
    ];
    let external = ExternalNameEnvironment::new(
        [module],
        declarations,
        [wrapper.clone(), choice.clone(), empty_seq.clone()],
    )
    .unwrap();
    let generic = |name: &str| TrustedGenericParameter {
        name: name.into(),
        bounds: Box::new([]),
    };
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            wrapper.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([generic("T")]),
            },
        ),
        TrustedDeclarationSignature::new(
            choice.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([generic("T"), generic("E")]),
            },
        ),
        TrustedDeclarationSignature::new(
            empty_seq.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([generic("T")]),
            },
        ),
    ])
    .unwrap();
    let trusted_inhabitance = TrustedInhabitanceEnvironment::new([
        TrustedInhabitanceFact::new(
            wrapper,
            TrustedNominalInhabitance::Product(Box::new([TrustedTypeExpr::GenericParameter(0)])),
        ),
        TrustedInhabitanceFact::new(
            choice,
            TrustedNominalInhabitance::Sum(Box::new([
                TrustedInhabitanceProduct::new([TrustedTypeExpr::GenericParameter(0)]),
                TrustedInhabitanceProduct::new([TrustedTypeExpr::GenericParameter(1)]),
            ])),
        ),
        TrustedInhabitanceFact::new(empty_seq, TrustedNominalInhabitance::Inhabited),
    ])
    .unwrap();
    let mut analysis = try_analyze_with_inhabitance(
        concat!(
            "struct RecursiveContainer { field values: EmptySeq[RecursiveContainer] }\n",
            "fn wrapped(value: Wrapper[Never]) -> Unit = ()\n",
            "fn empty_choice(value: Choice[Never, Never]) -> Unit = ()\n",
            "fn inhabited_choice(value: Choice[Never, i64]) -> Unit = ()\n",
            "fn recursive_container(value: RecursiveContainer) -> Unit = ()\n",
        ),
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
        Some(&trusted_inhabitance),
        None,
    )
    .unwrap();
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));

    let cases = [
        ("wrapped", Inhabitance::Uninhabited),
        ("empty_choice", Inhabitance::Uninhabited),
        ("inhabited_choice", Inhabitance::Inhabited),
        ("recursive_container", Inhabitance::Inhabited),
    ];
    let mut signatures = analysis.signatures.take().unwrap();
    let mut inhabitance = analysis.inhabitance.take().unwrap();
    for (name, expected) in cases {
        let declaration = source_def(&analysis.declarations, name, DeclarationKind::Function);
        let FactState::Known(DeclarationSignature::Callable(callable)) =
            signatures.source_declaration(declaration)
        else {
            panic!("test function signature must resolve")
        };
        let parameter = signatures.callable(*callable).unwrap().parameters()[0].ty();
        assert_eq!(
            inhabitance.classify(&mut signatures, parameter).unwrap(),
            expected
        );
    }
}

#[test]
fn never_source_spelling_enforces_zero_arity_and_reserved_bindings() {
    let external = empty_external();
    let malformed = analyze(
        "fn invalid(value: Never[i64]) -> Unit = ()\n",
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&malformed.diagnostics),
        ["TYPE_ARG_ARITY_MISMATCH"]
    );
    let diagnostic = &malformed.diagnostics.diagnostics()[0];
    assert_eq!(diagnostic.data()["expected_count"].to_string(), "0");
    assert_eq!(diagnostic.data()["actual_count"].to_string(), "1");

    let reserved = analyze(
        r#"
type Never = i64
fn invalid[Never](value: i64) -> i64 = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&reserved.diagnostics),
        ["RESERVED_BUILTIN_NAME", "RESERVED_BUILTIN_NAME"]
    );
}

#[test]
fn declaration_signatures_preserve_struct_and_variant_member_kinds() {
    let external = empty_external();
    let analysis = analyze(
        r#"
struct Record {
  field raw: i64
  prop count: i64 {
    fn get_count(self: &Self) -> i64 = 0
  }
}

enum Token {
  End
  Pair(i64, Str)
  Number { value: i64 }
}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));
    let signatures = analysis.signatures.unwrap();

    let record = source_def(&analysis.declarations, "Record", DeclarationKind::Struct);
    let FactState::Known(DeclarationSignature::Struct {
        storage_fields,
        properties,
        ..
    }) = signatures.source_declaration(record)
    else {
        panic!("struct signature must preserve member partitions")
    };
    assert_eq!(storage_fields.len(), 1);
    assert_eq!(properties.len(), 1);

    let end = source_def(&analysis.declarations, "End", DeclarationKind::EnumVariant);
    assert!(matches!(
        signatures.source_declaration(end),
        FactState::Known(DeclarationSignature::EnumVariant {
            payload: EnumVariantPayloadSignature::Absent
        })
    ));
    let pair = source_def(&analysis.declarations, "Pair", DeclarationKind::EnumVariant);
    assert!(matches!(
        signatures.source_declaration(pair),
        FactState::Known(DeclarationSignature::EnumVariant {
            payload: EnumVariantPayloadSignature::Tuple(elements)
        }) if elements.len() == 2
    ));
    let number = source_def(
        &analysis.declarations,
        "Number",
        DeclarationKind::EnumVariant,
    );
    assert!(matches!(
        signatures.source_declaration(number),
        FactState::Known(DeclarationSignature::EnumVariant {
            payload: EnumVariantPayloadSignature::Named(fields)
        }) if fields.len() == 1
    ));
}

#[test]
fn malformed_source_signatures_emit_contract_data_and_alias_cycle_poison() {
    let external = empty_external();
    let analysis = analyze(
        r#"
type A = B
type B = A

struct Bad {
  field value: Opt[i64, Str]
}

fn missing(value: i64) = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    let codes = diagnostic_codes(&analysis.diagnostics);
    assert!(codes.contains(&"TYPE_ALIAS_CYCLE"));
    assert!(codes.contains(&"TYPE_ARG_ARITY_MISMATCH"));
    assert!(codes.contains(&"FN_RETURN_TYPE_REQUIRED"));
    let arity = analysis
        .diagnostics
        .diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code().as_str() == "TYPE_ARG_ARITY_MISMATCH")
        .unwrap();
    assert_eq!(arity.data()["expected_count"].to_string(), "1");
    assert_eq!(arity.data()["actual_count"].to_string(), "2");

    let signatures = analysis.signatures.unwrap();
    for alias in ["A", "B"] {
        let alias = source_def(&analysis.declarations, alias, DeclarationKind::TypeAlias);
        assert!(matches!(
            signatures.source_declaration(alias),
            FactState::Poisoned(_)
        ));
    }
}

#[test]
fn associated_binding_cycles_poison_the_impl_and_cycle_dependents() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait Linked {
  type Head
  type Tail
  type View
}

struct Node {}
impl Linked for Node {
  type View = Opt[Self::Head]
  type Head = Self::Tail
  type Tail = Self::Head
}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics)
            .into_iter()
            .filter(|code| *code == "ASSOCIATED_TYPE_CYCLE")
            .count(),
        1
    );
    let signatures = analysis.signatures.unwrap();
    let implementation = analysis
        .declarations
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.kind() == DeclarationKind::Impl)
        .unwrap()
        .id();
    assert!(matches!(
        signatures.source_declaration(implementation),
        FactState::Poisoned(_)
    ));
    for name in ["View", "Head", "Tail"] {
        let binding = source_def(
            &analysis.declarations,
            name,
            DeclarationKind::AssociatedTypeBinding,
        );
        assert!(matches!(
            signatures.source_declaration(binding),
            FactState::Poisoned(_)
        ));
    }
}

#[test]
fn impl_header_conflict_precedes_associated_binding_validation() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait HasItem { type Item }
struct Value {}
impl HasItem for Value { type Item = i64 }
impl HasItem for Value {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );

    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["TRAIT_IMPL_DUPLICATE"]
    );
    assert_eq!(analysis.diagnostics.diagnostics()[0].related().len(), 1);

    let implementation = analysis
        .declarations
        .modules()
        .flat_map(|module| module.declarations())
        .filter(|declaration| declaration.kind() == DeclarationKind::Impl)
        .nth(1)
        .unwrap()
        .id();
    let signatures = analysis.signatures.unwrap();
    assert!(matches!(
        signatures.source_declaration(implementation),
        FactState::Poisoned(_)
    ));
    assert!(matches!(
        signatures.impl_header(&CanonicalTarget::Source(implementation)),
        FactState::Poisoned(_)
    ));
}

#[test]
fn impl_header_conflict_precedes_unconstrained_parameter_validation() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait Marker {}
struct Value {}
impl Marker for Value {}
impl[T] Marker for Value {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );

    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["TRAIT_IMPL_DUPLICATE"]
    );
}

#[test]
fn impl_header_unification_distinguishes_overlap_from_specialization() {
    let external = empty_external();
    let specialization = analyze(
        r#"
trait Marker {}
struct Wrap[T] {}
impl[T] Marker for Wrap[T] {}
impl Marker for Wrap[i64] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&specialization.diagnostics),
        ["SPECIALIZATION_FORBIDDEN"]
    );

    let overlap = analyze(
        r#"
trait Rel[T] {}
struct Pair[A, B] {}
impl[A, B] Rel[A] for Pair[A, B] {}
impl[A, B] Rel[B] for Pair[A, B] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&overlap.diagnostics),
        ["TRAIT_IMPL_OVERLAP"]
    );
}

#[test]
fn generic_error_union_impl_headers_are_rejected_before_coherence() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait Marker {}
struct FirstError {}
struct SecondError {}
impl[T] Marker for Res[Unit, Union[T, FirstError]] {}
impl[U] Marker for Res[Unit, Union[U, SecondError]] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );

    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        [
            "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
            "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
        ]
    );
    assert_eq!(
        analysis.diagnostics.diagnostics()[0].data()["parameter"].to_string(),
        "T"
    );
    assert_eq!(
        analysis.diagnostics.diagnostics()[1].data()["parameter"].to_string(),
        "U"
    );
}

#[test]
fn generic_error_union_impl_restriction_follows_the_normalized_header_graph() {
    let external = empty_external();
    let rejected = analyze(
        r#"
trait Direct {}
trait Nested[E] {}
trait Aliased {}
trait TraitSide[T] {}
struct Wrapper[E] {}
struct Carrier {}
struct ParseError {}
type Outcome[E] = Res[Unit, Union[E, ParseError]]
impl[E] Direct for Res[Unit, Union[E, ParseError]] {}
impl[E] Nested[E] for Res[Unit, Union[Wrapper[E], ParseError]] {}
impl[E] Aliased for Outcome[E] {}
impl[E] TraitSide[Res[Unit, Union[E, ParseError]]] for Carrier {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&rejected.diagnostics),
        [
            "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
            "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
            "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
            "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
        ]
    );

    let accepted = analyze(
        r#"
trait Whole {}
trait Collapsed {}
impl[E] Whole for Res[Unit, E] {}
impl[E] Collapsed for Res[Unit, Union[E, Never]] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(
        accepted.diagnostics.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&accepted.diagnostics)
    );
}

#[test]
fn rigid_error_union_impl_headers_use_ordinary_coherence_relations() {
    let external = empty_external();
    let duplicate = analyze(
        r#"
trait Marker {}
struct IoError {}
struct ParseError {}
impl Marker for Res[Unit, Union[IoError, ParseError]] {}
impl Marker for Res[Unit, Union[Never, ParseError, IoError, IoError]] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&duplicate.diagnostics),
        ["TRAIT_IMPL_DUPLICATE"]
    );

    let disjoint = analyze(
        r#"
trait Marker {}
struct IoError {}
struct ParseError {}
struct DomainError {}
impl Marker for Res[Unit, Union[IoError, ParseError]] {}
impl Marker for Res[Unit, Union[IoError, DomainError]] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(
        disjoint.diagnostics.diagnostics().is_empty(),
        "unexpected diagnostics: {:?}",
        diagnostic_codes(&disjoint.diagnostics)
    );

    let specialization = analyze(
        r#"
trait Marker {}
struct IoError {}
struct ParseError {}
impl[E] Marker for Res[Unit, E] {}
impl Marker for Res[Unit, Union[IoError, ParseError]] {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&specialization.diagnostics),
        ["SPECIALIZATION_FORBIDDEN"]
    );
}

#[test]
fn declaration_constraints_are_checked_without_body_typing() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait Stateful {
  field value: i64
  prop count: i64 {
    fn get_count(self: &Self) -> i64 = 0
  }
}

struct Legacy {
  var old: i64
  field current: i64 = 0
}

trait Marker {}
struct Marked {}
impl !Marker for Marked {}

fn identity[Own](value: Own) -> Own = value
extern "C" fn bad_tuple(value: (i64, i64)) -> Unit
extern "C" fn bad_unit(value: Unit) -> Unit
pub extern "C" fn public_extern(value: i64) -> i64
extern "system" fn wrong_abi(value: i64) -> i64
extern "C" fn good_pointer(value: *const u8, flag: Bool) -> Unit
extern "\u{43}" fn escaped_abi(value: i64) -> i64
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    let codes = diagnostic_codes(&analysis.diagnostics);
    for expected in [
        "TRAIT_STATE_FORBIDDEN",
        "VAR_DECL_FORBIDDEN",
        "STRUCT_FIELD_DEFAULT_FORBIDDEN",
        "RESERVED_BUILTIN_NAME",
        "NEGATIVE_IMPL_UNSUPPORTED",
        "EXTERN_SIGNATURE_UNSUPPORTED",
        "EXTERN_VISIBILITY_FORBIDDEN",
        "EXTERN_ABI_UNSUPPORTED",
    ] {
        assert!(codes.contains(&expected), "missing diagnostic {expected}");
    }

    let signatures = analysis.signatures.unwrap();
    let identity = source_def(
        &analysis.declarations,
        "identity",
        DeclarationKind::Function,
    );
    assert!(matches!(
        signatures.source_declaration(identity),
        FactState::Poisoned(_)
    ));
    for name in ["good_pointer", "escaped_abi"] {
        let function = source_def(
            &analysis.declarations,
            name,
            DeclarationKind::ExternFunction,
        );
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Known(DeclarationSignature::Callable(_))
        ));
    }
}

#[test]
fn callable_effect_signatures_are_validated_before_body_typing() {
    let external = empty_external();
    let analysis = analyze(
        r#"
struct Counter {
  field mut value: i64
  fn bad_shared!(self: &Self) -> Unit {}
  fn bad_mut(self: &mut Self) -> i64 = 0
  fn good_owned!(self: Self) -> Unit {}
}

fn bad_arg(value: &mut Counter) -> i64 = 0
fn good_arg!(value: &mut Counter) -> Unit {}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    let codes = diagnostic_codes(&analysis.diagnostics);
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == "EFFECT_SIGNATURE")
            .count(),
        2
    );
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == "NON_BANG_MUT_PARAM_FORBIDDEN")
            .count(),
        1
    );
    let signatures = analysis.signatures.unwrap();
    for name in ["bad_shared", "bad_mut", "bad_arg"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Poisoned(_)
        ));
    }
    for name in ["good_owned", "good_arg"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Known(DeclarationSignature::Callable(_))
        ));
    }
}

#[test]
fn callable_parameter_patterns_require_identifier_bindings() {
    let external = empty_external();
    let analysis = analyze(
        r#"
fn tuple((left, right): (i64, i64)) -> i64 = left
fn unit((): Unit) -> Unit = ()
fn binding(value: i64) -> i64 = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics)
            .iter()
            .filter(|code| **code == "PARAM_PATTERN_UNSUPPORTED")
            .count(),
        2
    );
    let signatures = analysis.signatures.unwrap();
    for name in ["tuple", "unit"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Poisoned(_)
        ));
    }
    let binding = source_def(&analysis.declarations, "binding", DeclarationKind::Function);
    assert!(matches!(
        signatures.source_declaration(binding),
        FactState::Known(DeclarationSignature::Callable(_))
    ));
}

#[test]
fn alias_and_union_positions_cannot_bypass_source_type_rules() {
    let external = empty_external();
    let analysis = analyze(
        r#"
type Pointer = *const i64
type Errors = Union[Str, i64]
fn accepted(value: Res[i64, Union[Str, Str]]) -> Res[i64, Str] = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    let codes = diagnostic_codes(&analysis.diagnostics);
    assert!(codes.contains(&"RAW_POINTER_TYPE_UNSUPPORTED"));
    assert!(codes.contains(&"UNION_TYPE_UNSUPPORTED"));
    let accepted = source_def(
        &analysis.declarations,
        "accepted",
        DeclarationKind::Function,
    );
    assert!(matches!(
        analysis.signatures.unwrap().source_declaration(accepted),
        FactState::Known(DeclarationSignature::Callable(_))
    ));
}

#[test]
fn ownership_roots_are_limited_to_complete_callable_boundaries() {
    let external = empty_external();
    let analysis = analyze(
        r#"
fn direct(value: Own[i64]) -> Own[i64] = value
fn conditional(value: Opt[Own[i64]]) -> Res[Own[i64], Str] = Err("x")
fn borrowed(value: &Own[i64]) -> i64 = 0
fn bad_tuple(value: (Own[i64], i64)) -> i64 = 0
fn bad_nested(value: Opt[Opt[Own[i64]]]) -> i64 = 0
struct Holder {
  field value: Own[i64]
}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(
        diagnostic_codes(&analysis.diagnostics)
            .iter()
            .filter(|code| **code == "OWNERSHIP_TYPE_POSITION_FORBIDDEN")
            .count()
            >= 3
    );
    let signatures = analysis.signatures.unwrap();
    for name in ["direct", "conditional", "borrowed"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Known(DeclarationSignature::Callable(_))
        ));
    }
    for name in ["bad_tuple", "bad_nested"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Poisoned(_)
        ));
    }
    let field = source_def(
        &analysis.declarations,
        "value",
        DeclarationKind::StorageField,
    );
    assert!(matches!(
        signatures.source_declaration(field),
        FactState::Poisoned(_)
    ));
}

#[test]
fn missing_external_signatures_leave_dependent_source_facts_unperformed() {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("remote")]).unwrap();
    let remote = ExternalDefId::new("std::remote::Remote").unwrap();
    let alias = ExternalDefId::new("std::remote::RemoteAlias").unwrap();
    let remote_trait = ExternalDefId::new("std::remote::RemoteTrait").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            ExternalDeclaration::new(
                remote.clone(),
                module.clone(),
                NameKey::new("Remote", false).unwrap(),
                DeclarationKind::Struct,
            )
            .unwrap(),
            ExternalDeclaration::new(
                alias.clone(),
                module.clone(),
                NameKey::new("RemoteAlias", false).unwrap(),
                DeclarationKind::TypeAlias,
            )
            .unwrap(),
            ExternalDeclaration::new(
                remote_trait.clone(),
                module,
                NameKey::new("RemoteTrait", false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
        ],
        [remote, alias, remote_trait],
    )
    .unwrap();
    let analysis = analyze(
        r#"
fn nominal(value: Remote) -> Remote = value
fn aliased(value: RemoteAlias) -> RemoteAlias = value
fn bounded[T: RemoteTrait](value: T) -> T = value

struct Local {}
impl RemoteTrait for Local {
  fn inspect(self: &Self) -> i64 = 0
}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));
    let signatures = analysis.signatures.unwrap();
    for name in ["nominal", "aliased", "bounded", "inspect"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Unperformed
        ));
    }
    let parameter = source_def(
        &analysis.declarations,
        "T",
        DeclarationKind::GenericParameter,
    );
    assert!(matches!(
        signatures.generic_parameter(&GenericParameterId::Source(parameter)),
        FactState::Unperformed
    ));
}

#[test]
fn poisoned_owner_state_propagates_without_resolving_child_signatures() {
    let external = empty_external();
    let analysis = analyze(
        r#"
fn consume(value: self::Opt) -> i64 = 0

struct Opt {
  fn inspect(self: &Self) -> i64 {
    let value: i64 = 0
    yield value
  }
}
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["RESERVED_BUILTIN_NAME"]
    );
    let signatures = analysis.signatures.unwrap();
    let owner = source_def(&analysis.declarations, "Opt", DeclarationKind::Struct);
    let method = source_def(&analysis.declarations, "inspect", DeclarationKind::Function);
    let consumer = source_def(&analysis.declarations, "consume", DeclarationKind::Function);
    assert!(matches!(
        signatures.source_declaration(owner),
        FactState::Poisoned(_)
    ));
    assert!(matches!(
        signatures.source_declaration(method),
        FactState::Poisoned(_)
    ));
    assert!(matches!(
        signatures.source_declaration(consumer),
        FactState::Poisoned(_)
    ));
}

#[test]
fn failed_type_import_prevents_external_prelude_projection_fallback() {
    let prelude_module =
        ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("prelude")]).unwrap();
    let thing = ExternalDefId::new("std::prelude::Thing").unwrap();
    let declaration = ExternalDeclaration::new(
        thing.clone(),
        prelude_module.clone(),
        NameKey::new("Thing", false).unwrap(),
        DeclarationKind::Struct,
    )
    .unwrap();
    let external = ExternalNameEnvironment::new([prelude_module], [declaration], [thing]).unwrap();
    let mut support = CapabilitySet::signatures();
    support.0.insert(CompileCapability::Resolve(
        ResolveCapability::ResolveTrustedExternalNames,
    ));
    let analysis = analyze(
        "import missing::Thing\nfn use(value: Thing::Item) -> i64 = 0\n",
        &support,
        Some(&external),
        None,
    );

    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        ["IMPORT_NOT_FOUND"]
    );
    let signatures = analysis.signatures.unwrap();
    let function = source_def(&analysis.declarations, "use", DeclarationKind::Function);
    assert!(matches!(
        signatures.source_declaration(function),
        FactState::Poisoned(_)
    ));
}

#[test]
fn forward_trait_generics_resolve_independently_of_source_order() {
    let external = empty_external();
    let analysis = analyze(
        r#"
trait HasItem {
  type Item
}
trait Uses[T] {}
fn early[U: Uses[T::Item], T: HasItem](value: U) -> U = value
fn same[T: Uses[T::Item] + HasItem](value: T) -> T = value
fn mutual[U: Uses[T::Item] + HasItem, T: Uses[U::Item] + HasItem](value: U) -> T = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));
    let signatures = analysis.signatures.unwrap();
    for name in ["early", "same", "mutual"] {
        let function = source_def(&analysis.declarations, name, DeclarationKind::Function);
        assert!(matches!(
            signatures.source_declaration(function),
            FactState::Known(DeclarationSignature::Callable(_))
        ));
    }
}

#[test]
fn generic_parameter_redeclarations_poison_the_later_parameter_and_owner() {
    let external = empty_external();
    let analysis = analyze(
        r#"
struct Outer[T] {
  fn nested[T](self: &Self, value: T) -> T = value
}

fn duplicate[T, T](value: T) -> T = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert_eq!(
        diagnostic_codes(&analysis.diagnostics),
        [
            "GENERIC_PARAMETER_REDECLARATION",
            "GENERIC_PARAMETER_REDECLARATION"
        ]
    );
    for diagnostic in analysis.diagnostics.diagnostics() {
        assert_eq!(diagnostic.data()["name"].to_string(), "T");
    }

    let signatures = analysis.signatures.unwrap();
    let outer = source_def(&analysis.declarations, "Outer", DeclarationKind::Struct);
    let outer_parameters = analysis.declarations.child_candidates(
        outer,
        &NameKey::new("T", false).unwrap(),
        DeclarationKind::GenericParameter,
    );
    assert!(matches!(
        signatures.generic_parameter(&GenericParameterId::Source(outer_parameters[0])),
        FactState::Known(_)
    ));
    assert!(matches!(
        signatures.source_declaration(outer),
        FactState::Known(DeclarationSignature::Struct { .. })
    ));

    let nested = source_def(&analysis.declarations, "nested", DeclarationKind::Function);
    let nested_parameters = analysis.declarations.child_candidates(
        nested,
        &NameKey::new("T", false).unwrap(),
        DeclarationKind::GenericParameter,
    );
    assert!(matches!(
        signatures.generic_parameter(&GenericParameterId::Source(nested_parameters[0])),
        FactState::Poisoned(_)
    ));
    assert!(matches!(
        signatures.source_declaration(nested),
        FactState::Poisoned(_)
    ));

    let duplicate = source_def(
        &analysis.declarations,
        "duplicate",
        DeclarationKind::Function,
    );
    let duplicate_parameters = analysis.declarations.child_candidates(
        duplicate,
        &NameKey::new("T", false).unwrap(),
        DeclarationKind::GenericParameter,
    );
    assert_eq!(duplicate_parameters.len(), 2);
    assert!(matches!(
        signatures.generic_parameter(&GenericParameterId::Source(duplicate_parameters[0])),
        FactState::Known(_)
    ));
    assert!(matches!(
        signatures.generic_parameter(&GenericParameterId::Source(duplicate_parameters[1])),
        FactState::Poisoned(_)
    ));
    assert!(matches!(
        signatures.source_declaration(duplicate),
        FactState::Poisoned(_)
    ));
}

#[test]
fn generic_parameters_may_still_shadow_concrete_type_names() {
    let external = empty_external();
    let analysis = analyze(
        r#"
struct Value {}
fn identity[Value](value: Value) -> Value = value
"#,
        &CapabilitySet::signatures(),
        Some(&external),
        None,
    );
    assert!(matches!(
        analysis.diagnostics,
        DiagnosticSnapshot::Accepted(_)
    ));

    let signatures = analysis.signatures.unwrap();
    let identity = source_def(
        &analysis.declarations,
        "identity",
        DeclarationKind::Function,
    );
    let parameters = analysis.declarations.child_candidates(
        identity,
        &NameKey::new("Value", false).unwrap(),
        DeclarationKind::GenericParameter,
    );
    assert!(matches!(
        signatures.generic_parameter(&GenericParameterId::Source(parameters[0])),
        FactState::Known(_)
    ));
    assert!(matches!(
        signatures.source_declaration(identity),
        FactState::Known(DeclarationSignature::Callable(_))
    ));
}

fn trusted_callable_environment(
    kind: DeclarationKind,
    parameters: Box<[TrustedCallableParameter]>,
    result: TrustedTypeExpr,
) -> (ExternalNameEnvironment, TrustedSignatureEnvironment) {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("ffi")]).unwrap();
    let callable = ExternalDefId::new("std::ffi::callable").unwrap();
    let declaration = ExternalDeclaration::new(
        callable.clone(),
        module.clone(),
        NameKey::new("callable", false).unwrap(),
        kind,
    )
    .unwrap();
    let names = ExternalNameEnvironment::new([module], [declaration], [callable.clone()]).unwrap();
    let signatures = TrustedSignatureEnvironment::new([TrustedDeclarationSignature::new(
        callable,
        TrustedSignatureKind::Callable {
            bang: false,
            generic_parameters: Box::new([]),
            parameters,
            result,
            requirements: Box::new([]),
        },
    )])
    .unwrap();
    (names, signatures)
}

#[test]
fn trusted_method_contracts_are_rejected_at_portable_catalog_ingress() {
    fn id(value: &str) -> ExternalDefId {
        ExternalDefId::new(value).unwrap()
    }

    fn callable(id: ExternalDefId, parameter: TrustedTypeExpr) -> TrustedDeclarationSignature {
        TrustedDeclarationSignature::new(
            id,
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([]),
                parameters: Box::new([TrustedCallableParameter {
                    name: Some("self".into()),
                    ty: parameter,
                }]),
                result: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
                requirements: Box::new([]),
            },
        )
    }

    fn reference(ty: TrustedTypeExpr) -> TrustedTypeExpr {
        TrustedTypeExpr::Reference {
            mutability: ReferenceMutability::Shared,
            referent: Box::new(ty),
        }
    }

    let trait_id = id("test::Display");
    let method_id = id("test::Display::to_str");
    let implementation_id = id("test::impl::Display::Bool");
    let callable_id = id("test::impl::Display::Bool::to_str");
    let unknown_trait = id("test::MissingTrait");
    assert_eq!(
        TrustedSignatureEnvironment::new([TrustedDeclarationSignature::new(
            implementation_id.clone(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: unknown_trait.clone(),
                    arguments: Box::new([]),
                },
                target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool),
                associated_bindings: Box::new([]),
            },
        )])
        .err(),
        Some(TrustedSignatureError::UnknownTypeHead(unknown_trait))
    );
    let trait_declaration = TrustedDeclarationSignature::new(
        trait_id.clone(),
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([]),
            associated_types: Box::new([]),
        },
    )
    .with_trait_methods([TrustedTraitMethod {
        declaration: method_id.clone(),
        name: NameKey::new("to_str", false).unwrap(),
        has_default: false,
    }]);
    let method = callable(
        method_id.clone(),
        reference(TrustedTypeExpr::SelfType(trait_id.clone())),
    );
    let bang_method = TrustedDeclarationSignature::new(
        method_id.clone(),
        TrustedSignatureKind::Callable {
            bang: true,
            generic_parameters: Box::new([]),
            parameters: Box::new([TrustedCallableParameter {
                name: Some("self".into()),
                ty: reference(TrustedTypeExpr::SelfType(trait_id.clone())),
            }]),
            result: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
            requirements: Box::new([]),
        },
    );
    assert!(matches!(
        TrustedSignatureEnvironment::new([trait_declaration.clone(), bang_method]).err(),
        Some(TrustedSignatureError::MethodBangMismatch { .. })
    ));

    let default_trait = TrustedDeclarationSignature::new(
        trait_id.clone(),
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([]),
            associated_types: Box::new([]),
        },
    )
    .with_trait_methods([TrustedTraitMethod {
        declaration: method_id.clone(),
        name: NameKey::new("to_str", false).unwrap(),
        has_default: true,
    }]);
    assert!(
        TrustedSignatureEnvironment::new([
            default_trait,
            method.clone(),
            TrustedDeclarationSignature::new(
                implementation_id.clone(),
                TrustedSignatureKind::Impl {
                    negative: false,
                    generic_parameters: Box::new([]),
                    trait_ref: TrustedTraitExpr {
                        declaration: trait_id.clone(),
                        arguments: Box::new([]),
                    },
                    target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool),
                    associated_bindings: Box::new([]),
                },
            ),
        ])
        .is_ok()
    );
    let missing = TrustedSignatureEnvironment::new([
        trait_declaration.clone(),
        method.clone(),
        TrustedDeclarationSignature::new(
            implementation_id.clone(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: trait_id.clone(),
                    arguments: Box::new([]),
                },
                target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool),
                associated_bindings: Box::new([]),
            },
        ),
    ])
    .err()
    .expect("missing required method must be rejected");
    assert!(matches!(
        missing,
        TrustedSignatureError::MissingRequiredMethod { .. }
    ));

    let mismatched = TrustedSignatureEnvironment::new([
        trait_declaration.clone(),
        method.clone(),
        TrustedDeclarationSignature::new(
            implementation_id.clone(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: trait_id.clone(),
                    arguments: Box::new([]),
                },
                target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool),
                associated_bindings: Box::new([]),
            },
        )
        .with_impl_methods([TrustedImplMethod {
            trait_method: method_id.clone(),
            callable: callable_id.clone(),
        }]),
        callable(
            callable_id,
            reference(TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64)),
        ),
    ])
    .err()
    .expect("mismatched trusted method must be rejected");
    assert!(matches!(
        mismatched,
        TrustedSignatureError::MethodContractMismatch { .. }
    ));

    let adapter_id = id("test::to_str");
    let invalid_adapter = TrustedSignatureEnvironment::new([
        trait_declaration,
        method,
        TrustedDeclarationSignature::new(
            adapter_id.clone(),
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "T".into(),
                    bounds: Box::new([]),
                }]),
                parameters: Box::new([TrustedCallableParameter {
                    name: Some("value".into()),
                    ty: TrustedTypeExpr::GenericParameter(0),
                }]),
                result: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
                requirements: Box::new([]),
            },
        )
        .with_trait_adapter(TrustedTraitAdapter {
            method: method_id,
            argument: 0,
        }),
    ])
    .err()
    .expect("adapter without its trait bound must be rejected");
    assert_eq!(
        invalid_adapter,
        TrustedSignatureError::AdapterContractMismatch(adapter_id)
    );
}

#[test]
fn trusted_method_contracts_specialize_associated_bindings_and_alpha_bounds() {
    fn id(value: &str) -> ExternalDefId {
        ExternalDefId::new(value).unwrap()
    }

    let trait_id = id("test::HasItem");
    let marker_id = id("test::Marker");
    let associated_id = id("test::HasItem::Item");
    let method_id = id("test::HasItem::get");
    let implementation_id = id("test::impl::HasItem::Bool");
    let binding_id = id("test::impl::HasItem::Bool::Item");
    let callable_id = id("test::impl::HasItem::Bool::get");

    let trait_declaration = TrustedDeclarationSignature::new(
        trait_id.clone(),
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([]),
            associated_types: Box::new([TrustedAssociatedType {
                id: associated_id.clone(),
                name: "Item".into(),
            }]),
        },
    )
    .with_trait_methods([TrustedTraitMethod {
        declaration: method_id.clone(),
        name: NameKey::new("get", false).unwrap(),
        has_default: false,
    }]);
    let marker = TrustedDeclarationSignature::new(
        marker_id.clone(),
        TrustedSignatureKind::Trait {
            closed: false,
            generic_parameters: Box::new([]),
            associated_types: Box::new([]),
        },
    );
    let method = TrustedDeclarationSignature::new(
        method_id.clone(),
        TrustedSignatureKind::Callable {
            bang: false,
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "U".into(),
                bounds: Box::new([TrustedTraitExpr {
                    declaration: marker_id.clone(),
                    arguments: Box::new([]),
                }]),
            }]),
            parameters: Box::new([TrustedCallableParameter {
                name: Some("self".into()),
                ty: TrustedTypeExpr::Reference {
                    mutability: ReferenceMutability::Shared,
                    referent: Box::new(TrustedTypeExpr::SelfType(trait_id.clone())),
                },
            }]),
            result: TrustedTypeExpr::AssociatedProjection {
                base: Box::new(TrustedTypeExpr::SelfType(trait_id.clone())),
                trait_declaration: trait_id.clone(),
                trait_arguments: Box::new([]),
                associated: associated_id.clone(),
            },
            requirements: Box::new([]),
        },
    );
    let implementation = TrustedDeclarationSignature::new(
        implementation_id,
        TrustedSignatureKind::Impl {
            negative: false,
            generic_parameters: Box::new([]),
            trait_ref: TrustedTraitExpr {
                declaration: trait_id.clone(),
                arguments: Box::new([]),
            },
            target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool),
            associated_bindings: Box::new([TrustedAssociatedBinding {
                declaration: binding_id,
                associated: associated_id.clone(),
                value: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
            }]),
        },
    )
    .with_impl_methods([TrustedImplMethod {
        trait_method: method_id.clone(),
        callable: callable_id.clone(),
    }]);
    let actual = |with_bound| {
        TrustedDeclarationSignature::new(
            callable_id.clone(),
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "Renamed".into(),
                    bounds: if with_bound {
                        Box::new([TrustedTraitExpr {
                            declaration: marker_id.clone(),
                            arguments: Box::new([]),
                        }])
                    } else {
                        Box::new([])
                    },
                }]),
                parameters: Box::new([TrustedCallableParameter {
                    name: Some("receiver".into()),
                    ty: TrustedTypeExpr::Reference {
                        mutability: ReferenceMutability::Shared,
                        referent: Box::new(TrustedTypeExpr::Primitive(
                            eglur_sema::PrimitiveType::Bool,
                        )),
                    },
                }]),
                result: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
                requirements: Box::new([]),
            },
        )
    };

    let recursive_implementation = TrustedDeclarationSignature::new(
        id("test::impl::HasItem::RecursiveBool"),
        TrustedSignatureKind::Impl {
            negative: false,
            generic_parameters: Box::new([]),
            trait_ref: TrustedTraitExpr {
                declaration: trait_id.clone(),
                arguments: Box::new([]),
            },
            target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool),
            associated_bindings: Box::new([TrustedAssociatedBinding {
                declaration: id("test::impl::HasItem::RecursiveBool::Item"),
                associated: associated_id.clone(),
                value: TrustedTypeExpr::AssociatedProjection {
                    base: Box::new(TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool)),
                    trait_declaration: trait_id,
                    trait_arguments: Box::new([]),
                    associated: associated_id,
                },
            }]),
        },
    )
    .with_impl_methods([TrustedImplMethod {
        trait_method: method_id,
        callable: callable_id.clone(),
    }]);
    assert!(matches!(
        TrustedSignatureEnvironment::new([
            trait_declaration.clone(),
            marker.clone(),
            method.clone(),
            recursive_implementation,
            actual(true),
        ])
        .err(),
        Some(TrustedSignatureError::MethodContractMismatch { .. })
    ));

    assert!(
        TrustedSignatureEnvironment::new([
            trait_declaration.clone(),
            marker.clone(),
            method.clone(),
            implementation.clone(),
            actual(true),
        ])
        .is_ok()
    );
    assert!(matches!(
        TrustedSignatureEnvironment::new([
            trait_declaration,
            marker,
            method,
            implementation,
            actual(false),
        ])
        .err(),
        Some(TrustedSignatureError::MethodContractMismatch { .. })
    ));
}

#[test]
fn trusted_signature_ingress_enforces_type_positions() {
    fn trusted_error(
        kind: DeclarationKind,
        parameters: Box<[TrustedCallableParameter]>,
        result: TrustedTypeExpr,
    ) -> TrustedSignatureError {
        let (external, trusted) = trusted_callable_environment(kind, parameters, result);
        match try_analyze(
            "fn local() -> i64 = 0\n",
            &CapabilitySet::signatures(),
            Some(&external),
            Some(&trusted),
        ) {
            Err(SignatureResolutionError::Trusted(error)) => error,
            Err(other) => panic!("unexpected signature error: {other}"),
            Ok(_) => panic!("malformed trusted signature was accepted"),
        }
    }

    let standalone_union = trusted_error(
        DeclarationKind::Function,
        Box::new([]),
        TrustedTypeExpr::ErrorUnion(Box::new([
            TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
            TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
        ])),
    );
    assert!(matches!(
        standalone_union,
        TrustedSignatureError::InvalidTypePosition { .. }
    ));

    let ordinary_pointer = trusted_error(
        DeclarationKind::Function,
        Box::new([TrustedCallableParameter {
            name: Some("value".into()),
            ty: TrustedTypeExpr::RawPointer {
                mutability: RawPointerMutability::Const,
                referent: Box::new(TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::U8)),
            },
        }]),
        TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Unit),
    );
    assert!(matches!(
        ordinary_pointer,
        TrustedSignatureError::InvalidTypePosition { .. }
    ));

    let invalid_extern_pointer = trusted_error(
        DeclarationKind::ExternFunction,
        Box::new([TrustedCallableParameter {
            name: Some("value".into()),
            ty: TrustedTypeExpr::RawPointer {
                mutability: RawPointerMutability::Const,
                referent: Box::new(TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Bool)),
            },
        }]),
        TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Unit),
    );
    assert!(matches!(
        invalid_extern_pointer,
        TrustedSignatureError::InvalidTypePosition { .. }
    ));

    let nested_ownership = trusted_error(
        DeclarationKind::Function,
        Box::new([TrustedCallableParameter {
            name: Some("value".into()),
            ty: TrustedTypeExpr::Tuple(Box::new([TrustedTypeExpr::Intrinsic {
                head: IntrinsicNominal::Own,
                arguments: Box::new([TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64)]),
            }])),
        }]),
        TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Unit),
    );
    assert!(matches!(
        nested_ownership,
        TrustedSignatureError::InvalidTypePosition { .. }
    ));

    let (external, trusted) = trusted_callable_environment(
        DeclarationKind::Function,
        Box::new([]),
        TrustedTypeExpr::Intrinsic {
            head: IntrinsicNominal::Res,
            arguments: Box::new([
                TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64),
                TrustedTypeExpr::ErrorUnion(Box::new([
                    TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
                    TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64),
                ])),
            ]),
        },
    );
    assert!(
        try_analyze(
            "fn local() -> i64 = 0\n",
            &CapabilitySet::signatures(),
            Some(&external),
            Some(&trusted),
        )
        .is_ok()
    );

    let owned = TrustedTypeExpr::Intrinsic {
        head: IntrinsicNominal::Own,
        arguments: Box::new([TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64)]),
    };
    let (external, trusted) = trusted_callable_environment(
        DeclarationKind::Function,
        Box::new([TrustedCallableParameter {
            name: Some("value".into()),
            ty: owned.clone(),
        }]),
        owned,
    );
    assert!(
        try_analyze(
            "fn local() -> i64 = 0\n",
            &CapabilitySet::signatures(),
            Some(&external),
            Some(&trusted),
        )
        .is_ok()
    );
}

#[test]
fn trusted_negative_impl_is_rejected_at_ingress() {
    let module =
        ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("negative")]).unwrap();
    let marker = ExternalDefId::new("std::negative::Marker").unwrap();
    let marked = ExternalDefId::new("std::negative::Marked").unwrap();
    let implementation = ExternalDefId::new("std::negative::Marked::Marker").unwrap();
    let declarations = [
        ExternalDeclaration::new(
            marker.clone(),
            module.clone(),
            NameKey::new("Marker", false).unwrap(),
            DeclarationKind::Trait,
        )
        .unwrap(),
        ExternalDeclaration::new(
            marked.clone(),
            module.clone(),
            NameKey::new("Marked", false).unwrap(),
            DeclarationKind::Struct,
        )
        .unwrap(),
    ];
    let external = ExternalNameEnvironment::new([module], declarations, []).unwrap();
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            marker.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            marked.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            implementation.clone(),
            TrustedSignatureKind::Impl {
                negative: true,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: marker,
                    arguments: Box::new([]),
                },
                target: TrustedTypeExpr::Nominal {
                    head: marked,
                    arguments: Box::new([]),
                },
                associated_bindings: Box::new([]),
            },
        ),
    ])
    .unwrap();

    let error = match try_analyze(
        "fn local() -> i64 = 0\n",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    ) {
        Err(SignatureResolutionError::Trusted(error)) => error,
        Err(other) => panic!("unexpected signature error: {other}"),
        Ok(_) => panic!("trusted negative impl was accepted"),
    };
    assert_eq!(
        error,
        TrustedSignatureError::NegativeImplUnsupported(implementation)
    );
}

#[test]
fn trusted_impl_generic_below_error_union_is_rejected_at_ingress() {
    let module =
        ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("generic_union")]).unwrap();
    let marker = ExternalDefId::new("std::generic_union::Marker").unwrap();
    let implementation = ExternalDefId::new("std::generic_union::Res::Marker").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [ExternalDeclaration::new(
            marker.clone(),
            module,
            NameKey::new("Marker", false).unwrap(),
            DeclarationKind::Trait,
        )
        .unwrap()],
        [marker.clone()],
    )
    .unwrap();
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            marker.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            implementation.clone(),
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "E".into(),
                    bounds: Box::new([]),
                }]),
                trait_ref: TrustedTraitExpr {
                    declaration: marker,
                    arguments: Box::new([]),
                },
                target: TrustedTypeExpr::Intrinsic {
                    head: IntrinsicNominal::Res,
                    arguments: Box::new([
                        TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Unit),
                        TrustedTypeExpr::ErrorUnion(Box::new([
                            TrustedTypeExpr::GenericParameter(0),
                            TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
                        ])),
                    ]),
                },
                associated_bindings: Box::new([]),
            },
        ),
    ])
    .unwrap();

    let error = match try_analyze(
        "fn local() -> i64 = 0\n",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    ) {
        Err(SignatureResolutionError::Trusted(error)) => error,
        Err(other) => panic!("unexpected signature error: {other}"),
        Ok(_) => panic!("trusted generic error-union impl was accepted"),
    };
    assert_eq!(
        error,
        TrustedSignatureError::InvalidTypePosition {
            declaration: implementation,
            reason: "generic parameter below structural error union in impl header",
        }
    );
}

#[test]
fn trusted_associated_binding_cycles_are_rejected_at_ingress() {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("cycle")]).unwrap();
    let linked = ExternalDefId::new("std::cycle::Linked").unwrap();
    let item = ExternalDefId::new("std::cycle::Linked::Item").unwrap();
    let node = ExternalDefId::new("std::cycle::Node").unwrap();
    let implementation = ExternalDefId::new("std::cycle::Node::Linked").unwrap();
    let binding = ExternalDefId::new("std::cycle::Node::Linked::Item").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            ExternalDeclaration::new(
                linked.clone(),
                module.clone(),
                NameKey::new("Linked", false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
            ExternalDeclaration::new(
                node.clone(),
                module,
                NameKey::new("Node", false).unwrap(),
                DeclarationKind::Struct,
            )
            .unwrap(),
        ],
        [linked.clone(), node.clone()],
    )
    .unwrap();
    let node_type = TrustedTypeExpr::Nominal {
        head: node.clone(),
        arguments: Box::new([]),
    };
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            linked.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([TrustedAssociatedType {
                    id: item.clone(),
                    name: "Item".into(),
                }]),
            },
        ),
        TrustedDeclarationSignature::new(
            node,
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            implementation,
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: linked.clone(),
                    arguments: Box::new([]),
                },
                target: node_type.clone(),
                associated_bindings: Box::new([TrustedAssociatedBinding {
                    declaration: binding,
                    associated: item.clone(),
                    value: TrustedTypeExpr::AssociatedProjection {
                        base: Box::new(node_type),
                        trait_declaration: linked,
                        trait_arguments: Box::new([]),
                        associated: item,
                    },
                }]),
            },
        ),
    ])
    .unwrap();
    let error = match try_analyze(
        "fn local() -> i64 = 0\n",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    ) {
        Err(SignatureResolutionError::Trusted(error)) => error,
        Err(other) => panic!("unexpected signature error: {other}"),
        Ok(_) => panic!("trusted associated binding cycle was accepted"),
    };
    assert!(matches!(
        error,
        TrustedSignatureError::InvalidTypePosition {
            reason: "associated type binding cycle",
            ..
        }
    ));
}

#[test]
fn trusted_associated_bindings_are_reference_free() {
    let module =
        ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("binding")]).unwrap();
    let linked = ExternalDefId::new("std::binding::Linked").unwrap();
    let item = ExternalDefId::new("std::binding::Linked::Item").unwrap();
    let node = ExternalDefId::new("std::binding::Node").unwrap();
    let implementation = ExternalDefId::new("std::binding::Node::Linked").unwrap();
    let binding = ExternalDefId::new("std::binding::Node::Linked::Item").unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [
            ExternalDeclaration::new(
                linked.clone(),
                module.clone(),
                NameKey::new("Linked", false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
            ExternalDeclaration::new(
                node.clone(),
                module,
                NameKey::new("Node", false).unwrap(),
                DeclarationKind::Struct,
            )
            .unwrap(),
        ],
        [linked.clone(), node.clone()],
    )
    .unwrap();
    let trusted = TrustedSignatureEnvironment::new([
        TrustedDeclarationSignature::new(
            linked.clone(),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([TrustedAssociatedType {
                    id: item.clone(),
                    name: "Item".into(),
                }]),
            },
        ),
        TrustedDeclarationSignature::new(
            node.clone(),
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        ),
        TrustedDeclarationSignature::new(
            implementation,
            TrustedSignatureKind::Impl {
                negative: false,
                generic_parameters: Box::new([]),
                trait_ref: TrustedTraitExpr {
                    declaration: linked,
                    arguments: Box::new([]),
                },
                target: TrustedTypeExpr::Nominal {
                    head: node,
                    arguments: Box::new([]),
                },
                associated_bindings: Box::new([TrustedAssociatedBinding {
                    declaration: binding,
                    associated: item,
                    value: TrustedTypeExpr::Reference {
                        mutability: ReferenceMutability::Shared,
                        referent: Box::new(TrustedTypeExpr::Primitive(
                            eglur_sema::PrimitiveType::I64,
                        )),
                    },
                }]),
            },
        ),
    ])
    .unwrap();
    let error = match try_analyze(
        "fn local() -> i64 = 0\n",
        &CapabilitySet::signatures(),
        Some(&external),
        Some(&trusted),
    ) {
        Err(SignatureResolutionError::Trusted(error)) => error,
        Err(other) => panic!("unexpected signature error: {other}"),
        Ok(_) => panic!("reference-bearing trusted associated binding was accepted"),
    };
    assert!(matches!(
        error,
        TrustedSignatureError::InvalidTypePosition {
            reason: "reference-bearing associated type binding",
            ..
        }
    ));
}

fn trusted_environment(reverse: bool) -> (ExternalNameEnvironment, TrustedSignatureEnvironment) {
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("core")]).unwrap();
    let readable = ExternalDefId::new("std::core::Readable").unwrap();
    let cell = ExternalDefId::new("std::core::Cell").unwrap();
    let read = ExternalDefId::new("std::core::read").unwrap();
    let declarations = [
        ExternalDeclaration::new(
            readable.clone(),
            module.clone(),
            NameKey::new("Readable", false).unwrap(),
            DeclarationKind::Trait,
        )
        .unwrap(),
        ExternalDeclaration::new(
            cell.clone(),
            module.clone(),
            NameKey::new("Cell", false).unwrap(),
            DeclarationKind::Struct,
        )
        .unwrap(),
        ExternalDeclaration::new(
            read.clone(),
            module.clone(),
            NameKey::new("read", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    ];
    let names = ExternalNameEnvironment::new(
        [module],
        declarations,
        [readable.clone(), cell.clone(), read.clone()],
    )
    .unwrap();
    let item = ExternalDefId::new("std::core::Readable::Item").unwrap();
    let mut signatures = vec![
        TrustedDeclarationSignature::new(
            readable.clone(),
            TrustedSignatureKind::Trait {
                closed: true,
                generic_parameters: Box::new([]),
                associated_types: Box::new([TrustedAssociatedType {
                    id: item.clone(),
                    name: "Item".into(),
                }]),
            },
        ),
        TrustedDeclarationSignature::new(
            cell,
            TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "T".into(),
                    bounds: Box::new([]),
                }]),
            },
        ),
        TrustedDeclarationSignature::new(
            read,
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([TrustedGenericParameter {
                    name: "C".into(),
                    bounds: Box::new([TrustedTraitExpr {
                        declaration: readable.clone(),
                        arguments: Box::new([]),
                    }]),
                }]),
                parameters: Box::new([TrustedCallableParameter {
                    name: Some("value".into()),
                    ty: TrustedTypeExpr::GenericParameter(0),
                }]),
                result: TrustedTypeExpr::AssociatedProjection {
                    base: Box::new(TrustedTypeExpr::GenericParameter(0)),
                    trait_declaration: readable,
                    trait_arguments: Box::new([]),
                    associated: item,
                },
                requirements: Box::new([]),
            },
        ),
    ];
    if reverse {
        signatures.reverse();
    }
    (names, TrustedSignatureEnvironment::new(signatures).unwrap())
}

#[test]
fn trusted_environment_normalizes_unordered_member_sets() {
    fn environment(reverse: bool) -> TrustedSignatureEnvironment {
        let trait_id = ExternalDefId::new("std::pair::Pair").unwrap();
        let left = ExternalDefId::new("std::pair::Pair::Left").unwrap();
        let right = ExternalDefId::new("std::pair::Pair::Right").unwrap();
        let mut associated_types = vec![
            TrustedAssociatedType {
                id: left.clone(),
                name: "Left".into(),
            },
            TrustedAssociatedType {
                id: right.clone(),
                name: "Right".into(),
            },
        ];
        let mut associated_bindings = vec![
            TrustedAssociatedBinding {
                declaration: ExternalDefId::new("std::pair::impl::left").unwrap(),
                associated: left,
                value: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64),
            },
            TrustedAssociatedBinding {
                declaration: ExternalDefId::new("std::pair::impl::right").unwrap(),
                associated: right,
                value: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::Str),
            },
        ];
        if reverse {
            associated_types.reverse();
            associated_bindings.reverse();
        }
        let mut declarations = vec![
            TrustedDeclarationSignature::new(
                trait_id.clone(),
                TrustedSignatureKind::Trait {
                    closed: true,
                    generic_parameters: Box::new([]),
                    associated_types: associated_types.into_boxed_slice(),
                },
            ),
            TrustedDeclarationSignature::new(
                ExternalDefId::new("std::pair::impl").unwrap(),
                TrustedSignatureKind::Impl {
                    negative: false,
                    generic_parameters: Box::new([]),
                    trait_ref: TrustedTraitExpr {
                        declaration: trait_id,
                        arguments: Box::new([]),
                    },
                    target: TrustedTypeExpr::Primitive(eglur_sema::PrimitiveType::I64),
                    associated_bindings: associated_bindings.into_boxed_slice(),
                },
            ),
        ];
        if reverse {
            declarations.reverse();
        }
        TrustedSignatureEnvironment::new(declarations).unwrap()
    }

    let forward = environment(false)
        .declarations()
        .cloned()
        .collect::<Vec<_>>();
    let reverse = environment(true)
        .declarations()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(forward, reverse);
}

#[test]
fn trusted_signature_order_does_not_change_canonical_graph() {
    fn graph(reverse: bool) -> Vec<TypeKind> {
        let (external, trusted) = trusted_environment(reverse);
        let analysis = analyze(
            "fn local() -> i64 = 1\n",
            &CapabilitySet::signatures(),
            Some(&external),
            Some(&trusted),
        );
        assert!(matches!(
            analysis.diagnostics,
            DiagnosticSnapshot::Accepted(_)
        ));
        analysis
            .signatures
            .unwrap()
            .types()
            .types()
            .map(|(_, kind)| kind.clone())
            .collect()
    }

    let forward = graph(false);
    let reverse = graph(true);
    assert_eq!(forward, reverse);
    assert!(forward.iter().any(|kind| {
        matches!(
            kind,
            TypeKind::AssociatedProjection(AssociatedProjection { .. })
        )
    }));
}

#[test]
fn unsupported_signature_capability_leaves_phase_unperformed() {
    let external = empty_external();
    let analysis = analyze(
        "fn value() -> i64 = 1\n",
        &CapabilitySet::without_signatures(),
        Some(&external),
        None,
    );
    assert!(analysis.signatures.is_none());
    assert!(analysis.demands.iter().any(|demand| {
        demand.capability()
            == CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures)
            && demand.disposition() == DemandDisposition::Missing
    }));
}
