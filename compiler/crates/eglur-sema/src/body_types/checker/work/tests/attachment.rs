use super::*;
use crate::body_types::checker::expectation::tests::{Event, ResumeKind, capture};

struct Observed {
    events: Vec<Event>,
    policy: Vec<BodyAnalysisInput>,
    accounting: AnalysisWorkAccounting,
    calls: Vec<(ExpressionKey, CallExpr)>,
    names: Vec<(ExpressionKey, String)>,
    owner: DefId,
    returned: ExpectationCustody,
}

impl Observed {
    fn ticket(&self, custody: ExpectationCustody) -> AnalysisRootTicket {
        let tickets = self
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Request { request, .. } if request.custody() == custody => {
                    Some(request.work.as_ref())
                }
                Event::Normalize(value) | Event::Candidate(value) if value.custody() == custody => {
                    Some(value.work())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!tickets.is_empty(), "unobserved demand {custody:?}");
        let ticket = tickets[0].expect("observed demand has no ticket");
        assert!(tickets.iter().all(|value| *value == Some(ticket)));
        ticket.clone()
    }

    fn requests(&self, name: &str) -> Vec<&TypeRequest> {
        self.events
            .iter()
            .filter_map(|event| match event {
                Event::Request { key, request }
                    if self
                        .names
                        .iter()
                        .any(|(found, text)| found == key && text == name) =>
                {
                    Some(request)
                }
                _ => None,
            })
            .collect()
    }

    fn resumes(&self, kind: ResumeKind) -> Vec<&AnalysisRootTicket> {
        self.events
            .iter()
            .filter_map(|event| match event {
                Event::Resume {
                    owner,
                    kind: found,
                    work,
                } if *owner == Some(self.owner) && *found == kind => {
                    Some(work.as_ref().expect("continuation lost work"))
                }
                _ => None,
            })
            .collect()
    }

    fn call_policy(&self) -> Vec<BodyAnalysisInput> {
        self.policy
            .iter()
            .copied()
            .filter(|input| !matches!(input, BodyAnalysisInput::WrittenType(_)))
            .collect()
    }
}

fn exercise(text: &str) -> Observed {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    exercise_with_inputs(text, &external, None)
}

fn exercise_with_inputs(
    text: &str,
    external: &ExternalNameEnvironment,
    trusted: Option<crate::TrustedDeclarationInputs<'_>>,
) -> Observed {
    let mut observed = None;
    with_phase_inputs(text, external, trusted, |mut inputs| {
        let module = inputs.collection.modules().next().unwrap();
        let owner = module
            .declarations()
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.text() == "inspect")
            })
            .unwrap()
            .id();
        let hir = inputs.sources[0].hir().unwrap();
        let calls = hir
            .bodies()
            .flat_map(|(_, body)| body.expressions())
            .filter_map(|(id, value)| match value.kind() {
                ExprKind::Call(call) => Some((ExpressionKey::new(module.id(), id), call.clone())),
                _ => None,
            })
            .collect();
        let names = hir
            .bodies()
            .flat_map(|(_, body)| body.expressions())
            .filter_map(|(id, value)| match value.kind() {
                ExprKind::Name(path) => Some((
                    ExpressionKey::new(module.id(), id),
                    path.segments().last().unwrap().text().unwrap().to_owned(),
                )),
                _ => None,
            })
            .collect();
        let returned = ExpectationCustody::Written(return_occurrence(&inputs));
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let policy = RefCell::new(Vec::new());
        let (result, events) = capture(|| {
            run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|input| {
                    policy.borrow_mut().push(input);
                    ALLOWANCE
                }),
            )
        });
        let product = completed(result.unwrap());
        if inputs.diagnostics.has_errors() {
            let diagnostics =
                std::mem::replace(inputs.diagnostics, DiagnosticStore::new()).finalize(None);
            panic!("fixture produced source errors: {text}\n{diagnostics:?}");
        }
        let metered = snapshot(&product);
        let metered_fingerprints = fingerprints(&mut inputs, &product);
        let unmetered = typecheck_bodies(
            inputs.collection,
            inputs.imports,
            inputs.names,
            Some(external),
            inputs.sources,
            &Support,
            inputs.declarations,
            inputs.trait_environment,
            inputs.diagnostics,
        )
        .unwrap();
        let PhaseProduct::Performed(unmetered) = unmetered.into_parts().0 else {
            panic!("unmetered phase did not perform")
        };
        assert_eq!(metered, snapshot(&unmetered));
        assert_eq!(metered_fingerprints, fingerprints(&mut inputs, &unmetered));
        assert!(!inputs.diagnostics.has_errors());
        observed = Some(Observed {
            events,
            policy: policy.into_inner(),
            accounting,
            calls,
            names,
            owner,
            returned,
        });
    });
    observed.unwrap()
}

