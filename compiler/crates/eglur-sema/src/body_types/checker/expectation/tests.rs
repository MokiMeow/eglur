use std::cell::RefCell;

use super::*;
use crate::ExternalDefId;
use crate::body_types::checker::test_support::{with_checker, with_checker_declarations};

#[derive(Clone, Copy)]
pub(super) enum Event {
    Request {
        key: ExpressionKey,
        request: TypeRequest,
    },
    Normalize(ExpectedType),
    Candidate(ExpectedType),
}

thread_local! {
    static EVENTS: RefCell<Option<Vec<Event>>> = const { RefCell::new(None) };
}

pub(super) fn observe(event: Event) {
    EVENTS.with_borrow_mut(|events| {
        if let Some(events) = events {
            events.push(event);
        }
    });
}

struct Recording;

impl Drop for Recording {
    fn drop(&mut self) {
        EVENTS.with_borrow_mut(|events| *events = None);
    }
}

fn with_events(text: &str, inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>, &[Event])) {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_events_inputs(text, &external, None, inspect);
}

fn with_events_inputs(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
    inspect: impl FnOnce(&mut BodyChecker<'_, '_, '_>, &[Event]),
) {
    EVENTS.with_borrow_mut(|events| {
        assert!(events.is_none());
        *events = Some(Vec::new());
    });
    let _recording = Recording;
    with_checker_declarations(text, external, trusted, |checker| {
        checker.check_all().unwrap();
        let events = EVENTS.with_borrow_mut(|events| events.take().unwrap());
        inspect(checker, &events);
    });
}

fn declaration(checker: &BodyChecker<'_, '_, '_>, name: &str) -> DefId {
    checker
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|value| value.text() == name))
        .unwrap()
        .id()
}

fn return_custody(checker: &BodyChecker<'_, '_, '_>, name: &str) -> ExpectationCustody {
    let (_, source, _, item) = checker.source_item(declaration(checker, name)).unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    ExpectationCustody::Written(TypeOccurrenceId::new(
        source.file(),
        function.return_type.unwrap(),
    ))
}

fn variable_requests(
    checker: &BodyChecker<'_, '_, '_>,
    events: &[Event],
    variable: &str,
) -> Vec<ExpectedType> {
    let (module, _, hir, item) = checker
        .source_item(declaration(checker, "inspect"))
        .unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    let body = hir.body(function.body);
    let keys = body
        .expressions()
        .filter_map(|(id, expression)| {
            let ExprKind::Name(path) = expression.kind() else {
                return None;
            };
            (path.segments().last().unwrap().text() == Some(variable))
                .then_some(ExpressionKey::new(module, id))
        })
        .collect::<BTreeSet<_>>();
    events
        .iter()
        .filter_map(|event| match event {
            Event::Request { key, request } if keys.contains(key) => request.expected,
            _ => None,
        })
        .collect()
}

fn calls(checker: &BodyChecker<'_, '_, '_>) -> Vec<(ExpressionKey, CallExpr)> {
    let (module, _, hir, item) = checker
        .source_item(declaration(checker, "inspect"))
        .unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    hir.body(function.body)
        .expressions()
        .filter_map(|(id, expression)| match expression.kind() {
            ExprKind::Call(call) => Some((ExpressionKey::new(module, id), call.clone())),
            _ => None,
        })
        .collect()
}

fn normalized(events: &[Event]) -> Vec<ExpectedType> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Normalize(expected) => Some(*expected),
            _ => None,
        })
        .collect()
}

#[test]
fn source_return_and_tuple_children_keep_one_written_demand() {
    with_events(
        "type Pair = (i32, Bool)\nfn inspect(x: i32, y: Bool) -> Pair = (x, y)",
        |checker, events| {
            let expected = return_custody(checker, "inspect");
            let left = variable_requests(checker, events, "x");
            let right = variable_requests(checker, events, "y");
            assert!(!left.is_empty() && !right.is_empty());
            assert!(
                left.iter()
                    .chain(&right)
                    .all(|value| value.custody() == expected)
            );
            assert_ne!(left[0].ty(), right[0].ty());
            assert!(
                normalized(events)
                    .iter()
                    .any(|value| value.custody() == expected)
            );
        },
    );
}

