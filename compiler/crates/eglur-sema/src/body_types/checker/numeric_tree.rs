use super::*;

enum NumericTypeStep {
    Enter(ExprId, TypeRequest, NormalContinuation),
    AfterRightProbe {
        expression: ExprId,
        left: ExprId,
        right: ExprId,
        request: TypeRequest,
        continuation: NormalContinuation,
    },
    AfterLeft {
        expression: ExprId,
        left: ExprId,
        right: ExprId,
        request: TypeRequest,
        continuation: NormalContinuation,
    },
    Finish(ExprId, TypeRequest, NormalContinuation),
}

pub(super) struct NumericTreeFrame {
    pending: Vec<NumericTypeStep>,
    literal_trees: BTreeMap<ExprId, bool>,
    outcome: Outcome<TypedExpression>,
}

pub(super) enum NumericTreePlan {
    Complete(Outcome<TypedExpression>),
    Expression {
        expression: ExprId,
        request: TypeRequest,
        path: NormalContinuation,
        finishing: bool,
        frame: Box<NumericTreeFrame>,
    },
}

impl NumericTreeFrame {
    pub(super) fn accepts(kind: &ExprKind) -> bool {
        match kind {
            ExprKind::Binary { operator, .. } => operator.numeric_symbol().is_some(),
            ExprKind::Group(_)
            | ExprKind::Unary {
                operator: UnaryOperator::Negate,
                ..
            } => true,
            _ => false,
        }
    }

    pub(super) fn new(expression: ExprId, request: TypeRequest) -> Self {
        Self {
            pending: vec![NumericTypeStep::Enter(
                expression,
                request,
                NormalContinuation::Present,
            )],
            literal_trees: BTreeMap::new(),
            outcome: Outcome::Unperformed,
        }
    }

    pub(super) fn accept_child(&mut self, outcome: Outcome<TypedExpression>) {
        match &outcome {
            Outcome::Deferred => {
                // An inconclusive opposite-operand probe resumes at its caller.
                while self
                    .pending
                    .last()
                    .is_some_and(|step| !matches!(step, NumericTypeStep::AfterRightProbe { .. }))
                {
                    self.pending.pop();
                }
            }
            Outcome::Unperformed | Outcome::NotApplicable => self.pending.clear(),
            Outcome::Known(_) | Outcome::Poisoned(_) => {}
        }
        self.outcome = outcome;
    }

