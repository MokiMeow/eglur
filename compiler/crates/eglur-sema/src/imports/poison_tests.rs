use std::cell::Cell;

use super::*;
use crate::type_namespace::{TypeNameLookup, lookup_source_module_type};
use crate::{ResolvedName, collect_declarations, resolve_names};
use eglur_diagnostics::{DiagnosticFailure, PoisonAccumulator, SourceLabel, SourceSpan};
use eglur_hir::{HirFile, OriginId, lower};
use eglur_source::{LineIndex, SourceFile};
use eglur_syntax::parse_validated;

mod states;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}

thread_local! {
    static WORK: Cell<[Work; 2]> = const { Cell::new([Work { inputs: 0, completed: 0 }; 2]) };
}

pub(crate) fn input(stage: usize) {
    WORK.update(|mut work| {
        work[stage].inputs += 1;
        work
    });
}

pub(crate) fn complete(stage: usize) {
    WORK.update(|mut work| {
        work[stage].completed += 1;
        work
    });
}

struct Support(bool);

impl CompileSupport for Support {
    fn supports(&self, capability: CompileCapability) -> bool {
        self.0 || capability != CompileCapability::Resolve(ResolveCapability::ResolveModuleValues)
    }
}

struct PreparedSource {
    file: FileId,
    module: ModuleAddress,
    hir: HirFile,
    spans: Vec<(OriginId, SourceSpan)>,
}

fn with_imports(
    texts: &[(&str, String)],
    supported: bool,
    inspect: impl FnOnce(
        &DeclarationCollection,
        &ResolvedImports,
        &[SemanticSource<'_>],
        &Support,
        &ExternalNameEnvironment,
        &mut DiagnosticStore,
    ),
) {
    let prepared = texts
        .iter()
        .enumerate()
        .map(|(index, (name, text))| {
            let file = FileId::new(u32::try_from(index).unwrap());
            let input = SourceFile::new(file, text.as_bytes().to_vec());
            let validated = input.validate().unwrap();
            let lines = LineIndex::new(validated);
            let parse = parse_validated(validated);
            assert!(parse.diagnostics().is_empty());
            let hir = lower(&parse);
            assert!(hir.invalid_occurrences().is_empty());
            let label = SourceLabel::new(format!("{name}.eglur")).unwrap();
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
                module: ModuleAddress::new(name.split('/').map(Box::<str>::from)).unwrap(),
                hir,
                spans,
            }
        })
        .collect::<Vec<_>>();
    let sources = prepared
        .iter()
        .map(|source| {
            SemanticSource::new(
                source.file,
                source.module.clone(),
                Some(&source.hir),
                source.spans.clone(),
                [],
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let support = Support(supported);
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let mut diagnostics = DiagnosticStore::new();
    let PhaseProduct::Performed(collection) = collect_declarations(&sources, &support)
        .unwrap()
        .into_parts()
        .0
    else {
        panic!("declarations unavailable")
    };
    let PhaseProduct::Performed(plan) = plan_imports(
        &collection,
        &sources,
        &support,
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("import plan unavailable")
    };
    let units = sources
        .iter()
        .map(|source| ImportAnalysisUnit::new([source.file()]).unwrap())
        .collect::<Vec<_>>();
    let PhaseProduct::Performed(imports) = resolve_imports(
        &collection,
        &plan,
        &units,
        &sources,
        &support,
        Some(&external),
        &mut diagnostics,
    )
    .unwrap()
    .into_parts()
    .0
    else {
        panic!("imports unavailable")
    };
    inspect(
        &collection,
        &imports,
        &sources,
        &support,
        &external,
        &mut diagnostics,
    );
}

#[test]
fn lookup_folds_complete_only_the_final_import_union() {
    for count in [0usize, 1, 8, 32, 128] {
        for mode in ["distinct", "mixed", "healthy", "unsupported"] {
            let imports = (0..count)
                .map(|index| {
                    let symbol = if mode == "healthy" || (mode == "mixed" && index % 3 == 0) {
                        "Existing".to_owned()
                    } else {
                        format!("missing_{index}")
                    };
                    format!("import provider::{symbol} as Wanted\n")
                })
                .collect::<String>();
            let source = format!(
                "{imports}fn probe(value: Wanted) -> Wanted = Wanted\nfn after(x: i64) -> i64 = x\n"
            );
            for cross_module in [false, true] {
                with_imports(
                    &[
                        ("provider", "pub struct Existing {}\n".to_owned()),
                        ("main", source.clone()),
                    ],
                    mode != "unsupported",
                    |collection, imports, sources, support, external, diagnostics| {
                        WORK.set([Work::default(); 2]);
                        let name = NameKey::new("Wanted", false).unwrap();
                        let module = collection.module_for_file(FileId::new(1)).unwrap();
                        let ty = lookup_source_module_type(
                            collection,
                            imports,
                            collection.module(module).unwrap(),
                            &name,
                            cross_module,
                        )
                        .unwrap();
                        let PhaseProduct::Performed(names) = resolve_names(
                            collection,
                            imports,
                            sources,
                            support,
                            Some(external),
                            diagnostics,
                        )
                        .unwrap()
                        .into_parts()
                        .0
                        else {
                            panic!("names unavailable")
                        };
                        let inputs = match mode {
                            "distinct" => count,
                            "mixed" => count - count.div_ceil(3),
                            _ => 0,
                        };
                        let expected = Work {
                            inputs,
                            completed: usize::from(inputs != 0),
                        };
                        assert_eq!(WORK.get(), [expected; 2], "{mode}/{count}/{cross_module}");
                        let local = names
                            .locals()
                            .iter()
                            .find(|binding| binding.name().text() == "x")
                            .unwrap();
                        assert!(names.facts().any(|(_, fact)| matches!(fact, SemanticFact::Known(ResolvedName::Local(id)) if *id == local.id())));

                        if inputs != 0 {
                            let TypeNameLookup::Poisoned(poison) = ty else {
                                panic!("import poison wins")
                            };
                            let mut all = PoisonAccumulator::new();
                            for &import in imports.lookup(module, Namespace::Type, &name) {
                                if let FactState::Poisoned(root) = imports.fact(import) {
                                    all.include(root).unwrap();
                                }
                            }
                            diagnostics
                                .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                                .unwrap();
                            diagnostics
                                .select_preceding_roots(&poison, &all.finish().unwrap())
                                .unwrap();
                            let snapshot = std::mem::replace(diagnostics, DiagnosticStore::new())
                                .finalize(None);
                            assert_eq!(
                                snapshot
                                    .diagnostics()
                                    .iter()
                                    .map(|d| d.code().as_str())
                                    .collect::<Vec<_>>(),
                                vec!["IMPORT_NOT_FOUND"; inputs]
                            );
                        } else if count == 0 {
                            assert!(matches!(ty, TypeNameLookup::NotFound));
                        } else if mode == "unsupported" {
                            assert!(matches!(ty, TypeNameLookup::Unperformed));
                        } else if cross_module {
                            assert!(matches!(ty, TypeNameLookup::VisibilityViolation));
                        } else if count == 1 {
                            assert!(matches!(
                                ty,
                                TypeNameLookup::Unique(CanonicalTarget::Source(_))
                            ));
                        } else {
                            let TypeNameLookup::Ambiguous(targets) = ty else {
                                panic!("ambiguous type expected")
                            };
                            assert_eq!(targets.len(), count);
                        }
                    },
                );
            }
        }
    }
}
