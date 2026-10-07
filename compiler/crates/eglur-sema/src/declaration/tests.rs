use std::cell::Cell;

use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
use eglur_hir::{HirFile, lower};
use eglur_source::{LineIndex, SourceFile};
use eglur_syntax::parse_validated;

use super::*;
use crate::{
    DeclarationSignatures, ExternalNameEnvironment, ImportAnalysisUnit, SignatureResolutionInputs,
    StableDeclarationKeys, TypecheckCapability, plan_imports, resolve_declaration_signatures,
    resolve_imports,
};

mod fingerprints;
mod resolver_children;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RoleWork {
    insertions: usize,
    queries: usize,
    generic_index_insertions: usize,
    generic_index_queries: usize,
}

thread_local! {
    static ROLE_WORK: Cell<RoleWork> = const { Cell::new(RoleWork {
        insertions: 0, queries: 0, generic_index_insertions: 0, generic_index_queries: 0,
    }) };
}

pub(super) fn observe_role_insertion() {
    ROLE_WORK.update(|mut work| {
        work.insertions += 1;
        work
    });
}

pub(super) fn observe_role_query() {
    ROLE_WORK.update(|mut work| {
        work.queries += 1;
        work
    });
}

pub(super) fn observe_generic_index_insertion() {
    ROLE_WORK.update(|mut work| {
        work.generic_index_insertions += 1;
        work
    });
}

