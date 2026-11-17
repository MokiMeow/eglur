use std::alloc::{Layout, LayoutError};
use std::fmt;
use std::sync::Arc;

use eglur_hir::{HirFile, TypeRefKind};
use eglur_source::FileId;

use crate::TypeOccurrenceId;
use crate::record_reservation::RecordReservation;

use super::*;

mod set;
pub use set::AnalysisSourceInputs;

/// Invalid source custody, unrepresentable setup or interrupted analysis work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisSourceInputFailure {
    Layout(LayoutError),
    InvalidHir(FileId),
    NotOriginalType(TypeOccurrenceId),
    MissingHir(FileId),
    DuplicateFile(FileId),
    MissingFile(FileId),
    SourceMismatch(FileId),
    Work(AnalysisWorkFailure),
}

impl From<AnalysisWorkFailure> for AnalysisSourceInputFailure {
    fn from(failure: AnalysisWorkFailure) -> Self {
        Self::Work(failure)
    }
}

impl fmt::Display for AnalysisSourceInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => error.fmt(formatter),
            Self::InvalidHir(file) => write!(formatter, "invalid source HIR: {file:?}"),
            Self::NotOriginalType(site) => {
                write!(formatter, "not an original written type input: {site:?}")
            }
            Self::MissingHir(file) => write!(formatter, "source has no HIR: {file:?}"),
            Self::DuplicateFile(file) => write!(formatter, "duplicate source custody: {file:?}"),
            Self::MissingFile(file) => write!(formatter, "missing source custody: {file:?}"),
            Self::SourceMismatch(file) => {
                write!(formatter, "source custody HIR mismatch: {file:?}")
            }
            Self::Work(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for AnalysisSourceInputFailure {}

#[derive(Clone, Copy)]
enum SourceTypeSlot {
    Child,
    Original(Option<usize>),
}

/// Retains written whole-type tickets outside canonical semantic facts.
///
/// The source owner retains one registry per attempt/file and supplies the
/// canonical file/HIR association. Reconstruction is not replay. Setup indexes
/// written HIR nodes and direct edges only; it neither expands aliases nor
/// admits roots. Worklists must carry inherited tickets instead of admitting
/// the children of tuples, function types, references or type applications.
/// This adapter does not activate compiler limits or meter semantic resolution.
pub struct AnalysisSourceInputRegistry<'hir> {
    file: FileId,
    hir: &'hir HirFile,
    consumer: usize,
    attempt: Arc<()>,
    slots: Vec<SourceTypeSlot>,
    tickets: Vec<AnalysisRootTicket>,
}

impl<'hir> AnalysisSourceInputRegistry<'hir> {
    pub fn new(
        accounting: &AnalysisWorkAccounting,
        consumer: usize,
        file: FileId,
        hir: &'hir HirFile,
    ) -> Result<Self, AnalysisSourceInputFailure> {
        accounting.bind_input_consumer(consumer)?;
        if !hir.invalid_occurrences().is_empty() {
            return Err(AnalysisSourceInputFailure::InvalidHir(file));
        }
        Self::index(accounting, consumer, file, hir)
    }

    /// Accepts recovered HIR through its validated source/poison association.
    pub fn from_source(
        accounting: &AnalysisWorkAccounting,
        consumer: usize,
        source: &crate::SemanticSource<'hir>,
    ) -> Result<Self, AnalysisSourceInputFailure> {
        let hir = source
            .hir()
            .ok_or(AnalysisSourceInputFailure::MissingHir(source.file()))?;
        Self::index(accounting, consumer, source.file(), hir)
    }

    fn index(
        accounting: &AnalysisWorkAccounting,
        consumer: usize,
        file: FileId,
        hir: &'hir HirFile,
    ) -> Result<Self, AnalysisSourceInputFailure> {
        let attempt = accounting.bind_input_consumer(consumer)?;

        let count = hir.type_refs().len();
        Layout::array::<SourceTypeSlot>(count).map_err(AnalysisSourceInputFailure::Layout)?;
        let mut slots = vec![SourceTypeSlot::Original(None); count];
        for (_, ty) in hir.type_refs() {
            let mut child = |type_ref: eglur_hir::TypeRefId| {
                slots[type_ref.get() as usize] = SourceTypeSlot::Child;
            };
            match ty.kind() {
                TypeRefKind::Reference { referent, .. }
                | TypeRefKind::Pointer { referent, .. }
                | TypeRefKind::Group(referent)
                | TypeRefKind::Dynamic {
                    trait_ref: referent,
                } => child(*referent),
                TypeRefKind::Path { arguments, .. } | TypeRefKind::Tuple(arguments) => {
                    for &argument in arguments {
                        child(argument);
                    }
                }
                TypeRefKind::Function { parameters, result } => {
                    for &parameter in parameters {
                        child(parameter);
                    }
                    child(*result);
                }
                TypeRefKind::Invalid => {}
            }
        }

        Ok(Self {
            file,
            hir,
            consumer,
            attempt,
            slots,
            tickets: Vec::new(),
        })
    }

