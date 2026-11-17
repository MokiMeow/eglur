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
        "fn f(x: i64) -> Unit {{ {} }}\nfn after() -> Unit = ()\n",
        (0..count)
            .map(|index| format!("let v{index} = x\n"))
            .collect::<String>()
    )
}

struct BlockRun {
    outcome: Result<Outcome<TypedExpression>, BodyFlow>,
    paths: Vec<NormalContinuation>,
    metadata: Vec<(Option<TypeId>, bool)>,
}

fn drive(checker: &mut BodyChecker<'_, '_, '_>, outcomes: Vec<Outcome<BlockStep>>) -> BlockRun {
    let owner = checker
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|name| name.text() == "f"))
        .unwrap()
        .id();
    let (module, _, hir, item) = checker.source_item(owner).unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    let body = hir.body(function.body);
    let statements = body
        .expressions()
        .find_map(|(_, expression)| {
            if let ExprKind::Block { statements } = expression.kind() {
                Some(statements.clone())
            } else {
                None
            }
        })
        .unwrap();
    let expressions = body
        .expressions()
        .filter_map(|(id, expression)| matches!(expression.kind(), ExprKind::Name(_)).then_some(id))
        .collect::<Vec<_>>();
    assert_eq!(statements.len(), outcomes.len());
    assert_eq!(expressions.len(), outcomes.len());

    let integer = checker.primitive(PrimitiveType::I64).unwrap();
    checker.current_owner = Some(owner);
    let region = ControlRegionKey::Callable(owner);
    checker.enter_control_region(region.clone()).unwrap();
    let mut frame = BlockFrame::new(statements.clone(), TypeRequest::local(Some(integer)));
    let mut paths = Vec::new();
    let mut metadata = Vec::new();
    WORK.set(Work::default());

    for (index, next) in outcomes.into_iter().enumerate() {
        let (statement, expected, path) = frame
            .next_statement()
            .expect("every later statement must be visited");
        assert_eq!(statement, statements[index]);
        assert_eq!(expected.expected_type(), Some(integer));
        paths.push(path);
        let child = next.clone().map(|_| checker.value_expression(integer));
        checker
            .record_expression(ExpressionKey::new(module, expressions[index]), child)
            .unwrap();

        match checker.resume_block(frame, next) {
            Ok(next) => {
                metadata.push((next.result, next.reachable));
                frame = next;
            }
            Err(error) => {
                checker.leave_control_region(&region).unwrap();
                return BlockRun {
                    outcome: Err(error),
                    paths,
                    metadata,
                };
            }
        }
    }

    assert!(frame.next_statement().is_none());
    let outcome = checker.finish_block(frame);
    checker.leave_control_region(&region).unwrap();
    BlockRun {
        outcome,
        paths,
        metadata,
    }
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
fn block_failures_complete_once_and_preserve_every_selected_root() {
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
                let run = drive(checker, outcomes);
                assert_eq!(run.paths, vec![NormalContinuation::Present; count]);
                assert_eq!(run.metadata, vec![(None, true); count]);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: count,
                        completed: 1
                    }
                );
                let Outcome::Poisoned(poison) = run.outcome.unwrap() else {
                    panic!("poisoned block expected")
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
fn block_completion_preserves_all_twenty_five_ordered_state_pairs() {
    for left in 0..5 {
        for right in 0..5 {
            with_checker(&source(2), |checker| {
                let roots = roots(checker, 2);
                let all = union(&roots, 0..2);
                let input = |state: usize, index: usize| match state {
                    0 => Outcome::Known(BlockStep::continuing(NormalContinuation::Present)),
                    1 => Outcome::NotApplicable,
                    2 => Outcome::Deferred,
                    3 => Outcome::Unperformed,
                    _ => Outcome::Poisoned(roots[index].clone()),
                };
                let run = drive(checker, vec![input(left, 0), input(right, 1)]);
                assert_eq!(run.paths, vec![NormalContinuation::Present; 2]);
                assert_eq!(run.metadata, vec![(None, true); 2]);
                let inputs = usize::from(left == 4) + usize::from(right == 4);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs,
                        completed: usize::from(inputs != 0)
                    }
                );
                match run.outcome.unwrap() {
                    Outcome::Known(value) => {
                        assert_eq!(left.max(right), 0);
                        assert_eq!(value.ty, checker.primitive(PrimitiveType::Unit).unwrap());
                        assert_eq!(value.normal_continuation, NormalContinuation::Present);
                    }
                    Outcome::NotApplicable => assert_eq!(left.max(right), 1),
                    Outcome::Deferred => assert_eq!(left.max(right), 2),
                    Outcome::Unperformed => assert_eq!(left.max(right), 3),
                    Outcome::Poisoned(poison) => {
                        assert_eq!(left.max(right), 4);
                        assert_selected(
                            checker,
                            poison,
                            &all,
                            (0..2).filter(|&index| if index == 0 { left } else { right } == 4),
                        );
                    }
                }
                assert_eq!(
                    checker
                        .transfers
                        .iter()
                        .filter(|transfer| transfer.kind() == ControlTransferKind::NormalCompletion)
                        .count(),
                    usize::from(left.max(right) == 0)
                );
            });
        }
    }
}

