use super::{NameResolutionError, Resolver, VisitFold, VisitOutcome};
use crate::{ModuleId, SemanticSource};
use eglur_hir::{HirFile, ItemId};

impl Resolver<'_, '_> {
    pub(super) fn visit_item_children(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        seed: VisitOutcome,
        children: &[ItemId],
    ) -> Result<VisitOutcome, NameResolutionError> {
        let mut folded = VisitFold::Empty.include(seed)?;
        for &child in children {
            let outcome = self.visit_item(module, source, hir, child)?;
            folded = folded.include(outcome)?;
        }
        Ok(folded.finish())
    }
}

#[cfg(test)]
mod tests;
