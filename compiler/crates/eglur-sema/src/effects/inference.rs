use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_diagnostics::{DiagnosticStore, Poison};

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    TypecheckCapability,
};
use crate::{
    BodyTypes, CanonicalTarget, DeclarationCollection, DeclarationKind, DeclarationSignature,
    DeclarationSignatures, DefId, DuplicateFact, FactState, FactTable, NameResolution,
    SemanticFact, SemanticLocation, SemanticSource, TraitEvidence,
};

use super::authorization::authorize_effects;
use super::body::{
    EffectBody, EffectBodyError, EffectBodyFact, EffectDispatch, EffectOperation, MutablePath,
    MutableRoot, collect_effect_bodies,
};
use super::{
    EffectCategory, EffectSummary, KnownEffectSummary, StateScope, TrustedEffectEnvironment,
};
use crate::declaration_graph::{DeclarationGraphError, dependency_first_sccs};

// A strict chain can add IO, NONDET, localizable STATE, nonlocal STATE, then Unknown.
const EFFECT_LATTICE_HEIGHT: usize = 6;

/// One source-callable effect SCC in deterministic callee-first order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectScc {
    members: Box<[DefId]>,
}

impl EffectScc {
    /// Returns source callable identities in canonical declaration order.
    #[must_use]
    pub fn members(&self) -> &[DefId] {
        &self.members
    }
}

/// Canonical callable summaries and the source call-graph schedule that established them.
pub struct EffectFacts {
    summaries: FactTable<CanonicalTarget, EffectSummary>,
    sccs: Box<[EffectScc]>,
}

impl EffectFacts {
    #[must_use]
    pub fn summary(&self, target: &CanonicalTarget) -> FactState<'_, EffectSummary> {
        self.summaries.state(target)
    }

    #[must_use]
    pub fn source_summary(&self, declaration: DefId) -> FactState<'_, EffectSummary> {
        self.summary(&CanonicalTarget::Source(declaration))
    }

    pub fn summaries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&CanonicalTarget, FactState<'_, EffectSummary>)> {
        self.summaries
            .iter()
            .map(|(target, _)| (target, self.summaries.state(target)))
    }

    #[must_use]
    pub fn sccs(&self) -> &[EffectScc] {
        &self.sccs
    }

    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.summaries.iter().len()
    }

    pub(crate) fn has_poisoned_facts(&self) -> bool {
        self.summaries.has_poisoned()
    }
}

/// Computes bounded callable summaries without repeating name, type, or trait resolution.
#[allow(
    clippy::too_many_arguments,
    reason = "the phase boundary keeps independently owned semantic products explicit"
)]
pub fn analyze_effects(
    collection: &DeclarationCollection,
    names: &NameResolution,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    signatures: &DeclarationSignatures,
    body_types: &BodyTypes,
    trait_evidence: &TraitEvidence,
    trusted: Option<&TrustedEffectEnvironment>,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<EffectFacts>, EffectAnalysisError> {
    let mut recorder = DemandRecorder::new(support);
    let capability = CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries);
    let mut supported = true;
    for module in collection.modules() {
        for declaration in module.declarations().filter(|declaration| {
            matches!(
                declaration.kind(),
                DeclarationKind::Function | DeclarationKind::ExternFunction
            )
        }) {
            supported &= recorder.require(
                capability,
                SemanticLocation::origin(module.file(), declaration.origin()),
            );
        }
    }
    if !supported {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }

    let bodies = collect_effect_bodies(
        collection,
        names,
        sources,
        signatures,
        body_types,
        trait_evidence,
    )?;
    let facts = infer_effects(collection, signatures, body_types, trusted, &bodies)?;
    authorize_effects(
        collection,
        sources,
        signatures,
        &bodies,
        &facts,
        diagnostics,
    )?;
    Ok(recorder.finish(PhaseProduct::Performed(facts)))
}