#[test]
fn annotation_is_a_distinct_original_input_even_with_the_same_canonical_type() {
    with_events(
        "fn inspect(x: i32) -> i32 {\nlet y: i32 = x\nyield y\n}",
        |checker, events| {
            let (_, source, hir, item) = checker
                .source_item(declaration(checker, "inspect"))
                .unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function expected")
            };
            let annotation = hir
                .body(function.body)
                .statements()
                .find_map(|(_, statement)| match statement.kind() {
                    StmtKind::Let { ty, .. } => *ty,
                    _ => None,
                })
                .unwrap();
            let written =
                ExpectationCustody::Written(TypeOccurrenceId::new(source.file(), annotation));
            let returned = return_custody(checker, "inspect");
            assert_ne!(written, returned);
            let x = variable_requests(checker, events, "x");
            let y = variable_requests(checker, events, "y");
            assert!(x.iter().any(|value| value.custody() == written));
            assert!(y.iter().any(|value| value.custody() == returned));
            assert_eq!(x[0].ty(), y[0].ty());
        },
    );
}

#[test]
fn nested_blocks_yields_and_explicit_returns_preserve_declared_return_custody() {
    for body in ["{ yield { yield x } }", "{ return x }"] {
        with_events(
            &format!("fn inspect(x: i32) -> i32 {body}"),
            |checker, events| {
                let expected = return_custody(checker, "inspect");
                let requests = variable_requests(checker, events, "x");
                assert!(!requests.is_empty());
                assert!(requests.iter().all(|value| value.custody() == expected));
                assert!(checker.current_return.is_none());
            },
        );
    }
}

#[test]
fn branch_common_type_retargets_without_changing_written_custody() {
    with_events(
        "fn inspect(flag: Bool, x: i32, y: i32) -> f64 = if flag { yield x } else { yield y }",
        |checker, events| {
            let expected = return_custody(checker, "inspect");
            let x = variable_requests(checker, events, "x");
            let y = variable_requests(checker, events, "y");
            assert!(!x.is_empty() && !y.is_empty());
            assert!(x.iter().chain(&y).all(|value| value.custody() == expected));
            assert_ne!(x[0].ty(), y[0].ty());
            assert_eq!(
                checker.signatures.types().kind(y[0].ty()).unwrap(),
                &TypeKind::Primitive(PrimitiveType::I32)
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "inspect")),
                FactState::Poisoned(_)
            ));
        },
    );
}

#[test]
fn named_arguments_keep_written_ordinals_across_normalization_and_checking() {
    with_events(
        "fn target(left: i32, right: Bool) -> Unit = ()\nfn inspect(x: i32, y: Bool) -> Unit = target(right = y, left = x)",
        |checker, events| {
            let (site, _) = &calls(checker)[0];
            for (variable, index) in [("y", 0), ("x", 1)] {
                let expected = ExpectationCustody::CallArgument { call: *site, index };
                let requests = variable_requests(checker, events, variable);
                assert!(!requests.is_empty());
                assert!(requests.iter().all(|value| value.custody() == expected));
                assert!(
                    normalized(events)
                        .iter()
                        .filter(|value| value.custody() == expected)
                        .count()
                        >= 2
                );
            }
            assert!(
                normalized(events)
                    .iter()
                    .any(|value| value.custody() == ExpectationCustody::Selection(*site))
            );
        },
    );
}

