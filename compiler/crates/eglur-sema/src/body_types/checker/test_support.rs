use super::*;
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticPhase, DiagnosticRoot, PrimarySpan,
    Severity, SourceLabel,
};
use eglur_source::{LineIndex, SourceFile};

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = phase.into_parts().0 else {
        panic!("phase not performed")
    };
    value
}

pub(super) fn with_checker(text: &str, inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>)) {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_checker_inputs(text, &external, None, inspect);
}

pub(super) fn with_checker_inputs(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<&crate::TrustedSignatureEnvironment>,
    inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>),
) {
    with_checker_declarations(
        text,
        external,
        trusted.map(|signatures| crate::TrustedDeclarationInputs::new(signatures, None)),
        inspect,
    );
}

pub(super) fn with_checker_declarations(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
    inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>),
) {
    with_checker_work(text, external, trusted, None, inspect);
}

pub(super) fn with_checker_work(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
    analysis_work: Option<&mut crate::AnalysisWorkAccounting>,
    inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>),
) {
    with_checker_owned(text, external, trusted, analysis_work, |mut checker| {
        inspect(&mut checker);
    });
}

pub(super) fn with_checker_owned(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
    analysis_work: Option<&mut crate::AnalysisWorkAccounting>,
    inspect: impl FnOnce(BodyChecker<'_, '_, '_>),
) {
    with_phase_inputs(text, external, trusted, |inputs| {
        let (signatures, inhabitance) = inputs.declarations.parts_mut();
        let checker = BodyChecker {
            collection: inputs.collection,
            imports: inputs.imports,
            names: inputs.names,
            external_names: Some(external),
            sources: inputs
                .sources
                .iter()
                .map(|source| (source.file(), source))
                .collect(),
            signatures,
            inhabitance,
            trait_environment: inputs.trait_environment,
            diagnostics: inputs.diagnostics,
            recorder: DemandRecorder::new(&Support),
            expressions: FactTable::new(),
            const_eligibility: BTreeMap::new(),
            preparing_numeric_tree: false,
            evaluate_constant_declarations: true,
            locals: FactTable::new(),
            members: FactTable::new(),
            invocations: FactTable::new(),
            callable_values: FactTable::new(),
            reduce_by_commitments: FactTable::new(),
            regions: FactTable::new(),
            checked_numeric: FactTable::new(),
            ownership_operations: FactTable::new(),
            bodies: FactTable::new(),
            transfers: Vec::new(),
            callable_instances: Vec::new(),
            callable_instance_ids: BTreeMap::new(),
            obligations: Vec::new(),
            method_dispatches: Vec::new(),
            region_stack: Vec::new(),
            block_region_overrides: Vec::new(),
            result_contexts: Vec::new(),
            question_floors: vec![0],
            checked_contexts: Vec::new(),
            control_path_barriers: Vec::new(),
            current_return: None,
            current_owner: None,
            analysis_work,
            internal_failures: Vec::new(),
        };
        inspect(checker);
    });
}

pub(super) struct BodyPhaseInputs<'input, 'hir> {
    pub collection: &'input DeclarationCollection,
    pub imports: &'input ResolvedImports,
    pub names: &'input NameResolution,
    pub sources: &'input [SemanticSource<'hir>],
    pub declarations: &'input mut ResolvedDeclarationSemantics,
    pub trait_environment: &'input mut TraitEnvironment,
    pub diagnostics: &'input mut DiagnosticStore,
}

pub(super) fn with_phase_inputs(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
    inspect: impl FnOnce(BodyPhaseInputs<'_, '_>),
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
    inspect(BodyPhaseInputs {
        collection: &collection,
        imports: &imports,
        names: &names,
        sources: &sources,
        declarations: &mut declarations,
        trait_environment: &mut trait_environment,
        diagnostics: &mut diagnostics,
    });
}

pub(super) fn root(store: &mut DiagnosticStore, i: usize) -> Poison {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new(format!("ROOT_{i:03}")).unwrap(),
        Severity::Error,
        DiagnosticPhase::new("type").unwrap(),
        "independent failure",
        PrimarySpan::Unavailable,
    );
    let root = DiagnosticRoot::boundary(
        BoundaryRootKey::new(format!("root-{i}")).unwrap(),
        diagnostic,
    )
    .unwrap();
    store
        .record_failure(eglur_diagnostics::DiagnosticFailure::Root(root))
        .unwrap()
}