fn infer_effects(
    collection: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    body_types: &BodyTypes,
    trusted: Option<&TrustedEffectEnvironment>,
    body_facts: &[EffectBodyFact],
) -> Result<EffectFacts, EffectAnalysisError> {
    let body_fact_by_owner = body_facts
        .iter()
        .map(|body| (body.owner(), body))
        .collect::<BTreeMap<_, _>>();
    if body_fact_by_owner.len() != body_facts.len() {
        return Err(EffectAnalysisError::DuplicateEffectBody);
    }
    let body_by_owner = body_facts
        .iter()
        .filter_map(|fact| fact.known().map(|body| (body.owner(), body)))
        .collect::<BTreeMap<_, _>>();
    let body_nodes = body_by_owner.keys().copied().collect::<BTreeSet<_>>();
    let mut solved = BTreeMap::<CanonicalTarget, SolvedFact>::new();
    for (_, invocation) in body_types.invocations() {
        if let crate::InvocationTarget::TensorConstructor {
            declaration: CanonicalTarget::External(declaration),
            ..
        } = invocation.target()
        {
            let summary = trusted
                .and_then(|environment| environment.summary(declaration))
                .cloned()
                .unwrap_or(EffectSummary::Unknown);
            solved.insert(
                CanonicalTarget::External(declaration.clone()),
                SolvedFact::Known(summary),
            );
        }
    }

    let declaration_targets = signatures
        .declarations()
        .map(|(target, _)| target.clone())
        .collect::<Vec<_>>();
    for target in declaration_targets {
        match signatures.declaration(&target) {
            FactState::Known(DeclarationSignature::Callable(signature_id)) => {
                let signature = signatures
                    .callable(*signature_id)
                    .ok_or(EffectAnalysisError::UnknownCallableSignature(*signature_id))?;
                match target.clone() {
                    CanonicalTarget::External(declaration) => {
                        let summary = trusted
                            .and_then(|environment| environment.summary(&declaration))
                            .cloned()
                            .unwrap_or(EffectSummary::Unknown);
                        solved.insert(target, SolvedFact::Known(summary));
                    }
                    CanonicalTarget::Source(declaration) => {
                        let source = collection
                            .declaration(declaration)
                            .ok_or(EffectAnalysisError::UnknownSourceDeclaration(declaration))?;
                        if source.kind() == DeclarationKind::ExternFunction {
                            solved.insert(target, SolvedFact::Known(EffectSummary::Unknown));
                        } else {
                            match body_types.body(declaration) {
                                FactState::Known(_) => {
                                    match body_fact_by_owner.get(&declaration).copied() {
                                        Some(EffectBodyFact::Known(_)) => {}
                                        Some(EffectBodyFact::Poisoned { poison, .. }) => {
                                            solved.insert(
                                                target,
                                                SolvedFact::Poisoned(poison.clone()),
                                            );
                                        }
                                        Some(EffectBodyFact::Unperformed { .. }) => {}
                                        None => {
                                            return Err(EffectAnalysisError::MissingEffectBody(
                                                declaration,
                                            ));
                                        }
                                    }
                                }
                                FactState::Poisoned(poison) => {
                                    solved.insert(target, SolvedFact::Poisoned(poison.clone()));
                                }
                                FactState::Unperformed => {}
                                FactState::NotApplicable => {
                                    let summary = if is_trait_requirement(collection, declaration)
                                        && !signature.is_bang()
                                    {
                                        EffectSummary::pure()
                                    } else {
                                        EffectSummary::Unknown
                                    };
                                    solved.insert(target, SolvedFact::Known(summary));
                                }
                            }
                        }
                    }
                }
            }
            FactState::Poisoned(poison) => {
                if let CanonicalTarget::Source(declaration) = target
                    && collection
                        .declaration(declaration)
                        .is_some_and(|declaration| {
                            matches!(
                                declaration.kind(),
                                DeclarationKind::Function | DeclarationKind::ExternFunction
                            )
                        })
                {
                    solved.insert(
                        CanonicalTarget::Source(declaration),
                        SolvedFact::Poisoned(poison.clone()),
                    );
                }
            }
            FactState::Known(_) | FactState::Unperformed | FactState::NotApplicable => {}
        }
    }

    let dependencies = body_by_owner.values().flat_map(|body| {
        body.operations().iter().filter_map(|operation| {
            let EffectOperation::Call(call) = operation else {
                return None;
            };
            let EffectDispatch::Target(CanonicalTarget::Source(callee)) = call.dispatch() else {
                return None;
            };
            body_nodes
                .contains(callee)
                .then_some((body.owner(), *callee))
        })
    });
    let components = dependency_first_sccs(body_nodes.iter().copied(), dependencies)?;
    for members in &components {
        solve_component(members, &body_by_owner, &mut solved)?;
    }

    let mut summaries = FactTable::new();
    for (target, fact) in solved {
        summaries.insert(target, fact.into_semantic())?;
    }
    let sccs = components
        .into_vec()
        .into_iter()
        .map(|members| EffectScc { members })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(EffectFacts { summaries, sccs })
}

