use std::cell::Cell;

use eglur_diagnostics::{DiagnosticSnapshot, DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::lower;
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

use super::super::*;
use crate::{
    CompileCapability, CompileSupport, ExternalNameEnvironment, ImportAnalysisUnit, ModuleId,
    PhaseAnalysis, PhaseProduct, ResolveCapability, SemanticSource, SignatureResolutionInputs,
    TypecheckCapability, collect_declarations, plan_imports, resolve_declaration_signatures,
    resolve_imports,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    queries: usize,
    owners: usize,
    parameters: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { queries: 0, owners: 0, parameters: 0 }) };
}

pub(in crate::fingerprint::identity) fn observe_query() {
    let mut work = WORK.get();
    work.queries += 1;
    WORK.set(work);
}

pub(in crate::fingerprint::identity) fn observe_owner() {
    let mut work = WORK.get();
    work.owners += 1;
    WORK.set(work);
}

pub(in crate::fingerprint::identity) fn observe_parameter() {
    let mut work = WORK.get();
    work.parameters += 1;
    WORK.set(work);
}

struct Support;

impl CompileSupport for Support {
    fn supports(&self, capability: CompileCapability) -> bool {
        matches!(
            capability,
            CompileCapability::Resolve(
                ResolveCapability::CollectDeclarations
                    | ResolveCapability::ResolveFileSetImports
                    | ResolveCapability::ResolveModuleValues
            ) | CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures)
        )
    }
}

fn performed<T>(analysis: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = analysis.into_parts().0 else {
        panic!("required header phase was not performed");
    };
    value
}

