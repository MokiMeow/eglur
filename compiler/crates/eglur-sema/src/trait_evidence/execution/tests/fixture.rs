use super::*;
use crate::*;
use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_source::{FileId, LineIndex, SourceFile};

pub(super) struct Fixture<'input, 'hir> {
    pub collection: &'input DeclarationCollection,
    pub imports: &'input ResolvedImports,
    pub names: &'input NameResolution,
    pub sources: &'input [SemanticSource<'hir>],
    pub declarations: &'input mut ResolvedDeclarationSemantics,
    pub trait_environment: &'input mut TraitEnvironment,
    pub diagnostics: &'input mut DiagnosticStore,
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = phase.into_parts().0 else {
        panic!("phase not performed")
    };
    value
}

pub(super) fn trusted_contracts() -> (ExternalNameEnvironment, TrustedSignatureEnvironment) {
    let module = ModuleAddress::new(["std".into(), "probe".into()]).unwrap();
    let id = |name: &str| ExternalDefId::new(name).unwrap();
    let mut declarations = Vec::new();
    let mut signatures = Vec::new();
    for name in ["Alpha", "Zulu"] {
        let target = id(&format!("std::probe::{name}"));
        declarations.push(
            ExternalDeclaration::new(
                target.clone(),
                module.clone(),
                NameKey::new(name, false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
        );
        signatures.push(TrustedDeclarationSignature::new(
            target,
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        ));
    }
    let pick = id("std::probe::pick");
    declarations.push(
        ExternalDeclaration::new(
            pick.clone(),
            module.clone(),
            NameKey::new("pick", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    );
    signatures.push(TrustedDeclarationSignature::new(
        pick,
        TrustedSignatureKind::Callable {
            bang: false,
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "T".into(),
                bounds: ["std::probe::Zulu", "std::probe::Alpha"]
                    .map(|name| TrustedTraitExpr {
                        declaration: id(name),
                        arguments: Box::new([]),
                    })
                    .into(),
            }]),
            parameters: Box::new([TrustedCallableParameter {
                name: Some("value".into()),
                ty: TrustedTypeExpr::GenericParameter(0),
            }]),
            result: TrustedTypeExpr::GenericParameter(0),
            requirements: Box::new([]),
        },
    ));
    (
        ExternalNameEnvironment::new([module], declarations, []).unwrap(),
        TrustedSignatureEnvironment::new(signatures).unwrap(),
    )
}

pub(super) fn with_fixture(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
    inspect: impl FnOnce(Fixture<'_, '_>),
) {
    let file = FileId::new(0);
    let raw = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = raw.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parsed = eglur_syntax::parse_validated(validated);
    assert!(parsed.diagnostics().is_empty());
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    let label = SourceLabel::new("main.eglur").unwrap();
    let source = SemanticSource::new(
        file,
        crate::ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        hir.origins().map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        }),
        [],
        None,
    )
    .unwrap();
    let sources = [source];
    let mut diagnostics = DiagnosticStore::new();
    let collection = performed(crate::collect_declarations(&sources, &Support).unwrap());
    let plan = performed(
        crate::plan_imports(
            &collection,
            &sources,
            &Support,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let unit = crate::ImportAnalysisUnit::new([file]).unwrap();
    let imports = performed(
        crate::resolve_imports(
            &collection,
            &plan,
            &[unit],
            &sources,
            &Support,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let names = performed(
        crate::resolve_names(
            &collection,
            &imports,
            &sources,
            &Support,
            Some(external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut declarations = performed(
        crate::resolve_declaration_signatures(
            &collection,
            &imports,
            &sources,
            &Support,
            crate::SignatureResolutionInputs::new(Some(external), trusted),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut trait_environment = performed(
        crate::build_trait_environment(
            &collection,
            &sources,
            &Support,
            declarations.signatures_mut(),
            &mut diagnostics,
        )
        .unwrap(),
    );
    inspect(Fixture {
        collection: &collection,
        imports: &imports,
        names: &names,
        sources: &sources,
        declarations: &mut declarations,
        trait_environment: &mut trait_environment,
        diagnostics: &mut diagnostics,
    });
}