    /// Observes custody without cloning, charging, admission or renewal.
    pub fn admitted_ticket(
        &self,
        occurrence: TypeOccurrenceId,
    ) -> Result<Option<&AnalysisRootTicket>, AnalysisSourceInputFailure> {
        let ordinal = self.original_ordinal(occurrence)?;
        Ok(match self.slots[ordinal] {
            SourceTypeSlot::Original(Some(index)) => Some(&self.tickets[index]),
            SourceTypeSlot::Original(None) => None,
            SourceTypeSlot::Child => unreachable!("validated original occurrence"),
        })
    }

    /// Admits before source-context preparation or reclaims the written input.
    /// A replay ignores the supplied allowance and pays retrieval before cloning.
    pub fn written_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        occurrence: TypeOccurrenceId,
        allowance_ticks: u64,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        let ordinal = self.original_ordinal(occurrence)?;
        if let SourceTypeSlot::Original(Some(index)) = self.slots[ordinal] {
            let ticket = &self.tickets[index];
            accounting
                .claim(ticket)?
                .precharge(Self::retrieval_work())?;
            return Ok(ticket.clone());
        }

        let reservation = RecordReservation::try_for_append(&self.tickets);
        let work = reservation
            .as_ref()
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))
            .and_then(|_| Self::installation_work());
        let ticket = accounting.admit_with_work(self.consumer, allowance_ticks, work)?;

        reservation
            .expect("admission checked source ticket growth")
            .reserve(&mut self.tickets);
        let index = self.tickets.len();
        self.tickets.push(ticket);
        self.slots[ordinal] = SourceTypeSlot::Original(Some(index));
        Ok(self.tickets[index].clone())
    }

    /// Carries a demand into alias or structural work without original admission.
    /// The ticket may originate in another source file of the same attempt.
    pub fn inherited_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        upstream: &AnalysisRootTicket,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        // Invalid ticket custody stays an internal failure even after a stop.
        accounting
            .balance(upstream)
            .map_err(AnalysisWorkFailure::Claim)?;
        accounting.validate_input_binding(self.consumer, &self.attempt)?;
        accounting.claim(upstream)?.precharge(AnalysisWork {
            steps: 1,
            inspected_bytes: (size_of::<AnalysisRootTicket>() + size_of::<AnalysisRootBalance>())
                as u64,
            transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
            reserved_bytes: 0,
        })?;
        Ok(upstream.clone())
    }

    fn original_ordinal(
        &self,
        occurrence: TypeOccurrenceId,
    ) -> Result<usize, AnalysisSourceInputFailure> {
        let ordinal = occurrence.type_ref().get() as usize;
        if occurrence.file() != self.file
            || ordinal >= self.hir.type_refs().len()
            || !matches!(self.slots.get(ordinal), Some(SourceTypeSlot::Original(_)))
        {
            return Err(AnalysisSourceInputFailure::NotOriginalType(occurrence));
        }
        Ok(ordinal)
    }

    fn installation_work() -> Result<AnalysisWork, AnalysisWorkOverflow> {
        let header = size_of::<AnalysisRootTicket>() as u64;
        AnalysisWork {
            steps: 1,
            inspected_bytes: 0,
            transferred_bytes: header
                .checked_mul(3)
                .and_then(|bytes| bytes.checked_add(size_of::<SourceTypeSlot>() as u64))
                .ok_or(AnalysisWorkOverflow::Quantity(
                    AnalysisWorkCategory::TransferredBytes,
                ))?,
            reserved_bytes: header.checked_mul(4).ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))?,
        }
        .checked_add(Self::retrieval_work())
    }

    const fn retrieval_work() -> AnalysisWork {
        AnalysisWork {
            steps: 1,
            inspected_bytes: (size_of::<SourceTypeSlot>() + size_of::<AnalysisRootTicket>()) as u64,
            transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
            reserved_bytes: 0,
        }
    }
}

#[cfg(test)]
mod tests;