#[test]
fn generic_probe_without_a_type_retains_slot_custody_for_final_normalization() {
    with_events(
        "fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> Unit {\nlet y = id(x)\n}",
        |checker, events| {
            let (site, call) = &calls(checker)[0];
            let key = ExpressionKey::new(site.module(), call.arguments[0].value);
            assert!(events.iter().any(|event| matches!(event, Event::Request { key: found, request } if *found == key && request.expected.is_none() && request.allow_deferred)));
            let expected = ExpectationCustody::CallArgument {
                call: *site,
                index: 0,
            };
            assert!(
                normalized(events)
                    .iter()
                    .any(|value| value.custody() == expected)
            );
            assert!(
                normalized(events)
                    .iter()
                    .any(|value| value.custody() == ExpectationCustody::Selection(*site))
            );
        },
    );
}

#[test]
fn function_value_candidates_keep_the_whole_fn_expectation() {
    with_events(
        "fn id[T](value: T) -> T = value\nfn inspect() -> Unit {\nlet f: Fn(i32) -> i32 = id\n}",
        |checker, events| {
            let (_, source, hir, item) = checker
                .source_item(declaration(checker, "inspect"))
                .unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function expected")
            };
            let annotation = hir
                .body(function.body)
                .statements()
                .find_map(|(_, statement)| match statement.kind() {
                    StmtKind::Let { ty, .. } => *ty,
                    _ => None,
                })
                .unwrap();
            let expected =
                ExpectationCustody::Written(TypeOccurrenceId::new(source.file(), annotation));
            let candidates = events
                .iter()
                .filter_map(|event| match event {
                    Event::Candidate(value) => Some(*value),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(candidates.len() >= 2);
            assert!(candidates.iter().all(|value| value.custody() == expected));
        },
    );
}

#[test]
fn semantic_type_projection_does_not_merge_distinct_original_inputs() {
    with_checker("fn inspect(x: i32) -> i32 = x", |checker| {
        let (_, source, hir, item) = checker
            .source_item(declaration(checker, "inspect"))
            .unwrap();
        let ItemKind::Function(function) = item.kind() else {
            panic!("function expected")
        };
        let ty = checker
            .signatures
            .types_mut()
            .primitive(PrimitiveType::I32)
            .unwrap();
        let first = ExpectedType::written(ty, source.file(), function.return_type.unwrap());
        let second = ExpectedType::written(ty, source.file(), function.parameters[0].ty.unwrap());
        assert_eq!(first.ty(), second.ty());
        assert_ne!(first.custody(), second.custody());
        let boolean = checker
            .signatures
            .types_mut()
            .primitive(PrimitiveType::Bool)
            .unwrap();
        assert_eq!(first.with_type(boolean).custody(), first.custody());
        assert_eq!(
            TypeRequest::definitive(Some(first))
                .retarget(Some(boolean))
                .unwrap()
                .custody(),
            first.custody()
        );
        assert_eq!(
            TypeRequest::local(Some(boolean)).custody(),
            ExpectationCustody::Caller
        );
        assert_eq!(hir.file(), source.file());
    });
}

#[test]
fn postfix_generated_arguments_borrow_caller_instead_of_inventing_slots() {
    for expression in ["x.then(id)", "(x, x).apply(pair)"] {
        with_events(
            &format!(
                "fn id(value: i32) -> i32 = value\nfn pair(left: i32, right: i32) -> i32 = left\nfn inspect(x: i32) -> i32 = {expression}"
            ),
            |checker, events| {
                let (site, call) = &calls(checker)[0];
                for index in 0..call.arguments.len() {
                    assert_eq!(
                        ExpectationCustody::call_argument(*site, call, index),
                        ExpectationCustody::Caller
                    );
                }
                assert!(!normalized(events).iter().any(|value| matches!(value.custody(), ExpectationCustody::CallArgument { call, .. } if call == *site)));
            },
        );
    }
}

#[test]
fn const_initializer_uses_its_written_type_occurrence() {
    with_events(
        "const VALUE: i32 = 3\nfn inspect() -> i32 = VALUE",
        |checker, events| {
            let (_, source, _, item) = checker.source_item(declaration(checker, "VALUE")).unwrap();
            let ItemKind::Const(value) = item.kind() else {
                panic!("const expected")
            };
            let expected =
                ExpectationCustody::Written(TypeOccurrenceId::new(source.file(), value.ty));
            assert!(
                normalized(events)
                    .iter()
                    .any(|value| value.custody() == expected)
            );
            assert!(events.iter().any(|event| matches!(event, Event::Request { request, .. } if request.custody() == expected)));
        },
    );
}

#[test]
fn match_common_type_keeps_input_custody_while_guard_borrows_caller() {
    with_events(
        "fn inspect(flag: Bool, x: i32, y: i32, z: i32) -> f64 = match x {\n_ if flag => y,\n_ => z\n}",
        |checker, events| {
            let expected = return_custody(checker, "inspect");
            let y = variable_requests(checker, events, "y");
            let z = variable_requests(checker, events, "z");
            assert!(!y.is_empty() && !z.is_empty());
            assert!(y.iter().chain(&z).all(|value| value.custody() == expected));
            assert_ne!(y[0].ty(), z[0].ty());
            let guard = variable_requests(checker, events, "flag");
            assert!(!guard.is_empty());
            assert!(
                guard
                    .iter()
                    .all(|value| value.custody() == ExpectationCustody::Caller)
            );
        },
    );
}

#[test]
fn result_block_preserves_the_existing_whole_carrier_expectation() {
    with_events(
        "fn inspect(x: Opt[i32]) -> Opt[i32] = result { yield x }",
        |checker, events| {
            let expected = return_custody(checker, "inspect");
            let requests = variable_requests(checker, events, "x");
            assert!(!requests.is_empty());
            assert!(requests.iter().all(|value| value.custody() == expected));
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "inspect")),
                FactState::Known(_)
            ));
        },
    );
}

