use super::*;

pub(super) enum StatementPlan {
    Complete(Outcome<BlockStep>),
    Expression {
        expression: ExprId,
        request: TypeRequest,
        path: NormalContinuation,
        continuation: StatementContinuation,
    },
}

pub(super) enum StatementContinuation {
    Let(PreparedLet),
    AssignmentTarget {
        origin: OriginId,
        target: ExprId,
        value: ExprId,
    },
    AssignmentValue {
        origin: OriginId,
        target: TypedExpression,
        value_origin: OriginId,
    },
    Discard(OriginId),
    Yield(StmtId),
    Return {
        statement: StmtId,
        value: ExprId,
        expected: TypeId,
    },
}

pub(super) struct PreparedLet {
    origin: OriginId,
    mutability: Mutability,
    pattern: PatternId,
    initializer: ExprId,
    expected: Option<TypeId>,
}

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
        let mut plan = self.prepare_statement(context, statement_id, expected)?;
        loop {
            match plan {
                StatementPlan::Complete(outcome) => return Ok(outcome),
                StatementPlan::Expression {
                    expression,
                    request,
                    path,
                    continuation,
                } => {
                    let outcome = self.with_control_path(path, |checker| {
                        checker.type_expression(
                            context.module,
                            context.source,
                            context.hir,
                            context.body,
                            expression,
                            request,
                        )
                    })?;
                    plan = self.resume_statement(context, continuation, outcome)?;
                }
            }
        }
    }

    pub(super) fn prepare_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        statement_id: StmtId,
        expected: Option<TypeId>,
    ) -> Result<StatementPlan, BodyTypingError> {
        let BodyContext { source, body, .. } = context;
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
            } => {
                return self.prepare_let_statement(
                    context,
                    LetBindingRequest {
                        origin: statement.origin(),
                        syntax: *syntax,
                        mutability: *mutability,
                        pattern: *pattern,
                        annotation: *ty,
                        initializer: *initializer,
                    },
                );
            }
            StmtKind::Assign { target, value } => {
                let target_expression = body
                    .expression(*target)
                    .ok_or(BodyTypingError::WrongBodyExpression(*target))?;
                if matches!(target_expression.kind(), ExprKind::Hole) {
                    StatementPlan::Expression {
                        expression: *value,
                        request: TypeRequest::definitive(None),
                        path: NormalContinuation::Present,
                        continuation: StatementContinuation::Discard(statement.origin()),
                    }
                } else {
                    StatementPlan::Expression {
                        expression: *target,
                        request: TypeRequest::definitive(None),
                        path: NormalContinuation::Present,
                        continuation: StatementContinuation::AssignmentTarget {
                            origin: statement.origin(),
                            target: *target,
                            value: *value,
                        },
                    }
                }
            }
            StmtKind::Expression(value) => StatementPlan::Expression {
                expression: *value,
                request: TypeRequest::discarded(),
                path: NormalContinuation::Present,
                continuation: StatementContinuation::Discard(statement.origin()),
            },
            StmtKind::Yield(value) => StatementPlan::Expression {
                expression: *value,
                request: TypeRequest::definitive(expected),
                path: NormalContinuation::Present,
                continuation: StatementContinuation::Yield(statement_id),
            },
            StmtKind::Return(value) => {
                return self.prepare_return_statement(
                    context,
                    ReturnStatementRequest {
                        statement: statement_id,
                        origin: statement.origin(),
                        value: *value,
                    },
                );
            }
            StmtKind::While { .. }
            | StmtKind::For { .. }
            | StmtKind::Break
            | StmtKind::Continue => StatementPlan::Complete(
                self.unsupported::<()>(TypecheckCapability::CheckLoops, source, statement.origin())
                    .map(|_| BlockStep::exit()),
            ),
            StmtKind::Invalid => StatementPlan::Complete(Outcome::Poisoned(
                source.invalid_poison(statement.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: statement.origin(),
                    },
                )?,
            )),
        })
    }

    pub(super) fn resume_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        continuation: StatementContinuation,
        outcome: Outcome<TypedExpression>,
    ) -> Result<StatementPlan, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let result = match continuation {
            StatementContinuation::Let(request) => self
                .finish_let_statement(context, request, outcome)?
                .map(BlockStep::continuing),
            StatementContinuation::AssignmentTarget {
                origin,
                target,
                value,
            } => {
                return self.prepare_assignment_value(context, origin, target, value, outcome);
            }
            StatementContinuation::AssignmentValue {
                origin,
                target,
                value_origin,
            } => self
                .finish_assignment_value(source, origin, target, value_origin, outcome)?
                .map(BlockStep::continuing),
            StatementContinuation::Discard(origin) => self
                .check_discarded_value(source, origin, outcome)?
                .map(BlockStep::continuing),
            StatementContinuation::Yield(statement) => {
                if matches!(&outcome,
                    Outcome::Known(value) if value.normal_continuation.is_present())
                {
                    self.record_control_transfer(
                        ControlTransferSource::Statement(StatementKey::new(module, statement)),
                        ControlTransferKind::Yield,
                        self.innermost_yield_target()?,
                    )?;
                }
                outcome.map(BlockStep::yielding)
            }
            StatementContinuation::Return {
                statement,
                value,
                expected,
            } => {
                let outcome = self.check_return_value(source, value, expected, outcome)?;
                if matches!(&outcome,
                    Outcome::Known(value) if value.normal_continuation.is_present())
                {
                    self.record_return_transfer(module, statement)?;
                }
                outcome.map(|_| BlockStep::exit())
            }
        };
        Ok(StatementPlan::Complete(result))
    }

    fn prepare_let_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        request: LetBindingRequest,
    ) -> Result<StatementPlan, BodyTypingError> {
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
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison.clone())));
        }
        if syntax == eglur_hir::LetSyntax::Var {
            let poison = self.type_error(
                source,
                origin,
                "VAR_DECL_FORBIDDEN",
                "var declarations are forbidden in v0.1",
                BTreeMap::new(),
            )?;
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        }
        let Some(initializer) = initializer else {
            let poison = self.type_error(
                source,
                origin,
                "BINDING_TYPE_UNDETERMINED",
                "binding type is underdetermined because its initializer is missing",
                BTreeMap::new(),
            )?;
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        };
        let expected = match annotation {
            Some(annotation) => match self.type_occurrence(source, annotation)? {
                Outcome::Known(ty) => {
                    match self.normalize_type_at(source, hir.type_ref(annotation).origin(), ty)? {
                        Outcome::Known(ty) => Some(ty),
                        Outcome::Poisoned(poison) => {
                            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
                        }
                        Outcome::Unperformed | Outcome::Deferred | Outcome::NotApplicable => {
                            return Ok(StatementPlan::Complete(Outcome::Unperformed));
                        }
                    }
                }
                Outcome::Poisoned(poison) => {
                    return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
                }
                Outcome::Unperformed | Outcome::Deferred | Outcome::NotApplicable => {
                    return Ok(StatementPlan::Complete(Outcome::Unperformed));
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
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
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
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        }
        Ok(StatementPlan::Expression {
            expression: initializer,
            request: TypeRequest::definitive(expected),
            path: NormalContinuation::Present,
            continuation: StatementContinuation::Let(PreparedLet {
                origin,
                mutability,
                pattern,
                initializer,
                expected,
            }),
        })
    }

    fn finish_let_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        request: PreparedLet,
        outcome: Outcome<TypedExpression>,
    ) -> Result<Outcome<NormalContinuation>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            body,
            ..
        } = context;
        let PreparedLet {
            origin,
            mutability,
            pattern,
            initializer,
            expected,
        } = request;
        let value = match outcome {
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

    fn prepare_assignment_value(
        &mut self,
        context: BodyContext<'_, '_>,
        origin: OriginId,
        target: ExprId,
        value: ExprId,
        outcome: Outcome<TypedExpression>,
    ) -> Result<StatementPlan, BodyTypingError> {
        let BodyContext {
            module,
            source,
            body,
            ..
        } = context;
        let target_value = match outcome {
            Outcome::Known(value) => value,
            Outcome::NotApplicable => {
                let poison = self.type_error(
                    source,
                    origin,
                    "ASSIGN_TARGET_NOT_ASSIGNABLE",
                    "assignment target is not assignable",
                    BTreeMap::new(),
                )?;
                return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
            }
            other => return Ok(StatementPlan::Complete(other.map(|_| BlockStep::exit()))),
        };
        if let FactState::Known(member) = self.members.state(&ExpressionKey::new(module, target))
            && member.kind == MemberKind::Property
        {
            let poison = self.type_error(
                source,
                origin,
                "PROPERTY_ASSIGN_FORBIDDEN",
                "property assignment is forbidden; use its explicit setter",
                BTreeMap::new(),
            )?;
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        }
        let Some(place) = target_value.place.as_ref() else {
            let poison = self.type_error(
                source,
                origin,
                "ASSIGN_TARGET_NOT_ASSIGNABLE",
                "assignment target is not assignable",
                BTreeMap::new(),
            )?;
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        };
        if !place.writable {
            let poison = self.type_error(
                source,
                origin,
                "IMMUTABLE_BINDING_ASSIGN",
                "immutable assignment target cannot be written",
                BTreeMap::new(),
            )?;
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        }
        let value_origin = body
            .expression(value)
            .ok_or(BodyTypingError::WrongBodyExpression(value))?
            .origin();
        Ok(StatementPlan::Expression {
            expression: value,
            request: TypeRequest::definitive(Some(target_value.ty)),
            path: target_value.normal_continuation,
            continuation: StatementContinuation::AssignmentValue {
                origin,
                target: target_value,
                value_origin,
            },
        })
    }

    fn finish_assignment_value(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        target: TypedExpression,
        value_origin: OriginId,
        outcome: Outcome<TypedExpression>,
    ) -> Result<Outcome<NormalContinuation>, BodyTypingError> {
        match outcome {
            Outcome::Known(value)
                if self.signatures.types().is_assignable(value.ty, target.ty)? =>
            {
                if self.signatures.types().ownership_root(target.ty)?.is_some() {
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
                    target.normal_continuation.strict(value.normal_continuation),
                ))
            }
            Outcome::Known(value) => self
                .assignment_mismatch(source, value_origin, target.ty, value.ty)
                .map(Outcome::Poisoned),
            other => Ok(other.map(|_| NormalContinuation::Absent)),
        }
    }

    fn prepare_return_statement(
        &mut self,
        context: BodyContext<'_, '_>,
        request: ReturnStatementRequest,
    ) -> Result<StatementPlan, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let ReturnStatementRequest {
            statement,
            origin,
            value,
        } = request;
        let Some(expected) = self.current_return else {
            let poison = self.type_error(
                source,
                origin,
                "RETURN_OUTSIDE_FUNCTION",
                "return is outside a function body",
                BTreeMap::new(),
            )?;
            return Ok(StatementPlan::Complete(Outcome::Poisoned(poison)));
        };
        let unit = self.primitive(PrimitiveType::Unit)?;
        match (value, expected == unit) {
            (None, true) => {
                self.record_return_transfer(module, statement)?;
                Ok(StatementPlan::Complete(Outcome::Known(BlockStep::exit())))
            }
            (None, false) => {
                let poison = self.type_error(
                    source,
                    origin,
                    "RETURN_VALUE_REQUIRED",
                    "return value is required by the function return type",
                    BTreeMap::new(),
                )?;
                Ok(StatementPlan::Complete(Outcome::Poisoned(poison)))
            }
            (Some(_), true) => {
                let poison = self.type_error(
                    source,
                    origin,
                    "RETURN_VALUE_FORBIDDEN",
                    "return value is forbidden for Unit return type",
                    BTreeMap::new(),
                )?;
                Ok(StatementPlan::Complete(Outcome::Poisoned(poison)))
            }
            (Some(value), false) => Ok(StatementPlan::Expression {
                expression: value,
                request: TypeRequest::definitive(Some(expected)),
                path: NormalContinuation::Present,
                continuation: StatementContinuation::Return {
                    statement,
                    value,
                    expected,
                },
            }),
        }
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
