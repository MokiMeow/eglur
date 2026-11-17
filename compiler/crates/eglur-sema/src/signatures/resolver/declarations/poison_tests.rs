use std::cell::Cell;

use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticSnapshot, PrimarySpan, Severity,
};

use super::super::tests::with_resolver;
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Stage {
    Parameters,
    Mutability,
}

pub(super) enum Step {
    Input(Stage),
    Complete(Stage),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}

thread_local! {
    static WORK: Cell<[Work; 2]> = const { Cell::new([Work { inputs: 0, completed: 0 }; 2]) };
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input(stage) => work[stage as usize].inputs += 1,
            Step::Complete(stage) => work[stage as usize].completed += 1,
        }
        work
    });
}

fn with_callable(
    text: &str,
    inspect: impl for<'input, 'hir> FnOnce(
        &mut SignatureResolver<'input, 'hir>,
        &'input SemanticSource<'hir>,
        DefId,
        &'hir eglur_hir::FunctionDecl,
    ),
) {
    with_resolver(&[text], |resolver, sources| {
        let source = &sources[0];
        let owner = resolver
            .collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.text() == "drive")
            })
            .unwrap()
            .id();
        let item = resolver
            .collection
            .declaration(owner)
            .unwrap()
            .item()
            .unwrap();
        let ItemKind::Function(function) = source.hir().unwrap().item(item).kind() else {
            panic!("drive is a function");
        };
        WORK.set([Work::default(); 2]);
        inspect(resolver, source, owner, function);
    });
}

fn resolve<'hir>(
    resolver: &mut SignatureResolver<'_, 'hir>,
    source: &SemanticSource<'hir>,
    owner: DefId,
    function: &eglur_hir::FunctionDecl,
) -> Resolution<DeclarationSignature> {
    resolver
        .resolve_source_callable(
            owner,
            source,
            source.hir().unwrap(),
            SourceCallable::Function(function),
        )
        .unwrap()
}

fn assert_selected_roots(
    mut store: DiagnosticStore,
    poison: Poison,
    all: &Poison,
    expected: &[(&str, usize)],
) {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(&poison, all).unwrap();
    let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
        panic!("nonempty callable provenance retains its roots");
    };
    let mut actual = BTreeMap::new();
    for diagnostic in snapshot.diagnostics() {
        *actual.entry(diagnostic.code().as_str()).or_insert(0) += 1;
    }
    assert_eq!(actual, expected.iter().copied().collect());
}

fn roots(store: &mut DiagnosticStore, count: usize) -> Vec<Poison> {
    (0..count)
        .map(|index| {
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new(format!("ROOT_{index:03}")).unwrap(),
                Severity::Error,
                DiagnosticPhase::new("type").unwrap(),
                "independent failure",
                PrimarySpan::Unavailable,
            );
            let root = DiagnosticRoot::boundary(
                BoundaryRootKey::new(format!("root-{index}")).unwrap(),
                diagnostic,
            )
            .unwrap();
            store.record_failure(DiagnosticFailure::Root(root)).unwrap()
        })
        .collect()
}

