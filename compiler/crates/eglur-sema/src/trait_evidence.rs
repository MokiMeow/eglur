use std::collections::BTreeMap;
use std::fmt;

use eglur_diagnostics::{DiagnosticDataValue, DiagnosticStore, DiagnosticStoreError, Poison};
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    TypecheckCapability,
};
use crate::diagnostic::{SemanticDiagnosticError, record_type_error};
use crate::type_rendering::{DiagnosticTypeRenderer, DiagnosticTypeRenderingError};
use crate::{
    BodyTypes, CallableInstanceId, CanonicalTarget, DeclarationCollection, DeclarationSignatures,
    DefId, DuplicateFact, EvidenceEnvironmentId, ExpressionKey, FactState, FactTable,
    GenericParameterId, IdentityOverflow, InvocationFact, InvocationTarget, PendingMethodDispatch,
    SemanticFact, SemanticLocation, SemanticSource, SubstId, TraitEnvironment,
    TraitEnvironmentError, TraitImplEvidence, TraitObligation, TraitProof, TraitRef, TypeId,
    WitnessSlotId,
};

/// One checked source for a first-order nominal trait obligation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum EvidenceSource {
    Bound(WitnessSlotId),
    Implementation {
        implementation: CanonicalTarget,
        substitution: SubstId,
    },
}

/// One generic parameter bound and the evidence selected for it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct EvidenceEntry {
    parameter: GenericParameterId,
    subject: TypeId,
    trait_ref: TraitRef,
    source: EvidenceSource,
    destination: Option<WitnessSlotId>,
}

impl EvidenceEntry {
    #[must_use]
    pub const fn parameter(&self) -> &GenericParameterId {
        &self.parameter
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
    pub const fn source(&self) -> &EvidenceSource {
        &self.source
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
        implementation: CanonicalTarget,
        callable: CanonicalTarget,
        substitution: SubstId,
    },
}

/// One invocation paired with its checked dispatch and evidence vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedInvocation {
    dispatch: CheckedDispatch,
    evidence_environment: EvidenceEnvironmentId,
}

impl CheckedInvocation {
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
    environments: Box<[EvidenceEnvironment]>,
}

