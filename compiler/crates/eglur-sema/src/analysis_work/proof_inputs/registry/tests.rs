use std::cell::{Cell, RefCell};

use crate::{AnalysisRootClaimError, AnalysisWorkPricing, FactState};

use super::super::tests::test_support::{Fixture, fixture};
use super::*;

const TEXT: &str = "trait Alpha {}\ntrait Zulu {}\nfn pick[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = ()\nfn inspect[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = pick(x, y)";
const ALLOWANCE: u64 = 100_000_000;

thread_local! {
    static CHARGES: RefCell<Vec<crate::analysis_work::ChargeEvent>> = const { RefCell::new(Vec::new()) };
}

fn observe(accounting: &mut AnalysisWorkAccounting) {
    CHARGES.with_borrow_mut(Vec::clear);
    accounting.observe_charges(|event| CHARGES.with_borrow_mut(|events| events.push(event)));
}

fn callable_value(fixture: &Fixture) -> ExpressionKey {
    let body = fixture.hir.body(fixture.site.expression().body());
    let (id, _) = body
        .expressions()
        .find(|(_, expression)| matches!(expression.kind(), ExprKind::Name(_)))
        .unwrap();
    ExpressionKey::new(fixture.site.module(), id)
}

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(7, 2, 3).unwrap(), 2).unwrap()
}

fn registry<'hir>(
    accounting: &AnalysisWorkAccounting,
    fixture: &'hir Fixture,
) -> AnalysisProofInputRegistry<'hir> {
    AnalysisProofInputRegistry::new(
        accounting,
        0,
        fixture.site.module(),
        fixture.hir.body(fixture.site.expression().body()),
    )
    .unwrap()
}

fn parameter(fixture: &Fixture, index: usize) -> &GenericParameterRecord {
    let FactState::Known(record) = fixture
        .signatures
        .generic_parameter_record(&fixture.parameters[index])
    else {
        panic!("original generic definition must be available")
    };
    record
}

#[test]
fn setup_has_only_original_expression_slots_and_no_admissions() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let registry = registry(&accounting, &fixture);
    assert_eq!(
        registry.occurrences.len(),
        fixture
            .hir
            .body(fixture.site.expression().body())
            .expressions()
            .len()
    );
    assert!(registry.bounds.is_empty());
    assert!(registry.dispatch.is_empty());
    assert!(
        registry
            .occurrences
            .iter()
            .all(|slot| slot.bounds.is_none() && slot.dispatch == [None; 2])
    );
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
fn original_source_slots_keep_their_balances_across_reordered_replay() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    let calls = Cell::new(0);
    let allowance = || {
        calls.set(calls.get() + 1);
        ALLOWANCE
    };
    let mut tickets = Vec::new();
    for (p, bound) in [(1, 1), (0, 0), (1, 0), (0, 1)] {
        let ticket = registry
            .bound_ticket(
                &mut accounting,
                fixture.site,
                parameter(&fixture, p),
                bound,
                allowance,
            )
            .unwrap();
        tickets.push((p, bound, ticket));
    }
    for _ in 0..4 {
        for (p, bound, ticket) in tickets.iter().rev() {
            let before = accounting.balance(ticket).unwrap().charged_ticks();
            assert_eq!(
                &registry
                    .bound_ticket(
                        &mut accounting,
                        fixture.site,
                        parameter(&fixture, *p),
                        *bound,
                        || panic!("replay renewed an allowance")
                    )
                    .unwrap(),
                ticket
            );
            assert_eq!(
                &registry
                    .existing_bound_ticket(
                        &mut accounting,
                        fixture.site,
                        parameter(&fixture, *p).id(),
                        *bound
                    )
                    .unwrap(),
                ticket
            );
            assert!(accounting.balance(ticket).unwrap().charged_ticks() > before);
        }
    }
    assert_eq!(calls.get(), tickets.len());
    assert_eq!(registry.bounds.len(), fixture.parameters.len());
    for (index, (_, _, ticket)) in tickets.iter().enumerate() {
        assert_eq!(ticket.identity().requirement(), index);
        assert_eq!(
            accounting.balance(ticket).unwrap().allowance_ticks(),
            ALLOWANCE
        );
    }
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        tickets.len()
    );
}

