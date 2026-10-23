use std::cell::RefCell;

use super::*;
use crate::{
    AnalysisRootStopReason, AnalysisRootTicket, AnalysisWork, AnalysisWorkAccounting,
    AnalysisWorkPricing,
};

mod facts;
mod sweep;
pub(in crate::body_types::checker) mod sweep_support;

#[derive(Clone, Copy)]
enum InjectedFailure {
    Exhaustion,
    Overflow,
    MissingCause,
    Internal,
    InternalAfterStop,
}

struct Probe {
    target: StopAt,
    ticket: AnalysisRootTicket,
    failure: InjectedFailure,
    corrupt_cleanup: bool,
    visited: Vec<ExpressionKey>,
}

#[derive(Clone, Copy)]
enum StopAt {
    Ordinal(usize),
    Expression(ExpressionKey),
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
    sweep_support::expression(key);
    PROBE.with_borrow_mut(|probe| {
        let Some(probe) = probe else {
            return Ok(());
        };
        probe.visited.push(key);
        let stop_here = match probe.target {
            StopAt::Ordinal(ordinal) => probe.visited.len() == ordinal,
            StopAt::Expression(expression) => expression == key,
        };
        if !stop_here {
            return Ok(());
        }
        let result = match probe.failure {
            InjectedFailure::MissingCause => Err(BodyFlow::Interrupted),
            InjectedFailure::Internal => Err(BodyTypingError::InvalidIntegerConstant.into()),
            InjectedFailure::Exhaustion
            | InjectedFailure::Overflow
            | InjectedFailure::InternalAfterStop => {
                let work = AnalysisWork {
                    steps: if matches!(probe.failure, InjectedFailure::Overflow) {
                        u64::MAX - 1
                    } else {
                        1_000_000
                    },
                    inspected_bytes: if matches!(probe.failure, InjectedFailure::Overflow) {
                        2
                    } else {
                        0
                    },
                    transferred_bytes: 0,
                    reserved_bytes: 0,
                };
                let charged = checker
                    .analysis_work
                    .as_mut()
                    .unwrap()
                    .accounting
                    .claim(&probe.ticket)
                    .and_then(|mut scope| scope.precharge(work))
                    .map_err(BodyFlow::from);
                if matches!(probe.failure, InjectedFailure::InternalAfterStop) {
                    assert!(matches!(charged, Err(BodyFlow::Interrupted)));
                    Err(BodyTypingError::InvalidIntegerConstant.into())
                } else {
                    charged
                }
            }
        };
        if probe.corrupt_cleanup {
            checker.region_stack.clear();
        }
        result
    })
}

fn accounting() -> (AnalysisWorkAccounting, AnalysisRootTicket) {
    let pricing = AnalysisWorkPricing::for_test(1, 1, 1).unwrap();
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let ticket = accounting.admit(0, 100_000).unwrap();
    (accounting, ticket)
}

fn record(
    ordinal: usize,
    ticket: AnalysisRootTicket,
    failure: InjectedFailure,
    corrupt_cleanup: bool,
) -> Recording {
    PROBE.with_borrow_mut(|probe| {
        assert!(probe.is_none());
        *probe = Some(Probe {
            target: StopAt::Ordinal(ordinal),
            ticket,
            failure,
            corrupt_cleanup,
            visited: Vec::new(),
        });
    });
    Recording
}

fn with_execution(
    text: &str,
    work: Option<&mut AnalysisWorkAccounting>,
    inspect: impl FnOnce(BodyChecker<'_, '_, '_>),
) {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    super::super::test_support::with_checker_owned(text, &external, None, work, inspect);
}

fn owner(checker: &BodyChecker<'_, '_, '_>, name: &str) -> DefId {
    checker
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.name().is_some_and(|n| n.text() == name))
        .unwrap()
        .id()
}

fn interrupted(execution: BodyExecution) -> Box<InterruptedBody> {
    match execution {
        BodyExecution::Interrupted(value) => value,
        BodyExecution::Completed(_) => panic!("stopped execution was completed"),
    }
}

