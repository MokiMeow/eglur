use super::execution::EvidenceFlow;
use super::*;
use crate::{
    AnalysisRootScope, AnalysisWork, AnalysisWorkAccounting, AnalysisWorkIncomplete,
    MeteredBodyProofInputs,
};

pub(super) enum EvidenceInputs<'a> {
    Unmetered,
    Metered {
        accounting: &'a mut AnalysisWorkAccounting,
        inputs: &'a MeteredBodyProofInputs,
    },
}

impl EvidenceInputs<'_> {
    pub(super) fn validate(&self) -> Result<(), EvidenceFlow> {
        match self {
            Self::Unmetered => Ok(()),
            Self::Metered { accounting, inputs } => {
                inputs.validate_accounting(accounting).map_err(Into::into)
            }
        }
    }

    pub(super) fn incomplete(&self) -> Option<AnalysisWorkIncomplete> {
        match self {
            Self::Unmetered => None,
            Self::Metered { accounting, .. } => accounting.incomplete(),
        }
    }

    pub(super) fn attempt_binding(&self) -> Option<std::sync::Arc<()>> {
        match self {
            Self::Unmetered => None,
            Self::Metered { accounting, .. } => Some(accounting.attempt_binding()),
        }
    }

    pub(super) fn bound(
        &mut self,
        index: usize,
        pending: &PendingTraitObligation,
        signatures: &DeclarationSignatures,
    ) -> Result<ProofWork<'_>, EvidenceFlow> {
        match self {
            Self::Unmetered => Ok(ProofWork::Unmetered),
            Self::Metered { accounting, inputs } => {
                let connection =
                    inputs
                        .bounds()
                        .get(index)
                        .ok_or(TraitEvidenceError::InvalidProofCustody(
                            "missing original bound",
                        ))?;
                let mut work = ProofWork::Metered(accounting.claim(connection.ticket())?);
                work.checkpoint()?;
                let FactState::Known(parameter) =
                    signatures.generic_parameter_record(pending.parameter())
                else {
                    return Err(TraitEvidenceError::InvalidProofCustody(
                        "unavailable original bound declaration",
                    )
                    .into());
                };
                if connection.site() != pending.site()
                    || connection.parameter() != parameter.id()
                    || connection.bound_index() != pending.bound_index()
                {
                    return Err(TraitEvidenceError::InvalidProofCustody(
                        "original bound identity mismatch",
                    )
                    .into());
                }
                Ok(work)
            }
        }
    }

    pub(super) fn dispatch(
        &mut self,
        index: usize,
        pending: &PendingMethodDispatch,
    ) -> Result<ProofWork<'_>, EvidenceFlow> {
        match self {
            Self::Unmetered => Ok(ProofWork::Unmetered),
            Self::Metered { accounting, inputs } => {
                let connection = inputs.dispatches().get(index).ok_or(
                    TraitEvidenceError::InvalidProofCustody("missing original dispatch"),
                )?;
                let mut work = ProofWork::Metered(accounting.claim(connection.ticket())?);
                work.checkpoint()?;
                if connection.site() != pending.site() {
                    return Err(TraitEvidenceError::InvalidProofCustody(
                        "original dispatch identity mismatch",
                    )
                    .into());
                }
                Ok(work)
            }
        }
    }
}

pub(super) enum ProofWork<'a> {
    Unmetered,
    Metered(AnalysisRootScope<'a>),
}

impl ProofWork<'_> {
    #[cfg(test)]
    pub(super) fn observe(&self, stage: super::execution::tests::ProofStage) {
        super::execution::tests::observe_proof_work(stage, matches!(self, Self::Metered(_)));
    }

    pub(super) fn checkpoint(&mut self) -> Result<(), EvidenceFlow> {
        if let Self::Metered(scope) = self {
            // This counts custody/traversal only, not the operation's payload cost.
            scope.precharge(AnalysisWork {
                steps: 1,
                inspected_bytes: 0,
                transferred_bytes: 0,
                reserved_bytes: 0,
            })?;
        }
        Ok(())
    }
}
