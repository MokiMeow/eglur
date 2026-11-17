use super::*;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum BodyNode {
    Expression(ExprId),
    Statement(StmtId),
}

impl NodeHasher<'_, '_> {
    pub(super) fn hash_body_nodes(
        &mut self,
        root: BodyNode,
    ) -> Result<FingerprintDigest, SemanticFingerprintError> {
        let mut pending = vec![(root, false)];
        let mut active = BTreeSet::new();
        while let Some((node, ready)) = pending.pop() {
            if self.body_digest(node).is_some() {
                continue;
            }
            if ready {
                match node {
                    BodyNode::Expression(id) => self.hash_expression_node(id)?,
                    BodyNode::Statement(id) => self.hash_statement_node(id)?,
                };
                active.remove(&node);
                continue;
            }
            if !active.insert(node) {
                return Err(SemanticFingerprintError::CyclicBodyNode);
            }

            pending.push((node, true));
            let dependencies = self.body_dependencies(node)?;
            pending.extend(dependencies.into_iter().rev().map(|child| (child, false)));
        }
        self.body_digest(root)
            .ok_or(SemanticFingerprintError::IncompleteBodyFact("body digest"))
    }

    fn body_digest(&self, node: BodyNode) -> Option<FingerprintDigest> {
        match node {
            BodyNode::Expression(id) => self.expressions.get(&id),
            BodyNode::Statement(id) => self.statements.get(&id),
        }
        .copied()
    }

    pub(super) fn expression_digest(
        &self,
        id: ExprId,
    ) -> Result<FingerprintDigest, SemanticFingerprintError> {
        self.expressions
            .get(&id)
            .copied()
            .ok_or(SemanticFingerprintError::IncompleteBodyFact(
                "expression digest",
            ))
    }

    pub(super) fn statement_digest(
        &self,
        id: StmtId,
    ) -> Result<FingerprintDigest, SemanticFingerprintError> {
        self.statements
            .get(&id)
            .copied()
            .ok_or(SemanticFingerprintError::IncompleteBodyFact(
                "statement digest",
            ))
    }

    fn body_dependencies(&self, node: BodyNode) -> Result<Vec<BodyNode>, SemanticFingerprintError> {
        let mut children = Vec::new();
        match node {
            BodyNode::Expression(id) => {
                self.expression_dependencies(id, &mut children)?;
                let key = ExpressionKey::new(self.module, id);
                if let FactState::Known(typed) = self.body_types.expression(key)
                    && let Some(place) = typed.place()
                {
                    match place.kind() {
                        TypedPlaceKind::Local(_) => {}
                        TypedPlaceKind::StoredField { receiver, .. }
                        | TypedPlaceKind::TupleElement { receiver, .. } => {
                            children.push(BodyNode::Expression(*receiver));
                        }
                        TypedPlaceKind::Dereference { reference } => {
                            children.push(BodyNode::Expression(*reference));
                        }
                    }
                }
                if let FactState::Known(invocation) = self.body_types.invocation(key) {
                    children.extend(
                        invocation
                            .arguments()
                            .iter()
                            .map(|argument| BodyNode::Expression(argument.expression())),
                    );
                }
            }
            BodyNode::Statement(id) => {
                let kind = self
                    .hir
                    .body(id.body())
                    .statement(id)
                    .ok_or(SemanticFingerprintError::IncompleteBodyFact("statement"))?
                    .kind();
                match kind {
                    StmtKind::Let { initializer, .. } | StmtKind::Return(initializer) => {
                        children.extend(initializer.iter().copied().map(BodyNode::Expression));
                    }
                    StmtKind::Assign { target, value } => {
                        children.extend([*target, *value].map(BodyNode::Expression));
                    }
                    StmtKind::While { condition, body } => {
                        children.extend([*condition, *body].map(BodyNode::Expression));
                    }
                    StmtKind::For { iterable, body, .. } => {
                        children.extend([*iterable, *body].map(BodyNode::Expression));
                    }
                    StmtKind::Expression(value) | StmtKind::Yield(value) => {
                        children.push(BodyNode::Expression(*value));
                    }
                    StmtKind::Break | StmtKind::Continue | StmtKind::Invalid => {}
                }
            }
        }
        Ok(children)
    }