#[test]
fn nested_stop_preserves_completed_prefix_without_finishing_or_visiting_later_bodies() {
    let (mut work, ticket) = accounting();
    let _recording = record(4, ticket, InjectedFailure::Exhaustion, false);
    with_execution(
        "fn first(x: i32) -> i32 = x\nfn inspect(x: i32) -> i32 {\nlet y: i32 = { yield x }\nyield y\n}\nfn later(x: i32) -> i32 = x",
        Some(&mut work),
        |mut checker| {
            assert!(checker.recorder.require(
                CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
                SemanticLocation::source(FileId::new(0)),
            ));
            let first = owner(&checker, "first");
            let current = owner(&checker, "inspect");
            let later = owner(&checker, "later");
            let traversal = checker.check_all();
            assert_eq!(traversal, Err(BodyFlow::Interrupted));
            assert!(checker.region_stack.is_empty());
            assert_eq!(checker.current_owner, None);
            assert_eq!(checker.current_return, None);
            let execution = checker.complete_execution(traversal).unwrap();
            let stopped = interrupted(execution);
            assert!(matches!(stopped.prefix().body(first), FactState::Known(_)));
            assert!(matches!(
                stopped.prefix().body(current),
                FactState::Unperformed
            ));
            assert!(matches!(
                stopped.prefix().body(later),
                FactState::Unperformed
            ));
            assert!(stopped.prefix().fact_count() > 0);
            assert!(!stopped.demands().is_empty());
            assert!(stopped.internal_failures().is_empty());
            let visited = PROBE.with_borrow(|probe| probe.as_ref().unwrap().visited.clone());
            assert_eq!(visited.len(), 4);
            assert!(matches!(
                stopped.prefix().expression(*visited.last().unwrap()),
                FactState::Unperformed
            ));
            assert!(matches!(
                stopped.stop().reason(),
                AnalysisRootStopReason::Exhausted { .. }
            ));
        },
    );
    assert!(work.incomplete().is_some());
}

#[test]
fn callable_unwind_preserves_an_internal_flow_that_coexists_with_a_sticky_stop() {
    let (mut work, ticket) = accounting();
    let _recording = record(2, ticket, InjectedFailure::InternalAfterStop, true);
    with_execution(
        "fn inspect(x: i32) -> i32 { yield x }",
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            assert_eq!(
                traversal,
                Err(BodyFlow::Internal(BodyTypingError::InvalidIntegerConstant))
            );
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(
                stopped
                    .internal_failures()
                    .contains(&BodyTypingError::InvalidIntegerConstant)
            );
            assert!(
                stopped
                    .internal_failures()
                    .contains(&BodyTypingError::MissingControlRegion)
            );
        },
    );
}

struct AllSupported;

