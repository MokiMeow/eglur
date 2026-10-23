use super::*;
use crate::{BodyProofInputs, GenericParameterRecordId, MethodDispatchSite};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Event {
    AllowanceRequest(BodyAnalysisInput),
    Preparation(BodyAnalysisInput),
}

thread_local! {
    static EVENTS: RefCell<Option<Vec<Event>>> = const { RefCell::new(None) };
    static CHARGES: RefCell<Vec<crate::analysis_work::ChargeEvent>> = const { RefCell::new(Vec::new()) };
}

struct Recording;

impl Drop for Recording {
    fn drop(&mut self) {
        EVENTS.with_borrow_mut(|events| *events = None);
    }
}

fn capture<R>(operation: impl FnOnce() -> R) -> (R, Vec<Event>) {
    EVENTS.with_borrow_mut(|events| {
        assert!(events.is_none());
        *events = Some(Vec::new());
    });
    let recording = Recording;
    let result = operation();
    let events = EVENTS.with_borrow_mut(|events| events.take().unwrap());
    drop(recording);
    (result, events)
}

fn record(event: Event) {
    EVENTS.with_borrow_mut(|events| {
        if let Some(events) = events {
            events.push(event);
        }
    });
}

pub(in crate::body_types::checker) fn before_bound_preparation(
    site: ExpressionKey,
    parameter: GenericParameterRecordId,
    index: usize,
) {
    record(Event::Preparation(BodyAnalysisInput::ParameterBound {
        site,
        parameter,
        index,
    }));
}

pub(in crate::body_types::checker) fn before_dispatch_preparation(site: MethodDispatchSite) {
    record(Event::Preparation(BodyAnalysisInput::MethodDispatch(site)));
}

pub(in crate::body_types::checker) fn sources() -> Vec<String> {
    let bounds = "trait Alpha {}\ntrait Zulu {}\nfn pick[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = ()\n";
    let inspect = "fn inspect[T: Alpha+Zulu, U: Alpha+Zulu](x: T, y: U) -> Unit";
    let methods = "trait Rank { fn rank(self: &Self) -> i64 }\n";
    vec![
        format!("{bounds}{inspect} = pick(x, y)"),
        format!("{bounds}{inspect} {{ let f: Fn(T, U) -> Unit = pick\nyield f(x, y) }}"),
        format!("{bounds}{inspect} = (x, y).apply(pick)"),
        "trait Mark {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: T) -> T = x.then(id)".into(),
        format!("{methods}fn inspect[T: Rank](x: &T) -> i64 = x.rank()"),
        format!("{methods}fn inspect[T: Rank](x: &T) -> i64 = Rank.rank(x)"),
        format!("{methods}fn inspect[T: Rank](x: &T) -> i64 {{ let f: Fn(&T) -> i64 = Rank.rank\nyield f(x) }}"),
        "trait Reducer { fn reduce_by(self: &Self, pair: (i64, i64)) -> i64 }\nfn inspect[T: Reducer](x: &T, value: i64) -> i64 = x.reduce_by((value, value))".into(),
        "trait Mark {}\nstruct Tagged[T: Mark] { field value: T }\nfn inspect[T: Mark](x: T) -> Unit { let tagged = Tagged { value: x } }".into(),
        "trait Mark {}\nenum Tagged[T: Mark] { Value(T) }\nfn inspect[T: Mark](x: T) -> Unit { _ = Tagged::Value(x) }".into(),
    ]
}

