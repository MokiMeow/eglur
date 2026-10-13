use super::*;

pub(super) enum NumericPlan {
    Complete(Outcome<TypedExpression>),
    Expression {
        expression: ExprId,
        request: TypeRequest,
        path: NormalContinuation,
        continuation: NumericContinuation,
    },
}

pub(super) enum NumericContinuation {
    BinaryLeft {
        frame: BinaryFrame,
        right: ExprId,
        expected: Option<ExpectedType>,
    },
    BinaryRight {
        frame: BinaryFrame,
        left: Outcome<TypedExpression>,
    },
    Negation(NegationFrame),
}

pub(super) struct BinaryFrame {
    key: ExpressionKey,
    origin: OriginId,
    operator: BinaryOperator,
    symbol: &'static str,
    left_const: bool,
    right_const: bool,
}

pub(super) struct NegationFrame {
    key: ExpressionKey,
    origin: OriginId,
    is_const: bool,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn resume_numeric(
        &mut self,
        source: &SemanticSource<'_>,
        continuation: NumericContinuation,
        outcome: Outcome<TypedExpression>,
    ) -> Result<NumericPlan, BodyFlow> {
        let outcome = match continuation {
            NumericContinuation::BinaryLeft {
                frame,
                right,
                expected,
            } => {
                let right_expected = match &outcome {
                    Outcome::Known(left) if self.is_fixed_width_integer(left.ty)? => {
                        Some(expected.map_or_else(
                            || ExpectedType::caller(left.ty),
                            |ty| ty.with_type(left.ty),
                        ))
                    }
                    _ => expected,
                };
                let path = match &outcome {
                    Outcome::Known(left) => left.normal_continuation,
                    _ => NormalContinuation::Absent,
                };
                return Ok(NumericPlan::Expression {
                    expression: right,
                    request: TypeRequest::definitive(right_expected),
                    path,
                    continuation: NumericContinuation::BinaryRight {
                        frame,
                        left: outcome,
                    },
                });
            }
            NumericContinuation::BinaryRight { frame, left } => {
                self.finish_checked_binary(source, frame, left, outcome)?
            }
            NumericContinuation::Negation(frame) => {
                self.finish_checked_negation(source, frame, outcome)?
            }
        };
        Ok(NumericPlan::Complete(outcome))
    }

    pub(super) fn prepare_checked_binary(
        &mut self,
        context: BodyContext<'_, '_>,
        request: CheckedBinaryRequest,
    ) -> Result<NumericPlan, BodyFlow> {
        let BodyContext {
            module,
            source,
            body,
            ..
        } = context;
        let CheckedBinaryRequest {
            expression,
            origin,
            operator,
            left,
            right,
            ty: request,
        } = request;
        let Some(symbol) = operator.numeric_symbol() else {
            return Ok(NumericPlan::Complete(self.unsupported(
                TypecheckCapability::CheckOperators,
                source,
                origin,
            )));
        };
        if !self.recorder.require(
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(NumericPlan::Complete(Outcome::Unperformed));
        }
        let left_const = self.is_const_evaluable_expression(module, body, left)?;
        let right_const = self.is_const_evaluable_expression(module, body, right)?;
        let expected = match request.expected {
            Some(ty) if self.is_fixed_width_integer(ty.ty())? => Some(ty),
            _ => None,
        };
        Ok(NumericPlan::Expression {
            expression: left,
            request: TypeRequest::definitive(expected),
            path: NormalContinuation::Present,
            continuation: NumericContinuation::BinaryLeft {
                frame: BinaryFrame {
                    key: ExpressionKey::new(module, expression),
                    origin,
                    operator,
                    symbol,
                    left_const,
                    right_const,
                },
                right,
                expected,
            },
        })
    }

