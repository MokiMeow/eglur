use super::*;
use crate::{
    CompileCapability, CompileSupport, DeclarationCollection, ImportAnalysisUnit, ModuleAddress,
    PhaseProduct, SemanticSource, SignatureResolutionInputs, collect_declarations, plan_imports,
    resolve_declaration_signatures, resolve_imports,
};
use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::lower;
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

struct AllCapabilities;

impl CompileSupport for AllCapabilities {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

pub(super) struct Fixture {
    pub(super) declarations: DeclarationCollection,
    pub(super) signatures: DeclarationSignatures,
    pub(super) environment: TraitEnvironment,
    pub(super) diagnostics: DiagnosticStore,
    pub(super) span: SourceSpan,
}

impl Fixture {
    pub(super) fn declaration(&self, name: &str) -> DefId {
        self.declarations
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|candidate| candidate.spelling() == name)
            })
            .unwrap()
            .id()
    }
}

pub(super) fn fixture_from_source(text: &str) -> Fixture {
    let file = FileId::new(0);
    let input = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parse = parse_validated(validated);
    assert!(parse.diagnostics().is_empty());
    let hir = lower(&parse);
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
    let span = spans[0].1.clone();
    let sources = [SemanticSource::new(
        file,
        ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        spans,
        [],
        None,
    )
    .unwrap()];
    let mut diagnostics = DiagnosticStore::new();
    let PhaseProduct::Performed(declarations) = collect_declarations(&sources, &AllCapabilities)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declarations unavailable")
    };
    let PhaseProduct::Performed(plan) = plan_imports(
        &declarations,
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import plan unavailable")
    };
    let PhaseProduct::Performed(imports) = resolve_imports(
        &declarations,
        &plan,
        &[ImportAnalysisUnit::new([file]).unwrap()],
        &sources,
        &AllCapabilities,
        None,
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("imports unavailable")
    };
    let PhaseProduct::Performed(resolved) = resolve_declaration_signatures(
        &declarations,
        &imports,
        &sources,
        &AllCapabilities,
        SignatureResolutionInputs::new(None, None),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("signatures unavailable")
    };
    let (mut signatures, _) = resolved.into_parts();
    let PhaseProduct::Performed(environment) = crate::build_trait_environment(
        &declarations,
        &sources,
        &AllCapabilities,
        &mut signatures,
        &mut diagnostics,
    )
    .unwrap_or_else(|error| panic!("trait environment failed for {text}: {error}"))
    .into_parts()
    .0
    else {
        panic!("trait environment unavailable")
    };
    Fixture {
        declarations,
        signatures,
        environment,
        diagnostics,
        span,
    }
}
