use std::sync::Arc;

use super::work::EvidenceInputs;
use super::*;
use crate::{
    AnalysisWorkAccounting, AnalysisWorkFailure, AnalysisWorkIncomplete, BodyProofInputs,
    CapabilityDemand, CompletedBodyExecution,
};

/// Evidence completion and resource interruption are distinct from internal failure.
#[must_use]
pub enum TraitEvidenceExecution {
    Completed(Box<PhaseAnalysis<TraitEvidence>>),
    Interrupted(Box<InterruptedTraitEvidence>),
}

/// Completed local evidence with its backing stores, not a completed evidence phase.
pub struct TraitEvidencePrefix {
    invocations: FactTable<ExpressionKey, CheckedInvocation>,
    callable_values: FactTable<ExpressionKey, CheckedCallableValue>,
    environments: Vec<EvidenceEnvironment>,
    proofs: Vec<EvidenceSource>,
    method_routes: BTreeMap<(CanonicalTarget, CanonicalTarget), MethodFrameRoute>,
}

impl TraitEvidencePrefix {
    #[must_use]
    pub fn invocation(&self, key: ExpressionKey) -> FactState<'_, CheckedInvocation> {
        self.invocations.state(&key)
    }

    #[must_use]
    pub fn callable_value(&self, key: ExpressionKey) -> FactState<'_, CheckedCallableValue> {
        self.callable_values.state(&key)
    }

    #[must_use]
    pub fn environment(&self, id: EvidenceEnvironmentId) -> Option<&EvidenceEnvironment> {
        self.environments.get(id.index())
    }

    #[must_use]
    pub fn proof(&self, id: EvidenceProofId) -> Option<&EvidenceSource> {
        self.proofs.get(id.index())
    }

    #[must_use]
    pub fn method_route(
        &self,
        implementation: &CanonicalTarget,
        method: &CanonicalTarget,
    ) -> Option<&MethodFrameRoute> {
        self.method_routes
            .get(&(implementation.clone(), method.clone()))
    }

    pub fn invocations(
        &self,
    ) -> impl ExactSizeIterator<Item = (ExpressionKey, FactState<'_, CheckedInvocation>)> {
        self.invocations
            .iter()
            .map(|(key, _)| (*key, self.invocations.state(key)))
    }

    pub fn environments(&self) -> impl ExactSizeIterator<Item = &EvidenceEnvironment> {
        self.environments.iter()
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.invocations.iter().len()
            + self.callable_values.iter().len()
            + self.proofs.len()
            + self.method_routes.len()
            + self
                .environments
                .iter()
                .map(|environment| 1 + environment.entries.len())
                .sum::<usize>()
    }
}

/// A non-resumable interruption; semantic declaration/type/trait stores remain caller-owned.
pub struct InterruptedTraitEvidence {
    stop: AnalysisWorkIncomplete,
    attempt: Arc<()>,
    prefix: TraitEvidencePrefix,
    demands: Vec<CapabilityDemand>,
    internal_failures: Vec<TraitEvidenceError>,
}

impl InterruptedTraitEvidence {
    #[must_use]
    pub const fn stop(&self) -> AnalysisWorkIncomplete {
        self.stop
    }

    #[must_use]
    pub const fn prefix(&self) -> &TraitEvidencePrefix {
        &self.prefix
    }

    #[must_use]
    pub fn demands(&self) -> &[CapabilityDemand] {
        &self.demands
    }

    #[must_use]
    pub fn internal_failures(&self) -> &[TraitEvidenceError] {
        &self.internal_failures
    }

    pub fn validate_accounting(
        &self,
        accounting: &AnalysisWorkAccounting,
    ) -> Result<(), AnalysisWorkFailure> {
        accounting.validate_attempt_binding(&self.attempt)
    }
}

#[derive(Debug)]
pub(super) enum EvidenceFlow {
    Internal(TraitEvidenceError),
    Interrupted,
}

impl From<TraitEvidenceError> for EvidenceFlow {
    fn from(error: TraitEvidenceError) -> Self {
        Self::Internal(error)
    }
}

impl From<AnalysisWorkFailure> for EvidenceFlow {
    fn from(error: AnalysisWorkFailure) -> Self {
        match error {
            AnalysisWorkFailure::Claim(error) => {
                Self::Internal(TraitEvidenceError::AnalysisRootClaim(error))
            }
            AnalysisWorkFailure::Incomplete(_) => Self::Interrupted,
        }
    }
}

macro_rules! internal_conversion {
    ($($ty:ty),* $(,)?) => {
        $(impl From<$ty> for EvidenceFlow {
            fn from(error: $ty) -> Self {
                TraitEvidenceError::from(error).into()
            }
        })*
    };
}

internal_conversion!(IdentityOverflow, DuplicateFact, TraitEnvironmentError);

