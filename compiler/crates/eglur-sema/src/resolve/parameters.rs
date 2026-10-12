use super::{NameResolutionError, Resolver, VisitFold};
use crate::{ModuleId, SemanticSource};
use eglur_hir::{Body, HirFile, Parameter};

impl Resolver<'_, '_> {
    pub(super) fn visit_parameters(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        parameters: &[Parameter],
    ) -> Result<VisitFold, NameResolutionError> {
        let mut outcome = VisitFold::Known;
        for parameter in parameters {
            let group = self.pattern_bindings(source, body, parameter.pattern)?;
            let binding = if let Some(collision) = self.first_collision(&group) {
                self.record_binding_collision(source, collision)?
            } else {
                self.bind_group(source, group, None)?
            };
            outcome = outcome.include(binding)?;
            outcome =
                outcome.include(self.visit_optional_type(module, source, hir, parameter.ty)?)?;
        }
        Ok(outcome)
    }
}
