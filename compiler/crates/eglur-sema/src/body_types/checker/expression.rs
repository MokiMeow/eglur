use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_expression_in_region(
        &mut self,
        region: ControlRegionKey,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        self.enter_control_region(region.clone())?;
        let outcome =
            self.type_expression_in_current_region(module, source, hir, body, expression, request);
        self.leave_control_region(&region)?;
        outcome
    }

    pub(super) fn type_expression_in_current_region(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        expression: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let is_block = matches!(
            body.expression(expression)
                .ok_or(BodyTypingError::WrongBodyExpression(expression))?
                .kind(),
            ExprKind::Block { .. }
        );
        if is_block {
            self.block_region_overrides.push(expression);
        }
        let outcome = self.type_expression(module, source, hir, body, expression, request);
        if is_block {
            let removed = self
                .block_region_overrides
                .pop()
                .ok_or(BodyTypingError::MissingBlockRegionOverride)?;
            if removed != expression {
                return Err(BodyTypingError::BlockRegionOverrideMismatch {
                    expected: expression,
                    actual: removed,
                });
            }
        }
        let outcome = outcome?;
        if !is_block
            && matches!(
                &outcome,
                Outcome::Known(value) if value.normal_continuation.is_present()
            )
        {
            let region = self.current_control_region()?.clone();
            self.record_control_transfer(
                ControlTransferSource::RegionCompletion(region.clone()),
                ControlTransferKind::NormalCompletion,
                region,
            )?;
        }
        Ok(outcome)
    }

    pub(super) fn type_block_expression(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        statements: &[StmtId],
        expected: Option<TypeId>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if self.block_region_overrides.last() == Some(&expression) {
            return self.type_block(module, source, hir, body, statements, expected);
        }
        let region = ControlRegionKey::Block(ExpressionKey::new(module, expression));
        self.enter_control_region(region.clone())?;
        let outcome = self.type_block(module, source, hir, body, statements, expected);
        self.leave_control_region(&region)?;
        outcome
    }

    pub(super) fn type_expression(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        expression_id: ExprId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let key = ExpressionKey::new(module, expression_id);
        match self.expressions.state(&key) {
            FactState::Known(expression) => return Ok(Outcome::Known(expression.clone())),
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::NotApplicable => return Ok(Outcome::NotApplicable),
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
                    self.record_expression(key, outcome.clone())?;
                    return Ok(outcome);
                }
                Outcome::Deferred => return Ok(Outcome::Deferred),
                Outcome::Unperformed | Outcome::NotApplicable => {
                    return Ok(Outcome::Unperformed);
                }
            }
        } else {
            request
        };
        let kind = expression.kind().clone();
        let context = BodyContext {
            module,
            source,
            hir,
            body,
        };
        if !self.preparing_integer_constants
            && matches!(
                kind,
                ExprKind::Binary { .. }
                    | ExprKind::Unary {
                        operator: UnaryOperator::Negate,
                        ..
                    }
                    | ExprKind::Group(_)
            )
            && let Some(outcome) =
                self.type_integer_constant_tree(context, expression_id, request)?
        {
            return Ok(outcome);
        }
        let outcome = match kind {
            ExprKind::Block { statements } => {
                self.type_block_expression(context, expression_id, &statements, request.expected)?
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
                self.type_expression(module, source, hir, body, inner, request)?
            }
            ExprKind::Tuple(values) => {
                self.type_tuple(module, source, hir, body, &values, request)?
            }
            ExprKind::Reference { mutability, value } => {
                self.type_reference(module, source, hir, body, value, mutability)?
            }
            ExprKind::Unary {
                operator: UnaryOperator::Dereference,
                operand,
            } => self.type_dereference(module, source, hir, body, operand)?,
            ExprKind::Call(call) => {
                self.type_call(context, expression_id, origin, &call, request)?
            }
            ExprKind::DTypeCall(call) => {
                self.type_dtype_call(context, expression_id, origin, &call)?
            }
            ExprKind::Member { receiver, member } => {
                self.type_member(context, expression_id, receiver, &member)?
            }
            ExprKind::TupleProjection { receiver, index } => self.type_tuple_projection(
                module,
                source,
                hir,
                body,
                receiver,
                TupleProjectionRequest {
                    index: &index,
                    origin,
                },
            )?,
            ExprKind::StructLiteral { path, fields } => self.type_struct_literal(
                context,
                StructLiteralRequest {
                    expression: expression_id,
                    origin,
                    head_origin: path.origin(),
                    fields: &fields,
                    ty: request,
                },
            )?,
            ExprKind::VariantConstructor { path, payload } => self.type_variant_constructor(
                context,
                VariantConstructorRequest {
                    expression: expression_id,
                    origin,
                    head_origin: path.origin(),
                    qualified: path.is_qualified(),
                    payload: &payload,
                    ty: request,
                },
            )?,
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
            } => self.type_checked_binary(
                context,
                CheckedBinaryRequest {
                    expression: expression_id,
                    origin,
                    operator,
                    left,
                    right,
                    ty: request,
                },
            )?,
            ExprKind::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } => self.type_checked_negation(context, expression_id, origin, operand, request)?,
            ExprKind::Unary { .. } => {
                self.unsupported(TypecheckCapability::CheckOperators, source, origin)
            }
            ExprKind::Match(match_expression) if match_expression.arms.is_empty() => {
                self.type_empty_match(context, origin, match_expression.scrutinee)?
            }
            ExprKind::If(if_expression) => {
                self.type_if(context, expression_id, origin, &if_expression, request)?
            }
            ExprKind::Match(match_expression) => {
                self.type_match(context, expression_id, origin, &match_expression, request)?
            }
            ExprKind::Try(operand) => {
                self.type_question(context, expression_id, origin, operand)?
            }
            ExprKind::Result(block) => {
                self.type_result(context, expression_id, origin, block, request)?
            }
            ExprKind::Checked(block) => {
                self.type_checked(context, expression_id, origin, block, request)?
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
                self.type_reduce_by_candidate(context, expression_id, origin, &candidate, request)?
            }
            ExprKind::CatalogCall(_) | ExprKind::TensorSchema(_) => {
                self.unsupported(TypecheckCapability::AnalyzeTensorSource, source, origin)
            }
        };
        self.record_expression(key, outcome.clone())?;
        Ok(outcome)
    }

    fn type_reduce_by_candidate(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        candidate: &ReduceByCandidateExpr,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        let mut receiver = candidate.receiver;
        let canonical = loop {
            let receiver_expression = body
                .expression(receiver)
                .ok_or(BodyTypingError::WrongBodyExpression(receiver))?;
            match receiver_expression.kind() {
                ExprKind::TensorSchema(schema) => {
                    break schema.family == TensorSchemaFamily::Reduce;
                }
                ExprKind::Group(inner) => receiver = *inner,
                _ => break false,
            }
        };

        if canonical {
            let commitment = if candidate
                .ordered_phases
                .as_ref()
                .is_some_and(|phases| !phases.phases.is_empty())
            {
                Some(ReduceByCommitment::TensorPhases)
            } else if candidate.single_reducer.is_some() {
                Some(ReduceByCommitment::TensorSingle)
            } else {
                None
            };
            let Some(commitment) = commitment else {
                return self.invalid_reduce_by_arguments(source, candidate.arguments_origin);
            };
            self.reduce_by_commitments.insert(
                ExpressionKey::new(module, expression),
                SemanticFact::Known(commitment),
            )?;
            return Ok(self.unsupported(TypecheckCapability::AnalyzeTensorSource, source, origin));
        }

        let receiver_type = self.type_expression(
            module,
            source,
            hir,
            body,
            candidate.receiver,
            TypeRequest::definitive(None),
        )?;
        let Outcome::Known(_) = receiver_type else {
            return Ok(receiver_type);
        };
        let Some(ordinary) = &candidate.ordinary else {
            return self.invalid_reduce_by_arguments(source, candidate.arguments_origin);
        };
        self.reduce_by_commitments.insert(
            ExpressionKey::new(module, expression),
            SemanticFact::Known(ReduceByCommitment::Ordinary),
        )?;
        self.type_call(context, expression, origin, &ordinary.call, request)
    }

    fn invalid_reduce_by_arguments(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        self.type_error(
            source,
            origin,
            "REDUCE_BY_ARGUMENTS_INVALID",
            "reduce_by arguments are invalid for the resolved receiver",
            BTreeMap::new(),
        )
        .map(Outcome::Poisoned)
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
        let BodyContext { source, .. } = context;
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
            ResolvedName::Declarations(_) => Ok(self.unsupported(
                TypecheckCapability::CheckClosures,
                source,
                self.expression_origin(source, expression)?,
            )),
        }
    }

    fn type_dtype_call(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: &eglur_hir::DTypeCallExpr,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let key = ResolutionKey::Expression {
            module,
            expression: call.callee,
        };
        match self.names.fact(key) {
            FactState::Known(ResolvedName::Declarations(targets)) if targets.len() == 1 => {
                if let Some(ExternalSemanticOwner::TensorSource(constructor)) =
                    self.external_semantic_owner(&targets[0])
                {
                    let target = targets[0].clone();
                    self.type_tensor_constructor(
                        context,
                        expression,
                        origin,
                        call,
                        target,
                        constructor,
                    )
                } else {
                    self.dtype_bracket_forbidden(source, origin)
                }
            }
            FactState::Known(ResolvedName::Local(_))
            | FactState::Known(ResolvedName::Declarations(_))
            | FactState::NotApplicable => self.dtype_bracket_forbidden(source, origin),
            FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => Ok(Outcome::Unperformed),
        }
    }

    fn dtype_bracket_forbidden(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        self.type_error(
            source,
            origin,
            "TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN",
            "type argument bracket syntax is forbidden for this callee",
            BTreeMap::new(),
        )
        .map(Outcome::Poisoned)
    }
}
