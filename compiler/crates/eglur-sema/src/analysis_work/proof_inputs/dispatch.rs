use super::*;

/// Original dispatch custody shared by initial lookup and subsequent evidence.
/// The source adapter owns once-only construction and source-site validation.
pub struct AnalysisDispatchInputs {
    site: MethodDispatchSite,
    ticket: AnalysisRootTicket,
}

impl AnalysisDispatchInputs {
    pub fn admit(
        accounting: &mut AnalysisWorkAccounting,
        consumer: usize,
        site: MethodDispatchSite,
        allowance_ticks: u64,
    ) -> Result<Self, AnalysisProofInputFailure> {
        Self::admit_with_work(accounting, consumer, site, allowance_ticks, Ok(no_work()))
    }

    pub(super) fn admit_with_work(
        accounting: &mut AnalysisWorkAccounting,
        consumer: usize,
        site: MethodDispatchSite,
        allowance_ticks: u64,
        additional: Result<AnalysisWork, AnalysisWorkOverflow>,
    ) -> Result<Self, AnalysisProofInputFailure> {
        let ticket = accounting.admit_with_work(
            consumer,
            allowance_ticks,
            additional.and_then(|work| {
                work.checked_add(AnalysisWork {
                    steps: 1,
                    inspected_bytes: 0,
                    transferred_bytes: size_of::<Self>() as u64,
                    reserved_bytes: 0,
                })
            }),
        )?;
        Ok(Self { site, ticket })
    }

    #[must_use]
    pub const fn site(&self) -> MethodDispatchSite {
        self.site
    }

    pub(super) const fn ticket(&self) -> &AnalysisRootTicket {
        &self.ticket
    }

    pub fn existing_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
    ) -> Result<AnalysisRootTicket, AnalysisProofInputFailure> {
        self.existing_ticket_with_work(accounting, no_work())
    }

    pub(super) fn existing_ticket_with_work(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        additional: AnalysisWork,
    ) -> Result<AnalysisRootTicket, AnalysisProofInputFailure> {
        charge_ticket_retrieval(accounting, &self.ticket, additional)?;
        Ok(self.ticket.clone())
    }
}
