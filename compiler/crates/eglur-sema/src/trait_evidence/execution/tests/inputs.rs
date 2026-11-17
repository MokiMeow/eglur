use super::*;

#[test]
fn unperformed_body_input_remains_unperformed_without_admitting_work() {
    struct NoBodies;
    impl CompileSupport for NoBodies {
        fn supports(&self, capability: CompileCapability) -> bool {
            capability != CompileCapability::Typecheck(TypecheckCapability::CheckBodies)
        }
    }
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_fixture(programs()[0], &external, None, |mut fixture| {
        let mut meter = accounting();
        let packet = body_with_support(
            &mut fixture,
            &external,
            &mut meter,
            &|_| panic!("unperformed body admitted work"),
            &NoBodies,
        );
        let TraitEvidenceExecution::Completed(phase) =
            evidence(&mut fixture, &packet, &mut meter).unwrap()
        else {
            panic!("unperformed body became interrupted")
        };
        assert!(matches!(phase.product(), PhaseProduct::Unperformed));
        assert!(phase.demands().is_empty());
        assert_eq!(
            meter.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );
    });
}

#[test]
fn foreign_attempts_are_rejected_even_when_no_proof_was_recorded() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    for source in ["fn inspect() -> Unit = ()", programs()[0]] {
        with_fixture(source, &external, None, |mut fixture| {
            let mut meter = accounting();
            let body = body(&mut fixture, &external, &mut meter, &|_| ALLOWANCE);
            let mut foreign = accounting();
            assert!(matches!(
                evidence(&mut fixture, &body, &mut foreign),
                Err(TraitEvidenceError::AnalysisRootClaim(
                    AnalysisRootClaimError::ForeignAttempt
                ))
            ));
            assert!(!fixture.diagnostics.has_errors());
            assert!(meter.incomplete().is_none());
        });
    }
}

#[test]
fn missing_or_wrong_original_slots_are_internal_failures() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let text = "trait Mark {}\nfn id[T: Mark](x: T) -> T = x\nfn inspect[T: Mark](x: T) -> T { let first = id(x)\nyield id(x) }";
    with_fixture(text, &external, None, |mut fixture| {
        let mut meter = accounting();
        let body = body(&mut fixture, &external, &mut meter, &|_| ALLOWANCE);
        let PhaseProduct::Performed(bodies) = body.analysis().product() else {
            panic!("body missing")
        };
        let BodyProofInputs::Metered(inputs) = body.proof_inputs() else {
            panic!("custody missing")
        };
        let mut work = EvidenceInputs::Metered {
            accounting: &mut meter,
            inputs,
        };
        let first = &bodies.obligations()[0];
        assert!(matches!(
            work.bound(
                inputs.bounds().len(),
                first,
                fixture.declarations.signatures()
            ),
            Err(EvidenceFlow::Internal(
                TraitEvidenceError::InvalidProofCustody("missing original bound")
            ))
        ));
        let wrong = bodies
            .obligations()
            .iter()
            .find(|pending| pending.site() != first.site())
            .unwrap();
        assert!(matches!(
            work.bound(0, wrong, fixture.declarations.signatures()),
            Err(EvidenceFlow::Internal(
                TraitEvidenceError::InvalidProofCustody("original bound identity mismatch")
            ))
        ));
        assert!(work.incomplete().is_none());
        assert!(!fixture.diagnostics.has_errors());
    });
    let text = "trait Rank { fn rank(self: &Self) -> i64 }\nfn inspect[T: Rank](x: &T) -> i64 { let first = x.rank()\nyield Rank.rank(x) }";
    with_fixture(text, &external, None, |mut fixture| {
        let mut meter = accounting();
        let body = body(&mut fixture, &external, &mut meter, &|_| ALLOWANCE);
        let PhaseProduct::Performed(bodies) = body.analysis().product() else {
            panic!("body missing")
        };
        let BodyProofInputs::Metered(inputs) = body.proof_inputs() else {
            panic!("custody missing")
        };
        let mut work = EvidenceInputs::Metered {
            accounting: &mut meter,
            inputs,
        };
        assert!(matches!(
            work.dispatch(inputs.dispatches().len(), &bodies.method_dispatches()[0]),
            Err(EvidenceFlow::Internal(
                TraitEvidenceError::InvalidProofCustody("missing original dispatch")
            ))
        ));
        assert!(matches!(
            work.dispatch(0, &bodies.method_dispatches()[1]),
            Err(EvidenceFlow::Internal(
                TraitEvidenceError::InvalidProofCustody("original dispatch identity mismatch")
            ))
        ));
    });
}

