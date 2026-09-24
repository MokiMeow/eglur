use super::*;

#[derive(Clone)]
struct CallableValueCandidate {
    callable: CallableSignature,
    replacements: BTreeMap<GenericParameterId, TypeId>,
    ty: TypeId,
}

enum CandidateOutcome {
    Ready(CallableValueCandidate),
    Underconstrained,
    NormalizationFailed(TraitNormalization),
    Mismatch,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_qualified_member_value(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        member: &Name,
        owner: &CanonicalTarget,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        match self.signatures.declaration(owner) {
            FactState::Known(DeclarationSignature::Trait { .. }) => self
                .type_qualified_trait_method_value(
                    module, source, expression, member, owner, request,
                ),
            FactState::Known(DeclarationSignature::Struct { .. }) => self
                .type_qualified_concrete_method_value(
                    module, source, expression, member, owner, request,
                ),
            FactState::Known(DeclarationSignature::Nominal { .. })
                if matches!(owner, CanonicalTarget::External(_)) =>
            {
                let origin = self.expression_origin(source, expression)?;
                Ok(self.unsupported(TypecheckCapability::CheckClosures, source, origin))
            }
            FactState::Known(_) | FactState::NotApplicable => self.method_not_found(source, member),
            FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => Ok(Outcome::Unperformed),
        }
    }

    fn type_qualified_concrete_method_value(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        member: &Name,
        owner: &CanonicalTarget,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let origin = self.expression_origin(source, expression)?;
        let Some(name) = NameKey::from_hir(member) else {
            return Ok(Outcome::Poisoned(
                source.invalid_poison(member.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: member.origin(),
                    },
                )?,
            ));
        };
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            SemanticLocation::origin(source.file(), member.origin()),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let CanonicalTarget::Source(owner_id) = owner else {
            return Ok(self.unsupported(TypecheckCapability::CheckClosures, source, origin));
        };
        let targets = self
            .collection
            .child_candidates(*owner_id, &name, DeclarationKind::Function);
        if targets.is_empty() {
            return self.method_not_found(source, member);
        }
        if self.inherent_family_is_inaccessible(module, &targets)? {
            return self
                .type_error(
                    source,
                    member.origin(),
                    "VISIBILITY_VIOLATION",
                    "visibility violation: inherent method family is private",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }

        let expected = match request.expected {
            Some(ty) if matches!(self.signatures.types().kind(ty)?, TypeKind::Callable { .. }) => {
                Some(ty)
            }
            _ => None,
        };
        let generic_parameters = match self.signatures.declaration(owner) {
            FactState::Known(DeclarationSignature::Struct {
                generic_parameters, ..
            }) => generic_parameters.len(),
            _ => return Err(BodyTypingError::UnknownDeclaration(*owner_id)),
        };
        let subject = if generic_parameters == 0 {
            self.signatures
                .types_mut()
                .nominal(NominalHead::Declaration(owner.clone()), [])?
        } else if let Some(expected) = expected {
            let TypeKind::Callable { parameters, .. } = self.signatures.types().kind(expected)?
            else {
                unreachable!("expected callable type was checked above");
            };
            let Some(&receiver) = parameters.first() else {
                return self.type_argument_inference_failure(source, origin);
            };
            self.method_subject(receiver)?
        } else {
            return self.missing_callable_value_context(source, origin, request);
        };
        let TypeKind::Nominal {
            head: NominalHead::Declaration(found),
            ..
        } = self.signatures.types().kind(subject)?
        else {
            return self.type_argument_inference_failure(source, origin);
        };
        if found != owner {
            return self.type_argument_inference_failure(source, origin);
        }
        let methods = match self.inherent_method_candidates(subject, &targets)? {
            Outcome::Known(methods) => methods,
            outcome => return Ok(outcome.map(|_| unreachable!())),
        };
        let overloaded = methods.len() > 1;
        if overloaded && expected.is_none() {
            return self.missing_callable_value_context(source, origin, request);
        }

        let mut matched = Vec::new();
        let mut presented = Vec::new();
        let mut first_ready = None;
        let mut underconstrained = false;
        let mut failed_normalization = None;
        for candidate in methods {
            let callable = candidate.callable;
            presented.push(callable.clone());
            if callable.is_bang() {
                continue;
            }
            match self.candidate_callable_value(callable, expected, overloaded)? {
                CandidateOutcome::Ready(value) => {
                    if first_ready.is_none() {
                        first_ready = Some(value.clone());
                    }
                    let compatible = match expected {
                        Some(expected) => {
                            self.signatures.types().is_assignable(value.ty, expected)?
                        }
                        None => true,
                    };
                    if compatible {
                        matched.push(value);
                    }
                }
                CandidateOutcome::Underconstrained => underconstrained = true,
                CandidateOutcome::NormalizationFailed(failure) => {
                    failed_normalization = Some(failure);
                }
                CandidateOutcome::Mismatch => {}
            }
        }

        let selected = match matched.len() {
            1 => matched.pop().expect("one compatible method"),
            n if n > 1 => {
                let callables = matched
                    .iter()
                    .map(|value| value.callable.clone())
                    .collect::<Vec<_>>();
                return self.overload_ambiguous(source, origin, &callables);
            }
            _ if underconstrained => {
                return self.missing_callable_value_context(source, origin, request);
            }
            _ if !overloaded && first_ready.is_some() => {
                first_ready.expect("one ready method value")
            }
            _ if failed_normalization.is_some() => {
                return self.normalization_failure_at(
                    source,
                    origin,
                    failed_normalization.expect("normalization failure exists"),
                );
            }
            _ if presented.iter().all(CallableSignature::is_bang) => {
                return Ok(self.unsupported(TypecheckCapability::CheckClosures, source, origin));
            }
            _ if !overloaded => return self.type_argument_inference_failure(source, origin),
            _ => return self.callable_value_no_match(source, origin, expected, &presented),
        };
        self.publish_callable_value(module, source, expression, origin, selected)
    }

