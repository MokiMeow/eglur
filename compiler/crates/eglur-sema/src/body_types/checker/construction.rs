use super::invocation::{InvocationFrame, InvocationPlan};
use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_struct_literal(
        &mut self,
        context: BodyContext<'_, '_>,
        request: StructLiteralRequest<'_>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let expression = request.expression;
        let origin = request.origin;
        let target = match self.constructor_target(module, source, expression, origin)? {
            Outcome::Known(target) => target,
            Outcome::Poisoned(poison) => {
                return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
            }
            Outcome::Unperformed => return Ok(InvocationPlan::Complete(Outcome::Unperformed)),
            Outcome::Deferred => return Ok(InvocationPlan::Complete(Outcome::Deferred)),
            Outcome::NotApplicable => return Ok(InvocationPlan::Complete(Outcome::NotApplicable)),
        };
        self.prepare_resolved_struct_literal(context, request, target)
    }

    pub(super) fn prepare_resolved_struct_literal(
        &mut self,
        context: BodyContext<'_, '_>,
        request: StructLiteralRequest<'_>,
        target: ConstructorTarget,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let source = context.source;
        let StructLiteralRequest {
            expression,
            origin,
            head_origin,
            fields,
            ty: request,
        } = request;
        if let Some(poison) = self.constructor_target_kind_error(
            source,
            head_origin,
            &target,
            ResolvedTargetKind::Struct,
        )? {
            return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
        }
        let ConstructorTarget {
            declaration: target,
            signature: target_signature,
            ..
        } = target;
        let DeclarationSignature::Struct {
            generic_parameters,
            storage_fields,
            properties,
        } = target_signature
        else {
            return Err(BodyTypingError::UnexpectedSignatureKind {
                target,
                expected: "struct after target-kind validation",
            });
        };
        let generic_parameters = generic_parameters.to_vec();
        let storage_fields = storage_fields.to_vec();
        let properties = properties.to_vec();
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }
        let owner_name = self.target_label(&target);
        let mut initializers = BTreeMap::<CanonicalTarget, usize>::new();
        for (index, field) in fields.iter().enumerate() {
            let Some(name) = field.name.text() else {
                return Ok(InvocationPlan::Complete(Outcome::Poisoned(
                    source.invalid_poison(field.name.origin()).cloned().ok_or(
                        BodyTypingError::MissingOriginSpan {
                            file: source.file(),
                            origin: field.name.origin(),
                        },
                    )?,
                )));
            };
            if self.find_named_target(&properties, name).is_some() {
                return self
                    .struct_initializer_error(
                        source,
                        field.origin,
                        "STRUCT_INIT_PROPERTY_FORBIDDEN",
                        "struct property initializer is forbidden",
                        &owner_name,
                        ("property", name),
                    )
                    .map(Outcome::Poisoned)
                    .map(InvocationPlan::Complete);
            }
            let Some(declaration) = self.find_named_target(&storage_fields, name) else {
                return self
                    .struct_initializer_error(
                        source,
                        field.origin,
                        "STRUCT_INIT_UNKNOWN_FIELD",
                        "struct initializer names an unknown field",
                        &owner_name,
                        ("field", name),
                    )
                    .map(Outcome::Poisoned)
                    .map(InvocationPlan::Complete);
            };
            if initializers.insert(declaration, index).is_some() {
                return self
                    .struct_initializer_error(
                        source,
                        field.origin,
                        "STRUCT_INIT_DUPLICATE_FIELD",
                        "struct initializer has a duplicate field",
                        &owner_name,
                        ("field", name),
                    )
                    .map(Outcome::Poisoned)
                    .map(InvocationPlan::Complete);
            }
        }
        if let Some(missing) = storage_fields
            .iter()
            .find(|field| !initializers.contains_key(*field))
        {
            let name = self.target_label(missing);
            return self
                .struct_initializer_error(
                    source,
                    origin,
                    "STRUCT_INIT_MISSING_FIELD",
                    "struct initializer has a missing field",
                    &owner_name,
                    ("field", &name),
                )
                .map(Outcome::Poisoned)
                .map(InvocationPlan::Complete);
        }

        let mut slots = Vec::with_capacity(storage_fields.len());
        for field_target in &storage_fields {
            let template = match self.signatures.declaration(field_target) {
                FactState::Known(DeclarationSignature::Field { ty }) => *ty,
                FactState::Poisoned(poison) => {
                    return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison.clone())));
                }
                FactState::Unperformed => {
                    return Ok(InvocationPlan::Complete(Outcome::Unperformed));
                }
                FactState::Known(_) | FactState::NotApplicable => {
                    return Err(BodyTypingError::UnexpectedSignatureKind {
                        target: field_target.clone(),
                        expected: "stored field",
                    });
                }
            };
            let initializer = &fields[initializers[field_target]];
            slots.push(ApplicationSlot {
                name: self.target_label(field_target).into_boxed_str(),
                template,
                value: initializer.value,
                origin: initializer.origin,
                related: self.target_span(field_target),
                evaluation_order: initializers[field_target],
                passing: ArgumentPassing::Value,
            });
        }
        let result_template = self.nominal_template(target.clone(), &generic_parameters)?;
        let plan = self.prepare_application(
            source,
            ApplicationRequest {
                origin,
                generic_parameters: &generic_parameters,
                slots: &slots,
                result_template,
                explicit_arguments: &[],
                explicit_parameters: None,
                preset_replacements: None,
                allowed_ownership_parameter: None,
                ty: request,
                defer_underconstrained: false,
            },
        )?;
        Ok(InvocationPlan::Application {
            plan,
            continuation: Box::new(InvocationFrame::construction(
                expression,
                origin,
                slots,
                target,
                generic_parameters.into_boxed_slice(),
            )),
        })
    }

    pub(super) fn prepare_variant_constructor(
        &mut self,
        context: BodyContext<'_, '_>,
        request: VariantConstructorRequest<'_>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let VariantConstructorRequest {
            expression,
            origin,
            head_origin,
            qualified,
            payload,
            ty,
        } = request;
        let target = match self.constructor_target(module, source, expression, origin)? {
            Outcome::Known(target) => target,
            Outcome::Poisoned(poison) => {
                return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
            }
            Outcome::Unperformed => return Ok(InvocationPlan::Complete(Outcome::Unperformed)),
            Outcome::Deferred => return Ok(InvocationPlan::Complete(Outcome::Deferred)),
            Outcome::NotApplicable => return Ok(InvocationPlan::Complete(Outcome::NotApplicable)),
        };
        if let ConstructorPayload::Named(fields) = payload
            && target.kind.is_type_namespace_target()
        {
            return self.prepare_resolved_struct_literal(
                context,
                StructLiteralRequest {
                    expression,
                    origin,
                    head_origin,
                    fields,
                    ty,
                },
                target,
            );
        }
        if let Some(poison) = self.constructor_target_kind_error(
            source,
            head_origin,
            &target,
            ResolvedTargetKind::VariantConstructor,
        )? {
            return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
        };
        let ConstructorTarget {
            declaration: target,
            signature: target_signature,
            ..
        } = target;
        let (generic_parameters, result_template, shape) = match target_signature {
            DeclarationSignature::Constructor(constructor) => (
                constructor.generic_parameters().to_vec(),
                constructor.result(),
                match constructor.payload() {
                    ConstructorPayloadSignature::Unit => ConstructorShape::Absent,
                    ConstructorPayloadSignature::Positional(types) => {
                        ConstructorShape::Tuple(types.to_vec())
                    }
                },
            ),
            DeclarationSignature::EnumVariant { payload } => {
                let CanonicalTarget::Source(variant) = &target else {
                    return Err(BodyTypingError::UnexpectedSignatureKind {
                        target,
                        expected: "source enum variant",
                    });
                };
                let owner = self
                    .collection
                    .declaration(*variant)
                    .and_then(|declaration| declaration.parent())
                    .ok_or(BodyTypingError::MissingDeclarationParent(*variant))?;
                let generic_parameters = match self.signatures.source_declaration(owner) {
                    FactState::Known(DeclarationSignature::Enum {
                        generic_parameters, ..
                    }) => generic_parameters.to_vec(),
                    FactState::Poisoned(poison) => {
                        return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison.clone())));
                    }
                    FactState::Unperformed => {
                        return Ok(InvocationPlan::Complete(Outcome::Unperformed));
                    }
                    FactState::Known(_) | FactState::NotApplicable => {
                        return Err(BodyTypingError::UnexpectedSignatureKind {
                            target: CanonicalTarget::Source(owner),
                            expected: "enum",
                        });
                    }
                };
                let result =
                    self.nominal_template(CanonicalTarget::Source(owner), &generic_parameters)?;
                let shape = match payload {
                    EnumVariantPayloadSignature::Absent => ConstructorShape::Absent,
                    EnumVariantPayloadSignature::Tuple(types) => {
                        ConstructorShape::Tuple(types.to_vec())
                    }
                    EnumVariantPayloadSignature::Named(fields) => {
                        ConstructorShape::Named(fields.to_vec())
                    }
                };
                (generic_parameters, result, shape)
            }
            _ => {
                return Err(BodyTypingError::UnexpectedSignatureKind {
                    target,
                    expected: "variant constructor after target-kind validation",
                });
            }
        };
        let slots = match self.bind_constructor_slots(source, origin, &target, &shape, payload)? {
            Outcome::Known(slots) => slots,
            Outcome::Poisoned(poison) => {
                return Ok(InvocationPlan::Complete(Outcome::Poisoned(poison)));
            }
            Outcome::Unperformed => return Ok(InvocationPlan::Complete(Outcome::Unperformed)),
            Outcome::Deferred => return Ok(InvocationPlan::Complete(Outcome::Deferred)),
            Outcome::NotApplicable => return Ok(InvocationPlan::Complete(Outcome::NotApplicable)),
        };
        let deferred_reserved = !qualified && self.is_reserved_constructor(&target);
        let plan = self.prepare_application(
            source,
            ApplicationRequest {
                origin,
                generic_parameters: &generic_parameters,
                slots: &slots,
                result_template,
                explicit_arguments: &[],
                explicit_parameters: None,
                preset_replacements: None,
                allowed_ownership_parameter: self
                    .conditional_ownership_parameter(&target, &generic_parameters),
                ty,
                defer_underconstrained: deferred_reserved,
            },
        )?;
        Ok(InvocationPlan::Application {
            plan,
            continuation: Box::new(InvocationFrame::construction(
                expression,
                origin,
                slots,
                target,
                generic_parameters.into_boxed_slice(),
            )),
        })
    }

    pub(super) fn bind_constructor_slots(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        target: &CanonicalTarget,
        shape: &ConstructorShape,
        payload: &ConstructorPayload,
    ) -> Result<Outcome<Vec<ApplicationSlot>>, BodyTypingError> {
        let expected_form = shape.form();
        let actual_form = ConstructorPayloadForm::from_payload(payload);
        if expected_form != actual_form {
            return self
                .constructor_payload_form_error(source, origin, target, expected_form, actual_form)
                .map(Outcome::Poisoned);
        }
        match (shape, payload) {
            (ConstructorShape::Absent, ConstructorPayload::Absent) => {
                Ok(Outcome::Known(Vec::new()))
            }
            (ConstructorShape::Tuple(types), ConstructorPayload::Tuple(values)) => {
                if values.len() > types.len() {
                    let extra = values[types.len()];
                    return self
                        .type_error(
                            source,
                            self.expression_origin(source, extra)?,
                            "CALL_TOO_MANY_ARGS",
                            "constructor call has too many payload arguments",
                            BTreeMap::new(),
                        )
                        .map(Outcome::Poisoned);
                }
                if values.len() < types.len() {
                    let parameter = format!("#{}", values.len());
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
                        .map(Outcome::Poisoned);
                }
                let mut slots = Vec::with_capacity(types.len());
                for (index, (&template, &value)) in types.iter().zip(values).enumerate() {
                    slots.push(ApplicationSlot {
                        name: format!("#{index}").into_boxed_str(),
                        template,
                        value,
                        origin: self.expression_origin(source, value)?,
                        related: None,
                        evaluation_order: index,
                        passing: ArgumentPassing::Value,
                    });
                }
                Ok(Outcome::Known(slots))
            }
            (ConstructorShape::Named(declarations), ConstructorPayload::Named(fields)) => {
                if declarations.is_empty() {
                    return Err(BodyTypingError::EmptyNamedConstructorPayload(
                        target.clone(),
                    ));
                }
                let owner = self.target_label(target);
                let mut initializers = BTreeMap::<CanonicalTarget, usize>::new();
                for (index, field) in fields.iter().enumerate() {
                    let Some(name) = field.name.text() else {
                        return Ok(Outcome::Poisoned(
                            source.invalid_poison(field.name.origin()).cloned().ok_or(
                                BodyTypingError::MissingOriginSpan {
                                    file: source.file(),
                                    origin: field.name.origin(),
                                },
                            )?,
                        ));
                    };
                    let Some(declaration) = self.find_named_target(declarations, name) else {
                        return self
                            .struct_initializer_error(
                                source,
                                field.origin,
                                "STRUCT_INIT_UNKNOWN_FIELD",
                                "constructor struct initializer names an unknown field",
                                &owner,
                                ("field", name),
                            )
                            .map(Outcome::Poisoned);
                    };
                    if initializers.insert(declaration, index).is_some() {
                        return self
                            .struct_initializer_error(
                                source,
                                field.origin,
                                "STRUCT_INIT_DUPLICATE_FIELD",
                                "constructor struct initializer has a duplicate field",
                                &owner,
                                ("field", name),
                            )
                            .map(Outcome::Poisoned);
                    }
                }
                if let Some(missing) = declarations
                    .iter()
                    .find(|field| !initializers.contains_key(*field))
                {
                    let name = self.target_label(missing);
                    return self
                        .struct_initializer_error(
                            source,
                            origin,
                            "STRUCT_INIT_MISSING_FIELD",
                            "constructor struct initializer has a missing field",
                            &owner,
                            ("field", &name),
                        )
                        .map(Outcome::Poisoned);
                }
                let mut slots = Vec::with_capacity(declarations.len());
                for declaration in declarations {
                    let template = match self.signatures.declaration(declaration) {
                        FactState::Known(DeclarationSignature::Field { ty }) => *ty,
                        FactState::Poisoned(poison) => {
                            return Ok(Outcome::Poisoned(poison.clone()));
                        }
                        FactState::Unperformed => return Ok(Outcome::Unperformed),
                        FactState::Known(_) | FactState::NotApplicable => {
                            return Err(BodyTypingError::UnexpectedSignatureKind {
                                target: declaration.clone(),
                                expected: "named constructor field",
                            });
                        }
                    };
                    let initializer = &fields[initializers[declaration]];
                    slots.push(ApplicationSlot {
                        name: self.target_label(declaration).into_boxed_str(),
                        template,
                        value: initializer.value,
                        origin: initializer.origin,
                        related: self.target_span(declaration),
                        evaluation_order: initializers[declaration],
                        passing: ArgumentPassing::Value,
                    });
                }
                Ok(Outcome::Known(slots))
            }
            _ => Err(BodyTypingError::InconsistentConstructorPayloadForm {
                expected: expected_form.spelling(),
                actual: actual_form.spelling(),
            }),
        }
    }

    pub(super) fn constructor_payload_form_error(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        target: &CanonicalTarget,
        expected: ConstructorPayloadForm,
        actual: ConstructorPayloadForm,
    ) -> Result<Poison, BodyTypingError> {
        let constructor = self.constructor_label(target)?;
        self.type_error(
            source,
            origin,
            "CONSTRUCTOR_PAYLOAD_FORM_MISMATCH",
            "constructor payload form mismatch",
            BTreeMap::from([
                (
                    Box::<str>::from("constructor"),
                    DiagnosticDataValue::from(constructor.as_str()),
                ),
                (
                    Box::<str>::from("expected_form"),
                    DiagnosticDataValue::from(expected.spelling()),
                ),
                (
                    Box::<str>::from("actual_form"),
                    DiagnosticDataValue::from(actual.spelling()),
                ),
            ]),
        )
    }

    pub(super) fn pattern_constructor_target(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        pattern: PatternId,
        origin: OriginId,
    ) -> Result<Outcome<ConstructorTarget>, BodyTypingError> {
        let target = match self.names.fact(ResolutionKey::Pattern { module, pattern }) {
            FactState::Known(ResolvedName::Declarations(targets)) if targets.len() == 1 => {
                targets[0].clone()
            }
            FactState::Known(ResolvedName::Declarations(_)) => {
                return Ok(self.unsupported(
                    TypecheckCapability::CheckAdvancedPatterns,
                    source,
                    origin,
                ));
            }
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Known(ResolvedName::Local(_)) => {
                return Err(BodyTypingError::UnexpectedPatternResolution {
                    module,
                    pattern,
                    fact: "local binding",
                });
            }
            FactState::NotApplicable => {
                return Err(BodyTypingError::UnexpectedPatternResolution {
                    module,
                    pattern,
                    fact: "not-applicable name",
                });
            }
            FactState::Unperformed => return Ok(Outcome::Unperformed),
        };
        let signature = match self.signatures.declaration(&target) {
            FactState::Known(signature) => signature.clone(),
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => return Ok(Outcome::Unperformed),
            FactState::NotApplicable => {
                return Err(BodyTypingError::UnexpectedSignatureKind {
                    target,
                    expected: "pattern constructor target",
                });
            }
        };
        let kind = self.resolved_target_kind(&target, &signature)?;
        Ok(Outcome::Known(ConstructorTarget {
            declaration: target,
            signature,
            kind,
        }))
    }

    pub(super) fn pattern_constructor_contract(
        &mut self,
        target: &ConstructorTarget,
    ) -> Result<PatternConstructorContract, BodyTypingError> {
        let (generic_parameters, result, shape) = match &target.signature {
            DeclarationSignature::Constructor(constructor) => (
                constructor.generic_parameters().to_vec(),
                constructor.result(),
                match constructor.payload() {
                    ConstructorPayloadSignature::Unit => ConstructorShape::Absent,
                    ConstructorPayloadSignature::Positional(types) => {
                        ConstructorShape::Tuple(types.to_vec())
                    }
                },
            ),
            DeclarationSignature::EnumVariant { payload } => {
                let CanonicalTarget::Source(variant) = &target.declaration else {
                    return Err(BodyTypingError::UnexpectedSignatureKind {
                        target: target.declaration.clone(),
                        expected: "source enum variant",
                    });
                };
                let owner = self
                    .collection
                    .declaration(*variant)
                    .and_then(|declaration| declaration.parent())
                    .ok_or(BodyTypingError::MissingDeclarationParent(*variant))?;
                let generic_parameters = match self.signatures.source_declaration(owner) {
                    FactState::Known(DeclarationSignature::Enum {
                        generic_parameters, ..
                    }) => generic_parameters.to_vec(),
                    _ => {
                        return Err(BodyTypingError::UnexpectedSignatureKind {
                            target: CanonicalTarget::Source(owner),
                            expected: "enum",
                        });
                    }
                };
                let result =
                    self.nominal_template(CanonicalTarget::Source(owner), &generic_parameters)?;
                let shape = match payload {
                    EnumVariantPayloadSignature::Absent => ConstructorShape::Absent,
                    EnumVariantPayloadSignature::Tuple(types) => {
                        ConstructorShape::Tuple(types.to_vec())
                    }
                    EnumVariantPayloadSignature::Named(fields) => {
                        ConstructorShape::Named(fields.to_vec())
                    }
                };
                (generic_parameters, result, shape)
            }
            _ => {
                return Err(BodyTypingError::UnexpectedSignatureKind {
                    target: target.declaration.clone(),
                    expected: "variant constructor",
                });
            }
        };
        Ok(PatternConstructorContract {
            declaration: target.declaration.clone(),
            generic_parameters,
            result,
            shape,
        })
    }

    pub(super) fn constructor_target(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        origin: OriginId,
    ) -> Result<Outcome<ConstructorTarget>, BodyTypingError> {
        let target = match self
            .names
            .fact(ResolutionKey::Expression { module, expression })
        {
            FactState::Known(ResolvedName::Declarations(targets)) if targets.len() == 1 => {
                targets[0].clone()
            }
            FactState::Known(ResolvedName::Declarations(_)) => {
                return Ok(self.unsupported(TypecheckCapability::CheckClosures, source, origin));
            }
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Known(ResolvedName::Local(_)) => {
                return Err(BodyTypingError::UnexpectedConstructorResolution {
                    expression: ExpressionKey::new(module, expression),
                    fact: "local binding",
                });
            }
            FactState::NotApplicable => {
                return Err(BodyTypingError::UnexpectedConstructorResolution {
                    expression: ExpressionKey::new(module, expression),
                    fact: "not-applicable name",
                });
            }
            FactState::Unperformed => return Ok(Outcome::Unperformed),
        };
        let signature = match self.signatures.declaration(&target) {
            FactState::Known(signature) => signature.clone(),
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => return Ok(Outcome::Unperformed),
            FactState::NotApplicable => {
                return Err(BodyTypingError::UnexpectedSignatureKind {
                    target,
                    expected: "constructor target",
                });
            }
        };
        let kind = self.resolved_target_kind(&target, &signature)?;
        Ok(Outcome::Known(ConstructorTarget {
            declaration: target,
            signature,
            kind,
        }))
    }

    pub(super) fn constructor_target_kind_error(
        &mut self,
        source: &SemanticSource<'_>,
        head_origin: OriginId,
        target: &ConstructorTarget,
        expected: ResolvedTargetKind,
    ) -> Result<Option<Poison>, BodyTypingError> {
        let actual = target.kind;
        if actual == expected {
            return Ok(None);
        }
        let target_label = self.target_label(&target.declaration);
        let related = self.target_span(&target.declaration).map(|span| {
            RelatedSpan::new(span, "target declared here")
                .expect("static related-span label is non-empty")
        });
        let data = BTreeMap::from([
            (
                Box::<str>::from("target"),
                DiagnosticDataValue::from(target_label.as_str()),
            ),
            (
                Box::<str>::from("expected_kind"),
                DiagnosticDataValue::from(expected.spelling()),
            ),
            (
                Box::<str>::from("actual_kind"),
                DiagnosticDataValue::from(actual.spelling()),
            ),
        ]);
        self.type_error_with_related(
            source,
            head_origin,
            "CONSTRUCTOR_TARGET_KIND_MISMATCH",
            format!(
                "constructor target kind mismatch for `{target_label}`: expected {}, found {}",
                expected.spelling(),
                actual.spelling()
            ),
            related,
            data,
        )
        .map(Some)
    }

    pub(super) fn resolved_target_kind(
        &self,
        target: &CanonicalTarget,
        signature: &DeclarationSignature,
    ) -> Result<ResolvedTargetKind, BodyTypingError> {
        let signature_kind = ResolvedTargetKind::from_signature(signature);
        if let CanonicalTarget::Source(declaration) = target {
            let declaration_kind = self
                .collection
                .declaration(*declaration)
                .ok_or(BodyTypingError::UnknownDeclaration(*declaration))?
                .kind();
            let declaration_kind = ResolvedTargetKind::from_declaration(declaration_kind);
            if declaration_kind != signature_kind {
                return Err(BodyTypingError::UnexpectedSignatureKind {
                    target: target.clone(),
                    expected: declaration_kind.spelling(),
                });
            }
        }
        Ok(signature_kind)
    }

    pub(super) fn nominal_template(
        &mut self,
        owner: CanonicalTarget,
        generic_parameters: &[GenericParameterId],
    ) -> Result<TypeId, BodyTypingError> {
        let mut arguments = Vec::with_capacity(generic_parameters.len());
        for parameter in generic_parameters {
            arguments.push(
                self.signatures
                    .types_mut()
                    .generic_parameter(parameter.clone())?,
            );
        }
        Ok(self
            .signatures
            .types_mut()
            .nominal(NominalHead::Declaration(owner), arguments)?)
    }

    pub(super) fn find_named_target(
        &self,
        targets: &[CanonicalTarget],
        name: &str,
    ) -> Option<CanonicalTarget> {
        targets
            .iter()
            .find(|target| self.target_name(target) == Some(name))
            .cloned()
    }

    pub(super) fn target_name<'target>(
        &'target self,
        target: &'target CanonicalTarget,
    ) -> Option<&'target str> {
        match target {
            CanonicalTarget::Source(declaration) => self
                .collection
                .declaration(*declaration)
                .and_then(|declaration| declaration.name())
                .map(NameKey::text),
            CanonicalTarget::External(declaration) => declaration.as_str().rsplit("::").next(),
        }
    }

    pub(super) fn constructor_label(
        &self,
        target: &CanonicalTarget,
    ) -> Result<String, BodyTypingError> {
        let CanonicalTarget::Source(variant) = target else {
            return Ok(self.target_label(target));
        };
        let variant_declaration = self
            .collection
            .declaration(*variant)
            .ok_or(BodyTypingError::UnknownDeclaration(*variant))?;
        let variant_name = variant_declaration
            .name()
            .ok_or(BodyTypingError::MissingDeclarationName(*variant))?;
        let owner = variant_declaration
            .parent()
            .ok_or(BodyTypingError::MissingDeclarationParent(*variant))?;
        let owner_declaration = self
            .collection
            .declaration(owner)
            .ok_or(BodyTypingError::UnknownDeclaration(owner))?;
        let owner_name = owner_declaration
            .name()
            .ok_or(BodyTypingError::MissingDeclarationName(owner))?;
        Ok(format!(
            "{}::{}",
            owner_name.spelling(),
            variant_name.spelling()
        ))
    }

    pub(super) fn target_span(&self, target: &CanonicalTarget) -> Option<SourceSpan> {
        let CanonicalTarget::Source(declaration) = target else {
            return None;
        };
        let declaration = self.collection.declaration(*declaration)?;
        let module = self.collection.module(declaration.id().module())?;
        self.sources
            .get(&module.file())?
            .origin_span(declaration.origin())
            .cloned()
    }

    pub(super) fn storage_field_mutability(
        &self,
        target: &CanonicalTarget,
    ) -> Result<Mutability, BodyTypingError> {
        let CanonicalTarget::Source(declaration) = target else {
            return Ok(Mutability::Immutable);
        };
        let (_, _, _hir, item) = self.source_item(*declaration)?;
        let ItemKind::StorageField(field) = item.kind() else {
            return Err(BodyTypingError::WrongDeclarationKind(*declaration));
        };
        Ok(field.mutability)
    }

    pub(super) fn struct_initializer_error(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        code: &'static str,
        message: &'static str,
        owner: &str,
        member: (&'static str, &str),
    ) -> Result<Poison, BodyTypingError> {
        let (member_key, member) = member;
        self.type_error(
            source,
            origin,
            code,
            message,
            BTreeMap::from([
                (Box::<str>::from("struct"), DiagnosticDataValue::from(owner)),
                (
                    Box::<str>::from(member_key),
                    DiagnosticDataValue::from(member),
                ),
            ]),
        )
    }

    pub(super) fn is_reserved_constructor(&self, target: &CanonicalTarget) -> bool {
        [
            LanguageItem::OptSome,
            LanguageItem::OptNone,
            LanguageItem::ResOk,
            LanguageItem::ResErr,
        ]
        .into_iter()
        .filter_map(|item| self.signatures.language_items().declaration(item))
        .any(|declaration| declaration == target)
    }

    pub(super) fn specialize_member_type(
        &mut self,
        template: TypeId,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
        owner: &CanonicalTarget,
        receiver: TypeId,
    ) -> Result<TypeId, BodyTypingError> {
        let substituted = self
            .signatures
            .types_mut()
            .substitute(template, replacements)?;
        Ok(self
            .signatures
            .types_mut()
            .replace_self_type(substituted, owner, receiver)?)
    }
}
