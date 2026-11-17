use std::alloc::{Layout, LayoutError};
use std::fmt;
use std::sync::Arc;

use eglur_hir::{Body, ExprKind};

use crate::record_reservation::RecordReservation;
use crate::{GenericParameterRecord, GenericParameterRecordId, ModuleId};

use super::*;

mod index;

use index::{BoundIndex, BoundLink, BoundLookup};

/// Invalid original proof input or an interrupted registration operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisProofRegistryFailure {
    Layout(LayoutError),
    Input(AnalysisProofInputFailure),
    NotOriginalInput(ExpressionKey),
    UnadmittedParameter {
        site: ExpressionKey,
        parameter: GenericParameterRecordId,
    },
    UnadmittedDispatch(MethodDispatchSite),
}

impl From<AnalysisProofInputFailure> for AnalysisProofRegistryFailure {
    fn from(failure: AnalysisProofInputFailure) -> Self {
        Self::Input(failure)
    }
}

impl From<AnalysisWorkFailure> for AnalysisProofRegistryFailure {
    fn from(failure: AnalysisWorkFailure) -> Self {
        Self::Input(failure.into())
    }
}

impl fmt::Display for AnalysisProofRegistryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => error.fmt(formatter),
            Self::Input(error) => error.fmt(formatter),
            Self::NotOriginalInput(site) => {
                write!(formatter, "not an original proof input: {site:?}")
            }
            Self::UnadmittedParameter { site, parameter } => write!(
                formatter,
                "parameter {parameter:?} was not admitted at {site:?}"
            ),
            Self::UnadmittedDispatch(site) => {
                write!(formatter, "dispatch input was not admitted: {site:?}")
            }
        }
    }
}

impl std::error::Error for AnalysisProofRegistryFailure {}

#[derive(Clone, Default)]
struct ProofOccurrence {
    bounds: Option<BoundLink>,
    dispatch: [Option<usize>; 2],
}

/// Once-only proof inputs for one original body and one immutable signature graph.
///
/// Source adapters retain this registry across passes. Original definition
/// references must be acquired under the upstream request's preparation budget;
/// this registry does not pay for converting declaration names to references.
/// Setup indexes only source expressions and does not activate compiler limits.
pub struct AnalysisProofInputRegistry<'hir> {
    module: ModuleId,
    body: &'hir Body,
    consumer: usize,
    attempt: Arc<()>,
    occurrences: Vec<ProofOccurrence>,
    index: BoundIndex,
    bounds: Vec<AnalysisBoundInputs>,
    dispatch: Vec<AnalysisDispatchInputs>,
}

impl<'hir> AnalysisProofInputRegistry<'hir> {
    pub fn new(
        accounting: &AnalysisWorkAccounting,
        consumer: usize,
        module: ModuleId,
        body: &'hir Body,
    ) -> Result<Self, AnalysisProofRegistryFailure> {
        let attempt = accounting.bind_input_consumer(consumer)?;
        let count = body.expressions().len();
        Layout::array::<ProofOccurrence>(count).map_err(AnalysisProofRegistryFailure::Layout)?;
        Ok(Self {
            module,
            body,
            consumer,
            attempt,
            occurrences: vec![ProofOccurrence::default(); count],
            index: BoundIndex::default(),
            bounds: Vec::new(),
            dispatch: Vec::new(),
        })
    }

    pub fn bound_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        parameter: &GenericParameterRecord,
        bound_index: usize,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisProofRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let occurrence = self.bound_occurrence(site)?;
        if bound_index >= parameter.signature().declared_bounds().len() {
            return Err(AnalysisProofInputFailure::UnknownDeclaredBound {
                site,
                index: bound_index,
            }
            .into());
        }