#[test]
fn trusted_definitions_use_the_same_once_only_path_and_declared_ordinals() {
    let fixture = fixture(
        "import std::probe::pick\nfn inspect() -> Unit = pick()",
        Some("std::probe::pick"),
    );
    let site = fixture.site;
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    let parameter = parameter(&fixture, 0);
    assert_ne!(
        parameter.signature().bounds(),
        parameter.signature().declared_bounds()
    );
    let first = registry
        .bound_ticket(&mut accounting, site, parameter, 1, || ALLOWANCE)
        .unwrap();
    let second = registry
        .bound_ticket(&mut accounting, site, parameter, 0, || ALLOWANCE)
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(
        registry
            .existing_bound_ticket(&mut accounting, site, parameter.id(), 1)
            .unwrap(),
        first
    );
    assert_eq!(
        registry
            .existing_bound_ticket(&mut accounting, site, parameter.id(), 0)
            .unwrap(),
        second
    );
    assert_eq!(registry.bounds.len(), 1);
}

#[test]
fn existing_only_and_invalid_ordinals_cannot_admit() {
    let fixture = fixture(TEXT, None);
    let parameter = parameter(&fixture, 0);
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    assert!(matches!(
        registry.existing_bound_ticket(&mut accounting, fixture.site, parameter.id(), 0),
        Err(AnalysisProofRegistryFailure::UnadmittedParameter { .. })
    ));
    assert!(matches!(
        registry.bound_ticket(&mut accounting, fixture.site, parameter, 2, || panic!(
            "invalid bound cannot request allowance"
        )),
        Err(AnalysisProofRegistryFailure::Input(
            AnalysisProofInputFailure::UnknownDeclaredBound { index: 2, .. }
        ))
    ));
    let ticket = registry
        .bound_ticket(&mut accounting, fixture.site, parameter, 1, || ALLOWANCE)
        .unwrap();
    assert!(matches!(
        registry.existing_bound_ticket(&mut accounting, fixture.site, parameter.id(), 0),
        Err(AnalysisProofRegistryFailure::Input(
            AnalysisProofInputFailure::UnadmittedBound { index: 0, .. }
        ))
    ));
    assert_eq!(ticket.identity().requirement(), 0);
    assert_eq!(registry.bounds.len(), 1);
}

#[test]
fn foreign_accounting_is_rejected_before_allowance_or_publication() {
    let fixture = fixture(TEXT, None);
    let first = accounting();
    let mut registry = registry(&first, &fixture);
    let mut foreign = accounting();
    assert!(matches!(
        registry.bound_ticket(
            &mut foreign,
            fixture.site,
            parameter(&fixture, 0),
            0,
            || panic!("foreign attempt")
        ),
        Err(AnalysisProofRegistryFailure::Input(
            AnalysisProofInputFailure::Work(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::ForeignAttempt
            ))
        ))
    ));
    assert!(registry.bounds.is_empty());
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
fn refusal_preserves_prior_entries_and_prevents_later_policy_calls() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    let first = registry
        .bound_ticket(
            &mut accounting,
            fixture.site,
            parameter(&fixture, 0),
            0,
            || ALLOWANCE,
        )
        .unwrap();
    let root = registry.occurrences[fixture.site.expression().local() as usize].bounds;
    let failed = registry.bound_ticket(
        &mut accounting,
        fixture.site,
        parameter(&fixture, 1),
        0,
        || 0,
    );
    let Err(AnalysisProofRegistryFailure::Input(AnalysisProofInputFailure::Work(
        AnalysisWorkFailure::Incomplete(stop),
    ))) = failed
    else {
        panic!("typed resource stop expected")
    };
    assert_eq!(registry.bounds.len(), 1);
    assert_eq!(
        registry.occurrences[fixture.site.expression().local() as usize].bounds,
        root
    );
    assert_eq!(registry.bounds[0].anchor_ticket(), &first);
    let repeated = registry.bound_ticket(
        &mut accounting,
        fixture.site,
        parameter(&fixture, 0),
        1,
        || panic!("sticky stop cannot call policy"),
    );
    assert!(
        matches!(repeated, Err(AnalysisProofRegistryFailure::Input(AnalysisProofInputFailure::Work(AnalysisWorkFailure::Incomplete(found)))) if found == stop)
    );
}