fn verify_replay(
    mut inputs: BodyPhaseInputs<'_, '_>,
    external: &ExternalNameEnvironment,
    text: &str,
    effects: Option<&crate::TrustedEffectEnvironment>,
) {
    let mut accounting = accounting();
    let (mut source_inputs, mut body_inputs) = analysis_inputs(&accounting, &inputs);
    let mut first = None;
    for pass in 0..2 {
        let policy = |input| {
            record(Event::AllowanceRequest(input));
            10_000_000
        };
        let (execution, events) = capture(|| {
            run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(
                    &mut accounting,
                    &mut source_inputs,
                    &mut body_inputs,
                    &policy,
                ),
            )
            .unwrap()
        });
        let BodyExecution::Completed(packet) = execution else {
            panic!("unexpected resource stop: {text}")
        };
        let PhaseProduct::Performed(product) = packet.analysis().product() else {
            panic!("body did not perform: {text}")
        };
        let BodyProofInputs::Metered(connections) = packet.proof_inputs() else {
            panic!("missing metered custody")
        };
        connections.validate_accounting(&accounting).unwrap();
        assert_eq!(connections.bounds().len(), product.obligations().len());
        assert_eq!(
            connections.dispatches().len(),
            product.method_dispatches().len()
        );
        let mut tickets = Vec::new();
        for (connection, pending) in connections.bounds().iter().zip(product.obligations()) {
            assert_eq!(connection.site(), pending.site());
            assert_eq!(connection.bound_index(), pending.bound_index());
            let FactState::Known(parameter) = inputs
                .declarations
                .signatures()
                .generic_parameter_record(pending.parameter())
            else {
                panic!("original definition missing")
            };
            assert_eq!(connection.parameter(), parameter.id());
            assert_eq!(
                body_inputs
                    .existing_bound_ticket(
                        &mut accounting,
                        pending.site(),
                        parameter.id(),
                        pending.bound_index()
                    )
                    .unwrap(),
                *connection.ticket()
            );
            tickets.push(connection.ticket().clone());
        }
        for (connection, pending) in connections
            .dispatches()
            .iter()
            .zip(product.method_dispatches())
        {
            assert_eq!(connection.site(), pending.site());
            assert_eq!(
                body_inputs
                    .existing_dispatch_ticket(&mut accounting, pending.site())
                    .unwrap(),
                *connection.ticket()
            );
            tickets.push(connection.ticket().clone());
        }
        assert!(
            !tickets.is_empty(),
            "fixture did not exercise proof work: {text}"
        );
        let fingerprint = crate::body_types::checker::test_support::fingerprints_with_effects(
            &mut inputs,
            product,
            effects,
        );
        if inputs.diagnostics.has_errors() {
            let diagnostics =
                std::mem::replace(inputs.diagnostics, DiagnosticStore::new()).finalize(None);
            panic!("invalid fixture: {text}\n{diagnostics:?}");
        }
        if pass == 0 {
            for (position, event) in events.iter().enumerate() {
                if let Event::Preparation(input) = event {
                    assert!(
                        events[..position].contains(&Event::AllowanceRequest(*input)),
                        "preparation preceded admission: {input:?}"
                    );
                }
            }
            first = Some((tickets, snapshot(product), fingerprint));
        } else {
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Event::AllowanceRequest(_))),
                "replay admitted new work: {events:?}"
            );
            let (previous, facts, previous_fingerprint) = first.as_ref().unwrap();
            assert_eq!(&tickets, previous);
            assert_eq!(&snapshot(product), facts);
            assert_eq!(&fingerprint, previous_fingerprint);
        }
    }
}

#[test]
fn actual_body_proof_connections_preserve_original_slots_and_replay() {
    for text in sources() {
        with_inputs(&text, |inputs, external| {
            verify_replay(inputs, external, &text, None)
        });
    }
}

#[test]
fn exhausted_original_proof_inputs_stop_before_their_preparation() {
    for text in sources() {
        with_inputs(&text, |mut inputs, external| {
            let mut accounting = accounting();
            let (mut source_inputs, mut body_inputs) = analysis_inputs(&accounting, &inputs);
            let (execution, events) = capture(|| {
                run(
                    &mut inputs,
                    external,
                    BodyAnalysisWork::new(
                        &mut accounting,
                        &mut source_inputs,
                        &mut body_inputs,
                        &|input| {
                            record(Event::AllowanceRequest(input));
                            if matches!(
                                input,
                                BodyAnalysisInput::ParameterBound { .. }
                                    | BodyAnalysisInput::MethodDispatch(_)
                            ) {
                                0
                            } else {
                                ALLOWANCE
                            }
                        },
                    ),
                )
                .unwrap()
            });
            assert!(matches!(execution, BodyExecution::Interrupted(_)), "{text}");
            assert!(events.iter().any(|event| matches!(
                event,
                Event::AllowanceRequest(
                    BodyAnalysisInput::ParameterBound { .. } | BodyAnalysisInput::MethodDispatch(_)
                )
            )));
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Event::Preparation(_))),
                "proof input was prepared after refused admission: {text}\n{events:?}"
            );
        });
    }
}

#[test]
fn empty_metered_packets_still_reject_foreign_accounting() {
    with_inputs("fn inspect() -> Unit = ()", |mut inputs, external| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let BodyExecution::Completed(packet) = run(
            &mut inputs,
            external,
            BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| ALLOWANCE),
        )
        .unwrap() else {
            panic!("completed body")
        };
        let BodyProofInputs::Metered(connections) = packet.proof_inputs() else {
            panic!("mode missing")
        };
        assert!(connections.bounds().is_empty());
        assert!(connections.dispatches().is_empty());
        connections.validate_accounting(&accounting).unwrap();
        let foreign =
            AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(1, 1, 1).unwrap(), 1)
                .unwrap();
        assert_eq!(
            connections.validate_accounting(&foreign),
            Err(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::ForeignAttempt
            ))
        );
    });
}

