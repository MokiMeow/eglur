use crate::record_reservation::observation::{assert_growth_envelope, observe};
use crate::{AnalysisRootClaimError, AnalysisRootStopReason};

use super::super::tests::{inspect_body, parse, pricing, sites};
use super::*;

const ALLOWANCE: u64 = 1_000_000;

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(pricing(), 2).unwrap()
}

fn registry<'a>(
    accounting: &AnalysisWorkAccounting,
    body: &'a Body,
) -> AnalysisCallInputRegistry<'a> {
    AnalysisCallInputRegistry::new(accounting, 0, ModuleId::from_index(0).unwrap(), body).unwrap()
}

fn incomplete(error: AnalysisCallRegistryFailure) -> super::super::super::AnalysisWorkIncomplete {
    let AnalysisCallRegistryFailure::Input(AnalysisCallInputFailure::Work(
        AnalysisWorkFailure::Incomplete(incomplete),
    )) = error
    else {
        panic!("typed incomplete expected: {error:?}")
    };
    incomplete
}

fn selection_cost(body: &Body, site: ExpressionKey) -> u64 {
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let ticket = inputs
        .selection_ticket(&mut accounting, site, ALLOWANCE)
        .unwrap();
    accounting.balance(&ticket).unwrap().charged_ticks()
}

#[test]
fn setup_prepares_only_input_indices_without_admitting_any_roots() {
    let hir = parse("fn inspect(x: i32) -> i32 = outer(inner(x), x)");
    let body = inspect_body(&hir);
    let mut accounting = accounting();
    let inputs = registry(&accounting, body);
    assert_eq!(inputs.indices.len(), body.expressions().len());
    assert!(inputs.indices.iter().all(Option::is_none));
    assert!(inputs.holders.is_empty());
    let first = accounting.admit(0, ALLOWANCE).unwrap();
    assert_eq!(first.identity().requirement(), 0);
}

#[test]
fn selection_and_written_slots_reuse_the_same_balances_on_every_pass() {
    for count in [0, 1, 8, 32] {
        let hir = parse(&format!(
            "fn inspect(x: i32) -> Unit = target({})",
            vec!["x"; count].join(", ")
        ));
        let body = inspect_body(&hir);
        let site = sites(body)[0];
        let mut accounting = accounting();
        let mut inputs = registry(&accounting, body);
        let selection = inputs
            .selection_ticket(&mut accounting, site, ALLOWANCE)
            .unwrap();
        let arguments = (0..count)
            .map(|index| {
                inputs
                    .argument_ticket(&mut accounting, site, index, ALLOWANCE)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let before = accounting.balance(&selection).unwrap();

        for _ in 0..5 {
            assert_eq!(
                inputs.selection_ticket(&mut accounting, site, 0).unwrap(),
                selection
            );
            for (index, expected) in arguments.iter().enumerate() {
                assert_eq!(
                    &inputs
                        .argument_ticket(&mut accounting, site, index, u64::MAX)
                        .unwrap(),
                    expected
                );
                assert_eq!(
                    accounting.balance(expected).unwrap().allowance_ticks(),
                    ALLOWANCE
                );
            }
        }
        assert!(
            accounting.balance(&selection).unwrap().remaining_ticks() < before.remaining_ticks()
        );
        assert_eq!(inputs.holders.len(), 1);
        assert_eq!(
            accounting
                .admit(0, ALLOWANCE)
                .unwrap()
                .identity()
                .requirement(),
            count + 1
        );
    }
}

#[test]
fn argument_retrieval_does_not_implicitly_admit_a_selection() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    assert!(
        matches!(inputs.argument_ticket(&mut accounting, site, 0, ALLOWANCE), Err(AnalysisCallRegistryFailure::UnadmittedCall(found)) if found == site)
    );
    assert!(inputs.holders.is_empty());
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        0
    );
}

