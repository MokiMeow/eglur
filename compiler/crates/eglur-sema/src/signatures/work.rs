use super::TypeOccurrenceId;
use crate::{AnalysisSourceInputFailure, AnalysisSourceInputs, AnalysisWorkAccounting};

/// Explicit source custody and allowance lookup, without a default pricing policy.
///
/// The caller retains inputs across phases. The allowance lookup must be
/// deterministic and input-bounded; it runs only on first original admission.
/// This loan does not enable driver limits or certify semantic-operation metering.
pub struct SignatureAnalysisWork<'work, 'hir> {
    pub(super) accounting: &'work mut AnalysisWorkAccounting,
    pub(super) inputs: &'work mut AnalysisSourceInputs<'hir>,
    pub(super) allowance: &'work dyn Fn(TypeOccurrenceId) -> u64,
}

impl<'work, 'hir> SignatureAnalysisWork<'work, 'hir> {
    pub fn new(
        accounting: &'work mut AnalysisWorkAccounting,
        inputs: &'work mut AnalysisSourceInputs<'hir>,
        allowance: &'work dyn Fn(TypeOccurrenceId) -> u64,
    ) -> Self {
        Self {
            accounting,
            inputs,
            allowance,
        }
    }

    pub(super) fn original_ticket(
        &mut self,
        occurrence: TypeOccurrenceId,
    ) -> Result<crate::AnalysisRootTicket, AnalysisSourceInputFailure> {
        self.inputs
            .written_ticket_with(self.accounting, occurrence, || (self.allowance)(occurrence))
    }
}
