use std::cell::{Cell, RefCell};

use super::super::test_support::{
    BodyPhaseInputs, analysis_inputs, fingerprints, with_phase_inputs,
};
use super::*;
use crate::{
    AnalysisCallInputFailure, AnalysisRootClaimError, AnalysisWorkFailure, AnalysisWorkPricing,
    SignatureAnalysisWork, SignatureExecution, SignatureResolutionInputs,
};

mod attachment;
pub(in crate::body_types::checker) mod proof_attachment;

const TEXT: &str = "fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> i32 = id(x)";
const ALLOWANCE: u64 = 1_000_000;

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

struct Probe {
    at: ExpressionKey,
    requests: Vec<BodyAnalysisInput>,
    tickets: Vec<AnalysisRootTicket>,
    fired: bool,
}

thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}

struct Recording;

impl Drop for Recording {
    fn drop(&mut self) {
        PROBE.with_borrow_mut(|probe| *probe = None);
    }
}

pub(in crate::body_types::checker) fn checkpoint(
    checker: &mut BodyChecker<'_, '_, '_>,
    key: ExpressionKey,
) -> Result<(), BodyFlow> {
    PROBE.with_borrow_mut(|probe| {
        let Some(probe) = probe else { return Ok(()) };
        if probe.fired || probe.at != key {
            return Ok(());
        }
        probe.fired = true;
        let work = checker
            .analysis_work
            .as_mut()
            .expect("probe needs execution custody");
        for request in &probe.requests {
            let ticket = match *request {
                BodyAnalysisInput::WrittenType(occurrence) => work.written_ticket(occurrence)?,
                BodyAnalysisInput::CallSelection(call) => work.selection_ticket(call)?,
                BodyAnalysisInput::CallArgument { call, index } => {
                    work.argument_ticket(call, index)?
                }
                BodyAnalysisInput::ParameterBound { .. } | BodyAnalysisInput::MethodDispatch(_) => {
                    unreachable!("proof inputs are observed at their actual preparation points")
                }
            };
            probe.tickets.push(ticket);
        }
        Ok(())
    })
}

fn record(at: ExpressionKey, requests: Vec<BodyAnalysisInput>) -> Recording {
    PROBE.with_borrow_mut(|probe| {
        assert!(probe.is_none());
        *probe = Some(Probe {
            at,
            requests,
            tickets: Vec::new(),
            fired: false,
        });
    });
    Recording
}

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(1, 1, 1).unwrap(), 1).unwrap()
}

fn with_inputs(
    text: &str,
    inspect: impl FnOnce(BodyPhaseInputs<'_, '_>, &ExternalNameEnvironment),
) {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_phase_inputs(text, &external, None, |inputs| inspect(inputs, &external));
}

fn first_call(inputs: &BodyPhaseInputs<'_, '_>) -> ExpressionKey {
    let module = inputs.collection.modules().next().unwrap();
    let hir = inputs.sources[0].hir().unwrap();
    hir.bodies()
        .flat_map(|(_, body)| body.expressions())
        .find_map(|(id, value)| {
            matches!(value.kind(), ExprKind::Call(_)).then_some(ExpressionKey::new(module.id(), id))
        })
        .unwrap()
}

fn return_occurrence(inputs: &BodyPhaseInputs<'_, '_>) -> TypeOccurrenceId {
    let source = &inputs.sources[0];
    let type_ref = source
        .hir()
        .unwrap()
        .items()
        .filter_map(|(_, item)| match item.kind() {
            ItemKind::Function(function) => function.return_type,
            _ => None,
        })
        .last()
        .unwrap();
    TypeOccurrenceId::new(source.file(), type_ref)
}

fn run<'hir, 'source: 'hir>(
    inputs: &mut BodyPhaseInputs<'_, 'source>,
    external: &ExternalNameEnvironment,
    work: BodyAnalysisWork<'_, 'hir>,
) -> Result<BodyExecution, BodyTypingError> {
    typecheck_bodies_with_work(
        inputs.collection,
        inputs.imports,
        inputs.names,
        Some(external),
        inputs.sources,
        &Support,
        inputs.declarations,
        inputs.trait_environment,
        inputs.diagnostics,
        work,
    )
}

fn completed(execution: BodyExecution) -> BodyTypes {
    let BodyExecution::Completed(packet) = execution else {
        panic!("unexpected interruption")
    };
    let PhaseProduct::Performed(product) = packet.into_parts().0.into_parts().0 else {
        panic!("body phase did not perform")
    };
    product
}

