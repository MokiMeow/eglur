use super::*;
use crate::body_types::checker::test_support::{root, with_checker};
use eglur_diagnostics::{DiagnosticFailure, DiagnosticSnapshot};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inputs: usize,
    completed: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { inputs: 0, completed: 0 }) };
}

pub(super) enum Step {
    Input,
    Complete,
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input => work.inputs += 1,
            Step::Complete => work.completed += 1,
        }
        work
    });
}

fn source(count: usize) -> String {
    format!(
        "fn f(x: i64) -> Unit = ({})\nfn after() -> Unit = ()\n",
        vec!["x"; count].join(", ")
    )
}

fn drive(
    checker: &mut BodyChecker<'_, '_, '_>,
    outcomes: Vec<Outcome<TypedExpression>>,
) -> (
    Result<Outcome<TypedExpression>, BodyFlow>,
    Vec<NormalContinuation>,
) {
    let owner = checker
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|name| name.text() == "f"))
        .unwrap()
        .id();
    let (module, source, hir, item) = checker.source_item(owner).unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    let body = hir.body(function.body);
    let values = body
        .expressions()
        .find_map(|(_, expression)| {
            if let ExprKind::Tuple(values) = expression.kind() {
                Some(values.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(values.len(), outcomes.len());

    let context = BodyContext {
        module,
        source,
        hir,
        body,
    };
    let integer = checker.primitive(PrimitiveType::I64).unwrap();
    let expected = checker
        .signatures
        .types_mut()
        .tuple(vec![integer; outcomes.len()])
        .unwrap();
    let request = TypeRequest::local(Some(expected));
    let mut plan = checker.prepare_tuple(values.clone(), request).unwrap();
    let mut paths = Vec::new();
    WORK.set(Work::default());

    for (index, outcome) in outcomes.into_iter().enumerate() {
        let ValuePlan::Expression {
            expression,
            request: actual,
            path,
            continuation,
        } = plan
        else {
            panic!("every tuple element must be visited")
        };
        assert_eq!(expression, values[index]);
        assert_eq!(actual.expected_type(), Some(integer));
        assert_eq!(actual.allow_deferred, request.allow_deferred);
        assert_eq!(actual.default_integer, request.default_integer);
        assert_eq!(actual.value_required, request.value_required);
        paths.push(path);
        checker
            .record_expression(ExpressionKey::new(module, expression), outcome.clone())
            .unwrap();

        match checker.resume_value(context, continuation, outcome) {
            Ok(next) => plan = next,
            Err(error) => return (Err(error), paths),
        }
    }

    let ValuePlan::Complete(outcome) = plan else {
        panic!("tuple completion expected")
    };
    (Ok(outcome), paths)
}

fn roots(checker: &mut BodyChecker<'_, '_, '_>, count: usize) -> Vec<Poison> {
    (0..count)
        .map(|index| root(checker.diagnostics, index))
        .collect()
}

fn union(roots: &[Poison], indices: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = eglur_diagnostics::PoisonAccumulator::new();
    for index in indices {
        pending.include(&roots[index]).unwrap();
    }
    pending.finish().unwrap()
}

fn assert_selected(
    checker: &mut BodyChecker<'_, '_, '_>,
    poison: Poison,
    all: &Poison,
    selected: impl IntoIterator<Item = usize>,
) {
    checker
        .diagnostics
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    checker
        .diagnostics
        .select_preceding_roots(&poison, all)
        .unwrap();
    let store = std::mem::replace(checker.diagnostics, DiagnosticStore::new());
    let snapshot = store.finalize(None);
    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    assert_eq!(
        snapshot
            .diagnostics()
            .iter()
            .map(|diagnostic| diagnostic.code().as_str().to_owned())
            .collect::<Vec<_>>(),
        selected
            .into_iter()
            .map(|index| format!("ROOT_{index:03}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn tuple_failures_complete_once_and_preserve_every_selected_root() {
    for count in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "overlap", "wide", "reversed"] {
            with_checker(&source(count), |checker| {
                let roots = roots(checker, count);
                let all = union(&roots, 0..count);
                let left = union(&roots, 0..count / 2 + 1);
                let right = union(&roots, count / 2..count);
                let outcomes = (0..count)
                    .map(|index| {
                        Outcome::Poisoned(match mode {
                            "repeated" => roots[0].clone(),
                            "wide" => all.clone(),
                            "overlap" if index % 2 == 0 => left.clone(),
                            "overlap" => right.clone(),
                            "reversed" => roots[count - 1 - index].clone(),
                            _ => roots[index].clone(),
                        })
                    })
                    .collect();
                let (outcome, paths) = drive(checker, outcomes);
                assert_eq!(paths.len(), count);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: count,
                        completed: 1
                    }
                );
                let Outcome::Poisoned(poison) = outcome.unwrap() else {
                    panic!("poisoned tuple expected")
                };
                assert_selected(
                    checker,
                    poison,
                    &all,
                    0..if mode == "repeated" { 1 } else { count },
                );
            });
        }
    }
}

#[test]
fn poisoned_tuple_visits_all_later_elements_and_dominates_partial_states() {
    for count in [8_usize, 32, 128] {
        for mode in ["known", "unperformed", "deferred", "notapplicable"] {
            with_checker(&source(count), |checker| {
                let roots = roots(checker, count);
                let all = union(&roots, 0..count);
                let integer = checker.primitive(PrimitiveType::I64).unwrap();
                let outcomes = (0..count)
                    .map(|index| {
                        if index % 2 == 1 {
                            Outcome::Poisoned(roots[index].clone())
                        } else {
                            match mode {
                                "known" => Outcome::Known(checker.value_expression(integer)),
                                "unperformed" => Outcome::Unperformed,
                                "deferred" => Outcome::Deferred,
                                _ => Outcome::NotApplicable,
                            }
                        }
                    })
                    .collect();
                let (outcome, paths) = drive(checker, outcomes);
                assert_eq!(paths, vec![NormalContinuation::Present; count]);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: count / 2,
                        completed: 1
                    }
                );
                let Outcome::Poisoned(poison) = outcome.unwrap() else {
                    panic!("poison must dominate partial tuple states")
                };
                assert_selected(
                    checker,
                    poison,
                    &all,
                    (0..count).filter(|index| index % 2 == 1),
                );
            });
        }
    }
}

