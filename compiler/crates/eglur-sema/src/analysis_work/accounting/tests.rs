use std::num::NonZeroU64;

use crate::record_reservation::observation::{assert_append, assert_growth_envelope, observe};

use super::*;

fn profile(step: u64, processed: u64, reserved: u64) -> AnalysisWorkPricing {
    AnalysisWorkPricing {
        ticks_per_step: NonZeroU64::new(step).unwrap(),
        ticks_per_processed_byte: NonZeroU64::new(processed).unwrap(),
        ticks_per_reserved_byte: NonZeroU64::new(reserved).unwrap(),
    }
}

fn zero_work() -> AnalysisWork {
    AnalysisWork {
        steps: 0,
        inspected_bytes: 0,
        transferred_bytes: 0,
        reserved_bytes: 0,
    }
}

fn admission_ticks(pricing: AnalysisWorkPricing) -> u64 {
    let bytes = size_of::<AnalysisRootBalance>() as u64;
    pricing.ticks_per_step.get()
        + 3 * bytes * pricing.ticks_per_processed_byte.get()
        + 4 * bytes * pricing.ticks_per_reserved_byte.get()
}

fn failure<T>(result: Result<T, AnalysisWorkFailure>) -> AnalysisWorkFailure {
    match result {
        Ok(_) => panic!("expected accounting failure"),
        Err(failure) => failure,
    }
}

fn incomplete(failure: AnalysisWorkFailure) -> AnalysisWorkIncomplete {
    match failure {
        AnalysisWorkFailure::Incomplete(incomplete) => incomplete,
        AnalysisWorkFailure::Claim(error) => panic!("expected incomplete, got {error:?}"),
    }
}

#[test]
fn setup_allocates_only_empty_consumer_headers() {
    let accounting = AnalysisWorkAccounting::new(profile(1, 1, 1), 128).unwrap();
    assert_eq!(accounting.consumers.len(), 128);
    assert!(
        accounting
            .consumers
            .iter()
            .all(|consumer| consumer.roots.is_empty() && consumer.roots.capacity() == 0)
    );
    assert_eq!(accounting.incomplete(), None);

    let empty = AnalysisWorkAccounting::new(profile(1, 1, 1), 0).unwrap();
    assert!(empty.consumers.is_empty());
    assert!(AnalysisWorkAccounting::new(profile(1, 1, 1), usize::MAX).is_err());
}

#[test]
fn admission_pays_maintenance_and_publication_before_returning_a_ticket() {
    let pricing = profile(7, 2, 3);
    let admission = admission_ticks(pricing);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 2).unwrap();
    let (ticket, observations) = observe(|| accounting.admit(1, admission + 100).unwrap());

    assert_eq!(ticket.identity().consumer(), 1);
    assert_eq!(ticket.identity().requirement(), 0);
    let balance = accounting.balance(&ticket).unwrap();
    assert_eq!(balance.allowance_ticks(), admission + 100);
    assert_eq!(balance.remaining_ticks(), 100);
    assert_eq!(balance.charged_ticks(), admission);
    assert_eq!(observations.len(), 1);
    assert_append(
        &observations[0],
        1,
        accounting.consumers[1].roots.capacity(),
    );
    assert_eq!(accounting.consumers[0].roots.capacity(), 0);
}

#[test]
fn exact_admission_allowance_is_allowed_but_cannot_pay_a_claim() {
    let pricing = profile(1, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let ticket = accounting.admit(0, admission_ticks(pricing)).unwrap();
    assert_eq!(accounting.balance(&ticket).unwrap().remaining_ticks(), 0);

    let stopped = incomplete(failure(accounting.claim(&ticket)));
    assert_eq!(stopped.root(), ticket.identity());
    assert_eq!(
        stopped.reason(),
        AnalysisRootStopReason::Exhausted {
            required_ticks: 1,
            remaining_ticks: 0,
        }
    );
    assert_eq!(accounting.balance(&ticket).unwrap().remaining_ticks(), 0);
}

#[test]
fn refused_admission_does_not_reserve_or_publish_and_stops_later_admission() {
    let pricing = profile(1, 1, 1);
    let admission = admission_ticks(pricing);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 2).unwrap();
    let first = accounting.admit(0, admission + 10).unwrap();
    let previous = accounting.balance(&first).unwrap();
    let capacity = accounting.consumers[0].roots.capacity();
    let (stopped, observations) =
        observe(|| incomplete(failure(accounting.admit(0, admission - 1))));

    assert!(observations.is_empty());
    assert_eq!(accounting.consumers[0].roots.len(), 1);
    assert_eq!(accounting.consumers[0].roots.capacity(), capacity);
    assert_eq!(accounting.balance(&first).unwrap(), previous);
    assert_eq!(stopped.root().requirement(), 1);
    assert_eq!(stopped.root().consumer(), 0);
    assert_eq!(
        failure(accounting.admit(1, u64::MAX)),
        AnalysisWorkFailure::Incomplete(stopped)
    );
    assert_eq!(accounting.consumers[1].roots.capacity(), 0);
}

