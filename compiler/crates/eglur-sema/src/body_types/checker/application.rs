use std::ops::ControlFlow;

use super::*;

pub(super) enum ApplicationPlan {
    Complete(Outcome<ResolvedApplication>),
    Expression {
        expression: ExprId,
        request: TypeRequest,
        path: NormalContinuation,
        continuation: ApplicationContinuation,
    },
}

pub(super) enum ApplicationContinuation {
    Infer {
        frame: Box<InferenceFrame>,
        index: usize,
        transfer_checkpoint: usize,
    },
    Check {
        frame: Box<CheckingFrame>,
        index: usize,
        transfer_checkpoint: usize,
    },
}

struct ApplicationArgument {
    slot: ApplicationSlot,
    typed: Option<TypedExpression>,
    // A deferred earlier argument may later diverge; keep exits for source-order pruning.
    transfers: Vec<ControlTransfer>,
}

struct ApplicationArguments {
    values: Box<[ApplicationArgument]>,
    evaluation_order: Vec<usize>,
    enclosing_regions: Vec<ControlRegionKey>,
}

struct PendingApplication {
    origin: OriginId,
    parameters: Box<[GenericParameterId]>,
    parameter_set: BTreeSet<GenericParameterId>,
    arguments: ApplicationArguments,
    result_template: TypeId,
    replacements: BTreeMap<GenericParameterId, TypeId>,
    allowed_ownership_parameter: Option<GenericParameterId>,
    allow_deferred: bool,
    defer_underconstrained: bool,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum InferencePass {
    Probe,
    Definitive,
}

pub(super) struct InferenceFrame {
    application: PendingApplication,
    pass: InferencePass,
    next: usize,
    normal_continuation: NormalContinuation,
    poison: Option<Poison>,
    unperformed: bool,
    conflict: bool,
}

pub(super) struct CheckingFrame {
    origin: OriginId,
    allow_deferred: bool,
    arguments: ApplicationArguments,
    substitution: SubstId,
    replacements: BTreeMap<GenericParameterId, TypeId>,
    specialized_slots: Box<[TypeId]>,
    result: TypeId,
    next: usize,
    normal_continuation: NormalContinuation,
    poison: Option<Poison>,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_application(
        &mut self,
        source: &SemanticSource<'_>,
        request: ApplicationRequest<'_>,
    ) -> Result<ApplicationPlan, BodyTypingError> {
        let ApplicationRequest {
            origin,
            generic_parameters,
            slots,
            result_template,
            explicit_arguments,
            explicit_parameters,
            preset_replacements,
            allowed_ownership_parameter,
            ty: request,
            defer_underconstrained,
        } = request;
        let mut evaluation_order = (0..slots.len()).collect::<Vec<_>>();
        evaluation_order.sort_by_key(|&index| slots[index].evaluation_order);
        let enclosing_regions = self.region_stack.clone();
        let parameter_set = generic_parameters.iter().cloned().collect::<BTreeSet<_>>();
        let explicit_parameters = explicit_parameters.unwrap_or(generic_parameters);
        let mut replacements = preset_replacements.cloned().unwrap_or_default();
        let explicit = !explicit_arguments.is_empty();
        if explicit {
            if explicit_arguments.len() != explicit_parameters.len() {
                return self
                    .type_argument_inference_failure(source, origin)
                    .map(ApplicationPlan::Complete);
            }
            for (parameter, &argument) in explicit_parameters.iter().zip(explicit_arguments) {
                match self.type_occurrence(source, argument)? {
                    Outcome::Known(ty) => {
                        replacements.insert(parameter.clone(), ty);
                    }
                    outcome => return Ok(ApplicationPlan::Complete(application_refusal(outcome))),
                }
            }
        } else if let Some(expected) = request.expected
            && self.infer_constraints(
                result_template,
                expected,
                &parameter_set,
                &mut replacements,
            )? == ConstraintOutcome::Conflict
        {
            return self
                .type_argument_inference_failure(source, origin)
                .map(ApplicationPlan::Complete);
        }

        let application = PendingApplication {
            origin,
            parameters: generic_parameters.into(),
            parameter_set,
            arguments: ApplicationArguments {
                values: slots
                    .iter()
                    .cloned()
                    .map(|slot| ApplicationArgument {
                        slot,
                        typed: None,
                        transfers: Vec::new(),
                    })
                    .collect(),
                evaluation_order,
                enclosing_regions,
            },
            result_template,
            replacements,
            allowed_ownership_parameter: allowed_ownership_parameter.cloned(),
            allow_deferred: request.allow_deferred,
            defer_underconstrained,
        };
        if explicit {
            self.prepare_application_checking(source, application, false)
        } else {
            self.advance_application_inference(
                source,
                Box::new(InferenceFrame {
                    application,
                    pass: InferencePass::Probe,
                    next: 0,
                    normal_continuation: NormalContinuation::Present,
                    poison: None,
                    unperformed: false,
                    conflict: false,
                }),
            )
        }
    }

