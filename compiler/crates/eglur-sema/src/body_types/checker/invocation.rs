use super::application::ApplicationPlan;
use super::dispatch::DispatchContinuation;
use super::tensor::TensorConstructorFrame;
use super::*;

pub(super) enum InvocationPlan {
    Complete(Outcome<TypedExpression>),
    Application {
        plan: ApplicationPlan,
        continuation: Box<InvocationFrame>,
    },
    Expression {
        expression: ExprId,
        request: TypeRequest,
        path: NormalContinuation,
        continuation: InvocationContinuation,
    },
}

pub(super) enum InvocationContinuation {
    ReduceBy(Box<ReduceByFrame>),
    TensorConstructor(Box<TensorConstructorFrame>),
    Dispatch(DispatchContinuation),
}

pub(super) struct InvocationFrame {
    expression: ExprId,
    origin: OriginId,
    slots: Vec<ApplicationSlot>,
    contract: InvocationContract,
}

impl InvocationFrame {
    pub(super) fn construction(
        expression: ExprId,
        origin: OriginId,
        slots: Vec<ApplicationSlot>,
        target: CanonicalTarget,
        parameters: Box<[GenericParameterId]>,
    ) -> Self {
        Self {
            expression,
            origin,
            slots,
            contract: InvocationContract::Constructor {
                target,
                parameters,
                callee: None,
            },
        }
    }
}

enum InvocationContract {
    Callable {
        callable: CallableSignature,
        callee: ExprId,
    },
    Method {
        candidate: Box<MethodCandidate>,
        member_origin: OriginId,
        callee: ExprId,
    },
    Constructor {
        target: CanonicalTarget,
        parameters: Box<[GenericParameterId]>,
        callee: Option<ExprId>,
    },
}

impl InvocationContract {
    fn generic_parameters(&self) -> &[GenericParameterId] {
        match self {
            Self::Callable { callable, .. } => callable.generic_parameters(),
            Self::Method { candidate, .. } => &candidate.explicit_parameters,
            Self::Constructor { parameters, .. } => parameters,
        }
    }