#[test]
fn nonpoisoned_tuple_states_do_not_complete_provenance() {
    for count in [8_usize, 32, 128] {
        for mode in [
            "known",
            "unperformed",
            "deferred",
            "notapplicable",
            "mixed-partial",
        ] {
            with_checker(&source(count), |checker| {
                let integer = checker.primitive(PrimitiveType::I64).unwrap();
                let outcomes = (0..count)
                    .map(|index| match mode {
                        "known" => Outcome::Known(checker.value_expression(integer)),
                        "unperformed" => Outcome::Unperformed,
                        "deferred" => Outcome::Deferred,
                        "notapplicable" => Outcome::NotApplicable,
                        _ if index % 2 == 0 => Outcome::Deferred,
                        _ => Outcome::Unperformed,
                    })
                    .collect();
                let (outcome, paths) = drive(checker, outcomes);
                assert_eq!(paths.len(), count);
                assert_eq!(WORK.get(), Work::default());
                match outcome.unwrap() {
                    Outcome::Known(value) => {
                        assert_eq!(mode, "known");
                        let TypeKind::Tuple(elements) =
                            checker.signatures.types().kind(value.ty).unwrap()
                        else {
                            panic!("tuple type expected")
                        };
                        assert_eq!(elements.as_ref(), vec![integer; count]);
                        assert_eq!(value.normal_continuation, NormalContinuation::Present);
                    }
                    Outcome::Deferred => assert_eq!(mode, "deferred"),
                    Outcome::Unperformed => assert!(matches!(
                        mode,
                        "unperformed" | "notapplicable" | "mixed-partial"
                    )),
                    Outcome::Poisoned(_) | Outcome::NotApplicable => {
                        panic!("unexpected terminal tuple state")
                    }
                }
            });
        }
    }
}

#[test]
fn tuple_preserves_strict_absent_continuation_and_type_order() {
    for count in [8_usize, 32, 128] {
        with_checker(&source(count), |checker| {
            let integer = checker.primitive(PrimitiveType::I64).unwrap();
            let mut outcomes = vec![Outcome::Known(checker.value_expression(integer)); count];
            let Outcome::Known(first) = &mut outcomes[0] else {
                unreachable!()
            };
            first.normal_continuation = NormalContinuation::Absent;
            let (outcome, paths) = drive(checker, outcomes);
            assert_eq!(paths[0], NormalContinuation::Present);
            assert!(
                paths[1..]
                    .iter()
                    .all(|&path| path == NormalContinuation::Absent)
            );
            assert_eq!(WORK.get(), Work::default());
            let Outcome::Known(value) = outcome.unwrap() else {
                panic!("known tuple expected")
            };
            assert_eq!(value.normal_continuation, NormalContinuation::Absent);
            let TypeKind::Tuple(elements) = checker.signatures.types().kind(value.ty).unwrap()
            else {
                panic!("tuple type expected")
            };
            assert_eq!(elements.as_ref(), vec![integer; count]);
        });
    }
}

#[test]
fn foreign_tuple_inputs_fail_immediately_without_completion_and_keep_child_facts() {
    for count in [8_usize, 32, 128] {
        for foreign_first in [true, false] {
            with_checker(&source(count), |checker| {
                let roots = roots(checker, count);
                let mut foreign = DiagnosticStore::new();
                let foreign_root = root(&mut foreign, count);
                let outcomes = (0..count)
                    .map(|index| {
                        Outcome::Poisoned(if index == if foreign_first { 0 } else { count - 1 } {
                            foreign_root.clone()
                        } else {
                            roots[index].clone()
                        })
                    })
                    .collect();
                let (outcome, paths) = drive(checker, outcomes);
                assert!(matches!(
                    outcome,
                    Err(BodyFlow::Internal(BodyTypingError::DiagnosticStore(
                        eglur_diagnostics::DiagnosticStoreError::ForeignPoison
                    )))
                ));
                let visited = if foreign_first { 2 } else { count };
                assert_eq!(paths.len(), visited);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: visited,
                        completed: 0
                    }
                );
                let facts = checker
                    .expressions
                    .iter()
                    .map(|(_, fact)| {
                        let SemanticFact::Poisoned(poison) = fact else {
                            panic!("published poison expected")
                        };
                        poison.clone()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(facts.len(), visited);
                for (index, poison) in facts.into_iter().enumerate() {
                    let selected = checker
                        .diagnostics
                        .record_failure(DiagnosticFailure::Derivative(poison));
                    if index == if foreign_first { 0 } else { count - 1 } {
                        assert!(matches!(
                            selected,
                            Err(eglur_diagnostics::DiagnosticStoreError::ForeignPoison)
                        ));
                    } else {
                        assert!(selected.is_ok());
                    }
                }
            });
        }
    }
}
