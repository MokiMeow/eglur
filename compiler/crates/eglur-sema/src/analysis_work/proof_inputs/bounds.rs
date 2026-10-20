use super::*;

/// Tickets for one original parameter's declaration-ordered bounds at a source site.
///
/// The source adapter must construct this holder once and retain it across passes.
/// This holder does not index source occurrences or activate compiler limits.
pub struct AnalysisBoundInputs {
    site: ExpressionKey,
    parameter: GenericParameterId,
    tickets: Vec<Option<AnalysisRootTicket>>,
    anchor: AnalysisRootTicket,
}

impl AnalysisBoundInputs {
    /// Pays original identifier and slot preparation before cloning or allocation.
    /// Other declared bounds retain independent, lazily admitted balances.
    pub fn admit(
        accounting: &mut AnalysisWorkAccounting,
        consumer: usize,
        site: ExpressionKey,
        parameter: &GenericParameterSignature,
        bound_index: usize,
        allowance_ticks: u64,
    ) -> Result<Self, AnalysisProofInputFailure> {
        let count = parameter.declared_bounds().len();
        if bound_index >= count {
            return Err(AnalysisProofInputFailure::UnknownDeclaredBound {
                site,
                index: bound_index,
            });
        }

        let ticket = accounting.admit_with_work(
            consumer,
            allowance_ticks,
            Self::preparation_work(parameter.id(), count),
        )?;
        let parameter = {
            #[cfg(test)]
            super::tests::record_parameter_copy();
            parameter.id().clone()
        };
        let mut tickets = Vec::with_capacity(count);
        tickets.resize_with(count, || {
            #[cfg(test)]
            super::tests::record_header_write();
            None
        });
        tickets[bound_index] = Some(ticket.clone());

        Ok(Self {
            site,
            parameter,
            tickets,
            anchor: ticket,
        })
    }

    #[must_use]
    pub const fn site(&self) -> ExpressionKey {
        self.site
    }

    #[must_use]
    pub const fn parameter(&self) -> &GenericParameterId {
        &self.parameter
    }

    #[must_use]
    pub fn declared_bound_count(&self) -> usize {
        self.tickets.len()
    }

    /// Only a missing original slot invokes the allowance policy.
    pub fn bound_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        index: usize,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisProofInputFailure> {
        self.validate(accounting, index)?;
        if let Some(ticket) = &self.tickets[index] {
            accounting.claim(ticket)?.precharge(ticket_work())?;
            return Ok(ticket.clone());
        }

        if let Some(stop) = accounting.incomplete() {
            return Err(AnalysisWorkFailure::Incomplete(stop).into());
        }

        let ticket = accounting.admit_with_work(
            self.anchor.identity().consumer(),
            allowance(),
            ticket_work().checked_add(AnalysisWork {
                steps: 1,
                inspected_bytes: 0,
                transferred_bytes: size_of::<Option<AnalysisRootTicket>>() as u64,
                reserved_bytes: 0,
            }),
        )?;
        self.tickets[index] = Some(ticket.clone());
        Ok(ticket)
    }

    /// Evidence consumers cannot admit a missing bound or renew its allowance.
    pub fn existing_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        index: usize,
    ) -> Result<AnalysisRootTicket, AnalysisProofInputFailure> {
        self.validate(accounting, index)?;
        let ticket =
            self.tickets[index]
                .as_ref()
                .ok_or(AnalysisProofInputFailure::UnadmittedBound {
                    site: self.site,
                    index,
                })?;
        accounting.claim(ticket)?.precharge(ticket_work())?;
        Ok(ticket.clone())
    }

    fn validate(
        &self,
        accounting: &AnalysisWorkAccounting,
        index: usize,
    ) -> Result<(), AnalysisProofInputFailure> {
        accounting
            .balance(&self.anchor)
            .map_err(AnalysisWorkFailure::Claim)?;
        if index >= self.tickets.len() {
            return Err(AnalysisProofInputFailure::UnknownDeclaredBound {
                site: self.site,
                index,
            });
        }
        Ok(())
    }

    fn preparation_work(
        parameter: &GenericParameterId,
        count: usize,
    ) -> Result<AnalysisWork, AnalysisWorkOverflow> {
        let overflow = AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::ReservedBytes);
        let layout = Layout::array::<Option<AnalysisRootTicket>>(count).map_err(|_| overflow)?;
        let payload = match parameter {
            GenericParameterId::Source(_) => 0,
            GenericParameterId::Trusted { owner, .. } => owner.as_str().len(),
        };
        let bytes = layout.size().checked_add(payload).ok_or(overflow)?;
        let reserved_bytes = u64::try_from(bytes).map_err(|_| overflow)?;
        let transferred_bytes = reserved_bytes
            .checked_add((size_of::<Self>() + size_of::<Option<AnalysisRootTicket>>()) as u64)
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::TransferredBytes,
            ))?;
        Ok(AnalysisWork {
            steps: 1,
            inspected_bytes: size_of::<GenericParameterId>() as u64,
            transferred_bytes,
            reserved_bytes,
        })
    }
}