#[test]
fn invalid_source_shapes_module_body_and_local_indices_refuse_before_admission() {
    let hir = parse("fn inspect(x: i32) -> i32 = x.then(f)\nfn other(x: i32) -> i32 = f(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let other = hir
        .items()
        .find_map(|(_, item)| match item.kind() {
            eglur_hir::ItemKind::Function(function) if function.name.text() == Some("other") => {
                Some(hir.body(function.body))
            }
            _ => None,
        })
        .unwrap();
    let larger_hir = parse("fn inspect(x: i32) -> i32 = f(f(f(f(f(f(x))))))");
    let larger = inspect_body(&larger_hir);
    let stale = sites(larger)
        .into_iter()
        .max_by_key(|key| key.expression().local())
        .unwrap();
    assert_eq!(stale.expression().body(), body.id());
    assert!(stale.expression().local() as usize >= body.expressions().len());
    let value = body
        .expressions()
        .find(|(_, expression)| matches!(expression.kind(), ExprKind::Name(_)))
        .unwrap()
        .0;
    for key in [
        site,
        ExpressionKey::new(ModuleId::from_index(1).unwrap(), site.expression()),
        sites(other)[0],
        stale,
        ExpressionKey::new(site.module(), value),
    ] {
        let mut accounting = accounting();
        let mut inputs = registry(&accounting, body);
        assert!(
            matches!(inputs.selection_ticket(&mut accounting, key, ALLOWANCE), Err(AnalysisCallRegistryFailure::Input(AnalysisCallInputFailure::NotOriginalCall(found))) if found == key)
        );
        assert!(inputs.holders.is_empty());
        assert!(inputs.indices.iter().all(Option::is_none));
        assert_eq!(
            accounting
                .admit(0, ALLOWANCE)
                .unwrap()
                .identity()
                .requirement(),
            0
        );
    }
    let partial_hir = parse("fn inspect(x: i32) -> Unit = f(_, x)");
    let body = inspect_body(&partial_hir);
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    assert!(matches!(
        inputs.selection_ticket(&mut accounting, sites(body)[0], ALLOWANCE),
        Err(AnalysisCallRegistryFailure::Input(
            AnalysisCallInputFailure::NotOriginalCall(_)
        ))
    ));
}

#[test]
fn valid_local_calls_do_not_match_another_module_or_body() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)\nfn other(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let other = hir
        .items()
        .find_map(|(_, item)| match item.kind() {
            eglur_hir::ItemKind::Function(function) if function.name.text() == Some("other") => {
                Some(hir.body(function.body))
            }
            _ => None,
        })
        .unwrap();
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let other_module = ExpressionKey::new(ModuleId::from_index(1).unwrap(), site.expression());
    for key in [other_module, sites(other)[0]] {
        assert!(
            matches!(inputs.selection_ticket(&mut accounting, key, ALLOWANCE), Err(AnalysisCallRegistryFailure::Input(AnalysisCallInputFailure::NotOriginalCall(found))) if found == key)
        );
    }
    assert_eq!(
        inputs
            .selection_ticket(&mut accounting, site, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        0
    );
}

#[test]
fn distinct_original_calls_and_nested_calls_do_not_share_selection() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(target(x), target(x))");
    let body = inspect_body(&hir);
    let calls = sites(body);
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let tickets = calls
        .iter()
        .map(|site| {
            inputs
                .selection_ticket(&mut accounting, *site, ALLOWANCE)
                .unwrap()
        })
        .collect::<Vec<_>>();
    for (index, ticket) in tickets.iter().enumerate() {
        assert!(tickets[..index].iter().all(|other| other != ticket));
        assert_eq!(
            inputs
                .selection_ticket(&mut accounting, calls[index], 0)
                .unwrap(),
            *ticket
        );
    }
    assert_eq!(inputs.holders.len(), calls.len());
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        calls.len()
    );
}