fn fingerprints(
    inputs: &mut BodyPhaseInputs<'_, '_>,
    bodies: &BodyTypes,
) -> crate::SemanticFingerprints {
    let evidence = crate::resolve_trait_evidence(
        inputs.collection,
        inputs.sources,
        &Support,
        inputs.declarations.signatures_mut(),
        inputs.trait_environment,
        bodies,
        inputs.diagnostics,
    )
    .unwrap()
    .into_parts()
    .0;
    let PhaseProduct::Performed(evidence) = evidence else {
        panic!("incomplete evidence")
    };
    let effects = crate::analyze_effects(
        inputs.collection,
        inputs.names,
        inputs.sources,
        &Support,
        inputs.declarations.signatures(),
        bodies,
        &evidence,
        None,
        inputs.diagnostics,
    )
    .unwrap()
    .into_parts()
    .0;
    crate::SemanticFingerprints::compute(
        inputs.collection,
        inputs.imports,
        inputs.names,
        inputs.declarations.signatures(),
        inputs.sources,
        bodies,
        inputs.trait_environment,
        &PhaseProduct::Performed(evidence),
        &effects,
    )
    .unwrap()
    .expect("complete bodies must retain fingerprints")
}

#[test]
fn direct_named_and_overload_replay_keep_written_argument_tickets() {
    for declaration in [
        "fn target(left: i32, right: Bool) -> Unit = ()",
        "overload fn target(left: i32, right: Bool) -> Unit = ()\noverload fn target(left: Bool, right: i32) -> Unit = ()",
    ] {
        let observed = exercise(&format!(
            "{declaration}\nfn inspect(x: i32, y: Bool) -> Unit = target(right = y, left = x)"
        ));
        let (site, _) = &observed.calls[0];
        let selection = observed.ticket(ExpectationCustody::Selection(*site));
        let upstream = observed.ticket(observed.returned);
        assert_ne!(selection, upstream);
        let mut arguments = Vec::new();
        for (name, index) in [("y", 0), ("x", 1)] {
            let custody = ExpectationCustody::CallArgument { call: *site, index };
            let ticket = observed.ticket(custody);
            assert_ne!(ticket, selection);
            assert_ne!(ticket, upstream);
            let requests = observed.requests(name);
            assert!(!requests.is_empty());
            assert!(
                requests.iter().all(|request| request.custody() == custody
                    && request.work.as_ref() == Some(&ticket))
            );
            assert!(observed.events.iter().filter(|event| matches!(event, Event::Normalize(expected) if expected.custody() == custody)).count() >= 2);
            arguments.push(ticket);
        }
        assert_ne!(arguments[0], arguments[1]);
        let policy = observed.call_policy();
        assert_eq!(policy.len(), 3);
        assert_eq!(policy[0], BodyAnalysisInput::CallSelection(*site));
        assert!(
            observed
                .resumes(ResumeKind::Application)
                .iter()
                .all(|ticket| **ticket == selection)
        );
        assert!(
            observed
                .resumes(ResumeKind::Invocation)
                .iter()
                .all(|ticket| **ticket == selection)
        );
        for ticket in [&selection, &upstream, &arguments[0], &arguments[1]] {
            let balance = observed.accounting.balance(ticket).unwrap();
            assert_eq!(balance.allowance_ticks(), ALLOWANCE);
            assert!(balance.charged_ticks() > 0);
        }
    }
}