#[test]
fn dispatch_roles_retain_independent_tickets_without_renewal() {
    let fixture = fixture(TEXT, None);
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    let sites = [
        MethodDispatchSite::Invocation(fixture.site),
        MethodDispatchSite::CallableValue(callable_value(&fixture)),
    ];
    let mut tickets = Vec::new();
    for site in sites {
        assert!(
            matches!(registry.existing_dispatch_ticket(&mut accounting, site), Err(AnalysisProofRegistryFailure::UnadmittedDispatch(found)) if found == site)
        );
        tickets.push(
            registry
                .dispatch_ticket(&mut accounting, site, || ALLOWANCE)
                .unwrap(),
        );
    }
    assert_ne!(tickets[0], tickets[1]);
    for (site, ticket) in sites.into_iter().zip(&tickets) {
        let before = accounting.balance(ticket).unwrap().charged_ticks();
        for _ in 0..3 {
            assert_eq!(
                registry
                    .dispatch_ticket(&mut accounting, site, || panic!("dispatch replay"))
                    .unwrap(),
                *ticket
            );
            assert_eq!(
                registry
                    .existing_dispatch_ticket(&mut accounting, site)
                    .unwrap(),
                *ticket
            );
        }
        assert!(accounting.balance(ticket).unwrap().charged_ticks() > before);
    }
    assert_eq!(registry.dispatch.len(), sites.len());
    assert_eq!(
        accounting
            .admit(0, ALLOWANCE)
            .unwrap()
            .identity()
            .requirement(),
        sites.len()
    );
}

#[test]
fn source_shapes_and_dispatch_roles_are_checked_before_admission() {
    let fixture = fixture(TEXT, None);
    let value = callable_value(&fixture);
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    for site in [
        MethodDispatchSite::CallableValue(fixture.site),
        MethodDispatchSite::Invocation(value),
    ] {
        assert!(
            matches!(registry.dispatch_ticket(&mut accounting, site, || panic!("invalid role")), Err(AnalysisProofRegistryFailure::NotOriginalInput(found)) if found == site.key())
        );
    }
    let wrong_module =
        ExpressionKey::new(ModuleId::from_index(1).unwrap(), fixture.site.expression());
    let wrong_body = fixture
        .hir
        .bodies()
        .find(|(id, _)| *id != fixture.site.expression().body())
        .unwrap()
        .1;
    let wrong_body = ExpressionKey::new(
        fixture.site.module(),
        wrong_body.expressions().last().unwrap().0,
    );
    let name = fixture
        .hir
        .body(fixture.site.expression().body())
        .expressions()
        .find(|(_, expression)| matches!(expression.kind(), ExprKind::Name(_)))
        .unwrap()
        .0;
    let name = ExpressionKey::new(fixture.site.module(), name);
    for site in [wrong_module, wrong_body] {
        assert!(
            matches!(registry.bound_ticket(&mut accounting, site, parameter(&fixture, 0), 0, || panic!("foreign source")), Err(AnalysisProofRegistryFailure::NotOriginalInput(found)) if found == site)
        );
    }
    // A written name can be a callable input, but a generated postfix call cannot.
    registry
        .bound_ticket(&mut accounting, name, parameter(&fixture, 0), 0, || {
            ALLOWANCE
        })
        .unwrap();
    let generated = fixture_fn("fn inspect() -> Unit = pick.then(pick)");
    let body = generated.hir.body(generated.site.expression().body());
    let mut generated_registry =
        AnalysisProofInputRegistry::new(&accounting, 0, generated.site.module(), body).unwrap();
    assert!(matches!(
        generated_registry.bound_ticket(
            &mut accounting,
            generated.site,
            parameter(&generated, 0),
            0,
            || panic!("generated call")
        ),
        Err(AnalysisProofRegistryFailure::NotOriginalInput(_))
    ));
}

