use super::*;
use crate::body_types::checker::test_support::{root, with_checker};
use eglur_diagnostics::{DiagnosticFailure, DiagnosticSnapshot, DiagnosticStoreError};
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Work {
    pub(super) inputs: usize,
    pub(super) completed: usize,
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

fn reset() {
    WORK.set(Work::default());
}

fn work() -> Work {
    WORK.get()
}

fn union(inputs: &[Poison], indices: impl IntoIterator<Item = usize>) -> Poison {
    let mut pending = eglur_diagnostics::PoisonAccumulator::new();
    for index in indices {
        pending.include(&inputs[index]).unwrap();
    }
    pending.finish().unwrap()
}

fn selected(store: &mut DiagnosticStore, poison: &Poison, all: &Poison) -> Vec<String> {
    store
        .record_failure(DiagnosticFailure::Derivative(poison.clone()))
        .unwrap();
    store.select_preceding_roots(poison, all).unwrap();
    let snapshot = std::mem::replace(store, DiagnosticStore::new()).finalize(None);
    assert!(matches!(snapshot, DiagnosticSnapshot::Rejected(_)));
    snapshot
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect()
}

#[test]
fn application_poison_completes_distinct_shared_overlapping_wide_and_reverse_roots_once() {
    for count in [8_usize, 32, 128] {
        for mode in ["distinct", "repeated", "overlap", "wide", "reversed"] {
            let mut store = DiagnosticStore::new();
            let roots = (0..count).map(|i| root(&mut store, i)).collect::<Vec<_>>();
            let all = union(&roots, 0..count);
            let left = union(&roots, 0..count / 2 + 1);
            let right = union(&roots, count / 2..count);
            reset();
            let mut pending = ApplicationPoison::default();
            for i in 0..count {
                let input = match mode {
                    "repeated" => &roots[0],
                    "overlap" if i % 2 == 0 => &left,
                    "overlap" => &right,
                    "wide" => &all,
                    "reversed" => &roots[count - 1 - i],
                    _ => &roots[i],
                };
                pending.include(input).unwrap();
                assert_eq!(work().completed, 0);
            }
            let poison = pending.take().unwrap();
            assert_eq!(
                work(),
                Work {
                    inputs: count,
                    completed: 1
                }
            );
            assert!(pending.take().is_none());
            assert_eq!(work().completed, 1);
            assert_eq!(
                selected(&mut store, &poison, &all),
                (0..if mode == "repeated" { 1 } else { count })
                    .map(|i| format!("ROOT_{i:03}"))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn empty_application_poison_reads_remain_reusable_between_inference_passes() {
    let mut store = DiagnosticStore::new();
    let root = root(&mut store, 0);
    let mut pending = ApplicationPoison::default();
    reset();
    for _ in 0..128 {
        assert!(pending.take().is_none());
    }
    assert_eq!(work(), Work::default());
    pending.include(&root).unwrap();
    let poison = pending.take().unwrap();
    assert_eq!(
        work(),
        Work {
            inputs: 1,
            completed: 1
        }
    );
    pending.include(&poison).unwrap();
    assert!(pending.take().is_some());
    assert_eq!(
        work(),
        Work {
            inputs: 2,
            completed: 2
        }
    );
}

#[test]
fn foreign_application_poison_refusal_clears_the_slot_in_both_pool_orders() {
    for foreign_first in [false, true] {
        let mut store = DiagnosticStore::new();
        let mut foreign = DiagnosticStore::new();
        let own = root(&mut store, 0);
        let other = root(&mut foreign, 1);
        let (first, second) = if foreign_first {
            (&other, &own)
        } else {
            (&own, &other)
        };
        reset();
        let mut pending = ApplicationPoison::default();
        pending.include(first).unwrap();
        assert!(matches!(
            pending.include(second),
            Err(BodyTypingError::DiagnosticStore(
                DiagnosticStoreError::ForeignPoison
            ))
        ));
        assert!(pending.take().is_none());
        assert_eq!(
            work(),
            Work {
                inputs: 2,
                completed: 0
            }
        );
        pending.include(&own).unwrap();
        let poison = pending.take().unwrap();
        assert_eq!(
            work(),
            Work {
                inputs: 3,
                completed: 1
            }
        );
        assert_eq!(selected(&mut store, &poison, &own), ["ROOT_000"]);
    }
}

#[test]
fn completed_singleton_retains_the_first_inputs_pool_without_checker_authorization() {
    let mut foreign = DiagnosticStore::new();
    let root = root(&mut foreign, 3);
    reset();
    let mut pending = ApplicationPoison::default();
    pending.include(&root).unwrap();
    let poison = pending.take().unwrap();
    assert_eq!(selected(&mut foreign, &poison, &root), ["ROOT_003"]);
    assert_eq!(
        work(),
        Work {
            inputs: 1,
            completed: 1
        }
    );
}

#[test]
fn checking_frames_preserve_early_abandonment_and_borrowed_foreign_refusal() {
    for count in [8_usize, 32, 128] {
        for mode in [
            "poison",
            "mixed",
            "mismatch",
            "deferred-first",
            "deferred-last",
            "forbidden-first",
            "forbidden-last",
            "unperformed-first",
            "unperformed-last",
            "inapplicable-first",
            "inapplicable-last",
            "foreign-first",
            "foreign-last",
        ] {
            let text = format!(
                "fn target({}) -> Unit = ()\nfn inspect(x: i64) -> Unit = target({})\n",
                (0..count)
                    .map(|i| format!("v{i}: i64"))
                    .collect::<Vec<_>>()
                    .join(", "),
                vec!["x"; count].join(", ")
            );
            with_checker(&text, |checker| {
                let owner = checker
                    .collection
                    .modules()
                    .flat_map(|m| m.declarations())
                    .find(|d| d.name().is_some_and(|n| n.text() == "inspect"))
                    .unwrap()
                    .id();
                let (_, source, hir, item) = checker.source_item(owner).unwrap();
                checker.current_owner = Some(owner);
                let ItemKind::Function(function) = item.kind() else {
                    panic!("function expected")
                };
                let body = hir.body(function.body);
                let (_, call) = body
                    .expressions()
                    .find_map(|(_, e)| {
                        if let ExprKind::Call(call) = e.kind() {
                            Some((e.origin(), call))
                        } else {
                            None
                        }
                    })
                    .unwrap();
                let origin = body
                    .expressions()
                    .find_map(|(_, e)| matches!(e.kind(), ExprKind::Call(_)).then_some(e.origin()))
                    .unwrap();
                let integer = checker.primitive(PrimitiveType::I64).unwrap();
                let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
                let unit = checker.primitive(PrimitiveType::Unit).unwrap();
                let substitution = checker
                    .signatures
                    .types_mut()
                    .intern_substitution([])
                    .unwrap();
                let roots = (0..count)
                    .map(|i| root(checker.diagnostics, i))
                    .collect::<Vec<_>>();
                let all = union(&roots, 0..count);
                let mut foreign = DiagnosticStore::new();
                let other = root(&mut foreign, count);
                let mut frame = CheckingFrame {
                    origin,
                    allow_deferred: !mode.starts_with("forbidden"),
                    arguments: ApplicationArguments {
                        values: call
                            .arguments
                            .iter()
                            .enumerate()
                            .map(|(i, a)| ApplicationArgument {
                                slot: ApplicationSlot {
                                    name: format!("v{i}").into_boxed_str(),
                                    template: integer,
                                    value: a.value,
                                    origin: body.expression(a.value).unwrap().origin(),
                                    related: None,
                                    evaluation_order: i,
                                    passing: ArgumentPassing::Value,
                                },
                                typed: None,
                                transfers: Vec::new(),
                            })
                            .collect(),
                        evaluation_order: (0..count).collect(),
                        enclosing_regions: Vec::new(),
                    },
                    substitution,
                    replacements: BTreeMap::new(),
                    specialized_slots: vec![integer; count].into(),
                    result: unit,
                    next: 0,
                    normal_continuation: NormalContinuation::Present,
                    poison: ApplicationPoison::default(),
                };
                reset();
                let mut result = None;
                let mut visits = 0;
                for (i, own) in roots.iter().enumerate() {
                    visits += 1;
                    frame.next = i + 1;
                    let boundary = i
                        == if mode.ends_with("first") {
                            0
                        } else {
                            count - 1
                        };
                    let outcome = if mode == "mismatch" {
                        Outcome::Known(checker.value_expression(boolean))
                    } else if mode == "mixed" && i % 2 == 0 {
                        Outcome::Known(checker.value_expression(integer))
                    } else if boundary
                        && (mode.starts_with("deferred") || mode.starts_with("forbidden"))
                    {
                        Outcome::Deferred
                    } else if boundary && mode.starts_with("unperformed") {
                        Outcome::Unperformed
                    } else if boundary && mode.starts_with("inapplicable") {
                        Outcome::NotApplicable
                    } else {
                        Outcome::Poisoned(if boundary && mode.starts_with("foreign") {
                            other.clone()
                        } else {
                            own.clone()
                        })
                    };
                    match checker.check_application_argument(source, &mut frame, i, outcome) {
                        Err(error) => {
                            assert!(mode.starts_with("foreign"), "{mode}: {error:?}");
                            assert!(frame.poison.pending.is_none());
                            result = Some(Err(error));
                            break;
                        }
                        Ok(ControlFlow::Break(outcome)) => {
                            result = Some(Ok(outcome));
                            break;
                        }
                        Ok(ControlFlow::Continue(())) => {}
                    }
                    assert_eq!(work().completed, 0);
                }
                assert_eq!(work().completed, 0);
                let result = result.unwrap_or_else(|| {
                    checker
                        .advance_application_checking(source, Box::new(frame))
                        .map(|plan| match plan {
                            ApplicationPlan::Complete(outcome) => outcome,
                            _ => panic!("all arguments supplied"),
                        })
                });
                if mode.starts_with("foreign") {
                    assert!(matches!(
                        result,
                        Err(BodyTypingError::DiagnosticStore(
                            DiagnosticStoreError::ForeignPoison
                        ))
                    ));
                    assert_eq!(visits, if mode.ends_with("first") { 2 } else { count });
                    assert_eq!(
                        work(),
                        Work {
                            inputs: visits,
                            completed: 0
                        }
                    );
                } else if mode == "poison" || mode == "mixed" || mode == "mismatch" {
                    let Outcome::Poisoned(poison) = result.unwrap() else {
                        panic!("completed poison expected")
                    };
                    let inputs = if mode == "mixed" { count / 2 } else { count };
                    assert_eq!(
                        work(),
                        Work {
                            inputs,
                            completed: 1
                        }
                    );
                    if mode == "mismatch" {
                        let snapshot =
                            std::mem::replace(checker.diagnostics, DiagnosticStore::new())
                                .finalize(None);
                        assert_eq!(
                            snapshot
                                .diagnostics()
                                .iter()
                                .filter(|d| d.code().as_str() == "CALL_ARG_TYPE_MISMATCH")
                                .count(),
                            count
                        );
                    } else {
                        let indices = (0..count).filter(|i| mode != "mixed" || i % 2 == 1);
                        assert_eq!(
                            selected(checker.diagnostics, &poison, &all),
                            indices.map(|i| format!("ROOT_{i:03}")).collect::<Vec<_>>()
                        );
                    }
                } else {
                    assert_eq!(visits, if mode.ends_with("first") { 1 } else { count });
                    assert_eq!(
                        work(),
                        Work {
                            inputs: visits - 1,
                            completed: 0
                        }
                    );
                    match result.unwrap() {
                        Outcome::Deferred => assert!(mode.starts_with("deferred")),
                        Outcome::Unperformed => assert!(
                            mode.starts_with("unperformed") || mode.starts_with("inapplicable")
                        ),
                        Outcome::Poisoned(poison) => {
                            assert!(mode.starts_with("forbidden"));
                            assert_eq!(
                                selected(checker.diagnostics, &poison, &all),
                                ["TYPE_ARG_INFERENCE_FAILED"]
                            );
                        }
                        _ => panic!("early refusal expected"),
                    }
                }
            });
        }
    }
}