    fn advance_application_inference(
        &mut self,
        source: &SemanticSource<'_>,
        mut frame: Box<InferenceFrame>,
    ) -> Result<ApplicationPlan, BodyTypingError> {
        loop {
            let application = &frame.application;
            if let Some(&index) = application.arguments.evaluation_order.get(frame.next) {
                frame.next += 1;
                let argument = &application.arguments.values[index];
                if frame.pass == InferencePass::Definitive
                    && let Some(value) = &argument.typed
                {
                    frame.normal_continuation =
                        frame.normal_continuation.strict(value.normal_continuation);
                    continue;
                }
                let expected = match self.specialize_if_resolved(
                    argument.slot.template,
                    &application.parameter_set,
                    &application.replacements,
                )? {
                    Some(expected) => {
                        match self.normalize_type_at(source, argument.slot.origin, expected)? {
                            Outcome::Known(expected) => Some(expected),
                            outcome => {
                                return Ok(ApplicationPlan::Complete(application_refusal(outcome)));
                            }
                        }
                    }
                    None => None,
                };
                let request = match frame.pass {
                    InferencePass::Probe => expected.map_or_else(TypeRequest::probe, |expected| {
                        TypeRequest::definitive(Some(expected))
                    }),
                    InferencePass::Definitive => TypeRequest::definitive(expected),
                };
                return Ok(ApplicationPlan::Expression {
                    expression: argument.slot.value,
                    request,
                    path: frame.normal_continuation,
                    continuation: ApplicationContinuation::Infer {
                        frame,
                        index,
                        transfer_checkpoint: self.transfers.len(),
                    },
                });
            }

            if let Some(poison) = frame.poison {
                return Ok(ApplicationPlan::Complete(Outcome::Poisoned(poison)));
            }
            if frame.unperformed {
                return Ok(ApplicationPlan::Complete(Outcome::Unperformed));
            }
            if frame.pass == InferencePass::Probe {
                if frame.conflict {
                    return self
                        .type_argument_inference_failure(source, frame.application.origin)
                        .map(ApplicationPlan::Complete);
                }
                frame.pass = InferencePass::Definitive;
                frame.next = 0;
                frame.normal_continuation = NormalContinuation::Present;
            } else {
                return self.prepare_application_checking(
                    source,
                    frame.application,
                    frame.conflict,
                );
            }
        }
    }

    pub(super) fn resume_application(
        &mut self,
        source: &SemanticSource<'_>,
        continuation: ApplicationContinuation,
        outcome: Outcome<TypedExpression>,
    ) -> Result<ApplicationPlan, BodyTypingError> {
        match continuation {
            ApplicationContinuation::Infer {
                mut frame,
                index,
                transfer_checkpoint,
            } => {
                let application = &mut frame.application;
                let argument = &mut application.arguments.values[index];
                argument
                    .transfers
                    .extend_from_slice(&self.transfers[transfer_checkpoint..]);
                match outcome {
                    Outcome::Known(value) => {
                        let value = self.adjust_application_value(&argument.slot, value)?;
                        frame.normal_continuation =
                            frame.normal_continuation.strict(value.normal_continuation);
                        if self.infer_constraints(
                            argument.slot.template,
                            value.ty,
                            &application.parameter_set,
                            &mut application.replacements,
                        )? == ConstraintOutcome::Conflict
                        {
                            frame.conflict = true;
                        }
                        argument.typed = Some(value);
                    }
                    Outcome::Poisoned(found) => self.merge_poison(&mut frame.poison, &found)?,
                    Outcome::Deferred => {}
                    Outcome::Unperformed | Outcome::NotApplicable => frame.unperformed = true,
                }
                self.advance_application_inference(source, frame)
            }
            ApplicationContinuation::Check {
                mut frame,
                index,
                transfer_checkpoint,
            } => {
                frame.arguments.values[index]
                    .transfers
                    .extend_from_slice(&self.transfers[transfer_checkpoint..]);
                match self.check_application_argument(source, &mut frame, index, outcome)? {
                    ControlFlow::Continue(()) => self.advance_application_checking(source, frame),
                    ControlFlow::Break(outcome) => Ok(ApplicationPlan::Complete(outcome)),
                }
            }
        }
    }

