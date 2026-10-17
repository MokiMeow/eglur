use std::collections::BTreeMap;

use super::*;

/// Caller-retained source custody shared across analysis phases of one attempt.
///
/// File/consumer association is supplied at input setup. Construction admits no
/// roots and indexes no alias expansion. Reconstructing this set is not replay.
pub struct AnalysisSourceInputs<'hir> {
    files: BTreeMap<FileId, AnalysisSourceInputRegistry<'hir>>,
}

impl<'hir> AnalysisSourceInputs<'hir> {
    pub fn new<'source>(
        accounting: &AnalysisWorkAccounting,
        sources: impl IntoIterator<Item = (usize, &'source crate::SemanticSource<'hir>)>,
    ) -> Result<Self, AnalysisSourceInputFailure>
    where
        'hir: 'source,
    {
        let mut files = BTreeMap::new();
        for (consumer, source) in sources {
            if files.contains_key(&source.file()) {
                return Err(AnalysisSourceInputFailure::DuplicateFile(source.file()));
            }
            files.insert(
                source.file(),
                AnalysisSourceInputRegistry::from_source(accounting, consumer, source)?,
            );
        }
        Ok(Self { files })
    }

    pub fn written_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        occurrence: TypeOccurrenceId,
        allowance_ticks: u64,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        self.files
            .get_mut(&occurrence.file())
            .ok_or(AnalysisSourceInputFailure::MissingFile(occurrence.file()))?
            .written_ticket(accounting, occurrence, allowance_ticks)
    }

    pub(crate) fn written_ticket_with(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        occurrence: TypeOccurrenceId,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        let registry = self
            .files
            .get_mut(&occurrence.file())
            .ok_or(AnalysisSourceInputFailure::MissingFile(occurrence.file()))?;
        accounting.validate_input_binding(registry.consumer, &registry.attempt)?;
        let allowance_ticks = if registry.admitted_ticket(occurrence)?.is_some() {
            0
        } else {
            allowance()
        };
        registry.written_ticket(accounting, occurrence, allowance_ticks)
    }

    pub fn inherited_ticket(
        &self,
        accounting: &mut AnalysisWorkAccounting,
        file: FileId,
        upstream: &AnalysisRootTicket,
    ) -> Result<AnalysisRootTicket, AnalysisSourceInputFailure> {
        self.files
            .get(&file)
            .ok_or(AnalysisSourceInputFailure::MissingFile(file))?
            .inherited_ticket(accounting, upstream)
    }

    /// Read-only evidence; consumers still pay retrieval before copying a ticket.
    pub fn admitted_ticket(
        &self,
        occurrence: TypeOccurrenceId,
    ) -> Result<Option<&AnalysisRootTicket>, AnalysisSourceInputFailure> {
        self.files
            .get(&occurrence.file())
            .ok_or(AnalysisSourceInputFailure::MissingFile(occurrence.file()))?
            .admitted_ticket(occurrence)
    }

    pub(crate) fn validate_source(
        &self,
        accounting: &AnalysisWorkAccounting,
        source: &crate::SemanticSource<'hir>,
    ) -> Result<(), AnalysisSourceInputFailure> {
        let registry = self
            .files
            .get(&source.file())
            .ok_or(AnalysisSourceInputFailure::MissingFile(source.file()))?;
        accounting.validate_input_binding(registry.consumer, &registry.attempt)?;
        if !source
            .hir()
            .is_some_and(|hir| std::ptr::eq(registry.hir, hir))
        {
            return Err(AnalysisSourceInputFailure::SourceMismatch(source.file()));
        }
        Ok(())
    }
}