impl CompileSupport for AllSupported {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

#[test]
fn generic_argument_probe_stop_is_not_a_failed_candidate_or_source_diagnostic() {
    let (mut work, ticket) = accounting();
    let _recording = record(4, ticket, InjectedFailure::Exhaustion, false);
    with_execution(
        "fn id[T](value: T) -> T = value\nfn inspect(x: i32) -> Unit {\nlet y = id(x)\n}",
        Some(&mut work),
        |mut checker| {
            let current = owner(&checker, "inspect");
            let traversal = checker.check_all();
            assert_eq!(traversal, Err(BodyFlow::Interrupted));
            assert!(!checker.diagnostics.has_errors());
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(matches!(
                stopped.prefix().body(current),
                FactState::Unperformed
            ));
            assert!(stopped.internal_failures().is_empty());
        },
    );
}

#[test]
fn stopped_prefix_keeps_completed_callable_instance_backing_and_transfer_order() {
    let (mut work, ticket) = accounting();
    let _recording = record(usize::MAX, ticket, InjectedFailure::Exhaustion, false);
    with_execution(
        "fn leaf(x: i32) -> i32 = x\nfn first(x: i32) -> i32 { return leaf(x) }\nfn inspect(x: i32) -> i32 { yield { yield x } }",
        Some(&mut work),
        |mut checker| {
            let first = owner(&checker, "first");
            let (module, _, hir, item) = checker.source_item(owner(&checker, "inspect")).unwrap();
            let ItemKind::Function(function) = item.kind() else {
                panic!("function required");
            };
            let (expression, _) = hir
                .body(function.body)
                .expressions()
                .find(|(_, e)| matches!(e.kind(), ExprKind::Name(_)))
                .unwrap();
            let key = ExpressionKey::new(module, expression);
            PROBE.with_borrow_mut(|probe| probe.as_mut().unwrap().target = StopAt::Expression(key));
            let traversal = checker.check_all();
            assert_eq!(traversal, Err(BodyFlow::Interrupted));
            let instances = checker
                .callable_instances
                .iter()
                .map(|v| (v.id(), v.signature(), v.substitution()))
                .collect::<Vec<_>>();
            assert!(!instances.is_empty());
            let transfers = checker.transfers.clone();
            assert!(!transfers.is_empty());
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(matches!(stopped.prefix().body(first), FactState::Known(_)));
            for (id, signature, substitution) in instances {
                let retained = stopped.prefix().callable_instance(id).unwrap();
                assert_eq!(retained.signature(), signature);
                assert_eq!(retained.substitution(), substitution);
            }
            assert_eq!(stopped.prefix().transfers(), transfers);
            assert!(matches!(
                stopped.prefix().expression(key),
                FactState::Unperformed
            ));
        },
    );
}

#[test]
fn accounting_entry_and_unmetered_entry_have_identical_completed_products() {
    let text = "fn id(x: i32) -> i32 = x\nfn inspect(x: i32) -> i32 = id(x)";
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let run = |work: Option<&mut AnalysisWorkAccounting>| {
        let mut snapshot = None;
        super::super::test_support::with_phase_inputs(text, &external, None, |inputs| {
            let phase = match work {
                Some(work) => {
                    let (mut source_inputs, mut body_inputs) =
                        super::super::test_support::analysis_inputs(work, &inputs);
                    match typecheck_bodies_with_work(
                        inputs.collection,
                        inputs.imports,
                        inputs.names,
                        Some(&external),
                        inputs.sources,
                        &AllSupported,
                        inputs.declarations,
                        inputs.trait_environment,
                        inputs.diagnostics,
                        BodyAnalysisWork::new(work, &mut source_inputs, &mut body_inputs, &|_| {
                            100_000
                        }),
                    )
                    .unwrap()
                    {
                        BodyExecution::Completed(packet) => packet.into_parts().0,
                        BodyExecution::Interrupted(_) => panic!("no stop was requested"),
                    }
                }
                None => typecheck_bodies(
                    inputs.collection,
                    inputs.imports,
                    inputs.names,
                    Some(&external),
                    inputs.sources,
                    &AllSupported,
                    inputs.declarations,
                    inputs.trait_environment,
                    inputs.diagnostics,
                )
                .unwrap(),
            };
            let (product, demands) = phase.into_parts();
            let PhaseProduct::Performed(body) = product else {
                panic!("body phase was not performed");
            };
            assert!(!inputs.diagnostics.has_errors());
            let expressions = body
                .expressions
                .iter()
                .map(|(key, fact)| (*key, format!("{fact:?}")))
                .collect::<Vec<_>>();
            let summaries = body
                .bodies
                .iter()
                .map(|(key, fact)| (*key, format!("{fact:?}")))
                .collect::<Vec<_>>();
            snapshot = Some((body.fact_count(), expressions, summaries, demands));
        });
        snapshot.unwrap()
    };
    let (mut work, ticket) = accounting();
    let before = work.balance(&ticket).unwrap();
    assert_eq!(run(None), run(Some(&mut work)));
    assert_eq!(work.balance(&ticket).unwrap(), before);
    assert!(work.incomplete().is_none());
}

#[test]
fn accounting_entry_refuses_a_preexisting_stop_before_new_capability_or_source_work() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let (mut work, ticket) = accounting();
    super::super::test_support::with_phase_inputs(
        "fn inspect(x: i32) -> i32 = x",
        &external,
        None,
        |inputs| {
            let (mut source_inputs, mut body_inputs) =
                super::super::test_support::analysis_inputs(&work, &inputs);
            work.claim(&ticket)
                .unwrap()
                .precharge(AnalysisWork {
                    steps: u64::MAX,
                    inspected_bytes: 0,
                    transferred_bytes: 0,
                    reserved_bytes: 0,
                })
                .unwrap_err();
            let original = work.incomplete().unwrap();
            let stopped = interrupted(
                typecheck_bodies_with_work(
                    inputs.collection,
                    inputs.imports,
                    inputs.names,
                    Some(&external),
                    inputs.sources,
                    &AllSupported,
                    inputs.declarations,
                    inputs.trait_environment,
                    inputs.diagnostics,
                    BodyAnalysisWork::new(&mut work, &mut source_inputs, &mut body_inputs, &|_| {
                        100_000
                    }),
                )
                .unwrap(),
            );
            assert_eq!(stopped.stop(), original);
            assert_eq!(stopped.prefix().fact_count(), 0);
            assert!(stopped.demands().is_empty());
            assert!(!inputs.diagnostics.has_errors());
        },
    );
}