    fn is_integer_literal_tree(
        &mut self,
        body: &Body,
        expression: ExprId,
    ) -> Result<bool, BodyFlow> {
        let known = &mut self.literal_trees;
        let mut pending = vec![(expression, false)];
        while let Some((current, finishing)) = pending.pop() {
            if known.contains_key(&current) {
                continue;
            }
            let children = match body
                .expression(current)
                .ok_or(BodyTypingError::WrongBodyExpression(current))?
                .kind()
            {
                ExprKind::Group(inner) => [Some(*inner), None],
                ExprKind::Unary {
                    operator: UnaryOperator::Negate,
                    operand,
                } => [Some(*operand), None],
                ExprKind::Binary {
                    operator,
                    left,
                    right,
                } if CheckedNumericOperator::from_binary(*operator).is_some() => {
                    [Some(*left), Some(*right)]
                }
                ExprKind::Literal(literal) if literal.kind() == LiteralKind::Integer => {
                    known.insert(current, true);
                    continue;
                }
                _ => {
                    known.insert(current, false);
                    continue;
                }
            };
            if finishing {
                known.insert(
                    current,
                    children.into_iter().flatten().all(|child| known[&child]),
                );
            } else {
                pending.push((current, true));
                pending.extend(children.into_iter().flatten().map(|child| (child, false)));
            }
        }
        Ok(known[&expression])
    }
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn advance_numeric_tree(
        &mut self,
        context: BodyContext<'_, '_>,
        mut frame: Box<NumericTreeFrame>,
    ) -> Result<NumericTreePlan, BodyFlow> {
        let BodyContext { module, body, .. } = context;
        while let Some(step) = frame.pending.pop() {
            let (expression, request, path, finishing) = match step {
                NumericTypeStep::Enter(current, request, continuation) => {
                    match self.expressions.state(&ExpressionKey::new(module, current)) {
                        FactState::Known(value) => {
                            frame.outcome = Outcome::Known(value.clone());
                            continue;
                        }
                        FactState::Poisoned(poison) => {
                            frame.outcome = Outcome::Poisoned(poison.clone());
                            continue;
                        }
                        FactState::NotApplicable => {
                            frame.outcome = Outcome::NotApplicable;
                            break;
                        }
                        FactState::Unperformed => {}
                    }
                    let kind = body
                        .expression(current)
                        .ok_or(BodyTypingError::WrongBodyExpression(current))?
                        .kind();
                    let expected = match request.expected_type() {
                        Some(ty) if self.is_fixed_width_integer(ty)? => Some(ty),
                        _ => None,
                    };
                    match kind {
                        ExprKind::Group(inner) => {
                            frame.pending.push(NumericTypeStep::Finish(
                                current,
                                request,
                                continuation,
                            ));
                            frame.pending.push(NumericTypeStep::Enter(
                                *inner,
                                request,
                                continuation,
                            ));
                            continue;
                        }
                        ExprKind::Unary {
                            operator: UnaryOperator::Negate,
                            operand,
                        } => {
                            frame.pending.push(NumericTypeStep::Finish(
                                current,
                                request,
                                continuation,
                            ));
                            frame.pending.push(NumericTypeStep::Enter(
                                *operand,
                                request.definitive_child(expected),
                                continuation,
                            ));
                            continue;
                        }
                        ExprKind::Binary {
                            operator,
                            left,
                            right,
                        } if operator.numeric_symbol().is_some() => {
                            if expected.is_none() && frame.is_integer_literal_tree(body, *left)? {
                                // A literal-only left side has no effects to reorder.
                                frame.pending.push(NumericTypeStep::AfterRightProbe {
                                    expression: current,
                                    left: *left,
                                    right: *right,
                                    request,
                                    continuation,
                                });
                                frame.pending.push(NumericTypeStep::Enter(
                                    *right,
                                    TypeRequest::probe(request.custody()),
                                    continuation,
                                ));
                                continue;
                            }
                            frame.pending.push(NumericTypeStep::AfterLeft {
                                expression: current,
                                left: *left,
                                right: *right,
                                request,
                                continuation,
                            });
                            frame.pending.push(NumericTypeStep::Enter(
                                *left,
                                request.definitive_child(expected),
                                continuation,
                            ));
                            continue;
                        }
                        _ => (current, request, continuation, false),
                    }
                }
                NumericTypeStep::AfterRightProbe {
                    expression,
                    left,
                    right,
                    request,
                    continuation,
                } => {
                    let expected = match self.expressions.state(&ExpressionKey::new(module, right))
                    {
                        FactState::Known(right) if self.is_fixed_width_integer(right.ty)? => {
                            Some(right.ty)
                        }
                        _ => None,
                    };
                    frame.pending.push(NumericTypeStep::AfterLeft {
                        expression,
                        left,
                        right,
                        request,
                        continuation,
                    });
                    frame.pending.push(NumericTypeStep::Enter(
                        left,
                        request.definitive_child(expected),
                        continuation,
                    ));
                    continue;
                }
                NumericTypeStep::AfterLeft {
                    expression,
                    left,
                    right,
                    request,
                    continuation,
                } => {
                    let left = self.expressions.state(&ExpressionKey::new(module, left));
                    let expected = match left {
                        FactState::Known(left) if self.is_fixed_width_integer(left.ty)? => {
                            Some(left.ty)
                        }
                        _ => match request.expected_type() {
                            Some(ty) if self.is_fixed_width_integer(ty)? => Some(ty),
                            _ => None,
                        },
                    };
                    let right_path = continuation.strict(match left {
                        FactState::Known(left) => left.normal_continuation,
                        _ => NormalContinuation::Absent,
                    });
                    frame
                        .pending
                        .push(NumericTypeStep::Finish(expression, request, continuation));
                    frame.pending.push(NumericTypeStep::Enter(
                        right,
                        request.definitive_child(expected),
                        right_path,
                    ));
                    continue;
                }
                NumericTypeStep::Finish(current, request, continuation) => {
                    (current, request, continuation, true)
                }
            };
            return Ok(NumericTreePlan::Expression {
                expression,
                request,
                path,
                finishing,
                frame,
            });
        }
        Ok(NumericTreePlan::Complete(frame.outcome))
    }
}
