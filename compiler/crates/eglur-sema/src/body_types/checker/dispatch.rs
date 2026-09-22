use super::invocation::{InvocationContinuation, InvocationPlan};
use super::*;

struct DispatchRequest {
    expression: ExprId,
    origin: OriginId,
    call: CallExpr,
    ty: TypeRequest,
}

pub(super) enum DispatchContinuation {
    Receiver(Box<MethodReceiverFrame>),
    Argument {
        frame: Box<DispatchArgumentsFrame>,
        argument: ExprId,
    },
}

pub(super) struct MethodReceiverFrame {
    request: DispatchRequest,
    receiver: ExprId,
    member: Name,
}

pub(super) struct DispatchArgumentsFrame {
    request: DispatchRequest,
    selection: DispatchSelection,
    next: usize,
    typed: BTreeMap<ExprId, TypedExpression>,
    normal_continuation: NormalContinuation,
    poison: Option<Poison>,
    unperformed: bool,
}

enum DispatchSelection {
    Method(MethodSelection),
    Overload(Vec<OverloadCandidate>),
}

struct MethodSelection {
    receiver: ExprId,
    member: Name,
    value: TypedExpression,
    candidates: Vec<MethodCandidate>,
    trait_tier: bool,
}

struct OverloadCandidate {
    callable: CallableSignature,
    bindings: Option<Vec<usize>>,
}

