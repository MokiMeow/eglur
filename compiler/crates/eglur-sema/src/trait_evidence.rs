use std::collections::BTreeMap;
use std::fmt;

use eglur_diagnostics::{DiagnosticDataValue, DiagnosticStore, DiagnosticStoreError, Poison};
use eglur_hir::ExprId;
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    TypecheckCapability,
};
use crate::diagnostic::{SemanticDiagnosticError, record_type_error};
use crate::record_reservation::RecordReservation;
use crate::type_rendering::{DiagnosticTypeRenderer, DiagnosticTypeRenderingError};
use crate::{
    BodyTypes, CallableInstanceId, CanonicalTarget, DeclarationCollection, DeclarationSignatures,
    DefId, DuplicateFact, EvidenceEnvironmentId, EvidenceProofId, ExpressionKey, FactState,
    FactTable, GenericParameterId, IdentityOverflow, InvocationFact, InvocationTarget,
    MethodDispatchSite, PendingMethodDispatch, PendingTraitObligation, SemanticFact,
    SemanticLocation, SemanticSource, SubstId, TraitEnvironment, TraitEnvironmentError,
    TraitImplEvidence, TraitObligation, TraitProof, TraitRef, TypeId, WitnessSlotId,
};

mod execution;
mod proof;
mod route;
mod work;

pub use execution::{
    InterruptedTraitEvidence, TraitEvidenceExecution, TraitEvidencePrefix,
    resolve_trait_evidence_with_work,
};

#[cfg(test)]
mod record_growth_tests;

use proof::ProofArena;
use route::build_method_routes;

/// One checked source for a first-order nominal trait obligation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EvidenceSource {
    Bound(WitnessSlotId),
    Implementation {
        implementation: CanonicalTarget,
        substitution: SubstId,
        implementation_bounds: Box<[EvidenceProofId]>,
        trait_bounds: Box<[EvidenceProofId]>,
    },
}

/// One generic parameter bound and the evidence selected for it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceEntry {
    parameter: GenericParameterId,
    bound_index: usize,
    subject: TypeId,
    trait_ref: TraitRef,
    proof: EvidenceProofId,
    destination: Option<WitnessSlotId>,
}

impl EvidenceEntry {
    #[must_use]
    pub const fn parameter(&self) -> &GenericParameterId {
        &self.parameter
    }

    #[must_use]
    pub const fn bound_index(&self) -> usize {
        self.bound_index
    }

    #[must_use]
    pub const fn subject(&self) -> TypeId {
        self.subject
    }

    #[must_use]
    pub const fn trait_ref(&self) -> &TraitRef {
        &self.trait_ref
    }

    #[must_use]
    pub const fn proof(&self) -> EvidenceProofId {
        self.proof
    }

    /// Returns the source-callable witness slot filled by this checked proof.
    #[must_use]
    pub const fn destination(&self) -> Option<WitnessSlotId> {
        self.destination
    }
}

/// Canonical checked evidence vector shared by equivalent invocations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceEnvironment {
    id: EvidenceEnvironmentId,
    entries: Box<[EvidenceEntry]>,
}

impl EvidenceEnvironment {
    #[must_use]
    pub const fn id(&self) -> EvidenceEnvironmentId {
        self.id
    }

    #[must_use]
    pub fn entries(&self) -> &[EvidenceEntry] {
        &self.entries
    }
}

/// Evaluator-facing dispatch selected without runtime semantic search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckedDispatch {
    Direct(CanonicalTarget),
    Bound {
        witness: WitnessSlotId,
        method: CanonicalTarget,
    },
    ImplTemplate {
        callable: CanonicalTarget,
        method: CanonicalTarget,
        proof: EvidenceProofId,
    },
}

/// A checked source for a witness required by a selected method body.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum MethodFrameSource {
    Root,
    ImplementationBound(usize),
    TraitBound(usize),
    MethodBound {
        parameter: GenericParameterId,
        bound_index: usize,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MethodFrameBinding {
    destination: WitnessSlotId,
    source: MethodFrameSource,
}

impl MethodFrameBinding {
    #[must_use]
    pub const fn destination(&self) -> WitnessSlotId {
        self.destination
    }

    #[must_use]
    pub const fn source(&self) -> &MethodFrameSource {
        &self.source
    }
}

/// The checked route from one selected implementation to its source body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodFrameRoute {
    target: DefId,
    bindings: Box<[MethodFrameBinding]>,
}

