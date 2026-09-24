use super::branching::BranchPlan;
use super::failure::FailurePlan;
use super::invocation::InvocationPlan;
use super::numeric::NumericPlan;
use super::numeric_tree::NumericTreeFrame;
use super::values::{ValueContinuation, ValuePlan};
use super::*;

pub(super) enum ExpressionPlan {
    Complete(Outcome<TypedExpression>),
    Publish {
        key: ExpressionKey,
        outcome: Outcome<TypedExpression>,
    },
    Block {
        key: ExpressionKey,
        statements: Box<[StmtId]>,
        expected: Option<TypeId>,
    },
    Forward {
        key: ExpressionKey,
        expression: ExprId,
        request: TypeRequest,
    },
    Value {
        key: ExpressionKey,
        plan: Box<ValuePlan>,
    },
    Invocation {
        key: ExpressionKey,
        plan: Box<InvocationPlan>,
    },
    Branch {
        key: ExpressionKey,
        plan: Box<BranchPlan>,
    },
    NumericTree(Box<NumericTreeFrame>),
    Numeric {
        key: ExpressionKey,
        plan: Box<NumericPlan>,
    },
    Failure {
        key: ExpressionKey,
        plan: Box<FailurePlan>,
    },
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_expression(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        expression: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        self.type_expression_steps(
            BodyContext {
                module,
                source,
                hir,
                body,
            },
            expression,
            request,
        )
    }

