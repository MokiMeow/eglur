use super::*;
use eglur_diagnostics::SourceLabel;
use eglur_source::{LineIndex, SourceFile};

mod diagnostic_names;

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn performed<T>(phase: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = phase.into_parts().0 else {
        panic!("required phase not performed")
    };
    value
}

pub(super) fn with_resolver(
    texts: &[&str],
    inspect: impl for<'input, 'hir> FnOnce(
        &mut SignatureResolver<'input, 'hir>,
        &'input [SemanticSource<'hir>],
    ),
) {
    let file = FileId::new(0);
    let prepared = texts
        .iter()
        .map(|text| {
            let source = SourceFile::new(file, text.as_bytes().to_vec());
            let validated = source.validate().unwrap();
            let lines = LineIndex::new(validated);
            let parsed = eglur_syntax::parse_validated(validated);
            assert!(
                parsed.diagnostics().is_empty(),
                "{:?}",
                parsed.diagnostics()
            );
            let hir = eglur_hir::lower(&parsed);
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
            (hir, spans)
        })
        .collect::<Vec<_>>();
    let sources = prepared
        .iter()
        .map(|(hir, spans)| {
            SemanticSource::new(
                file,
                ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
                Some(hir),
                spans.clone(),
                [],
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let collection = performed(crate::collect_declarations(&sources[..1], &Support).unwrap());
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let mut diagnostics = DiagnosticStore::new();
    let plan = performed(
        crate::plan_imports(
            &collection,
            &sources[..1],
            &Support,
            Some(&external),
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
            &sources[..1],
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap(),
    );
    let mut resolver = SignatureResolver::new(
        &collection,
        &imports,
        BTreeMap::from([(file, &sources[0])]),
        None,
        Some(&external),
        None,
        &mut diagnostics,
    )
    .unwrap();
    inspect(&mut resolver, &sources);
}

#[test]
fn generic_hir_recovery_preserves_prerequisites_and_slot_bounds() {
    with_resolver(
        &[
            "fn header[A, B](value: B) -> B = value\n",
            "const header: Bool = true\n",
            "fn header[A](value: A) -> A = value\n",
        ],
        |resolver, sources| {
            let module = resolver.collection.modules().next().unwrap();
            let owner = module.declarations().next().unwrap().id();
            let generics = resolver
                .child_declarations(owner, DeclarationKind::GenericParameter)
                .collect::<Vec<_>>();
            for (index, &generic) in generics.iter().enumerate() {
                let (file, parameter) = resolver.source_generic_parameter(generic).unwrap();
                assert_eq!(file, module.file());
                assert_eq!(
                    parameter.name.text(),
                    Some(if index == 0 { "A" } else { "B" })
                );
            }
            assert_eq!(
                resolver.source_generic_parameter(owner).unwrap_err(),
                SignatureResolutionError::MissingGenericDeclaration(owner)
            );
            resolver.sources.clear();
            assert_eq!(
                resolver.source_generic_parameter(generics[1]).unwrap_err(),
                SignatureResolutionError::MissingHir(module.file())
            );
            resolver.sources.insert(module.file(), &sources[1]);
            assert_eq!(
                resolver.source_generic_parameter(generics[1]).unwrap_err(),
                SignatureResolutionError::WrongDeclarationItem(owner)
            );
            resolver.sources.insert(module.file(), &sources[2]);
            assert_eq!(
                resolver.source_generic_parameter(generics[1]).unwrap_err(),
                SignatureResolutionError::MissingGenericDeclaration(generics[1])
            );
            assert!(resolver.source_generic_parameter(generics[0]).is_ok());
        },
    );
}

#[test]
fn non_generic_children_keep_missing_item_and_missing_generic_errors() {
    with_resolver(
        &["struct Host[A] { field value: A }\nenum Choice[A] { Pair { value: A } }\n"],
        |resolver, _| {
            let module = resolver.collection.modules().next().unwrap();
            let field = module
                .declarations()
                .find(|declaration| declaration.kind() == DeclarationKind::StorageField)
                .unwrap()
                .id();
            assert_eq!(
                resolver.source_generic_parameter(field).unwrap_err(),
                SignatureResolutionError::MissingGenericDeclaration(field)
            );
            let field = module
                .declarations()
                .find(|declaration| declaration.kind() == DeclarationKind::EnumVariantField)
                .unwrap();
            assert_eq!(
                resolver.source_generic_parameter(field.id()).unwrap_err(),
                SignatureResolutionError::MissingDeclarationItem(field.parent().unwrap())
            );
            resolver.sources.clear();
            assert_eq!(
                resolver.source_generic_parameter(field.id()).unwrap_err(),
                SignatureResolutionError::MissingHir(module.file())
            );
        },
    );
}

pub(super) fn with_modules(
    texts: &[(&str, &str)],
    inspect: impl FnOnce(&mut SignatureResolver<'_, '_>),
) {
    crate::declaration::tests::with_sources(texts, |sources| {
        let PhaseProduct::Performed(collection) = crate::collect_declarations(sources, &Support)
            .unwrap()
            .into_parts()
            .0
        else {
            panic!("collection is supported")
        };
        assert_eq!(collection.modules().len(), texts.len());
        let external = ExternalNameEnvironment::new([], [], []).unwrap();
        let mut diagnostics = DiagnosticStore::new();
        let PhaseProduct::Performed(plan) = crate::plan_imports(
            &collection,
            sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0
        else {
            panic!("planning is supported")
        };
        let unit = crate::ImportAnalysisUnit::new(sources.iter().map(|s| s.file())).unwrap();
        let PhaseProduct::Performed(imports) = crate::resolve_imports(
            &collection,
            &plan,
            &[unit],
            sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0
        else {
            panic!("imports are supported")
        };
        let mut resolver = SignatureResolver::new(
            &collection,
            &imports,
            sources.iter().map(|s| (s.file(), s)).collect(),
            None,
            Some(&external),
            None,
            &mut diagnostics,
        )
        .unwrap();
        inspect(&mut resolver);
    });
}