fn snapshot(product: &BodyTypes) -> Vec<String> {
    vec![
        format!("{:?}", product.expressions),
        format!("{:?}", product.bodies),
        format!("{:?}", product.invocations),
        format!("{:?}", product.locals),
        format!("{:?}", product.members),
        format!("{:?}", product.callable_values),
        format!("{:?}", product.reduce_by_commitments),
        format!("{:?}", product.regions),
        format!("{:?}", product.checked_numeric),
        format!("{:?}", product.ownership_operations),
        format!("{:?}", product.transfers),
        format!("{:?}", product.callable_instances),
        format!("{:?}", product.obligations),
        format!("{:?}", product.method_dispatches),
    ]
}

#[test]
fn actual_body_entry_reuses_source_phase_and_call_tickets_across_replay() {
    with_inputs(TEXT, |mut inputs, external| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let source_policy = RefCell::new(Vec::new());
        let source_phase = crate::resolve_declaration_signatures_with_work(
            inputs.collection,
            inputs.imports,
            inputs.sources,
            &Support,
            SignatureResolutionInputs::new(Some(external), None),
            inputs.diagnostics,
            SignatureAnalysisWork::new(&mut accounting, &mut sources, &|occurrence| {
                source_policy.borrow_mut().push(occurrence);
                ALLOWANCE
            }),
        )
        .unwrap();
        assert!(matches!(source_phase, SignatureExecution::Completed(_)));
        let written = return_occurrence(&inputs);
        let original = sources.admitted_ticket(written).unwrap().unwrap().clone();
        let call = first_call(&inputs);
        let requests = vec![
            BodyAnalysisInput::WrittenType(written),
            BodyAnalysisInput::CallSelection(call),
            BodyAnalysisInput::CallArgument { call, index: 0 },
        ];
        let policy = RefCell::new(Vec::new());
        let allowance = Cell::new(ALLOWANCE);
        let policy_fn = |request| {
            policy.borrow_mut().push(request);
            allowance.get()
        };
        let mut first = None;
        let mut balances = None;
        for pass in 0..2 {
            let _recording = record(call, requests.clone());
            let product = completed(
                run(
                    &mut inputs,
                    external,
                    BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &policy_fn),
                )
                .unwrap(),
            );
            let observed = PROBE.with_borrow(|probe| {
                let probe = probe.as_ref().unwrap();
                assert!(probe.fired);
                probe.tickets.clone()
            });
            assert_eq!(observed[0], original);
            assert_eq!(policy.borrow().as_slice(), &requests[1..]);
            assert!(
                observed.iter().all(
                    |ticket| accounting.balance(ticket).unwrap().allowance_ticks() == ALLOWANCE
                )
            );
            if pass == 0 {
                balances = Some(
                    observed
                        .iter()
                        .map(|ticket| accounting.balance(ticket).unwrap().remaining_ticks())
                        .collect::<Vec<_>>(),
                );
                first = Some((observed, snapshot(&product)));
                allowance.set(1);
            } else {
                let (previous, previous_product) = first.as_ref().unwrap();
                assert_eq!(&observed, previous);
                assert_eq!(&snapshot(&product), previous_product);
                assert!(observed.iter().zip(balances.as_ref().unwrap()).all(
                    |(ticket, previous)| accounting.balance(ticket).unwrap().remaining_ticks()
                        < *previous
                ));
            }
        }
        assert!(!inputs.diagnostics.has_errors());
        assert!(source_policy.borrow().contains(&written));
    });
}

#[test]
fn first_body_written_request_uses_the_same_source_registry_on_later_signature_passes() {
    with_inputs(TEXT, |inputs, external| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let occurrence = return_occurrence(&inputs);
        let policy = RefCell::new(Vec::new());
        let ticket =
            BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|request| {
                policy.borrow_mut().push(request);
                ALLOWANCE
            })
            .written_ticket(occurrence)
            .unwrap();
        let before = accounting.balance(&ticket).unwrap();
        let source_policy = RefCell::new(Vec::new());
        let source_phase = crate::resolve_declaration_signatures_with_work(
            inputs.collection,
            inputs.imports,
            inputs.sources,
            &Support,
            SignatureResolutionInputs::new(Some(external), None),
            inputs.diagnostics,
            SignatureAnalysisWork::new(&mut accounting, &mut sources, &|occurrence| {
                source_policy.borrow_mut().push(occurrence);
                ALLOWANCE
            }),
        )
        .unwrap();
        assert!(matches!(source_phase, SignatureExecution::Completed(_)));
        assert!(!source_policy.borrow().contains(&occurrence));
        assert_eq!(sources.admitted_ticket(occurrence).unwrap(), Some(&ticket));
        assert_eq!(
            policy.borrow().as_slice(),
            [BodyAnalysisInput::WrittenType(occurrence)]
        );
        assert!(accounting.balance(&ticket).unwrap().remaining_ticks() < before.remaining_ticks());
    });
}