    fn expression_dependencies(
        &self,
        id: ExprId,
        children: &mut Vec<BodyNode>,
    ) -> Result<(), SemanticFingerprintError> {
        let kind = self
            .hir
            .body(id.body())
            .expression(id)
            .ok_or(SemanticFingerprintError::IncompleteBodyFact("expression"))?
            .kind();
        match kind {
            ExprKind::Block { statements } => {
                children.extend(statements.iter().copied().map(BodyNode::Statement));
            }
            ExprKind::Binary { left, right, .. } => {
                children.extend([*left, *right].map(BodyNode::Expression));
            }
            ExprKind::Unary { operand, .. }
            | ExprKind::Reference { value: operand, .. }
            | ExprKind::Try(operand)
            | ExprKind::Group(operand)
            | ExprKind::Unsafe(operand)
            | ExprKind::Result(operand)
            | ExprKind::Checked(operand)
            | ExprKind::TupleProjection {
                receiver: operand, ..
            } => {
                children.push(BodyNode::Expression(*operand));
            }
            ExprKind::Member { receiver, .. } => {
                if !self.is_type_member_owner(id) {
                    children.push(BodyNode::Expression(*receiver));
                }
            }
            ExprKind::Call(call) => {
                children.push(BodyNode::Expression(call.callee));
                call_dependencies(&call.arguments, children);
            }
            ExprKind::CatalogCall(call) => call_dependencies(&call.arguments, children),
            ExprKind::DTypeCall(call) => {
                children.push(BodyNode::Expression(call.callee));
                call_dependencies(&call.arguments, children);
            }
            ExprKind::ReduceByCandidate(candidate) => {
                match self
                    .body_types
                    .reduce_by_commitment(ExpressionKey::new(self.module, id))
                {
                    FactState::Known(ReduceByCommitment::Ordinary) => {
                        let ordinary = candidate
                            .ordinary
                            .as_ref()
                            .expect("ordinary commitment retains an ordinary candidate");
                        children.push(BodyNode::Expression(ordinary.call.callee));
                        call_dependencies(&ordinary.call.arguments, children);
                    }
                    FactState::Known(ReduceByCommitment::TensorSingle) => {
                        children.push(BodyNode::Expression(candidate.receiver));
                        let single = candidate
                            .single_reducer
                            .as_ref()
                            .expect("single commitment retains a reducer");
                        let reducer = match &single.value {
                            ReduceBySingleValue::Expression(value) => *value,
                            ReduceBySingleValue::PhasePair { reducer, .. } => *reducer,
                        };
                        children.push(BodyNode::Expression(reducer));
                    }
                    FactState::Known(ReduceByCommitment::TensorPhases) => {
                        children.push(BodyNode::Expression(candidate.receiver));
                        let phases = candidate
                            .ordered_phases
                            .as_ref()
                            .expect("phase commitment retains ordered phases");
                        children.extend(
                            phases
                                .phases
                                .iter()
                                .map(|phase| BodyNode::Expression(phase.reducer)),
                        );
                    }
                    FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                        return Err(SemanticFingerprintError::IncompleteBodyFact(
                            "reduce_by commitment",
                        ));
                    }
                }
            }
            ExprKind::Tuple(values) | ExprKind::Array(values) => {
                children.extend(values.iter().copied().map(BodyNode::Expression));
            }
            ExprKind::Map(entries) => {
                for entry in entries {
                    children.extend([entry.key, entry.value].map(BodyNode::Expression));
                }
            }
            ExprKind::StructLiteral { fields, .. } => {
                children.extend(fields.iter().map(|field| BodyNode::Expression(field.value)));
            }
            ExprKind::VariantConstructor { payload, .. } => match payload {
                ConstructorPayload::Absent => {}
                ConstructorPayload::Tuple(values) => {
                    children.extend(values.iter().copied().map(BodyNode::Expression));
                }
                ConstructorPayload::Named(fields) => {
                    children.extend(fields.iter().map(|field| BodyNode::Expression(field.value)));
                }
            },
            ExprKind::Lambda(lambda) => {
                children.extend(lambda.statements.iter().copied().map(BodyNode::Statement));
                children.extend(lambda.tail.iter().copied().map(BodyNode::Expression));
            }
            ExprKind::If(value) => {
                children.extend([value.condition, value.then_branch].map(BodyNode::Expression));
                children.extend(value.else_branch.iter().copied().map(BodyNode::Expression));
                children.extend(
                    value
                        .unknown_branch
                        .iter()
                        .copied()
                        .map(BodyNode::Expression),
                );
            }
            ExprKind::Match(value) => {
                children.push(BodyNode::Expression(value.scrutinee));
                for arm in &value.arms {
                    if let Some(guard) = &arm.guard {
                        children.push(BodyNode::Expression(guard.condition));
                    }
                    children.push(BodyNode::Expression(arm.value));
                }
            }
            ExprKind::Own(_)
            | ExprKind::Move(_)
            | ExprKind::DropOwn { .. }
            | ExprKind::Literal(_)
            | ExprKind::Name(_)
            | ExprKind::Hole
            | ExprKind::Unit
            | ExprKind::TensorSchema(_)
            | ExprKind::Invalid => {}
        }
        Ok(())
    }
}

fn call_dependencies(arguments: &[CallArgument], children: &mut Vec<BodyNode>) {
    children.extend(
        arguments
            .iter()
            .map(|argument| BodyNode::Expression(argument.value)),
    );
}