#[test]
fn explicit_and_trailing_calls_reuse_only_their_actual_written_slots() {
    for expression in [
        "target::[i32](x)",
        "target { |y| y }",
        "target(x) { |y| y }",
    ] {
        let hir = parse(&format!("fn inspect(x: i32) -> Unit = {expression}"));
        let body = inspect_body(&hir);
        let mut accounting = accounting();
        let mut inputs = registry(&accounting, body);
        let calls = sites(body);
        let mut count = 0;
        for site in &calls {
            let selected = inputs
                .selection_ticket(&mut accounting, *site, ALLOWANCE)
                .unwrap();
            assert_eq!(
                inputs.selection_ticket(&mut accounting, *site, 0).unwrap(),
                selected
            );
            let written = original_call(body, *site).unwrap().arguments.len();
            for index in 0..written {
                let ticket = inputs
                    .argument_ticket(&mut accounting, *site, index, ALLOWANCE)
                    .unwrap();
                assert_eq!(
                    inputs
                        .argument_ticket(&mut accounting, *site, index, 0)
                        .unwrap(),
                    ticket
                );
            }
            count += written + 1;
        }
        assert_eq!(
            accounting
                .admit(0, ALLOWANCE)
                .unwrap()
                .identity()
                .requirement(),
            count
        );
        assert_eq!(inputs.holders.len(), calls.len());
    }
}

#[test]
fn late_and_changed_contexts_are_loans_not_cached_call_identity() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let selection = inputs
        .selection_ticket(&mut accounting, site, ALLOWANCE)
        .unwrap();
    let first = accounting.admit(1, ALLOWANCE).unwrap();
    let second = accounting.admit(1, ALLOWANCE).unwrap();
    let before = accounting.balance(&first).unwrap();
    assert_eq!(
        inputs.contextual_ticket(&mut accounting, &first).unwrap(),
        first
    );
    assert_eq!(
        inputs.contextual_ticket(&mut accounting, &second).unwrap(),
        second
    );
    assert_eq!(
        inputs.contextual_ticket(&mut accounting, &first).unwrap(),
        first
    );
    assert!(accounting.balance(&first).unwrap().remaining_ticks() < before.remaining_ticks());
    assert_eq!(
        inputs.selection_ticket(&mut accounting, site, 0).unwrap(),
        selection
    );
    assert!(inputs.holders[0].contextual_result_ticket().is_none());
    assert_eq!(
        accounting
            .admit(1, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        2
    );
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        1
    );
}

#[test]
fn foreign_attempts_are_not_source_errors_or_new_roots() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut first = accounting();
    let mut second = accounting();
    let foreign = second.admit(0, ALLOWANCE).unwrap();
    let mut inputs = registry(&first, body);
    for result in [
        inputs.selection_ticket(&mut second, site, ALLOWANCE),
        inputs.contextual_ticket(&mut first, &foreign),
    ] {
        assert!(matches!(
            result,
            Err(AnalysisCallRegistryFailure::Input(
                AnalysisCallInputFailure::Work(AnalysisWorkFailure::Claim(
                    AnalysisRootClaimError::ForeignAttempt
                ))
            ))
        ));
    }
    assert!(inputs.holders.is_empty());
    assert_eq!(first.incomplete(), None);
    assert_eq!(second.incomplete(), None);
}

#[test]
fn insufficient_selection_allowance_publishes_neither_holder_nor_index() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let cost = selection_cost(body, site);
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let stop = incomplete(
        inputs
            .selection_ticket(&mut accounting, site, cost - 1)
            .unwrap_err(),
    );
    assert!(
        matches!(stop.reason(), AnalysisRootStopReason::Exhausted { required_ticks, .. } if required_ticks == cost)
    );
    assert!(inputs.holders.is_empty());
    assert!(inputs.indices.iter().all(Option::is_none));
    assert_eq!(
        incomplete(
            inputs
                .selection_ticket(&mut accounting, site, u64::MAX)
                .unwrap_err()
        ),
        stop
    );
    assert_eq!(accounting.incomplete(), Some(stop));
}

