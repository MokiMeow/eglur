use super::*;

#[cfg(test)]
mod poison_tests;

pub(super) enum ValuePlan {
    Complete(Outcome<TypedExpression>),
    Expression {
        expression: ExprId,
        request: TypeRequest,
        path: NormalContinuation,
        continuation: ValueContinuation,
    },
}

impl ValuePlan {
    pub(super) fn single(expression: ExprId, continuation: ValueContinuation) -> Self {
        Self::Expression {
            expression,
            request: TypeRequest::definitive(None),
            path: NormalContinuation::Present,
            continuation,
        }
    }
}

pub(super) enum ValueContinuation {
    Tuple(Box<TupleFrame>),
    Reference {
        value: ExprId,
        mutability: Mutability,
    },
    Dereference {
        operand: ExprId,
    },
    Member {
        expression: ExprId,
        receiver: ExprId,
        member: Name,
    },
    TupleProjection {
        receiver: ExprId,
        index: Box<str>,
        origin: OriginId,
    },
}

pub(super) struct TupleFrame {
    values: Box<[ExprId]>,
    request: TypeRequest,
    expected: Option<Vec<TypeId>>,
    next: usize,
    types: Vec<TypeId>,
    normal_continuation: NormalContinuation,
    poison: eglur_diagnostics::PoisonAccumulator,
    unperformed: bool,
    deferred: bool,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_tuple(
        &mut self,
        values: Box<[ExprId]>,
        request: TypeRequest,
    ) -> Result<ValuePlan, BodyTypingError> {
        let expected = match request.expected {
            Some(expected) => match self.signatures.types().kind(expected)? {
                TypeKind::Tuple(elements) if elements.len() == values.len() => {
                    Some(elements.to_vec())
                }
                _ => None,
            },
            None => None,
        };
        let types = Vec::with_capacity(values.len());
        self.advance_tuple(Box::new(TupleFrame {
            values,
            request,
            expected,
            next: 0,
            types,
            normal_continuation: NormalContinuation::Present,
            poison: eglur_diagnostics::PoisonAccumulator::new(),
            unperformed: false,
            deferred: false,
        }))
    }

    fn advance_tuple(&mut self, mut frame: Box<TupleFrame>) -> Result<ValuePlan, BodyTypingError> {
        if let Some(&expression) = frame.values.get(frame.next) {
            let request = TypeRequest {
                expected: frame.expected.as_ref().map(|elements| elements[frame.next]),
                ..frame.request
            };
            frame.next += 1;
            return Ok(ValuePlan::Expression {
                expression,
                request,
                path: frame.normal_continuation,
                continuation: ValueContinuation::Tuple(frame),
            });
        }
        if let Some(poison) = frame.poison.finish() {
            #[cfg(test)]
            poison_tests::observe(poison_tests::Step::Complete);
            return Ok(ValuePlan::Complete(Outcome::Poisoned(poison)));
        }
        if frame.unperformed {
            return Ok(ValuePlan::Complete(Outcome::Unperformed));
        }
        if frame.deferred {
            return Ok(ValuePlan::Complete(Outcome::Deferred));
        }
        let ty = self.signatures.types_mut().tuple(frame.types)?;
        Ok(ValuePlan::Complete(Outcome::Known(
            self.value_expression_with_continuation(ty, frame.normal_continuation),
        )))
    }

