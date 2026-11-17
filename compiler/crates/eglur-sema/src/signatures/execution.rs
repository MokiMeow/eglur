use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use eglur_diagnostics::Poison;

use super::*;
use crate::type_rendering::DiagnosticTypeNames;
use crate::{AnalysisWorkIncomplete, CapabilityDemand, PhaseAnalysis};

/// Signature analysis without conflating resource interruption with an internal error.
#[must_use]
pub enum SignatureExecution {
    Completed(Box<PhaseAnalysis<ResolvedDeclarationSemantics>>),
    Interrupted(Box<InterruptedSignatures>),
}

/// Source-owned backing and published facts, without a completed signature seal.
///
/// Source, declaration collection, imports, trusted inputs and diagnostics remain
/// caller-owned. This prefix is non-resumable and cannot be used as a completed
/// signature product.
///
/// ```compile_fail
/// use eglur_sema::{DeclarationSignatures, SignatureResolutionPrefix};
/// fn completed(prefix: SignatureResolutionPrefix) -> DeclarationSignatures { prefix }
/// ```
pub struct SignatureResolutionPrefix {
    pub(super) type_applications: super::applications::TypeApplicationFacts,
    pub(super) types: crate::CanonicalTypeStore,
    pub(super) diagnostic_type_names: DiagnosticTypeNames,
    pub(super) declarations: crate::FactTable<crate::CanonicalTarget, DeclarationSignature>,
    pub(super) enum_variant_names:
        HashMap<crate::CanonicalTarget, crate::SemanticFact<HashSet<crate::NameKey>>>,
    pub(super) overload_family_poisons: BTreeMap<crate::DefId, Poison>,
    pub(super) generic_parameters: GenericParameterRecords,
    pub(super) occurrences: crate::FactTable<TypeOccurrenceId, ResolvedTypeOccurrence>,
    pub(super) callables: Vec<CallableSignature>,
    pub(super) impl_headers: crate::FactTable<crate::CanonicalTarget, TraitImplHeader>,
    pub(super) rejected_impl_headers: BTreeMap<crate::CanonicalTarget, TraitImplHeader>,
    pub(super) impl_later_roots: BTreeMap<crate::DefId, Poison>,
    pub(super) trait_methods: BTreeMap<crate::CanonicalTarget, Box<[TraitMethodSignature]>>,
    pub(super) trait_method_owners: BTreeMap<crate::CanonicalTarget, crate::CanonicalTarget>,
    pub(super) declared_impl_methods: BTreeMap<crate::CanonicalTarget, Box<[DeclaredImplMethod]>>,
    pub(super) trait_adapters: BTreeMap<crate::CanonicalTarget, TraitAdapterSignature>,
    pub(super) trusted_inherent_owners: BTreeMap<ConcreteOwnerHead, InherentMethodInventory>,
    pub(super) prelude_traits: BTreeSet<PreludeTraitSignature>,
    pub(super) language_items: crate::LanguageItems,
    pub(super) inhabitance: Option<crate::InhabitanceFacts>,
}

impl SignatureResolutionPrefix {
    #[must_use]
    pub const fn types(&self) -> &crate::CanonicalTypeStore {
        &self.types
    }

    #[must_use]
    pub fn declaration(
        &self,
        target: &crate::CanonicalTarget,
    ) -> crate::FactState<'_, DeclarationSignature> {
        super::model::declaration_fact_state(
            &self.declarations,
            &self.overload_family_poisons,
            target,
        )
    }

    #[must_use]
    pub fn occurrence(
        &self,
        occurrence: TypeOccurrenceId,
    ) -> crate::FactState<'_, ResolvedTypeOccurrence> {
        self.occurrences.state(&occurrence)
    }

    #[must_use]
    pub fn generic_parameter(
        &self,
        parameter: &crate::GenericParameterId,
    ) -> crate::FactState<'_, GenericParameterSignature> {
        self.generic_parameters.state(parameter)
    }

    #[must_use]
    pub fn callable(&self, id: crate::CallableSignatureId) -> Option<&CallableSignature> {
        self.callables.get(id.index())
    }

    #[must_use]
    pub fn declared_name(&self, target: &crate::CanonicalTarget) -> Option<&str> {
        self.diagnostic_type_names.target_name(target)
    }

    #[must_use]
    pub const fn language_items(&self) -> &crate::LanguageItems {
        &self.language_items
    }

    #[must_use]
    pub const fn inhabitance(&self) -> Option<&crate::InhabitanceFacts> {
        self.inhabitance.as_ref()
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.type_applications.applications.len()
            + self.declarations.iter().len()
            + self.enum_variant_names.len()
            + self.overload_family_poisons.len()
            + self.generic_parameters.len()
            + self.occurrences.iter().len()
            + self.callables.len()
            + self.impl_headers.iter().len()
            + self.rejected_impl_headers.len()
            + self.impl_later_roots.len()
            + self.trait_methods.len()
            + self.trait_method_owners.len()
            + self.declared_impl_methods.len()
            + self.trait_adapters.len()
            + self.trusted_inherent_owners.len()
            + self.prelude_traits.len()
    }
}

/// First accounting stop and the signature evidence already produced.
pub struct InterruptedSignatures {
    pub(super) stop: AnalysisWorkIncomplete,
    pub(super) prefix: Option<SignatureResolutionPrefix>,
    pub(super) demands: Vec<CapabilityDemand>,
    pub(super) internal_failures: Vec<SignatureResolutionError>,
}

impl InterruptedSignatures {
    #[must_use]
    pub const fn stop(&self) -> AnalysisWorkIncomplete {
        self.stop
    }

    /// None means the resolver was never constructed, not an empty completed phase.
    #[must_use]
    pub const fn prefix(&self) -> Option<&SignatureResolutionPrefix> {
        self.prefix.as_ref()
    }

    #[must_use]
    pub fn demands(&self) -> &[CapabilityDemand] {
        &self.demands
    }

    #[must_use]
    pub fn internal_failures(&self) -> &[SignatureResolutionError] {
        &self.internal_failures
    }
}
