use super::*;

struct Run {
    snapshot: Snapshot,
    charges: Vec<ChargeEvent>,
    evidence_start: usize,
    policy: Vec<BodyAnalysisInput>,
    roots: Vec<AnalysisRootIdentity>,
    stop: Option<AnalysisWorkIncomplete>,
    demands: Vec<CapabilityDemand>,
    diagnostics: Vec<eglur_diagnostics::Diagnostic>,
}

fn run(text: &str, cut: Option<(BodyAnalysisInput, u64)>) -> Run {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    let mut result = None;
    with_fixture(text, &external, None, |mut fixture| {
        let mut meter = accounting();
        CHARGES.with_borrow_mut(Vec::clear);
        meter.observe_charges(|event| CHARGES.with_borrow_mut(|events| events.push(event)));
        let policy = RefCell::new(Vec::new());
        let body = body(&mut fixture, &external, &mut meter, &|input| {
            policy.borrow_mut().push(input);
            cut.filter(|(target, _)| *target == input)
                .map_or(ALLOWANCE, |(_, allowance)| allowance)
        });
        let BodyProofInputs::Metered(inputs) = body.proof_inputs() else {
            panic!("custody missing")
        };
        let roots = inputs
            .bounds()
            .iter()
            .map(|input| input.ticket().identity())
            .chain(
                inputs
                    .dispatches()
                    .iter()
                    .map(|input| input.ticket().identity()),
            )
            .collect::<Vec<_>>();
        let evidence_start = CHARGES.with_borrow(Vec::len);
        let (snapshot, stop, demands) = match evidence(&mut fixture, &body, &mut meter).unwrap() {
            TraitEvidenceExecution::Completed(phase) => {
                assert!(cut.is_none(), "cut evidence completed");
                let PhaseProduct::Performed(product) = phase.product() else {
                    panic!("evidence missing")
                };
                (Snapshot::complete(product), None, phase.demands().to_vec())
            }
            TraitEvidenceExecution::Interrupted(stopped) => {
                assert!(cut.is_some(), "uncut evidence interrupted");
                assert_eq!(meter.incomplete(), Some(stopped.stop()));
                assert!(stopped.internal_failures().is_empty());
                validate_backing(stopped.prefix(), fixture.declarations.signatures());
                (
                    Snapshot::prefix(stopped.prefix()),
                    Some(stopped.stop()),
                    stopped.demands().to_vec(),
                )
            }
        };
        let diagnostics = std::mem::replace(fixture.diagnostics, DiagnosticStore::new())
            .finalize(None)
            .diagnostics()
            .to_vec();
        result = Some(Run {
            snapshot,
            charges: CHARGES.with_borrow(Clone::clone),
            evidence_start,
            policy: policy.into_inner(),
            roots,
            stop,
            demands,
            diagnostics,
        });
    });
    result.unwrap()
}

pub(super) fn validate_backing(prefix: &TraitEvidencePrefix, signatures: &DeclarationSignatures) {
    let dispatch = |dispatch: &CheckedDispatch| {
        if let CheckedDispatch::ImplTemplate { proof, .. } = dispatch {
            assert!(prefix.proof(*proof).is_some());
        }
    };
    for (_, fact) in prefix.invocations.iter() {
        if let SemanticFact::Known(CheckedInvocation::Static {
            dispatch: target,
            evidence_environment,
        }) = fact
        {
            dispatch(target);
            assert!(prefix.environment(*evidence_environment).is_some());
        }
    }
    for (_, fact) in prefix.callable_values.iter() {
        if let SemanticFact::Known(value) = fact {
            dispatch(value.dispatch());
            assert!(prefix.environment(value.evidence_environment()).is_some());
        }
    }
    for environment in prefix.environments() {
        for entry in environment.entries() {
            assert!(prefix.proof(entry.proof()).is_some());
            assert!(signatures.types().kind(entry.subject()).is_ok());
            assert!(
                signatures
                    .types()
                    .substitution(entry.trait_ref().substitution())
                    .is_ok()
            );
        }
    }
    for proof in &prefix.proofs {
        if let EvidenceSource::Implementation { substitution, .. } = proof {
            assert!(signatures.types().substitution(*substitution).is_ok());
        }
    }
}

#[test]
fn every_evidence_charge_stops_on_the_original_root_with_backed_prefix_only() {
    let mut sources = programs();
    sources.push("trait Mark {}\nstruct Plain {}\nfn id[T: Mark](value: T) -> T = value\nfn inspect(value: Plain) -> Plain = id(value)");
    let mut retained_child = false;
    for source in sources {
        let full = run(source, None);
        assert!(full.charges.len() > full.evidence_start);
        let admissions = full
            .charges
            .iter()
            .filter(|event| event.kind == ChargeKind::Admission)
            .collect::<Vec<_>>();
        assert_eq!(admissions.len(), full.policy.len());
        for ordinal in full.evidence_start..full.charges.len() {
            let event = full.charges[ordinal];
            assert_eq!(event.kind, ChargeKind::Work);
            assert!(full.roots.contains(&event.root));
            let input = full.policy[admissions
                .iter()
                .position(|admission| admission.root == event.root)
                .unwrap()];
            let allowance = full.charges[..ordinal]
                .iter()
                .filter(|prior| prior.root == event.root)
                .map(|prior| prior.ticks)
                .sum();
            let stopped = run(source, Some((input, allowance)));
            assert_eq!(stopped.evidence_start, full.evidence_start);
            assert_eq!(stopped.charges, full.charges[..=ordinal]);
            assert_eq!(stopped.stop.unwrap().root(), event.root);
            assert_eq!(
                stopped.stop.unwrap().reason(),
                AnalysisRootStopReason::Exhausted {
                    required_ticks: event.ticks,
                    remaining_ticks: 0
                }
            );
            stopped.snapshot.assert_retained_from(&full.snapshot);
            assert!(
                stopped.snapshot.routes.is_empty(),
                "success routes after a proof stop"
            );
            for diagnostic in &stopped.diagnostics {
                assert!(full.diagnostics.contains(diagnostic));
            }
            for demand in &stopped.demands {
                assert!(full.demands.contains(demand));
            }
            retained_child |= !stopped.snapshot.proofs.is_empty()
                && stopped.snapshot.proofs.len() < full.snapshot.proofs.len();
        }
    }
    assert!(
        retained_child,
        "fixtures never stopped between a completed child and its parent"
    );
}
