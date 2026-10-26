mod traversal;

use std::collections::{BTreeMap, BTreeSet};

use eglur_hir::{
    Body, ConstructorPayload, ExprId, ExprKind, OriginId, PatternId, PatternKind,
    ReduceBySingleValue, StmtId, StmtKind,
};

use crate::{
    BodyTypes, ControlRegionKey, ControlTransfer, ControlTransferSource, DeclarationSignatures,
    DefId, ExpressionKey, FactState, IfBranch, LocalBindingId, ModuleId, NameResolution,
    NormalContinuation, ReduceByCommitment, ResolutionKey, ResolvedName, StatementKey, TypeKind,
    TypedOwnershipOperationKind, TypedPlaceKind,
};

use super::analysis::OwnershipAnalysisError;
use super::{
    OwnershipAvailability, OwnershipBodyStatus, OwnershipBodySummary, OwnershipFailure,
    OwnershipFailureKind, OwnershipRootFact,
};

pub(crate) struct OwnershipBodyAnalysis {
    pub(crate) summary: OwnershipBodySummary,
    pub(crate) failures: Vec<OwnershipFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AvailabilityFact {
    state: OwnershipAvailability,
    first_consume: Option<OriginId>,
}

impl AvailabilityFact {
    const fn owned() -> Self {
        Self {
            state: OwnershipAvailability::Owned,
            first_consume: None,
        }
    }
}

#[derive(Clone, Default)]
struct FlowState {
    availability: BTreeMap<LocalBindingId, AvailabilityFact>,
    aliases: BTreeMap<LocalBindingId, ValueProvenance>,
}

#[derive(Clone, Debug, Default)]
struct ValueProvenance {
    roots_by_tuple_path: BTreeMap<Vec<usize>, BTreeSet<LocalBindingId>>,
}

impl ValueProvenance {
    fn direct(roots: BTreeSet<LocalBindingId>) -> Self {
        let mut provenance = Self::default();
        if !roots.is_empty() {
            provenance.roots_by_tuple_path.insert(Vec::new(), roots);
        }
        provenance
    }

    fn extend(&mut self, other: Self) {
        for (path, roots) in other.roots_by_tuple_path {
            self.roots_by_tuple_path
                .entry(path)
                .or_default()
                .extend(roots);
        }
    }

    fn prefixed(self, index: usize) -> Self {
        let roots_by_tuple_path = self
            .roots_by_tuple_path
            .into_iter()
            .map(|(path, roots)| {
                let mut prefixed = Vec::with_capacity(path.len() + 1);
                prefixed.push(index);
                prefixed.extend(path);
                (prefixed, roots)
            })
            .collect();
        Self {
            roots_by_tuple_path,
        }
    }

    fn projected(&self, index: usize) -> Self {
        let mut projected = Self::default();
        for (path, roots) in &self.roots_by_tuple_path {
            if path.first() == Some(&index) {
                projected
                    .roots_by_tuple_path
                    .entry(path[1..].to_vec())
                    .or_default()
                    .extend(roots);
            }
        }
        projected
    }

