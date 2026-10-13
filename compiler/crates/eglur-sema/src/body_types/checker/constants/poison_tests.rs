use super::*;
use crate::body_types::checker::test_support::{root, with_checker};
use eglur_diagnostics::{DiagnosticFailure, DiagnosticSnapshot};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}
thread_local! { static WORK:Cell<[Work;2]> = const {Cell::new([Work {inputs:0,completed:0};2])}; }
pub(in crate::body_types::checker) enum Step {
    Input(usize),
    Complete(usize),
}
pub(in crate::body_types::checker) fn observe(step: Step) {
    WORK.update(|mut w| {
        match step {
            Step::Input(stage) => w[stage].inputs += 1,
            Step::Complete(stage) => w[stage].completed += 1,
        };
        w
    });
}
fn reset() {
    WORK.set([Work::default(); 2]);
}
fn work(stage: usize) -> Work {
    WORK.get()[stage]
}
fn ids(checker: &BodyChecker<'_, '_, '_>) -> Vec<DefId> {
    checker
        .collection
        .modules()
        .flat_map(|m| m.declarations())
        .filter(|d| d.kind() == DeclarationKind::Const)
        .map(|d| d.id())
        .collect()
}
fn union(inputs: &[Poison], indices: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = eglur_diagnostics::PoisonAccumulator::new();
    for i in indices {
        pending.include(&inputs[i]).unwrap();
    }
    pending.finish().unwrap()
}