pub(super) struct EvidenceState {
    pub(super) invocations: FactTable<ExpressionKey, CheckedInvocation>,
    pub(super) callable_values: FactTable<ExpressionKey, CheckedCallableValue>,
    pub(super) environments: Vec<EvidenceEnvironment>,
    pub(super) environment_ids: BTreeMap<Box<[EvidenceEntry]>, EvidenceEnvironmentId>,
    pub(super) proofs: ProofArena,
    pub(super) method_routes: BTreeMap<(CanonicalTarget, CanonicalTarget), MethodFrameRoute>,
}

impl EvidenceState {
    fn new() -> Self {
        Self {
            invocations: FactTable::new(),
            callable_values: FactTable::new(),
            environments: Vec::new(),
            environment_ids: BTreeMap::new(),
            proofs: ProofArena::default(),
            method_routes: BTreeMap::new(),
        }
    }

    fn into_prefix(self) -> TraitEvidencePrefix {
        TraitEvidencePrefix {
            invocations: self.invocations,
            callable_values: self.callable_values,
            environments: self.environments,
            proofs: self.proofs.into_completed_nodes(),
            method_routes: self.method_routes,
        }
    }

    fn finish(self) -> TraitEvidence {
        TraitEvidence {
            invocations: self.invocations,
            callable_values: self.callable_values,
            environments: self.environments.into_boxed_slice(),
            proofs: self.proofs.into_nodes(),
            method_routes: self.method_routes,
        }
    }
}

pub(super) enum EvidenceTraversal {
    Performed,
    Unperformed,
}

/// Resolves a completed body's proof requirements under their original budgets.
/// The caller supplies and retains the body's declaration, type and trait backing.
#[allow(
    clippy::too_many_arguments,
    reason = "evidence execution retains independent semantic and accounting owners"
)]
pub fn resolve_trait_evidence_with_work(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    signatures: &mut DeclarationSignatures,
    trait_environment: &mut TraitEnvironment,
    body: &CompletedBodyExecution,
    diagnostics: &mut DiagnosticStore,
    accounting: &mut AnalysisWorkAccounting,
) -> Result<TraitEvidenceExecution, TraitEvidenceError> {
    let BodyProofInputs::Metered(inputs) = body.proof_inputs() else {
        return Err(TraitEvidenceError::InvalidProofCustody(
            "unmetered body packet",
        ));
    };
    let body_types = match body.analysis().product() {
        PhaseProduct::Performed(body) => Some(body),
        PhaseProduct::Unperformed => None,
    };
    execute(
        collection,
        sources,
        support,
        signatures,
        trait_environment,
        body_types,
        diagnostics,
        EvidenceInputs::Metered { accounting, inputs },
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "both execution modes preserve the independently owned phase inputs"
)]
pub(super) fn execute(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    signatures: &mut DeclarationSignatures,
    trait_environment: &mut TraitEnvironment,
    body_types: Option<&BodyTypes>,
    diagnostics: &mut DiagnosticStore,
    mut work: EvidenceInputs<'_>,
) -> Result<TraitEvidenceExecution, TraitEvidenceError> {
    let validation = work.validate();
    if let Err(EvidenceFlow::Internal(error)) = validation {
        return Err(error);
    }
    let mut state = EvidenceState::new();
    let mut recorder = DemandRecorder::new(support);
    let traversal = validation.and_then(|()| match body_types {
        Some(body_types) => super::traverse_evidence(
            collection,
            sources,
            signatures,
            trait_environment,
            body_types,
            diagnostics,
            &mut recorder,
            &mut state,
            &mut work,
        ),
        None => Ok(EvidenceTraversal::Unperformed),
    });
    complete_execution(state, recorder, work, traversal)
}

fn complete_execution(
    state: EvidenceState,
    recorder: DemandRecorder<'_>,
    work: EvidenceInputs<'_>,
    traversal: Result<EvidenceTraversal, EvidenceFlow>,
) -> Result<TraitEvidenceExecution, TraitEvidenceError> {
    if let Some(stop) = work.incomplete() {
        let internal_failures = match traversal {
            Err(EvidenceFlow::Internal(error)) => vec![error],
            _ => Vec::new(),
        };
        return Ok(TraitEvidenceExecution::Interrupted(Box::new(
            InterruptedTraitEvidence {
                stop,
                attempt: work.attempt_binding().expect("stop requires accounting"),
                prefix: state.into_prefix(),
                demands: recorder.into_demands(),
                internal_failures,
            },
        )));
    }
    let product = match traversal {
        Ok(EvidenceTraversal::Performed) => PhaseProduct::Performed(state.finish()),
        Ok(EvidenceTraversal::Unperformed) => PhaseProduct::Unperformed,
        Err(EvidenceFlow::Internal(error)) => return Err(error),
        Err(EvidenceFlow::Interrupted) => return Err(TraitEvidenceError::MissingAnalysisStop),
    };
    Ok(TraitEvidenceExecution::Completed(Box::new(
        recorder.finish(product),
    )))
}

#[cfg(test)]
pub(super) mod tests;
