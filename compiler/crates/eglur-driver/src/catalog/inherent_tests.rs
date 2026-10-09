use super::*;
use crate::{CompilerSession, FileInput, FileSetRequest, SessionOptions, SupportProfile};
use eglur_hir::Visibility;
use eglur_sema::{
    InherentMemberForm, TrustedInherentMember, TrustedInherentOwner, TrustedInherentOwnerHead,
};

mod dispatch;

fn signatures() -> TrustedSignatureEnvironment {
    let id = external_id("fixture::integer_zero");
    TrustedSignatureEnvironment::with_inherent_owners(
        [TrustedDeclarationSignature::new(
            id.clone(),
            TrustedSignatureKind::Callable {
                bang: false,
                generic_parameters: Box::new([]),
                parameters: Box::new([]),
                result: TrustedTypeExpr::Primitive(PrimitiveType::I64),
                requirements: Box::new([]),
            },
        )],
        [TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
            members: Box::new([TrustedInherentMember {
                callable: id,
                name: NameKey::new("zero", false).unwrap(),
                owner_pattern: TrustedTypeExpr::Primitive(PrimitiveType::I64),
                owner_parameter_count: 0,
                form: InherentMemberForm::Associated,
                visibility: Visibility::Public,
                overload: false,
            }]),
        }],
    )
    .unwrap()
}

fn catalog(expose_name: bool, include_effect: bool) -> Result<TrustedCatalog, TrustedCatalogError> {
    let id = external_id("fixture::integer_zero");
    let module = module(["fixture"]);
    let names = ExternalNameEnvironment::partial_standard_library(
        [module.clone()],
        expose_name.then(|| {
            declaration(id.clone(), module, "zero", false, DeclarationKind::Function).unwrap()
        }),
        [],
    )
    .unwrap();
    let effects = TrustedEffectEnvironment::new(
        include_effect.then(|| TrustedEffectSummary::new(id, EffectSummary::pure())),
    )
    .unwrap();
    TrustedCatalog::new(
        names,
        signatures(),
        TrustedInhabitanceEnvironment::new([]).unwrap(),
        effects,
        [],
    )
}

#[test]
fn ordinary_members_are_admitted_as_hidden_callables_with_exact_effect_identity() {
    let catalog = catalog(false, true).unwrap();
    let id = external_id("fixture::integer_zero");
    assert!(catalog.signatures().is_hidden_callable(&id));
    assert!(catalog.names().declaration(&id).is_none());
    assert_eq!(catalog.effects().summary(&id), Some(&EffectSummary::pure()));
    assert!(catalog.runtime_hook(&id).is_none());
}

#[test]
fn ordinary_members_cannot_bypass_name_or_effect_admission() {
    let id = external_id("fixture::integer_zero");
    assert!(
        matches!(catalog(true, true), Err(TrustedCatalogError::DeclarationKindMismatch(found)) if found == id)
    );
    assert!(
        matches!(catalog(false, false), Err(TrustedCatalogError::MissingEffect(found)) if found == id)
    );
}

#[test]
fn complete_owner_metadata_changes_real_analysis_keys_without_changing_source_semantics() {
    let analyze = |owners: Vec<TrustedInherentOwner>| {
        let TrustedCatalog {
            names,
            signatures,
            inhabitance,
            effects,
            language_items,
            runtime_hooks,
        } = TrustedCatalog::first_slice().unwrap();
        let signatures = TrustedSignatureEnvironment::with_inherent_owners(
            signatures.declarations().cloned(),
            owners,
        )
        .unwrap();
        let catalog = TrustedCatalog::new_with_language_items(
            names,
            signatures,
            inhabitance,
            effects,
            language_items,
            runtime_hooks,
        )
        .unwrap();
        let request = FileSetRequest::new(
            [FileInput::new(
                "main.eglur",
                b"fn local() -> Unit = ()\n".to_vec(),
            )],
            "main.eglur",
            None,
        )
        .unwrap();
        let mut session = CompilerSession::build(request, SessionOptions::default()).unwrap();
        let analysis = session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let inputs = analysis.fingerprints().unwrap();
        (
            inputs.catalog(),
            inputs.units()[0].key(),
            analysis
                .resolution()
                .unwrap()
                .fingerprints()
                .unwrap()
                .clone(),
        )
    };
    let absent = analyze(vec![]);
    let empty = analyze(vec![TrustedInherentOwner {
        head: TrustedInherentOwnerHead::Primitive(PrimitiveType::Str),
        members: Box::new([]),
    }]);
    assert_ne!(absent.0, empty.0);
    assert_ne!(absent.1, empty.1);
    assert_eq!(absent.2, empty.2);
}

#[test]
fn complete_empty_inventory_does_not_claim_an_unavailable_input() {
    for complete in [false, true] {
        let TrustedCatalog {
            names,
            signatures,
            inhabitance,
            effects,
            language_items,
            runtime_hooks,
        } = TrustedCatalog::first_slice().unwrap();
        let owner = external_id(NUMERIC_ERROR);
        let signatures = TrustedSignatureEnvironment::with_inherent_owners(
            signatures.declarations().cloned(),
            complete.then(|| TrustedInherentOwner {
                head: TrustedInherentOwnerHead::Nominal(owner.clone()),
                members: Box::new([]),
            }),
        )
        .unwrap();
        let catalog = TrustedCatalog::new_with_language_items(
            names,
            signatures,
            inhabitance,
            effects,
            language_items,
            runtime_hooks,
        )
        .unwrap();
        let mut session = CompilerSession::build(
            FileSetRequest::single(
                "main.eglur",
                b"fn inspect(value: &NumericError) -> Unit = NumericError.missing(value)\n"
                    .to_vec(),
            )
            .unwrap(),
            SessionOptions::default(),
        )
        .unwrap();
        session
            .resolve(&SupportProfile::semantic_core_v1(), Some(&catalog))
            .unwrap();
        let sealed = session.seal(None).unwrap();
        assert!(sealed.checked_program().is_none());
        if complete {
            assert!(!sealed.trace().has_unsupported());
            assert!(
                sealed
                    .diagnostics()
                    .diagnostics()
                    .iter()
                    .any(|diagnostic| diagnostic.code().as_str() == "METHOD_NOT_FOUND")
            );
        } else {
            let entry = sealed.trace().unsupported().next().unwrap();
            assert!(sealed.trace().missing().next().is_none());
            assert_eq!(
                entry.input_refusals()[0].input(),
                &eglur_sema::UnavailableCompileInput::TrustedInherentInventory { owner }
            );
        }
    }
}
