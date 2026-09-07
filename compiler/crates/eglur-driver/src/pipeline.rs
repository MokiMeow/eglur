use eglur_sema::{
    CapabilityDemand, CapabilityTrace, CapabilityTraceError, PhaseAnalysis, PhaseProduct,
};

use crate::observation::{ObservationCollector, Phase};
use crate::support::SupportProfile;

/// Session-local execution contract for observed semantic phases.
pub(crate) struct SemanticPipeline<'a> {
    profile: &'a SupportProfile,
    observations: &'a mut ObservationCollector,
    demands: Vec<CapabilityDemand>,
}

impl<'a> SemanticPipeline<'a> {
    pub(crate) const fn new(
        profile: &'a SupportProfile,
        observations: &'a mut ObservationCollector,
    ) -> Self {
        Self {
            profile,
            observations,
            demands: Vec::new(),
        }
    }

    pub(crate) fn measure<T, E>(
        &mut self,
        phase: Phase,
        input_count: usize,
        operation: impl FnOnce() -> Result<T, E>,
        output_count: impl FnOnce(&T) -> usize,
    ) -> Result<T, E> {
        self.observations
            .measure(phase, input_count, operation, output_count)
    }

    pub(crate) fn run<T, E>(
        &mut self,
        phase: Phase,
        input_count: usize,
        operation: impl FnOnce() -> Result<PhaseAnalysis<T>, E>,
        output_count: impl FnOnce(&T) -> usize,
    ) -> Result<PhaseProduct<T>, E> {
        let analysis = self.measure(phase, input_count, operation, |analysis| {
            match analysis.product() {
                PhaseProduct::Performed(product) => output_count(product),
                PhaseProduct::Unperformed => 0,
            }
        })?;
        let (product, demands) = analysis.into_parts();
        self.demands.extend(demands);
        Ok(product)
    }

    pub(crate) fn finish(self) -> Result<CapabilityTrace, CapabilityTraceError> {
        CapabilityTrace::from_demands(self.profile, self.demands)
    }
}
