use std::alloc::{Layout, LayoutError};
use std::fmt;
use std::sync::Arc;

use eglur_hir::BodyId;
use eglur_source::FileId;

use crate::ModuleId;
use crate::record_reservation::RecordReservation;

use super::*;

/// Invalid registry input, unrepresentable setup or interrupted analysis work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisCallRegistryFailure {
    Layout(LayoutError),
    Input(AnalysisCallInputFailure),
    UnadmittedCall(ExpressionKey),
    DuplicateModule(ModuleId),
    MissingModule(ModuleId),
    MissingHir(FileId),
    SourceMismatch { module: ModuleId, file: FileId },
    MissingBody { module: ModuleId, body: BodyId },
}

impl From<AnalysisCallInputFailure> for AnalysisCallRegistryFailure {
    fn from(failure: AnalysisCallInputFailure) -> Self {
        Self::Input(failure)
    }
}

impl From<AnalysisWorkFailure> for AnalysisCallRegistryFailure {
    fn from(failure: AnalysisWorkFailure) -> Self {
        Self::Input(failure.into())
    }
}

impl fmt::Display for AnalysisCallRegistryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => error.fmt(formatter),
            Self::Input(error) => error.fmt(formatter),
            Self::UnadmittedCall(site) => {
                write!(formatter, "call input was not admitted: {site:?}")
            }
            Self::DuplicateModule(module) => {
                write!(formatter, "duplicate call input module: {module:?}")
            }
            Self::MissingModule(module) => {
                write!(formatter, "missing call input module: {module:?}")
            }
            Self::MissingHir(file) => write!(formatter, "call input source has no HIR: {file:?}"),
            Self::SourceMismatch { module, file } => write!(
                formatter,
                "call input source mismatch: {module:?}, {file:?}"
            ),
            Self::MissingBody { module, body } => {
                write!(formatter, "missing call input body: {module:?}, {body:?}")
            }
        }
    }
}

impl std::error::Error for AnalysisCallRegistryFailure {}

/// Source-bound lookup of previously admitted call inputs, outside semantic facts.
///
/// The source owner retains one registry per body across passes. Reconstructing it
/// is not replay. Construction prepares only input-bounded occurrence indices;
/// holders and balances stay lazy. The owner supplies the canonical module/body
/// association. This registry neither activates a compiler budget nor meters the
/// finite index setup. Contextual expectations are per-visit loans, not call keys.
pub struct AnalysisCallInputRegistry<'hir> {
    module: ModuleId,
    body: &'hir Body,
    consumer: usize,
    attempt: Arc<()>,
    indices: Vec<Option<usize>>,
    holders: Vec<AnalysisCallInputs>,
}

impl<'hir> AnalysisCallInputRegistry<'hir> {
    pub fn new(
        accounting: &AnalysisWorkAccounting,
        consumer: usize,
        module: ModuleId,
        body: &'hir Body,
    ) -> Result<Self, AnalysisCallRegistryFailure> {
        let attempt = accounting.bind_input_consumer(consumer)?;
        let count = body.expressions().len();
        Layout::array::<Option<usize>>(count).map_err(AnalysisCallRegistryFailure::Layout)?;
        Ok(Self {
            module,
            body,
            consumer,
            attempt,
            indices: vec![None; count],
            holders: Vec::new(),
        })
    }

    /// Admits before candidate preparation, or reuses the existing selection.
    /// The supplied allowance is ignored on replay; callee results use this ticket.
    pub fn selection_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        allowance_ticks: u64,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.selection_ticket_with(accounting, site, || allowance_ticks)
    }

    pub(super) fn selection_ticket_with(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let occurrence = self.call_occurrence(site)?;

        if let Some(index) = self.indices[occurrence] {
            let ticket = self.holders[index].selection_ticket();
            accounting
                .claim(ticket)?
                .precharge(Self::retrieval_work())?;
            return Ok(ticket.clone());
        }

        let reservation = RecordReservation::try_for_append(&self.holders);
        let work = reservation
            .as_ref()
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))
            .and_then(|_| Self::installation_work());
        let holder = AnalysisCallInputs::admit_with_work(
            accounting,
            self.consumer,
            site,
            self.body,
            allowance(),
            None,
            work,
        )?;

        reservation
            .expect("admission checked holder growth")
            .reserve(&mut self.holders);
        let index = self.holders.len();
        self.holders.push(holder);
        self.indices[occurrence] = Some(index);
        Ok(self.holders[index].selection_ticket().clone())
    }

    /// Retrieves one written slot without admitting an implicit selection.
    pub fn argument_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        written_index: usize,
        allowance_ticks: u64,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.argument_ticket_with(accounting, site, written_index, || allowance_ticks)
    }

    pub(super) fn argument_ticket_with(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        written_index: usize,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let occurrence = self.call_occurrence(site)?;
        let index =
            self.indices[occurrence].ok_or(AnalysisCallRegistryFailure::UnadmittedCall(site))?;
        let argument = self.holders[index].arguments.get(written_index).ok_or(
            AnalysisCallInputFailure::UnknownArgument {
                call: site,
                index: written_index,
            },
        )?;
        let allowance_ticks = if argument.is_some() { 0 } else { allowance() };
        let ticket = self.holders[index].argument_ticket_with_work(
            accounting,
            written_index,
            allowance_ticks,
            Some(Self::retrieval_work()),
        )?;
        Ok(ticket.clone())
    }

    /// Borrows the current upstream expectation without caching or admitting it.
    pub fn contextual_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        upstream: &AnalysisRootTicket,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        accounting
            .claim(upstream)?
            .precharge(Self::retrieval_work())?;
        Ok(upstream.clone())
    }

    fn call_occurrence(&self, site: ExpressionKey) -> Result<usize, AnalysisCallInputFailure> {
        if site.module() != self.module {
            return Err(AnalysisCallInputFailure::NotOriginalCall(site));
        }
        original_call(self.body, site)?;
        Ok(site.expression().local() as usize)
    }

    fn installation_work() -> Result<AnalysisWork, AnalysisWorkOverflow> {
        let header = size_of::<AnalysisCallInputs>() as u64;
        let work = AnalysisWork {
            steps: 1,
            inspected_bytes: 0,
            // Holder publication is included by AnalysisCallInputs::header_work.
            transferred_bytes: header
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(size_of::<Option<usize>>() as u64))
                .ok_or(AnalysisWorkOverflow::Quantity(
                    AnalysisWorkCategory::TransferredBytes,
                ))?,
            reserved_bytes: header.checked_mul(4).ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))?,
        };
        work.checked_add(Self::retrieval_work())
    }

    const fn retrieval_work() -> AnalysisWork {
        AnalysisWork {
            steps: 1,
            inspected_bytes: (size_of::<Option<usize>>() + size_of::<AnalysisCallInputs>()) as u64,
            transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
            reserved_bytes: 0,
        }
    }
}

#[cfg(test)]
mod tests;