fn declaration(checker: &BodyChecker<'_, '_, '_>, name: &str) -> DefId {
    checker
        .collection
        .modules()
        .flat_map(|m| m.declarations())
        .find(|d| d.name().is_some_and(|n| n.text() == name))
        .unwrap()
        .id()
}
fn body_poison(checker: &BodyChecker<'_, '_, '_>, declaration: DefId) -> Poison {
    let FactState::Poisoned(poison) = checker.bodies.state(&declaration) else {
        panic!("poisoned body expected")
    };
    poison.clone()
}
fn assert_selected(
    checker: &mut BodyChecker<'_, '_, '_>,
    poison: &Poison,
    all: &Poison,
    count: usize,
    code: &str,
) {
    checker
        .diagnostics
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    checker
        .diagnostics
        .select_preceding_roots(poison, all)
        .unwrap();
    let store = std::mem::replace(checker.diagnostics, DiagnosticStore::new());
    let snapshot = store.finalize(None);
    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    assert_eq!(snapshot.diagnostics().len(), count);
    assert!(
        snapshot
            .diagnostics()
            .iter()
            .all(|d| d.code().as_str() == code)
    );
}
#[test]
fn known_constant_folds_do_not_complete_provenance() {
    with_checker(
        "const Base: i8 = 0\nconst Sink: i8 = Base\nconst After: i8 = 1\n",
        |checker| {
            reset();
            let declarations = ids(checker);
            checker.check_constant_declarations(&declarations).unwrap();
            assert_eq!(WORK.get(), [Work::default(); 2]);
            for d in declarations {
                assert!(matches!(checker.bodies.state(&d), FactState::Known(_)));
            }
        },
    );
}
#[test]
fn initializer_name_failures_complete_once_before_value_typing() {
    for n in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "reversed", "mixed"] {
            let mut names = (0..n)
                .map(|i| {
                    if mode == "repeated" {
                        "missing".into()
                    } else if mode == "mixed" && i % 2 == 0 {
                        "1".into()
                    } else {
                        format!("missing{i}")
                    }
                })
                .collect::<Vec<String>>();
            if mode == "reversed" {
                names.reverse();
            }
            let text = format!(
                "const Sink: i8 = {}\nconst After: i8 = 1\n",
                names.join(" + ")
            );
            with_checker(&text, |checker| {
                let sink = declaration(checker, "Sink");
                let after = declaration(checker, "After");
                let all = checker
                    .names
                    .facts()
                    .filter_map(|(_, v)| {
                        if let SemanticFact::Poisoned(p) = v {
                            Some(p.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                let expected = if mode == "mixed" { n / 2 } else { n };
                assert_eq!(all.len(), expected);
                reset();
                checker.check_constant_declarations(&ids(checker)).unwrap();
                assert_eq!(
                    work(0),
                    Work {
                        inputs: expected,
                        completed: 1
                    }
                );
                assert_eq!(work(1), Work::default());
                let p = body_poison(checker, sink);
                assert!(matches!(checker.bodies.state(&after), FactState::Known(_)));
                let (_, _, hir, item) = checker.source_item(sink).unwrap();
                let ItemKind::Const(value) = item.kind() else {
                    panic!("const")
                };
                let key = ExpressionKey::new(sink.module(), value.value);
                assert!(matches!(
                    checker.expressions.state(&key),
                    FactState::Unperformed
                ));
                assert!(hir.body(value.body).expression(value.value).is_some());
                let all = union(&all, 0..all.len());
                assert_selected(checker, &p, &all, expected, "NAME_NOT_FOUND");
            });
        }
    }
}
#[test]
fn seeded_dependency_failures_complete_once_and_preserve_later_body() {
    for n in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "overlap", "wide", "reversed"] {
            let mut references = (0..n).map(|i| format!("D{i}")).collect::<Vec<_>>();
            if mode == "reversed" {
                references.reverse();
            }
            let text = format!(
                "{}const Sink: i8 = {}\nconst After: i8 = 1\n",
                (0..n)
                    .map(|i| format!("const D{i}: i8 = 0\n"))
                    .collect::<String>(),
                references.join(" + ")
            );
            with_checker(&text, |checker| {
                let declarations = ids(checker);
                let sink = declarations[n];
                let roots = (0..n)
                    .map(|i| root(checker.diagnostics, i))
                    .collect::<Vec<_>>();
                let all = union(&roots, 0..n);
                let left = union(&roots, 0..n / 2 + 1);
                let right = union(&roots, n / 2..n);
                for (i, &d) in declarations[..n].iter().enumerate() {
                    let poison = match mode {
                        "repeated" => roots[0].clone(),
                        "wide" => all.clone(),
                        "overlap" => {
                            if i % 2 == 0 {
                                left.clone()
                            } else {
                                right.clone()
                            }
                        }
                        _ => roots[i].clone(),
                    };
                    checker
                        .bodies
                        .insert(d, SemanticFact::Poisoned(poison))
                        .unwrap();
                }
                reset();
                checker.check_constant_declarations(&declarations).unwrap();
                assert_eq!(work(0), Work::default());
                assert_eq!(
                    work(1),
                    Work {
                        inputs: n,
                        completed: 1
                    }
                );
                assert!(matches!(
                    checker.bodies.state(&declarations[n + 1]),
                    FactState::Known(_)
                ));
                let p = body_poison(checker, sink);
                let selected = if mode == "repeated" { 1 } else { n };
                checker
                    .diagnostics
                    .record_failure(DiagnosticFailure::Derivative(p.clone()))
                    .unwrap();
                checker
                    .diagnostics
                    .select_preceding_roots(&p, &all)
                    .unwrap();
                let store = std::mem::replace(checker.diagnostics, DiagnosticStore::new());
                let snapshot = store.finalize(None);
                assert_eq!(
                    snapshot
                        .diagnostics()
                        .iter()
                        .map(|d| d.code().as_str().to_owned())
                        .collect::<Vec<_>>(),
                    (0..selected)
                        .map(|i| format!("ROOT_{i:03}"))
                        .collect::<Vec<_>>()
                );
            });
        }
    }
}
#[test]
fn foreign_dependency_failure_keeps_prior_facts_and_skips_completion() {
    for n in [8_usize, 32, 128] {
        for foreign_first in [false, true] {
            let text = format!(
                "{}const Sink: i8 = {}\nconst After: i8 = 1\n",
                (0..n)
                    .map(|i| format!("const D{i}: i8 = 0\n"))
                    .collect::<String>(),
                (0..n)
                    .map(|i| format!("D{i}"))
                    .collect::<Vec<_>>()
                    .join(" + ")
            );
            with_checker(&text, |checker| {
                let declarations = ids(checker);
                let mut foreign = DiagnosticStore::new();
                let own = root(checker.diagnostics, 0);
                let other = root(&mut foreign, 1);
                for (i, &d) in declarations[..n].iter().enumerate() {
                    let poison = if i == usize::from(!foreign_first) {
                        other.clone()
                    } else {
                        own.clone()
                    };
                    checker
                        .bodies
                        .insert(d, SemanticFact::Poisoned(poison))
                        .unwrap();
                }
                reset();
                let result = checker.check_constant_declarations(&declarations);
                assert!(matches!(
                    result,
                    Err(BodyFlow::Internal(BodyTypingError::DiagnosticStore(
                        eglur_diagnostics::DiagnosticStoreError::ForeignPoison
                    )))
                ));
                assert_eq!(work(0), Work::default());
                assert_eq!(
                    work(1),
                    Work {
                        inputs: 2,
                        completed: 0
                    }
                );
                assert!(matches!(
                    checker.bodies.state(&declarations[n]),
                    FactState::Unperformed
                ));
                assert!(matches!(
                    checker.bodies.state(&declarations[n + 1]),
                    FactState::Unperformed
                ));
                for (i, &d) in declarations[..n].iter().enumerate() {
                    let p = body_poison(checker, d);
                    let result = checker
                        .diagnostics
                        .record_failure(DiagnosticFailure::Derivative(p));
                    if i == usize::from(!foreign_first) {
                        assert!(matches!(
                            result,
                            Err(eglur_diagnostics::DiagnosticStoreError::ForeignPoison)
                        ));
                    } else {
                        assert!(result.is_ok());
                    }
                }
            });
        }
    }
}
#[test]
fn inherited_cycles_precede_forward_references_and_preserve_later_constants() {
    for n in [8_usize, 32, 128] {
        let text = format!(
            "{}const Sink: i8 = {} + Later\nconst Later: i8 = 1\nconst After: i8 = 1\n",
            (0..n)
                .map(|i| format!("const C{i}: i8 = C{}\n", (i + 1) % n))
                .collect::<String>(),
            (0..n)
                .map(|i| format!("C{i}"))
                .collect::<Vec<_>>()
                .join(" + ")
        );
        with_checker(&text, |checker| {
            reset();
            checker.check_constant_declarations(&ids(checker)).unwrap();
            assert_eq!(work(0), Work::default());
            assert_eq!(
                work(1),
                Work {
                    inputs: n,
                    completed: 1
                }
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "After")),
                FactState::Known(_)
            ));
            let sink = body_poison(checker, declaration(checker, "Sink"));
            let roots = (0..n)
                .map(|i| body_poison(checker, declaration(checker, &format!("C{i}"))))
                .collect::<Vec<_>>();
            let all = union(&roots, 0..n);
            assert_selected(checker, &sink, &all, n, "CONST_CYCLE");
        });
    }
}
#[test]
fn forward_reference_priority_discards_inherited_roots_after_original_gate() {
    for n in [8_usize, 32, 128] {
        let text = format!(
            "const Sink: i8 = {}\n{}const After: i8 = 1\n",
            (0..n)
                .map(|i| format!("D{i}"))
                .collect::<Vec<_>>()
                .join(" + "),
            (0..n)
                .map(|i| format!("const D{i}: i8 = 127 + 1\n"))
                .collect::<String>()
        );
        with_checker(&text, |checker| {
            reset();
            checker.check_constant_declarations(&ids(checker)).unwrap();
            assert_eq!(work(0), Work::default());
            assert_eq!(
                work(1),
                Work {
                    inputs: n,
                    completed: 1
                }
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "After")),
                FactState::Known(_)
            ));
            let sink = body_poison(checker, declaration(checker, "Sink"));
            let mut all = (0..n)
                .map(|i| body_poison(checker, declaration(checker, &format!("D{i}"))))
                .collect::<Vec<_>>();
            all.push(sink.clone());
            let all = union(&all, 0..all.len());
            assert_selected(checker, &sink, &all, 1, "CONST_FORWARD_REFERENCE");
        });
    }
}