#[test]
fn absent_probe_type_does_not_erase_argument_work() {
    let observed = exercise(
        "fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> Unit {\nlet out = id(x)\n}",
    );
    let (site, _) = &observed.calls[0];
    let custody = ExpectationCustody::CallArgument {
        call: *site,
        index: 0,
    };
    let ticket = observed.ticket(custody);
    assert!(
        observed
            .requests("x")
            .iter()
            .any(|request| request.expected_type().is_none()
                && request.allow_deferred
                && request.work.as_ref() == Some(&ticket))
    );
    assert_eq!(observed.call_policy().len(), 2);

    let request = TypeRequest::probe(custody).with_work(Some(ticket.clone()));
    assert_eq!(request.retarget(None).work, Some(ticket.clone()));
    assert_eq!(request.definitive_child(None).work, Some(ticket.clone()));
    assert_eq!(
        request
            .with_custody(ExpectationCustody::Selection(*site))
            .work,
        None
    );
}

#[test]
fn nested_call_completion_restores_outer_application_and_source_work() {
    let observed = exercise(
        "fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> i32 = id({ yield (id(x), x).0 })",
    );
    assert_eq!(observed.calls.len(), 2);
    assert_eq!(observed.call_policy().len(), 4);
    let selections = observed
        .calls
        .iter()
        .map(|(site, _)| observed.ticket(ExpectationCustody::Selection(*site)))
        .collect::<Vec<_>>();
    assert_ne!(selections[0], selections[1]);
    let resumed = observed.resumes(ResumeKind::Application);
    assert!(
        selections
            .iter()
            .all(|selection| resumed.contains(&selection))
    );
    let argument = observed
        .calls
        .iter()
        .find(|(_, call)| {
            observed
                .names
                .iter()
                .any(|(key, name)| key.expression() == call.arguments[0].value && name == "x")
        })
        .unwrap()
        .0;
    let inner = observed.ticket(ExpectationCustody::CallArgument {
        call: argument,
        index: 0,
    });
    assert!(
        observed
            .requests("x")
            .iter()
            .any(|request| request.work.as_ref() == Some(&inner))
    );
    assert!(!observed.resumes(ResumeKind::Value).is_empty());
}

#[test]
fn annotated_statement_and_branch_resume_keep_distinct_source_loans() {
    let observed = exercise(
        "fn inspect(flag: Bool, x: i32) -> i32 {\nlet y: i32 = if flag { yield x } else { yield x }\nyield y\n}",
    );
    let returned = observed.ticket(observed.returned);
    let requests = observed.requests("x");
    let first = requests[0];
    assert!(matches!(first.custody(), ExpectationCustody::Written(_)));
    let annotation = observed.ticket(first.custody());
    assert_ne!(annotation, returned);
    assert!(
        requests
            .iter()
            .all(|request| request.work.as_ref() == Some(&annotation))
    );
    assert!(
        observed
            .requests("y")
            .iter()
            .all(|request| request.work.as_ref() == Some(&returned))
    );
    assert!(
        observed
            .resumes(ResumeKind::Statement)
            .contains(&&annotation)
    );
    let branches = observed.resumes(ResumeKind::Branch);
    assert!(!branches.is_empty());
    assert!(branches.iter().all(|ticket| **ticket == annotation));
    assert!(observed.call_policy().is_empty());
}

#[test]
fn matching_a_function_value_uses_one_source_input_for_all_components() {
    let observed = exercise(
        "fn id[T](value: T) -> T = value\nfn inspect() -> Unit {\nlet f: Fn(i32) -> i32 = id\n}",
    );
    let candidates = observed
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Candidate(expected) => Some(expected),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(candidates.len() >= 2);
    let custody = candidates[0].custody();
    let ticket = observed.ticket(custody);
    assert!(
        candidates
            .iter()
            .all(|value| value.custody() == custody && value.work() == Some(&ticket))
    );
    assert!(observed.call_policy().is_empty());
}