fn fixture_fn(body: &str) -> Fixture {
    fixture(
        &format!(
            "trait Alpha {{}}\ntrait Zulu {{}}\nfn pick[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = ()\n{body}"
        ),
        None,
    )
}

fn trace(
    registry: &mut AnalysisProofInputRegistry<'_>,
    accounting: &mut AnalysisWorkAccounting,
    fixture: &Fixture,
    allowances: &[u64],
) -> Result<(), AnalysisProofRegistryFailure> {
    let next = Cell::new(0);
    let allowance = || {
        let index = next.get();
        next.set(index + 1);
        allowances[index]
    };
    for (parameter_index, bound_index) in [(0, 0), (0, 0), (0, 1), (1, 1), (1, 0)] {
        registry.bound_ticket(
            accounting,
            fixture.site,
            parameter(fixture, parameter_index),
            bound_index,
            allowance,
        )?;
    }
    registry.existing_bound_ticket(accounting, fixture.site, parameter(fixture, 1).id(), 1)?;
    for site in [
        MethodDispatchSite::Invocation(fixture.site),
        MethodDispatchSite::CallableValue(callable_value(fixture)),
    ] {
        registry.dispatch_ticket(accounting, site, allowance)?;
        registry.dispatch_ticket(accounting, site, || panic!("replay cannot admit"))?;
        registry.existing_dispatch_ticket(accounting, site)?;
    }
    Ok(())
}

#[test]
fn every_registration_or_retrieval_charge_can_interrupt_without_publication() {
    let fixture = fixture(TEXT, None);
    let mut complete = accounting();
    observe(&mut complete);
    let mut full = registry(&complete, &fixture);
    trace(&mut full, &mut complete, &fixture, &[ALLOWANCE; 6]).unwrap();
    let events = CHARGES.with_borrow(Clone::clone);
    let mut prefixes = [0; 6];
    for event in events {
        let requirement = event.root.requirement();
        let mut allowances = [ALLOWANCE; 6];
        allowances[requirement] = prefixes[requirement];
        let mut limited = accounting();
        let mut inputs = registry(&limited, &fixture);
        let result = trace(&mut inputs, &mut limited, &fixture, &allowances);
        let Err(AnalysisProofRegistryFailure::Input(AnalysisProofInputFailure::Work(
            AnalysisWorkFailure::Incomplete(stop),
        ))) = result
        else {
            panic!("charge must stop before completion: {event:?}")
        };
        assert_eq!(limited.incomplete(), Some(stop));
        assert_eq!(stop.root(), event.root);
        assert!(inputs.bounds.len() <= full.bounds.len());
        assert!(inputs.dispatch.len() <= full.dispatch.len());
        for holder in &inputs.bounds {
            assert!(
                full.bounds
                    .iter()
                    .any(|original| original.parameter() == holder.parameter())
            );
        }
        assert!(
            matches!(inputs.dispatch_ticket(&mut limited, MethodDispatchSite::Invocation(fixture.site), || panic!("sticky stop")), Err(AnalysisProofRegistryFailure::Input(AnalysisProofInputFailure::Work(AnalysisWorkFailure::Incomplete(found)))) if found == stop)
        );
        prefixes[requirement] += event.ticks;
    }
    let mut exact = accounting();
    let mut inputs = registry(&exact, &fixture);
    trace(&mut inputs, &mut exact, &fixture, &prefixes).unwrap();
    assert!(exact.incomplete().is_none());
}