    fn roots(&self) -> BTreeSet<LocalBindingId> {
        self.roots_by_tuple_path
            .values()
            .flatten()
            .copied()
            .collect()
    }
}

impl FlowState {
    fn join(states: &[Self]) -> Self {
        debug_assert!(!states.is_empty());
        let availability_keys = states
            .iter()
            .flat_map(|state| state.availability.keys().copied())
            .collect::<BTreeSet<_>>();
        let mut availability = BTreeMap::new();
        for binding in availability_keys {
            let mut joined = AvailabilityFact::owned();
            let mut first = true;
            for state in states {
                let incoming = state
                    .availability
                    .get(&binding)
                    .copied()
                    .unwrap_or_else(AvailabilityFact::owned);
                if first {
                    joined = incoming;
                    first = false;
                } else {
                    joined.state = joined.state.join(incoming.state);
                    joined.first_consume = match (joined.first_consume, incoming.first_consume) {
                        (Some(left), Some(right)) => Some(left.min(right)),
                        (Some(origin), None) | (None, Some(origin)) => Some(origin),
                        (None, None) => None,
                    };
                }
            }
            availability.insert(binding, joined);
        }

        let alias_keys = states
            .iter()
            .flat_map(|state| state.aliases.keys().copied())
            .collect::<BTreeSet<_>>();
        let aliases = alias_keys
            .into_iter()
            .map(|binding| {
                let mut provenance = ValueProvenance::default();
                for incoming in states
                    .iter()
                    .filter_map(|state| state.aliases.get(&binding))
                {
                    provenance.extend(incoming.clone());
                }
                (binding, provenance)
            })
            .collect();
        Self {
            availability,
            aliases,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct ExpressionFlow {
    continuing: bool,
    provenance: ValueProvenance,
}

impl ExpressionFlow {
    fn continuing() -> Self {
        Self {
            continuing: true,
            provenance: ValueProvenance::default(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_body(
    owner: DefId,
    module: ModuleId,
    body: &Body,
    root: ExprId,
    parameters: BTreeSet<LocalBindingId>,
    names: &NameResolution,
    signatures: &DeclarationSignatures,
    body_types: &BodyTypes,
) -> Result<OwnershipBodyAnalysis, OwnershipAnalysisError> {
    let mut analyzer = BodyAnalyzer {
        owner,
        module,
        body,
        names,
        signatures,
        body_types,
        roots: BTreeMap::new(),
        failures: Vec::new(),
        operation_count: 0,
        unperformed: false,
        recoverable_exits: Vec::new(),
    };
    let mut state = FlowState::default();
    for parameter in parameters {
        analyzer.introduce_root(parameter, &mut state)?;
    }
    let flow =
        analyzer.visit_expression(root, &mut state, Some(ControlRegionKey::Callable(owner)))?;
    if !matches!(
        body.expression(root).map(eglur_hir::Expr::kind),
        Some(ExprKind::Block { .. })
    ) {
        analyzer.recover_missing_move(root, &mut state)?;
    }
    if flow.continuing
        && !matches!(
            body.expression(root).map(eglur_hir::Expr::kind),
            Some(ExprKind::Block { .. })
        )
    {
        let origin = body
            .expression(root)
            .ok_or(OwnershipAnalysisError::WrongBodyExpression(root))?
            .origin();
        analyzer.check_transfer_source(
            &ControlTransferSource::RegionCompletion(ControlRegionKey::Callable(owner)),
            origin,
            false,
            &mut state,
        )?;
    }
    let ownership_local_exists = body.patterns().try_fold(
        false,
        |found, (pattern_id, pattern)| -> Result<_, OwnershipAnalysisError> {
            if found {
                return Ok(true);
            }
            match pattern.kind() {
                PatternKind::Binding(_) | PatternKind::Alias { .. } => {}
                PatternKind::Wildcard
                | PatternKind::Literal(_)
                | PatternKind::Unit
                | PatternKind::Group(_)
                | PatternKind::Or(_)
                | PatternKind::Tuple(_)
                | PatternKind::Variant { .. }
                | PatternKind::Struct { .. }
                | PatternKind::Invalid => return Ok(false),
            }
            let Some(binding) = names.body_bindings(module, body.id()).and_then(|bindings| {
                bindings.binding(crate::resolve::PatternBindingSite::Name(pattern_id))
            }) else {
                return Ok(true);
            };
            match body_types.recorded_local(binding) {
                Some(FactState::Known(local)) => {
                    Ok(signatures.types().contains_ownership(local.ty())?)
                }
                Some(FactState::NotApplicable) => Ok(false),
                Some(FactState::Poisoned(_) | FactState::Unperformed) | None => Ok(true),
            }
        },
    )?;
    let ownership_expression_exists = body.expressions().try_fold(
        false,
        |found, (expression, _)| -> Result<_, OwnershipAnalysisError> {
            if found {
                return Ok(true);
            }
            match body_types.recorded_expression(ExpressionKey::new(module, expression)) {
                Some(FactState::Known(expression)) => {
                    Ok(signatures.types().contains_ownership(expression.ty())?)
                }
                Some(FactState::Poisoned(_) | FactState::Unperformed) => Ok(true),
                Some(FactState::NotApplicable) | None => Ok(false),
            }
        },
    )?;
    let ownership_relevant =
        ownership_local_exists || ownership_expression_exists || analyzer.operation_count != 0;
    let unperformed = analyzer.unperformed && ownership_relevant;
    let roots = analyzer
        .roots
        .into_iter()
        .map(|(binding, region)| OwnershipRootFact::new(binding, region));
    Ok(OwnershipBodyAnalysis {
        summary: OwnershipBodySummary::new(
            if unperformed {
                OwnershipBodyStatus::Unperformed
            } else {
                OwnershipBodyStatus::Checked
            },
            roots,
            analyzer.operation_count,
        ),
        failures: if unperformed {
            Vec::new()
        } else {
            analyzer.failures
        },
    })
}

struct BodyAnalyzer<'analysis> {
    owner: DefId,
    module: ModuleId,
    body: &'analysis Body,
    names: &'analysis NameResolution,
    signatures: &'analysis DeclarationSignatures,
    body_types: &'analysis BodyTypes,
    roots: BTreeMap<LocalBindingId, ControlRegionKey>,
    failures: Vec<OwnershipFailure>,
    operation_count: usize,
    unperformed: bool,
    recoverable_exits: Vec<(ControlRegionKey, Vec<FlowState>)>,
}

impl BodyAnalyzer<'_> {
    fn visit_ownership_operation(
        &mut self,
        key: ExpressionKey,
        origin: OriginId,
        state: &mut FlowState,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        let FactState::Known(operation) = self.body_types.ownership_operation(key) else {
            return Err(OwnershipAnalysisError::MissingExpressionType(key));
        };
        self.operation_count += 1;
        match operation.kind() {
            TypedOwnershipOperationKind::Own => {
                self.consume_source(operation.source(), origin, state);
            }
            TypedOwnershipOperationKind::Move | TypedOwnershipOperationKind::DropOwn => {
                self.consume_root(operation.source(), origin, state);
            }
        }
        let continuing = !matches!(
            operation.kind(),
            TypedOwnershipOperationKind::Own
                | TypedOwnershipOperationKind::Move
                | TypedOwnershipOperationKind::DropOwn
        ) || self.expression_continues(key, true);
        Ok(ExpressionFlow {
            continuing,
            provenance: ValueProvenance::default(),
        })
    }

    fn visit_name(
        &mut self,
        expression: ExprId,
        origin: OriginId,
        state: &FlowState,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        let Some(binding) = self.expression_local(expression)? else {
            return Ok(ExpressionFlow::continuing());
        };
        self.use_binding(binding, origin, state);
        let provenance = if self.binding_contains_reference(binding)? {
            state.aliases.get(&binding).cloned().unwrap_or_default()
        } else {
            ValueProvenance::default()
        };
        Ok(ExpressionFlow {
            continuing: true,
            provenance,
        })
    }

    fn retain_in_flight(
        &self,
        expression: ExprId,
        provenance: &ValueProvenance,
        in_flight: &mut Vec<(OriginId, BTreeSet<LocalBindingId>)>,
    ) -> Result<(), OwnershipAnalysisError> {
        let roots = provenance.roots();
        if !roots.is_empty() {
            in_flight.push((self.expression_origin(expression)?, roots));
        }
        Ok(())
    }

    fn check_in_flight(
        &mut self,
        in_flight: &[(OriginId, BTreeSet<LocalBindingId>)],
        state: &FlowState,
    ) {
        for (origin, roots) in in_flight {
            for &root in roots {
                if let Some(fact) = state.availability.get(&root)
                    && fact.state != OwnershipAvailability::Owned
                {
                    self.failures.push(OwnershipFailure::new(
                        self.owner,
                        OwnershipFailureKind::UseAfterDischarge,
                        *origin,
                        root,
                        fact.first_consume,
                    ));
                }
            }
        }
    }

    fn bind_let_pattern(
        &mut self,
        pattern: PatternId,
        initializer: ExprId,
        flow: &ExpressionFlow,
        state: &mut FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        let bindings = self
            .names
            .pattern_bindings(self.module, self.body, pattern)?;
        let receives_root = bindings
            .iter()
            .copied()
            .map(|binding| self.binding_is_root(binding))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|is_root| is_root);
        if receives_root {
            self.recover_missing_move(initializer, state)?;
        }
        for binding in bindings {
            if self.binding_is_root(binding)? {
                self.introduce_root(binding, state)?;
            } else if self.binding_contains_reference(binding)? {
                state.aliases.insert(binding, flow.provenance.clone());
            }
        }
        Ok(())
    }

    fn introduce_root(
        &mut self,
        binding: LocalBindingId,
        state: &mut FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        if !self.binding_is_root(binding)? {
            return Ok(());
        }
        let FactState::Known(local) = self.body_types.local(binding) else {
            return Err(OwnershipAnalysisError::MissingLocalType(binding));
        };
        self.roots
            .entry(binding)
            .or_insert_with(|| local.region().clone());
        state
            .availability
            .insert(binding, AvailabilityFact::owned());
        Ok(())
    }

    fn consume_root(&mut self, binding: LocalBindingId, origin: OriginId, state: &mut FlowState) {
        let fact = state
            .availability
            .entry(binding)
            .or_insert_with(AvailabilityFact::owned);
        match fact.state {
            OwnershipAvailability::Owned => {
                fact.state = OwnershipAvailability::Consumed;
                fact.first_consume = Some(origin);
            }
            OwnershipAvailability::Consumed | OwnershipAvailability::MaybeConsumed => {
                let related = fact.first_consume;
                fact.state = OwnershipAvailability::Consumed;
                self.failures.push(OwnershipFailure::new(
                    self.owner,
                    OwnershipFailureKind::DoubleMove,
                    origin,
                    binding,
                    related,
                ));
            }
        }
    }

    fn consume_source(&mut self, binding: LocalBindingId, origin: OriginId, state: &mut FlowState) {
        let fact = state
            .availability
            .entry(binding)
            .or_insert_with(AvailabilityFact::owned);
        if fact.state == OwnershipAvailability::Owned {
            fact.state = OwnershipAvailability::Consumed;
            fact.first_consume = Some(origin);
            return;
        }
        self.failures.push(OwnershipFailure::new(
            self.owner,
            OwnershipFailureKind::UseAfterDischarge,
            origin,
            binding,
            fact.first_consume,
        ));
        fact.state = OwnershipAvailability::Consumed;
    }

    fn use_binding(&mut self, binding: LocalBindingId, origin: OriginId, state: &FlowState) {
        if let Some(fact) = state.availability.get(&binding)
            && fact.state != OwnershipAvailability::Owned
        {
            self.failures.push(OwnershipFailure::new(
                self.owner,
                OwnershipFailureKind::UseAfterDischarge,
                origin,
                binding,
                fact.first_consume,
            ));
        }
        if let Some(provenance) = state.aliases.get(&binding) {
            self.use_roots(&provenance.roots(), origin, state);
        }
    }

    fn use_roots(&mut self, roots: &BTreeSet<LocalBindingId>, origin: OriginId, state: &FlowState) {
        for &root in roots {
            if let Some(fact) = state.availability.get(&root)
                && fact.state != OwnershipAvailability::Owned
            {
                self.failures.push(OwnershipFailure::new(
                    self.owner,
                    OwnershipFailureKind::UseAfterDischarge,
                    origin,
                    root,
                    fact.first_consume,
                ));
            }
        }
    }

    fn recover_missing_move(
        &mut self,
        expression: ExprId,
        state: &mut FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        let mut pending = vec![expression];
        while let Some(id) = pending.pop() {
            let expression = self
                .body
                .expression(id)
                .ok_or(OwnershipAnalysisError::WrongBodyExpression(id))?;
            match expression.kind() {
                ExprKind::Group(inner) => pending.push(*inner),
                ExprKind::If(value) => {
                    pending.extend(
                        [
                            Some(value.then_branch),
                            value.else_branch,
                            value.unknown_branch,
                        ]
                        .into_iter()
                        .flatten()
                        .rev(),
                    );
                }
                ExprKind::Match(value) => {
                    pending.extend(value.arms.iter().rev().map(|arm| arm.value));
                }
                ExprKind::Name(_) => {
                    if let Some(binding) = self.named_root(id)? {
                        let origin = expression.origin();
                        self.failures.push(OwnershipFailure::new(
                            self.owner,
                            OwnershipFailureKind::TransferRequiresMove,
                            origin,
                            binding,
                            None,
                        ));
                        self.recover_consumed(binding, origin, state);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn recover_consumed(&self, binding: LocalBindingId, origin: OriginId, state: &mut FlowState) {
        let fact = state
            .availability
            .entry(binding)
            .or_insert_with(AvailabilityFact::owned);
        if fact.state == OwnershipAvailability::Owned {
            fact.first_consume = Some(origin);
        }
        fact.state = OwnershipAvailability::Consumed;
    }

    fn named_root(
        &self,
        mut expression: ExprId,
    ) -> Result<Option<LocalBindingId>, OwnershipAnalysisError> {
        loop {
            let value = self
                .body
                .expression(expression)
                .ok_or(OwnershipAnalysisError::WrongBodyExpression(expression))?;
            if let ExprKind::Group(inner) = value.kind() {
                expression = *inner;
                continue;
            }
            if !matches!(value.kind(), ExprKind::Name(_)) {
                return Ok(None);
            }
            let Some(binding) = self.expression_local(expression)? else {
                return Ok(None);
            };
            return self
                .binding_is_root(binding)
                .map(|is_root| is_root.then_some(binding));
        }
    }

    fn check_side_exits(
        &mut self,
        key: ExpressionKey,
        origin: OriginId,
        state: &FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        let source = ControlTransferSource::Expression(key);
        let transfers = self
            .body_types
            .transfers()
            .iter()
            .filter(|transfer| transfer.source() == &source)
            .cloned()
            .collect::<Vec<_>>();
        for transfer in transfers {
            let mut exit_state = state.clone();
            self.check_transfer(&transfer, origin, false, &mut exit_state)?;
            if let Some((_, exits)) = self
                .recoverable_exits
                .iter_mut()
                .rev()
                .find(|(region, _)| region == transfer.target())
            {
                exits.push(exit_state);
            }
        }
        Ok(())
    }

    fn check_transfer_source(
        &mut self,
        source: &ControlTransferSource,
        origin: OriginId,
        closing_delimiter: bool,
        state: &mut FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        let transfers = self
            .body_types
            .transfers()
            .iter()
            .filter(|transfer| transfer.source() == source)
            .cloned()
            .collect::<Vec<_>>();
        for transfer in transfers {
            self.check_transfer(&transfer, origin, closing_delimiter, state)?;
        }
        Ok(())
    }

    fn check_transfer(
        &mut self,
        transfer: &ControlTransfer,
        origin: OriginId,
        closing_delimiter: bool,
        state: &mut FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        let exited = transfer.exited_regions().iter().collect::<BTreeSet<_>>();
        for (&binding, region) in &self.roots {
            if exited.contains(region)
                && state
                    .availability
                    .get(&binding)
                    .map_or(OwnershipAvailability::Owned, |fact| fact.state)
                    != OwnershipAvailability::Consumed
            {
                let related = state
                    .availability
                    .get(&binding)
                    .and_then(|fact| fact.first_consume);
                let failure = OwnershipFailure::new(
                    self.owner,
                    OwnershipFailureKind::PathUnbalanced,
                    origin,
                    binding,
                    related,
                );
                self.failures.push(if closing_delimiter {
                    failure.at_closing_delimiter()
                } else {
                    failure
                });
            }
        }
        let locals_to_remove = state
            .availability
            .keys()
            .copied()
            .filter(|binding| {
                matches!(
                    self.body_types.local(*binding),
                    FactState::Known(local) if exited.contains(local.region())
                )
            })
            .collect::<Vec<_>>();
        for binding in locals_to_remove {
            state.availability.remove(&binding);
            state.aliases.remove(&binding);
        }
        let aliases_to_remove = state
            .aliases
            .keys()
            .copied()
            .filter(|binding| {
                matches!(
                    self.body_types.local(*binding),
                    FactState::Known(local) if exited.contains(local.region())
                )
            })
            .collect::<Vec<_>>();
        for binding in aliases_to_remove {
            state.aliases.remove(&binding);
        }
        Ok(())
    }

    fn expression_roots(
        &self,
        mut expression: ExprId,
        state: &FlowState,
    ) -> Result<BTreeSet<LocalBindingId>, OwnershipAnalysisError> {
        loop {
            let key = ExpressionKey::new(self.module, expression);
            let FactState::Known(typed) = self.body_types.expression(key) else {
                return Ok(BTreeSet::new());
            };
            let Some(place) = typed.place() else {
                return Ok(BTreeSet::new());
            };
            match place.kind() {
                TypedPlaceKind::Local(binding) if self.binding_is_reference(*binding)? => {
                    return Ok(state
                        .aliases
                        .get(binding)
                        .map(ValueProvenance::roots)
                        .unwrap_or_default());
                }
                TypedPlaceKind::Local(binding) => return Ok(BTreeSet::from([*binding])),
                TypedPlaceKind::StoredField { receiver, .. }
                | TypedPlaceKind::TupleElement { receiver, .. } => expression = *receiver,
                TypedPlaceKind::Dereference { reference } => expression = *reference,
            }
        }
    }

    fn expression_local(
        &self,
        expression: ExprId,
    ) -> Result<Option<LocalBindingId>, OwnershipAnalysisError> {
        let key = ExpressionKey::new(self.module, expression);
        let FactState::Known(typed) = self.body_types.expression(key) else {
            return Ok(None);
        };
        Ok(match typed.place().map(crate::TypedPlace::kind) {
            Some(TypedPlaceKind::Local(binding)) => Some(*binding),
            _ => None,
        })
    }

    fn expression_continues(&self, key: ExpressionKey, fallback: bool) -> bool {
        match self.body_types.expression(key) {
            FactState::Known(typed) => typed.normal_continuation() == NormalContinuation::Present,
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => fallback,
        }
    }

    fn expression_origin(&self, expression: ExprId) -> Result<OriginId, OwnershipAnalysisError> {
        self.body
            .expression(expression)
            .map(eglur_hir::Expr::origin)
            .ok_or(OwnershipAnalysisError::WrongBodyExpression(expression))
    }

    fn binding_is_root(&self, binding: LocalBindingId) -> Result<bool, OwnershipAnalysisError> {
        let FactState::Known(local) = self.body_types.local(binding) else {
            return Err(OwnershipAnalysisError::MissingLocalType(binding));
        };
        Ok(self
            .signatures
            .types()
            .ownership_root(local.ty())?
            .is_some())
    }

    fn binding_is_reference(
        &self,
        binding: LocalBindingId,
    ) -> Result<bool, OwnershipAnalysisError> {
        let FactState::Known(local) = self.body_types.local(binding) else {
            return Err(OwnershipAnalysisError::MissingLocalType(binding));
        };
        Ok(matches!(
            self.signatures.types().kind(local.ty())?,
            TypeKind::Reference { .. }
        ))
    }

    fn binding_contains_reference(
        &self,
        binding: LocalBindingId,
    ) -> Result<bool, OwnershipAnalysisError> {
        let FactState::Known(local) = self.body_types.local(binding) else {
            return Err(OwnershipAnalysisError::MissingLocalType(binding));
        };
        Ok(self.signatures.types().contains_reference(local.ty())?)
    }
}
