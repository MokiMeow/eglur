use super::*;
use crate::{AnalysisWorkIncomplete, CapabilityDemand, PhaseAnalysis};

mod proof_inputs;

pub use proof_inputs::{
    BodyProofInputs, BoundProofInput, DispatchProofInput, MeteredBodyProofInputs,
};

/// Terminal body analysis; interruption is not a source or infrastructure error.
#[must_use]
pub enum BodyExecution {
    Completed(Box<CompletedBodyExecution>),
    Interrupted(Box<InterruptedBody>),
}

/// Completed semantic facts paired with their execution-only proof connections.
pub struct CompletedBodyExecution {
    analysis: PhaseAnalysis<BodyTypes>,
    proof_inputs: BodyProofInputs,
}

impl CompletedBodyExecution {
    pub(super) fn new(
        analysis: PhaseAnalysis<BodyTypes>,
        proof_inputs: BodyProofInputs,
    ) -> Result<Self, BodyTypingError> {
        let (bounds, dispatches) = match analysis.product() {
            crate::PhaseProduct::Performed(bodies) => {
                (bodies.obligations().len(), bodies.method_dispatches().len())
            }
            crate::PhaseProduct::Unperformed => (0, 0),
        };
        proof_inputs.validate_lengths(bounds, dispatches)?;
        Ok(Self {
            analysis,
            proof_inputs,
        })
    }

    #[must_use]
    pub const fn analysis(&self) -> &PhaseAnalysis<BodyTypes> {
        &self.analysis
    }

    #[must_use]
    pub const fn proof_inputs(&self) -> &BodyProofInputs {
        &self.proof_inputs
    }

    #[must_use]
    pub fn into_parts(self) -> (PhaseAnalysis<BodyTypes>, BodyProofInputs) {
        (self.analysis, self.proof_inputs)
    }
}

/// Completed local facts retained without claiming completion of the body phase.
///
/// Referenced type, declaration, trait and diagnostic stores remain caller-owned.
/// This prefix cannot be supplied as a completed body product.
///
/// ```compile_fail
/// use eglur_sema::{BodyTypes, BodyTypingPrefix};
/// fn completed(prefix: BodyTypingPrefix) -> BodyTypes { prefix }
/// ```
pub struct BodyTypingPrefix {
    pub(super) expressions: crate::FactTable<crate::ExpressionKey, TypedExpression>,
    pub(super) locals: LocalTypeFacts,
    pub(super) members: crate::FactTable<crate::ExpressionKey, MemberAccess>,
    pub(super) invocations: crate::FactTable<crate::ExpressionKey, InvocationFact>,
    pub(super) callable_values: crate::FactTable<crate::ExpressionKey, crate::CallableInstanceId>,
    pub(super) reduce_by_commitments: crate::FactTable<crate::ExpressionKey, ReduceByCommitment>,
    pub(super) regions: crate::FactTable<crate::ControlRegionKey, crate::ControlRegion>,
    pub(super) checked_numeric:
        crate::FactTable<crate::ExpressionKey, crate::CheckedNumericOperation>,
    pub(super) ownership_operations:
        crate::FactTable<crate::ExpressionKey, TypedOwnershipOperation>,
    pub(super) bodies: crate::FactTable<crate::DefId, BodyTypeSummary>,
    pub(super) transfers: Vec<crate::ControlTransfer>,
    pub(super) callable_instances: Vec<CallableInstance>,
    pub(super) obligations: Vec<PendingTraitObligation>,
    pub(super) method_dispatches: Vec<PendingMethodDispatch>,
}

impl BodyTypingPrefix {
    pub(super) fn empty() -> Self {
        Self {
            expressions: crate::FactTable::new(),
            locals: LocalTypeFacts::new(),
            members: crate::FactTable::new(),
            invocations: crate::FactTable::new(),
            callable_values: crate::FactTable::new(),
            reduce_by_commitments: crate::FactTable::new(),
            regions: crate::FactTable::new(),
            checked_numeric: crate::FactTable::new(),
            ownership_operations: crate::FactTable::new(),
            bodies: crate::FactTable::new(),
            transfers: Vec::new(),
            callable_instances: Vec::new(),
            obligations: Vec::new(),
            method_dispatches: Vec::new(),
        }
    }

    #[must_use]
    pub fn expression(&self, key: crate::ExpressionKey) -> crate::FactState<'_, TypedExpression> {
        self.expressions.state(&key)
    }

    #[must_use]
    pub fn body(&self, owner: crate::DefId) -> crate::FactState<'_, BodyTypeSummary> {
        self.bodies.state(&owner)
    }

    #[must_use]
    pub fn callable_instance(&self, id: crate::CallableInstanceId) -> Option<&CallableInstance> {
        self.callable_instances.get(id.index())
    }

    #[must_use]
    pub fn transfers(&self) -> &[crate::ControlTransfer] {
        &self.transfers
    }

    #[must_use]
    pub fn obligations(&self) -> &[PendingTraitObligation] {
        &self.obligations
    }

    #[must_use]
    pub fn method_dispatches(&self) -> &[PendingMethodDispatch] {
        &self.method_dispatches
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.expressions.iter().len()
            + self.locals.len()
            + self.members.iter().len()
            + self.invocations.iter().len()
            + self.callable_values.iter().len()
            + self.reduce_by_commitments.iter().len()
            + self.regions.iter().len()
            + self.checked_numeric.iter().len()
            + self.ownership_operations.iter().len()
            + self.bodies.iter().len()
            + self.transfers.len()
            + self.callable_instances.len()
            + self.obligations.len()
            + self.method_dispatches.len()
    }
}

/// A sticky accounting stop and its retained, non-resumable local evidence.
pub struct InterruptedBody {
    pub(super) stop: AnalysisWorkIncomplete,
    pub(super) prefix: BodyTypingPrefix,
    pub(super) demands: Vec<CapabilityDemand>,
    pub(super) internal_failures: Vec<BodyTypingError>,
    pub(super) proof_inputs: BodyProofInputs,
}

impl InterruptedBody {
    #[must_use]
    pub const fn proof_inputs(&self) -> &BodyProofInputs {
        &self.proof_inputs
    }

    #[must_use]
    pub const fn stop(&self) -> AnalysisWorkIncomplete {
        self.stop
    }

    #[must_use]
    pub const fn prefix(&self) -> &BodyTypingPrefix {
        &self.prefix
    }

    #[must_use]
    pub fn demands(&self) -> &[CapabilityDemand] {
        &self.demands
    }

    #[must_use]
    pub fn internal_failures(&self) -> &[BodyTypingError] {
        &self.internal_failures
    }
}