#[test]
fn overloaded_function_candidates_do_not_split_the_fn_input() {
    with_events(
        "overload fn pick(value: i32) -> i32 = value\noverload fn pick(value: Bool) -> Bool = value\nfn inspect() -> Unit {\nlet f: Fn(i32) -> i32 = pick\n}",
        |checker, events| {
            let candidates = events
                .iter()
                .filter_map(|event| match event {
                    Event::Candidate(value) => Some(*value),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(candidates.len() >= 3);
            let custody = candidates[0].custody();
            assert!(matches!(custody, ExpectationCustody::Written(_)));
            assert!(candidates.iter().all(|value| value.custody() == custody));
            assert!(
                candidates
                    .iter()
                    .any(|value| value.ty() != candidates[0].ty())
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "inspect")),
                FactState::Known(_)
            ));
        },
    );
}

#[test]
fn receiver_prefix_does_not_change_written_argument_identity() {
    with_events(
        "struct S { fn choose(self: &Self, left: i32, right: Bool) -> i32 = left }\nfn inspect(s: &S, x: i32, y: Bool) -> i32 = s.choose(right = y, left = x)",
        |checker, events| {
            let (site, _) = &calls(checker)[0];
            for (name, index) in [("y", 0), ("x", 1)] {
                let requests = variable_requests(checker, events, name);
                assert!(!requests.is_empty());
                assert!(requests.iter().all(|value| value.custody()
                    == ExpectationCustody::CallArgument { call: *site, index }));
            }
        },
    );
}

