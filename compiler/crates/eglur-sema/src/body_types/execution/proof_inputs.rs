use std::sync::Arc;

use crate::record_reservation::RecordReservation;
use crate::{
    AnalysisRootTicket, AnalysisWork, AnalysisWorkAccounting, AnalysisWorkCategory,
    AnalysisWorkFailure, AnalysisWorkOverflow, ExpressionKey, GenericParameterRecordId,
    MethodDispatchSite,
};

use super::super::BodyTypingError;

/// Execution custody is separate from semantic body facts and never owns balances.
pub enum BodyProofInputs {
    Unmetered,
    Metered(MeteredBodyProofInputs),
}

/// Original request connections in the same order as the body's pending evidence.
pub struct MeteredBodyProofInputs {
    attempt: Arc<()>,
    bounds: Vec<BoundProofInput>,
    dispatches: Vec<DispatchProofInput>,
}

/// An original declaration-ordered bound and its existing attempt-local budget.
pub struct BoundProofInput {
    site: ExpressionKey,
    parameter: GenericParameterRecordId,
    bound_index: usize,
    ticket: AnalysisRootTicket,
}

impl BoundProofInput {
    #[must_use]
    pub const fn site(&self) -> ExpressionKey {
        self.site
    }

    #[must_use]
    pub const fn parameter(&self) -> GenericParameterRecordId {
        self.parameter
    }

    #[must_use]
    pub const fn bound_index(&self) -> usize {
        self.bound_index
    }

    #[must_use]
    pub const fn ticket(&self) -> &AnalysisRootTicket {
        &self.ticket
    }
}

/// The initial method-lookup budget reused for its later proof.
pub struct DispatchProofInput {
    site: MethodDispatchSite,
    ticket: AnalysisRootTicket,
}

impl DispatchProofInput {
    #[must_use]
    pub const fn site(&self) -> MethodDispatchSite {
        self.site
    }

    #[must_use]
    pub const fn ticket(&self) -> &AnalysisRootTicket {
        &self.ticket
    }
}

impl MeteredBodyProofInputs {
    #[must_use]
    pub fn bounds(&self) -> &[BoundProofInput] {
        &self.bounds
    }

    #[must_use]
    pub fn dispatches(&self) -> &[DispatchProofInput] {
        &self.dispatches
    }

    pub fn validate_accounting(
        &self,
        accounting: &AnalysisWorkAccounting,
    ) -> Result<(), AnalysisWorkFailure> {
        accounting.validate_attempt_binding(&self.attempt)
    }
}

impl BodyProofInputs {
    pub(in crate::body_types) fn new(accounting: Option<&AnalysisWorkAccounting>) -> Self {
        accounting.map_or(Self::Unmetered, |accounting| {
            Self::Metered(MeteredBodyProofInputs {
                attempt: accounting.attempt_binding(),
                bounds: Vec::new(),
                dispatches: Vec::new(),
            })
        })
    }
}

impl MeteredBodyProofInputs {
    pub(in crate::body_types) fn push_bound(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        parameter: GenericParameterRecordId,
        bound_index: usize,
        ticket: AnalysisRootTicket,
    ) -> Result<(), AnalysisWorkFailure> {
        self.validate_accounting(accounting)?;
        let reservation = RecordReservation::try_for_append(&self.bounds);
        let work = reservation
            .as_ref()
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))
            .and_then(|_| attachment_work::<BoundProofInput>());
        charge_attachment(accounting, &ticket, work)?;
        reservation
            .expect("paid bound attachment growth")
            .reserve(&mut self.bounds);
        self.bounds.push(BoundProofInput {
            site,
            parameter,
            bound_index,
            ticket,
        });
        Ok(())
    }

    pub(in crate::body_types) fn push_dispatch(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: MethodDispatchSite,
        ticket: AnalysisRootTicket,
    ) -> Result<(), AnalysisWorkFailure> {
        self.validate_accounting(accounting)?;
        let reservation = RecordReservation::try_for_append(&self.dispatches);
        let work = reservation
            .as_ref()
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))
            .and_then(|_| attachment_work::<DispatchProofInput>());
        charge_attachment(accounting, &ticket, work)?;
        reservation
            .expect("paid dispatch attachment growth")
            .reserve(&mut self.dispatches);
        self.dispatches.push(DispatchProofInput { site, ticket });
        Ok(())
    }
}

impl BodyProofInputs {
    pub(in crate::body_types) fn validate_lengths(
        &self,
        bounds: usize,
        dispatches: usize,
    ) -> Result<(), BodyTypingError> {
        match self {
            Self::Unmetered => Ok(()),
            Self::Metered(inputs)
                if inputs.bounds.len() == bounds && inputs.dispatches.len() == dispatches =>
            {
                Ok(())
            }
            Self::Metered(_) => Err(BodyTypingError::ProofInputCardinalityMismatch),
        }
    }
}

fn attachment_work<T>() -> Result<AnalysisWork, AnalysisWorkOverflow> {
    let header = size_of::<T>() as u64;
    Ok(AnalysisWork {
        steps: 3,
        inspected_bytes: 0,
        transferred_bytes: header.checked_mul(3).ok_or(AnalysisWorkOverflow::Quantity(
            AnalysisWorkCategory::TransferredBytes,
        ))?,
        reserved_bytes: header.checked_mul(4).ok_or(AnalysisWorkOverflow::Quantity(
            AnalysisWorkCategory::ReservedBytes,
        ))?,
    })
}

fn charge_attachment(
    accounting: &mut AnalysisWorkAccounting,
    ticket: &AnalysisRootTicket,
    work: Result<AnalysisWork, AnalysisWorkOverflow>,
) -> Result<(), AnalysisWorkFailure> {
    let mut scope = accounting.claim(ticket)?;
    match work {
        Ok(work) => scope.precharge(work),
        Err(overflow) => Err(scope.stop_unrepresentable(overflow)),
    }
}
