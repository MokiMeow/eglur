use super::*;
use crate::{
    AnalysisBodyInputs, AnalysisCallRegistryFailure, AnalysisRootTicket,
    AnalysisSourceInputFailure, AnalysisSourceInputs, AnalysisWorkAccounting,
};

#[derive(Clone)]
pub(super) struct CallTypingRequest {
    pub(super) expected: TypeRequest,
    pub(super) selection: Option<AnalysisRootTicket>,
    // Only source-written argument slots use this origin; generated slots have none.
    pub(super) original: Option<ExpressionKey>,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn expectation_work(
        &mut self,
        file: FileId,
        custody: ExpectationCustody,
        inherited: Option<&AnalysisRootTicket>,
    ) -> Result<Option<AnalysisRootTicket>, BodyFlow> {
        let Some(work) = &mut self.analysis_work else {
            return Ok(None);
        };
        if let Some(ticket) = inherited {
            return Ok(Some(work.inherited_ticket(file, ticket)?));
        }
        Ok(match custody {
            ExpectationCustody::Caller => None,
            ExpectationCustody::Written(occurrence) => Some(work.written_ticket(occurrence)?),
            ExpectationCustody::CallArgument { call, index } => {
                Some(work.argument_ticket(call, index)?)
            }
            ExpectationCustody::Selection(call) => Some(
                work.bodies
                    .existing_selection_ticket(work.accounting, call)?,
            ),
        })
    }

    pub(super) fn attach_request_work(
        &mut self,
        file: FileId,
        mut request: TypeRequest,
    ) -> Result<TypeRequest, BodyFlow> {
        request.work = self.expectation_work(file, request.custody(), request.work.as_ref())?;
        Ok(request)
    }

    pub(super) fn prepare_call_work(
        &mut self,
        site: ExpressionKey,
        body: &Body,
        expected: TypeRequest,
    ) -> Result<CallTypingRequest, BodyFlow> {
        let original = crate::analysis_work::original_call(body, site)
            .ok()
            .map(|_| site);
        let selection = if original.is_some() {
            match &mut self.analysis_work {
                Some(work) => Some(work.selection_ticket(site)?),
                None => None,
            }
        } else {
            expected.work.clone()
        };
        Ok(CallTypingRequest {
            expected,
            selection,
            original,
        })
    }
}

/// Original source inputs supplied to an explicit allowance policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyAnalysisInput {
    WrittenType(TypeOccurrenceId),
    CallSelection(ExpressionKey),
    CallArgument {
        call: ExpressionKey,
        index: usize,
    },
    ParameterBound {
        site: ExpressionKey,
        parameter: crate::GenericParameterRecordId,
        index: usize,
    },
    MethodDispatch(crate::MethodDispatchSite),
}

/// Borrowed execution custody; the caller retains both input sets across phases.
///
/// Policy must be deterministic and input-bounded. Only first original admission
/// invokes it. This loan supplies no default allowance and does not activate
/// production limits or certify complete semantic-operation metering.
pub struct BodyAnalysisWork<'work, 'hir> {
    pub(super) accounting: &'work mut AnalysisWorkAccounting,
    sources: &'work mut AnalysisSourceInputs<'hir>,
    bodies: &'work mut AnalysisBodyInputs<'hir>,
    allowance: &'work dyn Fn(BodyAnalysisInput) -> u64,
}

impl<'work, 'hir> BodyAnalysisWork<'work, 'hir> {
    pub fn new(
        accounting: &'work mut AnalysisWorkAccounting,
        sources: &'work mut AnalysisSourceInputs<'hir>,
        bodies: &'work mut AnalysisBodyInputs<'hir>,
        allowance: &'work dyn Fn(BodyAnalysisInput) -> u64,
    ) -> Self {
        Self {
            accounting,
            sources,
            bodies,
            allowance,
        }
    }

    pub fn accounting(&self) -> &AnalysisWorkAccounting {
        self.accounting
    }