impl MethodFrameRoute {
    #[must_use]
    pub const fn target(&self) -> DefId {
        self.target
    }

    #[must_use]
    pub fn bindings(&self) -> &[MethodFrameBinding] {
        &self.bindings
    }
}

/// A checked static call or a call through a typed runtime callable value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckedInvocation {
    Static {
        dispatch: CheckedDispatch,
        evidence_environment: EvidenceEnvironmentId,
    },
    Indirect {
        callee: ExprId,
        ty: TypeId,
    },
}

/// Dispatch and witnesses fixed when a named callable value is constructed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedCallableValue {
    dispatch: CheckedDispatch,
    evidence_environment: EvidenceEnvironmentId,
}

impl CheckedCallableValue {
    #[must_use]
    pub const fn dispatch(&self) -> &CheckedDispatch {
        &self.dispatch
    }

    #[must_use]
    pub const fn evidence_environment(&self) -> EvidenceEnvironmentId {
        self.evidence_environment
    }
}

/// Post-body checked dispatch and evidence facts.
pub struct TraitEvidence {
    invocations: FactTable<ExpressionKey, CheckedInvocation>,
    callable_values: FactTable<ExpressionKey, CheckedCallableValue>,
    environments: Box<[EvidenceEnvironment]>,
    proofs: Box<[EvidenceSource]>,
    method_routes: BTreeMap<(CanonicalTarget, CanonicalTarget), MethodFrameRoute>,
}

impl TraitEvidence {
    #[cfg(test)]
    pub(crate) fn replace_proofs_for_test(&mut self, proofs: Vec<EvidenceSource>) {
        self.proofs = proofs.into_boxed_slice();
    }

    #[must_use]
    pub fn invocation(&self, key: ExpressionKey) -> FactState<'_, CheckedInvocation> {
        self.invocations.state(&key)
    }

    #[must_use]
    pub fn callable_value(&self, key: ExpressionKey) -> FactState<'_, CheckedCallableValue> {
        self.callable_values.state(&key)
    }

    #[must_use]
    pub fn environment(&self, id: EvidenceEnvironmentId) -> Option<&EvidenceEnvironment> {
        self.environments.get(id.index())
    }

    #[must_use]
    pub fn proof(&self, id: EvidenceProofId) -> Option<&EvidenceSource> {
        self.proofs.get(id.index())
    }

    #[must_use]
    pub fn method_route(
        &self,
        implementation: &CanonicalTarget,
        method: &CanonicalTarget,
    ) -> Option<&MethodFrameRoute> {
        self.method_routes
            .get(&(implementation.clone(), method.clone()))
    }

    pub fn invocations(
        &self,
    ) -> impl ExactSizeIterator<Item = (ExpressionKey, FactState<'_, CheckedInvocation>)> {
        self.invocations
            .iter()
            .map(|(key, _)| (*key, self.invocations.state(key)))
    }

    pub fn environments(&self) -> impl ExactSizeIterator<Item = &EvidenceEnvironment> {
        self.environments.iter()
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.invocations.iter().len()
            + self.callable_values.iter().len()
            + self.proofs.len()
            + self.method_routes.len()
            + self
                .environments
                .iter()
                .map(|environment| 1 + environment.entries.len())
                .sum::<usize>()
    }

    pub(crate) fn is_complete_for(&self, body_types: &BodyTypes) -> bool {
        self.invocations.iter().len() == body_types.invocations().len()
            && self.invocations.all_known()
            && self.callable_values.iter().len() == body_types.callable_values().len()
            && self.callable_values.all_known()
    }
}