    fn callee(&self) -> Option<ExprId> {
        match self {
            Self::Callable { callee, .. } | Self::Method { callee, .. } => Some(*callee),
            Self::Constructor { callee, .. } => *callee,
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

        let (callee_id, callee) = context
            .body
            .grouping_chain(call.callee)
            .last()
            .ok_or(BodyTypingError::WrongBodyExpression(call.callee))?;
        if let ExprKind::Member { receiver, member } = callee.kind() {
            return self.prepare_method_call(
                context,
                MethodCallRequest {
                    expression,
                    origin,
                    call,
                    receiver: *receiver,
                    member,
                    ty: request,
                },
            );
        }
        if !matches!(callee.kind(), ExprKind::Name(_)) {
            return Ok(InvocationPlan::Complete(self.unsupported(
                TypecheckCapability::CheckClosures,
                source,
                origin,
            )));
        }

        let callee_key = ResolutionKey::Expression {
            module,
            expression: callee_id,
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
                .prepare_overloaded_call(context, expression, origin, call, request, targets);
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
                slots,
                contract: InvocationContract::Callable {
                    callable,
                    callee: call.callee,
                },
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
                passing: ArgumentPassing::Value,
            })
            .collect::<Vec<_>>();
        let qualified = body
            .grouping_chain(call.callee)
            .last()
            .and_then(|(_, callee)| match callee.kind() {
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
                slots,
                contract: InvocationContract::Constructor {
                    callee: Some(call.callee),
                    target: target.clone(),
                    parameters: constructor.generic_parameters().into(),
                },
            }),
        })
    }

    pub(super) fn prepare_selected_method(
        &mut self,
        context: BodyContext<'_, '_>,
        request: MethodCallRequest<'_>,
        candidate: MethodCandidate,
        passing: ArgumentPassing,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        let bindings =
            Self::bind_method_argument_indices(&candidate.callable, &request.call.arguments)
                .map_err(|_| BodyTypingError::InconsistentMethodBinding)?;
        let receiver_parameter = candidate.callable.parameters().first().ok_or(
            BodyTypingError::MissingMethodReceiver(candidate.callable.id()),
        )?;
        let mut slots = Vec::with_capacity(candidate.callable.parameters().len());
        slots.push(ApplicationSlot {
            name: receiver_parameter.name().unwrap_or("self").into(),
            template: receiver_parameter.ty(),
            value: request.receiver,
            origin: self.expression_origin(source, request.receiver)?,
            related: self.parameter_span(&candidate.callable, receiver_parameter)?,
            evaluation_order: 0,
            passing,
        });
        for (parameter_index, parameter) in candidate.callable.parameters()[1..].iter().enumerate()
        {
            let argument_index = bindings[parameter_index];
            let argument = &request.call.arguments[argument_index];
            slots.push(ApplicationSlot {
                name: parameter.name().map_or_else(
                    || format!("#{}", parameter_index + 1).into_boxed_str(),
                    Box::<str>::from,
                ),
                template: parameter.ty(),
                value: argument.value,
                origin: argument.origin,
                related: self.parameter_span(&candidate.callable, parameter)?,
                evaluation_order: argument_index + 1,
                passing: ArgumentPassing::Value,
            });
        }
        let plan = self.prepare_application(
            source,
            ApplicationRequest {
                origin: request.origin,
                generic_parameters: candidate.callable.generic_parameters(),
                slots: &slots,
                result_template: candidate.callable.result(),
                explicit_arguments: &request.call.type_arguments,
                explicit_parameters: Some(&candidate.explicit_parameters),
                preset_replacements: Some(&candidate.preset_replacements),
                allowed_ownership_parameter: None,
                ty: request.ty,
                defer_underconstrained: false,
            },
        )?;
        Ok(InvocationPlan::Application {
            plan,
            continuation: Box::new(InvocationFrame {
                expression: request.expression,
                origin: request.origin,
                slots,
                contract: InvocationContract::Method {
                    callee: request.call.callee,
                    candidate: Box::new(candidate),
                    member_origin: request.member.origin(),
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
        if let InvocationContract::Callable { callable, .. } = &frame.contract
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
        let target = match &frame.contract {
            InvocationContract::Callable { callable, .. } => InvocationTarget::Callable(
                self.intern_callable_instance(callable.id(), application.substitution)?,
            ),
            InvocationContract::Method { candidate, .. } => {
                let arguments = candidate
                    .explicit_parameters
                    .iter()
                    .map(|parameter| {
                        application
                            .replacements
                            .get(parameter)
                            .copied()
                            .ok_or_else(|| {
                                BodyTypingError::UnavailableGenericParameter(parameter.clone())
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let substitution = self.signatures.types_mut().intern_substitution(arguments)?;
                InvocationTarget::Callable(
                    self.intern_callable_instance(candidate.callable.id(), substitution)?,
                )
            }
            InvocationContract::Constructor { target, .. } => InvocationTarget::Constructor {
                declaration: target.clone(),
                substitution: application.substitution,
            },
        };
        self.invocations.insert(
            key,
            SemanticFact::Known(invocation_fact(target, &frame.slots)),
        )?;

        if let InvocationContract::Method {
            candidate,
            member_origin,
            ..
        } = &frame.contract
            && let MethodDispatchTemplate::Trait {
                subject,
                trait_ref,
                method,
            } = &candidate.dispatch
        {
            let trait_ref = self.specialize_trait_ref(trait_ref, &application.replacements)?;
            let owner = self
                .current_owner
                .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
            self.method_dispatches.push(PendingMethodDispatch {
                invocation: key,
                owner,
                location: SemanticLocation::origin(source.file(), *member_origin),
                subject: *subject,
                trait_ref,
                method: method.clone(),
            });
        }

        if let Some(callee) = frame.contract.callee() {
            let callee_ty = self.signatures.types_mut().callable(
                application.specialized_slots.iter().copied(),
                application.result,
            )?;
            self.record_callee_type(context, callee, callee_ty)?;
        }
        Ok(Outcome::Known(self.value_expression_with_continuation(
            application.result,
            application.normal_continuation,
        )))
    }

    pub(super) fn record_callee_type(
        &mut self,
        context: BodyContext<'_, '_>,
        callee: ExprId,
        ty: TypeId,
    ) -> Result<(), BodyTypingError> {
        let mut chain = context.body.grouping_chain(callee).peekable();
        if chain.peek().is_none() {
            return Err(BodyTypingError::WrongBodyExpression(callee));
        }
        for (expression, _) in chain {
            self.record_expression(
                ExpressionKey::new(context.module, expression),
                Outcome::Known(self.value_expression(ty)),
            )?;
        }
        Ok(())
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
        let (_, receiver) = body
            .grouping_chain(candidate.receiver)
            .last()
            .ok_or(BodyTypingError::WrongBodyExpression(candidate.receiver))?;
        let canonical = matches!(receiver.kind(), ExprKind::TensorSchema(schema) if schema.family == TensorSchemaFamily::Reduce);

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
        Ok(InvocationPlan::Expression {
            expression: candidate.receiver,
            request: TypeRequest::definitive(None),
            path: NormalContinuation::Present,
            continuation: InvocationContinuation::ReduceBy(Box::new(ReduceByFrame {
                expression,
                origin,
                candidate,
                request,
            })),
        })
    }

    pub(super) fn resume_invocation(
        &mut self,
        context: BodyContext<'_, '_>,
        continuation: InvocationContinuation,
        outcome: Outcome<TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        match continuation {
            InvocationContinuation::TensorConstructor(frame) => {
                self.resume_tensor_constructor(context, frame, outcome)
            }
            InvocationContinuation::ReduceBy(frame) => {
                self.resume_reduce_by_receiver(context, *frame, outcome)
            }
            InvocationContinuation::Dispatch(continuation) => {
                self.resume_dispatch(context, continuation, outcome)
            }
        }
    }

    fn resume_reduce_by_receiver(
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