impl TraitEvidence {
    #[must_use]
    pub fn invocation(&self, key: ExpressionKey) -> FactState<'_, CheckedInvocation> {
        self.invocations.state(&key)
    }

    #[must_use]
    pub fn environment(&self, id: EvidenceEnvironmentId) -> Option<&EvidenceEnvironment> {
        self.environments.get(id.index())
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
            + self
                .environments
                .iter()
                .map(|environment| 1 + environment.entries.len())
                .sum::<usize>()
    }

    pub(crate) fn is_complete_for(&self, body_types: &BodyTypes) -> bool {
        self.invocations.iter().len() == body_types.invocations().len()
            && self.invocations.all_known()
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
    let source_map = sources
        .iter()
        .map(|source| (source.file(), source))
        .collect::<BTreeMap<_, _>>();
    let invocations = body_types.invocations().collect::<Vec<_>>();
    let mut recorder = DemandRecorder::new(support);
    let capability = CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence);
    let mut supported = true;
    for (key, _) in &invocations {
        supported &= recorder.require(
            capability,
            invocation_location(collection, &source_map, *key)?,
        );
    }
    if !supported {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }

    let mut obligations = BTreeMap::<ExpressionKey, Vec<_>>::new();
    for obligation in body_types.obligations() {
        obligations
            .entry(obligation.invocation())
            .or_default()
            .push(obligation);
    }
    let mut method_dispatches = BTreeMap::new();
    for dispatch in body_types.method_dispatches() {
        if method_dispatches
            .insert(dispatch.invocation(), dispatch)
            .is_some()
        {
            return Err(TraitEvidenceError::DuplicateMethodDispatch(
                dispatch.invocation(),
            ));
        }
    }
    let mut checked = FactTable::new();
    let mut environments = Vec::new();
    let mut environment_ids = BTreeMap::<Box<[EvidenceEntry]>, EvidenceEnvironmentId>::new();
    for (key, invocation) in invocations {
        let mut entries = Vec::new();
        let mut poison = None;
        for pending in obligations.get(&key).map_or(&[][..], Vec::as_slice) {
            let obligation = TraitObligation::new(pending.subject(), pending.trait_ref().clone());
            match trait_environment.prove_for_owner(pending.owner(), signatures, obligation)? {
                TraitProof::Bound(witness) => entries.push(EvidenceEntry {
                    parameter: pending.parameter().clone(),
                    subject: pending.subject(),
                    trait_ref: pending.trait_ref().clone(),
                    source: EvidenceSource::Bound(witness),
                    destination: pending.destination(),
                }),
                TraitProof::Implementation(evidence) => entries.push(EvidenceEntry {
                    parameter: pending.parameter().clone(),
                    subject: pending.subject(),
                    trait_ref: pending.trait_ref().clone(),
                    source: implementation_source(evidence),
                    destination: pending.destination(),
                }),
                TraitProof::Poisoned(found) => {
                    poison = Some(found);
                    break;
                }
                TraitProof::Unsatisfied(failed) | TraitProof::Ambiguous(failed) => {
                    poison = Some(record_unsatisfied_bound(
                        collection,
                        &source_map,
                        signatures,
                        pending.owner(),
                        pending.location(),
                        &failed,
                        diagnostics,
                    )?);
                    break;
                }
            }
        }
        if let Some(poison) = poison {
            checked.insert(key, SemanticFact::Poisoned(poison))?;
            continue;
        }
        let dispatch = if let Some(pending) = method_dispatches.get(&key) {
            let obligation = TraitObligation::new(pending.subject(), pending.trait_ref().clone());
            match trait_environment.prove_for_owner(pending.owner(), signatures, obligation)? {
                TraitProof::Bound(witness) => CheckedDispatch::Bound {
                    witness,
                    method: pending.method().clone(),
                },
                TraitProof::Implementation(evidence) => {
                    checked_impl_dispatch(trait_environment, pending, evidence)?
                }
                TraitProof::Poisoned(found) => {
                    checked.insert(key, SemanticFact::Poisoned(found))?;
                    continue;
                }
                TraitProof::Unsatisfied(failed) | TraitProof::Ambiguous(failed) => {
                    let poison = record_unsatisfied_bound(
                        collection,
                        &source_map,
                        signatures,
                        pending.owner(),
                        pending.location(),
                        &failed,
                        diagnostics,
                    )?;
                    checked.insert(key, SemanticFact::Poisoned(poison))?;
                    continue;
                }
            }
        } else {
            direct_dispatch(invocation, body_types, signatures)?
        };
        let environment = intern_environment(
            entries.into_boxed_slice(),
            &mut environments,
            &mut environment_ids,
        )?;
        checked.insert(
            key,
            SemanticFact::Known(CheckedInvocation {
                dispatch,
                evidence_environment: environment,
            }),
        )?;
    }

    Ok(recorder.finish(PhaseProduct::Performed(TraitEvidence {
        invocations: checked,
        environments: environments.into_boxed_slice(),
    })))
}

fn checked_impl_dispatch(
    trait_environment: &TraitEnvironment,
    pending: &PendingMethodDispatch,
    evidence: TraitImplEvidence,
) -> Result<CheckedDispatch, TraitEvidenceError> {
    let callable = trait_environment
        .checked_impl_method(evidence.implementation(), pending.method())
        .cloned()
        .ok_or_else(|| TraitEvidenceError::MissingCheckedImplMethod {
            implementation: evidence.implementation().clone(),
            method: pending.method().clone(),
        })?;
    Ok(CheckedDispatch::ImplTemplate {
        implementation: evidence.implementation().clone(),
        callable,
        substitution: evidence.substitution(),
    })
}

fn implementation_source(evidence: TraitImplEvidence) -> EvidenceSource {
    EvidenceSource::Implementation {
        implementation: evidence.implementation().clone(),
        substitution: evidence.substitution(),
    }
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
        InvocationTarget::Constructor { declaration, .. } => declaration.clone(),
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

/// Trait-evidence infrastructure failure; source failures are diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TraitEvidenceError {
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
    MissingCheckedImplMethod {
        implementation: CanonicalTarget,
        method: CanonicalTarget,
    },
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
            Self::MissingCheckedImplMethod {
                implementation,
                method,
            } => write!(
                formatter,
                "checked implementation {implementation:?} lacks required method {method:?}"
            ),
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