#[test]
fn invalid_and_unadmitted_call_requests_do_not_invoke_policy() {
    with_inputs(TEXT, |inputs, _| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let call = first_call(&inputs);
        let count = Cell::new(0);
        let policy = |_| {
            count.set(count.get() + 1);
            ALLOWANCE
        };
        let mut work = BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &policy);
        assert!(
            matches!(work.argument_ticket(call, 0), Err(AnalysisCallRegistryFailure::UnadmittedCall(found)) if found == call)
        );
        assert_eq!(count.get(), 0);
        let selection = work.selection_ticket(call).unwrap();
        assert_eq!(count.get(), 1);
        assert!(matches!(
            work.argument_ticket(call, 9),
            Err(AnalysisCallRegistryFailure::Input(
                AnalysisCallInputFailure::UnknownArgument { index: 9, .. }
            ))
        ));
        assert_eq!(count.get(), 1);
        assert_eq!(work.selection_ticket(call).unwrap(), selection);
        assert_eq!(count.get(), 1);
        assert!(work.accounting().incomplete().is_none());
    });
}

#[test]
fn invalid_original_type_or_foreign_attempt_does_not_invoke_policy() {
    with_inputs("fn inspect(x: (i32, Bool)) -> Unit = ()", |inputs, _| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let child = inputs.sources[0]
            .hir()
            .unwrap()
            .type_refs()
            .find_map(|(_, ty)| match ty.kind() {
                eglur_hir::TypeRefKind::Tuple(children) => children.first().copied(),
                _ => None,
            })
            .unwrap();
        let occurrence = TypeOccurrenceId::new(inputs.sources[0].file(), child);
        let never = |_| panic!("invalid input invoked allowance policy");
        let mut work = BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &never);
        assert!(
            matches!(work.written_ticket(occurrence), Err(AnalysisSourceInputFailure::NotOriginalType(found)) if found == occurrence)
        );
        let occurrence = return_occurrence(&inputs);
        let mut foreign = super::tests::accounting();
        let mut work = BodyAnalysisWork::new(&mut foreign, &mut sources, &mut bodies, &never);
        assert!(matches!(
            work.written_ticket(occurrence),
            Err(AnalysisSourceInputFailure::Work(
                AnalysisWorkFailure::Claim(AnalysisRootClaimError::ForeignAttempt)
            ))
        ));
    });
}

#[test]
fn actual_body_entry_rejects_foreign_source_or_call_attempts_without_policy() {
    for foreign_calls in [false, true] {
        with_inputs(TEXT, |mut inputs, external| {
            let owner = accounting();
            let mut active = accounting();
            let (mut owner_sources, mut owner_bodies) = analysis_inputs(&owner, &inputs);
            let (mut active_sources, mut active_bodies) = analysis_inputs(&active, &inputs);
            let (sources, bodies) = if foreign_calls {
                (&mut active_sources, &mut owner_bodies)
            } else {
                (&mut owner_sources, &mut active_bodies)
            };
            let result = run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut active, sources, bodies, &|_| {
                    panic!("invalid attempt invoked policy")
                }),
            );
            if foreign_calls {
                let Err(BodyTypingError::CallInputs(error)) = result else {
                    panic!("foreign call inputs accepted")
                };
                assert_eq!(
                    error,
                    AnalysisCallRegistryFailure::Input(AnalysisCallInputFailure::Work(
                        AnalysisWorkFailure::Claim(AnalysisRootClaimError::ForeignAttempt)
                    ))
                );
            } else {
                assert!(matches!(
                    result,
                    Err(BodyTypingError::SourceInputs(
                        AnalysisSourceInputFailure::Work(AnalysisWorkFailure::Claim(
                            AnalysisRootClaimError::ForeignAttempt
                        ))
                    ))
                ));
            }
            assert!(!inputs.diagnostics.has_errors());
            assert!(active.incomplete().is_none());
        });
    }
}

