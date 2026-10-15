use super::*;

pub(super) enum FailurePlan {
    Complete(Outcome<TypedExpression>),
    ResultBlock {
        block: ExprId,
        request: TypeRequest,
        context: ResultContext,
        frame: ResultFrame,
    },
    CheckedBlock {
        block: ExprId,
        request: TypeRequest,
        context: CheckedContext,
        frame: CheckedFrame,
    },
    QuestionOperand {
        operand: ExprId,
        request: TypeRequest,
        frame: QuestionFrame,
    },
}

pub(super) struct ResultFrame {
    origin: OriginId,
    block: ExprId,
}

pub(super) struct CheckedFrame {
    region: ControlRegionKey,
    block: ExprId,
    numeric_error: TypeId,
}

pub(super) struct QuestionFrame {
    key: ExpressionKey,
    origin: OriginId,
    context_index: usize,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_result(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        block: ExprId,
        request: TypeRequest,
    ) -> Result<FailurePlan, BodyFlow> {
        let BodyContext { module, source, .. } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(FailurePlan::Complete(Outcome::Unperformed));
        }

        let expected = request
            .expected_type()
            .map(|ty| self.failure_carrier(ty))
            .transpose()?
            .flatten();
        if expected.is_none() {
            return Ok(FailurePlan::Complete(self.unsupported(
                TypecheckCapability::InferResultBlockType,
                source,
                origin,
            )));
        }
        let region = ControlRegionKey::Result(ExpressionKey::new(module, expression));
        Ok(FailurePlan::ResultBlock {
            block,
            request: request.definitive_child(expected.map(|carrier| carrier.ty)),
            context: ResultContext {
                region,
                expected,
                family: expected.map(|carrier| carrier.family),
            },
            frame: ResultFrame { origin, block },
        })
    }

    pub(super) fn finish_result(
        &mut self,
        context: BodyContext<'_, '_>,
        frame: ResultFrame,
        result_context: ResultContext,
        body_outcome: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
        let BodyContext { source, body, .. } = context;
        let ResultFrame { origin, block } = frame;
        let expected = result_context.expected;
        let region = result_context.region.clone();
        let has_short_circuit = self.transfers.iter().any(|transfer| {
            transfer.kind() == ControlTransferKind::QuestionShortCircuit
                && transfer.target() == &region
        });

        let missing_yield = self.final_yield_expression(body, block)?.is_none();
        let missing_yield_poison = if missing_yield {
            Some(self.type_error(
                source,
                origin,
                "RESULT_MISSING_YIELD",
                "result block must end with an explicit yield",
                BTreeMap::new(),
            )?)
        } else {
            None
        };
        if missing_yield {
            return self.apply_additional_poison(body_outcome, missing_yield_poison);
        }

        let outcome = match body_outcome {
            Outcome::Known(value) => {
                let inferred = self.failure_carrier(value.ty)?;
                let carrier = expected.or(inferred);
                let Some(carrier) = carrier else {
                    return Ok(match missing_yield_poison {
                        Some(poison) => Outcome::Poisoned(poison),
                        None => Outcome::Deferred,
                    });
                };
                if let Some(family) = result_context.family
                    && family != carrier.family
                {
                    let poison = self.type_error(
                        source,
                        origin,
                        "RETURN_TYPE_MISMATCH",
                        "result block return type mismatch",
                        BTreeMap::from([
                            (
                                Box::<str>::from("expected_type"),
                                DiagnosticDataValue::from(self.format_type(carrier.ty)?),
                            ),
                            (
                                Box::<str>::from("actual_type"),
                                DiagnosticDataValue::from(self.format_type(value.ty)?),
                            ),
                        ]),
                    )?;
                    Outcome::Poisoned(poison)
                } else if !self
                    .signatures
                    .types()
                    .is_assignable(value.ty, carrier.ty)?
                {
                    Outcome::Poisoned(self.return_mismatch(source, origin, carrier.ty, value.ty)?)
                } else {
                    let continuation =
                        if value.normal_continuation.is_present() || has_short_circuit {
                            NormalContinuation::Present
                        } else {
                            NormalContinuation::Absent
                        };
                    Outcome::Known(
                        self.value_expression_with_continuation(carrier.ty, continuation),
                    )
                }
            }
            Outcome::Poisoned(poison) => Outcome::Poisoned(poison),
            Outcome::Deferred => Outcome::Deferred,
            Outcome::Unperformed => Outcome::Unperformed,
            Outcome::NotApplicable => Outcome::NotApplicable,
        };
        self.apply_additional_poison(outcome, missing_yield_poison)
    }

    pub(super) fn prepare_question(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        operand: ExprId,
    ) -> Result<FailurePlan, BodyFlow> {
        let BodyContext { module, source, .. } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(FailurePlan::Complete(Outcome::Unperformed));
        }
        let Some(context_index) = self.active_result_context_index() else {
            return self
                .type_error(
                    source,
                    origin,
                    "Q_OUTSIDE_RESULT",
                    "? is only valid inside a result block",
                    BTreeMap::new(),
                )
                .map(|poison| FailurePlan::Complete(Outcome::Poisoned(poison)));
        };
        Ok(FailurePlan::QuestionOperand {
            operand,
            request: TypeRequest::definitive(None),
            frame: QuestionFrame {
                key: ExpressionKey::new(module, expression),
                origin,
                context_index,
            },
        })
    }

    pub(super) fn finish_question(
        &mut self,
        source: &SemanticSource<'_>,
        frame: QuestionFrame,
        operand: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
        let QuestionFrame {
            key,
            origin,
            context_index,
        } = frame;
        let operand = match operand {
            Outcome::Known(operand) => operand,
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred => return Ok(Outcome::Deferred),
            Outcome::Unperformed => return Ok(Outcome::Unperformed),
            Outcome::NotApplicable => return Ok(Outcome::NotApplicable),
        };
        let Some(carrier) = self.failure_carrier(operand.ty)? else {
            self.demand_unimplemented(TypecheckCapability::CheckOperators, source, origin);
            return Ok(Outcome::Unperformed);
        };

        let existing_family = self.result_contexts[context_index].family;
        if existing_family.is_some_and(|family| family != carrier.family) {
            return self
                .type_error(
                    source,
                    origin,
                    "RESULT_BLOCK_MIXED_KINDS",
                    "Res and Opt question operations are mixed in one result block",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        self.result_contexts[context_index].family = Some(carrier.family);

        if let (
            Some(FailureCarrier {
                family: FailureFamily::Res,
                error: Some(expected_error),
                ..
            }),
            FailureCarrier {
                family: FailureFamily::Res,
                error: Some(actual_error),
                ..
            },
        ) = (self.result_contexts[context_index].expected, carrier)
            && !self
                .signatures
                .types()
                .is_assignable(actual_error, expected_error)?
        {
            return Ok(Outcome::Poisoned(self.return_mismatch(
                source,
                origin,
                expected_error,
                actual_error,
            )?));
        }

        let has_exit = carrier
            .error
            .is_none_or(|error| error != self.signatures.types().never());
        if has_exit && operand.normal_continuation.is_present() {
            let target = self.result_contexts[context_index].region.clone();
            self.record_control_transfer(
                ControlTransferSource::Expression(key),
                ControlTransferKind::QuestionShortCircuit,
                target,
            )?;
        }
        Ok(Outcome::Known(
            self.value_expression_with_continuation(
                carrier.payload,
                operand
                    .normal_continuation
                    .strict(self.continuation_for_type(carrier.payload)),
            ),
        ))
    }

    pub(super) fn prepare_checked(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        block: ExprId,
        request: TypeRequest,
    ) -> Result<FailurePlan, BodyFlow> {
        let BodyContext { module, source, .. } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(FailurePlan::Complete(Outcome::Unperformed));
        }
        let Some(numeric_error) = self.numeric_error_type(source, origin)? else {
            return Ok(FailurePlan::Complete(Outcome::Unperformed));
        };
        let expected_carrier = request
            .expected_type()
            .map(|ty| self.failure_carrier(ty))
            .transpose()?
            .flatten();
        let expected_success = match expected_carrier {
            Some(FailureCarrier {
                family: FailureFamily::Res,
                payload,
                error: Some(error),
                ..
            }) if self
                .signatures
                .types()
                .is_assignable(numeric_error, error)? =>
            {
                Some(payload)
            }
            _ => None,
        };

        let region = ControlRegionKey::Checked(ExpressionKey::new(module, expression));
        Ok(FailurePlan::CheckedBlock {
            block,
            request: request.definitive_child(expected_success),
            context: CheckedContext {
                region: region.clone(),
            },
            frame: CheckedFrame {
                region,
                block,
                numeric_error,
            },
        })
    }

    pub(super) fn finish_checked(
        &mut self,
        context: BodyContext<'_, '_>,
        frame: CheckedFrame,
        body_outcome: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyFlow> {
        let BodyContext { module, body, .. } = context;
        let CheckedFrame {
            region,
            block,
            numeric_error,
        } = frame;
        let has_fault = self.transfers.iter().any(|transfer| {
            matches!(transfer.kind(), ControlTransferKind::CheckedFault(_))
                && transfer.target() == &region
        });

        let outcome = match body_outcome {
            Outcome::Known(value) => {
                let final_yield = self.final_yield_expression(body, block)?;
                let success = if let Some(final_yield) = final_yield {
                    if value.normal_continuation.is_present() {
                        value.ty
                    } else {
                        match self
                            .expressions
                            .state(&ExpressionKey::new(module, final_yield))
                        {
                            FactState::Known(final_value) => final_value.ty,
                            FactState::Unperformed
                            | FactState::Poisoned(_)
                            | FactState::NotApplicable => {
                                return Err(BodyFlow::Internal(
                                    BodyTypingError::MissingFinalYieldType(final_yield),
                                ));
                            }
                        }
                    }
                } else {
                    self.primitive(PrimitiveType::Unit)?
                };
                let result = self.signatures.types_mut().nominal(
                    NominalHead::Intrinsic(IntrinsicNominal::Res),
                    [success, numeric_error],
                )?;
                let continuation = if value.normal_continuation.is_present() || has_fault {
                    NormalContinuation::Present
                } else {
                    NormalContinuation::Absent
                };
                Outcome::Known(self.value_expression_with_continuation(result, continuation))
            }
            Outcome::Poisoned(poison) => Outcome::Poisoned(poison),
            Outcome::Deferred => Outcome::Deferred,
            Outcome::Unperformed => Outcome::Unperformed,
            Outcome::NotApplicable => Outcome::NotApplicable,
        };
        Ok(outcome)
    }

    pub(super) fn known_binary_operands(
        &mut self,
        left: Outcome<TypedExpression>,
        right: Outcome<TypedExpression>,
    ) -> Result<Outcome<(TypedExpression, TypedExpression)>, BodyFlow> {
        match (left, right) {
            (Outcome::Known(left), Outcome::Known(right)) => Ok(Outcome::Known((left, right))),
            (Outcome::Poisoned(left), Outcome::Poisoned(right)) => {
                Ok(Outcome::Poisoned(left.merge(&right)?))
            }
            (Outcome::Poisoned(poison), _) | (_, Outcome::Poisoned(poison)) => {
                Ok(Outcome::Poisoned(poison))
            }
            (Outcome::Unperformed, _) | (_, Outcome::Unperformed) => Ok(Outcome::Unperformed),
            (Outcome::Deferred, _) | (_, Outcome::Deferred) => Ok(Outcome::Deferred),
            (Outcome::NotApplicable, _) | (_, Outcome::NotApplicable) => Ok(Outcome::NotApplicable),
        }
    }

    pub(super) fn record_checked_numeric_operation(
        &mut self,
        key: ExpressionKey,
        operator: CheckedNumericOperator,
        result: TypeId,
        continuation: NormalContinuation,
        total: bool,
        conversion_can_fail: bool,
    ) -> Result<(), BodyFlow> {
        let faults = if total {
            Box::new([]) as Box<[NumericFaultCause]>
        } else {
            operator
                .faults(
                    self.is_signed_fixed_width_integer(result)?,
                    conversion_can_fail,
                )
                .to_vec()
                .into_boxed_slice()
        };
        let operation = CheckedNumericOperation::new(operator, result, faults.clone());
        match self.checked_numeric.state(&key) {
            FactState::Unperformed => self
                .checked_numeric
                .insert(key, SemanticFact::Known(operation))?,
            FactState::Known(existing) if existing == &operation => {}
            FactState::Known(_) | FactState::Poisoned(_) | FactState::NotApplicable => {
                return Err(DuplicateFact.into());
            }
        }
        if !continuation.is_present() || faults.is_empty() {
            return Ok(());
        }
        let context_index = self
            .checked_contexts
            .len()
            .checked_sub(1)
            .ok_or(BodyTypingError::MissingCheckedContext)?;
        let target = self.checked_contexts[context_index].region.clone();
        for fault in faults {
            self.record_control_transfer(
                ControlTransferSource::Expression(key),
                ControlTransferKind::CheckedFault(fault),
                target.clone(),
            )?;
        }
        Ok(())
    }

    pub(super) fn has_active_checked_context(&self) -> bool {
        !self.checked_contexts.is_empty()
    }

    pub(super) fn active_result_context_index(&self) -> Option<usize> {
        let floor = self.question_floors.last().copied().unwrap_or(0);
        (self.result_contexts.len() > floor).then(|| self.result_contexts.len() - 1)
    }

    pub(super) fn failure_carrier(&self, ty: TypeId) -> Result<Option<FailureCarrier>, BodyFlow> {
        let TypeKind::Nominal {
            head: NominalHead::Intrinsic(head),
            substitution,
        } = self.signatures.types().kind(ty)?
        else {
            return Ok(None);
        };
        let arguments = self.signatures.types().substitution(*substitution)?;
        Ok(match head {
            IntrinsicNominal::Res => Some(FailureCarrier {
                ty,
                family: FailureFamily::Res,
                payload: arguments[0],
                error: Some(arguments[1]),
            }),
            IntrinsicNominal::Opt => Some(FailureCarrier {
                ty,
                family: FailureFamily::Opt,
                payload: arguments[0],
                error: None,
            }),
            IntrinsicNominal::Own | IntrinsicNominal::Box => None,
        })
    }

    pub(super) fn numeric_error_type(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<Option<TypeId>, BodyFlow> {
        let Some(declaration) = self
            .signatures
            .language_items()
            .declaration(LanguageItem::NumericError)
            .cloned()
        else {
            self.recorder.require(
                CompileCapability::Resolve(
                    crate::ResolveCapability::ResolveCompleteStandardLibraryNames,
                ),
                SemanticLocation::origin(source.file(), origin),
            );
            return Ok(None);
        };
        Ok(Some(
            self.signatures
                .types_mut()
                .nominal(NominalHead::Declaration(declaration), [])?,
        ))
    }

    pub(super) fn final_yield_expression(
        &self,
        body: &Body,
        block: ExprId,
    ) -> Result<Option<ExprId>, BodyFlow> {
        let expression = body
            .expression(block)
            .ok_or(BodyTypingError::WrongBodyExpression(block))?;
        let ExprKind::Block { statements } = expression.kind() else {
            return Err(BodyFlow::Internal(BodyTypingError::FailureBodyIsNotBlock(
                block,
            )));
        };
        let Some(statement) = statements.last() else {
            return Ok(None);
        };
        let statement = body
            .statement(*statement)
            .ok_or(BodyTypingError::WrongBodyStatement(*statement))?;
        Ok(match statement.kind() {
            StmtKind::Yield(value) => Some(*value),
            StmtKind::Let { .. }
            | StmtKind::Assign { .. }
            | StmtKind::Expression(_)
            | StmtKind::Return(_)
            | StmtKind::While { .. }
            | StmtKind::For { .. }
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Invalid => None,
        })
    }

    pub(super) fn is_const_evaluable_expression(
        &mut self,
        module: ModuleId,
        body: &Body,
        expression: ExprId,
    ) -> Result<bool, BodyFlow> {
        let mut pending = vec![(expression, false)];
        while let Some((current, visited)) = pending.pop() {
            let key = ExpressionKey::new(module, current);
            if self.const_eligibility.contains_key(&key) {
                continue;
            }
            let value = body
                .expression(current)
                .ok_or(BodyTypingError::WrongBodyExpression(current))?;
            if !visited {
                pending.push((current, true));
                match value.kind() {
                    ExprKind::Group(inner) | ExprKind::Unary { operand: inner, .. } => {
                        pending.push((*inner, false))
                    }
                    ExprKind::Binary { left, right, .. } => {
                        pending.push((*right, false));
                        pending.push((*left, false));
                    }
                    _ => {}
                }
                continue;
            }
            let child = |id| self.const_eligibility[&ExpressionKey::new(module, id)];
            let eligible = match value.kind() {
                ExprKind::Literal(_) | ExprKind::Unit => true,
                ExprKind::Name(_) => {
                    matches!(self.names.fact(ResolutionKey::Expression { module, expression: current }),
                    FactState::Known(ResolvedName::Declarations(targets)) if targets.len() == 1
                        && matches!(self.signatures.declaration(&targets[0]), FactState::Known(DeclarationSignature::Const { .. })))
                }
                ExprKind::Group(inner) => child(*inner),
                ExprKind::Unary {
                    operator: UnaryOperator::Negate | UnaryOperator::Not,
                    operand,
                } => child(*operand),
                ExprKind::Binary {
                    operator,
                    left,
                    right,
                } => {
                    !matches!(
                        operator,
                        BinaryOperator::Range | BinaryOperator::RangeInclusive
                    ) && child(*left)
                        && child(*right)
                }
                _ => false,
            };
            self.const_eligibility.insert(key, eligible);
        }
        Ok(self.const_eligibility[&ExpressionKey::new(module, expression)])
    }

    pub(super) fn apply_additional_poison<T>(
        &mut self,
        outcome: Outcome<T>,
        additional: Option<Poison>,
    ) -> Result<Outcome<T>, BodyFlow> {
        let Some(additional) = additional else {
            return Ok(outcome);
        };
        Ok(match outcome {
            Outcome::Poisoned(existing) => Outcome::Poisoned(existing.merge(&additional)?),
            _ => Outcome::Poisoned(additional),
        })
    }
}
