use std::cell::Cell;

use super::*;
use crate::diagnostic::observation::{self, RootEvidence};
use crate::{AnalysisRootTicket, BodyTypingPrefix};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Event {
    Charge(ChargeEvent),
    Diagnostic(RootEvidence),
    Request(ProofRequest),
    Stage(ProofStage),
    Published(EvidencePublication),
    BodyStarted(usize),
    BodyCompleted(usize),
    EvidenceStarted(usize),
    EvidenceCompleted(usize),
}

thread_local! {
    static TRACE: RefCell<Option<Vec<Event>>> = const { RefCell::new(None) };
}

fn observe(event: Event) {
    TRACE.with_borrow_mut(|trace| {
        if let Some(trace) = trace {
            trace.push(event);
        }
    });
}

pub(super) fn observe_request(request: ProofRequest) {
    observe(Event::Request(request));
}

pub(super) fn observe_stage(stage: ProofStage) {
    observe(Event::Stage(stage));
}

pub(super) fn observe_publication(publication: EvidencePublication) {
    observe(Event::Published(publication));
}

fn capture<R>(run: impl FnOnce() -> R) -> (R, Vec<Event>) {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            TRACE.with_borrow_mut(|trace| *trace = None);
        }
    }
    TRACE.with_borrow_mut(|trace| {
        assert!(trace.is_none());
        *trace = Some(Vec::new());
    });
    let restore = Restore;
    let result = observation::capture(|root| observe(Event::Diagnostic(root)), run);
    let trace = TRACE.with_borrow_mut(|trace| trace.take().unwrap());
    drop(restore);
    (result, trace)
}

#[derive(Debug, Eq, PartialEq)]
struct BodySnapshot {
    expressions: Vec<(ExpressionKey, String)>,
    bodies: Vec<(DefId, String)>,
    transfers: Vec<String>,
    facts: usize,
}

fn expression_keys(fixture: &Fixture<'_, '_>) -> Vec<ExpressionKey> {
    let mut keys = Vec::new();
    for module in fixture.collection.modules() {
        let hir = fixture
            .sources
            .iter()
            .find(|source| source.file() == module.file())
            .unwrap()
            .hir()
            .unwrap();
        for (_, body) in hir.bodies() {
            keys.extend(
                body.expressions()
                    .map(|(id, _)| ExpressionKey::new(module.id(), id)),
            );
        }
    }
    keys
}

fn recorded<T: std::fmt::Debug>(fact: FactState<'_, T>) -> Option<String> {
    if matches!(fact, FactState::Unperformed) {
        None
    } else {
        Some(format!("{fact:?}"))
    }
}

impl BodySnapshot {
    fn complete(fixture: &Fixture<'_, '_>, body: &BodyTypes) -> Self {
        Self {
            expressions: expression_keys(fixture)
                .into_iter()
                .filter_map(|key| recorded(body.expression(key)).map(|value| (key, value)))
                .collect(),
            bodies: fixture
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .filter_map(|declaration| {
                    recorded(body.body(declaration.id())).map(|value| (declaration.id(), value))
                })
                .collect(),
            transfers: body
                .transfers()
                .iter()
                .map(|transfer| format!("{transfer:?}"))
                .collect(),
            facts: body.fact_count(),
        }
    }

    fn prefix(fixture: &Fixture<'_, '_>, body: &BodyTypingPrefix) -> Self {
        let expressions = expression_keys(fixture)
            .into_iter()
            .filter_map(|key| {
                let fact = body.expression(key);
                if let FactState::Known(typed) = fact {
                    assert!(
                        fixture
                            .declarations
                            .signatures()
                            .types()
                            .kind(typed.ty())
                            .is_ok()
                    );
                }
                recorded(fact).map(|value| (key, value))
            })
            .collect();
        Self {
            expressions,
            bodies: fixture
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .filter_map(|declaration| {
                    recorded(body.body(declaration.id())).map(|value| (declaration.id(), value))
                })
                .collect(),
            transfers: body
                .transfers()
                .iter()
                .map(|transfer| format!("{transfer:?}"))
                .collect(),
            facts: body.fact_count(),
        }
    }