#[test]
fn public_accounting_entry_propagates_a_deep_source_stop_and_retains_its_prefix() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let (mut work, ticket) = accounting();
    let _recording = record(4, ticket, InjectedFailure::Exhaustion, false);
    super::super::test_support::with_phase_inputs(
        "fn first(x: i32) -> i32 = x\nfn inspect(x: i32) -> i32 { yield { yield x } }",
        &external,
        None,
        |inputs| {
            let (mut source_inputs, mut body_inputs) =
                super::super::test_support::analysis_inputs(&work, &inputs);
            let stopped = interrupted(
                typecheck_bodies_with_work(
                    inputs.collection,
                    inputs.imports,
                    inputs.names,
                    Some(&external),
                    inputs.sources,
                    &AllSupported,
                    inputs.declarations,
                    inputs.trait_environment,
                    inputs.diagnostics,
                    BodyAnalysisWork::new(&mut work, &mut source_inputs, &mut body_inputs, &|_| {
                        100_000
                    }),
                )
                .unwrap(),
            );
            assert!(stopped.prefix().fact_count() > 0);
            assert!(!stopped.demands().is_empty());
            assert!(stopped.internal_failures().is_empty());
            assert!(!inputs.diagnostics.has_errors());
        },
    );
}

#[test]
fn result_contexts_unwind_without_publishing_stopped_parents() {
    {
        let text = "fn inspect(x: Opt[i32]) -> Opt[i32] = result { yield x }";
        let (mut work, ticket) = accounting();
        let _recording = record(3, ticket, InjectedFailure::Exhaustion, false);
        with_execution(text, Some(&mut work), |mut checker| {
            let traversal = checker.check_all();
            assert_eq!(traversal, Err(BodyFlow::Interrupted));
            assert!(checker.region_stack.is_empty());
            assert!(checker.result_contexts.is_empty());
            assert!(checker.checked_contexts.is_empty());
            assert_eq!(checker.question_floors, [0]);
            assert!(checker.block_region_overrides.is_empty());
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(stopped.internal_failures().is_empty());
        });
    }
}

#[test]
fn checked_and_numeric_preparation_contexts_unwind_on_stop() {
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
    let (mut work, ticket) = accounting();
    let _recording = record(4, ticket, InjectedFailure::Exhaustion, false);
    super::super::test_support::with_checker_owned(
        "fn inspect(x: i8, y: i8) -> Res[i8, NumericError] = checked { yield x + y }",
        &external,
        Some(inputs),
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            assert_eq!(traversal, Err(BodyFlow::Interrupted));
            assert!(checker.region_stack.is_empty());
            assert!(checker.checked_contexts.is_empty());
            assert_eq!(checker.question_floors, [0]);
            assert!(!checker.preparing_numeric_tree);
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(stopped.internal_failures().is_empty());
        },
    );
}

