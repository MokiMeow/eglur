use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::{HirFile, ItemKind};
use eglur_source::{FileId, LineIndex, SourceFile};

use crate::*;

pub(crate) struct Fixture {
    pub(crate) signatures: DeclarationSignatures,
    pub(crate) parameters: Vec<GenericParameterId>,
    pub(crate) site: ExpressionKey,
    pub(crate) hir: HirFile,
}

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(product) = phase.into_parts().0 else {
        panic!("expected performed fixture phase")
    };
    product
}

pub(crate) fn fixture(text: &str, trusted_owner: Option<&str>) -> Fixture {
    let input = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parsed = eglur_syntax::parse_validated(validated);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let hir: HirFile = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let module = ModuleAddress::new([Box::<str>::from("main")]).unwrap();
    let label = SourceLabel::new("main.eglur").unwrap();
    let sources = [SemanticSource::new(
        hir.file(),
        module,
        Some(&hir),
        hir.origins().map(|(id, origin)| {
            (
                id,
                SourceSpan::from_origin(label.clone(), origin, &lines).unwrap(),
            )
        }),
        [],
        None,
    )
    .unwrap()];
    let (external, trusted) = trusted_environment(trusted_owner);
    let mut diagnostics = DiagnosticStore::new();
    let collection = performed(collect_declarations(&sources, &Support).unwrap());
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
    let imports = performed(
        resolve_imports(
            &collection,
            &plan,
            &[ImportAnalysisUnit::new([hir.file()]).unwrap()],
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let declarations = performed(
        resolve_declaration_signatures(
            &collection,
            &imports,
            &sources,
            &Support,
            SignatureResolutionInputs::new(
                Some(&external),
                trusted
                    .as_ref()
                    .map(|signatures| TrustedDeclarationInputs::new(signatures, None)),
            ),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let (signatures, _) = declarations.into_parts();
    let snapshot = diagnostics.finalize(None);
    assert!(snapshot.diagnostics().is_empty(), "{snapshot:?}");

    let target = trusted_owner.map_or_else(
        || {
            CanonicalTarget::Source(
                collection
                    .modules()
                    .flat_map(|module| module.declarations())
                    .find(|declaration| {
                        declaration.name().is_some_and(|name| name.text() == "pick")
                    })
                    .unwrap()
                    .id(),
            )
        },
        |owner| CanonicalTarget::External(ExternalDefId::new(owner).unwrap()),
    );
    let FactState::Known(DeclarationSignature::Callable(callable)) =
        signatures.declaration(&target)
    else {
        panic!("expected callable fixture signature")
    };
    let parameters = signatures
        .callable(*callable)
        .unwrap()
        .generic_parameters()
        .to_vec();
    let function = hir
        .items()
        .find_map(|(_, item)| match item.kind() {
            ItemKind::Function(function) if function.name.text() == Some("inspect") => {
                Some(function)
            }
            _ => None,
        })
        .unwrap();
    let site = ExpressionKey::new(
        collection.modules().next().unwrap().id(),
        hir.body(function.body).expressions().last().unwrap().0,
    );
    Fixture {
        signatures,
        parameters,
        site,
        hir,
    }
}

fn trusted_environment(
    owner: Option<&str>,
) -> (ExternalNameEnvironment, Option<TrustedSignatureEnvironment>) {
    let Some(owner) = owner else {
        return (ExternalNameEnvironment::new([], [], []).unwrap(), None);
    };
    let module = ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("probe")]).unwrap();
    let id = |name| ExternalDefId::new(name).unwrap();
    let traits = ["std::probe::Alpha", "std::probe::Zulu"];
    let mut names = Vec::new();
    let mut signatures = Vec::new();
    for name in traits {
        names.push(
            ExternalDeclaration::new(
                id(name),
                module.clone(),
                NameKey::new(name.rsplit("::").next().unwrap(), false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
        );
        signatures.push(TrustedDeclarationSignature::new(
            id(name),
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        ));
    }
    names.push(
        ExternalDeclaration::new(
            id(owner),
            module.clone(),
            NameKey::new("pick", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    );
    signatures.push(TrustedDeclarationSignature::new(
        id(owner),
        TrustedSignatureKind::Callable {
            bang: false,
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "T".into(),
                bounds: [traits[1], traits[0]]
                    .map(|name| TrustedTraitExpr {
                        declaration: id(name),
                        arguments: Box::new([]),
                    })
                    .into(),
            }]),
            parameters: Box::new([]),
            result: TrustedTypeExpr::Tuple(Box::new([])),
            requirements: Box::new([]),
        },
    ));
    (
        ExternalNameEnvironment::new([module], names, []).unwrap(),
        Some(TrustedSignatureEnvironment::new(signatures).unwrap()),
    )
}