    pub(super) fn type_qualified_trait_method_value(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        member: &Name,
        trait_target: &CanonicalTarget,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let origin = self.expression_origin(source, expression)?;
        let Some(expected) = request.expected else {
            return self.missing_callable_value_context(source, origin, request);
        };
        let TypeKind::Callable { parameters, .. } = self.signatures.types().kind(expected)? else {
            return self.missing_callable_value_context(source, origin, request);
        };
        let Some(&receiver_type) = parameters.first() else {
            return self.type_argument_inference_failure(source, origin);
        };
        let Some(name) = NameKey::from_hir(member) else {
            return Ok(Outcome::Poisoned(
                source.invalid_poison(member.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: member.origin(),
                    },
                )?,
            ));
        };
        if !matches!(
            self.signatures.declaration(trait_target),
            FactState::Known(DeclarationSignature::Trait { .. })
        ) {
            return Ok(self.unsupported(TypecheckCapability::CheckClosures, source, origin));
        }
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            SemanticLocation::origin(source.file(), member.origin()),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let methods = self
            .signatures
            .trait_methods(trait_target)
            .iter()
            .filter(|method| method.name() == &name)
            .map(|method| method.declaration().clone())
            .collect::<Vec<_>>();
        if methods.is_empty() {
            return self.method_not_found(source, member);
        }
        if member.is_bang() {
            return Ok(self.unsupported(TypecheckCapability::CheckClosures, source, origin));
        }

