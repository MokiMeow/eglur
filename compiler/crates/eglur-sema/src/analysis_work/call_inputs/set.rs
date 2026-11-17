use std::collections::BTreeMap;

use eglur_hir::HirFile;
use eglur_source::FileId;

use crate::{ModuleAddress, ModuleDeclarations, ModuleId, SemanticSource};

use super::*;

struct ModuleInputs<'hir> {
    file: FileId,
    address: ModuleAddress,
    hir: &'hir HirFile,
    consumer: usize,
    attempt: std::sync::Arc<()>,
    bodies: Vec<AnalysisCallInputRegistry<'hir>>,
}

/// Caller-retained call registries for the original bodies of one attempt.
///
/// Setup indexes normalized input HIR without root admission or alias expansion.
/// Retain this set across passes; reconstruction is not replay. Setup is not
/// semantic-operation metering and does not enable compiler limits.
pub struct AnalysisBodyInputs<'hir> {
    modules: BTreeMap<ModuleId, ModuleInputs<'hir>>,
}

impl<'hir> AnalysisBodyInputs<'hir> {
    pub fn new<'source, 'module>(
        accounting: &AnalysisWorkAccounting,
        inputs: impl IntoIterator<
            Item = (
                usize,
                &'module ModuleDeclarations,
                &'source SemanticSource<'hir>,
            ),
        >,
    ) -> Result<Self, AnalysisCallRegistryFailure>
    where
        'hir: 'source,
    {
        let mut modules = BTreeMap::new();
        for (consumer, module, source) in inputs {
            let attempt = accounting.bind_input_consumer(consumer)?;
            if modules.contains_key(&module.id()) {
                return Err(AnalysisCallRegistryFailure::DuplicateModule(module.id()));
            }
            if module.file() != source.file() || module.address() != source.module() {
                return Err(AnalysisCallRegistryFailure::SourceMismatch {
                    module: module.id(),
                    file: source.file(),
                });
            }
            let hir = source
                .hir()
                .ok_or(AnalysisCallRegistryFailure::MissingHir(source.file()))?;
            std::alloc::Layout::array::<AnalysisCallInputRegistry<'hir>>(hir.bodies().len())
                .map_err(AnalysisCallRegistryFailure::Layout)?;
            let bodies = hir
                .bodies()
                .map(|(_, body)| {
                    AnalysisCallInputRegistry::new(accounting, consumer, module.id(), body)
                })
                .collect::<Result<Vec<_>, _>>()?;
            modules.insert(
                module.id(),
                ModuleInputs {
                    file: source.file(),
                    address: source.module().clone(),
                    hir,
                    consumer,
                    attempt,
                    bodies,
                },
            );
        }
        Ok(Self { modules })
    }

    pub(crate) fn validate_source(
        &self,
        accounting: &AnalysisWorkAccounting,
        module: &ModuleDeclarations,
        source: &SemanticSource<'hir>,
    ) -> Result<(), AnalysisCallRegistryFailure> {
        let inputs = self
            .modules
            .get(&module.id())
            .ok_or(AnalysisCallRegistryFailure::MissingModule(module.id()))?;
        accounting.validate_input_binding(inputs.consumer, &inputs.attempt)?;
        if inputs.file != source.file()
            || module.file() != source.file()
            || inputs.address != *source.module()
            || module.address() != source.module()
            || !source
                .hir()
                .is_some_and(|hir| std::ptr::eq(inputs.hir, hir))
        {
            return Err(AnalysisCallRegistryFailure::SourceMismatch {
                module: module.id(),
                file: source.file(),
            });
        }
        Ok(())
    }

    pub(crate) fn selection_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.registry(site)?
            .selection_ticket_with(accounting, site, allowance)
    }

    pub(crate) fn argument_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        index: usize,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.registry(site)?
            .argument_ticket_with(accounting, site, index, allowance)
    }

    pub(crate) fn existing_selection_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
    ) -> Result<AnalysisRootTicket, AnalysisCallRegistryFailure> {
        self.registry(site)?
            .existing_selection_ticket(accounting, site)
    }

    fn registry(
        &mut self,
        site: ExpressionKey,
    ) -> Result<&mut AnalysisCallInputRegistry<'hir>, AnalysisCallRegistryFailure> {
        self.modules
            .get_mut(&site.module())
            .ok_or(AnalysisCallRegistryFailure::MissingModule(site.module()))?
            .bodies
            .get_mut(site.expression().body().get() as usize)
            .ok_or(AnalysisCallRegistryFailure::MissingBody {
                module: site.module(),
                body: site.expression().body(),
            })
    }
}
