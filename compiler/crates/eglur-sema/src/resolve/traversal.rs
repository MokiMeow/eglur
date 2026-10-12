use std::collections::BTreeSet;

use eglur_hir::{
    Body, ConstructorPayload, ExprId, ExprKind, HirFile, MatchArm, Name, ReduceBySingleValue,
    StmtId, StmtKind, TypeRefId,
};

use super::{
    BindingGroup, LookupPurpose, NameResolutionError, PathLookup, ResolutionKey, ResolvedName,
    Resolver, ScopeBarrier, ScopeKind, VisitFold, VisitOutcome,
};
use crate::{
    CompileCapability, ModuleId, Namespace, ResolveCapability, SemanticLocation, SemanticSource,
};

#[cfg(test)]
mod poison_tests;

enum VisitStep<'hir> {
    Expression(&'hir Body, ExprId),
    Statement(&'hir Body, StmtId),
    AssignmentTarget(&'hir Body, ExprId),
    Type(TypeRefId),
    Arm(&'hir Body, &'hir MatchArm),
    Member(&'hir Name),
    Outcome(VisitOutcome),
    Merge(Option<VisitOutcome>, usize),
    ExitScope,
    BindLet(BindingGroup, VisitOutcome, bool),
    ForBinding(&'hir Body, &'hir Name, ExprId),
}

#[derive(Default)]
struct Traversal<'hir> {
    pending: Vec<VisitStep<'hir>>,
    outcomes: Vec<VisitOutcome>,
}

impl<'hir> Traversal<'hir> {
    fn complete_sequence(
        &mut self,
        seed: Option<VisitOutcome>,
        count: usize,
    ) -> Result<(), NameResolutionError> {
        let start = self
            .outcomes
            .len()
            .checked_sub(count)
            .expect("all sequence children are visited");
        let children = self.outcomes.split_off(start);
        let mut folded = VisitFold::Empty;
        if let Some(seed) = seed {
            folded = folded.include(seed)?;
        }
        for outcome in children {
            folded = folded.include(outcome)?;
        }
        self.outcomes.push(folded.finish());
        Ok(())
    }

    fn sequence(&mut self, seed: Option<VisitOutcome>, children: Vec<VisitStep<'hir>>) {
        // A Known seed erases a lone Resolved identity; forwarding constructs must
        // keep an unseeded child outcome rather than flattening all visits together.
        self.pending.push(VisitStep::Merge(seed, children.len()));
        self.pending.extend(children.into_iter().rev());
    }

    fn outcome(&mut self) -> VisitOutcome {
        self.outcomes.pop().expect("scheduled child has an outcome")
    }
}

impl Resolver<'_, '_> {
    pub(super) fn visit_expr<'hir>(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &'hir HirFile,
        body: &'hir Body,
        expression: ExprId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let mut traversal = Traversal::default();
        traversal
            .pending
            .push(VisitStep::Expression(body, expression));
        while let Some(step) = traversal.pending.pop() {
            match step {
                VisitStep::Expression(body, id) => {
                    self.schedule_expression(module, source, hir, body, id, &mut traversal)?
                }
                VisitStep::Statement(body, id) => {
                    self.schedule_statement(module, source, hir, body, id, &mut traversal)?
                }
                VisitStep::AssignmentTarget(body, id) => {
                    self.schedule_assignment(module, source, body, id, &mut traversal)?
                }
                VisitStep::Type(id) => traversal
                    .outcomes
                    .push(self.visit_type(module, source, hir, id)?),
                VisitStep::Arm(body, arm) => {
                    self.schedule_arm(module, source, body, arm, &mut traversal)?
                }
                VisitStep::Member(member) => traversal.outcomes.push(if member.is_valid() {
                    VisitOutcome::Known
                } else {
                    self.invalid_outcome(source, member.origin())?
                }),
                VisitStep::Outcome(outcome) => traversal.outcomes.push(outcome),
                VisitStep::Merge(seed, count) => {
                    traversal.complete_sequence(seed, count)?;
                }
                VisitStep::ExitScope => self.pop_scope(),
                VisitStep::BindLet(group, mut outcome, initializer) => {
                    if initializer {
                        outcome = outcome.merge(traversal.outcome())?;
                    }
                    let binding = self.bind_group(source, group, Some(outcome.clone()))?;
                    traversal.outcomes.push(outcome.merge(binding)?);
                }
                VisitStep::ForBinding(body, binding, loop_body) => {
                    let outcome = traversal.outcome();
                    self.push_scope(module, body.id(), ScopeKind::ForBinding)?;
                    let group = self.single_name_group(source, binding)?;
                    let outcome = outcome.merge(self.bind_group(source, group, None)?)?;
                    traversal.pending.push(VisitStep::ExitScope);
                    traversal.sequence(Some(outcome), vec![VisitStep::Expression(body, loop_body)]);
                }
            }
        }
        assert_eq!(
            traversal.outcomes.len(),
            1,
            "one expression outcome is returned"
        );
        Ok(traversal.outcome())
    }

    fn schedule_expression<'hir>(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &'hir HirFile,
        body: &'hir Body,
        id: ExprId,
        traversal: &mut Traversal<'hir>,
    ) -> Result<(), NameResolutionError> {
        let expression = body
            .expression(id)
            .ok_or(NameResolutionError::WrongBodyExpression(id))?;
        let key = ResolutionKey::Expression {
            module,
            expression: id,
        };
        let mut seed = None;
        let children = match expression.kind() {
            ExprKind::Block { statements } => {
                self.push_scope(module, body.id(), ScopeKind::Block)?;
                traversal.pending.push(VisitStep::ExitScope);
                seed = Some(VisitOutcome::Known);
                statements
                    .iter()
                    .map(|&id| VisitStep::Statement(body, id))
                    .collect()
            }
            ExprKind::Binary { left, right, .. } => vec![
                VisitStep::Expression(body, *left),
                VisitStep::Expression(body, *right),
            ],
            ExprKind::Unary { operand, .. }
            | ExprKind::Reference { value: operand, .. }
            | ExprKind::Try(operand)
            | ExprKind::Group(operand)
            | ExprKind::Unsafe(operand)
            | ExprKind::Result(operand)
            | ExprKind::Checked(operand) => {
                traversal
                    .pending
                    .push(VisitStep::Expression(body, *operand));
                return Ok(());
            }
            ExprKind::Own(name) | ExprKind::Move(name) | ExprKind::DropOwn { name, .. } => {
                traversal.outcomes.push(self.resolve_name(
                    module,
                    source,
                    name,
                    Namespace::Value,
                    key,
                    LookupPurpose::Ordinary,
                )?);
                return Ok(());
            }
            ExprKind::Call(call) => std::iter::once(VisitStep::Expression(body, call.callee))
                .chain(
                    call.arguments
                        .iter()
                        .map(|arg| VisitStep::Expression(body, arg.value)),
                )
                .chain(call.type_arguments.iter().copied().map(VisitStep::Type))
                .collect(),
            ExprKind::CatalogCall(call) => {
                let outcome = if call.owner.text().is_some() {
                    self.demand_external(source, call.owner.origin())?
                } else {
                    self.invalid_path_outcome(source, &call.owner)?
                };
                seed = Some(outcome.merge(if call.member.is_valid() {
                    VisitOutcome::Known
                } else {
                    self.invalid_outcome(source, call.member.origin())?
                })?);
                call.owner_arguments
                    .iter()
                    .copied()
                    .map(VisitStep::Type)
                    .chain(
                        call.arguments
                            .iter()
                            .map(|arg| VisitStep::Expression(body, arg.value)),
                    )
                    .collect()
            }
            ExprKind::Member { receiver, member } => {
                if let Some((receiver_id, receiver_expr)) = body.grouping_chain(*receiver).last() {
                    let path = match receiver_expr.kind() {
                        ExprKind::Name(path)
                        | ExprKind::VariantConstructor {
                            path,
                            payload: ConstructorPayload::Absent,
                        } => Some(path),
                        _ => None,
                    };
                    if let Some(path) = path {
                        let value = self.lookup_path(
                            module,
                            source,
                            path,
                            PathLookup::Namespace(Namespace::Value),
                            LookupPurpose::Probe,
                        )?;
                        if matches!(value, VisitOutcome::NotApplicable) {
                            let owner = self.lookup_path(
                                module,
                                source,
                                path,
                                PathLookup::Namespace(Namespace::Type),
                                LookupPurpose::Probe,
                            )?;
                            if let VisitOutcome::Resolved(ResolvedName::Declarations(owners)) =
                                &owner
                                && let [owner] = owners.as_ref()
                            {
                                let outcome = if member.is_valid() {
                                    VisitOutcome::Resolved(ResolvedName::TypeMemberOwner(
                                        owner.clone(),
                                    ))
                                } else {
                                    self.invalid_outcome(source, member.origin())?
                                };
                                self.record_fact(key, outcome.clone())?;
                                traversal.outcomes.push(outcome);
                                return Ok(());
                            }
                            if matches!(
                                owner,
                                VisitOutcome::Poisoned(_) | VisitOutcome::Unperformed
                            ) {
                                self.record_fact(key, owner.clone())?;
                                traversal.outcomes.push(owner);
                                return Ok(());
                            }
                        } else {
                            self.record_fact(
                                ResolutionKey::Expression {
                                    module,
                                    expression: receiver_id,
                                },
                                value.clone(),
                            )?;
                            let member = if member.is_valid() {
                                VisitOutcome::Known
                            } else {
                                self.invalid_outcome(source, member.origin())?
                            };
                            traversal.outcomes.push(value.merge(member)?);
                            return Ok(());
                        }
                    }
                }
                let member = if member.is_valid() {
                    VisitOutcome::Known
                } else {
                    self.invalid_outcome(source, member.origin())?
                };
                vec![
                    VisitStep::Expression(body, *receiver),
                    VisitStep::Outcome(member),
                ]
            }
            ExprKind::TupleProjection { receiver, .. } => {
                traversal
                    .pending
                    .push(VisitStep::Expression(body, *receiver));
                return Ok(());
            }
            ExprKind::DTypeCall(call) => std::iter::once(VisitStep::Expression(body, call.callee))
                .chain(std::iter::once(VisitStep::Type(call.dtype)))
                .chain(
                    call.arguments
                        .iter()
                        .map(|arg| VisitStep::Expression(body, arg.value)),
                )
                .collect(),
            ExprKind::ReduceByCandidate(candidate) => {
                let mut children = vec![candidate.receiver];
                if let Some(ordinary) = &candidate.ordinary {
                    if ordinary.candidate_names.is_empty() {
                        children.extend(ordinary.call.arguments.iter().map(|arg| arg.value));
                    }
                    for &name in &ordinary.candidate_names {
                        self.defer_candidate_name(module, source, body, name)?;
                    }
                }
                if let Some(single) = &candidate.single_reducer {
                    let value = match single.value {
                        ReduceBySingleValue::Expression(value)
                        | ReduceBySingleValue::PhasePair { reducer: value, .. } => value,
                    };
                    children.push(value);
                }
                if let Some(phases) = &candidate.ordered_phases {
                    children.extend(phases.phases.iter().map(|phase| phase.reducer));
                }
                let mut seen = BTreeSet::new();
                seed = Some(VisitOutcome::Known);
                children
                    .into_iter()
                    .filter(|id| seen.insert(*id))
                    .map(|id| VisitStep::Expression(body, id))
                    .collect()
            }
            ExprKind::Name(path) => {
                traversal.outcomes.push(self.resolve_path(
                    module,
                    source,
                    path,
                    PathLookup::Namespace(Namespace::Value),
                    key,
                    LookupPurpose::Ordinary,
                )?);
                return Ok(());
            }
            ExprKind::Literal(_) | ExprKind::Hole | ExprKind::Unit | ExprKind::TensorSchema(_) => {
                traversal.outcomes.push(VisitOutcome::Known);
                return Ok(());
            }
            ExprKind::Tuple(values) | ExprKind::Array(values) => {
                seed = Some(VisitOutcome::Known);
                values
                    .iter()
                    .map(|&id| VisitStep::Expression(body, id))
                    .collect()
            }
            ExprKind::Map(entries) => {
                seed = Some(VisitOutcome::Known);
                entries
                    .iter()
                    .flat_map(|entry| {
                        [
                            VisitStep::Expression(body, entry.key),
                            VisitStep::Expression(body, entry.value),
                        ]
                    })
                    .collect()
            }
            ExprKind::StructLiteral { path, fields } => {
                seed = Some(self.resolve_path(
                    module,
                    source,
                    path,
                    PathLookup::Namespace(Namespace::Type),
                    key,
                    LookupPurpose::Ordinary,
                )?);
                fields
                    .iter()
                    .map(|field| VisitStep::Expression(body, field.value))
                    .collect()
            }
            ExprKind::VariantConstructor { path, payload } => {
                let lookup = if matches!(payload, ConstructorPayload::Named(_)) {
                    PathLookup::BracedConstructor
                } else {
                    PathLookup::Namespace(Namespace::Value)
                };
                seed = Some(self.resolve_path(
                    module,
                    source,
                    path,
                    lookup,
                    key,
                    LookupPurpose::Ordinary,
                )?);
                match payload {
                    ConstructorPayload::Absent => Vec::new(),
                    ConstructorPayload::Tuple(values) => values
                        .iter()
                        .map(|&id| VisitStep::Expression(body, id))
                        .collect(),
                    ConstructorPayload::Named(fields) => fields
                        .iter()
                        .map(|field| VisitStep::Expression(body, field.value))
                        .collect(),
                }
            }
            ExprKind::Lambda(lambda) => {
                let body = hir.body(lambda.body);
                self.push_scope(module, body.id(), ScopeKind::LambdaParameters)?;
                let outcome = self
                    .visit_parameters(module, source, hir, body, &lambda.parameters)?
                    .finish();
                self.push_scope(module, body.id(), ScopeKind::LambdaBody)?;
                traversal.pending.push(VisitStep::ExitScope);
                traversal.pending.push(VisitStep::ExitScope);
                seed = Some(outcome);
                lambda
                    .statements
                    .iter()
                    .map(|&id| VisitStep::Statement(body, id))
                    .chain(
                        lambda
                            .tail
                            .into_iter()
                            .map(|id| VisitStep::Expression(body, id)),
                    )
                    .collect()
            }
            ExprKind::If(value) => [
                Some(value.condition),
                Some(value.then_branch),
                value.else_branch,
                value.unknown_branch,
            ]
            .into_iter()
            .flatten()
            .map(|id| VisitStep::Expression(body, id))
            .collect(),
            ExprKind::Match(value) => std::iter::once(VisitStep::Expression(body, value.scrutinee))
                .chain(value.arms.iter().map(|arm| VisitStep::Arm(body, arm)))
                .collect(),
            ExprKind::Invalid => {
                traversal
                    .outcomes
                    .push(self.invalid_outcome(source, expression.origin())?);
                return Ok(());
            }
        };
        traversal.sequence(seed, children);
        Ok(())
    }

    fn schedule_arm<'hir>(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        body: &'hir Body,
        arm: &'hir MatchArm,
        traversal: &mut Traversal<'hir>,
    ) -> Result<(), NameResolutionError> {
        let pattern = body
            .pattern(arm.pattern)
            .ok_or(NameResolutionError::WrongBodyPattern(arm.pattern))?;
        let capability = CompileCapability::Resolve(ResolveCapability::ResolvePatternScopes);
        let location = SemanticLocation::origin(source.file(), pattern.origin());
        let outcome = if self.recorder.require(capability, location) {
            self.visit_pattern_heads(module, source, body, arm.pattern)?
        } else {
            VisitOutcome::Unperformed
        };
        self.push_scope(module, body.id(), ScopeKind::MatchArm)?;
        let group = self.pattern_bindings(source, body, arm.pattern)?;
        let binding = self.bind_group(source, group, Some(outcome.clone()))?;
        traversal.pending.push(VisitStep::ExitScope);
        let children = arm
            .guard
            .iter()
            .map(|guard| VisitStep::Expression(body, guard.condition))
            .chain(std::iter::once(VisitStep::Expression(body, arm.value)))
            .collect();
        traversal.sequence(Some(outcome.merge(binding)?), children);
        Ok(())
    }

    fn schedule_statement<'hir>(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &'hir HirFile,
        body: &'hir Body,
        id: StmtId,
        traversal: &mut Traversal<'hir>,
    ) -> Result<(), NameResolutionError> {
        let statement = body
            .statement(id)
            .ok_or(NameResolutionError::WrongBodyStatement(id))?;
        let children = match statement.kind() {
            StmtKind::Let {
                pattern,
                ty,
                initializer,
                ..
            } => {
                let group = self.pattern_bindings(source, body, *pattern)?;
                if let Some(collision) = self.first_collision(&group) {
                    let outcome = self.record_binding_collision(source, collision)?;
                    self.record_fact(
                        ResolutionKey::Pattern {
                            module,
                            pattern: *pattern,
                        },
                        outcome.clone(),
                    )?;
                    traversal.outcomes.push(outcome);
                    return Ok(());
                }
                let outcome = self.visit_optional_type(module, source, hir, *ty)?;
                traversal
                    .pending
                    .push(VisitStep::BindLet(group, outcome, initializer.is_some()));
                if let Some(id) = initializer {
                    traversal.pending.push(VisitStep::Expression(body, *id));
                }
                return Ok(());
            }
            StmtKind::Assign { target, value } => vec![
                VisitStep::AssignmentTarget(body, *target),
                VisitStep::Expression(body, *value),
            ],
            StmtKind::While {
                condition,
                body: loop_body,
            } => vec![
                VisitStep::Expression(body, *condition),
                VisitStep::Expression(body, *loop_body),
            ],
            StmtKind::For {
                binding,
                iterable,
                body: loop_body,
            } => {
                traversal
                    .pending
                    .push(VisitStep::ForBinding(body, binding, *loop_body));
                traversal
                    .pending
                    .push(VisitStep::Expression(body, *iterable));
                return Ok(());
            }
            StmtKind::Break | StmtKind::Continue => Vec::new(),
            StmtKind::Return(value) => value
                .iter()
                .map(|&id| VisitStep::Expression(body, id))
                .collect(),
            StmtKind::Expression(value) | StmtKind::Yield(value) => {
                traversal.pending.push(VisitStep::Expression(body, *value));
                return Ok(());
            }
            StmtKind::Invalid => {
                let outcome = self.invalid_outcome(source, statement.origin())?;
                self.set_scope_barrier(ScopeBarrier::from_outcome(&outcome))?;
                traversal.outcomes.push(outcome);
                return Ok(());
            }
        };
        traversal.sequence(None, children);
        Ok(())
    }

    fn schedule_assignment<'hir>(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        body: &'hir Body,
        id: ExprId,
        traversal: &mut Traversal<'hir>,
    ) -> Result<(), NameResolutionError> {
        let expression = body
            .expression(id)
            .ok_or(NameResolutionError::WrongBodyExpression(id))?;
        match expression.kind() {
            ExprKind::Name(path) => traversal.outcomes.push(self.resolve_path(
                module,
                source,
                path,
                PathLookup::Namespace(Namespace::Value),
                ResolutionKey::Expression {
                    module,
                    expression: id,
                },
                LookupPurpose::AssignmentTarget,
            )?),
            ExprKind::Member { receiver, member } => traversal.sequence(
                None,
                vec![
                    VisitStep::Expression(body, *receiver),
                    VisitStep::Member(member),
                ],
            ),
            ExprKind::Group(inner) => traversal
                .pending
                .push(VisitStep::AssignmentTarget(body, *inner)),
            _ => traversal.pending.push(VisitStep::Expression(body, id)),
        }
        Ok(())
    }
}