#[test]
fn identical_local_ids_from_another_hir_are_rejected_at_actual_phase_entry() {
    for foreign_calls in [false, true] {
        with_inputs(TEXT, |mut inputs, external| {
            let mut accounting = accounting();
            let original = &inputs.sources[0];
            let hir = original.hir().unwrap().clone();
            let alternate = SemanticSource::new(
                original.file(),
                original.module().clone(),
                Some(&hir),
                hir.origins()
                    .map(|(origin, _)| (origin, original.origin_span(origin).unwrap().clone())),
                [],
                None,
            )
            .unwrap();
            let (current_sources, current_bodies) = analysis_inputs(&accounting, &inputs);
            let alternate_sources =
                AnalysisSourceInputs::new(&accounting, [(0, &alternate)]).unwrap();
            let alternate_bodies = AnalysisBodyInputs::new(
                &accounting,
                [(0, inputs.collection.modules().next().unwrap(), &alternate)],
            )
            .unwrap();
            let (mut sources, mut bodies) = if foreign_calls {
                (current_sources, alternate_bodies)
            } else {
                (alternate_sources, current_bodies)
            };
            let result = run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
                    panic!("stale HIR invoked policy")
                }),
            );
            if foreign_calls {
                assert!(matches!(
                    result,
                    Err(BodyTypingError::CallInputs(
                        AnalysisCallRegistryFailure::SourceMismatch { .. }
                    ))
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(BodyTypingError::SourceInputs(
                        AnalysisSourceInputFailure::SourceMismatch(_)
                    ))
                ));
            }
            assert!(!inputs.diagnostics.has_errors());
        });
    }
}

#[test]
fn actual_phase_preserves_a_registry_stop_and_completed_prefix() {
    for kind in 0..3 {
        with_inputs(TEXT, |mut inputs, external| {
            let mut accounting = accounting();
            let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
            let call = first_call(&inputs);
            let request = match kind {
                0 => BodyAnalysisInput::WrittenType(return_occurrence(&inputs)),
                1 => BodyAnalysisInput::CallSelection(call),
                _ => BodyAnalysisInput::CallArgument { call, index: 0 },
            };
            if kind == 2 {
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| ALLOWANCE)
                    .selection_ticket(call)
                    .unwrap();
            }
            let _recording = record(call, vec![request]);
            let result = run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|input| {
                    if input == request { 0 } else { ALLOWANCE }
                }),
            )
            .unwrap();
            let BodyExecution::Interrupted(stopped) = result else {
                panic!("registry refusal completed")
            };
            assert_eq!(Some(stopped.stop()), accounting.incomplete());
            assert!(stopped.prefix().fact_count() > 0);
            assert!(matches!(
                stopped.prefix().expression(call),
                FactState::Unperformed
            ));
            assert!(!inputs.diagnostics.has_errors());
            assert!(stopped.internal_failures().is_empty());
        });
    }
}

#[test]
fn body_input_setup_rejects_duplicate_modules_without_admitting_roots() {
    with_inputs(TEXT, |inputs, _| {
        let mut accounting = accounting();
        let module = inputs.collection.modules().next().unwrap();
        let source = &inputs.sources[0];
        assert!(matches!(
            AnalysisBodyInputs::new(&accounting, [(0, module, source), (0, module, source)]),
            Err(crate::AnalysisBodyInputFailure::Call(AnalysisCallRegistryFailure::DuplicateModule(found))) if found == module.id()
        ));
        let ticket = accounting.admit(0, ALLOWANCE).unwrap();
        assert_eq!(ticket.identity().requirement(), 0);
        assert!(accounting.incomplete().is_none());
    });
}

#[test]
fn body_input_owner_retains_proof_tickets_without_changing_semantic_fingerprints() {
    with_inputs(
        "trait Mark {}\nfn id[T: Mark](value: T) -> T = value\nfn inspect[U: Mark](value: U) -> U = id(value)",
        |mut inputs, external| {
            let mut accounting = accounting();
            let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
            let call = first_call(&inputs);
            let work =
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| ALLOWANCE);
            let product = completed(run(&mut inputs, external, work).unwrap());
            let before = fingerprints(&mut inputs, &product);
            let parameter = inputs
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .find(|declaration| declaration.name().is_some_and(|name| name.text() == "T"))
                .unwrap()
                .id();
            let FactState::Known(parameter) = inputs
                .declarations
                .signatures()
                .generic_parameter_record(&GenericParameterId::Source(parameter))
            else {
                panic!("original bound definition")
            };
            let bound = bodies
                .bound_ticket(&mut accounting, call, parameter, 0, || ALLOWANCE)
                .unwrap();
            assert_eq!(
                bodies
                    .bound_ticket(&mut accounting, call, parameter, 0, || panic!(
                        "bound replay"
                    ))
                    .unwrap(),
                bound
            );
            assert_eq!(
                bodies
                    .existing_bound_ticket(&mut accounting, call, parameter.id(), 0)
                    .unwrap(),
                bound
            );
            let dispatch_site = MethodDispatchSite::Invocation(call);
            let dispatch = bodies
                .dispatch_ticket(&mut accounting, dispatch_site, || ALLOWANCE)
                .unwrap();
            assert_eq!(
                bodies
                    .dispatch_ticket(&mut accounting, dispatch_site, || panic!("dispatch replay"))
                    .unwrap(),
                dispatch
            );
            assert_eq!(
                bodies
                    .existing_dispatch_ticket(&mut accounting, dispatch_site)
                    .unwrap(),
                dispatch
            );
            assert_ne!(bound, dispatch);
            assert_eq!(fingerprints(&mut inputs, &product), before);
            assert!(!inputs.diagnostics.has_errors());
        },
    );
}