    fn prepare_application_checking(
        &mut self,
        source: &SemanticSource<'_>,
        application: PendingApplication,
        conflict: bool,
    ) -> Result<ApplicationPlan, BodyTypingError> {
        let complete = application
            .parameters
            .iter()
            .all(|parameter| application.replacements.contains_key(parameter));
        if conflict || !complete {
            if !conflict && (application.allow_deferred || application.defer_underconstrained) {
                return Ok(ApplicationPlan::Complete(Outcome::Deferred));
            }
            return self
                .type_argument_inference_failure(source, application.origin)
                .map(ApplicationPlan::Complete);
        }
        if let Some(poison) = self.validate_type_arguments(
            source,
            application.origin,
            &application.parameters,
            &application.replacements,
            application.allowed_ownership_parameter.as_ref(),
        )? {
            return Ok(ApplicationPlan::Complete(Outcome::Poisoned(poison)));
        }

        let arguments = application
            .parameters
            .iter()
            .map(|parameter| application.replacements[parameter])
            .collect::<Vec<_>>();
        let substitution = self.signatures.types_mut().intern_substitution(arguments)?;
        let mut specialized_slots = Vec::with_capacity(application.arguments.values.len());
        for argument in &application.arguments.values {
            let specialized = self
                .signatures
                .types_mut()
                .substitute(argument.slot.template, &application.replacements)?;
            match self.normalize_type_at(source, argument.slot.origin, specialized)? {
                Outcome::Known(ty) => specialized_slots.push(ty),
                outcome => return Ok(ApplicationPlan::Complete(application_refusal(outcome))),
            }
        }
        let result = self
            .signatures
            .types_mut()
            .substitute(application.result_template, &application.replacements)?;
        let result = match self.normalize_type_at(source, application.origin, result)? {
            Outcome::Known(ty) => ty,
            outcome => return Ok(ApplicationPlan::Complete(application_refusal(outcome))),
        };
        self.advance_application_checking(
            source,
            Box::new(CheckingFrame {
                origin: application.origin,
                allow_deferred: application.allow_deferred,
                arguments: application.arguments,
                substitution,
                replacements: application.replacements,
                specialized_slots: specialized_slots.into_boxed_slice(),
                result,
                next: 0,
                normal_continuation: NormalContinuation::Present,
                poison: None,
            }),
        )
    }

    fn advance_application_checking(
        &mut self,
        source: &SemanticSource<'_>,
        mut frame: Box<CheckingFrame>,
    ) -> Result<ApplicationPlan, BodyTypingError> {
        while let Some(&index) = frame.arguments.evaluation_order.get(frame.next) {
            frame.next += 1;
            let argument = &frame.arguments.values[index];
            if !frame.normal_continuation.is_present() {
                self.discard_unreachable_enclosing_transfers(
                    &argument.transfers,
                    &frame.arguments.enclosing_regions,
                );
            }
            if let Some(value) = argument.typed.clone() {
                if let ControlFlow::Break(outcome) = self.check_application_argument(
                    source,
                    &mut frame,
                    index,
                    Outcome::Known(value),
                )? {
                    return Ok(ApplicationPlan::Complete(outcome));
                }
            } else {
                return Ok(ApplicationPlan::Expression {
                    expression: argument.slot.value,
                    request: TypeRequest::definitive(Some(frame.specialized_slots[index])),
                    path: frame.normal_continuation,
                    continuation: ApplicationContinuation::Check {
                        frame,
                        index,
                        transfer_checkpoint: self.transfers.len(),
                    },
                });
            }
        }
        if let Some(poison) = frame.poison {
            return Ok(ApplicationPlan::Complete(Outcome::Poisoned(poison)));
        }
        Ok(ApplicationPlan::Complete(Outcome::Known(
            ResolvedApplication {
                substitution: frame.substitution,
                replacements: frame.replacements,
                specialized_slots: frame.specialized_slots,
                result: frame.result,
                normal_continuation: frame.normal_continuation,
            },
        )))
    }

    fn check_application_argument(
        &mut self,
        source: &SemanticSource<'_>,
        frame: &mut CheckingFrame,
        index: usize,
        outcome: Outcome<TypedExpression>,
    ) -> Result<ControlFlow<Outcome<ResolvedApplication>>, BodyTypingError> {
        let slot = &frame.arguments.values[index].slot;
        let expected = frame.specialized_slots[index];
        let outcome = match outcome {
            Outcome::Known(value) => Outcome::Known(self.adjust_application_value(slot, value)?),
            outcome => outcome,
        };
        match outcome {
            Outcome::Known(value) => {
                let assignable = self.signatures.types().is_assignable(value.ty, expected)?;
                frame.normal_continuation =
                    frame.normal_continuation.strict(value.normal_continuation);
                if !assignable {
                    let found = self.call_argument_mismatch(source, slot, expected, value.ty)?;
                    self.merge_poison(&mut frame.poison, &found)?;
                }
            }
            Outcome::Poisoned(found) => self.merge_poison(&mut frame.poison, &found)?,
            Outcome::Deferred => {
                let outcome = if frame.allow_deferred {
                    Outcome::Deferred
                } else {
                    self.type_argument_inference_failure(source, frame.origin)?
                };
                return Ok(ControlFlow::Break(outcome));
            }
            Outcome::Unperformed | Outcome::NotApplicable => {
                return Ok(ControlFlow::Break(Outcome::Unperformed));
            }
        }
        Ok(ControlFlow::Continue(()))
    }
}

fn application_refusal<T>(outcome: Outcome<T>) -> Outcome<ResolvedApplication> {
    match outcome {
        Outcome::Known(_) => unreachable!("known application values are handled before refusal"),
        Outcome::Poisoned(poison) => Outcome::Poisoned(poison),
        Outcome::Deferred => Outcome::Deferred,
        Outcome::Unperformed | Outcome::NotApplicable => Outcome::Unperformed,
    }
}