        let mut matched = Vec::new();
        let mut presented = Vec::new();
        let mut underconstrained = false;
        let mut failed_normalization = None;
        let mut receiver_matched = false;
        let mut missing_subject = None;
        for method in methods {
            let callable = match self.signatures.declaration(&method) {
                FactState::Known(DeclarationSignature::Callable(id)) => self
                    .signatures
                    .callable(*id)
                    .cloned()
                    .ok_or(BodyTypingError::UnknownCallable(*id))?,
                FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                _ => continue,
            };
            if callable.is_bang() {
                continue;
            }
            let Some(subject) =
                self.qualified_receiver_subject(trait_target, &callable, receiver_type)?
            else {
                continue;
            };
            receiver_matched = true;
            missing_subject = Some(subject);
            let candidates = match self.trait_method_candidates_for_owner(
                source,
                member.origin(),
                subject,
                &name,
                trait_target,
            )? {
                Outcome::Known(candidates) => candidates,
                outcome => return Ok(outcome.map(|_| unreachable!())),
            };
            for candidate in candidates
                .into_iter()
                .filter(|candidate| candidate.callable.id() == callable.id())
            {
                let specialized = candidate
                    .callable
                    .with_specialized_contract(
                        candidate.explicit_parameters.iter().cloned(),
                        candidate
                            .callable
                            .parameters()
                            .iter()
                            .map(CallableParameter::ty),
                        candidate.callable.result(),
                    )
                    .ok_or(BodyTypingError::MethodSpecializationArityMismatch(
                        callable.id(),
                    ))?;
                presented.push(specialized.clone());
                match self.candidate_callable_value(specialized, Some(expected), true)? {
                    CandidateOutcome::Ready(mut value)
                        if self.signatures.types().is_assignable(value.ty, expected)? =>
                    {
                        value
                            .replacements
                            .extend(candidate.preset_replacements.clone());
                        matched.push((candidate, value));
                    }
                    CandidateOutcome::Underconstrained => underconstrained = true,
                    CandidateOutcome::NormalizationFailed(failure) => {
                        failed_normalization = Some(failure);
                    }
                    CandidateOutcome::Ready(_) | CandidateOutcome::Mismatch => {}
                }
            }
        }