    pub(super) fn prepare_member(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        receiver: ExprId,
        member: Name,
    ) -> Result<ValuePlan, BodyTypingError> {
        let source = context.source;
        if !member.is_valid() {
            return Ok(ValuePlan::Complete(Outcome::Poisoned(
                source.invalid_poison(member.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: member.origin(),
                    },
                )?,
            )));
        }
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
            SemanticLocation::origin(source.file(), member.origin()),
        ) {
            return Ok(ValuePlan::Complete(Outcome::Unperformed));
        }
        Ok(ValuePlan::single(
            receiver,
            ValueContinuation::Member {
                expression,
                receiver,
                member,
            },
        ))
    }

    pub(super) fn resume_value(
        &mut self,
        context: BodyContext<'_, '_>,
        continuation: ValueContinuation,
        outcome: Outcome<TypedExpression>,
    ) -> Result<ValuePlan, BodyTypingError> {
        let outcome = match continuation {
            ValueContinuation::Tuple(mut frame) => {
                match outcome {
                    Outcome::Known(value) => {
                        frame.types.push(value.ty);
                        frame.normal_continuation =
                            frame.normal_continuation.strict(value.normal_continuation);
                    }
                    Outcome::Poisoned(found) => {
                        #[cfg(test)]
                        poison_tests::observe(poison_tests::Step::Input);
                        frame.poison.include(&found)?;
                    }
                    Outcome::Deferred => frame.deferred = true,
                    Outcome::Unperformed | Outcome::NotApplicable => frame.unperformed = true,
                }
                return self.advance_tuple(frame);
            }
            ValueContinuation::Reference { value, mutability } => {
                self.finish_reference(context.source, value, mutability, outcome)?
            }
            ValueContinuation::Dereference { operand } => {
                self.finish_dereference(context.source, operand, outcome)?
            }
            ValueContinuation::Member {
                expression,
                receiver,
                member,
            } => self.finish_member(context, expression, receiver, &member, outcome)?,
            ValueContinuation::TupleProjection {
                receiver,
                index,
                origin,
            } => self.finish_tuple_projection(context.source, receiver, &index, origin, outcome)?,
        };
        Ok(ValuePlan::Complete(outcome))
    }

    fn finish_member(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        receiver: ExprId,
        member: &Name,
        receiver_value: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let Outcome::Known(receiver_value) = receiver_value else {
            return Ok(receiver_value);
        };
        let (subject, reference_writable) = match self.signatures.types().kind(receiver_value.ty)? {
            TypeKind::Reference {
                mutability,
                referent,
            } => (*referent, Some(*mutability == ReferenceMutability::Mutable)),
            _ => (receiver_value.ty, None),
        };
        let TypeKind::Nominal {
            head: NominalHead::Declaration(owner),
            substitution,
        } = self.signatures.types().kind(subject)?.clone()
        else {
            return self.missing_value_member(source, member, subject);
        };
        let (generic_parameters, storage_fields, properties) =
            match self.signatures.declaration(&owner) {
                FactState::Known(DeclarationSignature::Struct {
                    generic_parameters,
                    storage_fields,
                    properties,
                }) => (
                    generic_parameters.to_vec(),
                    storage_fields.to_vec(),
                    properties.to_vec(),
                ),
                FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                FactState::Known(DeclarationSignature::Enum { .. }) => {
                    return self.missing_value_member(source, member, subject);
                }
                FactState::Known(DeclarationSignature::Nominal { .. }) => {
                    let CanonicalTarget::External(owner) = owner else {
                        return Err(BodyTypingError::UnexpectedSignatureKind {
                            target: owner,
                            expected: "struct or enum",
                        });
                    };
                    self.recorder.refuse_input(
                        crate::UnavailableCompileInput::TrustedValueMemberInventory { owner },
                        SemanticLocation::origin(source.file(), member.origin()),
                    );
                    return Ok(Outcome::Unperformed);
                }
                FactState::Known(_) | FactState::NotApplicable => {
                    return Err(BodyTypingError::UnexpectedSignatureKind {
                        target: owner,
                        expected: "struct, enum or trusted nominal",
                    });
                }
            };
        let member_name = member.text().expect("valid member has text");
        let stored = self.find_named_target(&storage_fields, member_name);
        let property = self.find_named_target(&properties, member_name);
        let (declaration, kind, template) = if let Some(declaration) = stored {
            let template = match self.signatures.declaration(&declaration) {
                FactState::Known(DeclarationSignature::Field { ty, .. }) => *ty,
                FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                FactState::Known(_) | FactState::NotApplicable => {
                    return Err(BodyTypingError::UnexpectedSignatureKind {
                        target: declaration,
                        expected: "stored field",
                    });
                }
            };
            let mutability = self.storage_field_mutability(&declaration)?;
            (
                declaration,
                MemberKind::StoredField { mutability },
                template,
            )
        } else if let Some(declaration) = property {
            let template = match self.signatures.declaration(&declaration) {
                FactState::Known(DeclarationSignature::Property { ty }) => *ty,
                FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                FactState::Known(_) | FactState::NotApplicable => {
                    return Err(BodyTypingError::UnexpectedSignatureKind {
                        target: declaration,
                        expected: "property",
                    });
                }
            };
            (declaration, MemberKind::Property, template)
        } else {
            return self.missing_value_member(source, member, subject);
        };
        let arguments = self.signatures.types().substitution(substitution)?.to_vec();
        let replacements = generic_parameters
            .iter()
            .cloned()
            .zip(arguments)
            .collect::<BTreeMap<_, _>>();
        let ty = self.specialize_member_type(template, &replacements, &owner, subject)?;
        let ty = match self.normalize_type_at(source, member.origin(), ty)? {
            Outcome::Known(ty) => ty,
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred => return Ok(Outcome::Deferred),
            Outcome::Unperformed | Outcome::NotApplicable => return Ok(Outcome::Unperformed),
        };
        let place = match kind {
            MemberKind::StoredField { mutability } => reference_writable
                .or_else(|| receiver_value.place.as_ref().map(TypedPlace::is_writable))
                .map(|receiver_writable| TypedPlace {
                    kind: TypedPlaceKind::StoredField {
                        receiver,
                        field: declaration.clone(),
                    },
                    writable: receiver_writable && mutability == Mutability::Mutable,
                }),
            MemberKind::Property => None,
        };
        self.members.insert(
            ExpressionKey::new(module, expression),
            SemanticFact::Known(MemberAccess {
                declaration,
                kind,
                ty,
            }),
        )?;
        Ok(Outcome::Known(TypedExpression {
            ty,
            place,
            integer_constant: None,
            normal_continuation: self
                .continuation_for_type(ty)
                .strict(receiver_value.normal_continuation),
        }))
    }

    fn missing_value_member(
        &mut self,
        source: &SemanticSource<'_>,
        member: &Name,
        subject: TypeId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let name = member.text().expect("valid member has text");
        let receiver_type = self.format_type(subject)?;
        let poison = self.type_error(
            source,
            member.origin(),
            "MEMBER_NOT_FOUND",
            format!("field or property member `{name}` not found on `{receiver_type}`"),
            BTreeMap::from([
                ("member".into(), DiagnosticDataValue::from(name)),
                (
                    "receiver_type".into(),
                    DiagnosticDataValue::from(receiver_type),
                ),
            ]),
        )?;
        Ok(Outcome::Poisoned(poison))
    }

    fn finish_reference(
        &mut self,
        source: &SemanticSource<'_>,
        value_id: ExprId,
        mutability: Mutability,
        value: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let Outcome::Known(value) = value else {
            return Ok(value);
        };
        let valid = value
            .place
            .as_ref()
            .is_some_and(|place| mutability == Mutability::Immutable || place.writable);
        if !valid {
            let origin = self.expression_origin(source, value_id)?;
            let poison = self.type_error(
                source,
                origin,
                "REFERENCE_TARGET_INVALID",
                "reference target is invalid for the requested access",
                BTreeMap::new(),
            )?;
            return Ok(Outcome::Poisoned(poison));
        }
        let mutability = match mutability {
            Mutability::Immutable => ReferenceMutability::Shared,
            Mutability::Mutable => ReferenceMutability::Mutable,
        };
        let ty = self
            .signatures
            .types_mut()
            .reference(mutability, value.ty)?;
        Ok(Outcome::Known(self.value_expression_with_continuation(
            ty,
            value.normal_continuation,
        )))
    }

    fn finish_dereference(
        &mut self,
        source: &SemanticSource<'_>,
        operand: ExprId,
        operand_value: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let Outcome::Known(operand_value) = operand_value else {
            return Ok(operand_value);
        };
        let TypeKind::Reference {
            mutability,
            referent,
        } = self.signatures.types().kind(operand_value.ty)?
        else {
            return Ok(self.unsupported(
                TypecheckCapability::CheckOperators,
                source,
                self.expression_origin(source, operand)?,
            ));
        };
        let ty = *referent;
        Ok(Outcome::Known(TypedExpression {
            ty,
            place: Some(TypedPlace {
                kind: TypedPlaceKind::Dereference { reference: operand },
                writable: *mutability == ReferenceMutability::Mutable,
            }),
            integer_constant: None,
            normal_continuation: self
                .continuation_for_type(ty)
                .strict(operand_value.normal_continuation),
        }))
    }

    fn finish_tuple_projection(
        &mut self,
        source: &SemanticSource<'_>,
        receiver: ExprId,
        index: &str,
        origin: OriginId,
        receiver_value: Outcome<TypedExpression>,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let Outcome::Known(receiver_value) = receiver_value else {
            return Ok(receiver_value);
        };
        let TypeKind::Tuple(elements) = self.signatures.types().kind(receiver_value.ty)? else {
            let data = BTreeMap::from([(
                Box::<str>::from("receiver_type"),
                DiagnosticDataValue::from(self.format_type(receiver_value.ty)?),
            )]);
            return self
                .type_error(
                    source,
                    origin,
                    "TUPLE_PROJECTION_NON_TUPLE",
                    "tuple projection applied to a non-tuple receiver",
                    data,
                )
                .map(Outcome::Poisoned);
        };
        let parsed = index.parse::<usize>().ok();
        let Some(index) = parsed.filter(|&index| index < elements.len()) else {
            return self
                .type_error(
                    source,
                    origin,
                    "TUPLE_PROJECTION_OUT_OF_BOUNDS",
                    "tuple projection index is out of bounds",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        };
        let place = receiver_value.place.map(|place| TypedPlace {
            kind: TypedPlaceKind::TupleElement { receiver, index },
            writable: place.writable,
        });
        let ty = elements[index];
        Ok(Outcome::Known(TypedExpression {
            ty,
            place,
            integer_constant: None,
            normal_continuation: self
                .continuation_for_type(ty)
                .strict(receiver_value.normal_continuation),
        }))
    }
}
