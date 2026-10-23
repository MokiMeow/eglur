use std::collections::BTreeMap;

use eglur_hir::HirFile;
use eglur_source::FileId;

use crate::{
    ExpressionKey, GenericParameterRecord, GenericParameterRecordId, MethodDispatchSite,
    ModuleAddress, ModuleDeclarations, ModuleId, SemanticSource,
};

use super::*;

/// Failure to bind original call or proof registries to a compilation attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisBodyInputFailure {
    Call(AnalysisCallRegistryFailure),
    Proof(AnalysisProofRegistryFailure),
}

impl From<AnalysisCallRegistryFailure> for AnalysisBodyInputFailure {
    fn from(failure: AnalysisCallRegistryFailure) -> Self {
        Self::Call(failure)
    }
}

impl From<AnalysisProofRegistryFailure> for AnalysisBodyInputFailure {
    fn from(failure: AnalysisProofRegistryFailure) -> Self {
        Self::Proof(failure)
    }
}

impl From<AnalysisWorkFailure> for AnalysisBodyInputFailure {
    fn from(failure: AnalysisWorkFailure) -> Self {
        Self::Call(failure.into())
    }
}

impl std::fmt::Display for AnalysisBodyInputFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Call(failure) => failure.fmt(formatter),
            Self::Proof(failure) => failure.fmt(formatter),
        }
    }
}

impl std::error::Error for AnalysisBodyInputFailure {}

struct BodyInputs<'hir> {
    calls: AnalysisCallInputRegistry<'hir>,
    proofs: AnalysisProofInputRegistry<'hir>,
}

struct ModuleInputs<'hir> {
    file: FileId,
    address: ModuleAddress,
    hir: &'hir HirFile,
    consumer: usize,
    attempt: std::sync::Arc<()>,
    bodies: Vec<BodyInputs<'hir>>,
}

/// Caller-retained call and proof registries for the original bodies of one attempt.
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
    ) -> Result<Self, AnalysisBodyInputFailure>
    where
        'hir: 'source,
    {
        let mut modules = BTreeMap::new();
        for (consumer, module, source) in inputs {
            let attempt = accounting.bind_input_consumer(consumer)?;
            if modules.contains_key(&module.id()) {
                return Err(AnalysisCallRegistryFailure::DuplicateModule(module.id()).into());
            }
            if module.file() != source.file() || module.address() != source.module() {
                return Err(AnalysisCallRegistryFailure::SourceMismatch {
                    module: module.id(),
                    file: source.file(),
                }
                .into());
            }
            let hir = source
                .hir()
                .ok_or(AnalysisCallRegistryFailure::MissingHir(source.file()))?;
            std::alloc::Layout::array::<BodyInputs<'hir>>(hir.bodies().len())
                .map_err(AnalysisCallRegistryFailure::Layout)?;
            let bodies = hir
                .bodies()
                .map(|(_, body)| {
                    Ok(BodyInputs {
                        calls: AnalysisCallInputRegistry::new(
                            accounting,
                            consumer,
                            module.id(),
                            body,
                        )?,
                        proofs: AnalysisProofInputRegistry::new(
                            accounting,
                            consumer,
                            module.id(),
                            body,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, AnalysisBodyInputFailure>>()?;
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
        Ok(&mut self.body(site)?.calls)
    }

    fn body(
        &mut self,
        site: ExpressionKey,
    ) -> Result<&mut BodyInputs<'hir>, AnalysisCallRegistryFailure> {
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

    pub fn bound_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        parameter: &GenericParameterRecord,
        index: usize,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisBodyInputFailure> {
        Ok(self
            .body(site)?
            .proofs
            .bound_ticket(accounting, site, parameter, index, allowance)?)
    }

    pub fn existing_bound_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: ExpressionKey,
        parameter: GenericParameterRecordId,
        index: usize,
    ) -> Result<AnalysisRootTicket, AnalysisBodyInputFailure> {
        Ok(self
            .body(site)?
            .proofs
            .existing_bound_ticket(accounting, site, parameter, index)?)
    }

    pub fn dispatch_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: MethodDispatchSite,
        allowance: impl FnOnce() -> u64,
    ) -> Result<AnalysisRootTicket, AnalysisBodyInputFailure> {
        Ok(self
            .body(site.key())?
            .proofs
            .dispatch_ticket(accounting, site, allowance)?)
    }

    pub fn existing_dispatch_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        site: MethodDispatchSite,
    ) -> Result<AnalysisRootTicket, AnalysisBodyInputFailure> {
        Ok(self
            .body(site.key())?
            .proofs
            .existing_dispatch_ticket(accounting, site)?)
    }
}
