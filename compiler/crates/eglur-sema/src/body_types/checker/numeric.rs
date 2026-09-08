use super::*;

enum NumericTypeStep {
    Enter(ExprId, TypeRequest, NormalContinuation),
    AfterLeft {
        expression: ExprId,
        left: ExprId,
        right: ExprId,
        request: TypeRequest,
        continuation: NormalContinuation,
    },
    Finish(ExprId, TypeRequest, NormalContinuation),
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_numeric_tree(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        request: TypeRequest,
    ) -> Result<Option<Outcome<TypedExpression>>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if matches!(body.expression(expression).ok_or(BodyTypingError::WrongBodyExpression(expression))?.kind(),
            ExprKind::Binary { operator, .. } if CheckedNumericOperator::from_binary(*operator).is_none())
        {
            return Ok(None);
        }
        self.preparing_numeric_tree = true;
        // Right operands inherit both the left type and its normal continuation.
        let result = (|| {
            let mut pending = vec![NumericTypeStep::Enter(
                expression,
                request,
                NormalContinuation::Present,
            )];
            let mut outcome = Outcome::Unperformed;
            while let Some(step) = pending.pop() {
                let (current, request, continuation, finishing) = match step {
                    NumericTypeStep::Enter(current, request, continuation) => {
                        let kind = body
                            .expression(current)
                            .ok_or(BodyTypingError::WrongBodyExpression(current))?
                            .kind();
                        let expected = match request.expected {
                            Some(ty) if self.is_fixed_width_integer(ty)? => Some(ty),
                            _ => None,
                        };
                        match kind {
                            ExprKind::Group(inner) => {
                                pending.push(NumericTypeStep::Finish(
                                    current,
                                    request,
                                    continuation,
                                ));
                                pending.push(NumericTypeStep::Enter(*inner, request, continuation));
                                continue;
                            }
                            ExprKind::Unary {
                                operator: UnaryOperator::Negate,
                                operand,
                            } => {
                                pending.push(NumericTypeStep::Finish(
                                    current,
                                    request,
                                    continuation,
                                ));
                                pending.push(NumericTypeStep::Enter(
                                    *operand,
                                    TypeRequest::definitive(expected),
                                    continuation,
                                ));
                                continue;
                            }
                            ExprKind::Binary {
                                operator,
                                left,
                                right,
                            } if CheckedNumericOperator::from_binary(*operator).is_some() => {
                                pending.push(NumericTypeStep::AfterLeft {
                                    expression: current,
                                    left: *left,
                                    right: *right,
                                    request,
                                    continuation,
                                });
                                pending.push(NumericTypeStep::Enter(
                                    *left,
                                    TypeRequest::definitive(expected),
                                    continuation,
                                ));
                                continue;
                            }
                            _ => (current, request, continuation, false),
                        }
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
                            _ => match request.expected {
                                Some(ty) if self.is_fixed_width_integer(ty)? => Some(ty),
                                _ => None,
                            },
                        };
                        let right_path = continuation.strict(match left {
                            FactState::Known(left) => left.normal_continuation,
                            _ => NormalContinuation::Absent,
                        });
                        pending.push(NumericTypeStep::Finish(expression, request, continuation));
                        pending.push(NumericTypeStep::Enter(
                            right,
                            TypeRequest::definitive(expected),
                            right_path,
                        ));
                        continue;
                    }
                    NumericTypeStep::Finish(current, request, continuation) => {
                        (current, request, continuation, true)
                    }
                };
                self.preparing_numeric_tree = finishing;
                let checked = self.with_control_path(continuation, |checker| {
                    checker.type_expression(module, source, hir, body, current, request)
                });
                self.preparing_numeric_tree = true;
                outcome = checked?;
                if matches!(
                    outcome,
                    Outcome::Deferred | Outcome::Unperformed | Outcome::NotApplicable
                ) {
                    break;
                }
            }
            Ok(Some(outcome))
        })();
        self.preparing_numeric_tree = false;
        result
    }

    pub(super) fn type_checked_binary(
        &mut self,
        context: BodyContext<'_, '_>,
        request: CheckedBinaryRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        let CheckedBinaryRequest {
            expression,
            origin,
            operator,
            left,
            right,
            ty: request,
        } = request;
        if !self.recorder.require(
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let Some(operator) = CheckedNumericOperator::from_binary(operator) else {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        };
        let left_const = self.is_const_evaluable_expression(module, body, left)?;
        let right_const = self.is_const_evaluable_expression(module, body, right)?;
        if (left_const || right_const) && !self.require_integer_const(source, origin) {
            return Ok(Outcome::Unperformed);
        }
        let expected = match request.expected {
            Some(ty) if self.is_fixed_width_integer(ty)? => Some(ty),
            _ => None,
        };
        let left = self.type_expression(
            module,
            source,
            hir,
            body,
            left,
            TypeRequest::definitive(expected),
        )?;
        let right_expected = match &left {
            Outcome::Known(left) if self.is_fixed_width_integer(left.ty)? => Some(left.ty),
            _ => expected,
        };
        let left_continuation = match &left {
            Outcome::Known(left) => left.normal_continuation,
            _ => NormalContinuation::Absent,
        };
        let right = self.with_control_path(left_continuation, |checker| {
            checker.type_expression(
                module,
                source,
                hir,
                body,
                right,
                TypeRequest::definitive(right_expected),
            )
        })?;
        let (left, right) = match self.known_binary_operands(left, right)? {
            Outcome::Known(operands) => operands,
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred => return Ok(Outcome::Deferred),
            Outcome::Unperformed | Outcome::NotApplicable => return Ok(Outcome::Unperformed),
        };
        if left.ty != right.ty || !self.is_fixed_width_integer(left.ty)? {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        }
        if (left_const && left.integer_constant.is_none())
            || (right_const && right.integer_constant.is_none())
        {
            return Ok(self.unsupported(
                TypecheckCapability::EvaluateScalarConstExpressions,
                source,
                origin,
            ));
        }
        if let (Some(left), Some(right)) = (left.integer_constant, right.integer_constant) {
            return self.finish_integer_constant(
                source,
                origin,
                operator,
                left.checked(operator, left.primitive(), Some(right)),
                left.primitive(),
            );
        }
        let signed = self.is_signed_fixed_width_integer(left.ty)?;
        let total = right.integer_constant.is_some_and(|right| match operator {
            CheckedNumericOperator::Remainder => right.value() != 0,
            CheckedNumericOperator::Divide => {
                right.value() != 0 && (!signed || right.value() != -1)
            }
            _ => false,
        });
        if !total && !self.has_active_checked_context() {
            return self.numeric_checked_required(source, origin, operator);
        }
        let continuation = left.normal_continuation.strict(right.normal_continuation);
        self.record_checked_numeric_operation(
            module,
            expression,
            operator,
            left.ty,
            continuation,
            total,
        )?;
        Ok(Outcome::Known(
            self.value_expression_with_continuation(left.ty, continuation),
        ))
    }

    pub(super) fn type_checked_negation(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        operand: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let is_const = self.is_const_evaluable_expression(module, body, operand)?;
        if is_const && !self.require_integer_const(source, origin) {
            return Ok(Outcome::Unperformed);
        }
        let expected = match request.expected {
            Some(ty) if self.is_fixed_width_integer(ty)? => Some(ty),
            _ => None,
        };
        let operand = self.type_expression(
            module,
            source,
            hir,
            body,
            operand,
            TypeRequest::definitive(expected),
        )?;
        let Outcome::Known(operand) = operand else {
            return Ok(operand);
        };
        if !self.is_fixed_width_integer(operand.ty)? {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        }
        if let Some(value) = operand.integer_constant {
            return self.finish_integer_constant(
                source,
                origin,
                CheckedNumericOperator::Negate,
                value.checked(CheckedNumericOperator::Negate, value.primitive(), None),
                value.primitive(),
            );
        }
        if is_const {
            return Ok(self.unsupported(
                TypecheckCapability::EvaluateScalarConstExpressions,
                source,
                origin,
            ));
        }
        if !self.has_active_checked_context() {
            return self.numeric_checked_required(source, origin, CheckedNumericOperator::Negate);
        }
        self.record_checked_numeric_operation(
            module,
            expression,
            CheckedNumericOperator::Negate,
            operand.ty,
            operand.normal_continuation,
            false,
        )?;
        Ok(Outcome::Known(self.value_expression_with_continuation(
            operand.ty,
            operand.normal_continuation,
        )))
    }

    fn require_integer_const(&mut self, source: &SemanticSource<'_>, origin: OriginId) -> bool {
        self.recorder.require(
            CompileCapability::Typecheck(
                TypecheckCapability::EvaluateSelectedIntegerConstExpressions,
            ),
            SemanticLocation::origin(source.file(), origin),
        )
    }

    fn finish_integer_constant(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        operator: CheckedNumericOperator,
        evaluation: IntegerEvaluation,
        primitive: PrimitiveType,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        match evaluation {
            IntegerEvaluation::Value(value) => {
                let ty = self.primitive(value.primitive())?;
                let mut typed = self.value_expression(ty);
                typed.integer_constant = Some(value);
                Ok(Outcome::Known(typed))
            }
            IntegerEvaluation::Fault(fault) => {
                let (code, message) = match fault {
                    NumericFaultCause::Overflow => ("INT_OVERFLOW", "integer overflow"),
                    NumericFaultCause::DivisionByZero => {
                        ("INT_DIV_BY_ZERO", "integer division by zero")
                    }
                    NumericFaultCause::RemainderByZero => {
                        ("INT_REM_BY_ZERO", "integer remainder by zero")
                    }
                };
                Ok(Outcome::Poisoned(self.type_error(
                    source,
                    origin,
                    code,
                    message,
                    BTreeMap::from([
                        (
                            Box::<str>::from("operation"),
                            DiagnosticDataValue::from(operator.diagnostic_operation()),
                        ),
                        (
                            Box::<str>::from("dtype"),
                            DiagnosticDataValue::from(primitive.spelling()),
                        ),
                    ]),
                )?))
            }
            IntegerEvaluation::Invalid => Err(BodyTypingError::InvalidIntegerConstant),
        }
    }

    fn numeric_checked_required(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        operator: CheckedNumericOperator,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let symbol = match operator {
            CheckedNumericOperator::Add => "+",
            CheckedNumericOperator::Subtract | CheckedNumericOperator::Negate => "-",
            CheckedNumericOperator::Multiply => "*",
            CheckedNumericOperator::Divide => "/",
            CheckedNumericOperator::Remainder => "%",
        };
        Ok(Outcome::Poisoned(self.type_error(
            source,
            origin,
            "NUMERIC_CHECKED_CONTEXT_REQUIRED",
            format!("numeric {symbol} requires a checked block"),
            BTreeMap::new(),
        )?))
    }
}
