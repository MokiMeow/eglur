use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_block(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        statements: &[eglur_hir::StmtId],
        expected: Option<TypeId>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let context = BodyContext {
            module,
            source,
            hir,
            body,
        };
        let mut state = Outcome::Known(());
        let mut result = None;
        let mut reachable = true;
        for &statement_id in statements {
            let continuation = if reachable {
                NormalContinuation::Present
            } else {
                NormalContinuation::Absent
            };
            let next = self.with_control_path(continuation, |checker| {
                checker.type_block_statement(context, statement_id, expected)
            })?;
            if reachable && let Outcome::Known(step) = &next {
                if let Some(ty) = step.yield_type {
                    result = Some(ty);
                }
                reachable = step.normal_continuation.is_present();
            }
            state = self.merge_unit(state, next.map_unit())?;
        }
        match state {
            Outcome::Known(()) => {
                if reachable {
                    let region = self.current_control_region()?.clone();
                    self.record_control_transfer(
                        ControlTransferSource::RegionCompletion(region.clone()),
                        ControlTransferKind::NormalCompletion,
                        region,
                    )?;
                }
                let result = if let Some(result) = result {
                    result
                } else if reachable {
                    self.primitive(PrimitiveType::Unit)?
                } else {
                    self.signatures.types().never()
                };
                Ok(Outcome::Known(self.value_expression(result)))
            }
            Outcome::Poisoned(poison) => Ok(Outcome::Poisoned(poison)),
            Outcome::Unperformed => Ok(Outcome::Unperformed),
            Outcome::Deferred => Ok(Outcome::Deferred),
            Outcome::NotApplicable => Ok(Outcome::NotApplicable),
        }
    }

    pub(super) fn type_block_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        statement_id: StmtId,
        expected: Option<TypeId>,
    ) -> Result<Outcome<BlockStep>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        let statement = body
            .statement(statement_id)
            .ok_or(BodyTypingError::WrongBodyStatement(statement_id))?;
        Ok(match statement.kind() {
            StmtKind::Let {
                syntax,
                mutability,
                pattern,
                ty,
                initializer,
            } => self
                .type_let_statement(
                    context,
                    LetBindingRequest {
                        origin: statement.origin(),
                        syntax: *syntax,
                        mutability: *mutability,
                        pattern: *pattern,
                        annotation: *ty,
                        initializer: *initializer,
                    },
                )?
                .map(BlockStep::continuing),
            StmtKind::Assign { target, value } => self
                .type_assignment(context, statement.origin(), *target, *value)?
                .map(BlockStep::continuing),
            StmtKind::Expression(value) => {
                let value = self.type_expression(
                    module,
                    source,
                    hir,
                    body,
                    *value,
                    TypeRequest::discarded(),
                )?;
                self.check_discarded_value(source, statement.origin(), value)?
                    .map(BlockStep::continuing)
            }
            StmtKind::Yield(value) => {
                let value = self.type_expression(
                    module,
                    source,
                    hir,
                    body,
                    *value,
                    TypeRequest::definitive(expected),
                )?;
                if matches!(
                    &value,
                    Outcome::Known(value) if value.normal_continuation.is_present()
                ) {
                    self.record_control_transfer(
                        ControlTransferSource::Statement(StatementKey::new(module, statement_id)),
                        ControlTransferKind::Yield,
                        self.innermost_yield_target()?,
                    )?;
                }
                value.map(BlockStep::yielding)
            }
            StmtKind::Return(value) => self
                .type_return_statement(
                    context,
                    ReturnStatementRequest {
                        statement: statement_id,
                        origin: statement.origin(),
                        value: *value,
                    },
                )?
                .map(|_| BlockStep::exit()),
            StmtKind::While { .. }
            | StmtKind::For { .. }
            | StmtKind::Break
            | StmtKind::Continue => self
                .unsupported::<()>(TypecheckCapability::CheckLoops, source, statement.origin())
                .map(|_| BlockStep::exit()),
            StmtKind::Invalid => {
                Outcome::Poisoned(source.invalid_poison(statement.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: statement.origin(),
                    },
                )?)
            }
        })
    }

    pub(super) fn type_let_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        request: LetBindingRequest,
    ) -> Result<Outcome<NormalContinuation>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        let LetBindingRequest {
            origin,
            syntax,
            mutability,
            pattern,
            annotation,
            initializer,
        } = request;
        if let FactState::Poisoned(poison) =
            self.names.fact(ResolutionKey::Pattern { module, pattern })
        {
            return Ok(Outcome::Poisoned(poison.clone()));
        }
        if syntax == eglur_hir::LetSyntax::Var {
            return self
                .type_error(
                    source,
                    origin,
                    "VAR_DECL_FORBIDDEN",
                    "var declarations are forbidden in v0.1",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        let Some(initializer) = initializer else {
            return self
                .type_error(
                    source,
                    origin,
                    "BINDING_TYPE_UNDETERMINED",
                    "binding type is underdetermined because its initializer is missing",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        };
        let expected = match annotation {
            Some(annotation) => match self.type_occurrence(source, annotation)? {
                Outcome::Known(ty) => {
                    match self.normalize_type_at(source, hir.type_ref(annotation).origin(), ty)? {
                        Outcome::Known(ty) => Some(ty),
                        Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
                        Outcome::Unperformed | Outcome::Deferred | Outcome::NotApplicable => {
                            return Ok(Outcome::Unperformed);
                        }
                    }
                }
                Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
                Outcome::Unperformed | Outcome::Deferred | Outcome::NotApplicable => {
                    return Ok(Outcome::Unperformed);
                }
            },
            None => None,
        };
        if let Some((name, duplicate_origin)) = self.first_pattern_duplicate(body, pattern)? {
            let poison = self.type_error(
                source,
                duplicate_origin,
                "LET_PATTERN_DUPLICATE_BINDING",
                format!("let pattern binding `{}` is duplicated", name.spelling()),
                BTreeMap::from([(
                    Box::<str>::from("name"),
                    DiagnosticDataValue::from(name.spelling()),
                )]),
            )?;
            self.bind_pattern_poison(module, body, pattern, poison.clone())?;
            return Ok(Outcome::Poisoned(poison));
        }
        if let Some(pattern_origin) = Self::unsupported_let_pattern(body, pattern)? {
            let poison = self.type_error(
                source,
                pattern_origin,
                "LET_PATTERN_UNSUPPORTED",
                "let pattern is unsupported in v0.1",
                BTreeMap::new(),
            )?;
            self.bind_pattern_poison(module, body, pattern, poison.clone())?;
            return Ok(Outcome::Poisoned(poison));
        }
        let value = self.type_expression(
            module,
            source,
            hir,
            body,
            initializer,
            TypeRequest::definitive(expected),
        )?;
        let value = match value {
            Outcome::Deferred => {
                return self
                    .type_error(
                        source,
                        origin,
                        "BINDING_TYPE_UNDETERMINED",
                        "binding type is underdetermined after local initializer inference",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            Outcome::Known(value) => value,
            Outcome::Poisoned(poison) => {
                self.bind_pattern_poison(module, body, pattern, poison.clone())?;
                return Ok(Outcome::Poisoned(poison));
            }
            Outcome::Unperformed | Outcome::NotApplicable => return Ok(Outcome::Unperformed),
        };
        if let Some(expected) = expected
            && !self.signatures.types().is_assignable(value.ty, expected)?
        {
            let value_origin = body
                .expression(initializer)
                .ok_or(BodyTypingError::WrongBodyExpression(initializer))?
                .origin();
            let poison = self.assignment_mismatch(source, value_origin, expected, value.ty)?;
            self.bind_pattern_poison(module, body, pattern, poison.clone())?;
            return Ok(Outcome::Poisoned(poison));
        }
        let normal_continuation = value.normal_continuation;
        self.bind_pattern(
            module,
            source,
            body,
            pattern,
            expected.unwrap_or(value.ty),
            PatternBindingMode::Let(mutability),
        )
        .map(|outcome| outcome.map(|_| normal_continuation))
    }

    pub(super) fn type_assignment(
        &mut self,
        context: BodyContext<'_, '_>,
        origin: OriginId,
        target: ExprId,
        value: ExprId,
    ) -> Result<Outcome<NormalContinuation>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        let target_expression = body
            .expression(target)
            .ok_or(BodyTypingError::WrongBodyExpression(target))?;
        if matches!(target_expression.kind(), ExprKind::Hole) {
            let value = self.type_expression(
                module,
                source,
                hir,
                body,
                value,
                TypeRequest::definitive(None),
            )?;
            return self.check_discarded_value(source, origin, value);
        }
        let target_value = self.type_expression(
            module,
            source,
            hir,
            body,
            target,
            TypeRequest::definitive(None),
        )?;
        let target_value = match target_value {
            Outcome::Known(value) => value,
            Outcome::NotApplicable => {
                return self
                    .type_error(
                        source,
                        origin,
                        "ASSIGN_TARGET_NOT_ASSIGNABLE",
                        "assignment target is not assignable",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            other => return Ok(other.map(|_| NormalContinuation::Absent)),
        };
        if let FactState::Known(member) = self.members.state(&ExpressionKey::new(module, target))
            && member.kind == MemberKind::Property
        {
            return self
                .type_error(
                    source,
                    origin,
                    "PROPERTY_ASSIGN_FORBIDDEN",
                    "property assignment is forbidden; use its explicit setter",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        let Some(place) = target_value.place.as_ref() else {
            return self
                .type_error(
                    source,
                    origin,
                    "ASSIGN_TARGET_NOT_ASSIGNABLE",
                    "assignment target is not assignable",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        };
        if !place.writable {
            return self
                .type_error(
                    source,
                    origin,
                    "IMMUTABLE_BINDING_ASSIGN",
                    "immutable assignment target cannot be written",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        let value_origin = body
            .expression(value)
            .ok_or(BodyTypingError::WrongBodyExpression(value))?
            .origin();
        let value = self.with_control_path(target_value.normal_continuation, |checker| {
            checker.type_expression(
                module,
                source,
                hir,
                body,
                value,
                TypeRequest::definitive(Some(target_value.ty)),
            )
        })?;
        match value {
            Outcome::Known(value)
                if self
                    .signatures
                    .types()
                    .is_assignable(value.ty, target_value.ty)? =>
            {
                if self
                    .signatures
                    .types()
                    .ownership_root(target_value.ty)?
                    .is_some()
                {
                    return self
                        .type_error(
                            source,
                            origin,
                            "OWNERSHIP_DISCARD_FORBIDDEN",
                            "ownership root replacement would discard its obligation",
                            BTreeMap::new(),
                        )
                        .map(Outcome::Poisoned);
                }
                Ok(Outcome::Known(
                    target_value
                        .normal_continuation
                        .strict(value.normal_continuation),
                ))
            }
            Outcome::Known(value) => self
                .assignment_mismatch(source, value_origin, target_value.ty, value.ty)
                .map(Outcome::Poisoned),
            other => Ok(other.map(|_| NormalContinuation::Absent)),
        }
    }

    pub(super) fn type_exit_value(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        value: ExprId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let expected = self
            .current_return
            .unwrap_or(self.primitive(PrimitiveType::Unit)?);
        let outcome = self.type_expression(
            module,
            source,
            hir,
            body,
            value,
            TypeRequest::definitive(Some(expected)),
        )?;
        self.check_return_value(source, value, expected, outcome)
    }

    pub(super) fn check_discarded_value(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        value: Outcome<TypedExpression>,
    ) -> Result<Outcome<NormalContinuation>, BodyTypingError> {
        match value {
            Outcome::Known(value)
                if self.signatures.types().ownership_root(value.ty)?.is_some() =>
            {
                self.type_error(
                    source,
                    origin,
                    "OWNERSHIP_DISCARD_FORBIDDEN",
                    "ownership value discard is forbidden",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned)
            }
            Outcome::Known(value) => Ok(Outcome::Known(value.normal_continuation)),
            Outcome::Poisoned(poison) => Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred => Ok(Outcome::Deferred),
            Outcome::Unperformed => Ok(Outcome::Unperformed),
            Outcome::NotApplicable => Ok(Outcome::NotApplicable),
        }
    }

    pub(super) fn type_return_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        request: ReturnStatementRequest,
    ) -> Result<Outcome<NormalContinuation>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        let ReturnStatementRequest {
            statement,
            origin,
            value,
        } = request;
        let Some(expected) = self.current_return else {
            return self
                .type_error(
                    source,
                    origin,
                    "RETURN_OUTSIDE_FUNCTION",
                    "return is outside a function body",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        };
        let unit = self.primitive(PrimitiveType::Unit)?;
        match (value, expected == unit) {
            (None, true) => {
                self.record_return_transfer(module, statement)?;
                Ok(Outcome::Known(NormalContinuation::Absent))
            }
            (None, false) => self
                .type_error(
                    source,
                    origin,
                    "RETURN_VALUE_REQUIRED",
                    "return value is required by the function return type",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned),
            (Some(_), true) => self
                .type_error(
                    source,
                    origin,
                    "RETURN_VALUE_FORBIDDEN",
                    "return value is forbidden for Unit return type",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned),
            (Some(value), false) => {
                let outcome = self.type_exit_value(module, source, hir, body, value)?;
                if matches!(
                    &outcome,
                    Outcome::Known(value) if value.normal_continuation.is_present()
                ) {
                    self.record_return_transfer(module, statement)?;
                }
                Ok(outcome.map(|_| NormalContinuation::Absent))
            }
        }
    }

    pub(super) fn check_return_value(
        &mut self,
        source: &SemanticSource<'_>,
        expression: ExprId,
        expected: TypeId,
        outcome: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let Outcome::Known(value) = outcome else {
            return Ok(outcome);
        };
        if self.signatures.types().is_assignable(value.ty, expected)? {
            return Ok(Outcome::Known(value));
        }
        let origin = source
            .hir()
            .and_then(|hir| hir.body(expression.body()).expression(expression))
            .map(eglur_hir::Expr::origin)
            .ok_or(BodyTypingError::WrongBodyExpression(expression))?;
        let poison = self.return_mismatch(source, origin, expected, value.ty)?;
        Ok(Outcome::Poisoned(poison))
    }
}