#[test]
fn trusted_bound_contracts_keep_their_declared_order_and_original_tickets() {
    use crate::{
        ExternalDeclaration, ExternalDefId, InherentMemberForm, ModuleAddress,
        TrustedCallableParameter, TrustedDeclarationInputs, TrustedDeclarationSignature,
        TrustedGenericParameter, TrustedInherentMember, TrustedInherentOwner,
        TrustedInherentOwnerHead, TrustedSignatureEnvironment, TrustedSignatureKind,
        TrustedTraitExpr, TrustedTypeExpr,
    };

    let module = ModuleAddress::new(["std".into(), "probe".into()]).unwrap();
    let id = |name: &str| ExternalDefId::new(name).unwrap();
    let mut declarations = Vec::new();
    let mut signatures = Vec::new();
    for name in ["Alpha", "Zulu"] {
        let target = id(&format!("std::probe::{name}"));
        declarations.push(
            ExternalDeclaration::new(
                target.clone(),
                module.clone(),
                NameKey::new(name, false).unwrap(),
                DeclarationKind::Trait,
            )
            .unwrap(),
        );
        signatures.push(TrustedDeclarationSignature::new(
            target,
            TrustedSignatureKind::Trait {
                closed: false,
                generic_parameters: Box::new([]),
                associated_types: Box::new([]),
            },
        ));
    }
    let pick = id("std::probe::pick");
    declarations.push(
        ExternalDeclaration::new(
            pick.clone(),
            module.clone(),
            NameKey::new("pick", false).unwrap(),
            DeclarationKind::Function,
        )
        .unwrap(),
    );
    signatures.push(TrustedDeclarationSignature::new(
        pick,
        TrustedSignatureKind::Callable {
            bang: false,
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "T".into(),
                bounds: ["std::probe::Zulu", "std::probe::Alpha"]
                    .map(|name| TrustedTraitExpr {
                        declaration: id(name),
                        arguments: Box::new([]),
                    })
                    .into(),
            }]),
            parameters: Box::new([TrustedCallableParameter {
                name: Some("value".into()),
                ty: TrustedTypeExpr::GenericParameter(0),
            }]),
            result: TrustedTypeExpr::GenericParameter(0),
            requirements: Box::new([]),
        },
    ));
    let bag = id("std::probe::Bag");
    let read = id("std::probe::Bag::read");
    declarations.push(
        ExternalDeclaration::new(
            bag.clone(),
            module.clone(),
            NameKey::new("Bag", false).unwrap(),
            DeclarationKind::OpaqueType,
        )
        .unwrap(),
    );
    signatures.push(TrustedDeclarationSignature::new(
        bag.clone(),
        TrustedSignatureKind::Nominal {
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "T".into(),
                bounds: Box::new([]),
            }]),
        },
    ));
    let owner_pattern = TrustedTypeExpr::Nominal {
        head: bag.clone(),
        arguments: Box::new([TrustedTypeExpr::GenericParameter(0)]),
    };
    signatures.push(TrustedDeclarationSignature::new(
        read.clone(),
        TrustedSignatureKind::Callable {
            bang: false,
            generic_parameters: Box::new([TrustedGenericParameter {
                name: "T".into(),
                bounds: ["std::probe::Zulu", "std::probe::Alpha"]
                    .map(|name| TrustedTraitExpr {
                        declaration: id(name),
                        arguments: Box::new([]),
                    })
                    .into(),
            }]),
            parameters: Box::new([TrustedCallableParameter {
                name: Some("self".into()),
                ty: TrustedTypeExpr::Reference {
                    mutability: ReferenceMutability::Shared,
                    referent: Box::new(owner_pattern.clone()),
                },
            }]),
            result: TrustedTypeExpr::GenericParameter(0),
            requirements: Box::new([]),
        },
    ));
    let external = ExternalNameEnvironment::new([module], declarations, []).unwrap();
    let trusted = TrustedSignatureEnvironment::with_inherent_owners(
        signatures,
        [TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Nominal(bag),
            members: Box::new([TrustedInherentMember {
                callable: read,
                name: NameKey::new("read", false).unwrap(),
                owner_pattern,
                owner_parameter_count: 1,
                form: InherentMemberForm::SharedReceiver,
                visibility: eglur_hir::Visibility::Public,
                overload: false,
            }]),
        }],
    )
    .unwrap();
    let effects =
        crate::TrustedEffectEnvironment::new(["std::probe::pick", "std::probe::Bag::read"].map(
            |target| crate::TrustedEffectSummary::new(id(target), crate::EffectSummary::pure()),
        ))
        .unwrap();
    let prefix = "import std::probe::Alpha\nimport std::probe::Zulu\nimport std::probe::pick\n";
    let mut sources = ["= pick(x)", "{ let f: Fn(T) -> T = pick\nyield f(x) }"]
        .map(|body| format!("{prefix}fn inspect[T: Alpha+Zulu](x: T) -> T {body}"))
        .to_vec();
    sources.push(format!("{prefix}import std::probe::Bag\nstruct Local {{}}\nimpl Alpha for Local {{}}\nimpl Zulu for Local {{}}\ntype Fixed = Bag[Local]\nfn inspect(value: &Fixed) -> Local {{ let read = Fixed.read\nyield read(value) }}"));
    for source in sources {
        with_phase_inputs(
            &source,
            &external,
            Some(TrustedDeclarationInputs::new(&trusted, None)),
            |inputs| {
                verify_replay(inputs, &external, &source, Some(&effects));
            },
        );
        with_phase_inputs(
            &source,
            &external,
            Some(TrustedDeclarationInputs::new(&trusted, None)),
            |mut inputs| {
                let mut accounting = accounting();
                let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
                let BodyExecution::Completed(packet) = run(
                    &mut inputs,
                    &external,
                    BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
                        ALLOWANCE
                    }),
                )
                .unwrap() else {
                    panic!("completed trusted body")
                };
                let PhaseProduct::Performed(body) = packet.analysis().product() else {
                    panic!("performed body")
                };
                let pending = body.obligations();
                assert_eq!(pending.len(), 2);
                for (ordinal, (pending, target)) in pending
                    .iter()
                    .zip(["std::probe::Zulu", "std::probe::Alpha"])
                    .enumerate()
                {
                    assert!(matches!(
                        pending.parameter(),
                        GenericParameterId::Trusted { .. }
                    ));
                    assert_eq!(pending.bound_index(), ordinal);
                    assert_eq!(
                        pending.trait_ref().declaration(),
                        &CanonicalTarget::External(id(target))
                    );
                    assert!(pending.destination().is_none());
                }
            },
        );
    }
}