/// Resolves body-recorded obligations and materializes checked direct dispatch.
pub fn resolve_trait_evidence(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    signatures: &mut DeclarationSignatures,
    trait_environment: &mut TraitEnvironment,
    body_types: &BodyTypes,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<TraitEvidence>, TraitEvidenceError> {
    match execution::execute(
        collection,
        sources,
        support,
        signatures,
        trait_environment,
        Some(body_types),
        diagnostics,
        work::EvidenceInputs::Unmetered,
    )? {
        TraitEvidenceExecution::Completed(phase) => Ok(*phase),
        TraitEvidenceExecution::Interrupted(_) => {
            Err(TraitEvidenceError::UnexpectedUnmeteredInterruption)
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "evidence traversal coordinates independent phase, storage and work owners"
)]
fn traverse_evidence(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    signatures: &mut DeclarationSignatures,
    trait_environment: &mut TraitEnvironment,
    body_types: &BodyTypes,
    diagnostics: &mut DiagnosticStore,
    recorder: &mut DemandRecorder<'_>,
    state: &mut execution::EvidenceState,
    work: &mut work::EvidenceInputs<'_>,
) -> Result<execution::EvidenceTraversal, execution::EvidenceFlow> {
    let source_map = sources
        .iter()
        .map(|source| (source.file(), source))
        .collect::<BTreeMap<_, _>>();
    let invocations = body_types.invocations().collect::<Vec<_>>();
    let capability = CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence);
    let mut supported = true;
    for (key, _) in &invocations {
        supported &= recorder.require(
            capability,
            invocation_location(collection, &source_map, *key)?,
        );
    }
    for (key, _) in body_types.callable_values() {
        supported &= recorder.require(
            capability,
            invocation_location(collection, &source_map, key)?,
        );
    }
    if !supported {
        return Ok(execution::EvidenceTraversal::Unperformed);
    }

    let mut obligations = BTreeMap::<ExpressionKey, Vec<_>>::new();
    for (index, obligation) in body_types.obligations().iter().enumerate() {
        obligations
            .entry(obligation.site())
            .or_default()
            .push(index);
    }
    let mut invocation_dispatches = BTreeMap::new();
    let mut callable_dispatches = BTreeMap::new();
    for (index, dispatch) in body_types.method_dispatches().iter().enumerate() {
        let site = dispatch.site();
        let target = match site {
            MethodDispatchSite::Invocation(_) => &mut invocation_dispatches,
            MethodDispatchSite::CallableValue(_) => &mut callable_dispatches,
        };
        if target.insert(site.key(), index).is_some() {
            return Err(TraitEvidenceError::DuplicateMethodDispatch(site.key()).into());
        }
    }
    let execution::EvidenceState {
        invocations: checked,
        callable_values,
        environments,
        environment_ids,
        proofs,
        method_routes,
    } = state;
    for (key, invocation) in invocations {
        if let InvocationTarget::CallableValue { callee, ty } = invocation.target() {
            if obligations.contains_key(&key) || invocation_dispatches.contains_key(&key) {
                return Err(TraitEvidenceError::UnexpectedIndirectEvidence(key).into());
            }
            checked.insert(
                key,
                SemanticFact::Known(CheckedInvocation::Indirect {
                    callee: *callee,
                    ty: *ty,
                }),
            )?;
            continue;
        }
        let entries = match resolve_obligations(
            collection,
            &source_map,
            signatures,
            trait_environment,
            diagnostics,
            proofs,
            body_types,
            obligations.get(&key).map_or(&[][..], Vec::as_slice),
            work,
        )? {
            ObligationResolution::Known(entries) => entries,
            ObligationResolution::Poisoned(poison) => {
                checked.insert(key, SemanticFact::Poisoned(poison))?;
                continue;
            }
        };
        let dispatch = if let Some(&index) = invocation_dispatches.get(&key) {
            let pending = &body_types.method_dispatches()[index];
            let mut request_work = work.dispatch(index, pending)?;
            match resolve_method_dispatch(
                pending,
                collection,
                &source_map,
                signatures,
                trait_environment,
                diagnostics,
                proofs,
                &mut request_work,
            )? {
                Ok(dispatch) => dispatch,
                Err(poison) => {
                    checked.insert(key, SemanticFact::Poisoned(poison))?;
                    continue;
                }
            }
        } else {
            direct_dispatch(invocation, body_types, signatures)?
        };
        let environment =
            intern_environment(entries.into_boxed_slice(), environments, environment_ids)?;
        checked.insert(
            key,
            SemanticFact::Known(CheckedInvocation::Static {
                dispatch,
                evidence_environment: environment,
            }),
        )?;
    }

    for (key, instance) in body_types.callable_values() {
        let entries = match resolve_obligations(
            collection,
            &source_map,
            signatures,
            trait_environment,
            diagnostics,
            proofs,
            body_types,
            obligations.get(&key).map_or(&[][..], Vec::as_slice),
            work,
        )? {
            ObligationResolution::Known(entries) => entries,
            ObligationResolution::Poisoned(poison) => {
                callable_values.insert(key, SemanticFact::Poisoned(poison))?;
                continue;
            }
        };
        let dispatch = if let Some(&index) = callable_dispatches.get(&key) {
            let pending = &body_types.method_dispatches()[index];
            let mut request_work = work.dispatch(index, pending)?;
            match resolve_method_dispatch(
                pending,
                collection,
                &source_map,
                signatures,
                trait_environment,
                diagnostics,
                proofs,
                &mut request_work,
            )? {
                Ok(dispatch) => dispatch,
                Err(poison) => {
                    callable_values.insert(key, SemanticFact::Poisoned(poison))?;
                    continue;
                }
            }
        } else {
            CheckedDispatch::Direct(callable_declaration(instance, body_types, signatures)?)
        };
        let environment =
            intern_environment(entries.into_boxed_slice(), environments, environment_ids)?;
        callable_values.insert(
            key,
            SemanticFact::Known(CheckedCallableValue {
                dispatch,
                evidence_environment: environment,
            }),
        )?;
    }

    work.validate()?;
    *method_routes = build_method_routes(collection, signatures, trait_environment)?;
    Ok(execution::EvidenceTraversal::Performed)
}

enum ObligationResolution {
    Known(Vec<EvidenceEntry>),
    Poisoned(Poison),
}

#[allow(
    clippy::too_many_arguments,
    reason = "trait proof resolution keeps distinct semantic owners explicit"
)]
fn resolve_obligations(
    collection: &DeclarationCollection,
    source_map: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &mut DeclarationSignatures,
    trait_environment: &mut TraitEnvironment,
    diagnostics: &mut DiagnosticStore,
    proofs: &mut ProofArena,
    body_types: &BodyTypes,
    pending: &[usize],
    work: &mut work::EvidenceInputs<'_>,
) -> Result<ObligationResolution, execution::EvidenceFlow> {
    let mut entries = Vec::new();
    for &index in pending {
        let pending = &body_types.obligations()[index];
        let mut request_work = work.bound(index, pending, signatures)?;
        request_work.checkpoint()?;
        let obligation = TraitObligation::new(pending.subject(), pending.trait_ref().clone());
        match trait_environment.prove_for_owner(pending.owner(), signatures, obligation.clone())? {
            selected @ (TraitProof::Bound(_) | TraitProof::Implementation(_)) => {
                let proof = proofs.materialize(
                    pending.owner(),
                    obligation,
                    selected,
                    signatures,
                    trait_environment,
                    &mut request_work,
                )?;
                entries.push(EvidenceEntry {
                    parameter: pending.parameter().clone(),
                    bound_index: pending.bound_index(),
                    subject: pending.subject(),
                    trait_ref: pending.trait_ref().clone(),
                    proof,
                    destination: pending.destination(),
                });
            }
            TraitProof::Poisoned(poison) => return Ok(ObligationResolution::Poisoned(poison)),
            TraitProof::InvalidArgument(violation) => {
                let poison = record_invalid_impl_argument(
                    source_map,
                    pending.location(),
                    violation,
                    diagnostics,
                )?;
                return Ok(ObligationResolution::Poisoned(poison));
            }
            TraitProof::Unsatisfied(failed) | TraitProof::Ambiguous(failed) => {
                let poison = record_unsatisfied_bound(
                    collection,
                    source_map,
                    signatures,
                    pending.owner(),
                    pending.location(),
                    &failed,
                    diagnostics,
                )?;
                return Ok(ObligationResolution::Poisoned(poison));
            }
        }
    }
    Ok(ObligationResolution::Known(entries))
}

