use super::*;
use crate::body_types::checker::invocation::InvocationPlan;
use crate::body_types::checker::poison::tests::{Work, reset, selected, union, work};
use crate::body_types::checker::test_support::{root, with_checker};

#[test]
fn overload_target_call_and_value_complete_poison_once_after_all_targets() {
    for count in [2_usize, 8, 32] {
        for mode in ["distinct", "shared", "mixed", "reversed"] {
            for value in [false, true] {
                let order = if mode == "reversed" {
                    (0..count).rev().collect::<Vec<_>>()
                } else {
                    (0..count).collect()
                };
                let mut text = if mode == "shared" {
                    "type Shared = Missing\n".to_owned()
                } else {
                    String::new()
                };
                for index in 0..count {
                    text.push_str(&format!("struct Type{index} {{}}\n"));
                }
                for index in order {
                    let result = if mode == "shared" {
                        "Shared".to_owned()
                    } else if mode == "mixed" && index % 2 == 0 {
                        "Unit".to_owned()
                    } else {
                        format!("Missing{index}")
                    };
                    text.push_str(&format!(
                        "overload fn target(value: Type{index}) -> {result} = ()\n"
                    ));
                }
                text.push_str("fn inspect() -> Unit = target(0)\n");

                with_checker(&text, |checker| {
                    let owner = checker
                        .collection
                        .modules()
                        .flat_map(|module| module.declarations())
                        .find(|declaration| {
                            declaration
                                .name()
                                .is_some_and(|name| name.text() == "inspect")
                        })
                        .unwrap()
                        .id();
                    let (module, source, hir, item) = checker.source_item(owner).unwrap();
                    let ItemKind::Function(function) = item.kind() else {
                        panic!("function expected")
                    };
                    let body = hir.body(function.body);
                    let (expression, origin, call) = body
                        .expressions()
                        .find_map(|(id, expression)| match expression.kind() {
                            ExprKind::Call(call) => Some((id, expression.origin(), call.clone())),
                            _ => None,
                        })
                        .unwrap();
                    let targets = checker
                        .collection
                        .modules()
                        .flat_map(|module| module.declarations())
                        .filter(|declaration| {
                            declaration
                                .name()
                                .is_some_and(|name| name.text() == "target")
                        })
                        .map(|declaration| CanonicalTarget::Source(declaration.id()))
                        .collect::<Vec<_>>();
                    let inputs = targets
                        .iter()
                        .filter_map(|target| match checker.signatures.declaration(target) {
                            FactState::Poisoned(poison) => Some(poison.clone()),
                            FactState::Known(_) => None,
                            state => panic!("unexpected signature state: {state:?}"),
                        })
                        .collect::<Vec<_>>();
                    let sentinel = root(checker.diagnostics, count);
                    let all = union(&inputs, 0..inputs.len()).merge(&sentinel).unwrap();
                    let context = BodyContext {
                        work: None,
                        module,
                        source,
                        hir,
                        body,
                    };

                    reset();
                    let poison = if value {
                        match checker
                            .type_resolved_name(
                                context,
                                call.callee,
                                TypeRequest::definitive(None),
                                &ResolvedName::Declarations(targets.into_boxed_slice()),
                            )
                            .unwrap()
                        {
                            Outcome::Poisoned(poison) => poison,
                            _ => panic!("poisoned value expected"),
                        }
                    } else {
                        match checker
                            .prepare_overloaded_call(
                                context,
                                expression,
                                origin,
                                &call,
                                CallTypingRequest {
                                    expected: TypeRequest::definitive(None),
                                    selection: None,
                                    original: None,
                                },
                                &targets,
                            )
                            .unwrap()
                        {
                            InvocationPlan::Complete(Outcome::Poisoned(poison)) => poison,
                            _ => panic!("poisoned invocation expected"),
                        }
                    };
                    assert_eq!(
                        work(),
                        Work {
                            inputs: inputs.len(),
                            completed: 1
                        }
                    );
                    let expected_roots = if mode == "shared" {
                        1
                    } else if mode == "mixed" {
                        count / 2
                    } else {
                        count
                    };
                    assert_eq!(
                        selected(checker.diagnostics, &poison, &all),
                        vec!["NAME_NOT_FOUND".to_owned(); expected_roots]
                    );
                });
            }
        }
    }
}