impl DispatchArgumentsFrame {
    fn new(
        request: DispatchRequest,
        selection: DispatchSelection,
        normal_continuation: NormalContinuation,
    ) -> Self {
        Self {
            request,
            selection,
            next: 0,
            typed: BTreeMap::new(),
            normal_continuation,
            poison: None,
            unperformed: false,
        }
    }
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_method_call(
        &mut self,
        context: BodyContext<'_, '_>,
        request: MethodCallRequest<'_>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        let member = request.member;
        if !member.is_valid() {
            return Ok(InvocationPlan::Complete(Outcome::Poisoned(
                source.invalid_poison(member.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: member.origin(),
                    },
                )?,
            )));
        }
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            SemanticLocation::origin(source.file(), member.origin()),
        ) {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }

        Ok(InvocationPlan::Expression {
            expression: request.receiver,
            request: TypeRequest::definitive(None),
            path: NormalContinuation::Present,
            continuation: InvocationContinuation::Dispatch(DispatchContinuation::Receiver(
                Box::new(MethodReceiverFrame {
                    request: DispatchRequest {
                        expression: request.expression,
                        origin: request.origin,
                        call: request.call.clone(),
                        ty: request.ty,
                    },
                    receiver: request.receiver,
                    member: member.clone(),
                }),
            )),
        })
    }

    fn resume_method_receiver(
        &mut self,
        context: BodyContext<'_, '_>,
        frame: MethodReceiverFrame,
        outcome: Outcome<TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let Outcome::Known(value) = outcome else {
            return Ok(InvocationPlan::Complete(outcome));
        };
        let BodyContext { module, source, .. } = context;
        let subject = self.method_subject(value.ty)?;
        let name =
            NameKey::from_hir(&frame.member).expect("valid method name has canonical spelling");
        let inherent_targets = self.inherent_method_targets(subject, &name)?;
        let (trait_tier, candidates) = if inherent_targets.is_empty() {
            let candidates = match self.trait_method_candidates(
                module,
                source,
                frame.request.origin,
                subject,
                &name,
            )? {
                Outcome::Known(candidates) => candidates,
                outcome => return Ok(InvocationPlan::Complete(outcome.map(|_| unreachable!()))),
            };
            (true, candidates)
        } else {
            if self.inherent_family_is_inaccessible(module, &inherent_targets)? {
                return self
                    .type_error(
                        source,
                        frame.member.origin(),
                        "VISIBILITY_VIOLATION",
                        "visibility violation: inherent method family is private",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned)
                    .map(InvocationPlan::Complete);
            }
            let candidates = match self.inherent_method_candidates(subject, &inherent_targets)? {
                Outcome::Known(candidates) => candidates,
                outcome => return Ok(InvocationPlan::Complete(outcome.map(|_| unreachable!()))),
            };
            (false, candidates)
        };
        if candidates.is_empty() {
            return self
                .method_not_found(source, &frame.member)
                .map(InvocationPlan::Complete);
        }

        let normal_continuation = value.normal_continuation;
        self.advance_dispatch_arguments(
            context,
            Box::new(DispatchArgumentsFrame::new(
                frame.request,
                DispatchSelection::Method(MethodSelection {
                    receiver: frame.receiver,
                    member: frame.member,
                    value,
                    candidates,
                    trait_tier,
                }),
                normal_continuation,
            )),
        )
    }

    pub(super) fn prepare_overloaded_call(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: &CallExpr,
        request: TypeRequest,
        targets: &[CanonicalTarget],
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }

        let mut candidates = Vec::with_capacity(targets.len());
        let mut poison = None;
        let mut unperformed = false;
        for target in targets {
            match self.signatures.declaration(target) {
                FactState::Known(DeclarationSignature::Callable(callable)) => {
                    let callable = self
                        .signatures
                        .callable(*callable)
                        .cloned()
                        .ok_or(BodyTypingError::UnknownCallable(*callable))?;
                    candidates.push(callable);
                }
                FactState::Poisoned(found) => {
                    let found = found.clone();
                    self.merge_poison(&mut poison, &found)?;
                }
                FactState::Unperformed => unperformed = true,
                FactState::Known(_) | FactState::NotApplicable => {
                    return Ok(InvocationPlan::Complete(self.unsupported(
                        TypecheckCapability::CheckClosures,
                        source,
                        origin,
                    )));
                }
            }
        }
        if let Some(poison) = poison {
            return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
        }
        if unperformed {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }

        let candidates = candidates
            .into_iter()
            .map(|callable| {
                let bindings = Self::bind_call_argument_indices(&callable, &call.arguments).ok();
                OverloadCandidate { callable, bindings }
            })
            .collect::<Vec<_>>();
        if candidates
            .iter()
            .all(|candidate| candidate.bindings.is_none())
        {
            let first = candidates
                .first()
                .ok_or(BodyTypingError::EmptyDeclarationSet)?;
            return match self.bind_call_slots(source, origin, &first.callable, &call.arguments)? {
                Outcome::Poisoned(poison) => {
                    Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)))
                }
                Outcome::Unperformed => Ok(InvocationPlan::Complete(Outcome::Unperformed)),
                Outcome::Deferred => Ok(InvocationPlan::Complete(Outcome::Deferred)),
                Outcome::Known(_) | Outcome::NotApplicable => {
                    Err(BodyTypingError::InconsistentOverloadBinding)
                }
            };
        }

        self.advance_dispatch_arguments(
            context,
            Box::new(DispatchArgumentsFrame::new(
                DispatchRequest {
                    expression,
                    origin,
                    call: call.clone(),
                    ty: request,
                },
                DispatchSelection::Overload(candidates),
                NormalContinuation::Present,
            )),
        )
    }

    fn advance_dispatch_arguments(
        &mut self,
        context: BodyContext<'_, '_>,
        mut frame: Box<DispatchArgumentsFrame>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        if let Some(argument) = frame.request.call.arguments.get(frame.next) {
            let argument = argument.value;
            frame.next += 1;
            return Ok(InvocationPlan::Expression {
                expression: argument,
                request: TypeRequest::definitive(None),
                path: frame.normal_continuation,
                continuation: InvocationContinuation::Dispatch(DispatchContinuation::Argument {
                    frame,
                    argument,
                }),
            });
        }
        if let Some(poison) = frame.poison.take() {
            return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
        }
        if frame.unperformed {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }

        match frame.selection {
            DispatchSelection::Method(selection) => {
                self.select_method(context, frame.request, selection, frame.typed)
            }
            DispatchSelection::Overload(candidates) => {
                self.select_overload(context, frame.request, candidates, frame.typed)
            }
        }
    }

    pub(super) fn resume_dispatch(
        &mut self,
        context: BodyContext<'_, '_>,
        continuation: DispatchContinuation,
        outcome: Outcome<TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        match continuation {
            DispatchContinuation::Receiver(frame) => {
                self.resume_method_receiver(context, *frame, outcome)
            }
            DispatchContinuation::Argument {
                mut frame,
                argument,
            } => {
                match outcome {
                    Outcome::Known(value) => {
                        frame.normal_continuation =
                            frame.normal_continuation.strict(value.normal_continuation);
                        frame.typed.insert(argument, value);
                    }
                    Outcome::Poisoned(found) => self.merge_poison(&mut frame.poison, &found)?,
                    Outcome::Unperformed | Outcome::NotApplicable => frame.unperformed = true,
                    Outcome::Deferred => return Ok(InvocationPlan::Complete(Outcome::Deferred)),
                }
                self.advance_dispatch_arguments(context, frame)
            }
        }
    }

    fn select_method(
        &mut self,
        context: BodyContext<'_, '_>,
        request: DispatchRequest,
        selection: MethodSelection,
        typed_arguments: BTreeMap<ExprId, TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        let call = &request.call;
        let explicit = match self.resolve_explicit_method_arguments(source, &call.type_arguments)? {
            Outcome::Known(arguments) => arguments,
            outcome => return Ok(InvocationPlan::Complete(outcome.map(|_| unreachable!()))),
        };
        let mut applicable = Vec::new();
        for candidate in &selection.candidates {
            let Ok(bindings) =
                Self::bind_method_argument_indices(&candidate.callable, &call.arguments)
            else {
                continue;
            };
            if !explicit.is_empty() && explicit.len() != candidate.explicit_parameters.len() {
                continue;
            }
            let Some((receiver_ty, passing)) =
                self.adjusted_method_receiver(&candidate.callable, &selection.value)?
            else {
                continue;
            };
            let mut actual = Vec::with_capacity(candidate.callable.parameters().len());
            actual.push(receiver_ty);
            actual.extend(
                bindings
                    .iter()
                    .map(|&argument| typed_arguments[&call.arguments[argument].value].ty),
            );
            let mut replacements = candidate.preset_replacements.clone();
            replacements.extend(
                candidate
                    .explicit_parameters
                    .iter()
                    .cloned()
                    .zip(explicit.iter().copied()),
            );
            if exact_parameter_match(
                self.signatures.types(),
                &candidate.callable,
                &actual,
                &mut replacements,
            )? {
                applicable.push((candidate.clone(), passing));
            }
        }

        match applicable.as_slice() {
            [] => self
                .method_not_found(source, &selection.member)
                .map(InvocationPlan::Complete),
            [(candidate, passing)] => self.prepare_selected_method(
                context,
                MethodCallRequest {
                    expression: request.expression,
                    origin: request.origin,
                    call,
                    receiver: selection.receiver,
                    member: &selection.member,
                    ty: request.ty,
                },
                candidate.clone(),
                *passing,
            ),
            candidates => self
                .method_ambiguous(
                    source,
                    &selection.member,
                    &candidates
                        .iter()
                        .map(|(candidate, _)| candidate.clone())
                        .collect::<Vec<_>>(),
                    selection.trait_tier,
                )
                .map(InvocationPlan::Complete),
        }
    }

    fn select_overload(
        &mut self,
        context: BodyContext<'_, '_>,
        request: DispatchRequest,
        candidates: Vec<OverloadCandidate>,
        typed_arguments: BTreeMap<ExprId, TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        let call = &request.call;
        let explicit_arguments = call
            .type_arguments
            .iter()
            .map(|&argument| self.type_occurrence(source, argument))
            .collect::<Result<Vec<_>, _>>()?;
        let mut explicit = Vec::with_capacity(explicit_arguments.len());
        let mut poison = None;
        let mut unperformed = false;
        for argument in explicit_arguments {
            match argument {
                Outcome::Known(argument) => explicit.push(argument),
                Outcome::Poisoned(found) => self.merge_poison(&mut poison, &found)?,
                Outcome::Deferred => return Ok(InvocationPlan::Complete(Outcome::Deferred)),
                Outcome::Unperformed | Outcome::NotApplicable => unperformed = true,
            }
        }
        if let Some(poison) = poison {
            return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
        }
        if unperformed {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }

        let mut applicable = Vec::new();
        let mut had_explicit_arity_candidate = explicit.is_empty();
        for candidate in &candidates {
            let Some(bindings) = &candidate.bindings else {
                continue;
            };
            let callable = &candidate.callable;
            if !explicit.is_empty() && explicit.len() != callable.generic_parameters().len() {
                continue;
            }
            had_explicit_arity_candidate = true;
            let actual = bindings
                .iter()
                .map(|&argument| typed_arguments[&call.arguments[argument].value].ty)
                .collect::<Vec<_>>();
            let mut replacements = if explicit.is_empty() {
                BTreeMap::new()
            } else {
                callable
                    .generic_parameters()
                    .iter()
                    .cloned()
                    .zip(explicit.iter().copied())
                    .collect()
            };
            if exact_parameter_match(
                self.signatures.types(),
                callable,
                &actual,
                &mut replacements,
            )? {
                applicable.push(callable.clone());
            }
        }

        match applicable.as_slice() {
            [] if !had_explicit_arity_candidate => self
                .type_argument_inference_failure(source, request.origin)
                .map(InvocationPlan::Complete),
            [] => {
                let candidates = candidates
                    .into_iter()
                    .map(|candidate| candidate.callable)
                    .collect::<Vec<_>>();
                self.overload_no_match(source, request.origin, call, &candidates, &typed_arguments)
                    .map(InvocationPlan::Complete)
            }
            [callable] => self.prepare_callable_call(
                context,
                request.expression,
                request.origin,
                call,
                request.ty,
                callable.clone(),
            ),
            _ => self
                .overload_ambiguous(source, request.origin, &applicable)
                .map(InvocationPlan::Complete),
        }
    }
}