#[test]
fn unperformed_and_prestopped_entries_retain_the_real_execution_mode() {
    struct NoBodies;
    impl CompileSupport for NoBodies {
        fn supports(&self, capability: CompileCapability) -> bool {
            capability != CompileCapability::Typecheck(TypecheckCapability::CheckBodies)
        }
    }
    with_inputs(TEXT, |inputs, external| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let execution = typecheck_bodies_with_work(
            inputs.collection,
            inputs.imports,
            inputs.names,
            Some(external),
            inputs.sources,
            &NoBodies,
            inputs.declarations,
            inputs.trait_environment,
            inputs.diagnostics,
            BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
                panic!("unperformed body admitted work")
            }),
        )
        .unwrap();
        let BodyExecution::Completed(packet) = execution else {
            panic!("unsupported capability is not resource exhaustion")
        };
        assert!(matches!(
            packet.analysis().product(),
            PhaseProduct::Unperformed
        ));
        let BodyProofInputs::Metered(connections) = packet.proof_inputs() else {
            panic!("lost execution mode")
        };
        assert!(connections.bounds().is_empty());
        assert!(connections.dispatches().is_empty());
        connections.validate_accounting(&accounting).unwrap();
        assert_eq!(
            accounting
                .admit(0, ALLOWANCE)
                .unwrap()
                .identity()
                .requirement(),
            0
        );
    });
    with_inputs(TEXT, |mut inputs, external| {
        let mut accounting = accounting();
        let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
        let AnalysisWorkFailure::Incomplete(stop) = accounting.admit(0, 0).unwrap_err() else {
            panic!("natural stop")
        };
        let execution = run(
            &mut inputs,
            external,
            BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| {
                panic!("prestopped body called policy")
            }),
        )
        .unwrap();
        let BodyExecution::Interrupted(stopped) = execution else {
            panic!("stop was lost")
        };
        assert_eq!(stopped.stop(), stop);
        assert_eq!(stopped.prefix().fact_count(), 0);
        let BodyProofInputs::Metered(connections) = stopped.proof_inputs() else {
            panic!("lost interrupted execution mode")
        };
        assert!(connections.bounds().is_empty());
        assert!(connections.dispatches().is_empty());
        assert_eq!(
            connections.validate_accounting(&accounting),
            Err(AnalysisWorkFailure::Incomplete(stop))
        );
    });
}

