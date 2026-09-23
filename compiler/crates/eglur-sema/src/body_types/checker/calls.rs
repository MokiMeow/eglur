use super::application::ApplicationPlan;
use super::invocation::InvocationFrame;
use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn method_subject(&self, receiver: TypeId) -> Result<TypeId, BodyTypingError> {
        Ok(match self.signatures.types().kind(receiver)? {
            TypeKind::Reference { referent, .. } => *referent,
            _ => receiver,
        })
    }

    pub(super) fn inherent_method_targets(
        &self,
        subject: TypeId,
        name: &NameKey,
    ) -> Result<Vec<DefId>, BodyTypingError> {
        let TypeKind::Nominal {
            head: NominalHead::Declaration(CanonicalTarget::Source(owner)),
            ..
        } = self.signatures.types().kind(subject)?
        else {
            return Ok(Vec::new());
        };
        Ok(self
            .collection
            .child_candidates(*owner, name, DeclarationKind::Function))
    }

    pub(super) fn inherent_family_is_inaccessible(
        &self,
        module: ModuleId,
        methods: &[DefId],
    ) -> Result<bool, BodyTypingError> {
        let Some(first) = methods.first() else {
            return Ok(false);
        };
        let method = self
            .collection
            .declaration(*first)
            .ok_or(BodyTypingError::UnknownDeclaration(*first))?;
        let owner = method
            .parent()
            .ok_or(BodyTypingError::MissingDeclarationParent(*first))?;
        if owner.module() == module {
            return Ok(false);
        }
        let owner = self
            .collection
            .declaration(owner)
            .ok_or(BodyTypingError::UnknownDeclaration(owner))?;
        Ok(!owner.visibility().is_public()
            || methods.iter().any(|method| {
                self.collection
                    .declaration(*method)
                    .is_some_and(|method| !method.visibility().is_public())
            }))
    }

    pub(super) fn inherent_method_candidates(
        &mut self,
        subject: TypeId,
        targets: &[DefId],
    ) -> Result<Outcome<Vec<MethodCandidate>>, BodyTypingError> {
        let TypeKind::Nominal {
            head: NominalHead::Declaration(owner),
            substitution,
        } = self.signatures.types().kind(subject)?.clone()
        else {
            return Ok(Outcome::Known(Vec::new()));
        };
        let generic_parameters = match self.signatures.declaration(&owner) {
            FactState::Known(DeclarationSignature::Struct {
                generic_parameters, ..
            }) => generic_parameters.to_vec(),
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => return Ok(Outcome::Unperformed),
            FactState::Known(_) | FactState::NotApplicable => {
                return Ok(Outcome::Known(Vec::new()));
            }
        };
        let arguments = self.signatures.types().substitution(substitution)?.to_vec();
        let replacements = generic_parameters
            .into_iter()
            .zip(arguments)
            .collect::<BTreeMap<_, _>>();
        let mut candidates = Vec::new();
        for &target in targets {
            let target = CanonicalTarget::Source(target);
            let callable = match self.signatures.declaration(&target) {
                FactState::Known(DeclarationSignature::Callable(callable)) => self
                    .signatures
                    .callable(*callable)
                    .cloned()
                    .ok_or(BodyTypingError::UnknownCallable(*callable))?,
                FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                FactState::Known(_) | FactState::NotApplicable => continue,
            };
            let explicit_parameters = callable.generic_parameters().to_vec().into_boxed_slice();
            let callable = self.specialize_method_callable(
                &callable,
                &owner,
                subject,
                &replacements,
                explicit_parameters.iter().cloned(),
            )?;
            candidates.push(MethodCandidate {
                callable,
                explicit_parameters,
                preset_replacements: BTreeMap::new(),
                dispatch: MethodDispatchTemplate::Direct,
            });
        }
        Ok(Outcome::Known(candidates))
    }

    pub(super) fn trait_method_candidates(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        origin: OriginId,
        subject: TypeId,
        name: &NameKey,
    ) -> Result<Outcome<Vec<MethodCandidate>>, BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        let mut candidates = Vec::new();
        for trait_target in self.visible_traits(module)? {
            let trait_parameters = match self.signatures.declaration(&trait_target) {
                FactState::Known(DeclarationSignature::Trait {
                    generic_parameters, ..
                }) => generic_parameters.to_vec(),
                FactState::Poisoned(poison) => {
                    return Ok(Outcome::Poisoned(poison.clone()));
                }
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                FactState::Known(_) | FactState::NotApplicable => continue,
            };
            let methods = self
                .signatures
                .trait_methods(&trait_target)
                .iter()
                .filter(|method| method.name() == name)
                .map(|method| method.declaration().clone())
                .collect::<Vec<_>>();
            if methods.is_empty() {
                continue;
            }
            let receiver_candidates = match self.trait_environment.receiver_candidates_for_trait(
                owner,
                self.signatures,
                subject,
                &trait_target,
            )? {
                Ok(candidates) => candidates,
                Err(failure) => return self.normalization_failure_at(source, origin, failure),
            };
            for method in methods {
                let callable = match self.signatures.declaration(&method) {
                    FactState::Known(DeclarationSignature::Callable(callable)) => self
                        .signatures
                        .callable(*callable)
                        .cloned()
                        .ok_or(BodyTypingError::UnknownCallable(*callable))?,
                    FactState::Poisoned(poison) => {
                        return Ok(Outcome::Poisoned(poison.clone()));
                    }
                    FactState::Unperformed => return Ok(Outcome::Unperformed),
                    FactState::Known(_) | FactState::NotApplicable => continue,
                };
                for receiver_candidate in &receiver_candidates {
                    candidates.push(self.trait_method_candidate(
                        subject,
                        &trait_target,
                        &trait_parameters,
                        &callable,
                        receiver_candidate,
                    )?);
                }
            }
        }
        Ok(Outcome::Known(candidates))
    }

    pub(super) fn trait_method_candidate(
        &mut self,
        subject: TypeId,
        trait_target: &CanonicalTarget,
        trait_parameters: &[GenericParameterId],
        callable: &CallableSignature,
        receiver: &TraitReceiverCandidate,
    ) -> Result<MethodCandidate, BodyTypingError> {
        let trait_arguments = self
            .signatures
            .types()
            .substitution(receiver.trait_ref().substitution())?
            .to_vec();
        if trait_parameters.len() != trait_arguments.len() {
            return Err(BodyTypingError::TraitMethodGenericArityMismatch {
                trait_declaration: trait_target.clone(),
                parameters: trait_parameters.len(),
                arguments: trait_arguments.len(),
            });
        }
        let replacements = trait_parameters
            .iter()
            .cloned()
            .zip(trait_arguments)
            .collect::<BTreeMap<_, _>>();
        let explicit_parameters = callable.generic_parameters().to_vec().into_boxed_slice();
        let mut combined = receiver.generic_parameters().to_vec();
        combined.extend(explicit_parameters.iter().cloned());
        let callable = self.specialize_method_callable(
            callable,
            trait_target,
            subject,
            &replacements,
            combined,
        )?;
        let method = callable.declaration().clone();
        let mut preset_replacements = receiver.replacements().clone();
        preset_replacements.extend(replacements);
        Ok(MethodCandidate {
            callable,
            explicit_parameters,
            preset_replacements,
            dispatch: MethodDispatchTemplate::Trait {
                subject,
                trait_ref: receiver.trait_ref().clone(),
                method,
            },
        })
    }

    pub(super) fn specialize_method_callable(
        &mut self,
        callable: &CallableSignature,
        owner: &CanonicalTarget,
        subject: TypeId,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        generic_parameters: impl IntoIterator<Item = GenericParameterId>,
    ) -> Result<CallableSignature, BodyTypingError> {
        let parameter_types = callable
            .parameters()
            .iter()
            .map(|parameter| {
                self.specialize_member_type(parameter.ty(), replacements, owner, subject)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let result =
            self.specialize_member_type(callable.result(), replacements, owner, subject)?;
        callable
            .with_specialized_contract(generic_parameters, parameter_types, result)
            .ok_or(BodyTypingError::MethodSpecializationArityMismatch(
                callable.id(),
            ))
    }

    pub(super) fn visible_traits(
        &self,
        module: ModuleId,
    ) -> Result<Vec<CanonicalTarget>, BodyTypingError> {
        let module_declarations = self
            .collection
            .module(module)
            .ok_or(BodyTypingError::UnknownModule(module))?;
        let mut traits = module_declarations
            .declarations()
            .filter(|declaration| declaration.kind() == DeclarationKind::Trait)
            .map(|declaration| CanonicalTarget::Source(declaration.id()))
            .collect::<BTreeSet<_>>();
        if module_declarations.declaration_poison().is_none()
            && module_declarations.import_poison().is_none()
        {
            for prelude in self.signatures.prelude_traits() {
                let name = prelude.name();
                if module_declarations
                    .candidates(Namespace::Type, name)
                    .is_empty()
                    && self
                        .imports
                        .lookup(module, Namespace::Type, name)
                        .is_empty()
                {
                    traits.insert(prelude.declaration().clone());
                }
            }
        }
        for import in module_declarations.imports() {
            let FactState::Known(bindings) = self.imports.fact(import.id()) else {
                continue;
            };
            for binding in bindings
                .iter()
                .filter(|binding| binding.namespace() == Namespace::Type)
            {
                for target in binding.targets() {
                    if matches!(
                        self.signatures.declaration(target),
                        FactState::Known(DeclarationSignature::Trait { .. })
                    ) {
                        traits.insert(target.clone());
                    }
                }
            }
        }
        Ok(traits.into_iter().collect())
    }

    pub(super) fn resolve_explicit_method_arguments(
        &mut self,
        source: &SemanticSource<'_>,
        arguments: &[TypeRefId],
    ) -> Result<Outcome<Vec<TypeId>>, BodyTypingError> {
        let mut resolved = Vec::with_capacity(arguments.len());
        let mut poison = None;
        let mut unperformed = false;
        for &argument in arguments {
            match self.type_occurrence(source, argument)? {
                Outcome::Known(argument) => resolved.push(argument),
                Outcome::Poisoned(found) => self.merge_poison(&mut poison, &found)?,
                Outcome::Unperformed | Outcome::NotApplicable => unperformed = true,
                Outcome::Deferred => return Ok(Outcome::Deferred),
            }
        }
        if let Some(poison) = poison {
            Ok(Outcome::Poisoned(poison))
        } else if unperformed {
            Ok(Outcome::Unperformed)
        } else {
            Ok(Outcome::Known(resolved))
        }
    }

    pub(super) fn bind_method_argument_indices(
        callable: &CallableSignature,
        arguments: &[CallArgument],
    ) -> Result<Vec<usize>, CallBindingFailure> {
        let Some(parameters) = callable.parameters().get(1..) else {
            return Err(CallBindingFailure::Missing { slot: 0 });
        };
        let mut bindings = vec![None; parameters.len()];
        let mut positional = 0;
        let mut saw_named = false;
        for (argument_index, argument) in arguments.iter().enumerate() {
            let slot = if let Some(name) = &argument.name {
                saw_named = true;
                let name = name
                    .text()
                    .ok_or(CallBindingFailure::InvalidName { argument_index })?;
                parameters
                    .iter()
                    .position(|parameter| parameter.name() == Some(name))
                    .ok_or(CallBindingFailure::UnknownNamed { argument_index })?
            } else {
                if saw_named {
                    return Err(CallBindingFailure::PositionalAfterNamed { argument_index });
                }
                if positional >= parameters.len() {
                    return Err(CallBindingFailure::TooMany { argument_index });
                }
                let slot = positional;
                positional += 1;
                slot
            };
            if bindings[slot].replace(argument_index).is_some() {
                return Err(CallBindingFailure::Duplicate {
                    argument_index,
                    slot,
                });
            }
        }
        bindings
            .into_iter()
            .enumerate()
            .map(|(slot, binding)| binding.ok_or(CallBindingFailure::Missing { slot }))
            .collect()
    }

    pub(super) fn adjusted_method_receiver(
        &mut self,
        callable: &CallableSignature,
        receiver: &TypedExpression,
    ) -> Result<Option<(TypeId, ArgumentPassing)>, BodyTypingError> {
        let Some(parameter) = callable.parameters().first() else {
            return Ok(None);
        };
        let TypeKind::Reference {
            mutability: expected,
            ..
        } = self.signatures.types().kind(parameter.ty())?
        else {
            return Ok(Some((receiver.ty, ArgumentPassing::Value)));
        };
        let expected = *expected;
        if let TypeKind::Reference {
            mutability: actual,
            referent,
        } = self.signatures.types().kind(receiver.ty)?
        {
            let actual = *actual;
            let referent = *referent;
            if actual == expected {
                return Ok(Some((receiver.ty, ArgumentPassing::Value)));
            }
            if expected == ReferenceMutability::Shared && actual == ReferenceMutability::Mutable {
                return Ok(Some((
                    self.signatures
                        .types_mut()
                        .reference(ReferenceMutability::Shared, referent)?,
                    ArgumentPassing::Value,
                )));
            }
            return Ok(None);
        }
        if expected == ReferenceMutability::Mutable
            && !receiver.place.as_ref().is_some_and(TypedPlace::is_writable)
        {
            return Ok(None);
        }
        let passing = if receiver.place.is_some() {
            ArgumentPassing::BorrowPlace(expected)
        } else {
            ArgumentPassing::BorrowTemporary(expected)
        };
        Ok(Some((
            self.signatures
                .types_mut()
                .reference(expected, receiver.ty)?,
            passing,
        )))
    }

    pub(super) fn specialize_trait_ref(
        &mut self,
        trait_ref: &TraitRef,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<TraitRef, BodyTypingError> {
        let arguments = self
            .signatures
            .types()
            .substitution(trait_ref.substitution())?
            .to_vec()
            .into_iter()
            .map(|argument| {
                self.signatures
                    .types_mut()
                    .substitute(argument, replacements)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let substitution = self.signatures.types_mut().intern_substitution(arguments)?;
        Ok(TraitRef::new(trait_ref.declaration().clone(), substitution))
    }

    pub(super) fn method_not_found<T>(
        &mut self,
        source: &SemanticSource<'_>,
        member: &Name,
    ) -> Result<Outcome<T>, BodyTypingError> {
        let name = member.text().unwrap_or("<invalid>");
        self.type_error(
            source,
            member.origin(),
            "METHOD_NOT_FOUND",
            format!("method `{name}` not found for receiver type"),
            BTreeMap::new(),
        )
        .map(Outcome::Poisoned)
    }

    pub(super) fn method_ambiguous(
        &mut self,
        source: &SemanticSource<'_>,
        member: &Name,
        candidates: &[MethodCandidate],
        trait_tier: bool,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let rendered = candidates
            .iter()
            .map(|candidate| self.qualified_method_label(candidate.callable.declaration()))
            .collect::<Result<Vec<_>, _>>()?;
        let related = candidates
            .iter()
            .zip(&rendered)
            .filter_map(|(candidate, rendered)| {
                self.target_span(candidate.callable.declaration())
                    .map(|span| {
                        RelatedSpan::new(span, format!("candidate `{rendered}` declared here"))
                            .expect("rendered candidate label is non-empty")
                    })
            })
            .collect::<Vec<_>>();
        let code = if trait_tier {
            "TRAIT_METHOD_AMBIGUOUS"
        } else {
            "OVERLOAD_AMBIGUOUS"
        };
        let name = member.text().unwrap_or("<invalid>");
        let message = if trait_tier {
            format!(
                "trait method `{name}` is ambiguous among {}; use a fully qualified call",
                rendered.join(", ")
            )
        } else {
            format!("overload method `{name}` is ambiguous")
        };
        self.type_error_with_related(
            source,
            member.origin(),
            code,
            message,
            related,
            BTreeMap::from([
                (Box::<str>::from("name"), DiagnosticDataValue::from(name)),
                (
                    Box::<str>::from("candidates"),
                    DiagnosticDataValue::from(rendered.join("; ")),
                ),
            ]),
        )
        .map(Outcome::Poisoned)
    }

    pub(super) fn qualified_method_label(
        &self,
        target: &CanonicalTarget,
    ) -> Result<String, BodyTypingError> {
        let CanonicalTarget::Source(method) = target else {
            return Ok(self.target_label(target));
        };
        let method_declaration = self
            .collection
            .declaration(*method)
            .ok_or(BodyTypingError::UnknownDeclaration(*method))?;
        let owner = method_declaration
            .parent()
            .ok_or(BodyTypingError::MissingDeclarationParent(*method))?;
        let owner = self
            .collection
            .declaration(owner)
            .ok_or(BodyTypingError::UnknownDeclaration(owner))?;
        Ok(format!(
            "{}.{}",
            owner
                .name()
                .map(NameKey::spelling)
                .unwrap_or_else(|| "<owner>".to_owned()),
            method_declaration
                .name()
                .map(NameKey::spelling)
                .unwrap_or_else(|| "<method>".to_owned())
        ))
    }

    pub(super) fn bind_call_argument_indices(
        callable: &CallableSignature,
        arguments: &[CallArgument],
    ) -> Result<Vec<usize>, CallBindingFailure> {
        let parameters = callable.parameters();
        let mut bindings = vec![None; parameters.len()];
        let mut positional = 0;
        let mut saw_named = false;
        for (argument_index, argument) in arguments.iter().enumerate() {
            let slot = if let Some(name) = &argument.name {
                saw_named = true;
                let name = name
                    .text()
                    .ok_or(CallBindingFailure::InvalidName { argument_index })?;
                parameters
                    .iter()
                    .position(|parameter| parameter.name() == Some(name))
                    .ok_or(CallBindingFailure::UnknownNamed { argument_index })?
            } else {
                if saw_named {
                    return Err(CallBindingFailure::PositionalAfterNamed { argument_index });
                }
                if positional >= parameters.len() {
                    return Err(CallBindingFailure::TooMany { argument_index });
                }
                let slot = positional;
                positional += 1;
                slot
            };
            if bindings[slot].replace(argument_index).is_some() {
                return Err(CallBindingFailure::Duplicate {
                    argument_index,
                    slot,
                });
            }
        }
        bindings
            .into_iter()
            .enumerate()
            .map(|(slot, binding)| binding.ok_or(CallBindingFailure::Missing { slot }))
            .collect()
    }

    pub(super) fn overload_no_match(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        call: &CallExpr,
        candidates: &[CallableSignature],
        typed_arguments: &BTreeMap<ExprId, TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let actual = call
            .arguments
            .iter()
            .map(|argument| typed_arguments[&argument.value].ty)
            .collect::<Vec<_>>();
        let actual = self.signatures.types_mut().tuple(actual)?;
        let rendered = self.format_callable_candidates(candidates)?;
        let related = self.callable_candidate_spans(candidates, &rendered)?;
        let poison = self.type_error_with_related(
            source,
            origin,
            "CALL_ARG_TYPE_MISMATCH",
            "call argument type mismatch because no overload candidate matches",
            related,
            BTreeMap::from([
                (
                    Box::<str>::from("reason"),
                    DiagnosticDataValue::from("overload_no_match"),
                ),
                (
                    Box::<str>::from("actual_type"),
                    DiagnosticDataValue::from(self.format_type(actual)?),
                ),
                (
                    Box::<str>::from("candidates"),
                    DiagnosticDataValue::from(rendered.join("; ")),
                ),
            ]),
        )?;
        Ok(Outcome::Poisoned(poison))
    }

    pub(super) fn overload_ambiguous(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        candidates: &[CallableSignature],
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let rendered = self.format_callable_candidates(candidates)?;
        let related = self.callable_candidate_spans(candidates, &rendered)?;
        let name = candidates
            .first()
            .map(|candidate| self.target_label(candidate.declaration()))
            .unwrap_or_else(|| "<overload>".to_owned());
        let poison = self.type_error_with_related(
            source,
            origin,
            "OVERLOAD_AMBIGUOUS",
            format!("overload `{name}` is ambiguous"),
            related,
            BTreeMap::from([
                (Box::<str>::from("name"), DiagnosticDataValue::from(name)),
                (
                    Box::<str>::from("candidates"),
                    DiagnosticDataValue::from(rendered.join("; ")),
                ),
            ]),
        )?;
        Ok(Outcome::Poisoned(poison))
    }

    pub(super) fn format_callable_candidates(
        &self,
        candidates: &[CallableSignature],
    ) -> Result<Vec<String>, BodyTypingError> {
        candidates
            .iter()
            .map(|candidate| self.format_callable_signature(candidate))
            .collect()
    }

    pub(super) fn format_callable_signature(
        &self,
        callable: &CallableSignature,
    ) -> Result<String, BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        Ok(DiagnosticTypeRenderer::new(
            self.collection,
            self.signatures.diagnostic_type_names(),
            &*self.signatures,
            owner,
        )
        .render_callable_signature(callable)?)
    }

    pub(super) fn callable_candidate_spans(
        &self,
        candidates: &[CallableSignature],
        rendered: &[String],
    ) -> Result<Vec<RelatedSpan>, BodyTypingError> {
        candidates
            .iter()
            .zip(rendered)
            .filter_map(|(candidate, rendered)| {
                let CanonicalTarget::Source(declaration) = candidate.declaration() else {
                    return None;
                };
                Some(self.source_item(*declaration).map(|(_, source, _, item)| {
                    RelatedSpan::new(
                        source
                            .origin_span(item.origin())
                            .cloned()
                            .expect("semantic source covers every HIR origin"),
                        format!("candidate `{rendered}` declared here"),
                    )
                    .expect("rendered candidate label is non-empty")
                }))
            })
            .collect()
    }

    pub(super) fn type_intrinsic_unit_constructor(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        target: &CanonicalTarget,
        constructor: &crate::ConstructorSignature,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext { source, body, .. } = context;
        let qualified = body
            .expression(expression)
            .and_then(|value| match value.kind() {
                ExprKind::Name(path) => Some(path.is_qualified()),
                _ => None,
            })
            .unwrap_or(false);
        let ApplicationPlan::Complete(outcome) = self.prepare_application(
            source,
            ApplicationRequest {
                origin,
                generic_parameters: constructor.generic_parameters(),
                slots: &[],
                result_template: constructor.result(),
                explicit_arguments: &[],
                explicit_parameters: None,
                preset_replacements: None,
                allowed_ownership_parameter: self
                    .conditional_ownership_parameter(target, constructor.generic_parameters()),
                ty: request,
                defer_underconstrained: !qualified && self.is_reserved_constructor(target),
            },
        )?
        else {
            unreachable!("unit constructors have no child expressions");
        };
        self.finish_invocation(
            context,
            InvocationFrame::construction(
                expression,
                origin,
                Vec::new(),
                target.clone(),
                constructor.generic_parameters().into(),
            ),
            outcome,
        )
    }

    pub(super) fn bind_call_slots(
        &mut self,
        source: &SemanticSource<'_>,
        call_origin: OriginId,
        callable: &CallableSignature,
        arguments: &[CallArgument],
    ) -> Result<Outcome<Vec<ApplicationSlot>>, BodyTypingError> {
        let parameters = callable.parameters();
        let bindings = match Self::bind_call_argument_indices(callable, arguments) {
            Ok(bindings) => bindings,
            Err(CallBindingFailure::InvalidName { argument_index }) => {
                let name = arguments[argument_index]
                    .name
                    .as_ref()
                    .ok_or(BodyTypingError::InconsistentOverloadBinding)?;
                let poison = source.invalid_poison(name.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: name.origin(),
                    },
                )?;
                return Ok(Outcome::Poisoned(poison));
            }
            Err(CallBindingFailure::UnknownNamed { argument_index }) => {
                let argument = &arguments[argument_index];
                let name = argument
                    .name
                    .as_ref()
                    .and_then(|name| name.text())
                    .ok_or(BodyTypingError::InconsistentOverloadBinding)?;
                let data =
                    BTreeMap::from([(Box::<str>::from("name"), DiagnosticDataValue::from(name))]);
                return self
                    .type_error(
                        source,
                        argument.origin,
                        "CALL_UNKNOWN_NAMED_ARG",
                        format!("call has unknown named argument `{name}`"),
                        data,
                    )
                    .map(Outcome::Poisoned);
            }
            Err(CallBindingFailure::PositionalAfterNamed { argument_index }) => {
                return self
                    .type_error(
                        source,
                        arguments[argument_index].origin,
                        "CALL_POSITIONAL_AFTER_NAMED",
                        "call positional argument violates named-argument order",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            Err(CallBindingFailure::TooMany { argument_index }) => {
                return self
                    .type_error(
                        source,
                        arguments[argument_index].origin,
                        "CALL_TOO_MANY_ARGS",
                        "call has too many arguments for the selected callable",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            Err(CallBindingFailure::Duplicate {
                argument_index,
                slot,
            }) => {
                let name = parameters[slot]
                    .name()
                    .map_or_else(|| format!("#{slot}"), str::to_owned);
                let data = BTreeMap::from([(
                    Box::<str>::from("name"),
                    DiagnosticDataValue::from(name.as_str()),
                )]);
                return self
                    .type_error(
                        source,
                        arguments[argument_index].origin,
                        "CALL_DUPLICATE_NAMED_ARG",
                        format!("call has duplicate named argument `{name}`"),
                        data,
                    )
                    .map(Outcome::Poisoned);
            }
            Err(CallBindingFailure::Missing { slot }) => {
                let parameter = parameters[slot]
                    .name()
                    .map_or_else(|| format!("#{slot}"), str::to_owned);
                let data = BTreeMap::from([(
                    Box::<str>::from("parameter"),
                    DiagnosticDataValue::from(parameter.as_str()),
                )]);
                return self
                    .type_error(
                        source,
                        call_origin,
                        "CALL_MISSING_ARG",
                        format!("call argument is missing required parameter `{parameter}`"),
                        data,
                    )
                    .map(Outcome::Poisoned);
            }
        };

        let mut slots = Vec::with_capacity(parameters.len());
        for (slot, parameter) in parameters.iter().enumerate() {
            let argument_index = bindings[slot];
            let argument = &arguments[argument_index];
            slots.push(ApplicationSlot {
                name: parameter
                    .name()
                    .map_or_else(|| format!("#{slot}").into_boxed_str(), Box::<str>::from),
                template: parameter.ty(),
                value: argument.value,
                origin: argument.origin,
                related: self.parameter_span(callable, parameter)?,
                evaluation_order: argument_index,
                passing: ArgumentPassing::Value,
            });
        }
        Ok(Outcome::Known(slots))
    }

    pub(super) fn parameter_span(
        &self,
        callable: &CallableSignature,
        parameter: &CallableParameter,
    ) -> Result<Option<SourceSpan>, BodyTypingError> {
        let (CanonicalTarget::Source(declaration), Some(pattern)) =
            (callable.declaration(), parameter.source_pattern())
        else {
            return Ok(None);
        };
        let (_, source, hir, _) = self.source_item(*declaration)?;
        let Some(body) = callable.body() else {
            return Ok(None);
        };
        let pattern = hir
            .body(body)
            .pattern(pattern)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern))?;
        Ok(source.origin_span(pattern.origin()).cloned())
    }

    pub(super) fn adjust_application_value(
        &mut self,
        slot: &ApplicationSlot,
        mut value: TypedExpression,
    ) -> Result<TypedExpression, BodyTypingError> {
        if matches!(
            self.signatures.types().kind(value.ty)?,
            TypeKind::Reference { .. }
        ) {
            return Ok(value);
        }
        let mutability = match slot.passing {
            ArgumentPassing::Value => return Ok(value),
            ArgumentPassing::BorrowPlace(mutability)
            | ArgumentPassing::BorrowTemporary(mutability) => mutability,
        };
        value.ty = self
            .signatures
            .types_mut()
            .reference(mutability, value.ty)?;
        value.place = None;
        value.integer_constant = None;
        Ok(value)
    }

    pub(super) fn discard_unreachable_enclosing_transfers(
        &mut self,
        candidates: &[ControlTransfer],
        enclosing_regions: &[ControlRegionKey],
    ) {
        let unreachable = candidates
            .iter()
            .filter(|transfer| enclosing_regions.contains(transfer.target()))
            .collect::<BTreeSet<_>>();
        self.transfers
            .retain(|transfer| !unreachable.contains(transfer));
    }

    pub(super) fn type_argument_inference_failure<T>(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<Outcome<T>, BodyTypingError> {
        self.type_error(
            source,
            origin,
            "TYPE_ARG_INFERENCE_FAILED",
            "type argument inference failed to produce one complete substitution",
            BTreeMap::new(),
        )
        .map(Outcome::Poisoned)
    }

    pub(super) fn specialize_if_resolved(
        &mut self,
        template: TypeId,
        parameters: &BTreeSet<GenericParameterId>,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<Option<TypeId>, BodyTypingError> {
        if self.type_mentions_unresolved(template, parameters, replacements)? {
            return Ok(None);
        }
        Ok(Some(
            self.signatures
                .types_mut()
                .substitute(template, replacements)?,
        ))
    }

    pub(super) fn type_mentions_unresolved(
        &self,
        root: TypeId,
        parameters: &BTreeSet<GenericParameterId>,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<bool, BodyTypingError> {
        let mut pending = vec![root];
        let mut seen = BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            match self.signatures.types().kind(current)? {
                TypeKind::GenericParameter(parameter)
                    if parameters.contains(parameter) && !replacements.contains_key(parameter) =>
                {
                    return Ok(true);
                }
                TypeKind::Nominal { substitution, .. } => pending.extend(
                    self.signatures
                        .types()
                        .substitution(*substitution)?
                        .iter()
                        .copied(),
                ),
                TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                    pending.push(*referent)
                }
                TypeKind::Tuple(elements) | TypeKind::ErrorUnion(elements) => {
                    pending.extend(elements.iter().copied());
                }
                TypeKind::Callable { parameters, result } => {
                    pending.extend(parameters.iter().copied());
                    pending.push(*result);
                }
                TypeKind::AssociatedProjection(projection) => {
                    pending.push(projection.base());
                    pending.extend(
                        self.signatures
                            .types()
                            .substitution(projection.trait_ref().substitution())?
                            .iter()
                            .copied(),
                    );
                }
                TypeKind::Never
                | TypeKind::Primitive(_)
                | TypeKind::GenericParameter(_)
                | TypeKind::SelfType(_) => {}
            }
        }
        Ok(false)
    }

    pub(super) fn infer_constraints(
        &self,
        template: TypeId,
        actual: TypeId,
        parameters: &BTreeSet<GenericParameterId>,
        replacements: &mut BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<ConstraintOutcome, BodyTypingError> {
        let template_kind = self.signatures.types().kind(template)?.clone();
        if let TypeKind::GenericParameter(parameter) = template_kind {
            if !parameters.contains(&parameter) {
                return Ok(ConstraintOutcome::NoEvidence);
            }
            if let Some(&existing) = replacements.get(&parameter) {
                if actual == self.signatures.types().never() || existing == actual {
                    return Ok(ConstraintOutcome::Evidence);
                }
                return Ok(ConstraintOutcome::Conflict);
            }
            replacements.insert(parameter, actual);
            return Ok(ConstraintOutcome::Evidence);
        }
        let actual_kind = self.signatures.types().kind(actual)?.clone();
        let pairs = match (template_kind, actual_kind) {
            (
                TypeKind::Nominal {
                    head: template_head,
                    substitution: template_substitution,
                },
                TypeKind::Nominal {
                    head: actual_head,
                    substitution: actual_substitution,
                },
            ) if template_head == actual_head => self
                .signatures
                .types()
                .substitution(template_substitution)?
                .iter()
                .copied()
                .zip(
                    self.signatures
                        .types()
                        .substitution(actual_substitution)?
                        .iter()
                        .copied(),
                )
                .collect::<Vec<_>>(),
            (TypeKind::Tuple(template), TypeKind::Tuple(actual))
                if template.len() == actual.len() =>
            {
                template
                    .iter()
                    .copied()
                    .zip(actual.iter().copied())
                    .collect()
            }
            (
                TypeKind::Callable {
                    parameters: template_parameters,
                    result: template_result,
                },
                TypeKind::Callable {
                    parameters: actual_parameters,
                    result: actual_result,
                },
            ) if template_parameters.len() == actual_parameters.len() => template_parameters
                .iter()
                .copied()
                .zip(actual_parameters.iter().copied())
                .chain([(template_result, actual_result)])
                .collect(),
            (
                TypeKind::Reference {
                    mutability: template_mutability,
                    referent: template_referent,
                },
                TypeKind::Reference {
                    mutability: actual_mutability,
                    referent: actual_referent,
                },
            ) if template_mutability == actual_mutability
                || (template_mutability == ReferenceMutability::Shared
                    && actual_mutability == ReferenceMutability::Mutable) =>
            {
                vec![(template_referent, actual_referent)]
            }
            (
                TypeKind::RawPointer {
                    mutability: template_mutability,
                    referent: template_referent,
                },
                TypeKind::RawPointer {
                    mutability: actual_mutability,
                    referent: actual_referent,
                },
            ) if template_mutability == actual_mutability
                || (template_mutability == RawPointerMutability::Const
                    && actual_mutability == RawPointerMutability::Mutable) =>
            {
                vec![(template_referent, actual_referent)]
            }
            _ => return Ok(ConstraintOutcome::NoEvidence),
        };
        let mut outcome = ConstraintOutcome::NoEvidence;
        for (template, actual) in pairs {
            match self.infer_constraints(template, actual, parameters, replacements)? {
                ConstraintOutcome::Conflict => return Ok(ConstraintOutcome::Conflict),
                ConstraintOutcome::Evidence => outcome = ConstraintOutcome::Evidence,
                ConstraintOutcome::NoEvidence => {}
            }
        }
        Ok(outcome)
    }

    pub(super) fn validate_type_arguments(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        parameters: &[GenericParameterId],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        allowed_ownership_parameter: Option<&GenericParameterId>,
    ) -> Result<Option<Poison>, BodyTypingError> {
        for parameter in parameters {
            let replacement = replacements[parameter];
            let admitted_conditional_payload = if allowed_ownership_parameter == Some(parameter) {
                match self.signatures.types().ownership_root(replacement)? {
                    Some(crate::OwnershipRoot::Direct { payload }) => {
                        !self.signatures.types().contains_ownership(payload)?
                    }
                    Some(crate::OwnershipRoot::Optional { .. })
                    | Some(crate::OwnershipRoot::Result { .. })
                    | None => false,
                }
            } else {
                false
            };
            if self.signatures.types().contains_ownership(replacement)?
                && !admitted_conditional_payload
            {
                return self
                    .type_error(
                        source,
                        origin,
                        "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                        "Own type position is forbidden for a generic argument",
                        BTreeMap::new(),
                    )
                    .map(Some);
            }
        }
        for parameter in parameters {
            if self
                .signatures
                .types()
                .contains_reference(replacements[parameter])?
            {
                return self
                    .type_error(
                        source,
                        origin,
                        "REFERENCE_TYPE_POSITION_FORBIDDEN",
                        "reference type position is forbidden for a generic argument",
                        BTreeMap::new(),
                    )
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub(super) fn conditional_ownership_parameter<'a>(
        &self,
        target: &CanonicalTarget,
        parameters: &'a [GenericParameterId],
    ) -> Option<&'a GenericParameterId> {
        self.is_reserved_constructor(target)
            .then(|| parameters.first())
            .flatten()
    }

    pub(super) fn call_argument_mismatch(
        &mut self,
        source: &SemanticSource<'_>,
        slot: &ApplicationSlot,
        expected: TypeId,
        actual: TypeId,
    ) -> Result<Poison, BodyTypingError> {
        let data = BTreeMap::from([
            (
                Box::<str>::from("reason"),
                DiagnosticDataValue::from("parameter_type"),
            ),
            (
                Box::<str>::from("parameter"),
                DiagnosticDataValue::from(slot.name.as_ref()),
            ),
            (
                Box::<str>::from("expected_type"),
                DiagnosticDataValue::from(self.format_type(expected)?),
            ),
            (
                Box::<str>::from("actual_type"),
                DiagnosticDataValue::from(self.format_type(actual)?),
            ),
        ]);
        let related = slot.related.clone().map(|span| {
            RelatedSpan::new(span, "parameter declared here")
                .expect("static related-span label is non-empty")
        });
        self.type_error_with_related(
            source,
            slot.origin,
            "CALL_ARG_TYPE_MISMATCH",
            format!("call argument type mismatch for parameter `{}`", slot.name),
            related,
            data,
        )
    }

    pub(super) fn check_callable_requirements(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        requirements: &[CallableRequirement],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<Option<Poison>, BodyTypingError> {
        for requirement in requirements {
            match requirement {
                CallableRequirement::FixedWidthInteger(template) => {
                    let actual = self
                        .signatures
                        .types_mut()
                        .substitute(*template, replacements)?;
                    if !self.is_fixed_width_integer(actual)? {
                        let data = BTreeMap::from([
                            (
                                Box::<str>::from("reason"),
                                DiagnosticDataValue::from("type_family"),
                            ),
                            (
                                Box::<str>::from("expected_family"),
                                DiagnosticDataValue::from("FixedWidthInteger"),
                            ),
                            (
                                Box::<str>::from("actual_type"),
                                DiagnosticDataValue::from(self.format_type(actual)?),
                            ),
                        ]);
                        return self
                            .type_error(
                                source,
                                origin,
                                "CALL_ARG_TYPE_MISMATCH",
                                "call argument type mismatch with the required type family",
                                data,
                            )
                            .map(Some);
                    }
                }
            }
        }
        Ok(None)
    }

    pub(super) fn record_trait_obligations(
        &mut self,
        invocation: ExpressionKey,
        source: &SemanticSource<'_>,
        origin: OriginId,
        parameters: &[GenericParameterId],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<(), BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        for parameter in parameters {
            let signature = match self.signatures.generic_parameter(parameter) {
                FactState::Known(signature) => signature.clone(),
                FactState::Unperformed | FactState::Poisoned(_) | FactState::NotApplicable => {
                    return Err(BodyTypingError::UnavailableGenericParameter(
                        parameter.clone(),
                    ));
                }
            };
            for (bound_index, bound) in signature
                .instantiate_bounds(self.signatures.types_mut(), replacements)?
                .into_iter()
                .enumerate()
            {
                let destination = match parameter {
                    GenericParameterId::Source(declaration) => {
                        let owner = self
                            .collection
                            .declaration(*declaration)
                            .and_then(|declaration| declaration.parent())
                            .ok_or(BodyTypingError::MissingDeclarationParent(*declaration))?;
                        let subject = self
                            .signatures
                            .types_mut()
                            .generic_parameter(parameter.clone())?;
                        Some(
                            self.trait_environment
                                .declared_witness_for_obligation(
                                    owner,
                                    &TraitObligation::new(subject, bound.declared().clone()),
                                )
                                .ok_or_else(|| {
                                    BodyTypingError::MissingBoundWitness(parameter.clone())
                                })?,
                        )
                    }
                    GenericParameterId::Trusted { .. } => None,
                };
                let obligation = bound.into_obligation();
                self.obligations.push(PendingTraitObligation {
                    invocation,
                    owner,
                    location: SemanticLocation::origin(source.file(), origin),
                    parameter: parameter.clone(),
                    bound_index,
                    subject: obligation.subject(),
                    trait_ref: obligation.trait_ref().clone(),
                    destination,
                });
            }
        }
        Ok(())
    }

    pub(super) fn intern_callable_instance(
        &mut self,
        signature: CallableSignatureId,
        substitution: SubstId,
    ) -> Result<CallableInstanceId, BodyTypingError> {
        if let Some(&id) = self.callable_instance_ids.get(&(signature, substitution)) {
            return Ok(id);
        }
        let id = CallableInstanceId::from_index(self.callable_instances.len())?;
        self.callable_instances.push(CallableInstance {
            id,
            signature,
            substitution,
        });
        self.callable_instance_ids
            .insert((signature, substitution), id);
        Ok(id)
    }
}