    fn assert_retained_from(&self, complete: &Self) {
        assert!(self.facts <= complete.facts);
        for value in &self.expressions {
            assert!(complete.expressions.contains(value));
        }
        for value in &self.bodies {
            assert!(complete.bodies.contains(value));
        }
        for value in &self.transfers {
            assert!(complete.transfers.contains(value));
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProofTickets {
    bounds: Vec<AnalysisRootTicket>,
    dispatches: Vec<AnalysisRootTicket>,
}

impl ProofTickets {
    fn identities(&self) -> ProofIdentities {
        ProofIdentities {
            bounds: self
                .bounds
                .iter()
                .map(AnalysisRootTicket::identity)
                .collect(),
            dispatches: self
                .dispatches
                .iter()
                .map(AnalysisRootTicket::identity)
                .collect(),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ProofIdentities {
    bounds: Vec<AnalysisRootIdentity>,
    dispatches: Vec<AnalysisRootIdentity>,
}

impl ProofIdentities {
    fn root(&self, request: ProofRequest) -> AnalysisRootIdentity {
        match request {
            ProofRequest::Bound(index) => self.bounds[index],
            ProofRequest::Dispatch(index) => self.dispatches[index],
        }
    }
    fn assert_retained_from(&self, complete: &Self) {
        assert!(complete.bounds.starts_with(&self.bounds));
        assert!(complete.dispatches.starts_with(&self.dispatches));
    }
}

fn tickets(
    inputs: &BodyProofInputs,
    bounds: &[PendingTraitObligation],
    dispatches: &[PendingMethodDispatch],
    signatures: &DeclarationSignatures,
    accounting: &AnalysisWorkAccounting,
) -> ProofTickets {
    let BodyProofInputs::Metered(inputs) = inputs else {
        panic!("metered body custody missing")
    };
    assert_eq!(
        inputs.validate_accounting(accounting),
        accounting
            .incomplete()
            .map_or(Ok(()), |stop| Err(AnalysisWorkFailure::Incomplete(stop)))
    );
    assert_eq!(inputs.bounds().len(), bounds.len());
    assert_eq!(inputs.dispatches().len(), dispatches.len());
    let mut tickets = ProofTickets {
        bounds: Vec::new(),
        dispatches: Vec::new(),
    };
    for (input, pending) in inputs.bounds().iter().zip(bounds) {
        let FactState::Known(record) = signatures.generic_parameter_record(pending.parameter())
        else {
            panic!("original record unavailable")
        };
        assert_eq!(input.site(), pending.site());
        assert_eq!(input.parameter(), record.id());
        assert_eq!(input.bound_index(), pending.bound_index());
        accounting.balance(input.ticket()).unwrap();
        tickets.bounds.push(input.ticket().clone());
    }
    for (input, pending) in inputs.dispatches().iter().zip(dispatches) {
        assert_eq!(input.site(), pending.site());
        accounting.balance(input.ticket()).unwrap();
        tickets.dispatches.push(input.ticket().clone());
    }
    tickets
}

#[derive(Debug)]
enum Partial {
    Body(BodySnapshot, ProofIdentities),
    Evidence(Snapshot),
}

struct Run {
    events: Vec<Event>,
    policy: Vec<BodyAnalysisInput>,
    products: Vec<(BodySnapshot, Snapshot, ProofIdentities)>,
    partial: Option<(usize, Partial)>,
    stop: Option<AnalysisWorkIncomplete>,
}

struct Input<'a> {
    text: &'a str,
    external: &'a ExternalNameEnvironment,
    trusted: Option<&'a crate::TrustedSignatureEnvironment>,
    effects: Option<&'a crate::TrustedEffectEnvironment>,
    valid: bool,
}

fn run(input: &Input<'_>, cut: Option<(BodyAnalysisInput, u64)>) -> Run {
    let mut result = None;
    let ((), events) = capture(|| {
        with_fixture(
            input.text,
            input.external,
            input
                .trusted
                .map(|trusted| crate::TrustedDeclarationInputs::new(trusted, None)),
            |mut fixture| {
                let mut meter = accounting();
                meter.observe_charges(|event| observe(Event::Charge(event)));
                let mut sources = AnalysisSourceInputs::new(
                    &meter,
                    fixture.sources.iter().map(|source| (0, source)),
                )
                .unwrap();
                let mut bodies = AnalysisBodyInputs::new(
                    &meter,
                    fixture.collection.modules().map(|module| {
                        (
                            0,
                            module,
                            fixture
                                .sources
                                .iter()
                                .find(|source| source.file() == module.file())
                                .unwrap(),
                        )
                    }),
                )
                .unwrap();
                let pass = Cell::new(0);
                let policy = RefCell::new(Vec::new());
                let allowance = |original| {
                    assert_eq!(pass.get(), 0, "replay requested a new allowance");
                    policy.borrow_mut().push(original);
                    cut.filter(|(target, _)| *target == original)
                        .map_or(ALLOWANCE, |(_, amount)| amount)
                };
                let mut products = Vec::new();
                let mut completed = Vec::new();
                let mut first_tickets = None;
                let mut stopped = None;
                let mut partial = None;
                for ordinal in 0..2 {
                    pass.set(ordinal);
                    observe(Event::BodyStarted(ordinal));
                    let execution = typecheck_bodies_with_work(
                        fixture.collection,
                        fixture.imports,
                        fixture.names,
                        Some(input.external),
                        fixture.sources,
                        &Support,
                        fixture.declarations,
                        fixture.trait_environment,
                        fixture.diagnostics,
                        BodyAnalysisWork::new(&mut meter, &mut sources, &mut bodies, &allowance),
                    )
                    .unwrap();
                    let packet = match execution {
                        BodyExecution::Completed(packet) => packet,
                        BodyExecution::Interrupted(body) => {
                            assert!(body.internal_failures().is_empty());
                            let retained = tickets(
                                body.proof_inputs(),
                                body.prefix().obligations(),
                                body.prefix().method_dispatches(),
                                fixture.declarations.signatures(),
                                &meter,
                            );
                            stopped = Some(body.stop());
                            partial = Some((
                                ordinal,
                                Partial::Body(
                                    BodySnapshot::prefix(&fixture, body.prefix()),
                                    retained.identities(),
                                ),
                            ));
                            break;
                        }
                    };
                    observe(Event::BodyCompleted(ordinal));
                    let PhaseProduct::Performed(body) = packet.analysis().product() else {
                        panic!("body missing")
                    };
                    let current_tickets = tickets(
                        packet.proof_inputs(),
                        body.obligations(),
                        body.method_dispatches(),
                        fixture.declarations.signatures(),
                        &meter,
                    );
                    if let Some(first) = &first_tickets {
                        assert_eq!(&current_tickets, first);
                    } else {
                        first_tickets = Some(current_tickets.clone());
                    }
                    observe(Event::EvidenceStarted(ordinal));
                    let (attempt, _) =
                        record_stages(|| evidence(&mut fixture, &packet, &mut meter).unwrap());
                    let phase = match attempt {
                        TraitEvidenceExecution::Completed(phase) => phase,
                        TraitEvidenceExecution::Interrupted(evidence) => {
                            assert!(evidence.internal_failures().is_empty());
                            assert_eq!(
                                evidence.validate_accounting(&meter),
                                Err(AnalysisWorkFailure::Incomplete(evidence.stop()))
                            );
                            super::sweep::validate_backing(
                                evidence.prefix(),
                                fixture.declarations.signatures(),
                            );
                            stopped = Some(evidence.stop());
                            partial = Some((
                                ordinal,
                                Partial::Evidence(Snapshot::prefix(evidence.prefix())),
                            ));
                            break;
                        }
                    };
                    observe(Event::EvidenceCompleted(ordinal));
                    let PhaseProduct::Performed(evidence) = phase.product() else {
                        panic!("evidence missing")
                    };
                    products.push((
                        BodySnapshot::complete(&fixture, body),
                        Snapshot::complete(evidence),
                        current_tickets.identities(),
                    ));
                    completed.push((packet, phase));
                }
                assert_eq!(stopped, meter.incomplete());
                if cut.is_none() {
                    assert!(stopped.is_none());
                    assert_eq!(products.len(), 2);
                    assert_eq!(products[0], products[1]);
                    assert_eq!(fixture.diagnostics.has_errors(), !input.valid);
                    if input.valid {
                        let mut fingerprints = Vec::new();
                        for (packet, phase) in &completed {
                            let PhaseProduct::Performed(body) = packet.analysis().product() else {
                                unreachable!()
                            };
                            fingerprints.push(fingerprint_with_effects(
                                &mut fixture,
                                body,
                                phase.product(),
                                input.effects,
                            ));
                        }
                        assert_eq!(fingerprints[0], fingerprints[1]);
                        assert!(!fixture.diagnostics.has_errors());
                        let PhaseProduct::Performed(body) = completed[0].0.analysis().product()
                        else {
                            unreachable!()
                        };
                        let normal = resolve_trait_evidence(
                            fixture.collection,
                            fixture.sources,
                            &Support,
                            fixture.declarations.signatures_mut(),
                            fixture.trait_environment,
                            body,
                            fixture.diagnostics,
                        )
                        .unwrap();
                        let PhaseProduct::Performed(normal_facts) = normal.product() else {
                            panic!("normal evidence missing")
                        };
                        assert_eq!(Snapshot::complete(normal_facts), products[0].1);
                        assert_eq!(
                            fingerprint_with_effects(
                                &mut fixture,
                                body,
                                normal.product(),
                                input.effects
                            ),
                            fingerprints[0]
                        );
                    }
                } else {
                    assert!(stopped.is_some(), "cut execution completed");
                }
                result = Some(Run {
                    events: Vec::new(),
                    policy: policy.into_inner(),
                    products,
                    partial,
                    stop: stopped,
                });
            },
        )
    });
    let mut result = result.unwrap();
    result.events = events;
    result
}

fn verify(input: &Input<'_>) {
    let full = run(input, None);
    assert_eq!(
        full.events
            .iter()
            .any(|event| matches!(event, Event::Diagnostic(_))),
        !input.valid
    );
    let mut evidence_pass = None;
    let mut expected_root = None;
    for event in &full.events {
        match event {
            Event::EvidenceStarted(pass) => {
                evidence_pass = Some(*pass);
                expected_root = None;
            }
            Event::EvidenceCompleted(_) | Event::BodyStarted(_) => {
                evidence_pass = None;
                expected_root = None;
            }
            Event::Request(request) => {
                let pass = evidence_pass.expect("original evidence request outside its phase");
                expected_root = Some(full.products[pass].2.root(*request));
            }
            Event::Charge(charge) if evidence_pass.is_some() => {
                assert_eq!(
                    Some(charge.root),
                    expected_root,
                    "evidence charged another original demand"
                );
            }
            _ => {}
        }
    }
    let charges = full
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            Event::Charge(event) => Some((index, *event)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let admissions = charges
        .iter()
        .filter(|(_, event)| event.kind == ChargeKind::Admission)
        .collect::<Vec<_>>();
    assert_eq!(admissions.len(), full.policy.len());
    assert!(!charges.is_empty());
    for (ordinal, (index, charge)) in charges.iter().enumerate() {
        let input_index = admissions
            .iter()
            .position(|(_, admission)| admission.root == charge.root)
            .unwrap();
        let original = full.policy[input_index];
        let allowance = charges[..ordinal]
            .iter()
            .filter(|(_, earlier)| earlier.root == charge.root)
            .map(|(_, event)| event.ticks)
            .sum();
        let stopped = run(input, Some((original, allowance)));
        assert_eq!(
            stopped.events,
            full.events[..=*index],
            "continued after charge {ordinal}"
        );
        assert_eq!(stopped.stop.unwrap().root(), charge.root);
        assert_eq!(
            stopped.stop.unwrap().reason(),
            AnalysisRootStopReason::Exhausted {
                required_ticks: charge.ticks,
                remaining_ticks: 0
            }
        );
        assert_eq!(stopped.products, full.products[..stopped.products.len()]);
        let (pass, partial) = stopped.partial.unwrap();
        match partial {
            Partial::Body(body, tickets) => {
                body.assert_retained_from(&full.products[pass].0);
                tickets.assert_retained_from(&full.products[pass].2);
            }
            Partial::Evidence(evidence) => evidence.assert_retained_from(&full.products[pass].1),
        }
    }
}

#[test]
fn original_budgets_survive_body_replay_and_both_evidence_crossings_at_every_charge() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let mut sources = programs();
    sources.extend([
        "trait Mark {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: T) -> T = x.then(id)",
        "trait Mark {}\nenum Tagged[T: Mark] { Value(T) }\nfn inspect[T: Mark](x: T) -> Unit { _ = Tagged::Value(x) }",
        "trait Mark {}\ntype Alias[T] = T\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: Alias[T]) -> Alias[T] = id(x)",
        "trait Mark {}\ntrait Rank { fn rank(self: &Self) -> i64 }\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark+Rank](x: T) -> T { let first = x.rank()\nlet value = id(x)\nlet second = value.rank()\nyield id(value) }",
    ]);
    for source in sources {
        verify(&Input {
            text: source,
            external: &external,
            trusted: None,
            effects: None,
            valid: true,
        });
    }
    for source in [
        "trait Mark {}\nstruct Plain {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect(x: Plain) -> Plain = id(x)",
        "trait Mark {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: T) -> T { let invalid: Bool = 1\nyield id(x) }",
    ] {
        verify(&Input {
            text: source,
            external: &external,
            trusted: None,
            effects: None,
            valid: false,
        });
    }
}

#[test]
fn trusted_original_bounds_keep_their_budgets_through_both_crossings() {
    let (external, trusted) = fixture::trusted_contracts();
    let effects = crate::TrustedEffectEnvironment::new([crate::TrustedEffectSummary::new(
        crate::ExternalDefId::new("std::probe::pick").unwrap(),
        crate::EffectSummary::pure(),
    )])
    .unwrap();
    for body in [
        "= pick(value)",
        "{ let f: Fn(T) -> T = pick\nyield f(value) }",
    ] {
        let source = format!(
            "import std::probe::Alpha\nimport std::probe::Zulu\nimport std::probe::pick\nfn inspect[T: Alpha+Zulu](value: T) -> T {body}"
        );
        verify(&Input {
            text: &source,
            external: &external,
            trusted: Some(&trusted),
            effects: Some(&effects),
            valid: true,
        });
    }
}