    fn finish_checked_binary(
        &mut self,
        source: &SemanticSource<'_>,
        frame: BinaryFrame,
        left: Outcome<TypedExpression>,
        right: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
        let BinaryFrame {
            key,
            origin,
            operator,
            symbol,
            left_const,
            right_const,
        } = frame;
        let (left, right) = match self.known_binary_operands(left, right)? {
            Outcome::Known(operands) => operands,
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred => return Ok(Outcome::Deferred),
            Outcome::Unperformed | Outcome::NotApplicable => return Ok(Outcome::Unperformed),
        };
        if self.is_nonnumeric_operand(left.ty)? || self.is_nonnumeric_operand(right.ty)? {
            let left_type = self.format_type(left.ty)?;
            let right_type = self.format_type(right.ty)?;
            return Ok(Outcome::Poisoned(self.type_error(
                source,
                origin,
                "NUMERIC_OPERAND_NOT_NUMERIC",
                format!("numeric {symbol} operand types are {left_type} and {right_type}"),
                BTreeMap::from([
                    ("reason".into(), "binary_operand".into()),
                    ("operation".into(), symbol.into()),
                    ("left_type".into(), left_type.into()),
                    ("right_type".into(), right_type.into()),
                ]),
            )?));
        }
        let Some(operator) = CheckedNumericOperator::from_binary(operator) else {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        };
        let (TypeKind::Primitive(left_dtype), TypeKind::Primitive(right_dtype)) = (
            self.signatures.types().kind(left.ty)?,
            self.signatures.types().kind(right.ty)?,
        ) else {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        };
        if !left_dtype.is_fixed_width_integer() || !right_dtype.is_fixed_width_integer() {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        }
        let destination = left_dtype
            .promote_numeric(*right_dtype)
            .ok_or(BodyTypingError::InvalidIntegerConstant)?;
        if (left_const || right_const) && !self.require_integer_const(source, origin) {
            return Ok(Outcome::Unperformed);
        }
        let conversion_can_fail = !self.integer_conversion_is_total(&left, destination)?
            || !self.integer_conversion_is_total(&right, destination)?;
        let result_type = self.primitive(destination)?;
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
                left.checked(operator, destination, Some(right)),
                destination,
            );
        }
        let signed = self.is_signed_fixed_width_integer(result_type)?;
        let total = !conversion_can_fail
            && right.integer_constant.is_some_and(|right| match operator {
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
            key,
            operator,
            result_type,
            continuation,
            total,
            conversion_can_fail,
        )?;
        Ok(Outcome::Known(self.value_expression_with_continuation(
            result_type,
            continuation,
        )))
    }

    pub(super) fn prepare_checked_negation(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        operand: ExprId,
        request: TypeRequest,
    ) -> Result<NumericPlan, BodyFlow> {
        let BodyContext {
            module,
            source,
            body,
            ..
        } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(NumericPlan::Complete(Outcome::Unperformed));
        }
        let is_const = self.is_const_evaluable_expression(module, body, operand)?;
        let expected = match request.expected {
            Some(ty) if self.is_fixed_width_integer(ty.ty())? => Some(ty),
            _ => None,
        };
        Ok(NumericPlan::Expression {
            expression: operand,
            request: TypeRequest::definitive(expected),
            path: NormalContinuation::Present,
            continuation: NumericContinuation::Negation(NegationFrame {
                key: ExpressionKey::new(module, expression),
                origin,
                is_const,
            }),
        })
    }

    fn finish_checked_negation(
        &mut self,
        source: &SemanticSource<'_>,
        frame: NegationFrame,
        operand: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
        let NegationFrame {
            key,
            origin,
            is_const,
        } = frame;
        let Outcome::Known(operand) = operand else {
            return Ok(operand);
        };
        if self.is_nonnumeric_operand(operand.ty)? {
            let actual = self.format_type(operand.ty)?;
            return Ok(Outcome::Poisoned(self.type_error(
                source,
                origin,
                "NUMERIC_OPERAND_NOT_NUMERIC",
                format!("numeric - operand has type {actual}"),
                BTreeMap::from([
                    ("reason".into(), "unary_operand".into()),
                    ("operation".into(), "-".into()),
                    ("actual_type".into(), actual.into()),
                ]),
            )?));
        }
        if !self.is_fixed_width_integer(operand.ty)? {
            return Ok(self.unsupported(TypecheckCapability::CheckOperators, source, origin));
        }
        if is_const && !self.require_integer_const(source, origin) {
            return Ok(Outcome::Unperformed);
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
            key,
            CheckedNumericOperator::Negate,
            operand.ty,
            operand.normal_continuation,
            false,
            false,
        )?;
        Ok(Outcome::Known(self.value_expression_with_continuation(
            operand.ty,
            operand.normal_continuation,
        )))
    }

    fn is_nonnumeric_operand(&self, ty: TypeId) -> Result<bool, BodyFlow> {
        Ok(match self.signatures.types().kind(ty)? {
            TypeKind::Primitive(dtype) => !dtype.is_numeric(),
            TypeKind::Never
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_)
            | TypeKind::AssociatedProjection(_) => false,
            TypeKind::Nominal { .. }
            | TypeKind::Reference { .. }
            | TypeKind::RawPointer { .. }
            | TypeKind::Tuple(_)
            | TypeKind::Callable { .. }
            | TypeKind::ErrorUnion(_) => true,
        })
    }

    fn integer_conversion_is_total(
        &self,
        value: &TypedExpression,
        destination: PrimitiveType,
    ) -> Result<bool, BodyFlow> {
        if let Some(value) = value.integer_constant {
            return Ok(IntegerValue::new(destination, value.value()).is_some());
        }
        let TypeKind::Primitive(source) = self.signatures.types().kind(value.ty)? else {
            return Err(BodyFlow::Internal(BodyTypingError::InvalidIntegerConstant));
        };
        let (Some((source_min, source_max)), Some((target_min, target_max))) =
            (source.integer_bounds(), destination.integer_bounds())
        else {
            return Err(BodyFlow::Internal(BodyTypingError::InvalidIntegerConstant));
        };
        Ok(target_min <= source_min && source_max <= target_max)
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
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
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
            IntegerEvaluation::Invalid => {
                Err(BodyFlow::Internal(BodyTypingError::InvalidIntegerConstant))
            }
        }
    }

    fn numeric_checked_required(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        operator: CheckedNumericOperator,
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
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
