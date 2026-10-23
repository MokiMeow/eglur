use std::cell::{Cell, RefCell};

use crate::{AnalysisRootClaimError, AnalysisWorkPricing, FactState};

use super::*;

#[path = "test_support.rs"]
pub(super) mod test_support;
use test_support::{Fixture, fixture};

const TEXT: &str = "trait Alpha {}\ntrait Zulu {}\nfn pick[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = ()\nfn inspect[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = pick(x, y)";
const ALLOWANCE: u64 = 1_000_000;

thread_local! {
    static HEADER_WRITES: Cell<usize> = const { Cell::new(0) };
    static PARAMETER_COPIES: Cell<usize> = const { Cell::new(0) };
    static CHARGES: RefCell<Vec<super::super::ChargeEvent>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn record_header_write() {
    HEADER_WRITES.set(HEADER_WRITES.get() + 1);
}

pub(super) fn record_parameter_copy() {
    PARAMETER_COPIES.set(PARAMETER_COPIES.get() + 1);
}

fn observe(accounting: &mut AnalysisWorkAccounting) {
    CHARGES.with_borrow_mut(Vec::clear);
    accounting.observe_charges(|event| CHARGES.with_borrow_mut(|events| events.push(event)));
}

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(7, 2, 3).unwrap(), 2).unwrap()
}

fn parameter(fixture: &Fixture, index: usize) -> &GenericParameterSignature {
    let FactState::Known(parameter) = fixture
        .signatures
        .generic_parameter(&fixture.parameters[index])
    else {
        panic!("expected resolved generic parameter")
    };
    parameter
}

fn admit(
    fixture: &Fixture,
    accounting: &mut AnalysisWorkAccounting,
    parameter_index: usize,
    bound_index: usize,
) -> AnalysisBoundInputs {
    AnalysisBoundInputs::admit(
        accounting,
        0,
        fixture.site,
        parameter(fixture, parameter_index),
        bound_index,
        ALLOWANCE,
    )
    .unwrap()
}