#[test]
fn callable_parameter_and_mutability_folds_complete_once() {
    for count in [8, 32, 128] {
        for mode in ["types", "patterns", "both", "mutable"] {
            let parameters = (0..count)
                .map(|index| match mode {
                    "types" => format!("p{index}: Missing{index}"),
                    "patterns" => "_: i64".to_owned(),
                    "both" => format!("_: Missing{index}"),
                    "mutable" => format!("p{index}: &mut i64"),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
                .join(",");
            let text = format!("fn drive({parameters}, end: Bool) -> Unit = ()\n");
            with_callable(&text, |resolver, source, owner, function| {
                let checkpoint = resolver.diagnostics.checkpoint();
                let span = source
                    .origin_span(resolver.collection.declaration(owner).unwrap().origin())
                    .unwrap();
                let Resolution::Poisoned(poison) = resolve(resolver, source, owner, function)
                else {
                    panic!("invalid callable is poisoned");
                };
                let mut expected = [Work::default(); 2];
                expected[usize::from(mode == "mutable")] = Work {
                    inputs: if mode == "both" { 2 * count } else { count },
                    completed: 1,
                };
                assert_eq!(WORK.get(), expected);
                assert!(resolver.callables.is_empty());
                assert!(matches!(
                    resolver.occurrences.state(&TypeOccurrenceId::new(
                        source.file(),
                        function.parameters[count].ty.unwrap()
                    )),
                    FactState::Known(_)
                ));
                assert!(matches!(
                    resolver.occurrences.state(&TypeOccurrenceId::new(
                        source.file(),
                        function.return_type.unwrap()
                    )),
                    FactState::Known(_)
                ));
                let all = resolver
                    .diagnostics
                    .roots_since_within(&checkpoint, span)
                    .unwrap()
                    .unwrap();
                let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                let codes = match mode {
                    "types" => vec![("NAME_NOT_FOUND", count)],
                    "patterns" => vec![("PARAM_PATTERN_UNSUPPORTED", count)],
                    "both" => vec![
                        ("NAME_NOT_FOUND", count),
                        ("PARAM_PATTERN_UNSUPPORTED", count),
                    ],
                    "mutable" => vec![("NON_BANG_MUT_PARAM_FORBIDDEN", count)],
                    _ => unreachable!(),
                };
                assert_selected_roots(store, poison, &all, &codes);
            });
        }
    }
}

#[test]
fn known_and_bang_callable_parameters_keep_order_without_poison_completion() {
    for text in [
        "fn drive(first: i64, second: Bool, third: Str) -> Unit = ()\n",
        "fn drive!(first: &mut i64, second: Bool, third: Str) -> Unit = ()\n",
        "fn drive() -> Unit = ()\n",
    ] {
        with_callable(text, |resolver, source, owner, function| {
            let Resolution::Known(DeclarationSignature::Callable(id)) =
                resolve(resolver, source, owner, function)
            else {
                panic!("known callable");
            };
            let callable = &resolver.callables[id.index()];
            assert_eq!(callable.is_bang(), function.name.is_bang());
            let names = callable
                .parameters()
                .iter()
                .map(CallableParameter::name)
                .collect::<Vec<_>>();
            let expected = if function.parameters.is_empty() {
                vec![]
            } else {
                vec![Some("first"), Some("second"), Some("third")]
            };
            assert_eq!(names, expected);
            for (parameter, written) in callable.parameters().iter().zip(&function.parameters) {
                assert_eq!(parameter.source_pattern(), Some(written.pattern));
                let FactState::Known(occurrence) = resolver
                    .occurrences
                    .state(&TypeOccurrenceId::new(source.file(), written.ty.unwrap()))
                else {
                    panic!("known parameter type occurrence");
                };
                assert_eq!(occurrence.resolved(), &ResolvedType::Type(parameter.ty()));
            }
            assert_eq!(WORK.get(), [Work::default(); 2]);
        });
    }
}

#[test]
fn callable_partial_aliases_preserve_poison_precedence_and_later_visits() {
    for poisoned in [None, Some("A"), Some("B")] {
        with_callable(
            "type A = i64\ntype B = Bool\nfn drive(a: A, b: B, end: Str) -> Unit = ()\n",
            |resolver, source, owner, function| {
                let inputs = roots(resolver.diagnostics, 1);
                let aliases = resolver
                    .collection
                    .modules()
                    .next()
                    .unwrap()
                    .declarations()
                    .filter_map(|declaration| {
                        declaration
                            .name()
                            .map(|name| (name.text().to_owned(), declaration.id()))
                    })
                    .collect::<BTreeMap<_, _>>();
                for name in ["A", "B"] {
                    let state = if poisoned == Some(name) {
                        AliasState::Poisoned(inputs[0].clone())
                    } else {
                        AliasState::Unperformed
                    };
                    resolver.alias_states.insert(aliases[name], state);
                }
                let result = resolve(resolver, source, owner, function);
                assert!(resolver.callables.is_empty());
                assert!(matches!(
                    resolver.occurrences.state(&TypeOccurrenceId::new(
                        source.file(),
                        function.parameters[2].ty.unwrap()
                    )),
                    FactState::Known(_)
                ));
                assert!(matches!(
                    resolver.occurrences.state(&TypeOccurrenceId::new(
                        source.file(),
                        function.return_type.unwrap()
                    )),
                    FactState::Known(_)
                ));
                if poisoned.is_some() {
                    let Resolution::Poisoned(poison) = result else {
                        panic!("poison wins over partial aliases");
                    };
                    assert_eq!(
                        WORK.get(),
                        [
                            Work {
                                inputs: 1,
                                completed: 1
                            },
                            Work::default()
                        ]
                    );
                    let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
                    assert_selected_roots(store, poison, &inputs[0], &[("ROOT_000", 1)]);
                } else {
                    assert!(matches!(result, Resolution::Unperformed));
                    assert_eq!(WORK.get(), [Work::default(); 2]);
                }
            },
        );
    }
}

#[test]
fn callable_foreign_error_keeps_visited_occurrences_and_stops_before_return() {
    for foreign_first in [false, true] {
        with_callable(
            "type A = i64\ntype B = Bool\ntype C = Str\nfn drive(a: A, b: B, c: C) -> Unit = ()\n",
            |resolver, source, owner, function| {
                let own = roots(resolver.diagnostics, 1);
                let mut foreign_store = DiagnosticStore::new();
                let foreign = roots(&mut foreign_store, 1);
                let aliases = resolver
                    .collection
                    .modules()
                    .next()
                    .unwrap()
                    .declarations()
                    .filter_map(|declaration| {
                        declaration
                            .name()
                            .map(|name| (name.text().to_owned(), declaration.id()))
                    })
                    .collect::<BTreeMap<_, _>>();
                for (name, foreign_input) in [("A", foreign_first), ("B", !foreign_first)] {
                    let poison = if foreign_input { &foreign[0] } else { &own[0] };
                    resolver
                        .alias_states
                        .insert(aliases[name], AliasState::Poisoned(poison.clone()));
                }
                let Err(error) = resolver.resolve_source_callable(
                    owner,
                    source,
                    source.hir().unwrap(),
                    SourceCallable::Function(function),
                ) else {
                    panic!("foreign parameter provenance is rejected");
                };
                assert_eq!(
                    error,
                    SignatureResolutionError::DiagnosticStore(DiagnosticStoreError::ForeignPoison)
                );
                assert_eq!(
                    WORK.get(),
                    [
                        Work {
                            inputs: 2,
                            completed: 0
                        },
                        Work::default()
                    ]
                );
                for (index, foreign_input) in [(0, foreign_first), (1, !foreign_first)] {
                    let FactState::Poisoned(poison) =
                        resolver.occurrences.state(&TypeOccurrenceId::new(
                            source.file(),
                            function.parameters[index].ty.unwrap(),
                        ))
                    else {
                        panic!("visited parameter occurrence remains poisoned");
                    };
                    let store = if foreign_input {
                        &mut foreign_store
                    } else {
                        &mut *resolver.diagnostics
                    };
                    store
                        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                        .unwrap();
                }
                for ty in [
                    function.parameters[2].ty.unwrap(),
                    function.return_type.unwrap(),
                ] {
                    assert!(matches!(
                        resolver
                            .occurrences
                            .state(&TypeOccurrenceId::new(source.file(), ty)),
                        FactState::Unperformed
                    ));
                }
                assert!(resolver.callables.is_empty());
            },
        );
    }
}

#[test]
fn callable_missing_return_skips_mutability_stage() {
    with_callable(
        "fn drive(value: &mut i64) = ()\n",
        |resolver, source, owner, function| {
            let Resolution::Poisoned(_) = resolve(resolver, source, owner, function) else {
                panic!("named return type is required");
            };
            assert_eq!(
                WORK.get(),
                [
                    Work {
                        inputs: 1,
                        completed: 1
                    },
                    Work::default()
                ]
            );
            let store = std::mem::replace(resolver.diagnostics, DiagnosticStore::new());
            let DiagnosticSnapshot::Rejected(snapshot) = store.finalize(None) else {
                panic!("missing return is diagnosed");
            };
            assert_eq!(
                snapshot
                    .diagnostics()
                    .iter()
                    .map(|x| x.code().as_str())
                    .collect::<Vec<_>>(),
                ["FN_RETURN_TYPE_REQUIRED"]
            );
        },
    );
}