        let insertion = match self
            .index
            .lookup(self.occurrences[occurrence].bounds, parameter.id().index())
        {
            BoundLookup::Existing(index) => {
                return Ok(self.bounds[index].bound_ticket_with_work(
                    accounting,
                    bound_index,
                    allowance,
                    Self::bound_lookup_work(),
                )?);
            }
            BoundLookup::Vacant(insertion) => insertion,
        };
        let holder_reservation = RecordReservation::try_for_append(&self.bounds);
        let branch_reservation = self.index.reservation(&insertion);
        let work = holder_reservation
            .as_ref()
            .zip(branch_reservation.as_ref())
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))
            .and_then(|_| Self::bound_installation_work(insertion.needs_branch()));
        let holder = AnalysisBoundInputs::admit_with_work(
            accounting,
            self.consumer,
            site,
            parameter.signature(),
            bound_index,
            allowance(),
            work,
        )?;
        let index = self.bounds.len();
        holder_reservation
            .expect("admission checked bound holder growth")
            .reserve(&mut self.bounds);
        self.bounds.push(holder);
        self.index.install(
            &mut self.occurrences[occurrence].bounds,
            insertion,
            index,
            branch_reservation.expect("admission checked branch growth"),
        );
        Ok(self.bounds[index].anchor_ticket().clone())
    }

    /// Retrieves the original slot without accepting admission authority.
    pub fn existing_bound_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        parameter: GenericParameterRecordId,
        bound_index: usize,
    ) -> Result<AnalysisRootTicket, AnalysisProofRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let occurrence = self.bound_occurrence(site)?;
        let BoundLookup::Existing(index) = self
            .index
            .lookup(self.occurrences[occurrence].bounds, parameter.index())
        else {
            return Err(AnalysisProofRegistryFailure::UnadmittedParameter { site, parameter });
        };
        Ok(self.bounds[index].existing_ticket_with_work(
            accounting,
            bound_index,
            Self::bound_lookup_work(),
        )?)
    }

    pub fn dispatch_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: MethodDispatchSite,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisProofRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let (occurrence, role) = self.dispatch_occurrence(site)?;
        if let Some(index) = self.occurrences[occurrence].dispatch[role] {
            return Ok(self.dispatch[index]
                .existing_ticket_with_work(accounting, Self::dispatch_lookup_work())?);
        }
        let reservation = RecordReservation::try_for_append(&self.dispatch);
        let work = reservation
            .as_ref()
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))
            .and_then(|_| {
                Self::record_installation_work(size_of::<AnalysisDispatchInputs>())?
                    .checked_add(Self::dispatch_lookup_work())
            });
        let holder = AnalysisDispatchInputs::admit_with_work(
            accounting,
            self.consumer,
            site,
            allowance(),
            work,
        )?;
        let ticket = holder.ticket().clone();
        reservation
            .expect("admission checked dispatch holder growth")
            .reserve(&mut self.dispatch);
        let index = self.dispatch.len();
        self.dispatch.push(holder);
        self.occurrences[occurrence].dispatch[role] = Some(index);
        Ok(ticket)
    }

    pub fn existing_dispatch_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        site: MethodDispatchSite,
    ) -> Result<AnalysisRootTicket, AnalysisProofRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let (occurrence, role) = self.dispatch_occurrence(site)?;
        let index = self.occurrences[occurrence].dispatch[role]
            .ok_or(AnalysisProofRegistryFailure::UnadmittedDispatch(site))?;
        Ok(self.dispatch[index]
            .existing_ticket_with_work(accounting, Self::dispatch_lookup_work())?)
    }

    fn occurrence(&self, site: ExpressionKey) -> Result<usize, AnalysisProofRegistryFailure> {
        if site.module() != self.module
            || site.expression().body() != self.body.id()
            || site.expression().local() as usize >= self.occurrences.len()
        {
            return Err(AnalysisProofRegistryFailure::NotOriginalInput(site));
        }
        Ok(site.expression().local() as usize)
    }

    fn bound_occurrence(&self, site: ExpressionKey) -> Result<usize, AnalysisProofRegistryFailure> {
        let occurrence = self.occurrence(site)?;
        let expression = self
            .body
            .expression(site.expression())
            .expect("validated occurrence");
        if matches!(
            expression.kind(),
            ExprKind::Name(_)
                | ExprKind::Member { .. }
                | ExprKind::StructLiteral { .. }
                | ExprKind::VariantConstructor { .. }
        ) || self.invocation_occurrence(site)
        {
            return Ok(occurrence);
        }
        Err(AnalysisProofRegistryFailure::NotOriginalInput(site))
    }

    fn dispatch_occurrence(
        &self,
        site: MethodDispatchSite,
    ) -> Result<(usize, usize), AnalysisProofRegistryFailure> {
        let occurrence = self.occurrence(site.key())?;
        let valid = match site {
            MethodDispatchSite::Invocation(key) => self.invocation_occurrence(key),
            MethodDispatchSite::CallableValue(key) => matches!(
                self.body
                    .expression(key.expression())
                    .expect("validated occurrence")
                    .kind(),
                ExprKind::Name(_) | ExprKind::Member { .. }
            ),
        };
        if !valid {
            return Err(AnalysisProofRegistryFailure::NotOriginalInput(site.key()));
        }
        Ok((
            occurrence,
            usize::from(matches!(site, MethodDispatchSite::CallableValue(_))),
        ))
    }

    const fn bound_lookup_work() -> AnalysisWork {
        let walks = 2 * usize::BITS as u64;
        AnalysisWork {
            steps: walks + 4,
            inspected_bytes: walks * BoundIndex::branch_size() as u64
                + size_of::<ProofOccurrence>() as u64
                + size_of::<AnalysisBoundInputs>() as u64,
            transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
            reserved_bytes: 0,
        }
    }

    fn invocation_occurrence(&self, site: ExpressionKey) -> bool {
        match self
            .body
            .expression(site.expression())
            .map(|value| value.kind())
        {
            Some(ExprKind::Call(call)) => !call.partial_application,
            Some(ExprKind::ReduceByCandidate(_)) => true,
            _ => false,
        }
    }

    const fn dispatch_lookup_work() -> AnalysisWork {
        AnalysisWork {
            steps: 4,
            inspected_bytes: (size_of::<ProofOccurrence>() + size_of::<AnalysisDispatchInputs>())
                as u64,
            transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
            reserved_bytes: 0,
        }
    }

    fn bound_installation_work(branch: bool) -> Result<AnalysisWork, AnalysisWorkOverflow> {
        let mut work = Self::record_installation_work(size_of::<AnalysisBoundInputs>())?
            .checked_add(Self::bound_lookup_work())?;
        if branch {
            work = work.checked_add(Self::record_installation_work(BoundIndex::branch_size())?)?;
        }
        Ok(work)
    }

    fn record_installation_work(header: usize) -> Result<AnalysisWork, AnalysisWorkOverflow> {
        let header = u64::try_from(header)
            .map_err(|_| AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::ReservedBytes))?;
        let transferred_bytes = header
            .checked_mul(3)
            .and_then(|bytes| bytes.checked_add(size_of::<ProofOccurrence>() as u64))
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::TransferredBytes,
            ))?;
        let reserved_bytes = header.checked_mul(4).ok_or(AnalysisWorkOverflow::Quantity(
            AnalysisWorkCategory::ReservedBytes,
        ))?;
        Ok(AnalysisWork {
            steps: 2,
            inspected_bytes: 0,
            transferred_bytes,
            reserved_bytes,
        })
    }
}

#[cfg(test)]
mod tests;