#[test]
fn method_receiver_and_synthetic_calls_do_not_become_written_arguments() {
    let observed = exercise(
        "struct S { fn choose(self: &Self, left: i32, right: Bool) -> i32 = left }\nfn inspect(s: &S, x: i32, y: Bool) -> i32 = s.choose(right = y, left = x)",
    );
    let (site, _) = &observed.calls[0];
    let selection = observed.ticket(ExpectationCustody::Selection(*site));
    assert!(
        observed
            .requests("s")
            .iter()
            .any(|request| request.work.as_ref() == Some(&selection))
    );
    for (name, index) in [("y", 0), ("x", 1)] {
        let custody = ExpectationCustody::CallArgument { call: *site, index };
        let ticket = observed.ticket(custody);
        assert!(
            observed
                .requests(name)
                .iter()
                .all(|request| request.work.as_ref() == Some(&ticket))
        );
    }
    assert_eq!(observed.call_policy().len(), 3);

    for text in [
        "fn id(x: i32) -> i32 = x\nfn inspect(x: i32) -> i32 = x.then(id)",
        "fn pair(x: i32, y: i32) -> i32 = x\nfn inspect(x: i32) -> i32 = (x, x).apply(pair)",
    ] {
        let observed = exercise(text);
        let upstream = observed.ticket(observed.returned);
        assert!(observed.call_policy().is_empty());
        let resumes = observed.resumes(ResumeKind::Application);
        assert!(!resumes.is_empty());
        assert!(resumes.iter().all(|ticket| **ticket == upstream));
    }
}

#[test]
fn explicit_type_arguments_keep_original_source_roots() {
    let observed =
        exercise("fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> i32 = id::[i32](x)");
    assert_eq!(observed.call_policy().len(), 2);
    let written = observed
        .policy
        .iter()
        .filter(|input| matches!(input, BodyAnalysisInput::WrittenType(_)))
        .collect::<Vec<_>>();
    assert_eq!(written.len(), 3); // Two function returns and one explicit type argument.
}

#[test]
fn result_and_question_finish_keep_the_enclosing_source_ticket() {
    let observed =
        exercise("fn inspect(x: Opt[i32]) -> Opt[i32] = result {\nlet y = x?\nyield x\n}");
    let upstream = observed.ticket(observed.returned);
    for kind in [ResumeKind::Result, ResumeKind::Question] {
        let resumes = observed.resumes(kind);
        assert!(!resumes.is_empty());
        assert!(resumes.iter().all(|ticket| **ticket == upstream));
    }
}

#[test]
fn checked_numeric_and_right_first_probe_continuations_retain_work() {
    let error = crate::ExternalDefId::new("fixture::NumericError").unwrap();
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
    let signatures =
        crate::TrustedSignatureEnvironment::new([crate::TrustedDeclarationSignature::new(
            error.clone(),
            crate::TrustedSignatureKind::Nominal {
                generic_parameters: Box::new([]),
            },
        )])
        .unwrap();
    let items =
        crate::TrustedLanguageItemEnvironment::new([(LanguageItem::NumericError, error)]).unwrap();
    for expression in [
        "-x",
        "1 + x",
        "x + y",
        "((1 + 2) * 3) + x",
        "x + ((1 + 2) * 3)",
    ] {
        let observed = exercise_with_inputs(
            &format!(
                "fn id[T](value: T) -> T = value\nfn inspect(x: i32, y: i32) -> Unit {{\nlet out = id(checked {{ yield {expression} }})\n}}"
            ),
            &external,
            Some(
                crate::TrustedDeclarationInputs::new(&signatures, None).with_language_items(&items),
            ),
        );
        let call = observed.calls[0].0;
        let argument = observed.ticket(ExpectationCustody::CallArgument { call, index: 0 });
        let requests = observed.requests("x");
        assert!(!requests.is_empty());
        assert!(
            requests
                .iter()
                .all(|request| request.work.as_ref() == Some(&argument))
        );
        let checked = observed.resumes(ResumeKind::Checked);
        assert!(!checked.is_empty());
        assert!(checked.iter().all(|ticket| **ticket == argument));
        let numeric = observed
            .resumes(ResumeKind::Numeric)
            .into_iter()
            .chain(observed.resumes(ResumeKind::NumericTree))
            .collect::<Vec<_>>();
        assert!(!numeric.is_empty(), "no numeric resume for {expression}");
        assert!(numeric.iter().all(|ticket| **ticket == argument));
        assert_eq!(observed.call_policy().len(), 2);
    }
}

