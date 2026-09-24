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