#[test]
fn overflowing_admission_price_stops_before_storage_growth() {
    let mut accounting = AnalysisWorkAccounting::new(profile(u64::MAX, 1, 1), 1).unwrap();
    let (stopped, observations) = observe(|| incomplete(failure(accounting.admit(0, u64::MAX))));
    assert_eq!(
        stopped.reason(),
        AnalysisRootStopReason::Unrepresentable(AnalysisWorkOverflow::Price)
    );
    assert!(observations.is_empty());
    assert_eq!(accounting.consumers[0].roots.capacity(), 0);
}

fn phase_work(
    accounting: &mut AnalysisWorkAccounting,
    ticket: &AnalysisRootTicket,
    work: AnalysisWork,
) {
    accounting.claim(ticket).unwrap().precharge(work).unwrap();
}

#[test]
fn cloned_and_moved_tickets_reuse_one_balance_across_phase_consumers() {
    let pricing = profile(3, 2, 5);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let ticket = accounting.admit(0, admission_ticks(pricing) + 100).unwrap();
    let cloned = ticket.clone();
    assert_eq!(ticket, cloned);

    phase_work(&mut accounting, &ticket, zero_work());
    let after_first = accounting.balance(&ticket).unwrap();
    assert_eq!(after_first.remaining_ticks(), 94);
    let moved = cloned;
    phase_work(
        &mut accounting,
        &moved,
        AnalysisWork {
            steps: 2,
            inspected_bytes: 4,
            transferred_bytes: 1,
            reserved_bytes: 0,
        },
    );

    assert_eq!(accounting.balance(&moved).unwrap().remaining_ticks(), 72);
    assert_eq!(
        accounting.balance(&ticket).unwrap(),
        accounting.balance(&moved).unwrap()
    );
    assert_eq!(accounting.consumers[0].roots.len(), 1);
}

#[test]
fn admission_and_claim_never_share_balances_between_distinct_demands() {
    let pricing = profile(1, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 2).unwrap();
    let first = accounting.admit(0, admission_ticks(pricing) + 20).unwrap();
    let second = accounting.admit(0, admission_ticks(pricing) + 20).unwrap();
    let other = accounting.admit(1, admission_ticks(pricing) + 20).unwrap();
    assert_ne!(first, second);
    assert_ne!(first, other);

    phase_work(
        &mut accounting,
        &first,
        AnalysisWork {
            steps: 8,
            ..zero_work()
        },
    );
    assert_eq!(accounting.balance(&first).unwrap().remaining_ticks(), 10);
    assert_eq!(accounting.balance(&second).unwrap().remaining_ticks(), 20);
    assert_eq!(accounting.balance(&other).unwrap().remaining_ticks(), 20);
}