fn is_trait_requirement(collection: &DeclarationCollection, declaration: DefId) -> bool {
    collection
        .declaration(declaration)
        .and_then(|declaration| declaration.parent())
        .and_then(|parent| collection.declaration(parent))
        .is_some_and(|parent| parent.kind() == DeclarationKind::Trait)
}

fn solve_component(
    members: &[DefId],
    bodies: &BTreeMap<DefId, &EffectBody>,
    solved: &mut BTreeMap<CanonicalTarget, SolvedFact>,
) -> Result<(), EffectAnalysisError> {
    let member_set = members.iter().copied().collect::<BTreeSet<_>>();
    let mut dependents = members
        .iter()
        .copied()
        .map(|member| (member, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for &caller in members {
        let body = bodies
            .get(&caller)
            .copied()
            .ok_or(EffectAnalysisError::MissingEffectBody(caller))?;
        for operation in body.operations() {
            let EffectOperation::Call(call) = operation else {
                continue;
            };
            let EffectDispatch::Target(CanonicalTarget::Source(callee)) = call.dispatch() else {
                continue;
            };
            if member_set.contains(callee) {
                dependents
                    .get_mut(callee)
                    .expect("every effect SCC member has a dependent set")
                    .insert(caller);
            }
        }
    }
    let mut current = members
        .iter()
        .copied()
        .map(|member| (member, EffectSummary::pure()))
        .collect::<BTreeMap<_, _>>();
    let mut advances = members
        .iter()
        .copied()
        .map(|member| (member, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut pending = member_set.clone();

    while let Some(&member) = pending.first() {
        pending.remove(&member);
        let body = bodies
            .get(&member)
            .copied()
            .ok_or(EffectAnalysisError::MissingEffectBody(member))?;
        let summary = match summarize_body(body, &member_set, &current, solved)? {
            SummaryOutcome::Known(summary) => summary,
            SummaryOutcome::Poisoned(poison) => {
                for &member in members {
                    solved.insert(
                        CanonicalTarget::Source(member),
                        SolvedFact::Poisoned(poison.clone()),
                    );
                }
                return Ok(());
            }
            SummaryOutcome::Unperformed => return Ok(()),
        };
        let previous = current
            .get(&member)
            .expect("every effect SCC member has an initial summary");
        let next = previous.join(&summary);
        if next == *previous {
            continue;
        }

        current.insert(member, next);
        let advance_count = advances
            .get_mut(&member)
            .expect("every effect SCC member has an advance count");
        *advance_count += 1;
        if *advance_count >= EFFECT_LATTICE_HEIGHT {
            return Err(EffectAnalysisError::FixedPointDidNotConverge(
                members.to_vec().into_boxed_slice(),
            ));
        }
        pending.extend(
            dependents
                .get(&member)
                .expect("every effect SCC member has a dependent set")
                .iter()
                .copied(),
        );
    }

    for (member, summary) in current {
        solved.insert(CanonicalTarget::Source(member), SolvedFact::Known(summary));
    }
    Ok(())
}

fn summarize_body(
    body: &EffectBody,
    component: &BTreeSet<DefId>,
    current: &BTreeMap<DefId, EffectSummary>,
    solved: &BTreeMap<CanonicalTarget, SolvedFact>,
) -> Result<SummaryOutcome, EffectAnalysisError> {
    let mut summary = EffectSummary::pure();
    for operation in body.operations() {
        let contribution = match operation {
            EffectOperation::Mutation(mutation) => mutation_summary(mutation.path()),
            EffectOperation::Call(call) => {
                let callee = match call.dispatch() {
                    EffectDispatch::Constructor => SummaryOutcome::Known(EffectSummary::pure()),
                    EffectDispatch::BoundRequirement { .. } if call.is_bang() => {
                        SummaryOutcome::Known(EffectSummary::Unknown)
                    }
                    EffectDispatch::BoundRequirement { .. } => {
                        SummaryOutcome::Known(EffectSummary::pure())
                    }
                    EffectDispatch::Target(CanonicalTarget::Source(declaration))
                        if component.contains(declaration) =>
                    {
                        SummaryOutcome::Known(
                            current
                                .get(declaration)
                                .cloned()
                                .ok_or(EffectAnalysisError::MissingCurrentSummary(*declaration))?,
                        )
                    }
                    EffectDispatch::Target(target) => solved
                        .get(target)
                        .map(SolvedFact::as_outcome)
                        .unwrap_or(SummaryOutcome::Unperformed),
                };
                match callee {
                    SummaryOutcome::Known(summary) => {
                        SummaryOutcome::Known(transfer_call(&summary, call.mutable_paths()))
                    }
                    blocked => blocked,
                }
            }
        };
        match contribution {
            SummaryOutcome::Known(contribution) => summary = summary.join(&contribution),
            SummaryOutcome::Poisoned(poison) => return Ok(SummaryOutcome::Poisoned(poison)),
            SummaryOutcome::Unperformed => return Ok(SummaryOutcome::Unperformed),
        }
    }
    Ok(SummaryOutcome::Known(summary))
}

fn mutation_summary(path: &MutablePath) -> SummaryOutcome {
    let scope = classify_paths(std::slice::from_ref(path));
    SummaryOutcome::Known(match scope {
        PathScope::Local => EffectSummary::pure(),
        PathScope::Boundary => state_summary(StateScope::Localizable),
        PathScope::Nonlocal | PathScope::Missing => state_summary(StateScope::Nonlocal),
    })
}

pub(super) fn transfer_call(summary: &EffectSummary, paths: &[MutablePath]) -> EffectSummary {
    let EffectSummary::Known(known) = summary else {
        return EffectSummary::Unknown;
    };
    if !known.contains(EffectCategory::State) || known.state_scope() == StateScope::Nonlocal {
        return summary.clone();
    }
    match classify_paths(paths) {
        PathScope::Local => EffectSummary::Known(known.without_state()),
        PathScope::Boundary => EffectSummary::Known(
            KnownEffectSummary::new(known.categories().iter().copied(), StateScope::Localizable)
                .expect("retaining STATE with localizable scope preserves summary validity"),
        ),
        PathScope::Nonlocal => EffectSummary::Known(
            KnownEffectSummary::new(known.categories().iter().copied(), StateScope::Nonlocal)
                .expect("retaining STATE with nonlocal scope preserves summary validity"),
        ),
        PathScope::Missing => EffectSummary::Unknown,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PathScope {
    Local,
    Boundary,
    Nonlocal,
    Missing,
}

pub(super) fn classify_paths(paths: &[MutablePath]) -> PathScope {
    if paths.is_empty() || paths.iter().any(|path| path.roots().is_empty()) {
        return PathScope::Missing;
    }
    let mut has_boundary = false;
    for root in paths.iter().flat_map(MutablePath::roots) {
        match root {
            MutableRoot::Local(_) => {}
            MutableRoot::Boundary(_) => has_boundary = true,
            MutableRoot::Nonlocal | MutableRoot::Unknown => return PathScope::Nonlocal,
        }
    }
    if has_boundary {
        PathScope::Boundary
    } else {
        PathScope::Local
    }
}

fn state_summary(scope: StateScope) -> EffectSummary {
    EffectSummary::Known(
        KnownEffectSummary::new([EffectCategory::State], scope)
            .expect("STATE summaries require a non-none state scope"),
    )
}

enum SummaryOutcome {
    Known(EffectSummary),
    Poisoned(Poison),
    Unperformed,
}

enum SolvedFact {
    Known(EffectSummary),
    Poisoned(Poison),
}

impl SolvedFact {
    fn as_outcome(&self) -> SummaryOutcome {
        match self {
            Self::Known(summary) => SummaryOutcome::Known(summary.clone()),
            Self::Poisoned(poison) => SummaryOutcome::Poisoned(poison.clone()),
        }
    }

    fn into_semantic(self) -> SemanticFact<EffectSummary> {
        match self {
            Self::Known(summary) => SemanticFact::Known(summary),
            Self::Poisoned(poison) => SemanticFact::Poisoned(poison),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectAnalysisError {
    BodyInvariant(Box<str>),
    GraphInvariant(Box<str>),
    DuplicateEffectBody,
    DuplicateFact,
    UnknownSourceDeclaration(DefId),
    UnknownCallableSignature(crate::CallableSignatureId),
    MissingEffectBody(DefId),
    MissingCurrentSummary(DefId),
    FixedPointDidNotConverge(Box<[DefId]>),
    AuthorizationInvariant(Box<str>),
}

impl fmt::Display for EffectAnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyInvariant(message) | Self::GraphInvariant(message) => {
                formatter.write_str(message)
            }
            Self::DuplicateEffectBody => {
                formatter.write_str("effect body collection repeated a callable identity")
            }
            Self::DuplicateFact => formatter.write_str("effect summary fact was assigned twice"),
            Self::UnknownSourceDeclaration(declaration) => {
                write!(formatter, "unknown source callable {declaration:?}")
            }
            Self::UnknownCallableSignature(signature) => {
                write!(formatter, "unknown callable signature {signature:?}")
            }
            Self::MissingEffectBody(declaration) => {
                write!(
                    formatter,
                    "typed callable {declaration:?} has no effect body"
                )
            }
            Self::MissingCurrentSummary(declaration) => {
                write!(
                    formatter,
                    "effect SCC member {declaration:?} has no current summary"
                )
            }
            Self::FixedPointDidNotConverge(members) => write!(
                formatter,
                "effect summary fixed point exceeded its finite bound for {members:?}"
            ),
            Self::AuthorizationInvariant(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for EffectAnalysisError {}

impl From<EffectBodyError> for EffectAnalysisError {
    fn from(error: EffectBodyError) -> Self {
        Self::BodyInvariant(error.to_string().into_boxed_str())
    }
}

impl From<DeclarationGraphError> for EffectAnalysisError {
    fn from(error: DeclarationGraphError) -> Self {
        Self::GraphInvariant(error.to_string().into_boxed_str())
    }
}

impl From<DuplicateFact> for EffectAnalysisError {
    fn from(_: DuplicateFact) -> Self {
        Self::DuplicateFact
    }
}

impl From<super::authorization::EffectAuthorizationError> for EffectAnalysisError {
    fn from(error: super::authorization::EffectAuthorizationError) -> Self {
        Self::AuthorizationInvariant(error.to_string().into_boxed_str())
    }
}