pub(in crate::fingerprint::identity) fn headers(
    text: &str,
) -> (
    DeclarationCollection,
    DeclarationSignatures,
    DiagnosticSnapshot,
) {
    let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let validated = source.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parsed = parse_validated(validated);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let hir = lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let label = SourceLabel::new("main.eglur").unwrap();
    let spans = hir
        .origins()
        .map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let sources = [SemanticSource::new(
        FileId::new(0),
        ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        spans,
        [],
        None,
    )
    .unwrap()];
    let collection = performed(collect_declarations(&sources, &Support).unwrap());
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let mut diagnostics = DiagnosticStore::new();
    let plan = performed(
        plan_imports(
            &collection,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let units = [ImportAnalysisUnit::new([sources[0].file()]).unwrap()];
    let imports = performed(
        resolve_imports(
            &collection,
            &plan,
            &units,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let signatures = performed(
        resolve_declaration_signatures(
            &collection,
            &imports,
            &sources,
            &Support,
            SignatureResolutionInputs::new(Some(&external), None),
            &mut diagnostics,
        )
        .unwrap(),
    )
    .into_parts()
    .0;
    (collection, signatures, diagnostics.finalize(None))
}

pub(in crate::fingerprint::identity) fn owner(
    collection: &DeclarationCollection,
    name: &str,
) -> DefId {
    collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|value| value.text() == name))
        .unwrap()
        .id()
}

#[test]
fn wide_generic_owners_are_indexed_once() {
    for public in [false, true] {
        for count in [8_usize, 32, 128] {
            let visibility = if public { "pub " } else { "" };
            let names = (0..count)
                .map(|index| format!("T{index:03}"))
                .collect::<Vec<_>>()
                .join(", ");
            let source = format!("{visibility}fn wide[{names}](value: Bool) -> Bool = value\n");
            let (collection, signatures, diagnostics) = headers(&source);
            assert!(diagnostics.diagnostics().is_empty());
            let owner = owner(&collection, "wide");
            let parameters = direct_generic_parameters(&signatures, owner).unwrap();
            assert_eq!(parameters.len(), count);

            WORK.set(Work::default());
            let keys = StableDeclarationKeys::derive(&collection, &signatures).unwrap();
            assert_eq!(
                WORK.get().parameters,
                count,
                "generics={count}, public={public}"
            );
            assert_eq!(
                WORK.get(),
                Work {
                    queries: count,
                    owners: 1,
                    parameters: count
                }
            );
            for (index, parameter) in parameters.iter().enumerate() {
                let GenericParameterId::Source(parameter) = parameter else {
                    panic!("source generic");
                };
                assert_eq!(
                    keys.source(*parameter).unwrap().segments.last(),
                    Some(&StableDeclarationSegment::GenericParameter(
                        u32::try_from(index).unwrap()
                    ))
                );
            }

            WORK.set(Work::default());
            let mut deriver = KeyDeriver::new(&collection, &signatures);
            for _ in 0..2 {
                for (index, parameter) in parameters.iter().enumerate().rev() {
                    let GenericParameterId::Source(parameter) = parameter else {
                        panic!("source generic");
                    };
                    assert_eq!(
                        deriver.generic_parameter_index(owner, *parameter).unwrap(),
                        u32::try_from(index).unwrap()
                    );
                }
            }
            assert_eq!(
                WORK.get(),
                Work {
                    queries: 2 * count,
                    owners: 1,
                    parameters: count
                }
            );
            assert_eq!(deriver.generic_parameter_indices.len(), 1);
            assert_eq!(deriver.generic_parameter_indices[&owner].len(), count);
        }
    }
}

#[test]
fn owner_families_preserve_direct_ordinals_and_inherited_contexts() {
    let source = "fn echo[A, B](value: Bool) -> Bool = value\n\
        struct Host[A, B] { fn same[C](self: &Self, value: C) -> C = value }\n\
        enum Choice[A, B] { Left(A)\n Right(B) }\n\
        trait Pick[A] { fn same[B](self: &Self, value: B) -> B }\n\
        type Pair[A, B] = (A, B)\n\
        trait Mark {}\n\
        impl[A, B] Mark for Host[A, B] {}\n";
    let (collection, signatures, diagnostics) = headers(source);
    assert!(
        diagnostics.diagnostics().is_empty(),
        "{:?}",
        diagnostics.diagnostics()
    );
    let keys = StableDeclarationKeys::derive(&collection, &signatures).unwrap();
    let mut deriver = KeyDeriver::new(&collection, &signatures);
    for declaration in collection
        .modules()
        .flat_map(|module| module.declarations())
    {
        if declaration.kind() != DeclarationKind::GenericParameter {
            continue;
        }
        let parent = declaration.parent().unwrap();
        let parameters = direct_generic_parameters(&signatures, parent).unwrap();
        let expected = parameters
            .iter()
            .position(|parameter| parameter == &GenericParameterId::Source(declaration.id()))
            .unwrap();
        let expected = u32::try_from(expected).unwrap();
        assert_eq!(
            deriver
                .generic_parameter_index(parent, declaration.id())
                .unwrap(),
            expected
        );
        assert_eq!(
            keys.source(declaration.id()).unwrap().segments.last(),
            Some(&StableDeclarationSegment::GenericParameter(expected))
        );

        if declaration.name().unwrap().text() == "C" {
            assert_eq!(expected, 0);
            let context = DeclarationBinderContexts::new(&collection, &signatures)
                .context(parent)
                .unwrap();
            assert_eq!(
                context.ordinal(&GenericParameterId::Source(declaration.id())),
                Some(2)
            );
        }
    }
}

#[test]
fn source_projection_keeps_first_match_and_trusted_slot_positions() {
    let module = ModuleId::from_index(0).unwrap();
    let first = DefId::from_index(module, 3).unwrap();
    let second = DefId::from_index(module, 4).unwrap();
    let trusted = GenericParameterId::Trusted {
        owner: ExternalDefId::new("probe::Owner").unwrap(),
        index: 0,
    };
    let parameters = [
        trusted,
        GenericParameterId::Source(first),
        GenericParameterId::Source(second),
        GenericParameterId::Source(first),
    ];
    let indices = index_source_generic_parameters(&parameters);
    for source in [first, second] {
        assert_eq!(
            indices[&source],
            parameters
                .iter()
                .position(|parameter| parameter == &GenericParameterId::Source(source))
                .unwrap()
        );
    }
    assert_eq!(indices.len(), 2);
    assert_eq!(indices[&first], 1);
    assert_eq!(indices[&second], 2);
}

#[test]
fn retained_impl_header_keeps_ordinal_authority_after_binding_failure() {
    let source = "trait Carries[A] { type Item }\n\
        struct Host[A] {}\n\
        impl[A] Carries[A] for Host[A] {}\n";
    let (collection, signatures, diagnostics) = headers(source);
    assert!(!diagnostics.diagnostics().is_empty());
    let implementation = collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.kind() == DeclarationKind::Impl)
        .unwrap()
        .id();
    assert!(matches!(
        signatures.source_declaration(implementation),
        FactState::Poisoned(_)
    ));
    let FactState::Known(header) = signatures.impl_header(&CanonicalTarget::Source(implementation))
    else {
        panic!("binding failure must preserve the valid impl header");
    };
    let GenericParameterId::Source(parameter) = header.generic_parameters()[0] else {
        panic!("source impl generic");
    };
    let mut deriver = KeyDeriver::new(&collection, &signatures);
    assert_eq!(
        deriver.generic_parameter_index(implementation, parameter),
        Ok(0)
    );
}

#[test]
fn missing_poisoned_and_wrong_owner_queries_preserve_errors() {
    let (collection, signatures, diagnostics) = headers(
        "fn left[A](value: Bool) -> Bool = value\nfn right[B](value: Bool) -> Bool = value",
    );
    assert!(diagnostics.diagnostics().is_empty());
    let left = owner(&collection, "left");
    let right = owner(&collection, "right");
    let GenericParameterId::Source(parameter) =
        direct_generic_parameters(&signatures, left).unwrap()[0]
    else {
        panic!("source generic");
    };
    let mut deriver = KeyDeriver::new(&collection, &signatures);
    let missing = DefId::from_index(ModuleId::from_index(99).unwrap(), 0).unwrap();
    assert_eq!(
        deriver.generic_parameter_index(missing, parameter),
        Err(StableDeclarationError::IncompleteSignature(missing))
    );
    assert!(!deriver.generic_parameter_indices.contains_key(&missing));
    assert_eq!(
        deriver.generic_parameter_index(right, parameter),
        Err(StableDeclarationError::UnknownGenericParameter {
            owner: right,
            parameter
        })
    );
    assert_eq!(deriver.generic_parameter_index(left, parameter), Ok(0));
    assert_eq!(
        deriver.generic_parameter_index(parameter, parameter),
        Err(StableDeclarationError::UnknownGenericParameter {
            owner: parameter,
            parameter
        })
    );

    let (collection, signatures, diagnostics) =
        headers("fn broken[A](value: Unknown) -> Bool = true");
    assert!(!diagnostics.diagnostics().is_empty());
    let broken = owner(&collection, "broken");
    assert!(matches!(
        signatures.source_declaration(broken),
        FactState::Poisoned(_)
    ));
    let parameter = owner(&collection, "A");
    let mut deriver = KeyDeriver::new(&collection, &signatures);
    assert_eq!(
        deriver.generic_parameter_index(broken, parameter),
        Err(StableDeclarationError::IncompleteSignature(broken))
    );
    assert!(deriver.generic_parameter_indices.is_empty());
}

#[test]
fn cached_ordinal_conversion_keeps_the_query_error() {
    let (collection, signatures, diagnostics) = headers("fn probe[A](value: Bool) -> Bool = value");
    assert!(diagnostics.diagnostics().is_empty());
    let owner = owner(&collection, "probe");
    let parameter = collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
        .unwrap()
        .id();
    let mut deriver = KeyDeriver::new(&collection, &signatures);
    assert_eq!(deriver.generic_parameter_index(owner, parameter), Ok(0));

    // Injected cache boundary, not a source requiring billions of parameters.
    deriver
        .generic_parameter_indices
        .get_mut(&owner)
        .unwrap()
        .insert(parameter, u32::MAX as usize);
    assert_eq!(
        deriver.generic_parameter_index(owner, parameter),
        Ok(u32::MAX)
    );
    if let Some(overflow) = (u32::MAX as usize).checked_add(1) {
        deriver
            .generic_parameter_indices
            .get_mut(&owner)
            .unwrap()
            .insert(parameter, overflow);
        assert_eq!(
            deriver.generic_parameter_index(owner, parameter),
            Err(StableDeclarationError::UnknownGenericParameter { owner, parameter })
        );
    }
}