    pub fn written_ticket(
        &mut self,
        occurrence: TypeOccurrenceId,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        self.sources
            .written_ticket_with(self.accounting, occurrence, || {
                (self.allowance)(BodyAnalysisInput::WrittenType(occurrence))
            })
    }

    pub fn selection_ticket(
        &mut self,
        call: ExpressionKey,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.bodies.selection_ticket(self.accounting, call, || {
            (self.allowance)(BodyAnalysisInput::CallSelection(call))
        })
    }

    pub fn argument_ticket(
        &mut self,
        call: ExpressionKey,
        index: usize,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.bodies
            .argument_ticket(self.accounting, call, index, || {
                (self.allowance)(BodyAnalysisInput::CallArgument { call, index })
            })
    }

    pub fn inherited_ticket(
        &mut self,
        file: FileId,
        upstream: &AnalysisRootTicket,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        self.sources
            .inherited_ticket(self.accounting, file, upstream)
    }

    pub(super) fn bound_ticket(
        &mut self,
        site: ExpressionKey,
        parameter: &crate::GenericParameterRecord,
        index: usize,
    ) -> Result<AnalysisRootTicket, crate::AnalysisBodyInputFailure> {
        self.bodies
            .bound_ticket(self.accounting, site, parameter, index, || {
                (self.allowance)(BodyAnalysisInput::ParameterBound {
                    site,
                    parameter: parameter.id(),
                    index,
                })
            })
    }

    fn dispatch_ticket(
        &mut self,
        site: crate::MethodDispatchSite,
    ) -> Result<AnalysisRootTicket, crate::AnalysisBodyInputFailure> {
        self.bodies.dispatch_ticket(self.accounting, site, || {
            (self.allowance)(BodyAnalysisInput::MethodDispatch(site))
        })
    }

    fn existing_dispatch_ticket(
        &mut self,
        site: crate::MethodDispatchSite,
    ) -> Result<AnalysisRootTicket, crate::AnalysisBodyInputFailure> {
        self.bodies.existing_dispatch_ticket(self.accounting, site)
    }

    pub(super) fn validate_source(
        &self,
        module: &crate::ModuleDeclarations,
        source: &SemanticSource<'hir>,
    ) -> Result<(), BodyTypingError> {
        self.sources
            .validate_source(self.accounting, source)
            .map_err(BodyTypingError::SourceInputs)?;
        self.bodies
            .validate_source(self.accounting, module, source)
            .map_err(BodyTypingError::CallInputs)
    }
}

impl BodyChecker<'_, '_, '_> {
    pub(super) fn prepare_dispatch_work(
        &mut self,
        site: crate::MethodDispatchSite,
    ) -> Result<(), BodyFlow> {
        if let Some(work) = &mut self.analysis_work {
            work.dispatch_ticket(site)?;
        }
        Ok(())
    }

    pub(super) fn existing_dispatch_work(
        &mut self,
        site: crate::MethodDispatchSite,
    ) -> Result<Option<AnalysisRootTicket>, BodyFlow> {
        match &mut self.analysis_work {
            Some(work) => Ok(Some(work.existing_dispatch_ticket(site)?)),
            None => Ok(None),
        }
    }

    pub(super) fn record_method_dispatch(
        &mut self,
        pending: PendingMethodDispatch,
        ticket: Option<AnalysisRootTicket>,
    ) -> Result<(), BodyFlow> {
        if let Some(work) = &mut self.analysis_work {
            let ticket = ticket.ok_or(BodyTypingError::ProofInputModeMismatch)?;
            let BodyProofInputs::Metered(inputs) = &mut self.proof_inputs else {
                return Err(BodyTypingError::ProofInputModeMismatch.into());
            };
            inputs.push_dispatch(work.accounting, pending.site(), ticket)?;
        }
        self.method_dispatches.push(pending);
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod tests;