#[test]
fn slot_copy_is_prepaid_before_first_slot_publication() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let cost = {
        let mut accounting = accounting();
        let mut inputs = registry(&accounting, body);
        inputs
            .selection_ticket(&mut accounting, site, ALLOWANCE)
            .unwrap();
        let ticket = inputs
            .argument_ticket(&mut accounting, site, 0, ALLOWANCE)
            .unwrap();
        accounting.balance(&ticket).unwrap().charged_ticks()
    };
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let selection = inputs
        .selection_ticket(&mut accounting, site, ALLOWANCE)
        .unwrap();
    let prefix = accounting.balance(&selection).unwrap();
    let stop = incomplete(
        inputs
            .argument_ticket(&mut accounting, site, 0, cost - 1)
            .unwrap_err(),
    );
    assert!(inputs.holders[0].arguments[0].is_none());
    assert_eq!(accounting.balance(&selection).unwrap(), prefix);
    assert_eq!(
        incomplete(
            inputs
                .selection_ticket(&mut accounting, site, ALLOWANCE)
                .unwrap_err()
        ),
        stop
    );
}

#[test]
fn replay_cannot_refill_an_exhausted_selection_and_keeps_completed_prefix() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let ticket = inputs
        .selection_ticket(&mut accounting, site, selection_cost(body, site))
        .unwrap();
    assert_eq!(accounting.balance(&ticket).unwrap().remaining_ticks(), 0);
    let stop = incomplete(
        inputs
            .selection_ticket(&mut accounting, site, u64::MAX)
            .unwrap_err(),
    );
    assert_eq!(inputs.holders.len(), 1);
    assert_eq!(inputs.holders[0].selection_ticket(), &ticket);
    assert_eq!(accounting.balance(&ticket).unwrap().remaining_ticks(), 0);
    assert_eq!(
        incomplete(
            inputs
                .argument_ticket(&mut accounting, site, 0, ALLOWANCE)
                .unwrap_err()
        ),
        stop
    );
}

#[test]
fn invalid_argument_and_consumer_do_not_create_balances() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let mut accounting = accounting();
    assert!(matches!(
        AnalysisCallInputRegistry::new(&accounting, 2, site.module(), body),
        Err(AnalysisCallRegistryFailure::Input(
            AnalysisCallInputFailure::Work(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::UnknownConsumer
            ))
        ))
    ));
    let mut inputs = registry(&accounting, body);
    inputs
        .selection_ticket(&mut accounting, site, ALLOWANCE)
        .unwrap();
    assert!(matches!(
        inputs.argument_ticket(&mut accounting, site, 1, ALLOWANCE),
        Err(AnalysisCallRegistryFailure::Input(
            AnalysisCallInputFailure::UnknownArgument { index: 1, .. }
        ))
    ));
    assert_eq!(accounting.incomplete(), None);
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        1
    );
}

#[test]
fn lazy_holder_growth_is_covered_without_charging_unrelated_prefix_size() {
    let source = format!("fn inspect() -> Unit {{ {} }}", "target()\n".repeat(65));
    let hir = parse(&source);
    let body = inspect_body(&hir);
    let calls = sites(body);
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let mut observations = Vec::new();
    let mut charges = Vec::new();
    for site in calls {
        let (ticket, records) = observe(|| {
            inputs
                .selection_ticket(&mut accounting, site, ALLOWANCE)
                .unwrap()
        });
        charges.push(accounting.balance(&ticket).unwrap().charged_ticks());
        observations.extend(
            records
                .into_iter()
                .filter(|record| record.record_bytes == size_of::<AnalysisCallInputs>()),
        );
    }
    assert!(charges.iter().all(|charge| *charge == charges[0]));
    assert_eq!(observations.len(), inputs.holders.len());
    assert_growth_envelope(
        &observations,
        inputs.holders.len() * size_of::<AnalysisCallInputs>(),
    );
    let count = inputs.holders.len() as u64;
    let paid = AnalysisCallInputRegistry::installation_work().unwrap();
    let requested = observations
        .iter()
        .filter_map(|record| {
            record
                .requested_capacity
                .map(|capacity| (capacity * record.record_bytes) as u64)
        })
        .sum::<u64>();
    let relocated = observations
        .iter()
        .filter(|record| record.requested_capacity.is_some())
        .map(|record| (record.len * record.record_bytes) as u64)
        .sum::<u64>();
    assert!(requested <= count * paid.reserved_bytes);
    assert!(
        relocated + count * size_of::<Option<usize>>() as u64 <= count * paid.transferred_bytes
    );
}