pub(super) fn observe_generic_index_query() {
    ROLE_WORK.update(|mut work| {
        work.generic_index_queries += 1;
        work
    });
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ChildWork {
    insertions: usize,
    queries: usize,
    visits: usize,
}

thread_local! {
    static DECLARATION_VISITS: Cell<usize> = const { Cell::new(0) };
    static CHILD_WORK: Cell<ChildWork> = const { Cell::new(ChildWork {
        insertions: 0, queries: 0, visits: 0,
    }) };
}

pub(super) fn observe_declaration_visit() {
    DECLARATION_VISITS.set(DECLARATION_VISITS.get() + 1);
}

pub(super) fn observe_child_insertion() {
    let mut work = CHILD_WORK.get();
    work.insertions += 1;
    CHILD_WORK.set(work);
}

pub(super) fn observe_child_query() {
    let mut work = CHILD_WORK.get();
    work.queries += 1;
    CHILD_WORK.set(work);
}

pub(super) fn observe_child_visit() {
    let mut work = CHILD_WORK.get();
    work.visits += 1;
    CHILD_WORK.set(work);
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

struct PreparedSource {
    file: FileId,
    module: ModuleAddress,
    hir: HirFile,
    spans: Vec<(OriginId, SourceSpan)>,
}

fn with_sources<R>(texts: &[(&str, &str)], inspect: impl FnOnce(&[SemanticSource<'_>]) -> R) -> R {
    let prepared = texts
        .iter()
        .enumerate()
        .map(|(index, &(name, text))| {
            let file = FileId::new(u32::try_from(index).unwrap());
            let source = SourceFile::new(file, text.as_bytes().to_vec());
            let validated = source.validate().unwrap();
            let lines = LineIndex::new(validated);
            let parsed = parse_validated(validated);
            assert!(
                parsed.diagnostics().is_empty(),
                "{text}: {:?}",
                parsed.diagnostics()
            );
            let hir = lower(&parsed);
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
                module: ModuleAddress::new([Box::<str>::from(name)]).unwrap(),
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

    inspect(&sources)
}

fn performed<T>(analysis: PhaseAnalysis<T>) -> T {
    let PhaseProduct::Performed(value) = analysis.into_parts().0 else {
        panic!("required phase was not performed");
    };
    value
}

fn collect(texts: &[(&str, &str)]) -> DeclarationCollection {
    with_sources(texts, |sources| {
        performed(collect_declarations(sources, &Support).unwrap())
    })
}

fn headers(text: &str) -> (DeclarationCollection, DeclarationSignatures) {
    with_sources(&[("main", text)], |sources| {
        let declarations = performed(collect_declarations(sources, &Support).unwrap());
        let external = ExternalNameEnvironment::new([], [], []).unwrap();
        let mut diagnostics = DiagnosticStore::new();
        let plan = performed(
            plan_imports(
                &declarations,
                sources,
                &Support,
                Some(&external),
                &mut diagnostics,
            )
            .unwrap(),
        );
        let unit = ImportAnalysisUnit::new([sources[0].file()]).unwrap();
        let imports = performed(
            resolve_imports(
                &declarations,
                &plan,
                &[unit],
                sources,
                &Support,
                Some(&external),
                &mut diagnostics,
            )
            .unwrap(),
        );
        let signatures = performed(
            resolve_declaration_signatures(
                &declarations,
                &imports,
                sources,
                &Support,
                SignatureResolutionInputs::new(Some(&external), None),
                &mut diagnostics,
            )
            .unwrap(),
        )
        .into_parts()
        .0;
        assert!(diagnostics.finalize(None).diagnostics().is_empty());

        (declarations, signatures)
    })
}

#[test]
fn direct_groups_match_collected_role_name_and_owner_in_allocation_order() {
    let seed = "fn seed(value: Bool) -> Bool = value\n";
    let mut collection = collect(&[("first", seed), ("second", seed)]);
    let kinds = [
        DeclarationKind::Function,
        DeclarationKind::ExternFunction,
        DeclarationKind::IntrinsicConstructor,
        DeclarationKind::Trait,
        DeclarationKind::Impl,
        DeclarationKind::Struct,
        DeclarationKind::Enum,
        DeclarationKind::OpaqueType,
        DeclarationKind::EnumVariant,
        DeclarationKind::EnumVariantField,
        DeclarationKind::TypeAlias,
        DeclarationKind::Const,
        DeclarationKind::AssociatedType,
        DeclarationKind::AssociatedTypeBinding,
        DeclarationKind::StorageField,
        DeclarationKind::Property,
        DeclarationKind::GenericParameter,
    ];
    let names = [
        Some(NameKey::new("same", false).unwrap()),
        Some(NameKey::new("same", true).unwrap()),
        Some(NameKey::new("different", false).unwrap()),
        None,
    ];

    // This is a collected inventory, not a claim that conflicting headers type-check.
    for module in &mut collection.modules {
        let origin = module.declarations[0].origin();
        let mut owners = Vec::new();
        for name in ["Left", "Right"] {
            owners.push(
                allocate_declaration(
                    module,
                    None,
                    Some(NameKey::new(name, false).unwrap()),
                    DeclarationKind::Struct,
                    DeclarationVisibility::Public,
                    origin,
                    None,
                )
                .unwrap(),
            );
        }
        for parent in [None, Some(owners[0]), Some(owners[1])] {
            for kind in kinds {
                for name in &names {
                    for visibility in [
                        DeclarationVisibility::Private,
                        DeclarationVisibility::Public,
                    ] {
                        allocate_declaration(
                            module,
                            parent,
                            name.clone(),
                            kind,
                            visibility,
                            origin,
                            None,
                        )
                        .unwrap();
                    }
                }
            }
        }
    }

    for module in collection.modules() {
        for declaration in module.declarations() {
            let children = module
                .declarations()
                .filter(|child| child.parent() == Some(declaration.id()))
                .map(Declaration::id)
                .collect::<Vec<_>>();
            assert_eq!(
                module
                    .children(declaration.id())
                    .map(Declaration::id)
                    .collect::<Vec<_>>(),
                children
            );
            for kind in kinds {
                let expected = module
                    .declarations()
                    .filter(|candidate| {
                        candidate.parent() == Some(declaration.id()) && candidate.kind() == kind
                    })
                    .map(Declaration::id)
                    .collect::<Vec<_>>();
                assert_eq!(module.child_ids(declaration.id(), kind), expected);
            }

            let expected = module
                .declarations()
                .filter(|candidate| {
                    candidate.parent() == declaration.parent()
                        && candidate.kind() == declaration.kind()
                        && candidate.name() == declaration.name()
                })
                .map(Declaration::id)
                .collect::<Vec<_>>();
            assert_eq!(
                module.direct_candidates(
                    declaration.parent(),
                    declaration.name(),
                    declaration.kind(),
                ),
                expected
            );
            if let (Some(parent), Some(name)) = (declaration.parent(), declaration.name()) {
                assert_eq!(
                    collection.child_candidates(parent, name, declaration.kind()),
                    expected
                );
            }
        }
        let same = NameKey::new("same", false).unwrap();
        assert_eq!(module.candidates(Namespace::Value, &same).len(), 8);
        assert_eq!(
            module
                .direct_candidates(None, Some(&same), DeclarationKind::Function)
                .len(),
            2
        );
        let missing = DefId::from_index(module.id(), module.declarations.len()).unwrap();
        assert_eq!(module.children(missing).len(), 0);
        assert!(
            collection
                .child_candidates(missing, &same, DeclarationKind::Function)
                .is_empty()
        );
        let other = DefId::from_index(ModuleId::from_index(9).unwrap(), 0).unwrap();
        assert_eq!(module.children(other).len(), 0);
        assert!(
            module
                .direct_candidates(Some(other), Some(&same), DeclarationKind::Function)
                .is_empty()
        );
        assert!(
            collection
                .child_candidates(other, &same, DeclarationKind::Function)
                .is_empty()
        );
        assert!(
            module
                .direct_candidates(
                    None,
                    Some(&NameKey::new("absent", false).unwrap()),
                    DeclarationKind::Function
                )
                .is_empty()
        );
    }
}

#[test]
fn child_queries_do_not_rescan_wide_modules() {
    for count in [8, 32, 128] {
        let methods = (0..count)
            .map(|index| format!("fn method_{index:03}(self: &Self) -> Bool = true\n"))
            .collect::<String>();
        let source = format!("struct Host {{\n{methods}}}\n");
        let collection = collect(&[("main", &source)]);
        let module = collection.modules().next().unwrap();
        let owner = module.declarations().next().unwrap().id();
        let methods = module.declarations().skip(1).collect::<Vec<_>>();
        assert_eq!(methods.len(), count);

        DECLARATION_VISITS.set(0);
        for method in methods {
            assert_eq!(
                collection.child_candidates(
                    owner,
                    method.name().unwrap(),
                    DeclarationKind::Function
                ),
                [method.id()]
            );
        }
        assert_eq!(DECLARATION_VISITS.get(), 0, "methods={count}");
    }
}

#[test]
fn all_child_queries_visit_only_stored_edges() {
    for count in [8, 32, 128] {
        let methods = (0..count)
            .map(|index| {
                format!("pub fn method_{index:03}[T](self: &Self, value: T) -> T = value\n")
            })
            .collect::<String>();
        let source = format!("pub struct Host[A] {{\n{methods}}}\n");
        CHILD_WORK.set(ChildWork::default());
        let collection = collect(&[("first", &source), ("second", &source)]);
        let prepared = CHILD_WORK.get();
        let expected_edges = 2 * (2 * count + 1);
        assert_eq!(prepared.insertions, expected_edges);
        assert_eq!(prepared.queries, 0);
        assert_eq!(prepared.visits, 0);

        let stored = collection
            .modules()
            .map(|module| {
                module
                    .children_by_parent
                    .values()
                    .map(Vec::len)
                    .sum::<usize>()
            })
            .sum::<usize>();
        assert_eq!(stored, expected_edges);
        let declarations = collection
            .modules()
            .flat_map(|module| {
                module
                    .declarations()
                    .map(move |declaration| (module, declaration))
            })
            .collect::<Vec<_>>();
        DECLARATION_VISITS.set(0);
        for (module, declaration) in &declarations {
            let children = module.children(declaration.id()).collect::<Vec<_>>();
            assert!(
                children
                    .iter()
                    .all(|child| child.parent() == Some(declaration.id()))
            );
            assert!(children.windows(2).all(|pair| pair[0].id() < pair[1].id()));
        }
        assert_eq!(DECLARATION_VISITS.get(), 0);
        assert_eq!(
            CHILD_WORK.get(),
            ChildWork {
                insertions: expected_edges,
                queries: declarations.len(),
                visits: expected_edges,
            }
        );

        let first = collection.modules().next().unwrap();
        let second = collection.modules().nth(1).unwrap();
        let foreign = second.declarations().next().unwrap().id();
        assert_eq!(first.children(foreign).len(), 0);
    }
}

#[test]
fn stable_keys_visit_wide_function_modules_once() {
    for public in [false, true] {
        for count in [8, 32, 128] {
            let visibility = if public { "pub " } else { "" };
            let source = (0..count)
                .map(|index| {
                    format!("{visibility}fn plain_{index:03}(value: Bool) -> Bool = value\n")
                })
                .collect::<String>();
            let (collection, signatures) = headers(&source);
            let declarations = collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .collect::<Vec<_>>();
            assert_eq!(declarations.len(), count);

            DECLARATION_VISITS.set(0);
            let keys = StableDeclarationKeys::derive(&collection, &signatures).unwrap();
            assert_eq!(
                DECLARATION_VISITS.get(),
                count,
                "functions={count}, public={public}"
            );
            for declaration in declarations {
                assert!(keys.source(declaration.id()).is_some());
            }
        }
    }
}