        let (candidate, selected) = match matched.len() {
            1 => matched.pop().expect("one compatible method"),
            n if n > 1 => {
                let callables = matched
                    .iter()
                    .map(|(_, value)| value.callable.clone())
                    .collect::<Vec<_>>();
                return self.overload_ambiguous(source, origin, &callables);
            }
            _ if !receiver_matched || underconstrained => {
                return self.type_argument_inference_failure(source, origin);
            }
            _ if presented.is_empty() => {
                return self.qualified_trait_impl_missing(
                    source,
                    member.origin(),
                    trait_target,
                    missing_subject.expect("matched receiver determines Self"),
                );
            }
            _ if failed_normalization.is_some() => {
                return self.normalization_failure_at(
                    source,
                    origin,
                    failed_normalization.expect("normalization failure exists"),
                );
            }
            _ => return self.callable_value_no_match(source, origin, Some(expected), &presented),
        };
        let result =
            self.publish_callable_value(module, source, expression, origin, selected.clone())?;
        if matches!(result, Outcome::Known(_)) {
            let MethodDispatchTemplate::Trait {
                subject,
                trait_ref,
                method,
            } = candidate.dispatch
            else {
                unreachable!("qualified trait method retains a trait dispatch");
            };
            let trait_ref = self.specialize_trait_ref(&trait_ref, &selected.replacements)?;
            self.method_dispatches.push(PendingMethodDispatch {
                site: MethodDispatchSite::CallableValue(ExpressionKey::new(module, expression)),
                owner: self
                    .current_owner
                    .ok_or(BodyTypingError::MissingDiagnosticOwner)?,
                location: SemanticLocation::origin(source.file(), member.origin()),
                subject,
                trait_ref,
                method,
            });
        }
        Ok(result)
    }

    pub(super) fn type_named_callable_value(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        request: TypeRequest,
        signatures: &[CallableSignatureId],
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let origin = self.expression_origin(source, expression)?;
        let expected = match request.expected {
            Some(ty) if matches!(self.signatures.types().kind(ty)?, TypeKind::Callable { .. }) => {
                Some(ty)
            }
            _ => None,
        };
        if signatures.len() > 1 && expected.is_none() {
            return self.missing_callable_value_context(source, origin, request);
        }

        let mut candidates = Vec::with_capacity(signatures.len());
        let mut applicable = Vec::new();
        let mut first_ready = None;
        let mut underconstrained = false;
        let mut failed_normalizations = Vec::new();
        for &signature in signatures {
            let callable = self
                .signatures
                .callable(signature)
                .cloned()
                .ok_or(BodyTypingError::UnknownCallable(signature))?;
            candidates.push(callable.clone());
            if callable.is_bang() {
                continue;
            }

            match self.candidate_callable_value(callable, expected, signatures.len() > 1)? {
                CandidateOutcome::Ready(candidate) => {
                    if first_ready.is_none() {
                        first_ready = Some(candidate.clone());
                    }
                    let compatible = match expected {
                        Some(expected) => self
                            .signatures
                            .types()
                            .is_assignable(candidate.ty, expected)?,
                        None => true,
                    };
                    if compatible {
                        applicable.push(candidate);
                    }
                }
                CandidateOutcome::Underconstrained => underconstrained = true,
                CandidateOutcome::NormalizationFailed(failure) => {
                    failed_normalizations.push(failure);
                }
                CandidateOutcome::Mismatch => {}
            }
        }

        let selected = match applicable.len() {
            1 => applicable.pop().expect("one applicable candidate"),
            n if n > 1 => {
                let compatible = applicable
                    .iter()
                    .map(|candidate| candidate.callable.clone())
                    .collect::<Vec<_>>();
                return self.overload_ambiguous(source, origin, &compatible);
            }
            _ if underconstrained => {
                return self.missing_callable_value_context(source, origin, request);
            }
            _ if signatures.len() == 1 => {
                if let Some(candidate) = first_ready {
                    candidate
                } else if let Some(failure) = failed_normalizations.pop() {
                    return self.normalization_failure_at(source, origin, failure);
                } else if candidates[0].is_bang() {
                    return Ok(self.unsupported(
                        TypecheckCapability::CheckClosures,
                        source,
                        origin,
                    ));
                } else {
                    return self.type_argument_inference_failure(source, origin);
                }
            }
            _ if failed_normalizations.len() == 1 => {
                return self.normalization_failure_at(
                    source,
                    origin,
                    failed_normalizations
                        .pop()
                        .expect("one normalization failure"),
                );
            }
            _ => return self.callable_value_no_match(source, origin, expected, &candidates),
        };

        self.publish_callable_value(module, source, expression, origin, selected)
    }

    fn publish_callable_value(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        origin: OriginId,
        selected: CallableValueCandidate,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        if let Some(poison) = self.validate_type_arguments(
            source,
            origin,
            selected.callable.generic_parameters(),
            &selected.replacements,
            None,
        )? {
            return Ok(Outcome::Poisoned(poison));
        }
        if let Some(poison) = self.check_callable_requirements(
            source,
            origin,
            selected.callable.requirements(),
            &selected.replacements,
        )? {
            return Ok(Outcome::Poisoned(poison));
        }

        let key = ExpressionKey::new(module, expression);
        self.record_trait_obligations(
            key,
            source,
            origin,
            selected.callable.generic_parameters(),
            &selected.replacements,
        )?;
        let arguments = selected
            .callable
            .generic_parameters()
            .iter()
            .map(|parameter| selected.replacements[parameter])
            .collect::<Vec<_>>();
        let substitution = self.signatures.types_mut().intern_substitution(arguments)?;
        let instance = self.intern_callable_instance(selected.callable.id(), substitution)?;
        self.callable_values
            .insert(key, SemanticFact::Known(instance))?;
        Ok(Outcome::Known(self.value_expression(selected.ty)))
    }

    fn candidate_callable_value(
        &mut self,
        callable: CallableSignature,
        expected: Option<TypeId>,
        filter_parameters: bool,
    ) -> Result<CandidateOutcome, BodyTypingError> {
        let expected_parameters = if filter_parameters {
            match expected {
                Some(expected) => match self.signatures.types().kind(expected)? {
                    TypeKind::Callable { parameters, .. } => Some(parameters.to_vec()),
                    _ => None,
                },
                None => None,
            }
        } else {
            None
        };
        if expected_parameters
            .as_ref()
            .is_some_and(|parameters| parameters.len() != callable.parameters().len())
        {
            return Ok(CandidateOutcome::Mismatch);
        }

        let parameters = callable
            .generic_parameters()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut replacements = BTreeMap::new();
        if !parameters.is_empty() {
            let Some(expected) = expected else {
                return Ok(CandidateOutcome::Underconstrained);
            };
            let template = self.signatures.types_mut().callable(
                callable.parameters().iter().map(CallableParameter::ty),
                callable.result(),
            )?;
            if self.infer_constraints(template, expected, &parameters, &mut replacements)?
                == ConstraintOutcome::Conflict
            {
                return Ok(CandidateOutcome::Mismatch);
            }
            if !parameters
                .iter()
                .all(|parameter| replacements.contains_key(parameter))
            {
                return Ok(CandidateOutcome::Underconstrained);
            }
        }

        let mut specialized = Vec::with_capacity(callable.parameters().len());
        for (index, parameter) in callable.parameters().iter().enumerate() {
            let ty = self
                .signatures
                .types_mut()
                .substitute(parameter.ty(), &replacements)?;
            let ty = match self.normalize_callable_candidate_type(ty)? {
                Ok(ty) => ty,
                Err(failure) => return Ok(CandidateOutcome::NormalizationFailed(failure)),
            };
            if let Some(expected) = expected_parameters
                .as_ref()
                .map(|parameters| parameters[index])
                && !self.signatures.types().is_assignable(expected, ty)?
            {
                return Ok(CandidateOutcome::Mismatch);
            }
            specialized.push(ty);
        }
        let result = self
            .signatures
            .types_mut()
            .substitute(callable.result(), &replacements)?;
        let result = match self.normalize_callable_candidate_type(result)? {
            Ok(ty) => ty,
            Err(failure) => return Ok(CandidateOutcome::NormalizationFailed(failure)),
        };
        let ty = self.signatures.types_mut().callable(specialized, result)?;
        Ok(CandidateOutcome::Ready(CallableValueCandidate {
            callable,
            replacements,
            ty,
        }))
    }

    fn normalize_callable_candidate_type(
        &mut self,
        ty: TypeId,
    ) -> Result<Result<TypeId, TraitNormalization>, BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        Ok(
            match self
                .trait_environment
                .normalize_for_owner(owner, self.signatures, ty)?
            {
                TraitNormalization::Normalized(ty) | TraitNormalization::Symbolic(ty) => Ok(ty),
                failure => Err(failure),
            },
        )
    }

    fn missing_callable_value_context(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        if request.allow_deferred {
            Ok(Outcome::Deferred)
        } else {
            self.type_argument_inference_failure(source, origin)
        }
    }

    fn callable_value_no_match(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        expected: Option<TypeId>,
        candidates: &[CallableSignature],
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let expected = expected.expect("an overload value requires expected Fn context");
        let rendered = self.format_callable_candidates(candidates)?;
        let related = self.callable_candidate_spans(candidates, &rendered)?;
        let poison = self.type_error_with_related(
            source,
            origin,
            "CALLABLE_VALUE_NO_MATCH",
            "callable value has no matching candidate",
            related,
            BTreeMap::from([
                (
                    Box::<str>::from("expected_type"),
                    DiagnosticDataValue::from(self.format_type(expected)?),
                ),
                (
                    Box::<str>::from("candidates"),
                    DiagnosticDataValue::from(rendered.join("; ")),
                ),
            ]),
        )?;
        Ok(Outcome::Poisoned(poison))
    }
}