#[test]
fn declared_bounds_stay_distinct_and_reuse_balances_after_semantic_instantiation() {
    let mut fixture = fixture(TEXT, None);
    assert_ne!(
        parameter(&fixture, 0).bounds(),
        parameter(&fixture, 0).declared_bounds()
    );
    let mut accounting = accounting();
    let mut holders = [
        admit(&fixture, &mut accounting, 0, 1),
        admit(&fixture, &mut accounting, 1, 0),
    ];
    let tickets = holders
        .iter_mut()
        .flat_map(|holder| {
            (0..2)
                .map(|index| {
                    holder
                        .bound_ticket(&mut accounting, index, || ALLOWANCE)
                        .unwrap()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for (index, ticket) in tickets.iter().enumerate() {
        assert!(!tickets[..index].contains(ticket));
    }

    let integer = fixture
        .signatures
        .types_mut()
        .primitive(crate::PrimitiveType::I32)
        .unwrap();
    let replacements = fixture
        .parameters
        .iter()
        .cloned()
        .map(|parameter| (parameter, integer))
        .collect();
    let first = parameter(&fixture, 0)
        .clone()
        .instantiate_bounds(fixture.signatures.types_mut(), &replacements)
        .unwrap();
    let second = parameter(&fixture, 1)
        .clone()
        .instantiate_bounds(fixture.signatures.types_mut(), &replacements)
        .unwrap();
    assert_eq!(first[0].declared(), second[0].declared());
    let first = first
        .into_iter()
        .map(|bound| bound.into_obligation())
        .collect::<Vec<_>>();
    let second = second
        .into_iter()
        .map(|bound| bound.into_obligation())
        .collect::<Vec<_>>();
    assert_eq!(first, second);

    let before = tickets
        .iter()
        .map(|ticket| accounting.balance(ticket).unwrap().charged_ticks())
        .collect::<Vec<_>>();
    for _ in 0..4 {
        for (holder_index, holder) in holders.iter_mut().enumerate() {
            for index in 0..2 {
                let expected = &tickets[holder_index * 2 + index];
                assert_eq!(
                    &holder
                        .bound_ticket(&mut accounting, index, || panic!("replay policy invoked"))
                        .unwrap(),
                    expected
                );
                assert_eq!(
                    &holder.existing_ticket(&mut accounting, index).unwrap(),
                    expected
                );
                assert_eq!(
                    accounting.balance(expected).unwrap().allowance_ticks(),
                    ALLOWANCE
                );
            }
        }
    }
    for (ticket, before) in tickets.iter().zip(before) {
        assert!(accounting.balance(ticket).unwrap().charged_ticks() > before);
    }
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        4
    );
}

#[test]
fn evidence_retrieval_never_admits_or_renews_a_bound() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let mut inputs = admit(&fixture, &mut accounting, 0, 1);
    assert_eq!(inputs.site(), fixture.site);
    assert_eq!(inputs.parameter(), &fixture.parameters[0]);
    assert_eq!(inputs.declared_bound_count(), 2);
    assert_eq!(
        inputs.existing_ticket(&mut accounting, 0),
        Err(AnalysisProofInputFailure::UnadmittedBound {
            site: fixture.site,
            index: 0
        })
    );
    let first = inputs.existing_ticket(&mut accounting, 1).unwrap();
    assert_eq!(first.identity().requirement(), 0);
    let other = inputs
        .bound_ticket(&mut accounting, 0, || ALLOWANCE)
        .unwrap();
    assert_ne!(other, first);
    assert_eq!(other.identity().requirement(), 1);
    assert_eq!(inputs.existing_ticket(&mut accounting, 0).unwrap(), other);
}

#[test]
fn invalid_ordinals_and_unbounded_parameters_refuse_without_admission() {
    for text in [
        TEXT,
        "fn pick[T](x: T) -> Unit = ()\nfn inspect(x: i32) -> Unit = pick(x)",
    ] {
        let fixture = fixture(text, None);
        let count = parameter(&fixture, 0).declared_bounds().len();
        let mut accounting = accounting();
        for index in [count, usize::MAX] {
            let error = AnalysisBoundInputs::admit(
                &mut accounting,
                0,
                fixture.site,
                parameter(&fixture, 0),
                index,
                ALLOWANCE,
            )
            .err()
            .unwrap();
            assert_eq!(
                error,
                AnalysisProofInputFailure::UnknownDeclaredBound {
                    site: fixture.site,
                    index
                }
            );
        }
        assert!(accounting.incomplete().is_none());
        assert_eq!(
            accounting
                .admit(0, ALLOWANCE)
                .unwrap()
                .identity()
                .requirement(),
            0
        );
    }
}

#[test]
fn first_admission_refuses_before_allocating_declared_slot_headers() {
    let fixture = fixture(TEXT, None);
    let mut reference = accounting();
    observe(&mut reference);
    let _inputs = admit(&fixture, &mut reference, 0, 1);
    let cost = CHARGES.with_borrow(|events| events[0].ticks);
    for allowance in [0, 1, cost - 1] {
        let mut accounting = accounting();
        HEADER_WRITES.set(0);
        PARAMETER_COPIES.set(0);
        let error = AnalysisBoundInputs::admit(
            &mut accounting,
            0,
            fixture.site,
            parameter(&fixture, 0),
            1,
            allowance,
        )
        .err()
        .unwrap();
        let AnalysisProofInputFailure::Work(AnalysisWorkFailure::Incomplete(stop)) = error else {
            panic!("expected incomplete: {error:?}")
        };
        assert_eq!(accounting.incomplete(), Some(stop));
        assert_eq!(HEADER_WRITES.get(), 0);
        assert_eq!(PARAMETER_COPIES.get(), 0);
        assert!(
            matches!(accounting.admit(1, ALLOWANCE), Err(AnalysisWorkFailure::Incomplete(same)) if same == stop)
        );
    }
    let mut accounting = accounting();
    HEADER_WRITES.set(0);
    PARAMETER_COPIES.set(0);
    assert!(
        AnalysisBoundInputs::admit(
            &mut accounting,
            0,
            fixture.site,
            parameter(&fixture, 0),
            1,
            cost
        )
        .is_ok()
    );
    assert_eq!(HEADER_WRITES.get(), 2);
    assert_eq!(PARAMETER_COPIES.get(), 1);
}

#[test]
fn failed_later_admission_keeps_existing_custody_and_sticky_cause() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let mut inputs = admit(&fixture, &mut accounting, 0, 0);
    let original = inputs.existing_ticket(&mut accounting, 0).unwrap();
    let error = inputs.bound_ticket(&mut accounting, 1, || 0).unwrap_err();
    assert_eq!(
        inputs.existing_ticket(&mut accounting, 1),
        Err(AnalysisProofInputFailure::UnadmittedBound {
            site: fixture.site,
            index: 1
        }),
    );
    let AnalysisProofInputFailure::Work(AnalysisWorkFailure::Incomplete(stop)) = error else {
        panic!("expected incomplete")
    };
    assert_eq!(inputs.existing_ticket(&mut accounting, 0), Err(error));
    assert_eq!(
        inputs.bound_ticket(&mut accounting, 1, || panic!(
            "policy invoked after sticky stop"
        )),
        Err(error)
    );
    assert_eq!(accounting.incomplete(), Some(stop));
    assert_eq!(
        accounting.balance(&original).unwrap().allowance_ticks(),
        ALLOWANCE
    );
}

#[test]
fn foreign_attempt_is_an_internal_custody_failure_without_side_effects() {
    let fixture = fixture(TEXT, None);
    let mut original = accounting();
    let mut inputs = admit(&fixture, &mut original, 0, 0);
    let dispatch = AnalysisDispatchInputs::admit(
        &mut original,
        0,
        MethodDispatchSite::Invocation(fixture.site),
        ALLOWANCE,
    )
    .unwrap();
    let mut foreign = accounting();
    let error = AnalysisProofInputFailure::Work(AnalysisWorkFailure::Claim(
        AnalysisRootClaimError::ForeignAttempt,
    ));
    assert_eq!(
        inputs.bound_ticket(&mut foreign, 1, || panic!("foreign policy invoked")),
        Err(error)
    );
    assert_eq!(inputs.existing_ticket(&mut foreign, 0), Err(error));
    assert_eq!(dispatch.existing_ticket(&mut foreign), Err(error));
    assert!(foreign.incomplete().is_none());
    assert_eq!(
        foreign
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        0
    );
}

#[test]
fn invocation_and_callable_value_dispatch_reuse_distinct_original_roots() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let sites = [
        MethodDispatchSite::Invocation(fixture.site),
        MethodDispatchSite::CallableValue(fixture.site),
    ];
    let holders = sites
        .map(|site| AnalysisDispatchInputs::admit(&mut accounting, 0, site, ALLOWANCE).unwrap());
    let tickets = holders
        .each_ref()
        .map(|holder| holder.existing_ticket(&mut accounting).unwrap());
    assert_ne!(tickets[0], tickets[1]);
    for (index, holder) in holders.iter().enumerate() {
        assert_eq!(holder.site(), sites[index]);
        for _ in 0..8 {
            assert_eq!(
                holder.existing_ticket(&mut accounting).unwrap(),
                tickets[index]
            );
        }
        assert_eq!(
            accounting
                .balance(&tickets[index])
                .unwrap()
                .allowance_ticks(),
            ALLOWANCE
        );
    }
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        2
    );
}

