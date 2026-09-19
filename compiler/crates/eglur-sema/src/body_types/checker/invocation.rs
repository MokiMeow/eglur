use super::application::ApplicationPlan;
use super::*;

pub(super) enum InvocationPlan {
    Complete(Outcome<TypedExpression>),
    Application {
        plan: ApplicationPlan,
        continuation: Box<InvocationFrame>,
    },
    Receiver {
        expression: ExprId,
        continuation: Box<ReduceByFrame>,
    },
}

pub(super) struct InvocationFrame {
    expression: ExprId,
    origin: OriginId,
    callee: ExprId,
    slots: Vec<ApplicationSlot>,
    contract: InvocationContract,
}

enum InvocationContract {
    Callable(CallableSignature),
    Constructor {
        target: CanonicalTarget,
        parameters: Box<[GenericParameterId]>,
    },
}

impl InvocationContract {
    fn generic_parameters(&self) -> &[GenericParameterId] {
        match self {
            Self::Callable(callable) => callable.generic_parameters(),
            Self::Constructor { parameters, .. } => parameters,
        }
    }
}

pub(super) struct ReduceByFrame {
    expression: ExprId,
    origin: OriginId,
    candidate: ReduceByCandidateExpr,
    request: TypeRequest,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_call(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: &CallExpr,
        request: TypeRequest,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        if call.partial_application {
            return Ok(InvocationPlan::Complete(self.unsupported(
                TypecheckCapability::CheckClosures,
                source,
                origin,
            )));
        }

        let callee = context
            .body
            .expression(call.callee)
            .ok_or(BodyTypingError::WrongBodyExpression(call.callee))?;
        if let ExprKind::Member { receiver, member } = callee.kind() {
            return self
                .type_method_call(
                    context, expression, origin, call, request, *receiver, member,
                )
                .map(InvocationPlan::Complete);
        }

        let callee_key = ResolutionKey::Expression {
            module,
            expression: call.callee,
        };
        let targets = match self.names.fact(callee_key) {
            FactState::Known(ResolvedName::Declarations(targets)) => targets,
            FactState::Known(ResolvedName::Local(_)) | FactState::NotApplicable => {
                return Ok(InvocationPlan::Complete(self.unsupported(
                    TypecheckCapability::CheckClosures,
                    source,
                    origin,
                )));
            }
            FactState::Poisoned(poison) => {
                return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison.clone())));
            }
            FactState::Unperformed => return Ok(InvocationPlan::Complete(Outcome::Unperformed)),
        };
        if targets.len() > 1 {
            return self
                .type_overloaded_call(context, expression, origin, call, request, targets)
                .map(InvocationPlan::Complete);
        }
        let target = targets
            .first()
            .cloned()
            .ok_or(BodyTypingError::EmptyDeclarationSet)?;
        if matches!(
            self.external_semantic_owner(&target),
            Some(ExternalSemanticOwner::TensorSource(_))
        ) {
            return Ok(InvocationPlan::Complete(self.unsupported(
                TypecheckCapability::AnalyzeTensorSource,
                source,
                origin,
            )));
        }
        let callable_id = match self.signatures.declaration(&target) {
            FactState::Known(DeclarationSignature::Callable(callable)) => *callable,
            FactState::Known(DeclarationSignature::Constructor(constructor)) => {
                let constructor = constructor.clone();
                return self.prepare_intrinsic_constructor_call(
                    context,
                    IntrinsicCallRequest {
                        expression,
                        origin,
                        call,
                        target: &target,
                        constructor: &constructor,
                        ty: request,
                    },
                );
            }
            FactState::Known(_) | FactState::NotApplicable => {
                return Ok(InvocationPlan::Complete(self.unsupported(
                    TypecheckCapability::CheckClosures,
                    source,
                    origin,
                )));
            }
            FactState::Poisoned(poison) => {
                return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison.clone())));
            }
            FactState::Unperformed => return Ok(InvocationPlan::Complete(Outcome::Unperformed)),
        };
        let callable = self
            .signatures
            .callable(callable_id)
            .cloned()
            .ok_or(BodyTypingError::UnknownCallable(callable_id))?;
        self.prepare_callable_call(context, expression, origin, call, request, callable)
    }

    pub(super) fn prepare_callable_call(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: &CallExpr,
        request: TypeRequest,
        callable: CallableSignature,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        let slots = match self.bind_call_slots(source, origin, &callable, &call.arguments)? {
            Outcome::Known(slots) => slots,
            outcome => return Ok(InvocationPlan::Complete(outcome.map(|_| unreachable!()))),
        };

        let plan = self.prepare_application(
            source,
            ApplicationRequest {
                origin,
                generic_parameters: callable.generic_parameters(),
                slots: &slots,
                result_template: callable.result(),
                explicit_arguments: &call.type_arguments,
                explicit_parameters: None,
                preset_replacements: None,
                allowed_ownership_parameter: None,
                ty: request,
                defer_underconstrained: false,
            },
        )?;
        Ok(InvocationPlan::Application {
            plan,
            continuation: Box::new(InvocationFrame {
                expression,
                origin,
                callee: call.callee,
                slots,
                contract: InvocationContract::Callable(callable),
            }),
        })
    }

    fn prepare_intrinsic_constructor_call(
        &mut self,
        context: BodyContext<'_, '_>,
        request: IntrinsicCallRequest<'_>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext { source, body, .. } = context;
        let IntrinsicCallRequest {
            expression,
            origin,
            call,
            target,
            constructor,
            ty: request,
        } = request;
        if !call.type_arguments.is_empty() {
            return self
                .type_argument_inference_failure(source, origin)
                .map(InvocationPlan::Complete);
        }
        let payload_types = match constructor.payload() {
            ConstructorPayloadSignature::Unit => {
                return self
                    .constructor_payload_form_error(
                        source,
                        origin,
                        target,
                        ConstructorPayloadForm::Unit,
                        ConstructorPayloadForm::Positional,
                    )
                    .map(Outcome::Poisoned)
                    .map(InvocationPlan::Complete);
            }
            ConstructorPayloadSignature::Positional(types) => types,
        };
        for argument in &call.arguments {
            if let Some(name) = &argument.name {
                let Some(name) = name.text() else {
                    return Ok(InvocationPlan::Complete(Outcome::Poisoned(
                        source.invalid_poison(name.origin()).cloned().ok_or(
                            BodyTypingError::MissingOriginSpan {
                                file: source.file(),
                                origin: name.origin(),
                            },
                        )?,
                    )));
                };
                let data =
                    BTreeMap::from([(Box::<str>::from("name"), DiagnosticDataValue::from(name))]);
                return self
                    .type_error(
                        source,
                        argument.origin,
                        "CALL_UNKNOWN_NAMED_ARG",
                        format!("constructor call has unknown named argument `{name}`"),
                        data,
                    )
                    .map(Outcome::Poisoned)
                    .map(InvocationPlan::Complete);
            }
        }
        if call.arguments.len() > payload_types.len() {
            return self
                .type_error(
                    source,
                    call.arguments[payload_types.len()].origin,
                    "CALL_TOO_MANY_ARGS",
                    "constructor call has too many payload arguments",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned)
                .map(InvocationPlan::Complete);
        }
        if call.arguments.len() < payload_types.len() {
            let parameter = format!("#{}", call.arguments.len());
            let data = BTreeMap::from([(
                Box::<str>::from("parameter"),
                DiagnosticDataValue::from(parameter.as_str()),
            )]);
            return self
                .type_error(
                    source,
                    origin,
                    "CALL_MISSING_ARG",
                    "constructor call argument is missing a required parameter",
                    data,
                )
                .map(Outcome::Poisoned)
                .map(InvocationPlan::Complete);
        }

        let slots = payload_types
            .iter()
            .copied()
            .zip(call.arguments.iter())
            .enumerate()
            .map(|(index, (template, argument))| ApplicationSlot {
                name: format!("#{index}").into_boxed_str(),
                template,
                value: argument.value,
                origin: argument.origin,
                related: None,
                evaluation_order: index,
                receiver_adjustment: false,
            })
            .collect::<Vec<_>>();
        let qualified = body
            .expression(call.callee)
            .and_then(|callee| match callee.kind() {
                ExprKind::Name(path) => Some(path.is_qualified()),
                _ => None,
            })
            .unwrap_or(false);
        let plan = self.prepare_application(
            source,
            ApplicationRequest {
                origin,
                generic_parameters: constructor.generic_parameters(),
                slots: &slots,
                result_template: constructor.result(),
                explicit_arguments: &[],
                explicit_parameters: None,
                preset_replacements: None,
                allowed_ownership_parameter: self
                    .conditional_ownership_parameter(target, constructor.generic_parameters()),
                ty: request,
                defer_underconstrained: !qualified && self.is_reserved_constructor(target),
            },
        )?;
        Ok(InvocationPlan::Application {
            plan,
            continuation: Box::new(InvocationFrame {
                expression,
                origin,
                callee: call.callee,
                slots,
                contract: InvocationContract::Constructor {
                    target: target.clone(),
                    parameters: constructor.generic_parameters().into(),
                },
            }),
        })
    }

    pub(super) fn finish_invocation(
        &mut self,
        context: BodyContext<'_, '_>,
        frame: InvocationFrame,
        outcome: Outcome<ResolvedApplication>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let Outcome::Known(application) = outcome else {
            return Ok(outcome.map(|_| unreachable!()));
        };

        let BodyContext { module, source, .. } = context;
        if let InvocationContract::Callable(callable) = &frame.contract
            && let Some(poison) = self.check_callable_requirements(
                source,
                frame.origin,
                callable.requirements(),
                &application.replacements,
            )?
        {
            return Ok(Outcome::Poisoned(poison));
        }

        let key = ExpressionKey::new(module, frame.expression);
        self.record_trait_obligations(
            key,
            source,
            frame.origin,
            frame.contract.generic_parameters(),
            &application.replacements,
        )?;
        let target = match frame.contract {
            InvocationContract::Callable(callable) => InvocationTarget::Callable(
                self.intern_callable_instance(callable.id(), application.substitution)?,
            ),
            InvocationContract::Constructor { target, .. } => InvocationTarget::Constructor {
                declaration: target,
                substitution: application.substitution,
            },
        };
        self.invocations.insert(
            key,
            SemanticFact::Known(invocation_fact(target, &frame.slots)),
        )?;

        let callee_ty = self.signatures.types_mut().callable(
            application.specialized_slots.iter().copied(),
            application.result,
        )?;
        self.record_expression(
            ExpressionKey::new(module, frame.callee),
            Outcome::Known(self.value_expression(callee_ty)),
        )?;
        Ok(Outcome::Known(self.value_expression_with_continuation(
            application.result,
            application.normal_continuation,
        )))
    }

    pub(super) fn prepare_reduce_by_candidate(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        candidate: ReduceByCandidateExpr,
        request: TypeRequest,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext {
            module,
            source,
            body,
            ..
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
                return self
                    .invalid_reduce_by_arguments(source, candidate.arguments_origin)
                    .map(InvocationPlan::Complete);
            };
            self.reduce_by_commitments.insert(
                ExpressionKey::new(module, expression),
                SemanticFact::Known(commitment),
            )?;
            return Ok(InvocationPlan::Complete(self.unsupported(
                TypecheckCapability::AnalyzeTensorSource,
                source,
                origin,
            )));
        }
        Ok(InvocationPlan::Receiver {
            expression: candidate.receiver,
            continuation: Box::new(ReduceByFrame {
                expression,
                origin,
                candidate,
                request,
            }),
        })
    }

    pub(super) fn resume_reduce_by_receiver(
        &mut self,
        context: BodyContext<'_, '_>,
        frame: ReduceByFrame,
        outcome: Outcome<TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let Outcome::Known(_) = outcome else {
            return Ok(InvocationPlan::Complete(outcome));
        };

        let Some(ordinary) = &frame.candidate.ordinary else {
            return self
                .invalid_reduce_by_arguments(context.source, frame.candidate.arguments_origin)
                .map(InvocationPlan::Complete);
        };
        self.reduce_by_commitments.insert(
            ExpressionKey::new(context.module, frame.expression),
            SemanticFact::Known(ReduceByCommitment::Ordinary),
        )?;
        self.prepare_call(
            context,
            frame.expression,
            frame.origin,
            &ordinary.call,
            frame.request,
        )
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
}
