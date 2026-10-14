use super::*;

pub(super) fn with_file_set(
    texts: &[(&str, &str)],
    inspect: impl for<'input, 'hir> FnOnce(
        &mut SignatureResolver<'input, 'hir>,
        &'input [SemanticSource<'hir>],
    ),
) {
    use eglur_diagnostics::SourceLabel;
    use eglur_source::{LineIndex, SourceFile};
    let prepared = texts
        .iter()
        .enumerate()
        .map(|(index, (module, text))| {
            let file = FileId::new(u32::try_from(index).unwrap());
            let raw = SourceFile::new(file, text.as_bytes().to_vec());
            let text = raw.validate().unwrap();
            let parsed = eglur_syntax::parse_validated(text);
            assert!(
                parsed.diagnostics().is_empty(),
                "{:?}",
                parsed.diagnostics()
            );
            let hir = eglur_hir::lower(&parsed);
            assert!(hir.invalid_occurrences().is_empty());
            let label = SourceLabel::new(format!("{module}.eglur")).unwrap();
            let lines = LineIndex::new(text);
            let spans = hir
                .origins()
                .map(|(id, origin)| {
                    (
                        id,
                        SourceSpan::from_origin(label.clone(), origin, &lines).unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            (hir, spans)
        })
        .collect::<Vec<_>>();
    let sources = prepared
        .iter()
        .zip(texts)
        .map(|((hir, spans), (module, _))| {
            SemanticSource::new(
                hir.file(),
                ModuleAddress::new([Box::<str>::from(*module)]).unwrap(),
                Some(hir),
                spans.clone(),
                [],
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    fn performed<T>(phase: PhaseAnalysis<T>) -> T {
        let PhaseProduct::Performed(value) = phase.into_parts().0 else {
            panic!("unperformed file set")
        };
        value
    }
    let collection = performed(crate::collect_declarations(&sources, &Support).unwrap());
    let mut diagnostics = DiagnosticStore::new();
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let plan = performed(
        crate::plan_imports(
            &collection,
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let unit = crate::ImportAnalysisUnit::new(sources.iter().map(SemanticSource::file)).unwrap();
    let imports = performed(
        crate::resolve_imports(
            &collection,
            &plan,
            &[unit],
            &sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    assert!(!diagnostics.has_errors());
    let mut resolver = SignatureResolver::new(
        &collection,
        &imports,
        sources
            .iter()
            .map(|source| (source.file(), source))
            .collect(),
        None,
        Some(&external),
        None,
        &mut diagnostics,
    )
    .unwrap();
    inspect(&mut resolver, &sources);
}
