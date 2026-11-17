use super::*;
use crate::{
    AnalysisCallInputFailure, AnalysisCallRegistryFailure, AnalysisSourceInputFailure,
    AnalysisWorkFailure,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum BodyFlow {
    Internal(BodyTypingError),
    Interrupted,
}

pub(super) struct CompletedBodyTraversal(());

impl From<BodyTypingError> for BodyFlow {
    fn from(error: BodyTypingError) -> Self {
        Self::Internal(error)
    }
}

impl From<AnalysisWorkFailure> for BodyFlow {
    fn from(failure: AnalysisWorkFailure) -> Self {
        match failure {
            AnalysisWorkFailure::Claim(error) => {
                Self::Internal(BodyTypingError::AnalysisRootClaim(error))
            }
            AnalysisWorkFailure::Incomplete(_) => Self::Interrupted,
        }
    }
}

impl From<AnalysisSourceInputFailure> for BodyFlow {
    fn from(failure: AnalysisSourceInputFailure) -> Self {
        match failure {
            AnalysisSourceInputFailure::Work(work) => work.into(),
            error => Self::Internal(BodyTypingError::SourceInputs(error)),
        }
    }
}

impl From<AnalysisCallRegistryFailure> for BodyFlow {
    fn from(failure: AnalysisCallRegistryFailure) -> Self {
        match failure {
            AnalysisCallRegistryFailure::Input(AnalysisCallInputFailure::Work(work)) => work.into(),
            error => Self::Internal(BodyTypingError::CallInputs(error)),
        }
    }
}

impl From<crate::AnalysisBodyInputFailure> for BodyFlow {
    fn from(failure: crate::AnalysisBodyInputFailure) -> Self {
        match failure {
            crate::AnalysisBodyInputFailure::Call(AnalysisCallRegistryFailure::Input(
                AnalysisCallInputFailure::Work(work),
            ))
            | crate::AnalysisBodyInputFailure::Proof(crate::AnalysisProofRegistryFailure::Input(
                crate::AnalysisProofInputFailure::Work(work),
            )) => work.into(),
            error => Self::Internal(BodyTypingError::ProofInputs(error)),
        }
    }
}

macro_rules! internal_conversion {
    ($($ty:ty),* $(,)?) => {
        $(impl From<$ty> for BodyFlow {
            fn from(error: $ty) -> Self {
                BodyTypingError::from(error).into()
            }
        })*
    };
}

internal_conversion!(
    crate::TypeStoreError,
    crate::type_rendering::DiagnosticTypeRenderingError,
    crate::TraitEnvironmentError,
    crate::InhabitanceError,
    crate::DuplicateFact,
    crate::IdentityOverflow,
    crate::diagnostic::SemanticDiagnosticError,
    eglur_diagnostics::DiagnosticStoreError,
);

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn ensure_analysis_active(&self) -> Result<(), BodyFlow> {
        if self
            .analysis_work
            .as_ref()
            .is_some_and(|work| work.accounting().incomplete().is_some())
        {
            return Err(BodyFlow::Interrupted);
        }
        Ok(())
    }

    pub(super) fn execute(mut self) -> Result<BodyExecution, BodyTypingError> {
        let traversal = self.check_all();
        self.complete_execution(traversal)
    }

    fn complete_execution(
        mut self,
        traversal: Result<(), BodyFlow>,
    ) -> Result<BodyExecution, BodyTypingError> {
        #[cfg(test)]
        tests::sweep_support::terminal(&self);
        if let Some(stop) = self
            .analysis_work
            .as_ref()
            .and_then(|work| work.accounting().incomplete())
        {
            if let Err(BodyFlow::Internal(error)) = traversal {
                self.internal_failures.push(error);
            }
            let prefix = BodyTypingPrefix {
                expressions: self.expressions,
                locals: self.locals,
                members: self.members,
                invocations: self.invocations,
                callable_values: self.callable_values,
                reduce_by_commitments: self.reduce_by_commitments,
                regions: self.regions,
                checked_numeric: self.checked_numeric,
                ownership_operations: self.ownership_operations,
                bodies: self.bodies,
                transfers: self.transfers,
                callable_instances: self.callable_instances,
                obligations: self.obligations,
                method_dispatches: self.method_dispatches,
            };
            return Ok(BodyExecution::Interrupted(Box::new(InterruptedBody {
                stop,
                prefix,
                demands: self.recorder.into_demands(),
                internal_failures: self.internal_failures,
                proof_inputs: self.proof_inputs,
            })));
        }
        match traversal {
            Ok(()) => Ok(BodyExecution::Completed(Box::new(
                self.finish(CompletedBodyTraversal(()))?,
            ))),
            Err(BodyFlow::Internal(error)) => Err(error),
            Err(BodyFlow::Interrupted) => Err(BodyTypingError::MissingAnalysisStop),
        }
    }

    pub(super) fn retain_cleanup_failure(&mut self, error: BodyTypingError) {
        self.internal_failures.push(error);
    }
}

#[cfg(test)]
pub(super) mod tests;
