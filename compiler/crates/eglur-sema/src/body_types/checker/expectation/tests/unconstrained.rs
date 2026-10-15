use super::*;

fn requests_for_variable(
    checker: &BodyChecker<'_, '_, '_>,
    events: &[Event],
    variable: &str,
) -> Vec<TypeRequest> {
    let (module, _, hir, item) = checker
        .source_item(declaration(checker, "inspect"))
        .unwrap();
    let ItemKind::Function(function) = item.kind() else {
        panic!("function expected")
    };
    let keys = hir
        .body(function.body)
        .expressions()
        .filter_map(|(id, expression)| match expression.kind() {
            ExprKind::Name(path) if path.segments().last().unwrap().text() == Some(variable) => {
                Some(ExpressionKey::new(module, id))
            }
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    events
        .iter()
        .filter_map(|event| match event {
            Event::Request { key, request } if keys.contains(key) => Some(*request),
            _ => None,
        })
        .collect()
}

#[test]
fn unconstrained_generic_children_retain_the_original_slot_through_suspension() {
    for (parameters, expression, variables) in [
        ("x: i32, y: Bool", "(x, y)", &["x", "y"][..]),
        ("x: i32, y: Bool", "{ yield (x, y) }", &["x", "y"]),
        ("x: i32", "{ yield { yield x } }", &["x"][..]),
        (
            "flag: Bool, x: i32, y: i32",
            "if flag { yield x } else { yield y }",
            &["x", "y"],
        ),
        (
            "flag: Bool, x: i32, y: i32",
            "match flag { _ if flag => x, _ => y }",
            &["x", "y"],
        ),
    ] {
        let text = format!(
            "fn id[T](value: T) -> T = value\nfn inspect({parameters}) -> Unit {{\nlet out = id({expression})\n}}"
        );
        with_events(&text, |checker, events| {
            assert!(!checker.diagnostics.has_errors(), "{expression}");
            let (site, _) = calls(checker).into_iter().next().unwrap();
            let expected = ExpectationCustody::CallArgument {
                call: site,
                index: 0,
            };
            for variable in variables {
                let requests = requests_for_variable(checker, events, variable);
                assert!(!requests.is_empty(), "{expression}: {variable}");
                assert!(
                    requests.iter().all(|request| request.custody() == expected),
                    "{expression}: {variable} lost original demand"
                );
            }
            assert!(matches!(
                checker.bodies.state(&declaration(checker, "inspect")),
                FactState::Known(_)
            ));
        });
    }
}

#[test]
fn unconstrained_numeric_children_retain_custody_across_right_first_probes() {
    for expression in ["-x", "1 + x", "x + y", "((1 + 2) * 3) + x"] {
        with_numeric_events(
            &format!(
                "fn id[T](value: T) -> T = value\nfn inspect(x: i32, y: i32) -> Unit {{\nlet out = id(checked {{ yield {expression} }})\n}}"
            ),
            |checker, events| {
                assert!(!checker.diagnostics.has_errors(), "{expression}");
                let (site, _) = calls(checker).into_iter().next().unwrap();
                let requests = requests_for_variable(checker, events, "x");
                assert!(!requests.is_empty(), "{expression}");
                assert!(
                    requests.iter().all(|request| request.custody()
                        == ExpectationCustody::CallArgument {
                            call: site,
                            index: 0
                        }),
                    "{expression}"
                );
                assert!(matches!(
                    checker.bodies.state(&declaration(checker, "inspect")),
                    FactState::Known(_)
                ));
            },
        );
    }
}

#[test]
fn unconstrained_named_arguments_keep_written_order_not_parameter_order() {
    with_events(
        "fn pair[T, U](left: T, right: U) -> Unit = ()\nfn inspect(x: i32, y: Bool) -> Unit {\npair(right = y, left = x)\n}",
        |checker, events| {
            assert!(!checker.diagnostics.has_errors());
            let (site, _) = calls(checker).into_iter().next().unwrap();
            for (variable, index) in [("y", 0), ("x", 1)] {
                let requests = requests_for_variable(checker, events, variable);
                assert!(!requests.is_empty());
                assert!(requests.iter().all(|request| request.custody()
                    == ExpectationCustody::CallArgument { call: site, index }));
                assert!(requests.iter().any(|request| request.expected().is_none()
                    && request.allow_deferred
                    && !request.default_integer
                    && request.value_required));
            }
        },
    );
}

#[test]
fn overload_argument_visits_retain_custody_before_candidate_specialization() {
    with_events(
        "overload fn pick(left: i32, right: Bool) -> Unit = ()\noverload fn pick(left: Bool, right: i32) -> Unit = ()\nfn inspect(x: i32, y: Bool) -> Unit {\npick(right = y, left = x)\n}",
        |checker, events| {
            assert!(!checker.diagnostics.has_errors());
            let (site, _) = calls(checker).into_iter().next().unwrap();
            for (variable, index) in [("y", 0), ("x", 1)] {
                let requests = requests_for_variable(checker, events, variable);
                assert!(!requests.is_empty());
                assert!(requests.iter().all(|request| request.custody()
                    == ExpectationCustody::CallArgument { call: site, index }));
                assert!(requests.iter().any(|request| request.expected().is_none()
                    && !request.allow_deferred
                    && request.default_integer));
            }
        },
    );
}

#[test]
fn nested_generic_calls_do_not_replace_the_inner_original_slot() {
    with_events(
        "fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> Unit {\nlet out = id(id({ yield x }))\n}",
        |checker, events| {
            assert!(!checker.diagnostics.has_errors());
            let (module, _, hir, item) = checker
                .source_item(declaration(checker, "inspect"))
                .unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function expected")
            };
            let body = hir.body(function.body);
            let (inner, _) = calls(checker)
                .into_iter()
                .find(|(_, call)| {
                    matches!(
                        body.expression(call.arguments[0].value).unwrap().kind(),
                        ExprKind::Block { .. }
                    )
                })
                .unwrap();
            assert_eq!(inner.module(), module);
            let requests = requests_for_variable(checker, events, "x");
            assert!(!requests.is_empty());
            assert!(requests.iter().all(|request| request.custody()
                == ExpectationCustody::CallArgument {
                    call: inner,
                    index: 0
                }));
        },
    );
}

#[test]
fn deferred_function_argument_keeps_the_same_slot_on_definitive_replay() {
    with_events(
        "fn id[T](value: T) -> T = value\nfn apply_fn[T](f: Fn(T) -> T, value: T) -> T = f(value)\nfn inspect(x: i32) -> Unit {\nlet out = apply_fn(id, x)\n}",
        |checker, events| {
            assert!(!checker.diagnostics.has_errors());
            let (site, _) = calls(checker).into_iter().next().unwrap();
            let requests = requests_for_variable(checker, events, "id");
            assert!(requests.iter().all(|request| request.custody()
                == ExpectationCustody::CallArgument {
                    call: site,
                    index: 0
                }));
            assert!(requests.iter().any(|request| request.expected().is_none()
                && request.allow_deferred
                && !request.default_integer));
            assert!(requests.iter().any(|request| request.expected().is_some()
                && !request.allow_deferred
                && request.default_integer));
        },
    );
}

#[test]
fn retaining_unconstrained_branch_custody_does_not_type_the_condition() {
    with_events(
        "fn id[T](value: T) -> T = value\nfn inspect(flag: Bool, x: i32) -> Unit {\nlet out = id(if flag { yield x } else { yield x })\n}",
        |checker, events| {
            assert!(!checker.diagnostics.has_errors());
            let conditions = requests_for_variable(checker, events, "flag");
            assert!(!conditions.is_empty());
            assert!(conditions.iter().all(|request| request.expected().is_none()
                && request.custody() == ExpectationCustody::Caller
                && !request.allow_deferred
                && request.default_integer));
        },
    );
}

#[test]
fn absent_type_retargeting_keeps_identity_and_inference_flags() {
    with_checker("fn inspect(x: i32) -> i32 = x", |checker| {
        let custody = return_custody(checker, "inspect");
        let ty = checker.primitive(PrimitiveType::I32).unwrap();
        for request in [
            TypeRequest::probe(custody),
            TypeRequest::definitive(None).with_custody(custody),
            TypeRequest::discarded().with_custody(custody),
        ] {
            let flags = (
                request.allow_deferred,
                request.default_integer,
                request.value_required,
            );
            for expected in [Some(ty), None, Some(ty)] {
                let child = request.retarget(expected);
                assert_eq!(child.expected_type(), expected);
                assert_eq!(child.custody(), custody);
                assert_eq!(
                    (
                        child.allow_deferred,
                        child.default_integer,
                        child.value_required
                    ),
                    flags
                );
                let definitive = child.definitive_child(None);
                assert!(definitive.expected().is_none());
                assert_eq!(definitive.custody(), custody);
                assert!(!definitive.allow_deferred);
                assert!(definitive.default_integer && definitive.value_required);
            }
        }
    });
}