#[test]
fn cached_expression_keeps_incoming_custody_without_partitioning_semantic_facts() {
    with_events("fn inspect(x: i32) -> i32 = x", |checker, _| {
        let owner = declaration(checker, "inspect");
        let (module, source, hir, item) = checker.source_item(owner).unwrap();
        let ItemKind::Function(function) = item.kind() else {
            panic!("function expected")
        };
        let body = hir.body(function.body);
        let FunctionBody::Expression(expression) = function.body_form else {
            panic!("expression body expected")
        };
        let key = ExpressionKey::new(module, expression);
        let FactState::Known(before) = checker.expressions.state(&key) else {
            panic!("cached value expected")
        };
        let before = before.clone();
        let expected =
            ExpectedType::written(before.ty, source.file(), function.parameters[0].ty.unwrap());
        EVENTS.with_borrow_mut(|events| *events = Some(Vec::new()));
        let plan = checker
            .prepare_expression(
                BodyContext {
                    module,
                    source,
                    hir,
                    body,
                },
                expression,
                TypeRequest::definitive(Some(expected)),
            )
            .unwrap();
        assert!(matches!(
            plan,
            super::super::expression::ExpressionPlan::Complete(Outcome::Known(_))
        ));
        let events = EVENTS.with_borrow_mut(|events| events.take().unwrap());
        assert!(events.iter().any(|event| matches!(event, Event::Request { key: found, request } if *found == key && request.expected == Some(expected))));
        assert!(normalized(&events).is_empty());
        let FactState::Known(after) = checker.expressions.state(&key) else {
            panic!("cached value lost")
        };
        assert_eq!(&before, after);
    });
}

#[test]
fn checked_success_and_numeric_children_share_the_whole_written_input() {
    let error = ExternalDefId::new("fixture::NumericError").unwrap();
    let module = crate::ModuleAddress::new(["fixture".into()]).unwrap();
    let external = ExternalNameEnvironment::new(
        [module.clone()],
        [crate::ExternalDeclaration::new(
            error.clone(),
            module,
            NameKey::new("NumericError", false).unwrap(),
            DeclarationKind::OpaqueType,
        )
        .unwrap()],
        [error.clone()],
    )
    .unwrap();
    let trusted =
        crate::TrustedSignatureEnvironment::new([crate::TrustedDeclarationSignature::new(
            error.clone(),
            crate::TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        )])
        .unwrap();
    let items =
        crate::TrustedLanguageItemEnvironment::new([(LanguageItem::NumericError, error)]).unwrap();
    let inputs = crate::TrustedDeclarationInputs::new(&trusted, None).with_language_items(&items);
    for expression in ["x + y", "-x", "((1 + 2) * 3) + x", "x + ((1 + 2) * 3)"] {
        with_events_inputs(
            &format!(
                "fn inspect(x: i8, y: i8) -> Res[i8, NumericError] = checked {{ yield {expression} }}"
            ),
            &external,
            Some(inputs),
            |checker, events| {
                let written = return_custody(checker, "inspect");
                let requests = variable_requests(checker, events, "x");
                assert!(!requests.is_empty(), "{expression}");
                assert!(requests.iter().all(|value| value.custody() == written));
                assert!(
                    matches!(
                        checker.bodies.state(&declaration(checker, "inspect")),
                        FactState::Known(_)
                    ),
                    "{expression}"
                );
                let requests = events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Request { request, .. } => request.expected,
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert!(requests.iter().all(|value| value.custody() == written));
                assert!(requests.iter().any(|value| value.ty() != requests[0].ty()));
            },
        );
    }
}

#[test]
fn struct_fields_inherit_the_context_instead_of_becoming_call_slots() {
    with_events(
        "struct Pair { field value: i32 }\nfn inspect(x: i32) -> Pair = Pair { value: x }",
        |checker, events| {
            let requests = variable_requests(checker, events, "x");
            assert!(!requests.is_empty());
            assert!(
                requests
                    .iter()
                    .all(|value| value.custody() == return_custody(checker, "inspect"))
            );
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "inspect")),
                FactState::Known(_)
            ));
        },
    );
}

#[test]
fn question_operand_does_not_gain_a_new_contextual_expectation() {
    with_events(
        "fn inspect(x: Opt[i32]) -> Opt[i32] = result { let y = x?\nyield x }",
        |checker, events| {
            let (_, _, hir, item) = checker
                .source_item(declaration(checker, "inspect"))
                .unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function expected")
            };
            let operand = hir
                .body(function.body)
                .expressions()
                .find_map(|(_, expression)| match expression.kind() {
                    ExprKind::Try(operand) => Some(*operand),
                    _ => None,
                })
                .unwrap();
            let requests = events
                .iter()
                .filter_map(|event| match event {
                    Event::Request { key, request } if key.expression() == operand => Some(request),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(!requests.is_empty());
            assert!(requests.iter().all(|request| request.expected.is_none()));
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "inspect")),
                FactState::Known(_)
            ));
        },
    );
}