#[test]
fn actual_local_growth_stays_within_paid_envelope_and_preserves_old_tickets() {
    let pricing = profile(1, 1, 1);
    let admission = admission_ticks(pricing);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 2).unwrap();
    let first = accounting.admit(0, admission + 100).unwrap();
    phase_work(&mut accounting, &first, zero_work());
    let prefix = accounting.balance(&first).unwrap();

    let (tickets, observations) = observe(|| {
        (0..1024)
            .map(|_| accounting.admit(1, admission + 20).unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(accounting.balance(&first).unwrap(), prefix);
    assert_eq!(observations.len(), tickets.len());
    assert_growth_envelope(
        &observations,
        tickets.len() * size_of::<AnalysisRootBalance>(),
    );

    let mut backing = 0;
    let mut transferred = tickets.len() * size_of::<AnalysisRootBalance>();
    for (index, observation) in observations.iter().enumerate() {
        assert_append(observation, index + 1, observation.after_capacity);
        if let Some(requested) = observation.requested_capacity {
            backing += requested * observation.record_bytes;
            transferred += observation.len * observation.record_bytes;
        }
    }
    let paid = AnalysisWorkAccounting::admission_work().unwrap();
    assert!(backing as u64 <= tickets.len() as u64 * paid.reserved_bytes);
    assert!(transferred as u64 <= tickets.len() as u64 * paid.transferred_bytes);

    for (index, ticket) in tickets.iter().enumerate() {
        assert_eq!(ticket.identity().requirement(), index);
        assert_eq!(accounting.balance(ticket).unwrap().remaining_ticks(), 20);
        phase_work(&mut accounting, ticket, zero_work());
        assert_eq!(accounting.balance(ticket).unwrap().remaining_ticks(), 18);
    }
}

#[test]
fn many_independent_consumers_have_no_shared_allowance() {
    let pricing = profile(1, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 32).unwrap();
    for round in 0..128 {
        for consumer in 0..32 {
            let ticket = accounting
                .admit(consumer, admission_ticks(pricing) + 2)
                .unwrap();
            assert_eq!(ticket.identity().requirement(), round);
            phase_work(&mut accounting, &ticket, zero_work());
            assert_eq!(accounting.balance(&ticket).unwrap().remaining_ticks(), 0);
        }
    }
    assert_eq!(accounting.incomplete(), None);
}

#[test]
fn tickets_cannot_claim_another_live_or_later_attempt() {
    let pricing = profile(1, 1, 1);
    let mut first = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let first_ticket = first.admit(0, u64::MAX).unwrap();
    let mut second = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let second_ticket = second.admit(0, u64::MAX).unwrap();
    assert_eq!(first_ticket.identity(), second_ticket.identity());
    assert_ne!(first_ticket, second_ticket);
    let before = second.balance(&second_ticket).unwrap();
    assert_eq!(
        failure(second.claim(&first_ticket)),
        AnalysisWorkFailure::Claim(AnalysisRootClaimError::ForeignAttempt)
    );
    drop(first);
    assert_eq!(
        second.balance(&first_ticket),
        Err(AnalysisRootClaimError::ForeignAttempt)
    );
    assert_eq!(second.balance(&second_ticket).unwrap(), before);
    assert_eq!(second.incomplete(), None);
}

#[test]
fn invalid_local_claims_are_not_budget_stops_or_balance_mutations() {
    let mut accounting = AnalysisWorkAccounting::new(profile(1, 1, 1), 1).unwrap();
    let ticket = accounting.admit(0, u64::MAX).unwrap();
    let before = accounting.balance(&ticket).unwrap();
    for (root, expected) in [
        (
            AnalysisRootIdentity {
                consumer: 1,
                requirement: 0,
            },
            AnalysisRootClaimError::UnknownConsumer,
        ),
        (
            AnalysisRootIdentity {
                consumer: 0,
                requirement: 1,
            },
            AnalysisRootClaimError::UnknownRoot,
        ),
    ] {
        let malformed = AnalysisRootTicket {
            attempt: Arc::clone(&accounting.attempt),
            root,
        };
        assert_eq!(
            failure(accounting.claim(&malformed)),
            AnalysisWorkFailure::Claim(expected)
        );
        assert_eq!(accounting.balance(&malformed), Err(expected));
    }
    assert_eq!(
        failure(accounting.admit(1, 0)),
        AnalysisWorkFailure::Claim(AnalysisRootClaimError::UnknownConsumer)
    );
    assert_eq!(accounting.balance(&ticket).unwrap(), before);
    assert_eq!(accounting.incomplete(), None);
}

#[test]
fn first_exhaustion_stops_all_valid_work_and_preserves_the_paid_prefix() {
    let pricing = profile(1, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 2).unwrap();
    let first = accounting.admit(0, admission_ticks(pricing) + 5).unwrap();
    let second = accounting.admit(1, u64::MAX).unwrap();
    let mut scope = accounting.claim(&first).unwrap();
    scope.precharge(zero_work()).unwrap();
    let stopped = incomplete(failure(scope.precharge(AnalysisWork {
        steps: 3,
        ..zero_work()
    })));
    assert_eq!(
        stopped.reason(),
        AnalysisRootStopReason::Exhausted {
            required_ticks: 4,
            remaining_ticks: 3
        }
    );
    assert_eq!(
        failure(scope.precharge(zero_work())),
        AnalysisWorkFailure::Incomplete(stopped)
    );
    assert_eq!(
        scope.stop_unrepresentable(AnalysisWorkOverflow::Price),
        AnalysisWorkFailure::Incomplete(stopped)
    );
    assert_eq!(accounting.balance(&first).unwrap().remaining_ticks(), 3);
    let second_balance = accounting.balance(&second).unwrap();
    assert_eq!(
        failure(accounting.claim(&second)),
        AnalysisWorkFailure::Incomplete(stopped)
    );
    assert_eq!(
        failure(accounting.admit(1, u64::MAX)),
        AnalysisWorkFailure::Incomplete(stopped)
    );
    assert_eq!(accounting.balance(&second).unwrap(), second_balance);
    assert_eq!(accounting.incomplete(), Some(stopped));
}

#[test]
fn foreign_claim_errors_do_not_overwrite_a_terminal_cause() {
    let pricing = profile(1, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let stopped = incomplete(failure(accounting.admit(0, 0)));
    let mut other = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let foreign = other.admit(0, u64::MAX).unwrap();
    assert_eq!(
        failure(accounting.claim(&foreign)),
        AnalysisWorkFailure::Claim(AnalysisRootClaimError::ForeignAttempt)
    );
    for (root, expected) in [
        (
            AnalysisRootIdentity {
                consumer: 1,
                requirement: 0,
            },
            AnalysisRootClaimError::UnknownConsumer,
        ),
        (
            AnalysisRootIdentity {
                consumer: 0,
                requirement: 0,
            },
            AnalysisRootClaimError::UnknownRoot,
        ),
    ] {
        let malformed = AnalysisRootTicket {
            attempt: Arc::clone(&accounting.attempt),
            root,
        };
        assert_eq!(
            failure(accounting.claim(&malformed)),
            AnalysisWorkFailure::Claim(expected)
        );
    }
    assert_eq!(accounting.incomplete(), Some(stopped));
}

#[test]
fn quantity_and_price_overflows_preserve_balances_and_terminal_custody() {
    for (work, expected) in [
        (
            AnalysisWork {
                steps: u64::MAX,
                ..zero_work()
            },
            AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::Steps),
        ),
        (
            AnalysisWork {
                reserved_bytes: u64::MAX,
                ..zero_work()
            },
            AnalysisWorkOverflow::Price,
        ),
        (
            AnalysisWork {
                inspected_bytes: u64::MAX,
                ..zero_work()
            },
            AnalysisWorkOverflow::Price,
        ),
        (
            AnalysisWork {
                transferred_bytes: u64::MAX,
                ..zero_work()
            },
            AnalysisWorkOverflow::Price,
        ),
    ] {
        let mut accounting = AnalysisWorkAccounting::new(profile(1, 2, 2), 1).unwrap();
        let ticket = accounting.admit(0, u64::MAX).unwrap();
        let mut scope = accounting.claim(&ticket).unwrap();
        let before = scope.accounting.balance(&ticket).unwrap();
        let stopped = incomplete(failure(scope.precharge(work)));
        assert_eq!(
            stopped.reason(),
            AnalysisRootStopReason::Unrepresentable(expected)
        );
        assert_eq!(scope.accounting.balance(&ticket).unwrap(), before);
        assert_eq!(stopped.root(), ticket.identity());
    }
}

#[test]
fn consumer_descriptor_overflow_has_the_same_terminal_stop_custody() {
    let mut accounting = AnalysisWorkAccounting::new(profile(1, 1, 1), 1).unwrap();
    let ticket = accounting.admit(0, u64::MAX).unwrap();
    let mut scope = accounting.claim(&ticket).unwrap();
    let before = scope.accounting.balance(&ticket).unwrap();
    let cause = AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::TransferredBytes);
    let stopped = incomplete(scope.stop_unrepresentable(cause));
    assert_eq!(
        stopped.reason(),
        AnalysisRootStopReason::Unrepresentable(cause)
    );
    assert_eq!(
        failure(scope.precharge(zero_work())),
        AnalysisWorkFailure::Incomplete(stopped)
    );
    assert_eq!(accounting.balance(&ticket).unwrap(), before);
}

#[test]
fn zero_reported_work_still_pays_for_each_accounting_transition() {
    let pricing = profile(4, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let ticket = accounting.admit(0, admission_ticks(pricing) + 12).unwrap();
    let mut scope = accounting.claim(&ticket).unwrap();
    scope.precharge(zero_work()).unwrap();
    scope.precharge(zero_work()).unwrap();
    let stopped = incomplete(failure(scope.precharge(zero_work())));
    assert_eq!(
        stopped.reason(),
        AnalysisRootStopReason::Exhausted {
            required_ticks: 4,
            remaining_ticks: 0
        }
    );
}

#[test]
fn splitting_work_adds_only_the_extra_bookkeeping_steps() {
    let pricing = profile(16, 1, 1);
    let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
    let whole = accounting.admit(0, u64::MAX).unwrap();
    let split = accounting.admit(0, u64::MAX).unwrap();
    phase_work(
        &mut accounting,
        &whole,
        AnalysisWork {
            transferred_bytes: 16,
            ..zero_work()
        },
    );
    let mut scope = accounting.claim(&split).unwrap();
    for _ in 0..16 {
        scope
            .precharge(AnalysisWork {
                transferred_bytes: 1,
                ..zero_work()
            })
            .unwrap();
    }
    let whole = accounting.balance(&whole).unwrap().charged_ticks();
    let split = accounting.balance(&split).unwrap().charged_ticks();
    assert_eq!(split - whole, 15 * 16);
}

#[test]
fn root_cost_is_independent_of_other_consumers_record_histories() {
    let pricing = profile(8, 1, 3);
    let mut observed = Vec::new();
    for unrelated_roots in [0, 1, 3, 63, 1024] {
        let mut accounting = AnalysisWorkAccounting::new(pricing, 3).unwrap();
        for index in 0..unrelated_roots {
            accounting.admit(index % 2, u64::MAX).unwrap();
        }
        let target = accounting.admit(2, u64::MAX).unwrap();
        phase_work(
            &mut accounting,
            &target,
            AnalysisWork {
                steps: 3,
                inspected_bytes: 21,
                transferred_bytes: 17,
                reserved_bytes: 5,
            },
        );
        observed.push(accounting.balance(&target).unwrap());
    }
    assert!(observed.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn root_cost_is_independent_of_its_local_capacity_and_unrelated_admission_order() {
    let pricing = profile(8, 1, 3);
    let work = AnalysisWork {
        steps: 3,
        inspected_bytes: 21,
        transferred_bytes: 17,
        reserved_bytes: 5,
    };
    let mut observed = Vec::new();
    for prefix in [0, 1, 3, 63, 1024] {
        for target_first in [false, true] {
            let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
            let mut target = None;
            if target_first {
                target = Some(accounting.admit(0, u64::MAX).unwrap());
            }
            for _ in 0..prefix {
                accounting.admit(0, u64::MAX).unwrap();
            }
            let target = target.unwrap_or_else(|| accounting.admit(0, u64::MAX).unwrap());
            phase_work(&mut accounting, &target, work);
            observed.push(accounting.balance(&target).unwrap());
        }
    }
    assert!(observed.windows(2).all(|pair| pair[0] == pair[1]));
}

#[test]
fn generated_work_traces_match_an_independent_balance_model() {
    let mut seed = 0x2317_8910_u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed
    };
    for _ in 0..256 {
        let step = next() % 32 + 1;
        let processed = next() % 8 + 1;
        let reserved = next() % 8 + 1;
        let pricing = profile(step, processed, reserved);
        let mut remaining = next() % 4096;
        let allowance = admission_ticks(pricing) + remaining;
        let mut accounting = AnalysisWorkAccounting::new(pricing, 1).unwrap();
        let ticket = accounting.admit(0, allowance).unwrap();

        if remaining < step {
            let stopped = incomplete(failure(accounting.claim(&ticket)));
            assert_eq!(
                stopped.reason(),
                AnalysisRootStopReason::Exhausted {
                    required_ticks: step,
                    remaining_ticks: remaining
                }
            );
            continue;
        }
        remaining -= step;
        let mut scope = accounting.claim(&ticket).unwrap();
        for _ in 0..64 {
            let work = AnalysisWork {
                steps: next() % 8,
                inspected_bytes: next() % 16,
                transferred_bytes: next() % 16,
                reserved_bytes: next() % 16,
            };
            let expected = (u128::from(work.steps) + 1) * u128::from(step)
                + u128::from(work.inspected_bytes + work.transferred_bytes) * u128::from(processed)
                + u128::from(work.reserved_bytes) * u128::from(reserved);
            let result = scope.precharge(work);
            if expected > u128::from(remaining) {
                let stopped = incomplete(failure(result));
                assert_eq!(
                    stopped.reason(),
                    AnalysisRootStopReason::Exhausted {
                        required_ticks: expected as u64,
                        remaining_ticks: remaining
                    }
                );
                assert_eq!(
                    scope.accounting.balance(&ticket).unwrap().remaining_ticks(),
                    remaining
                );
                break;
            }
            result.unwrap();
            remaining -= expected as u64;
            let observed = scope.accounting.balance(&ticket).unwrap();
            assert_eq!(observed.remaining_ticks(), remaining);
            assert_eq!(observed.charged_ticks(), allowance - remaining);
        }
    }
}