#[test]
fn fixed_header_reservations_obey_the_analytical_growth_envelope() {
    use crate::record_reservation::observation::{assert_growth_envelope, observe};

    let parameters = (0..64)
        .map(|index| format!("T{index}: Alpha"))
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!(
        "trait Alpha {{}}\nfn pick[{parameters}]() -> Unit = ()\nfn inspect() -> Unit = pick()"
    );
    let fixture = fixture(&text, None);
    let mut accounting = accounting();
    self::observe(&mut accounting);
    let mut registry = registry(&accounting, &fixture);
    let (_, observations) = observe(|| {
        for index in 0..fixture.parameters.len() {
            registry
                .bound_ticket(
                    &mut accounting,
                    fixture.site,
                    parameter(&fixture, index),
                    0,
                    || ALLOWANCE,
                )
                .unwrap();
        }
    });
    assert_eq!(registry.bounds.len(), fixture.parameters.len());
    // Each admission also publishes one accounting record. All observations use
    // the same checked schedule, so their headers can be verified together.
    let published_bytes = observations.iter().map(|event| event.record_bytes).sum();
    assert_growth_envelope(&observations, published_bytes);
    assert!(
        observations
            .iter()
            .any(|event| event.len > 0 && event.requested_capacity.is_some())
    );
    let charges = CHARGES.with_borrow(Clone::clone);
    let requested: u64 = observations
        .iter()
        .filter_map(|event| {
            event
                .requested_capacity
                .map(|capacity| (capacity * event.record_bytes) as u64)
        })
        .sum();
    let possible_relocation: u64 = observations
        .iter()
        .filter(|event| event.requested_capacity.is_some())
        .map(|event| (event.len * event.record_bytes) as u64)
        .sum();
    assert!(requested <= charges.iter().map(|event| event.work.reserved_bytes).sum());
    assert!(
        possible_relocation
            <= charges
                .iter()
                .map(|event| event.work.transferred_bytes)
                .sum()
    );
}

#[test]
fn replay_pays_for_lookup_without_storage_or_payload_preparation() {
    let parameters = (0..64)
        .map(|index| format!("T{index}: Alpha"))
        .collect::<Vec<_>>()
        .join(", ");
    let fixture = fixture(
        &format!(
            "trait Alpha {{}}\nfn pick[{parameters}]() -> Unit = ()\nfn inspect() -> Unit = pick()"
        ),
        None,
    );
    let mut accounting = accounting();
    let mut registry = registry(&accounting, &fixture);
    for index in 0..fixture.parameters.len() {
        registry
            .bound_ticket(
                &mut accounting,
                fixture.site,
                parameter(&fixture, index),
                0,
                || ALLOWANCE,
            )
            .unwrap();
    }
    for index in 0..fixture.parameters.len() {
        observe(&mut accounting);
        registry
            .bound_ticket(
                &mut accounting,
                fixture.site,
                parameter(&fixture, index),
                0,
                || panic!("replay"),
            )
            .unwrap();
        let visits = registry.index.inspected_branches() as u64;
        let charges = CHARGES.with_borrow(Clone::clone);
        assert!(visits > 0);
        assert!(charges.iter().map(|event| event.work.steps).sum::<u64>() >= visits);
        assert!(
            charges
                .iter()
                .map(|event| event.work.inspected_bytes)
                .sum::<u64>()
                >= visits * BoundIndex::branch_size() as u64
        );
        assert!(
            charges
                .iter()
                .map(|event| event.work.transferred_bytes)
                .sum::<u64>()
                >= size_of::<AnalysisRootTicket>() as u64
        );
        assert_eq!(
            charges
                .iter()
                .map(|event| event.work.reserved_bytes)
                .sum::<u64>(),
            0
        );
    }
}