#[test]
fn actual_candidate_slots_and_normalized_requests_retrieve_original_tickets() {
    with_events(
        "type Int = i32\noverload fn pick(left: Int, right: Bool) -> Unit = ()\noverload fn pick(left: Bool, right: Int) -> Unit = ()\nfn inspect(x: Int, y: Bool) -> Unit = pick(right = y, left = x)",
        |checker, events| {
            let owner = declaration(checker, "inspect");
            let (module, source, hir, item) = checker.source_item(owner).unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function expected")
            };
            let body = hir.body(function.body);
            let (site, call) = calls(checker).into_iter().next().unwrap();
            let candidates = checker
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .filter(|declaration| declaration.name().is_some_and(|name| name.text() == "pick"))
                .map(|declaration| {
                    let FactState::Known(DeclarationSignature::Callable(id)) =
                        checker.signatures.source_declaration(declaration.id())
                    else {
                        panic!("callable expected")
                    };
                    checker.signatures.callable(*id).unwrap().clone()
                })
                .collect::<Vec<_>>();
            let profile = crate::AnalysisWorkPricing::for_test(7, 2, 3).unwrap();
            let mut accounting = crate::AnalysisWorkAccounting::new(profile, 2).unwrap();
            let upstream = accounting.admit(1, 1_000_000).unwrap();
            let mut inputs =
                crate::AnalysisCallInputRegistry::new(&accounting, 0, module, body).unwrap();
            let selection = inputs
                .selection_ticket(&mut accounting, site, 1_000_000)
                .unwrap();
            let arguments = (0..call.arguments.len())
                .map(|index| {
                    inputs
                        .argument_ticket(&mut accounting, site, index, 1_000_000)
                        .unwrap()
                })
                .collect::<Vec<_>>();
            let origin = body.expression(site.expression()).unwrap().origin();

            for _ in 0..3 {
                for candidate in &candidates {
                    let Outcome::Known(slots) = checker
                        .bind_call_slots(source, origin, site, candidate, &call)
                        .unwrap()
                    else {
                        panic!("bound slots expected")
                    };
                    for slot in slots {
                        let ExpectationCustody::CallArgument { call: found, index } = slot.custody
                        else {
                            panic!("written slot expected")
                        };
                        assert_eq!(found, site);
                        assert_eq!(slot.value, call.arguments[index].value);
                        assert_eq!(
                            inputs
                                .argument_ticket(&mut accounting, found, index, 0)
                                .unwrap(),
                            arguments[index]
                        );
                    }
                }
                for expected in normalized(events) {
                    match expected.custody() {
                        ExpectationCustody::CallArgument { call: found, index }
                            if found == site =>
                        {
                            assert_eq!(
                                inputs
                                    .argument_ticket(&mut accounting, found, index, 0)
                                    .unwrap(),
                                arguments[index]
                            );
                        }
                        ExpectationCustody::Selection(found) if found == site => {
                            assert_eq!(
                                inputs.selection_ticket(&mut accounting, found, 0).unwrap(),
                                selection
                            );
                        }
                        custody if custody == return_custody(checker, "inspect") => {
                            assert_eq!(
                                inputs
                                    .contextual_ticket(&mut accounting, &upstream)
                                    .unwrap(),
                                upstream
                            );
                        }
                        _ => {}
                    }
                }
            }
            assert_eq!(
                accounting
                    .admit(0, 1_000_000)
                    .unwrap()
                    .identity()
                    .requirement(),
                call.arguments.len() + 1
            );
            assert_eq!(
                accounting
                    .admit(1, 1_000_000)
                    .unwrap()
                    .identity()
                    .requirement(),
                1
            );
            assert!(matches!(checker.bodies.state(&owner), FactState::Known(_)));
        },
    );
}