#[test]
fn trusted_original_identifier_payload_is_prepaid_without_copying_semantic_bounds() {
    let text = "fn inspect() -> Unit = ()";
    let mut totals = Vec::new();
    for owner in [
        "std::probe::pick".to_owned(),
        format!("std::probe::{}", "p".repeat(256)),
    ] {
        let fixture = fixture(text, Some(&owner));
        let parameter = parameter(&fixture, 0);
        assert!(matches!(parameter.id(), GenericParameterId::Trusted { .. }));
        assert_ne!(parameter.bounds(), parameter.declared_bounds());
        let mut accounting = accounting();
        let inputs = admit(&fixture, &mut accounting, 0, 0);
        let ticket = inputs.existing_ticket(&mut accounting, 0).unwrap();
        assert_eq!(inputs.parameter(), parameter.id());
        totals.push((
            owner.len(),
            accounting.balance(&ticket).unwrap().charged_ticks(),
        ));
    }
    assert_eq!(
        totals[1].1 - totals[0].1,
        (totals[1].0 - totals[0].0) as u64 * 5
    );
}

#[test]
fn every_holder_charge_can_stop_without_renewing_or_finishing() {
    let fixture = fixture(TEXT, None);
    for dispatch in [false, true] {
        let run = |accounting: &mut AnalysisWorkAccounting, allowance| {
            if dispatch {
                AnalysisDispatchInputs::admit(
                    accounting,
                    0,
                    MethodDispatchSite::Invocation(fixture.site),
                    allowance,
                )
                .and_then(|inputs| inputs.existing_ticket(accounting))
            } else {
                AnalysisBoundInputs::admit(
                    accounting,
                    0,
                    fixture.site,
                    parameter(&fixture, 0),
                    1,
                    allowance,
                )
                .and_then(|inputs| inputs.existing_ticket(accounting, 1))
            }
        };
        let mut complete = accounting();
        observe(&mut complete);
        run(&mut complete, ALLOWANCE).unwrap();
        let events = CHARGES.with_borrow(Clone::clone);
        assert!(!events.is_empty());
        let mut prefix = 0;
        for event in events {
            let mut limited = accounting();
            let result = run(&mut limited, prefix);
            let Err(AnalysisProofInputFailure::Work(AnalysisWorkFailure::Incomplete(stop))) =
                result
            else {
                panic!("expected interruption before {event:?}: {result:?}")
            };
            assert_eq!(limited.incomplete(), Some(stop));
            assert_eq!(stop.root(), event.root);
            assert_eq!(
                limited.admit(1, ALLOWANCE),
                Err(AnalysisWorkFailure::Incomplete(stop))
            );
            prefix += event.ticks;
        }
        let mut exact = accounting();
        let ticket = run(&mut exact, prefix).unwrap();
        assert_eq!(exact.balance(&ticket).unwrap().remaining_ticks(), 0);
        assert!(exact.incomplete().is_none());
    }
}