fn checked_impl_dispatch(
    trait_environment: &TraitEnvironment,
    pending: &PendingMethodDispatch,
    evidence: TraitImplEvidence,
    proof: EvidenceProofId,
) -> Result<CheckedDispatch, TraitEvidenceError> {
    let callable = trait_environment
        .checked_impl_method(evidence.implementation(), pending.method())
        .cloned()
        .ok_or_else(|| TraitEvidenceError::MissingCheckedImplMethod {
            implementation: evidence.implementation().clone(),
            method: pending.method().clone(),
        })?;
    Ok(CheckedDispatch::ImplTemplate {
        callable,
        method: pending.method().clone(),
        proof,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "trait proof construction retains the semantic and diagnostic owners"
)]
fn resolve_method_dispatch(
    pending: &PendingMethodDispatch,
    collection: &DeclarationCollection,
    source_map: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &mut DeclarationSignatures,
    trait_environment: &mut TraitEnvironment,
    diagnostics: &mut DiagnosticStore,
    proofs: &mut ProofArena,
    work: &mut work::ProofWork<'_>,
) -> Result<Result<CheckedDispatch, Poison>, execution::EvidenceFlow> {
    work.checkpoint()?;
    let obligation = TraitObligation::new(pending.subject(), pending.trait_ref().clone());
    Ok(
        match trait_environment.prove_for_owner(pending.owner(), signatures, obligation.clone())? {
            TraitProof::Bound(witness) => Ok(CheckedDispatch::Bound {
                witness,
                method: pending.method().clone(),
            }),
            TraitProof::Implementation(evidence) => {
                let proof = proofs.materialize(
                    pending.owner(),
                    obligation,
                    TraitProof::Implementation(evidence.clone()),
                    signatures,
                    trait_environment,
                    work,
                )?;
                Ok(checked_impl_dispatch(
                    trait_environment,
                    pending,
                    evidence,
                    proof,
                )?)
            }
            TraitProof::Poisoned(poison) => Err(poison),
            TraitProof::InvalidArgument(violation) => Err(record_invalid_impl_argument(
                source_map,
                pending.location(),
                violation,
                diagnostics,
            )?),
            TraitProof::Unsatisfied(failed) | TraitProof::Ambiguous(failed) => {
                Err(record_unsatisfied_bound(
                    collection,
                    source_map,
                    signatures,
                    pending.owner(),
                    pending.location(),
                    &failed,
                    diagnostics,
                )?)
            }
        },
    )
}

fn intern_environment(
    entries: Box<[EvidenceEntry]>,
    environments: &mut Vec<EvidenceEnvironment>,
    ids: &mut BTreeMap<Box<[EvidenceEntry]>, EvidenceEnvironmentId>,
) -> Result<EvidenceEnvironmentId, IdentityOverflow> {
    if let Some(&id) = ids.get(&entries) {
        return Ok(id);
    }
    let id = EvidenceEnvironmentId::from_index(environments.len())?;

    RecordReservation::for_append(environments).reserve(environments);

    ids.insert(entries.clone(), id);
    environments.push(EvidenceEnvironment { id, entries });
    Ok(id)
}

fn direct_dispatch(
    invocation: &InvocationFact,
    body_types: &BodyTypes,
    signatures: &DeclarationSignatures,
) -> Result<CheckedDispatch, TraitEvidenceError> {
    let declaration = match invocation.target() {
        InvocationTarget::Callable(instance) => {
            callable_declaration(*instance, body_types, signatures)?
        }
        InvocationTarget::CallableValue { .. } => {
            unreachable!("indirect calls have no static dispatch")
        }
        InvocationTarget::Constructor { declaration, .. }
        | InvocationTarget::TensorConstructor { declaration, .. } => declaration.clone(),
    };
    Ok(CheckedDispatch::Direct(declaration))
}

fn callable_declaration(
    instance: CallableInstanceId,
    body_types: &BodyTypes,
    signatures: &DeclarationSignatures,
) -> Result<CanonicalTarget, TraitEvidenceError> {
    let instance = body_types
        .callable_instance(instance)
        .ok_or(TraitEvidenceError::UnknownCallableInstance(instance))?;
    signatures
        .callable(instance.signature())
        .map(|callable| callable.declaration().clone())
        .ok_or(TraitEvidenceError::UnknownCallableSignature(
            instance.signature(),
        ))
}

fn invocation_location(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    key: ExpressionKey,
) -> Result<SemanticLocation, TraitEvidenceError> {
    let module = collection
        .module(key.module())
        .ok_or(TraitEvidenceError::UnknownModule(key.module()))?;
    let source = sources
        .get(&module.file())
        .copied()
        .ok_or(TraitEvidenceError::MissingSource(module.file()))?;
    let origin = source
        .hir()
        .and_then(|hir| {
            hir.body(key.expression().body())
                .expression(key.expression())
        })
        .map(eglur_hir::Expr::origin)
        .ok_or(TraitEvidenceError::WrongBodyExpression(key))?;
    Ok(SemanticLocation::origin(source.file(), origin))
}

fn record_unsatisfied_bound(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &DeclarationSignatures,
    owner: DefId,
    location: SemanticLocation,
    obligation: &TraitObligation,
    diagnostics: &mut DiagnosticStore,
) -> Result<Poison, TraitEvidenceError> {
    let source = sources
        .get(&location.file())
        .copied()
        .ok_or(TraitEvidenceError::MissingSource(location.file()))?;
    let origin = location
        .hir_origin()
        .ok_or(TraitEvidenceError::MissingOrigin(location.file()))?;
    let span =
        source
            .origin_span(origin)
            .cloned()
            .ok_or(TraitEvidenceError::MissingOriginSpan {
                file: location.file(),
                origin,
            })?;
    let renderer = DiagnosticTypeRenderer::new(
        collection,
        signatures.diagnostic_type_names(),
        signatures,
        owner,
    );
    let data = BTreeMap::from([
        (
            Box::<str>::from("trait"),
            DiagnosticDataValue::from(renderer.render_trait_ref(obligation.trait_ref())?),
        ),
        (
            Box::<str>::from("type"),
            DiagnosticDataValue::from(renderer.render_type(obligation.subject())?),
        ),
    ]);
    Ok(record_type_error(
        diagnostics,
        "TRAIT_BOUND_NOT_SATISFIED",
        "trait bound is not satisfied",
        span,
        data,
    )?)
}

fn record_invalid_impl_argument(
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    location: SemanticLocation,
    violation: crate::ImplArgumentViolation,
    diagnostics: &mut DiagnosticStore,
) -> Result<Poison, TraitEvidenceError> {
    let source = sources
        .get(&location.file())
        .copied()
        .ok_or(TraitEvidenceError::MissingSource(location.file()))?;
    let origin = location
        .hir_origin()
        .ok_or(TraitEvidenceError::MissingOrigin(location.file()))?;
    let span =
        source
            .origin_span(origin)
            .cloned()
            .ok_or(TraitEvidenceError::MissingOriginSpan {
                file: location.file(),
                origin,
            })?;
    let (code, message) = crate::diagnostic::impl_argument_diagnostic(violation);
    Ok(record_type_error(
        diagnostics,
        code,
        message,
        span,
        BTreeMap::new(),
    )?)
}

/// Trait-evidence infrastructure failure; source failures are diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitEvidenceError {
    AnalysisRootClaim(crate::AnalysisRootClaimError),
    InvalidProofCustody(&'static str),
    MissingAnalysisStop,
    UnexpectedUnmeteredInterruption,
    UnknownModule(crate::ModuleId),
    MissingSource(FileId),
    MissingOrigin(FileId),
    MissingOriginSpan {
        file: FileId,
        origin: eglur_hir::OriginId,
    },
    WrongBodyExpression(ExpressionKey),
    UnknownCallableInstance(CallableInstanceId),
    UnknownCallableSignature(crate::CallableSignatureId),
    DuplicateMethodDispatch(ExpressionKey),
    UnexpectedIndirectEvidence(ExpressionKey),
    UnexpectedCallableValueEvidence(ExpressionKey),
    MissingCheckedImplMethod {
        implementation: CanonicalTarget,
        method: CanonicalTarget,
    },
    UnknownCheckedImplementation(CanonicalTarget),
    InvalidCheckedProof(&'static str),
    CyclicCheckedProof(TraitObligation),
    UnmaterializablePrerequisite(TraitObligation),
    IdentityOverflow(IdentityOverflow),
    DuplicateFact(DuplicateFact),
    TraitEnvironment(TraitEnvironmentError),
    TypeRendering(Box<str>),
    SemanticDiagnostic(Box<str>),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for TraitEvidenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AnalysisRootClaim(error) => {
                write!(formatter, "invalid analysis root claim: {error:?}")
            }
            Self::InvalidProofCustody(reason) => {
                write!(formatter, "invalid proof custody: {reason}")
            }
            Self::MissingAnalysisStop => {
                formatter.write_str("evidence interruption lacks an accounting stop")
            }
            Self::UnexpectedUnmeteredInterruption => {
                formatter.write_str("unmetered evidence execution interrupted")
            }
            Self::UnknownModule(module) => write!(formatter, "unknown module {module:?}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file:?}"),
            Self::MissingOrigin(file) => {
                write!(
                    formatter,
                    "trait evidence location in {file:?} lacks an origin"
                )
            }
            Self::MissingOriginSpan { file, origin } => {
                write!(formatter, "missing span for origin {origin:?} in {file:?}")
            }
            Self::WrongBodyExpression(key) => {
                write!(formatter, "unknown invocation expression {key:?}")
            }
            Self::UnknownCallableInstance(instance) => {
                write!(formatter, "unknown callable instance {instance:?}")
            }
            Self::UnknownCallableSignature(signature) => {
                write!(formatter, "unknown callable signature {signature:?}")
            }
            Self::DuplicateMethodDispatch(invocation) => {
                write!(
                    formatter,
                    "invocation {invocation:?} has duplicate method dispatch"
                )
            }
            Self::UnexpectedIndirectEvidence(key) => {
                write!(formatter, "indirect call {key:?} has static trait evidence")
            }
            Self::UnexpectedCallableValueEvidence(key) => {
                write!(
                    formatter,
                    "callable value {key:?} has unchecked trait evidence"
                )
            }
            Self::MissingCheckedImplMethod {
                implementation,
                method,
            } => write!(
                formatter,
                "checked implementation {implementation:?} lacks required method {method:?}"
            ),
            Self::UnknownCheckedImplementation(implementation) => {
                write!(
                    formatter,
                    "unknown checked implementation {implementation:?}"
                )
            }
            Self::InvalidCheckedProof(reason) => {
                write!(formatter, "invalid checked proof: {reason}")
            }
            Self::CyclicCheckedProof(obligation) => {
                write!(formatter, "cyclic checked proof for {obligation:?}")
            }
            Self::UnmaterializablePrerequisite(obligation) => {
                write!(formatter, "unmaterializable prerequisite {obligation:?}")
            }
            Self::IdentityOverflow(error) => error.fmt(formatter),
            Self::DuplicateFact(error) => error.fmt(formatter),
            Self::TraitEnvironment(error) => error.fmt(formatter),
            Self::TypeRendering(error) => error.fmt(formatter),
            Self::SemanticDiagnostic(error) => {
                write!(formatter, "semantic diagnostic error: {error}")
            }
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TraitEvidenceError {}

impl From<IdentityOverflow> for TraitEvidenceError {
    fn from(error: IdentityOverflow) -> Self {
        Self::IdentityOverflow(error)
    }
}

impl From<DuplicateFact> for TraitEvidenceError {
    fn from(error: DuplicateFact) -> Self {
        Self::DuplicateFact(error)
    }
}

impl From<TraitEnvironmentError> for TraitEvidenceError {
    fn from(error: TraitEnvironmentError) -> Self {
        Self::TraitEnvironment(error)
    }
}

impl From<DiagnosticTypeRenderingError> for TraitEvidenceError {
    fn from(error: DiagnosticTypeRenderingError) -> Self {
        Self::TypeRendering(error.to_string().into_boxed_str())
    }
}

impl From<SemanticDiagnosticError> for TraitEvidenceError {
    fn from(error: SemanticDiagnosticError) -> Self {
        Self::SemanticDiagnostic(format!("{error:?}").into_boxed_str())
    }
}

impl From<DiagnosticStoreError> for TraitEvidenceError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}
