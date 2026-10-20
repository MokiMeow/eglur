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
        let ticket = accounting.admit_with_work(
            consumer,
            allowance_ticks,
            Ok(AnalysisWork {
                steps: 1,
                inspected_bytes: 0,
                transferred_bytes: size_of::<Self>() as u64,
                reserved_bytes: 0,
            }),
        )?;
        Ok(Self { site, ticket })
    }

    #[must_use]
    pub const fn site(&self) -> MethodDispatchSite {
        self.site
    }

    pub fn existing_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
    ) -> Result<AnalysisRootTicket, AnalysisProofInputFailure> {
        accounting.claim(&self.ticket)?.precharge(ticket_work())?;
        Ok(self.ticket.clone())
    }
}