#[test]
fn missing_slot_installation_declares_work_and_every_replay_charge_can_stop() {
    let fixture = fixture(TEXT, None);
    let run = |accounting: &mut AnalysisWorkAccounting, allowances: [u64; 2]| {
        let mut inputs = AnalysisBoundInputs::admit(
            accounting,
            0,
            fixture.site,
            parameter(&fixture, 0),
            0,
            allowances[0],
        )?;
        inputs.bound_ticket(accounting, 0, || panic!("replay renewed first root"))?;
        inputs.bound_ticket(accounting, 1, || allowances[1])?;
        inputs.bound_ticket(accounting, 1, || panic!("replay renewed second root"))?;
        inputs.existing_ticket(accounting, 1)
    };
    let mut complete = accounting();
    observe(&mut complete);
    run(&mut complete, [ALLOWANCE; 2]).unwrap();
    let events = CHARGES.with_borrow(Clone::clone);
    let installation = events
        .iter()
        .find(|event| event.root.requirement() == 1)
        .unwrap();
    assert_eq!(installation.kind, super::super::ChargeKind::Admission);
    let mut baseline = accounting();
    observe(&mut baseline);
    baseline.admit(0, ALLOWANCE).unwrap();
    baseline.admit(0, ALLOWANCE).unwrap();
    let bare = CHARGES.with_borrow(|events| events[1]);
    assert_eq!(
        installation.work,
        bare.work
            .checked_add(AnalysisWork {
                steps: 2,
                inspected_bytes: size_of::<Option<AnalysisRootTicket>>() as u64,
                transferred_bytes: (size_of::<Option<AnalysisRootTicket>>()
                    + size_of::<AnalysisRootTicket>()) as u64,
                reserved_bytes: 0,
            })
            .unwrap()
    );

    let mut prefixes = [0; 2];
    for event in events {
        let root = event.root.requirement();
        let mut allowances = [ALLOWANCE; 2];
        allowances[root] = prefixes[root];
        let mut limited = accounting();
        let result = run(&mut limited, allowances);
        let Err(AnalysisProofInputFailure::Work(AnalysisWorkFailure::Incomplete(stop))) = result
        else {
            panic!("expected interruption before {event:?}: {result:?}")
        };
        assert_eq!(limited.incomplete(), Some(stop));
        assert_eq!(stop.root(), event.root);
        prefixes[root] += event.ticks;
    }
    let mut exact = accounting();
    run(&mut exact, prefixes).unwrap();
    assert!(exact.incomplete().is_none());
}

#[test]
fn slot_preparation_scales_with_declared_input_without_eager_bound_admission() {
    let mut previous = None;
    for count in [1, 2, 16, 128] {
        let names = (0..count)
            .map(|index| format!("Mark{index}"))
            .collect::<Vec<_>>();
        let mut text = names
            .iter()
            .map(|name| format!("trait {name} {{}}\n"))
            .collect::<String>();
        text.push_str(&format!(
            "fn pick[T: {bounds}](x: T) -> Unit = ()\nfn inspect[T: {bounds}](x: T) -> Unit = pick(x)",
            bounds = names.join("+")
        ));
        let fixture = fixture(&text, None);
        let mut accounting = accounting();
        observe(&mut accounting);
        HEADER_WRITES.set(0);
        let inputs = admit(&fixture, &mut accounting, 0, count - 1);
        let charge = CHARGES.with_borrow(|events| events[0]);
        if let Some((previous_count, previous_charge)) = previous {
            let bytes = ((count - previous_count) * size_of::<Option<AnalysisRootTicket>>()) as u64;
            let previous_charge: super::super::ChargeEvent = previous_charge;
            assert_eq!(charge.work.steps, previous_charge.work.steps);
            assert_eq!(
                charge.work.inspected_bytes,
                previous_charge.work.inspected_bytes
            );
            assert_eq!(
                charge.work.transferred_bytes - previous_charge.work.transferred_bytes,
                bytes
            );
            assert_eq!(
                charge.work.reserved_bytes - previous_charge.work.reserved_bytes,
                bytes
            );
        }
        previous = Some((count, charge));
        assert_eq!(HEADER_WRITES.get(), count);
        assert_eq!(inputs.declared_bound_count(), count);
        assert_eq!(
            inputs.existing_ticket(&mut accounting, count),
            Err(AnalysisProofInputFailure::UnknownDeclaredBound {
                site: fixture.site,
                index: count
            })
        );
        for index in 0..count - 1 {
            assert_eq!(
                inputs.existing_ticket(&mut accounting, index),
                Err(AnalysisProofInputFailure::UnadmittedBound {
                    site: fixture.site,
                    index
                })
            );
        }
        assert_eq!(
            accounting
                .admit(0, ALLOWANCE)
                .unwrap()
                .identity()
                .requirement(),
            1
        );
    }
}
