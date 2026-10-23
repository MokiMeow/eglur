use std::cell::RefCell;

use super::*;
use crate::analysis_work::{ChargeEvent, ChargeKind};
use crate::{
    AnalysisBodyInputs, AnalysisRootClaimError, AnalysisRootIdentity, AnalysisRootStopReason,
    AnalysisSourceInputs, AnalysisWorkPricing, BodyAnalysisInput, BodyAnalysisWork, BodyExecution,
    ExternalNameEnvironment, SemanticFingerprints, typecheck_bodies_with_work,
};

mod fixture;
mod inputs;
mod sweep;
use fixture::{Fixture, with_fixture};

const ALLOWANCE: u64 = 10_000_000;

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

thread_local! {
    static CHARGES: RefCell<Vec<ChargeEvent>> = const { RefCell::new(Vec::new()) };
    static PROOF_STAGES: RefCell<Option<Vec<ProofStage>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::trait_evidence) enum ProofStage {
    Materialization,
    Implementation,
    ParameterCursor,
}

pub(in crate::trait_evidence) fn observe_proof_work(stage: ProofStage, metered: bool) {
    PROOF_STAGES.with_borrow_mut(|stages| {
        if let Some(stages) = stages {
            assert!(metered, "original budget lost at {stage:?}");
            stages.push(stage);
        }
    });
}

struct RecordingProofStages;

impl Drop for RecordingProofStages {
    fn drop(&mut self) {
        PROOF_STAGES.with_borrow_mut(|stages| *stages = None);
    }
}

fn record_stages<R>(operation: impl FnOnce() -> R) -> (R, Vec<ProofStage>) {
    PROOF_STAGES.with_borrow_mut(|stages| {
        assert!(stages.is_none());
        *stages = Some(Vec::new());
    });
    let recording = RecordingProofStages;
    let result = operation();
    let stages = PROOF_STAGES.with_borrow_mut(|stages| stages.take().unwrap());
    drop(recording);
    (result, stages)
}

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(1, 1, 1).unwrap(), 1).unwrap()
}

fn body(
    fixture: &mut Fixture<'_, '_>,
    external: &ExternalNameEnvironment,
    accounting: &mut AnalysisWorkAccounting,
    policy: &dyn Fn(BodyAnalysisInput) -> u64,
) -> CompletedBodyExecution {
    body_with_support(fixture, external, accounting, policy, &Support)
}

fn body_with_support(
    fixture: &mut Fixture<'_, '_>,
    external: &ExternalNameEnvironment,
    accounting: &mut AnalysisWorkAccounting,
    policy: &dyn Fn(BodyAnalysisInput) -> u64,
    support: &dyn CompileSupport,
) -> CompletedBodyExecution {
    let mut sources =
        AnalysisSourceInputs::new(accounting, fixture.sources.iter().map(|source| (0, source)))
            .unwrap();
    let mut bodies = AnalysisBodyInputs::new(
        accounting,
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
    let BodyExecution::Completed(packet) = typecheck_bodies_with_work(
        fixture.collection,
        fixture.imports,
        fixture.names,
        Some(external),
        fixture.sources,
        support,
        fixture.declarations,
        fixture.trait_environment,
        fixture.diagnostics,
        BodyAnalysisWork::new(accounting, &mut sources, &mut bodies, policy),
    )
    .unwrap() else {
        panic!("body unexpectedly interrupted")
    };
    *packet
}

fn evidence(
    fixture: &mut Fixture<'_, '_>,
    body: &CompletedBodyExecution,
    accounting: &mut AnalysisWorkAccounting,
) -> Result<TraitEvidenceExecution, TraitEvidenceError> {
    resolve_trait_evidence_with_work(
        fixture.collection,
        fixture.sources,
        &Support,
        fixture.declarations.signatures_mut(),
        fixture.trait_environment,
        body,
        fixture.diagnostics,
        accounting,
    )
}

fn programs() -> Vec<&'static str> {
    vec![
        "trait Mark {}\nfn id[T: Mark](value: T) -> T = value\nfn inspect[T: Mark](value: T) -> T = id(value)",
        "trait Alpha {}\ntrait Zulu {}\nimpl Alpha for i64 {}\nimpl Zulu for i64 {}\nfn pair[T: Zulu+Alpha, U: Zulu+Alpha](x: T, y: U) -> Unit = ()\nfn inspect(x: i64) -> Unit { pair(x, x)\npair(x, x) }",
        "trait Mark {}\nstruct Tagged[T: Mark] { field value: T }\nfn inspect[T: Mark](value: T) -> Unit { let tagged = Tagged { value: value } }",
        "trait Rank { fn rank(self: &Self) -> i64 }\nfn inspect[T: Rank](x: &T) -> i64 = x.rank()",
        "trait Rank { fn rank(self: &Self) -> i64 }\nstruct S {}\nimpl Rank for S { fn rank(self: &Self) -> i64 = 1 }\nfn inspect(x: &S) -> i64 = Rank.rank(x)",
        "trait Rank { fn rank(self: &Self) -> i64 }\nstruct S {}\nimpl Rank for S { fn rank(self: &Self) -> i64 = 1 }\nfn inspect(x: &S) -> i64 { let f: Fn(&S) -> i64 = Rank.rank\nyield f(x) }",
        "trait Mark {}\nfn id[T: Mark](value: T) -> T = value\nfn inspect[T: Mark](value: T) -> T { let f: Fn(T) -> T = id\nyield f(value) }",
        "trait Leaf {}\ntrait Left {}\ntrait Right {}\ntrait Root {}\nimpl Leaf for i64 {}\nimpl[T: Leaf] Left for T {}\nimpl[T: Leaf] Right for T {}\nimpl[T: Left+Right] Root for T {}\nfn id[T: Root](value: T) -> T = value\nfn inspect(value: i64) -> i64 = id(value)",
        "trait Leaf {}\ntrait Root[A: Leaf] {}\nimpl Leaf for i64 {}\nimpl[T: Leaf, A: Leaf] Root[A] for T {}\nfn id[T: Root[i64]](value: T) -> T = value\nfn inspect(value: i64) -> i64 = id(value)",
    ]
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    invocations: Vec<(ExpressionKey, String)>,
    callables: Vec<(ExpressionKey, String)>,
    environments: Vec<EvidenceEnvironment>,
    proofs: Vec<EvidenceSource>,
    routes: Vec<((CanonicalTarget, CanonicalTarget), MethodFrameRoute)>,
}

