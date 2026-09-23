use std::collections::BTreeMap;

use eglur_diagnostics::{
    DiagnosticDataValue, DiagnosticStore, DiagnosticStoreError, Poison, RelatedSpan, SourceSpan,
};
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    TypecheckCapability,
};
use crate::diagnostic::record_type_error_with_related;
use crate::type_rendering::DiagnosticTypeRenderer;
use crate::{
    CallableSignature, CanonicalTarget, CanonicalTypeStore, DeclarationCollection, DeclarationKind,
    DeclarationSignature, DeclarationSignatures, DeclaredImplMethod, DefId, FactState,
    GenericParameterId, IdentityOverflow, NameKey, SemanticFact, SemanticLocation, SemanticSource,
    TraitImplHeader, TraitMethodSignature, TraitRef, TypeId, WitnessSlotId,
};

use super::environment::{
    CollectedImplementations, TraitAssumption, TraitAssumptionEvidence, TraitEnvironment, TraitImpl,
};
use super::error::TraitEnvironmentError;
use super::model::{TraitNormalization, TraitObligation};
use super::overlap::{ImplBuckets, classify_overlap, target_bucket};

mod applications;
mod assembly;
mod contracts;

use assembly::{
    build_assumptions, build_buckets, collect_implementations, validate_checked_coherence,
};
use contracts::validate_impl_methods;
pub(crate) use contracts::{CheckedMethodBound, checked_method_bound_mapping};

/// Builds the only impl/coherence environment consumed by body and dispatch phases.
pub fn build_trait_environment(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    signatures: &mut DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<TraitEnvironment>, TraitEnvironmentError> {
    let mut recorder = DemandRecorder::new(support);
    let capability = CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment);
    let mut supported = true;
    for module in collection.modules() {
        supported &= recorder.require(capability, SemanticLocation::source(module.file()));
    }
    if !supported {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }

    let CollectedImplementations::Complete(implementations) = collect_implementations(signatures)?
    else {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    };
    validate_checked_coherence(signatures, &implementations)?;
    let buckets = build_buckets(signatures.types(), &implementations)?;
    let assumptions = build_assumptions(collection, signatures)?;
    let mut environment =
        TraitEnvironment::from_checked_parts(implementations, buckets, assumptions);
    if !applications::validate_type_applications(
        collection,
        sources,
        signatures,
        diagnostics,
        &mut environment,
    )? {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }
    validate_impl_methods(
        collection,
        sources,
        signatures,
        diagnostics,
        &mut environment,
    )?;
    if !applications::validate_type_applications(
        collection,
        sources,
        signatures,
        diagnostics,
        &mut environment,
    )? {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }
    Ok(recorder.finish(PhaseProduct::Performed(environment)))
}