#[test]
fn sticky_stop_wins_over_a_swallowed_marker_or_internal_flow() {
    for traversal in [Ok(()), Err(BodyTypingError::InvalidIntegerConstant.into())] {
        let (mut work, ticket) = accounting();
        with_execution(
            "fn inspect(x: i32) -> i32 = x",
            Some(&mut work),
            |mut checker| {
                let accounting = &mut *checker.analysis_work.as_mut().unwrap().accounting;
                accounting
                    .claim(&ticket)
                    .unwrap()
                    .precharge(AnalysisWork {
                        steps: u64::MAX,
                        inspected_bytes: 0,
                        transferred_bytes: 0,
                        reserved_bytes: 0,
                    })
                    .unwrap_err();
                let original = accounting.incomplete().unwrap();
                let has_internal = traversal.is_err();
                let stopped = interrupted(checker.complete_execution(traversal).unwrap());
                assert_eq!(stopped.stop(), original);
                assert_eq!(stopped.internal_failures().len(), usize::from(has_internal));
                assert_eq!(stopped.prefix().fact_count(), 0);
            },
        );
    }
}

#[test]
fn resource_stop_keeps_cleanup_defects_without_turning_them_into_source_errors() {
    let (mut work, ticket) = accounting();
    let _recording = record(3, ticket, InjectedFailure::Exhaustion, true);
    with_execution(
        "fn inspect(x: i32) -> i32 { yield { yield x } }",
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            assert!(checker.current_return.is_none());
            assert!(!checker.diagnostics.has_errors());
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(stopped.internal_failures().len() >= 2);
            assert!(
                stopped
                    .internal_failures()
                    .iter()
                    .all(|e| matches!(e, BodyTypingError::MissingControlRegion))
            );
        },
    );
}

#[test]
fn marker_without_sticky_cause_is_an_internal_error_not_an_interrupted_product() {
    let (mut work, ticket) = accounting();
    let _recording = record(2, ticket, InjectedFailure::MissingCause, false);
    with_execution(
        "fn inspect(x: i32) -> i32 { yield x }",
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            assert_eq!(traversal, Err(BodyFlow::Interrupted));
            assert!(matches!(
                checker.complete_execution(traversal),
                Err(BodyTypingError::MissingAnalysisStop)
            ));
        },
    );
    assert!(work.incomplete().is_none());
}

#[test]
fn internal_failure_without_stop_remains_in_the_error_channel() {
    let (mut work, ticket) = accounting();
    let _recording = record(2, ticket, InjectedFailure::Internal, false);
    with_execution(
        "fn inspect(x: i32) -> i32 { yield x }",
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            assert!(matches!(
                checker.complete_execution(traversal),
                Err(BodyTypingError::InvalidIntegerConstant)
            ));
        },
    );
    assert!(work.incomplete().is_none());
}

#[test]
fn unrepresentable_price_is_a_sticky_body_interruption() {
    let (mut work, ticket) = accounting();
    let _recording = record(1, ticket, InjectedFailure::Overflow, false);
    with_execution(
        "fn inspect(x: i32) -> i32 = x",
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            let stopped = interrupted(checker.complete_execution(traversal).unwrap());
            assert!(matches!(
                stopped.stop().reason(),
                AnalysisRootStopReason::Unrepresentable(_)
            ));
        },
    );
}

#[test]
fn foreign_root_claim_is_an_internal_error_and_does_not_stop_the_attempt() {
    let (_, foreign) = accounting();
    let (mut work, _) = accounting();
    let _recording = record(1, foreign, InjectedFailure::Exhaustion, false);
    with_execution(
        "fn inspect(x: i32) -> i32 = x",
        Some(&mut work),
        |mut checker| {
            let traversal = checker.check_all();
            assert!(matches!(
                checker.complete_execution(traversal),
                Err(BodyTypingError::AnalysisRootClaim(
                    crate::AnalysisRootClaimError::ForeignAttempt
                ))
            ));
        },
    );
    assert!(work.incomplete().is_none());
}
