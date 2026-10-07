use super::*;
use crate::body_types::checker::poison::tests::{Work, reset, selected, union, work};
use crate::body_types::checker::test_support::{root, with_checker};
use eglur_diagnostics::DiagnosticStoreError;

#[test]
fn dispatch_frames_keep_late_roots_and_caller_specific_refusal_precedence() {
    for count in [8_usize, 32, 128] {
        for mode in [
            "distinct",
            "repeated",
            "mixed-known",
            "mixed-diverging",
            "mixed-unperformed",
            "mixed-notapplicable",
            "deferred-first",
            "deferred-last",
            "foreign-first",
            "foreign-last",
            "all-unperformed",
            "all-notapplicable",
        ] {
            let source = format!(
                "overload fn target({}) -> Unit = ()\noverload fn target({}) -> Unit = ()\nfn inspect(x: i64) -> Unit = target({})\n",
                (0..count)
                    .map(|i| format!("v{i}: i64"))
                    .collect::<Vec<_>>()
                    .join(", "),
                (0..count)
                    .map(|i| format!("v{i}: Bool"))
                    .collect::<Vec<_>>()
                    .join(", "),
                vec!["x"; count].join(", ")
            );
            with_checker(&source, |checker| {
                let owner = checker
                    .collection
                    .modules()
                    .flat_map(|m| m.declarations())
                    .find(|d| d.name().is_some_and(|name| name.text() == "inspect"))
                    .unwrap()
                    .id();
                let (module, source, hir, item) = checker.source_item(owner).unwrap();
                let ItemKind::Function(function) = item.kind() else {
                    panic!("function expected")
                };
                let body = hir.body(function.body);
                let (expression, origin, call) = body
                    .expressions()
                    .find_map(|(id, e)| {
                        if let ExprKind::Call(call) = e.kind() {
                            Some((id, e.origin(), call.clone()))
                        } else {
                            None
                        }
                    })
                    .unwrap();
                let candidates = checker
                    .collection
                    .modules()
                    .flat_map(|m| m.declarations())
                    .filter(|d| d.name().is_some_and(|name| name.text() == "target"))
                    .map(|d| {
                        let FactState::Known(DeclarationSignature::Callable(id)) = checker
                            .signatures
                            .declaration(&CanonicalTarget::Source(d.id()))
                        else {
                            panic!("callable expected")
                        };
                        let callable = checker.signatures.callable(*id).unwrap().clone();
                        let bindings =
                            BodyChecker::bind_call_argument_indices(&callable, &call.arguments)
                                .ok();
                        OverloadCandidate { callable, bindings }
                    })
                    .collect();
                let context = BodyContext {
                    module,
                    source,
                    hir,
                    body,
                };
                let integer = checker.primitive(PrimitiveType::I64).unwrap();
                let roots = (0..count)
                    .map(|i| root(checker.diagnostics, i))
                    .collect::<Vec<_>>();
                let all = union(&roots, 0..count);
                let mut foreign = DiagnosticStore::new();
                let other = root(&mut foreign, count);
                let frame = DispatchArgumentsFrame::new(
                    DispatchRequest {
                        expression,
                        origin,
                        call,
                        ty: TypeRequest::definitive(None),
                    },
                    DispatchSelection::Overload(candidates),
                    NormalContinuation::Present,
                );
                reset();
                let mut visits = 0;
                let mut plan = checker.advance_dispatch_arguments(context, Box::new(frame));
                let result = loop {
                    match plan {
                        Err(error) => break Err(error),
                        Ok(InvocationPlan::Complete(outcome)) => break Ok(outcome),
                        Ok(InvocationPlan::Expression {
                            expression,
                            path,
                            continuation:
                                InvocationContinuation::Dispatch(DispatchContinuation::Argument {
                                    frame,
                                    argument,
                                }),
                            ..
                        }) => {
                            let index = frame.next - 1;
                            assert_eq!(index, visits);
                            assert_eq!(argument, expression);
                            assert_eq!(
                                path,
                                if mode == "mixed-diverging" && index > 0 {
                                    NormalContinuation::Absent
                                } else {
                                    NormalContinuation::Present
                                }
                            );
                            assert_eq!(work().completed, 0);
                            visits += 1;
                            let boundary = index
                                == if mode.ends_with("first") {
                                    0
                                } else {
                                    count - 1
                                };
                            let outcome = if mode == "all-unperformed" {
                                Outcome::Unperformed
                            } else if mode == "all-notapplicable" {
                                Outcome::NotApplicable
                            } else if boundary && mode.starts_with("deferred") {
                                Outcome::Deferred
                            } else if mode.starts_with("mixed") && index % 2 == 0 {
                                match mode {
                                    "mixed-known" => {
                                        Outcome::Known(checker.value_expression(integer))
                                    }
                                    "mixed-diverging" => Outcome::Known(
                                        checker
                                            .value_expression(checker.signatures.types().never()),
                                    ),
                                    "mixed-unperformed" => Outcome::Unperformed,
                                    _ => Outcome::NotApplicable,
                                }
                            } else {
                                Outcome::Poisoned(if boundary && mode.starts_with("foreign") {
                                    other.clone()
                                } else if mode == "repeated" {
                                    roots[0].clone()
                                } else {
                                    roots[index].clone()
                                })
                            };
                            plan = checker.resume_dispatch(
                                context,
                                DispatchContinuation::Argument { frame, argument },
                                outcome,
                            );
                        }
                        _ => panic!("argument continuation expected"),
                    }
                };
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
                } else if mode.starts_with("deferred") {
                    assert!(matches!(result.unwrap(), Outcome::Deferred));
                    assert_eq!(visits, if mode.ends_with("first") { 1 } else { count });
                    assert_eq!(
                        work(),
                        Work {
                            inputs: visits - 1,
                            completed: 0
                        }
                    );
                } else if mode.starts_with("all-") {
                    assert!(matches!(result.unwrap(), Outcome::Unperformed));
                    assert_eq!(visits, count);
                    assert_eq!(work(), Work::default());
                } else {
                    let Outcome::Poisoned(poison) = result.unwrap() else {
                        panic!("completed poison expected")
                    };
                    let inputs = if mode.starts_with("mixed") {
                        count / 2
                    } else {
                        count
                    };
                    assert_eq!(visits, count);
                    assert_eq!(
                        work(),
                        Work {
                            inputs,
                            completed: 1
                        }
                    );
                    let indices = (0..count).filter(|i| {
                        if mode == "repeated" {
                            *i == 0
                        } else {
                            !mode.starts_with("mixed") || i % 2 == 1
                        }
                    });
                    assert_eq!(
                        selected(checker.diagnostics, &poison, &all),
                        indices.map(|i| format!("ROOT_{i:03}")).collect::<Vec<_>>()
                    );
                }
            });
        }
    }
}
