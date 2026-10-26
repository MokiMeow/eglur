use super::super::Resolver;
use crate::{
    ExternalNameEnvironment, FactTable, ImportAnalysisUnit, SemanticSource, collect_declarations,
    plan_imports, resolve_imports,
};
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStore, PoisonAccumulator, PrimarySpan, Severity, SourceLabel,
    SourceSpan,
};
use eglur_hir::lower;
use eglur_source::{FileId, LineIndex, SourceFile};
use eglur_syntax::parse_validated;

pub(in crate::resolve) fn with_resolver(
    text: &str,
    support: &dyn crate::CompileSupport,
    inspect: impl FnOnce(&mut Resolver<'_, '_>, &SemanticSource<'_>),
) {
    let file = FileId::new(0);
    let input = SourceFile::new(file, text.as_bytes().to_vec());
    let validated = input.validate().unwrap();
    let lines = LineIndex::new(validated);
    let parse = parse_validated(validated);
    let hir = lower(&parse);
    let label = SourceLabel::new("main.eglur").unwrap();
    let mut diagnostics = DiagnosticStore::new();
    let syntax_poisons = parse
        .diagnostics()
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new(format!("SYNTAX_{index}")).unwrap(),
                Severity::Error,
                DiagnosticPhase::new("parse").unwrap(),
                "recovered syntax root",
                PrimarySpan::Unavailable,
            );
            diagnostics
                .record_failure(DiagnosticFailure::Root(
                    DiagnosticRoot::boundary(
                        BoundaryRootKey::new(format!("syntax-{index}")).unwrap(),
                        diagnostic,
                    )
                    .unwrap(),
                ))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let invalid_poisons = hir
        .invalid_occurrences()
        .iter()
        .map(|occurrence| {
            let mut pending = PoisonAccumulator::new();
            for cause in occurrence.syntax_causes() {
                pending
                    .include(&syntax_poisons[cause.get() as usize])
                    .unwrap();
            }
            (
                occurrence.origin(),
                pending
                    .finish()
                    .expect("parser invalid occurrence needs an actual syntax cause"),
            )
        })
        .collect::<Vec<_>>();
    let sources = [SemanticSource::new(
        file,
        crate::ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
        Some(&hir),
        hir.origins().map(|(origin, value)| {
            (
                origin,
                SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
            )
        }),
        invalid_poisons,
        None,
    )
    .unwrap()];
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let crate::PhaseProduct::Performed(collection) = collect_declarations(&sources, support)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declarations unavailable")
    };
    let crate::PhaseProduct::Performed(plan) = plan_imports(
        &collection,
        &sources,
        support,
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import plan unavailable")
    };
    let crate::PhaseProduct::Performed(imports) = resolve_imports(
        &collection,
        &plan,
        &[ImportAnalysisUnit::new([file]).unwrap()],
        &sources,
        support,
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("imports unavailable")
    };
    let mut resolver = Resolver {
        collection: &collection,
        imports: &imports,
        external: Some(&external),
        diagnostics: &mut diagnostics,
        recorder: crate::DemandRecorder::new(support),
        facts: FactTable::new(),
        deferred: Default::default(),
        scopes: Vec::new(),
        locals: Vec::new(),
        local_by_origin: Default::default(),
        pattern_bindings: Default::default(),
        active_scopes: Vec::new(),
        current_owner: None,
        next_scope: Default::default(),
        next_local: Default::default(),
    };
    inspect(&mut resolver, &sources[0]);
}