impl Snapshot {
    fn complete(evidence: &TraitEvidence) -> Self {
        Self::capture(
            &evidence.invocations,
            &evidence.callable_values,
            &evidence.environments,
            &evidence.proofs,
            &evidence.method_routes,
        )
    }

    fn prefix(evidence: &TraitEvidencePrefix) -> Self {
        Self::capture(
            &evidence.invocations,
            &evidence.callable_values,
            &evidence.environments,
            &evidence.proofs,
            &evidence.method_routes,
        )
    }

    fn capture(
        invocations: &FactTable<ExpressionKey, CheckedInvocation>,
        callables: &FactTable<ExpressionKey, CheckedCallableValue>,
        environments: &[EvidenceEnvironment],
        proofs: &[EvidenceSource],
        routes: &BTreeMap<(CanonicalTarget, CanonicalTarget), MethodFrameRoute>,
    ) -> Self {
        Self {
            invocations: invocations
                .iter()
                .map(|(key, fact)| (*key, format!("{fact:?}")))
                .collect(),
            callables: callables
                .iter()
                .map(|(key, fact)| (*key, format!("{fact:?}")))
                .collect(),
            environments: environments.to_vec(),
            proofs: proofs.to_vec(),
            routes: routes
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }
    }

    fn assert_retained_from(&self, full: &Self) {
        for item in &self.invocations {
            assert!(full.invocations.contains(item));
        }
        for item in &self.callables {
            assert!(full.callables.contains(item));
        }
        assert!(full.environments.starts_with(&self.environments));
        assert!(full.proofs.starts_with(&self.proofs));
        for item in &self.routes {
            assert!(full.routes.contains(item));
        }
        for environment in &self.environments {
            for entry in environment.entries() {
                assert!(self.proofs.get(entry.proof().index()).is_some());
            }
        }
        for (index, proof) in self.proofs.iter().enumerate() {
            if let EvidenceSource::Implementation {
                implementation_bounds,
                trait_bounds,
                ..
            } = proof
            {
                for child in implementation_bounds.iter().chain(trait_bounds) {
                    assert!(child.index() < index);
                }
            }
        }
    }
}

fn fingerprint(
    fixture: &mut Fixture<'_, '_>,
    body: &BodyTypes,
    evidence: &PhaseProduct<TraitEvidence>,
) -> SemanticFingerprints {
    let PhaseProduct::Performed(checked) = evidence else {
        panic!("evidence missing")
    };
    let effects = crate::analyze_effects(
        fixture.collection,
        fixture.names,
        fixture.sources,
        &Support,
        fixture.declarations.signatures(),
        body,
        checked,
        None,
        fixture.diagnostics,
    )
    .unwrap();
    crate::SemanticFingerprints::compute(
        fixture.collection,
        fixture.imports,
        fixture.names,
        fixture.declarations.signatures(),
        fixture.sources,
        body,
        fixture.trait_environment,
        evidence,
        effects.product(),
    )
    .unwrap()
    .expect("complete evidence fingerprint")
}