#[test]
fn actual_body_entry_rejects_a_missing_module_without_policy() {
    with_inputs(TEXT, |mut inputs, external| {
        let mut accounting = accounting();
        let (mut sources, _) = analysis_inputs(&accounting, &inputs);
        let mut bodies = AnalysisBodyInputs::new(&accounting, []).unwrap();
        let module = inputs.collection.modules().next().unwrap().id();
        assert!(matches!(
            run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
                    panic!("missing module invoked policy")
                }),
            ),
            Err(BodyTypingError::CallInputs(AnalysisCallRegistryFailure::MissingModule(found)))
                if found == module
        ));
        assert!(!inputs.diagnostics.has_errors());
        assert!(accounting.incomplete().is_none());
    });
}

#[test]
fn matching_local_call_ordinals_in_distinct_bodies_have_distinct_retained_roots() {
    with_inputs(
        "fn id(x: i32) -> i32 = x\nfn first(x: i32) -> i32 = id(x)\nfn second(x: i32) -> i32 = id(x)",
        |inputs, _| {
            let mut accounting = accounting();
            let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
            let module = inputs.collection.modules().next().unwrap().id();
            let calls = inputs.sources[0]
                .hir()
                .unwrap()
                .bodies()
                .flat_map(|(_, body)| body.expressions())
                .filter_map(|(id, expression)| {
                    matches!(expression.kind(), ExprKind::Call(_))
                        .then_some(ExpressionKey::new(module, id))
                })
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[0].expression().local(), calls[1].expression().local());
            assert_ne!(calls[0].expression().body(), calls[1].expression().body());
            let policy_calls = Cell::new(0);
            let policy = |_| {
                policy_calls.set(policy_calls.get() + 1);
                ALLOWANCE
            };
            let mut work =
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &policy);
            let first = work.selection_ticket(calls[0]).unwrap();
            let second = work.selection_ticket(calls[1]).unwrap();
            assert_eq!(first.identity().requirement(), 0);
            assert_ne!(first, second);
            assert_eq!(work.selection_ticket(calls[1]).unwrap(), second);
            assert_eq!(work.selection_ticket(calls[0]).unwrap(), first);
            assert_eq!(policy_calls.get(), 2);
        },
    );
}

#[test]
fn preexisting_stop_wins_over_missing_body_inputs_at_actual_entry() {
    with_inputs(TEXT, |mut inputs, external| {
        let mut accounting = accounting();
        let mut sources = AnalysisSourceInputs::new(&accounting, []).unwrap();
        let mut bodies = AnalysisBodyInputs::new(&accounting, []).unwrap();
        accounting.admit(0, 0).unwrap_err();
        let stop = accounting.incomplete().unwrap();
        let result = run(
            &mut inputs,
            external,
            BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
                panic!("stopped attempt invoked policy")
            }),
        )
        .unwrap();
        let BodyExecution::Interrupted(stopped) = result else {
            panic!("stopped attempt completed")
        };
        assert_eq!(stopped.stop(), stop);
        assert_eq!(stopped.prefix().fact_count(), 0);
        assert!(!inputs.diagnostics.has_errors());
    });
}

#[test]
fn inherited_requests_preserve_the_existing_balance_without_policy() {
    with_inputs(TEXT, |inputs, _| {
        let mut accounting = accounting();
        let ticket = accounting.admit(0, ALLOWANCE).unwrap();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let before = accounting.balance(&ticket).unwrap();
        let mut work = BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
            panic!("inheritance invoked policy")
        });
        assert_eq!(
            work.inherited_ticket(inputs.sources[0].file(), &ticket)
                .unwrap(),
            ticket
        );
        let after = work.accounting().balance(&ticket).unwrap();
        assert_eq!(before.allowance_ticks(), after.allowance_ticks());
        assert!(after.remaining_ticks() < before.remaining_ticks());
    });
}