#[test]
fn structural_values_and_guards_borrow_without_admitting_child_roots() {
    for (declarations, parameters, expression, names) in [
        (
            "",
            "flag: Bool, x: i32, y: i32",
            "match x { _ if flag => x, _ => y }",
            &["flag", "x", "y"][..],
        ),
        (
            "struct Pair { field value: i32 }",
            "x: i32",
            "Pair { value: x }",
            &["x"][..],
        ),
        ("", "x: i32", "* &x", &["x"][..]),
        (
            "struct Pair { field value: i32 }",
            "p: Pair",
            "p.value",
            &["p"][..],
        ),
        ("", "x: i32, y: Bool", "(x, y)", &["x", "y"][..]),
    ] {
        let observed = exercise(&format!(
            "{declarations}\nfn id[T](value: T) -> T = value\nfn inspect({parameters}) -> Unit {{\nlet out = id({expression})\n}}"
        ));
        let call = observed.calls[0].0;
        let argument = observed.ticket(ExpectationCustody::CallArgument { call, index: 0 });
        for name in names {
            let requests = observed.requests(name);
            assert!(!requests.is_empty(), "no request for {expression}: {name}");
            assert!(
                requests
                    .iter()
                    .all(|request| request.work.as_ref() == Some(&argument)),
                "{expression}: {name}"
            );
        }
        assert_eq!(observed.call_policy().len(), 2);
    }
}

#[test]
fn actual_argument_refusal_precedes_expected_type_specialization() {
    with_inputs(TEXT, |mut inputs, external| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let call = first_call(&inputs);
        let target = BodyAnalysisInput::CallArgument { call, index: 0 };
        let policy = RefCell::new(Vec::new());
        let (result, events) = capture(|| {
            run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|input| {
                    policy.borrow_mut().push(input);
                    if input == target { 0 } else { ALLOWANCE }
                }),
            )
        });
        let BodyExecution::Interrupted(stopped) = result.unwrap() else {
            panic!("argument refusal completed")
        };
        assert_eq!(Some(stopped.stop()), accounting.incomplete());
        assert_eq!(policy.borrow().last(), Some(&target));
        assert!(stopped.internal_failures().is_empty());
        assert!(!inputs.diagnostics.has_errors());
        assert!(!events.iter().any(|event| matches!(event,
            Event::Normalize(value) if value.custody() == ExpectationCustody::CallArgument { call, index: 0 }
        )));
    });
}

#[test]
fn ordinary_reduce_by_fallback_borrows_work_without_original_call_admission() {
    let observed = exercise(
        "struct Counter { fn reduce_by(self: &Self, pair: (i32, i32)) -> i32 = pair.0 }\nfn inspect(c: &Counter, x: i32) -> i32 = c.reduce_by((x, x))",
    );
    let upstream = observed.ticket(observed.returned);
    assert!(observed.call_policy().is_empty());
    let applications = observed.resumes(ResumeKind::Application);
    assert!(!applications.is_empty());
    assert!(applications.iter().all(|ticket| **ticket == upstream));
    let requests = observed.requests("x");
    assert!(!requests.is_empty());
    assert!(
        requests
            .iter()
            .all(|request| request.work.as_ref() == Some(&upstream))
    );
}

#[test]
fn actual_selection_refusal_precedes_callee_preparation() {
    with_inputs(
        "fn target(x: i32) -> i32 = x\nfn inspect(x: i32) -> i32 = target(x)",
        |mut inputs, external| {
            let mut accounting = accounting();
            let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
            let call = first_call(&inputs);
            let policy = RefCell::new(Vec::new());
            let (result, events) = capture(|| {
                run(
                    &mut inputs,
                    external,
                    BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|input| {
                        policy.borrow_mut().push(input);
                        if input == BodyAnalysisInput::CallSelection(call) {
                            0
                        } else {
                            ALLOWANCE
                        }
                    }),
                )
            });
            let BodyExecution::Interrupted(stopped) = result.unwrap() else {
                panic!("selection refusal completed")
            };
            assert_eq!(Some(stopped.stop()), accounting.incomplete());
            assert!(stopped.internal_failures().is_empty());
            assert!(!inputs.diagnostics.has_errors());
            assert_eq!(
                policy.borrow().last(),
                Some(&BodyAnalysisInput::CallSelection(call))
            );
            assert!(!events.iter().any(|event| matches!(event, Event::Normalize(expected) if matches!(expected.custody(), ExpectationCustody::Selection(found) | ExpectationCustody::CallArgument { call: found, .. } if found == call))));
            assert!(matches!(
                stopped.prefix().expression(call),
                FactState::Unperformed
            ));
        },
    );
}