#[test]
fn prestopped_and_unsupported_entries_do_not_execute_evidence() {
    struct NoEvidence;
    impl CompileSupport for NoEvidence {
        fn supports(&self, capability: CompileCapability) -> bool {
            capability != CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence)
        }
    }
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_fixture(programs()[0], &external, None, |mut fixture| {
        let mut meter = accounting();
        let body = body(&mut fixture, &external, &mut meter, &|_| ALLOWANCE);
        CHARGES.with_borrow_mut(Vec::clear);
        meter.observe_charges(|event| CHARGES.with_borrow_mut(|events| events.push(event)));
        let execution = resolve_trait_evidence_with_work(
            fixture.collection,
            fixture.sources,
            &NoEvidence,
            fixture.declarations.signatures_mut(),
            fixture.trait_environment,
            &body,
            fixture.diagnostics,
            &mut meter,
        )
        .unwrap();
        let TraitEvidenceExecution::Completed(phase) = execution else {
            panic!("capability absence is not resource interruption")
        };
        assert!(matches!(phase.product(), PhaseProduct::Unperformed));
        assert!(!phase.demands().is_empty());
        CHARGES.with_borrow(|events| assert!(events.is_empty()));
        let AnalysisWorkFailure::Incomplete(stop) = meter.admit(0, 0).unwrap_err() else {
            panic!("natural stop")
        };
        let TraitEvidenceExecution::Interrupted(stopped) =
            evidence(&mut fixture, &body, &mut meter).unwrap()
        else {
            panic!("prestopped analysis completed")
        };
        assert_eq!(stopped.stop(), stop);
        assert_eq!(stopped.prefix().fact_count(), 0);
        assert!(stopped.demands().is_empty());
        assert_eq!(
            stopped.validate_accounting(&meter),
            Err(AnalysisWorkFailure::Incomplete(stop))
        );
        let foreign = accounting();
        assert_eq!(
            stopped.validate_accounting(&foreign),
            Err(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::ForeignAttempt
            ))
        );
    });
}

#[test]
fn sticky_stop_precedes_internal_fault_and_missing_stop_is_internal() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    with_fixture(programs()[0], &external, None, |mut fixture| {
        let mut meter = accounting();
        let body = body(&mut fixture, &external, &mut meter, &|_| ALLOWANCE);
        let BodyProofInputs::Metered(inputs) = body.proof_inputs() else {
            panic!("custody missing")
        };
        let AnalysisWorkFailure::Incomplete(stop) = meter.admit(0, 0).unwrap_err() else {
            panic!("natural stop")
        };
        let fault = TraitEvidenceError::InvalidCheckedProof("supplemental failure");
        let outcome = complete_execution(
            EvidenceState::new(),
            DemandRecorder::new(&Support),
            EvidenceInputs::Metered {
                accounting: &mut meter,
                inputs,
            },
            Err(EvidenceFlow::Internal(fault.clone())),
        )
        .unwrap();
        let TraitEvidenceExecution::Interrupted(stopped) = outcome else {
            panic!("lost stop")
        };
        assert_eq!(stopped.stop(), stop);
        assert_eq!(stopped.internal_failures(), &[fault]);
        assert!(matches!(
            complete_execution(
                EvidenceState::new(),
                DemandRecorder::new(&Support),
                EvidenceInputs::Unmetered,
                Err(EvidenceFlow::Interrupted)
            ),
            Err(TraitEvidenceError::MissingAnalysisStop)
        ));
    });
}