#[test]
fn replay_ticket_copy_is_paid_before_returning_custody() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let expected = pricing()
        .price(AnalysisWork {
            steps: 3,
            inspected_bytes: (size_of::<Option<usize>>() + size_of::<AnalysisCallInputs>()) as u64,
            transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
            reserved_bytes: 0,
        })
        .unwrap()
        .ticks();
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let selection = inputs
        .selection_ticket(
            &mut accounting,
            site,
            selection_cost(body, site) + expected - 1,
        )
        .unwrap();
    let before = accounting.balance(&selection).unwrap();
    let stop = incomplete(
        inputs
            .selection_ticket(&mut accounting, site, u64::MAX)
            .unwrap_err(),
    );
    assert_eq!(stop.root(), selection.identity());
    assert_eq!(
        accounting.balance(&selection).unwrap().remaining_ticks(),
        before.remaining_ticks() - 7
    );
    assert_eq!(inputs.holders.len(), 1);
}

#[test]
fn price_overflow_stops_before_any_holder_publication() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let pricing = crate::AnalysisWorkPricing::for_test(u64::MAX, 1, 1).unwrap();
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let mut inputs = registry(&accounting, body);
    let stop = incomplete(
        inputs
            .selection_ticket(&mut accounting, site, u64::MAX)
            .unwrap_err(),
    );
    assert!(matches!(
        stop.reason(),
        AnalysisRootStopReason::Unrepresentable(AnalysisWorkOverflow::Price)
    ));
    assert!(inputs.holders.is_empty());
    assert!(inputs.indices.iter().all(Option::is_none));
    assert_eq!(
        incomplete(
            inputs
                .selection_ticket(&mut accounting, site, u64::MAX)
                .unwrap_err()
        ),
        stop
    );
}

#[test]
fn context_exhaustion_stops_the_attempt_without_modifying_call_balances() {
    let hir = parse("fn inspect(x: i32) -> i32 = target(x)");
    let body = inspect_body(&hir);
    let site = sites(body)[0];
    let cost = {
        let mut accounting = accounting();
        let ticket = accounting.admit(1, ALLOWANCE).unwrap();
        accounting.balance(&ticket).unwrap().charged_ticks()
    };
    let mut accounting = accounting();
    let mut inputs = registry(&accounting, body);
    let selection = inputs
        .selection_ticket(&mut accounting, site, ALLOWANCE)
        .unwrap();
    let before = accounting.balance(&selection).unwrap();
    let upstream = accounting.admit(1, cost).unwrap();
    let stop = incomplete(
        inputs
            .contextual_ticket(&mut accounting, &upstream)
            .unwrap_err(),
    );
    assert_eq!(stop.root(), upstream.identity());
    assert_eq!(accounting.balance(&selection).unwrap(), before);
    assert_eq!(
        incomplete(
            inputs
                .selection_ticket(&mut accounting, site, ALLOWANCE)
                .unwrap_err()
        ),
        stop
    );
    assert!(
        matches!(AnalysisCallInputRegistry::new(&accounting, 0, site.module(), body), Err(AnalysisCallRegistryFailure::Input(AnalysisCallInputFailure::Work(AnalysisWorkFailure::Incomplete(found)))) if found == stop)
    );
}
