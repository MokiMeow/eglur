use super::*;
use crate::AnalysisRootTicket;

pub(in crate::signatures::resolver) enum SourceCustody {
    Unmetered,
    Ticket(AnalysisRootTicket),
}

impl SignatureResolver<'_, '_> {
    pub(in crate::signatures::resolver) fn original_source_custody(
        &mut self,
        occurrence: TypeOccurrenceId,
    ) -> Result<SourceCustody, SignatureFlow> {
        self.ensure_analysis_active()?;
        let Some(work) = &mut self.analysis_work else {
            return Ok(SourceCustody::Unmetered);
        };
        Ok(SourceCustody::Ticket(work.original_ticket(occurrence)?))
    }

    pub(in crate::signatures::resolver) fn inherited_source_custody(
        &mut self,
        file: FileId,
        upstream: &SourceCustody,
    ) -> Result<SourceCustody, SignatureFlow> {
        self.ensure_analysis_active()?;
        match (&mut self.analysis_work, upstream) {
            (None, SourceCustody::Unmetered) => Ok(SourceCustody::Unmetered),
            (Some(work), SourceCustody::Ticket(ticket)) => Ok(SourceCustody::Ticket(
                work.inputs
                    .inherited_ticket(work.accounting, file, ticket)?,
            )),
            _ => Err(SignatureResolutionError::MissingSourceCustody.into()),
        }
    }

    pub(in crate::signatures::resolver) fn claim_source_custody(
        &mut self,
        custody: &SourceCustody,
    ) -> Result<(), SignatureFlow> {
        self.ensure_analysis_active()?;
        match (&mut self.analysis_work, custody) {
            (None, SourceCustody::Unmetered) => Ok(()),
            (Some(work), SourceCustody::Ticket(ticket)) => {
                // Claim pays constant-sized controller bookkeeping, not semantic work.
                #[cfg(test)]
                super::worklist::custody_tests::record_claim(ticket.identity());
                work.accounting.claim(ticket)?;
                Ok(())
            }
            _ => Err(SignatureResolutionError::MissingSourceCustody.into()),
        }
    }
}