    pub(super) fn prepare_expression(
        &mut self,
        context: BodyContext<'_, '_>,
        expression_id: ExprId,
        request: TypeRequest,
    ) -> Result<ExpressionPlan, BodyTypingError> {
        let BodyContext {
            module,
            source,
            body,
            ..
        } = context;
        let key = ExpressionKey::new(module, expression_id);
        match self.expressions.state(&key) {
            FactState::Known(expression) => {
                return Ok(ExpressionPlan::Complete(Outcome::Known(expression.clone())));
            }
            FactState::Poisoned(poison) => {
                return Ok(ExpressionPlan::Complete(Outcome::Poisoned(poison.clone())));
            }
            FactState::NotApplicable => {
                return Ok(ExpressionPlan::Complete(Outcome::NotApplicable));
            }
            FactState::Unperformed => {}
        }
        let expression = body
            .expression(expression_id)
            .ok_or(BodyTypingError::WrongBodyExpression(expression_id))?;
        let origin = expression.origin();
        let request = if let Some(expected) = request.expected {
            match self.normalize_type_at(source, origin, expected)? {
                Outcome::Known(expected) => TypeRequest {
                    expected: Some(expected),
                    ..request
                },
                Outcome::Poisoned(poison) => {
                    let outcome = Outcome::Poisoned(poison);
                    return Ok(ExpressionPlan::Publish { key, outcome });
                }
                Outcome::Deferred => return Ok(ExpressionPlan::Complete(Outcome::Deferred)),
                Outcome::Unperformed | Outcome::NotApplicable => {
                    return Ok(ExpressionPlan::Complete(Outcome::Unperformed));
                }
            }
        } else {
            request
        };
        let kind = expression.kind().clone();
        if !self.preparing_numeric_tree && NumericTreeFrame::accepts(&kind) {
            return Ok(ExpressionPlan::NumericTree(Box::new(
                NumericTreeFrame::new(expression_id, request),
            )));
        }
        let outcome = match kind {
            ExprKind::Block { statements } => {
                return Ok(ExpressionPlan::Block {
                    key,
                    statements,
                    expected: request.expected,
                });
            }
            ExprKind::Literal(literal) => self.type_literal(source, &literal, request)?,
            ExprKind::Name(_) => self.type_name(context, expression_id, request)?,
            ExprKind::Own(_) => self.type_own(module, source, expression_id, origin)?,
            ExprKind::Move(_) => self.type_move(module, source, expression_id, origin)?,
            ExprKind::DropOwn { explicit_move, .. } => {
                self.type_drop_own(module, source, expression_id, origin, explicit_move)?
            }
            ExprKind::Unit => {
                let unit = self.primitive(PrimitiveType::Unit)?;
                Outcome::Known(self.value_expression(unit))
            }
            ExprKind::Group(inner) => {
                return Ok(ExpressionPlan::Forward {
                    key,
                    expression: inner,
                    request,
                });
            }
            ExprKind::Tuple(values) => {
                return Ok(ExpressionPlan::Value {
                    key,
                    plan: Box::new(self.prepare_tuple(values, request)?),
                });
            }
            ExprKind::Reference { mutability, value } => {
                return Ok(ExpressionPlan::Value {
                    key,
                    plan: Box::new(ValuePlan::single(
                        value,
                        ValueContinuation::Reference { value, mutability },
                    )),
                });
            }
            ExprKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => {
                return Ok(ExpressionPlan::Value {
                    key,
                    plan: Box::new(ValuePlan::single(
                        operand,
                        ValueContinuation::Dereference { operand },
                    )),
                });
            }
            ExprKind::Call(call) => {
                return Ok(ExpressionPlan::Invocation {
                    key,
                    plan: Box::new(self.prepare_call(
                        context,
                        expression_id,
                        origin,
                        &call,
                        request,
                    )?),
                });
            }
            ExprKind::DTypeCall(call) => {
                return Ok(ExpressionPlan::Invocation {
                    key,
                    plan: Box::new(self.prepare_dtype_call(
                        context,
                        expression_id,
                        origin,
                        call,
                    )?),
                });
            }
            ExprKind::Member { receiver, member } => {
                if let FactState::Known(ResolvedName::TypeMemberOwner(owner)) =
                    self.names.fact(ResolutionKey::Expression {
                        module,
                        expression: expression_id,
                    })
                {
                    let owner = owner.clone();
                    return Ok(ExpressionPlan::Publish {
                        key,
                        outcome: self.type_qualified_member_value(
                            context,
                            expression_id,
                            &member,
                            &owner,
                            self.expression_origin(source, receiver)?,
                            request,
                        )?,
                    });
                }
                return Ok(ExpressionPlan::Value {
                    key,
                    plan: Box::new(self.prepare_member(
                        context,
                        expression_id,
                        receiver,
                        member,
                    )?),
                });
            }
            ExprKind::TupleProjection { receiver, index } => {
                return Ok(ExpressionPlan::Value {
                    key,
                    plan: Box::new(ValuePlan::single(
                        receiver,
                        ValueContinuation::TupleProjection {
                            receiver,
                            index,
                            origin,
                        },
                    )),
                });
            }
            ExprKind::StructLiteral { path, fields } => {
                return Ok(ExpressionPlan::Invocation {
                    key,
                    plan: Box::new(self.prepare_struct_literal(
                        context,
                        StructLiteralRequest {
                            expression: expression_id,
                            origin,
                            head_origin: path.origin(),
                            fields: &fields,
                            ty: request,
                        },
                    )?),
                });
            }
            ExprKind::VariantConstructor { path, payload } => {
                return Ok(ExpressionPlan::Invocation {
                    key,
                    plan: Box::new(self.prepare_variant_constructor(
                        context,
                        VariantConstructorRequest {
                            expression: expression_id,
                            origin,
                            head_origin: path.origin(),
                            qualified: path.is_qualified(),
                            payload: &payload,
                            ty: request,
                        },
                    )?),
                });
            }
            ExprKind::Invalid => Outcome::Poisoned(source.invalid_poison(origin).cloned().ok_or(
                BodyTypingError::MissingOriginSpan {
                    file: source.file(),
                    origin,
                },
            )?),
            ExprKind::Binary {
                operator,
                left,
                right,
            } => {
                return Ok(ExpressionPlan::Numeric {
                    key,
                    plan: Box::new(self.prepare_checked_binary(
                        context,
                        CheckedBinaryRequest {
                            expression: expression_id,
                            origin,
                            operator,
                            left,
                            right,
                            ty: request,
                        },
                    )?),
                });
            }
            ExprKind::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } => {
                return Ok(ExpressionPlan::Numeric {
                    key,
                    plan: Box::new(self.prepare_checked_negation(
                        context,
                        expression_id,
                        origin,
                        operand,
                        request,
                    )?),
                });
            }
            ExprKind::Unary { .. } => {
                self.unsupported(TypecheckCapability::CheckOperators, source, origin)
            }
            ExprKind::If(if_expression) => {
                return Ok(ExpressionPlan::Branch {
                    key,
                    plan: Box::new(self.prepare_if(source, key, origin, if_expression, request)),
                });
            }
            ExprKind::Match(match_expression) => {
                return Ok(ExpressionPlan::Branch {
                    key,
                    plan: Box::new(self.prepare_match(
                        source,
                        key,
                        origin,
                        match_expression,
                        request,
                    )),
                });
            }
            ExprKind::Try(operand) => {
                return Ok(ExpressionPlan::Failure {
                    key,
                    plan: Box::new(self.prepare_question(
                        context,
                        expression_id,
                        origin,
                        operand,
                    )?),
                });
            }
            ExprKind::Result(block) => {
                return Ok(ExpressionPlan::Failure {
                    key,
                    plan: Box::new(self.prepare_result(
                        context,
                        expression_id,
                        origin,
                        block,
                        request,
                    )?),
                });
            }
            ExprKind::Checked(block) => {
                return Ok(ExpressionPlan::Failure {
                    key,
                    plan: Box::new(self.prepare_checked(
                        context,
                        expression_id,
                        origin,
                        block,
                        request,
                    )?),
                });
            }
            ExprKind::Hole | ExprKind::Lambda(_) => {
                self.unsupported(TypecheckCapability::CheckClosures, source, origin)
            }
            ExprKind::Array(_) | ExprKind::Map(_) => {
                self.unsupported(TypecheckCapability::CheckContainers, source, origin)
            }
            ExprKind::Unsafe(_) => {
                self.unsupported(TypecheckCapability::CheckUnsafeExpressions, source, origin)
            }
            ExprKind::ReduceByCandidate(candidate) => {
                return Ok(ExpressionPlan::Invocation {
                    key,
                    plan: Box::new(self.prepare_reduce_by_candidate(
                        context,
                        expression_id,
                        origin,
                        candidate,
                        request,
                    )?),
                });
            }
            ExprKind::CatalogCall(_) | ExprKind::TensorSchema(_) => {
                self.unsupported(TypecheckCapability::AnalyzeTensorSource, source, origin)
            }
        };
        Ok(ExpressionPlan::Publish { key, outcome })
    }

    pub(super) fn type_literal(
        &mut self,
        source: &SemanticSource<'_>,
        literal: &Literal,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let primitive = match literal.kind() {
            LiteralKind::Integer => return self.type_integer_literal(source, literal, request),
            LiteralKind::Float => match request.expected {
                Some(expected)
                    if matches!(
                        self.signatures.types().kind(expected)?,
                        TypeKind::Primitive(
                            PrimitiveType::F16
                                | PrimitiveType::Bf16
                                | PrimitiveType::F32
                                | PrimitiveType::F64
                        )
                    ) =>
                {
                    return Ok(Outcome::Known(self.value_expression(expected)));
                }
                _ => PrimitiveType::F64,
            },
            LiteralKind::String | LiteralKind::FormatString => PrimitiveType::Str,
            LiteralKind::Bool => PrimitiveType::Bool,
            LiteralKind::AxisName | LiteralKind::AxisIndex => {
                return Ok(self.unsupported(
                    TypecheckCapability::AnalyzeTensorSource,
                    source,
                    literal.origin(),
                ));
            }
            LiteralKind::Invalid => {
                return Ok(Outcome::Poisoned(
                    source.invalid_poison(literal.origin()).cloned().ok_or(
                        BodyTypingError::MissingOriginSpan {
                            file: source.file(),
                            origin: literal.origin(),
                        },
                    )?,
                ));
            }
        };
        let ty = self.primitive(primitive)?;
        Ok(Outcome::Known(self.value_expression(ty)))
    }

    pub(super) fn type_integer_literal(
        &mut self,
        source: &SemanticSource<'_>,
        literal: &Literal,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let expected = match request.expected {
            Some(expected) => match self.signatures.types().kind(expected)? {
                TypeKind::Primitive(primitive) => Some((expected, *primitive)),
                _ => None,
            },
            None => None,
        };
        let (ty, primitive) = if let Some((expected, primitive)) = expected
            && (fixed_width_integer_max(primitive).is_some()
                || matches!(primitive, PrimitiveType::Nat | PrimitiveType::PositiveNat))
        {
            (expected, primitive)
        } else {
            if !request.default_integer {
                return Ok(Outcome::Deferred);
            }
            (self.primitive(PrimitiveType::I64)?, PrimitiveType::I64)
        };

        let (negative, magnitude) = literal
            .spelling()
            .strip_prefix('-')
            .map_or((false, literal.spelling()), |digits| (true, digits));
        let zero = decimal_literal_is_zero(magnitude);
        let maximum = if negative && !zero {
            match primitive {
                PrimitiveType::I8 => Some("128"),
                PrimitiveType::I16 => Some("32768"),
                PrimitiveType::I32 => Some("2147483648"),
                PrimitiveType::I64 => Some("9223372036854775808"),
                PrimitiveType::U8
                | PrimitiveType::U16
                | PrimitiveType::U32
                | PrimitiveType::U64 => Some("0"),
                _ => None,
            }
        } else {
            fixed_width_integer_max(primitive)
        };
        if let Some(maximum) = maximum
            && !decimal_literal_fits(magnitude, maximum)
        {
            let poison = self.type_error(
                source,
                literal.origin(),
                "INT_OVERFLOW",
                "integer literal causes int overflow",
                BTreeMap::from([
                    (
                        Box::<str>::from("operation"),
                        DiagnosticDataValue::from("literal"),
                    ),
                    (
                        Box::<str>::from("dtype"),
                        DiagnosticDataValue::from(primitive.spelling()),
                    ),
                ]),
            )?;
            return Ok(Outcome::Poisoned(poison));
        }

        if matches!(primitive, PrimitiveType::Nat | PrimitiveType::PositiveNat) {
            let in_range = (!negative || zero)
                && decimal_literal_fits(magnitude, "9223372036854775807")
                && (primitive == PrimitiveType::Nat || !zero);
            if !in_range {
                let poison = self.type_error(
                    source,
                    literal.origin(),
                    "DOMAIN_INVALID_VALUE",
                    "integer literal is an invalid domain value",
                    BTreeMap::from([
                        (
                            Box::<str>::from("domain"),
                            DiagnosticDataValue::from(primitive.spelling()),
                        ),
                        (
                            Box::<str>::from("value"),
                            DiagnosticDataValue::from(literal.spelling()),
                        ),
                    ]),
                )?;
                return Ok(Outcome::Poisoned(poison));
            }
        }

        let value = literal
            .spelling()
            .replace('_', "")
            .parse::<i128>()
            .ok()
            .and_then(|value| IntegerValue::new(primitive, value))
            .ok_or(BodyTypingError::InvalidIntegerConstant)?;
        let mut typed = self.value_expression(ty);
        typed.integer_constant = Some(value);
        Ok(Outcome::Known(typed))
    }

    pub(super) fn type_name(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let key = ResolutionKey::Expression { module, expression };
        if let Some(deferred) = self.names.deferred(key).cloned() {
            let origin = self.expression_origin(source, expression)?;
            let location = SemanticLocation::origin(source.file(), origin);
            for &capability in deferred.requirements() {
                if !self.recorder.require(capability, location) {
                    return Ok(Outcome::Unperformed);
                }
            }
            let span =
                source
                    .origin_span(origin)
                    .cloned()
                    .ok_or(BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin,
                    })?;
            return match materialize_name_lookup(
                self.diagnostics,
                span,
                deferred.name(),
                deferred.decision(),
                false,
            )? {
                NameLookupMaterialization::Resolved(resolution) => {
                    self.type_resolved_name(context, expression, request, &resolution)
                }
                NameLookupMaterialization::Poisoned(poison) => Ok(Outcome::Poisoned(poison)),
                NameLookupMaterialization::Unperformed => Ok(Outcome::Unperformed),
                NameLookupMaterialization::NotApplicable => Ok(Outcome::NotApplicable),
                NameLookupMaterialization::MissingCapabilityInput(capability) => {
                    Err(BodyTypingError::MissingCapabilityInput(capability))
                }
            };
        }
        match self.names.fact(key) {
            FactState::Known(resolution) => {
                self.type_resolved_name(context, expression, request, resolution)
            }
            FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
            FactState::NotApplicable => Ok(Outcome::NotApplicable),
            FactState::Unperformed => Ok(Outcome::Unperformed),
        }
    }

    fn type_resolved_name(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        request: TypeRequest,
        resolution: &ResolvedName,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        match resolution {
            ResolvedName::Local(binding) => match self.locals.state(binding) {
                FactState::Known(local) => Ok(Outcome::Known(TypedExpression {
                    ty: local.ty,
                    integer_constant: None,
                    place: Some(TypedPlace {
                        kind: TypedPlaceKind::Local(*binding),
                        writable: local.mutability == Mutability::Mutable,
                    }),
                    normal_continuation: self.continuation_for_type(local.ty),
                })),
                FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => Ok(Outcome::Unperformed),
                FactState::NotApplicable => Ok(Outcome::NotApplicable),
            },
            ResolvedName::Declarations(targets) if targets.len() == 1 => {
                let target = targets[0].clone();
                if matches!(
                    self.external_semantic_owner(&target),
                    Some(ExternalSemanticOwner::TensorSource(_))
                ) {
                    return Ok(self.unsupported(
                        TypecheckCapability::AnalyzeTensorSource,
                        source,
                        self.expression_origin(source, expression)?,
                    ));
                }
                let signature = match self.signatures.declaration(&target) {
                    FactState::Known(signature) => signature.clone(),
                    FactState::Poisoned(poison) => {
                        return Ok(Outcome::Poisoned(poison.clone()));
                    }
                    FactState::Unperformed | FactState::NotApplicable => {
                        return Ok(Outcome::Unperformed);
                    }
                };
                match signature {
                    DeclarationSignature::Const { ty } => {
                        self.type_constant_reference(source, expression, &target, ty)
                    }
                    DeclarationSignature::Callable(callable) => self.type_named_callable_value(
                        module,
                        source,
                        expression,
                        request,
                        &[callable],
                    ),
                    DeclarationSignature::Constructor(constructor)
                        if matches!(constructor.payload(), ConstructorPayloadSignature::Unit) =>
                    {
                        self.type_intrinsic_unit_constructor(
                            context,
                            expression,
                            self.expression_origin(source, expression)?,
                            &target,
                            &constructor,
                            request,
                        )
                    }
                    _ => Ok(self.unsupported(
                        TypecheckCapability::CheckClosures,
                        source,
                        self.expression_origin(source, expression)?,
                    )),
                }
            }
            ResolvedName::Declarations(targets) => {
                let origin = self.expression_origin(source, expression)?;
                if !self.recorder.require(
                    CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
                    SemanticLocation::origin(source.file(), origin),
                ) {
                    return Ok(Outcome::Unperformed);
                }

                let mut callables = Vec::with_capacity(targets.len());
                let mut poison = None;
                let mut unperformed = false;
                for target in targets {
                    match self.signatures.declaration(target) {
                        FactState::Known(DeclarationSignature::Callable(callable)) => {
                            callables.push(*callable);
                        }
                        FactState::Poisoned(found) => {
                            let found = found.clone();
                            self.merge_poison(&mut poison, &found)?;
                        }
                        FactState::Unperformed | FactState::NotApplicable => unperformed = true,
                        FactState::Known(_) => {
                            return Ok(self.unsupported(
                                TypecheckCapability::CheckClosures,
                                source,
                                origin,
                            ));
                        }
                    }
                }
                if let Some(poison) = poison {
                    return Ok(Outcome::Poisoned(poison));
                }
                if unperformed {
                    return Ok(Outcome::Unperformed);
                }
                self.type_named_callable_value(module, source, expression, request, &callables)
            }
            ResolvedName::TypeMemberOwner(_) => {
                Err(BodyTypingError::WrongBodyExpression(expression))
            }
        }
    }
}
