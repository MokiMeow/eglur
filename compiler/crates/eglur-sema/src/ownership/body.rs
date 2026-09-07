use std::collections::{BTreeMap, BTreeSet};

use eglur_hir::{
    Body, ConstructorPayload, ExprId, ExprKind, OriginId, PatternId, PatternKind,
    ReduceBySingleValue, StmtId, StmtKind,
};

use crate::{
    BodyTypes, ControlRegionKey, ControlTransfer, ControlTransferSource, DeclarationSignatures,
    DefId, ExpressionKey, FactState, IfBranch, LocalBindingId, ModuleId, NameResolution,
    NormalContinuation, ReduceByCommitment, StatementKey, TypeKind, TypedOwnershipOperationKind,
    TypedPlaceKind,
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
        |found, (_, pattern)| -> Result<_, OwnershipAnalysisError> {
            if found {
                return Ok(true);
            }
            let origin = match pattern.kind() {
                PatternKind::Binding(name) | PatternKind::Alias { name, .. } => name.origin(),
                PatternKind::Wildcard
                | PatternKind::Literal(_)
                | PatternKind::Unit
                | PatternKind::Group(_)
                | PatternKind::Or(_)
                | PatternKind::Tuple(_)
                | PatternKind::Variant { .. }
                | PatternKind::Struct { .. }
                | PatternKind::Invalid => return Ok(false),
            };
            let Some(binding) = names.local_binding(module, body.id(), origin) else {
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
    fn visit_expression(
        &mut self,
        expression_id: ExprId,
        state: &mut FlowState,
        region_override: Option<ControlRegionKey>,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        let expression = self
            .body
            .expression(expression_id)
            .ok_or(OwnershipAnalysisError::WrongBodyExpression(expression_id))?;
        let origin = expression.origin();
        let key = ExpressionKey::new(self.module, expression_id);
        let mut flow = match expression.kind() {
            ExprKind::Block { statements } => {
                let region = region_override.unwrap_or(ControlRegionKey::Block(key));
                self.visit_block(statements, region, origin, state)?
            }
            ExprKind::Own(_) | ExprKind::Move(_) | ExprKind::DropOwn { .. } => {
                self.visit_ownership_operation(key, origin, state)?
            }
            ExprKind::Name(_) => self.visit_name(expression_id, origin, state)?,
            ExprKind::Reference { value, .. } => {
                let inner = self.visit_expression(*value, state, None)?;
                let mut roots = inner.provenance.roots();
                roots.extend(self.expression_roots(*value, state)?);
                ExpressionFlow {
                    continuing: inner.continuing,
                    provenance: ValueProvenance::direct(roots),
                }
            }
            ExprKind::Group(inner) | ExprKind::Unsafe(inner) => {
                self.visit_expression(*inner, state, None)?
            }
            ExprKind::Unary { operand, .. } => {
                let inner = self.visit_expression(*operand, state, None)?;
                ExpressionFlow {
                    continuing: self.expression_continues(key, inner.continuing),
                    provenance: inner.provenance,
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.visit_expression(*left, state, None)?;
                self.visit_expression(*right, state, None)?;
                ExpressionFlow {
                    continuing: self.expression_continues(key, true),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::Call(call) => {
                let callee = self.visit_expression(call.callee, state, None)?;
                let mut in_flight = Vec::new();
                self.retain_in_flight(call.callee, &callee.provenance, &mut in_flight)?;
                let continuing = self.visit_call_arguments(
                    call.arguments.iter().map(|argument| argument.value),
                    state,
                    callee.continuing,
                    &mut in_flight,
                )?;
                self.check_in_flight(&in_flight, state);
                ExpressionFlow {
                    continuing: self.expression_continues(key, continuing),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::CatalogCall(call) => {
                let mut in_flight = Vec::new();
                let continuing = self.visit_call_arguments(
                    call.arguments.iter().map(|argument| argument.value),
                    state,
                    true,
                    &mut in_flight,
                )?;
                self.check_in_flight(&in_flight, state);
                ExpressionFlow {
                    continuing: self.expression_continues(key, continuing),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::DTypeCall(call) => {
                let callee = self.visit_expression(call.callee, state, None)?;
                let mut in_flight = Vec::new();
                self.retain_in_flight(call.callee, &callee.provenance, &mut in_flight)?;
                let continuing = self.visit_call_arguments(
                    call.arguments.iter().map(|argument| argument.value),
                    state,
                    callee.continuing,
                    &mut in_flight,
                )?;
                self.check_in_flight(&in_flight, state);
                ExpressionFlow {
                    continuing: self.expression_continues(key, continuing),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::Member { receiver, .. } => {
                let receiver = self.visit_expression(*receiver, state, None)?;
                ExpressionFlow {
                    continuing: self.expression_continues(key, receiver.continuing),
                    provenance: receiver.provenance,
                }
            }
            ExprKind::TupleProjection { receiver, index } => {
                let receiver = if let Some(binding) = self.expression_local(*receiver)?
                    && self.binding_contains_reference(binding)?
                {
                    ExpressionFlow {
                        continuing: self
                            .expression_continues(ExpressionKey::new(self.module, *receiver), true),
                        provenance: state.aliases.get(&binding).cloned().unwrap_or_default(),
                    }
                } else {
                    self.visit_expression(*receiver, state, None)?
                };
                let index = index.parse::<usize>().map_err(|_| {
                    OwnershipAnalysisError::InvalidTupleProjectionIndex {
                        expression: key,
                        index: index.clone(),
                    }
                })?;
                let provenance = receiver.provenance.projected(index);
                self.use_roots(&provenance.roots(), origin, state);
                ExpressionFlow {
                    continuing: self.expression_continues(key, receiver.continuing),
                    provenance,
                }
            }
            ExprKind::Try(operand) => {
                let operand_id = *operand;
                let operand = self.visit_expression(operand_id, state, None)?;
                self.recover_missing_move(operand_id, state)?;
                ExpressionFlow {
                    continuing: self.expression_continues(key, operand.continuing),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::Result(inner) => {
                self.visit_recoverable_region(*inner, ControlRegionKey::Result(key), state)?
            }
            ExprKind::Checked(inner) => {
                self.visit_recoverable_region(*inner, ControlRegionKey::Checked(key), state)?
            }
            ExprKind::If(value) => self.visit_if(key, value, state)?,
            ExprKind::Match(value) => {
                self.unperformed = true;
                self.visit_expression(value.scrutinee, state, None)?;
                for arm in value.arms.iter() {
                    if let Some(guard) = &arm.guard {
                        self.visit_expression(guard.condition, state, None)?;
                    }
                    self.visit_expression(arm.value, state, None)?;
                }
                ExpressionFlow {
                    continuing: self.expression_continues(key, true),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::Tuple(values) => {
                let mut continuing = true;
                let mut provenance = ValueProvenance::default();
                for (index, &value) in values.iter().enumerate() {
                    if !continuing {
                        break;
                    }
                    let flow = self.visit_expression(value, state, None)?;
                    if flow.continuing {
                        self.recover_missing_move(value, state)?;
                    }
                    provenance.extend(flow.provenance.prefixed(index));
                    continuing = flow.continuing;
                }
                ExpressionFlow {
                    continuing: self.expression_continues(key, continuing),
                    provenance,
                }
            }
            ExprKind::Array(values) => {
                let mut continuing = true;
                for &value in values.iter() {
                    if !continuing {
                        break;
                    }
                    let flow = self.visit_expression(value, state, None)?;
                    if flow.continuing {
                        self.recover_missing_move(value, state)?;
                    }
                    continuing = flow.continuing;
                }
                ExpressionFlow {
                    continuing: self.expression_continues(key, continuing),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries.iter() {
                    self.visit_expression(entry.key, state, None)?;
                    self.visit_expression(entry.value, state, None)?;
                }
                ExpressionFlow {
                    continuing: self.expression_continues(key, true),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::StructLiteral { fields, .. } => {
                for field in fields.iter() {
                    self.visit_expression(field.value, state, None)?;
                }
                ExpressionFlow {
                    continuing: self.expression_continues(key, true),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::VariantConstructor { payload, .. } => {
                self.visit_constructor_payload(payload, state)?;
                ExpressionFlow {
                    continuing: self.expression_continues(key, true),
                    provenance: ValueProvenance::default(),
                }
            }
            ExprKind::ReduceByCandidate(candidate) => {
                match self.body_types.reduce_by_commitment(key) {
                    FactState::Known(ReduceByCommitment::Ordinary) => {
                        let ordinary = candidate
                            .ordinary
                            .as_ref()
                            .expect("ordinary commitment retains an ordinary candidate");
                        let callee = self.visit_expression(ordinary.call.callee, state, None)?;
                        let mut in_flight = Vec::new();
                        self.retain_in_flight(
                            ordinary.call.callee,
                            &callee.provenance,
                            &mut in_flight,
                        )?;
                        let continuing = self.visit_call_arguments(
                            ordinary
                                .call
                                .arguments
                                .iter()
                                .map(|argument| argument.value),
                            state,
                            callee.continuing,
                            &mut in_flight,
                        )?;
                        self.check_in_flight(&in_flight, state);
                        ExpressionFlow {
                            continuing: self.expression_continues(key, continuing),
                            provenance: ValueProvenance::default(),
                        }
                    }
                    FactState::Known(ReduceByCommitment::TensorSingle) => {
                        self.visit_expression(candidate.receiver, state, None)?;
                        let single = candidate
                            .single_reducer
                            .as_ref()
                            .expect("single commitment retains a reducer");
                        let value = match single.value {
                            ReduceBySingleValue::Expression(value)
                            | ReduceBySingleValue::PhasePair { reducer: value, .. } => value,
                        };
                        self.visit_expression(value, state, None)?;
                        ExpressionFlow {
                            continuing: self.expression_continues(key, true),
                            provenance: ValueProvenance::default(),
                        }
                    }
                    FactState::Known(ReduceByCommitment::TensorPhases) => {
                        self.visit_expression(candidate.receiver, state, None)?;
                        let phases = candidate
                            .ordered_phases
                            .as_ref()
                            .expect("phase commitment retains ordered phases");
                        for phase in phases.phases.iter() {
                            self.visit_expression(phase.reducer, state, None)?;
                        }
                        ExpressionFlow {
                            continuing: self.expression_continues(key, true),
                            provenance: ValueProvenance::default(),
                        }
                    }
                    FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                        self.unperformed = true;
                        ExpressionFlow::continuing()
                    }
                }
            }
            ExprKind::Lambda(_) => {
                self.unperformed = true;
                ExpressionFlow::continuing()
            }
            ExprKind::Literal(_)
            | ExprKind::Hole
            | ExprKind::Unit
            | ExprKind::TensorSchema(_)
            | ExprKind::Invalid => ExpressionFlow {
                continuing: self.expression_continues(key, true),
                provenance: ValueProvenance::default(),
            },
        };
        self.check_side_exits(key, origin, state)?;
        flow.continuing = self.expression_continues(key, flow.continuing);
        Ok(flow)
    }

    fn visit_recoverable_region(
        &mut self,
        inner: ExprId,
        region: ControlRegionKey,
        state: &mut FlowState,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        self.recoverable_exits.push((region.clone(), Vec::new()));
        let result = self.visit_expression(inner, state, Some(region.clone()));
        let (recorded_region, mut exits) = self
            .recoverable_exits
            .pop()
            .expect("recoverable region collector is balanced");
        debug_assert_eq!(recorded_region, region);
        let mut flow = result?;
        if flow.continuing {
            exits.push(state.clone());
        }
        if !exits.is_empty() {
            *state = FlowState::join(&exits);
            flow.continuing = true;
        }
        Ok(flow)
    }

    fn visit_block(
        &mut self,
        statements: &[StmtId],
        region: ControlRegionKey,
        origin: OriginId,
        state: &mut FlowState,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        let mut continuing = true;
        let mut provenance = ValueProvenance::default();
        for &statement in statements {
            if !continuing {
                break;
            }
            let step = self.visit_statement(statement, state)?;
            provenance.extend(step.provenance);
            continuing = step.continuing;
        }
        if continuing {
            self.check_transfer_source(
                &ControlTransferSource::RegionCompletion(region),
                origin,
                true,
                state,
            )?;
        }
        Ok(ExpressionFlow {
            continuing,
            provenance,
        })
    }

    fn visit_statement(
        &mut self,
        statement_id: StmtId,
        state: &mut FlowState,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        let statement = self
            .body
            .statement(statement_id)
            .ok_or(OwnershipAnalysisError::WrongBodyStatement(statement_id))?;
        Ok(match statement.kind() {
            StmtKind::Let {
                pattern,
                initializer,
                ..
            } => {
                let flow = if let Some(initializer) = initializer {
                    let flow = self.visit_expression(*initializer, state, None)?;
                    if flow.continuing {
                        self.bind_let_pattern(*pattern, *initializer, &flow, state)?;
                    }
                    flow
                } else {
                    ExpressionFlow::continuing()
                };
                ExpressionFlow {
                    continuing: flow.continuing,
                    provenance: ValueProvenance::default(),
                }
            }
            StmtKind::Assign { target, value } => {
                let target_continues = if self.expression_local(*target)?.is_some() {
                    self.expression_continues(ExpressionKey::new(self.module, *target), true)
                } else {
                    self.visit_expression(*target, state, None)?.continuing
                };
                let value_flow = self.visit_expression(*value, state, None)?;
                if let Some(binding) = self.expression_local(*target)?
                    && self.binding_contains_reference(binding)?
                    && value_flow.continuing
                {
                    state.aliases.insert(binding, value_flow.provenance);
                }
                ExpressionFlow {
                    continuing: target_continues && value_flow.continuing,
                    provenance: ValueProvenance::default(),
                }
            }
            StmtKind::Expression(value) => {
                let flow = self.visit_expression(*value, state, None)?;
                ExpressionFlow {
                    continuing: flow.continuing,
                    provenance: ValueProvenance::default(),
                }
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.visit_expression(*value, state, None)?;
                    self.recover_missing_move(*value, state)?;
                }
                self.check_transfer_source(
                    &ControlTransferSource::Statement(StatementKey::new(self.module, statement_id)),
                    statement.origin(),
                    false,
                    state,
                )?;
                ExpressionFlow::default()
            }
            StmtKind::Yield(value) => {
                let flow = self.visit_expression(*value, state, None)?;
                self.recover_missing_move(*value, state)?;
                self.check_transfer_source(
                    &ControlTransferSource::Statement(StatementKey::new(self.module, statement_id)),
                    statement.origin(),
                    false,
                    state,
                )?;
                ExpressionFlow {
                    continuing: false,
                    provenance: if flow.continuing {
                        flow.provenance
                    } else {
                        ValueProvenance::default()
                    },
                }
            }
            StmtKind::While { condition, body } => {
                self.unperformed = true;
                self.visit_expression(*condition, state, None)?;
                self.visit_expression(*body, state, None)?;
                ExpressionFlow::continuing()
            }
            StmtKind::For { iterable, body, .. } => {
                self.unperformed = true;
                self.visit_expression(*iterable, state, None)?;
                self.visit_expression(*body, state, None)?;
                ExpressionFlow::continuing()
            }
            StmtKind::Break | StmtKind::Continue => {
                self.unperformed = true;
                ExpressionFlow::default()
            }
            StmtKind::Invalid => ExpressionFlow::continuing(),
        })
    }

    fn visit_if(
        &mut self,
        owner: ExpressionKey,
        expression: &eglur_hir::IfExpr,
        state: &mut FlowState,
    ) -> Result<ExpressionFlow, OwnershipAnalysisError> {
        let condition = self.visit_expression(expression.condition, state, None)?;
        if !condition.continuing {
            return Ok(ExpressionFlow::default());
        }
        let entry = state.clone();
        let branches = [
            (IfBranch::Then, Some(expression.then_branch)),
            (IfBranch::Else, expression.else_branch),
            (IfBranch::Unknown, expression.unknown_branch),
        ];
        let mut continuing_states = Vec::new();
        let mut provenance = ValueProvenance::default();
        for (branch, value) in branches {
            let Some(value) = value else {
                if matches!(branch, IfBranch::Else) {
                    continuing_states.push(entry.clone());
                }
                continue;
            };
            let mut branch_state = entry.clone();
            let flow = self.visit_expression(
                value,
                &mut branch_state,
                Some(ControlRegionKey::IfBranch { owner, branch }),
            )?;
            if condition.continuing && flow.continuing {
                provenance.extend(flow.provenance);
                continuing_states.push(branch_state);
            }
        }
        if continuing_states.is_empty() {
            return Ok(ExpressionFlow {
                continuing: false,
                provenance,
            });
        }
        *state = FlowState::join(&continuing_states);
        Ok(ExpressionFlow {
            continuing: condition.continuing,
            provenance,
        })
    }

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

    fn visit_call_arguments(
        &mut self,
        values: impl IntoIterator<Item = ExprId>,
        state: &mut FlowState,
        mut continuing: bool,
        in_flight: &mut Vec<(OriginId, BTreeSet<LocalBindingId>)>,
    ) -> Result<bool, OwnershipAnalysisError> {
        for value in values {
            if !continuing {
                break;
            }
            let flow = self.visit_expression(value, state, None)?;
            if flow.continuing {
                self.recover_missing_move(value, state)?;
            }
            self.retain_in_flight(value, &flow.provenance, in_flight)?;
            continuing = flow.continuing;
        }
        Ok(continuing)
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

    fn visit_constructor_payload(
        &mut self,
        payload: &ConstructorPayload,
        state: &mut FlowState,
    ) -> Result<(), OwnershipAnalysisError> {
        match payload {
            ConstructorPayload::Absent => {}
            ConstructorPayload::Tuple(values) => {
                for &value in values.iter() {
                    self.visit_expression(value, state, None)?;
                    self.recover_missing_move(value, state)?;
                }
            }
            ConstructorPayload::Named(fields) => {
                for field in fields.iter() {
                    self.visit_expression(field.value, state, None)?;
                    self.recover_missing_move(field.value, state)?;
                }
            }
        }
        Ok(())
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
        let expression_value = self
            .body
            .expression(expression)
            .ok_or(OwnershipAnalysisError::WrongBodyExpression(expression))?;
        match expression_value.kind() {
            ExprKind::Group(inner) => self.recover_missing_move(*inner, state)?,
            ExprKind::If(value) => {
                self.recover_missing_move(value.then_branch, state)?;
                for branch in [value.else_branch, value.unknown_branch]
                    .into_iter()
                    .flatten()
                {
                    self.recover_missing_move(branch, state)?;
                }
            }
            ExprKind::Match(value) => {
                for arm in value.arms.iter() {
                    self.recover_missing_move(arm.value, state)?;
                }
            }
            ExprKind::Name(_) => {
                if let Some(binding) = self.named_root(expression)? {
                    let origin = self.expression_origin(expression)?;
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
        expression: ExprId,
    ) -> Result<Option<LocalBindingId>, OwnershipAnalysisError> {
        let expression_value = self
            .body
            .expression(expression)
            .ok_or(OwnershipAnalysisError::WrongBodyExpression(expression))?;
        if let ExprKind::Group(inner) = expression_value.kind() {
            return self.named_root(*inner);
        }
        if !matches!(expression_value.kind(), ExprKind::Name(_)) {
            return Ok(None);
        }
        let Some(binding) = self.expression_local(expression)? else {
            return Ok(None);
        };
        self.binding_is_root(binding)
            .map(|is_root| is_root.then_some(binding))
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
        expression: ExprId,
        state: &FlowState,
    ) -> Result<BTreeSet<LocalBindingId>, OwnershipAnalysisError> {
        let key = ExpressionKey::new(self.module, expression);
        let FactState::Known(typed) = self.body_types.expression(key) else {
            return Ok(BTreeSet::new());
        };
        let Some(place) = typed.place() else {
            return Ok(BTreeSet::new());
        };
        self.place_roots(place.kind(), state)
    }

    fn place_roots(
        &self,
        place: &TypedPlaceKind,
        state: &FlowState,
    ) -> Result<BTreeSet<LocalBindingId>, OwnershipAnalysisError> {
        match place {
            TypedPlaceKind::Local(binding) if self.binding_is_reference(*binding)? => Ok(state
                .aliases
                .get(binding)
                .map(ValueProvenance::roots)
                .unwrap_or_default()),
            TypedPlaceKind::Local(binding) => Ok(BTreeSet::from([*binding])),
            TypedPlaceKind::StoredField { receiver, .. }
            | TypedPlaceKind::TupleElement { receiver, .. } => {
                self.expression_roots(*receiver, state)
            }
            TypedPlaceKind::Dereference { reference } => self.expression_roots(*reference, state),
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