#[test]
fn original_proof_custody_preserves_complete_facts_fingerprints_and_replay() {
    let external = ExternalNameEnvironment::new([], [], []).unwrap();
    for text in programs() {
        with_fixture(text, &external, None, |mut fixture| {
            let mut accounting = accounting();
            let body = body(&mut fixture, &external, &mut accounting, &|_| ALLOWANCE);
            let PhaseProduct::Performed(bodies) = body.analysis().product() else {
                panic!("body missing")
            };
            let normal = resolve_trait_evidence(
                fixture.collection,
                fixture.sources,
                &Support,
                fixture.declarations.signatures_mut(),
                fixture.trait_environment,
                bodies,
                fixture.diagnostics,
            )
            .unwrap();
            let PhaseProduct::Performed(normal_evidence) = normal.product() else {
                panic!("evidence missing")
            };
            let expected = Snapshot::complete(normal_evidence);
            let expected_fingerprint = fingerprint(&mut fixture, bodies, normal.product());
            let BodyProofInputs::Metered(custody) = body.proof_inputs() else {
                panic!("custody missing")
            };
            let roots = custody
                .bounds()
                .iter()
                .map(|input| input.ticket().identity())
                .chain(
                    custody
                        .dispatches()
                        .iter()
                        .map(|input| input.ticket().identity()),
                )
                .collect::<Vec<_>>();
            assert!(
                !roots.is_empty(),
                "fixture did not exercise original evidence"
            );
            CHARGES.with_borrow_mut(Vec::clear);
            accounting
                .observe_charges(|event| CHARGES.with_borrow_mut(|events| events.push(event)));
            for _ in 0..2 {
                let (execution, stages) =
                    record_stages(|| evidence(&mut fixture, &body, &mut accounting).unwrap());
                let TraitEvidenceExecution::Completed(phase) = execution else {
                    panic!("unexpected stop: {text}")
                };
                let PhaseProduct::Performed(checked) = phase.product() else {
                    panic!("evidence missing")
                };
                assert_eq!(Snapshot::complete(checked), expected, "{text}");
                if !checked.proofs.is_empty() {
                    assert!(stages.contains(&ProofStage::Materialization));
                }
                if checked
                    .proofs
                    .iter()
                    .any(|proof| matches!(proof, EvidenceSource::Implementation { .. }))
                {
                    assert!(stages.contains(&ProofStage::Implementation));
                    assert!(stages.contains(&ProofStage::ParameterCursor));
                }
                assert_eq!(phase.demands(), normal.demands());
                assert_eq!(
                    fingerprint(&mut fixture, bodies, phase.product()),
                    expected_fingerprint
                );
            }
            CHARGES.with_borrow(|charges| {
                assert!(!charges.is_empty());
                assert!(
                    charges
                        .iter()
                        .all(|event| event.kind == ChargeKind::Work && roots.contains(&event.root))
                );
            });
            assert!(!fixture.diagnostics.has_errors(), "{text}");
        });
    }
}

#[test]
fn trusted_declaration_ordinals_and_tickets_survive_actual_evidence_execution() {
    let (external, trusted) = fixture::trusted_contracts();
    for expression in [
        "= pick(value)",
        "{ let f: Fn(T) -> T = pick\nyield f(value) }",
    ] {
        let source = format!(
            "import std::probe::Alpha\nimport std::probe::Zulu\nimport std::probe::pick\nfn inspect[T: Alpha+Zulu](value: T) -> T {expression}"
        );
        with_fixture(
            &source,
            &external,
            Some(crate::TrustedDeclarationInputs::new(&trusted, None)),
            |mut fixture| {
                let mut meter = accounting();
                let body = body(&mut fixture, &external, &mut meter, &|_| ALLOWANCE);
                let PhaseProduct::Performed(bodies) = body.analysis().product() else {
                    panic!("body missing")
                };
                assert!(bodies.obligations().iter().all(|pending| matches!(
                    pending.parameter(),
                    GenericParameterId::Trusted { .. }
                )));
                let TraitEvidenceExecution::Completed(phase) =
                    evidence(&mut fixture, &body, &mut meter).unwrap()
                else {
                    panic!("unexpected stop")
                };
                let PhaseProduct::Performed(product) = phase.product() else {
                    panic!("evidence missing")
                };
                let entries = product
                    .environments()
                    .flat_map(EvidenceEnvironment::entries)
                    .collect::<Vec<_>>();
                assert_eq!(entries.len(), bodies.obligations().len());
                assert!(!entries.is_empty());
                for (entry, pending) in entries.into_iter().zip(bodies.obligations()) {
                    assert_eq!(entry.parameter(), pending.parameter());
                    assert_eq!(entry.bound_index(), pending.bound_index());
                    assert_eq!(entry.trait_ref(), pending.trait_ref());
                    assert!(entry.destination().is_none());
                    assert!(product.proof(entry.proof()).is_some());
                }
                let normal = resolve_trait_evidence(
                    fixture.collection,
                    fixture.sources,
                    &Support,
                    fixture.declarations.signatures_mut(),
                    fixture.trait_environment,
                    bodies,
                    fixture.diagnostics,
                )
                .unwrap();
                let PhaseProduct::Performed(normal) = normal.product() else {
                    panic!("evidence missing")
                };
                assert_eq!(Snapshot::complete(product), Snapshot::complete(normal));
                assert!(!fixture.diagnostics.has_errors());
            },
        );
    }
}
