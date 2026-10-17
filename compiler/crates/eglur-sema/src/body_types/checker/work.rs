use super::*;
use crate::{
    AnalysisBodyInputs, AnalysisCallRegistryFailure, AnalysisRootTicket,
    AnalysisSourceInputFailure, AnalysisSourceInputs, AnalysisWorkAccounting,
};

/// Original source inputs supplied to an explicit allowance policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BodyAnalysisInput {
    WrittenType(TypeOccurrenceId),
    CallSelection(ExpressionKey),
    CallArgument { call: ExpressionKey, index: usize },
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

#[cfg(test)]
pub(super) mod tests;