#[test]
fn block_known_metadata_is_updated_before_poison_join_and_not_after_absence() {
    for yield_value in [true, false] {
        with_checker(&source(8), |checker| {
            let roots = roots(checker, 8);
            let all = union(&roots, 0..8);
            let integer = checker.primitive(PrimitiveType::I64).unwrap();
            let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
            let mut outcomes = (0..8)
                .map(|index| Outcome::Poisoned(roots[index].clone()))
                .collect::<Vec<_>>();
            outcomes[1] = Outcome::Known(if yield_value {
                BlockStep::yielding(checker.value_expression(integer))
            } else {
                BlockStep::exit()
            });
            outcomes[3] = Outcome::Known(BlockStep::yielding(checker.value_expression(boolean)));

            let run = drive(checker, outcomes);
            assert_eq!(run.paths[..2], [NormalContinuation::Present; 2]);
            assert_eq!(run.paths[2..], [NormalContinuation::Absent; 6]);
            assert_eq!(run.metadata[0], (None, true));
            let result = yield_value.then_some(integer);
            assert_eq!(run.metadata[1..], [(result, false); 7]);
            assert_eq!(
                WORK.get(),
                Work {
                    inputs: 6,
                    completed: 1
                }
            );
            let Outcome::Poisoned(poison) = run.outcome.unwrap() else {
                panic!("poisoned block expected")
            };
            assert_selected(
                checker,
                poison,
                &all,
                (0..8).filter(|&index| index != 1 && index != 3),
            );
            assert!(checker.transfers.is_empty());
        });
    }
}

#[test]
fn known_blocks_preserve_empty_unit_first_yield_and_never_completion() {
    for count in [0, 8, 32, 128] {
        for mode in ["normal", "yield", "exit"] {
            with_checker(&source(count), |checker| {
                let integer = checker.primitive(PrimitiveType::I64).unwrap();
                let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
                let outcomes = (0..count)
                    .map(|index| {
                        Outcome::Known(match mode {
                            "yield" => {
                                BlockStep::yielding(checker.value_expression(if index == 0 {
                                    integer
                                } else {
                                    boolean
                                }))
                            }
                            "exit" if index == 0 => BlockStep::exit(),
                            _ => BlockStep::continuing(NormalContinuation::Present),
                        })
                    })
                    .collect();
                let run = drive(checker, outcomes);
                assert_eq!(run.paths.len(), count);
                assert_eq!(WORK.get(), Work::default());
                let absent = count != 0 && mode != "normal";
                if count != 0 {
                    assert_eq!(run.paths[0], NormalContinuation::Present);
                    assert_eq!(
                        run.paths[1..],
                        vec![
                            if absent {
                                NormalContinuation::Absent
                            } else {
                                NormalContinuation::Present
                            };
                            count - 1
                        ]
                    );
                }
                let Outcome::Known(value) = run.outcome.unwrap() else {
                    panic!("known block expected")
                };
                let expected = if count != 0 && mode == "yield" {
                    integer
                } else if absent {
                    checker.signatures.types().never()
                } else {
                    checker.primitive(PrimitiveType::Unit).unwrap()
                };
                assert_eq!(value.ty, expected);
                assert_eq!(
                    value.normal_continuation,
                    if absent && mode == "exit" {
                        NormalContinuation::Absent
                    } else {
                        NormalContinuation::Present
                    }
                );
                assert_eq!(checker.transfers.len(), usize::from(!absent));
                assert!(
                    checker
                        .transfers
                        .iter()
                        .all(|transfer| transfer.kind() == ControlTransferKind::NormalCompletion)
                );
            });
        }
    }
}

#[test]
fn foreign_block_inputs_fail_immediately_and_keep_published_child_pools() {
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
                let run = drive(checker, outcomes);
                assert!(matches!(
                    run.outcome,
                    Err(BodyFlow::Internal(BodyTypingError::DiagnosticStore(
                        eglur_diagnostics::DiagnosticStoreError::ForeignPoison
                    )))
                ));
                let visited = if foreign_first { 2 } else { count };
                assert_eq!(run.paths.len(), visited);
                assert_eq!(run.metadata.len(), visited - 1);
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
                    let result = checker
                        .diagnostics
                        .record_failure(DiagnosticFailure::Derivative(poison));
                    if index == if foreign_first { 0 } else { count - 1 } {
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
fn lone_block_poison_uses_its_own_pool_and_completes_once() {
    with_checker(&source(1), |checker| {
        let mut foreign = DiagnosticStore::new();
        let poison = root(&mut foreign, 0);
        let run = drive(checker, vec![Outcome::Poisoned(poison)]);
        assert_eq!(
            WORK.get(),
            Work {
                inputs: 1,
                completed: 1
            }
        );
        let Outcome::Poisoned(poison) = run.outcome.unwrap() else {
            panic!("poisoned block expected")
        };
        assert!(matches!(
            checker
                .diagnostics
                .record_failure(DiagnosticFailure::Derivative(poison.clone())),
            Err(eglur_diagnostics::DiagnosticStoreError::ForeignPoison)
        ));
        assert!(
            foreign
                .record_failure(DiagnosticFailure::Derivative(poison))
                .is_ok()
        );
    });
}