#[test]
fn completion_refuses_missing_proof_connections() {
    with_inputs(
        "trait Mark {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: T) -> T = id(x)",
        |mut inputs, external| {
            let mut accounting = accounting();
            let (mut sources, mut bodies) = analysis_inputs(&accounting, &inputs);
            let BodyExecution::Completed(packet) = run(
                &mut inputs,
                external,
                BodyAnalysisWork::new(&mut accounting, &mut sources, &mut bodies, &|_| ALLOWANCE),
            )
            .unwrap() else {
                panic!("completed body")
            };
            let (analysis, _) = packet.into_parts();
            assert!(matches!(
                crate::CompletedBodyExecution::new(
                    analysis,
                    BodyProofInputs::new(Some(&accounting))
                ),
                Err(BodyTypingError::ProofInputCardinalityMismatch)
            ));
        },
    );
}

#[test]
fn attachment_storage_is_prepaid_and_foreign_appends_do_not_publish() {
    use crate::record_reservation::observation::{assert_growth_envelope, observe};

    let source =
        "trait Mark {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: T) -> T = id(x)";
    with_inputs(source, |inputs, _| {
        let mut accounting = accounting();
        let (_, mut bodies) = analysis_inputs(&accounting, &inputs);
        let site = first_call(&inputs);
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
            panic!("original definition")
        };
        let bound = bodies
            .bound_ticket(&mut accounting, site, parameter, 0, || ALLOWANCE)
            .unwrap();
        let dispatch_site = MethodDispatchSite::Invocation(site);
        let dispatch = bodies
            .dispatch_ticket(&mut accounting, dispatch_site, || ALLOWANCE)
            .unwrap();
        for dispatch_input in [false, true] {
            let mut custody = BodyProofInputs::new(Some(&accounting));
            let BodyProofInputs::Metered(connections) = &mut custody else {
                panic!("metered custody")
            };
            CHARGES.with_borrow_mut(Vec::clear);
            accounting
                .observe_charges(|charge| CHARGES.with_borrow_mut(|charges| charges.push(charge)));
            let (_, observations) = observe(|| {
                for _ in 0..65 {
                    if dispatch_input {
                        connections
                            .push_dispatch(&mut accounting, dispatch_site, dispatch.clone())
                            .unwrap();
                    } else {
                        connections
                            .push_bound(&mut accounting, site, parameter.id(), 0, bound.clone())
                            .unwrap();
                    }
                }
            });
            let count = if dispatch_input {
                connections.dispatches().len()
            } else {
                connections.bounds().len()
            };
            let header = if dispatch_input {
                size_of::<crate::DispatchProofInput>()
            } else {
                size_of::<crate::BoundProofInput>()
            };
            assert_eq!(observations.len(), count);
            assert!(
                observations
                    .iter()
                    .all(|event| event.record_bytes == header)
            );
            assert_growth_envelope(&observations, count * header);
            let charges = CHARGES.with_borrow(Clone::clone);
            let reserved = charges
                .iter()
                .map(|event| event.work.reserved_bytes)
                .sum::<u64>();
            let transferred = charges
                .iter()
                .map(|event| event.work.transferred_bytes)
                .sum::<u64>();
            assert_eq!(reserved, (4 * count * header) as u64);
            assert_eq!(transferred, (3 * count * header) as u64);
            let requested = observations
                .iter()
                .filter_map(|event| {
                    event
                        .requested_capacity
                        .map(|capacity| capacity * event.record_bytes)
                })
                .sum::<usize>();
            let possible_move = observations
                .iter()
                .filter(|event| event.requested_capacity.is_some())
                .map(|event| event.len * event.record_bytes)
                .sum::<usize>();
            assert!(requested as u64 <= reserved);
            assert!((possible_move + count * header) as u64 <= transferred);
            let mut foreign =
                AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(1, 1, 1).unwrap(), 1)
                    .unwrap();
            assert_eq!(
                connections.push_bound(&mut foreign, site, parameter.id(), 0, bound.clone()),
                Err(AnalysisWorkFailure::Claim(
                    AnalysisRootClaimError::ForeignAttempt
                ))
            );
            let foreign_ticket = foreign.admit(0, ALLOWANCE).unwrap();
            assert_eq!(
                connections.push_bound(&mut accounting, site, parameter.id(), 0, foreign_ticket),
                Err(AnalysisWorkFailure::Claim(
                    AnalysisRootClaimError::ForeignAttempt
                ))
            );
            assert_eq!(
                connections.bounds().len() + connections.dispatches().len(),
                count
            );
            assert!(accounting.incomplete().is_none());
        }
    });
}
