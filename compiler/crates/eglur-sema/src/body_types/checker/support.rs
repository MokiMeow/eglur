use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn external_semantic_owner(
        &self,
        target: &CanonicalTarget,
    ) -> Option<ExternalSemanticOwner> {
        let CanonicalTarget::External(id) = target else {
            return None;
        };
        self.external_names
            .and_then(|names| names.declaration(id))
            .map(|declaration| declaration.semantic_owner())
    }

    pub(super) fn primitive(
        &mut self,
        primitive: PrimitiveType,
    ) -> Result<TypeId, BodyTypingError> {
        Ok(self.signatures.types_mut().primitive(primitive)?)
    }

    pub(super) fn diagnostic_count(
        &self,
        count: usize,
    ) -> Result<DiagnosticDataValue, BodyTypingError> {
        let count =
            i64::try_from(count).map_err(|_| BodyTypingError::DiagnosticCountOutOfRange(count))?;
        Ok(DiagnosticDataValue::from(count))
    }

    pub(super) fn value_expression(&self, ty: TypeId) -> TypedExpression {
        TypedExpression {
            ty,
            place: None,
            integer_constant: None,
            normal_continuation: self.continuation_for_type(ty),
        }
    }

    pub(super) fn value_expression_with_continuation(
        &self,
        ty: TypeId,
        normal_continuation: NormalContinuation,
    ) -> TypedExpression {
        TypedExpression {
            ty,
            place: None,
            integer_constant: None,
            normal_continuation: self.continuation_for_type(ty).strict(normal_continuation),
        }
    }

    pub(super) fn continuation_for_type(&self, ty: TypeId) -> NormalContinuation {
        if ty == self.signatures.types().never() {
            NormalContinuation::Absent
        } else {
            NormalContinuation::Present
        }
    }

    pub(super) fn type_occurrence(
        &self,
        source: &SemanticSource<'_>,
        ty: TypeRefId,
    ) -> Result<Outcome<TypeId>, BodyTypingError> {
        match self
            .signatures
            .occurrence(TypeOccurrenceId::new(source.file(), ty))
        {
            FactState::Known(occurrence) => occurrence
                .canonical_type()
                .map(Outcome::Known)
                .ok_or(BodyTypingError::NonTypeOccurrence(ty)),
            FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => Ok(Outcome::Unperformed),
            FactState::NotApplicable => Ok(Outcome::NotApplicable),
        }
    }

    pub(super) fn expression_origin(
        &self,
        source: &SemanticSource<'_>,
        expression: ExprId,
    ) -> Result<OriginId, BodyTypingError> {
        source
            .hir()
            .and_then(|hir| hir.body(expression.body()).expression(expression))
            .map(eglur_hir::Expr::origin)
            .ok_or(BodyTypingError::WrongBodyExpression(expression))
    }

    pub(super) fn record_expression(
        &mut self,
        key: ExpressionKey,
        outcome: Outcome<TypedExpression>,
    ) -> Result<(), BodyTypingError> {
        let fact = match outcome {
            Outcome::Known(value) => Some(SemanticFact::Known(value)),
            Outcome::Poisoned(poison) => Some(SemanticFact::Poisoned(poison)),
            Outcome::NotApplicable => Some(SemanticFact::NotApplicable),
            Outcome::Deferred | Outcome::Unperformed => None,
        };
        if let Some(fact) = fact {
            self.expressions.insert(key, fact)?;
        }
        Ok(())
    }

    pub(super) fn merge_unit(
        &mut self,
        left: Outcome<()>,
        right: Outcome<()>,
    ) -> Result<Outcome<()>, BodyTypingError> {
        match (left, right) {
            (Outcome::Poisoned(left), Outcome::Poisoned(right)) => {
                Ok(Outcome::Poisoned(left.merge(&right)?))
            }
            (Outcome::Poisoned(poison), _) | (_, Outcome::Poisoned(poison)) => {
                Ok(Outcome::Poisoned(poison))
            }
            (Outcome::Unperformed, _) | (_, Outcome::Unperformed) => Ok(Outcome::Unperformed),
            (Outcome::Deferred, _) | (_, Outcome::Deferred) => Ok(Outcome::Deferred),
            (Outcome::NotApplicable, _) | (_, Outcome::NotApplicable) => Ok(Outcome::NotApplicable),
            (Outcome::Known(()), Outcome::Known(())) => Ok(Outcome::Known(())),
        }
    }

    pub(super) fn merge_poison(
        &mut self,
        retained: &mut Option<Poison>,
        incoming: &Poison,
    ) -> Result<(), BodyTypingError> {
        *retained = Some(match retained.take() {
            Some(existing) => existing.merge(incoming)?,
            None => incoming.clone(),
        });
        Ok(())
    }

    pub(super) fn is_fixed_width_integer(&self, ty: TypeId) -> Result<bool, BodyTypingError> {
        Ok(matches!(
            self.signatures.types().kind(ty)?,
            TypeKind::Primitive(
                PrimitiveType::I8
                    | PrimitiveType::I16
                    | PrimitiveType::I32
                    | PrimitiveType::I64
                    | PrimitiveType::U8
                    | PrimitiveType::U16
                    | PrimitiveType::U32
                    | PrimitiveType::U64
            )
        ))
    }

    pub(super) fn is_signed_fixed_width_integer(
        &self,
        ty: TypeId,
    ) -> Result<bool, BodyTypingError> {
        Ok(matches!(
            self.signatures.types().kind(ty)?,
            TypeKind::Primitive(
                PrimitiveType::I8 | PrimitiveType::I16 | PrimitiveType::I32 | PrimitiveType::I64
            )
        ))
    }

    pub(super) fn normalize_type_at(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        ty: TypeId,
    ) -> Result<Outcome<TypeId>, BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        match self
            .trait_environment
            .normalize_for_owner(owner, self.signatures, ty)?
        {
            TraitNormalization::Normalized(ty) | TraitNormalization::Symbolic(ty) => {
                Ok(Outcome::Known(ty))
            }
            TraitNormalization::Poisoned(poison) => Ok(Outcome::Poisoned(poison)),
            TraitNormalization::InvalidArgument(violation) => {
                let (code, message) = crate::diagnostic::impl_argument_diagnostic(violation);
                self.type_error(source, origin, code, message, BTreeMap::new())
                    .map(Outcome::Poisoned)
            }
            TraitNormalization::Unsatisfied(obligation)
            | TraitNormalization::Ambiguous(obligation) => {
                let data = BTreeMap::from([
                    (
                        Box::<str>::from("trait"),
                        DiagnosticDataValue::from(self.format_trait_ref(obligation.trait_ref())?),
                    ),
                    (
                        Box::<str>::from("type"),
                        DiagnosticDataValue::from(self.format_type(obligation.subject())?),
                    ),
                ]);
                self.type_error(
                    source,
                    origin,
                    "TRAIT_BOUND_NOT_SATISFIED",
                    "trait bound is not satisfied by the projected type base",
                    data,
                )
                .map(Outcome::Poisoned)
            }
            TraitNormalization::Cyclic(_) => self
                .type_error(
                    source,
                    origin,
                    "ASSOCIATED_TYPE_CYCLE",
                    "associated type normalization contains a cycle",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned),
        }
    }

    pub(super) fn type_error(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        code: &'static str,
        message: impl Into<String>,
        data: BTreeMap<Box<str>, DiagnosticDataValue>,
    ) -> Result<Poison, BodyTypingError> {
        let span =
            source
                .origin_span(origin)
                .cloned()
                .ok_or(BodyTypingError::MissingOriginSpan {
                    file: source.file(),
                    origin,
                })?;
        Ok(record_type_error(
            self.diagnostics,
            code,
            message,
            span,
            data,
        )?)
    }

    pub(super) fn type_error_with_related(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        code: &'static str,
        message: impl Into<String>,
        related: impl IntoIterator<Item = RelatedSpan>,
        data: BTreeMap<Box<str>, DiagnosticDataValue>,
    ) -> Result<Poison, BodyTypingError> {
        let span =
            source
                .origin_span(origin)
                .cloned()
                .ok_or(BodyTypingError::MissingOriginSpan {
                    file: source.file(),
                    origin,
                })?;
        Ok(record_type_error_with_related(
            self.diagnostics,
            code,
            message,
            span,
            related,
            data,
        )?)
    }

    pub(super) fn assignment_mismatch(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        expected: TypeId,
        actual: TypeId,
    ) -> Result<Poison, BodyTypingError> {
        self.type_error(
            source,
            origin,
            "ASSIGNMENT_TYPE_MISMATCH",
            "assignment type mismatch with expected storage type",
            BTreeMap::from([
                (
                    Box::<str>::from("expected_type"),
                    DiagnosticDataValue::from(self.format_type(expected)?),
                ),
                (
                    Box::<str>::from("actual_type"),
                    DiagnosticDataValue::from(self.format_type(actual)?),
                ),
            ]),
        )
    }

    pub(super) fn return_mismatch(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        expected: TypeId,
        actual: TypeId,
    ) -> Result<Poison, BodyTypingError> {
        let data = BTreeMap::from([
            (
                Box::<str>::from("expected_type"),
                DiagnosticDataValue::from(self.format_type(expected)?),
            ),
            (
                Box::<str>::from("actual_type"),
                DiagnosticDataValue::from(self.format_type(actual)?),
            ),
        ]);
        self.type_error(
            source,
            origin,
            "RETURN_TYPE_MISMATCH",
            "return type mismatch with function result",
            data,
        )
    }

    pub(super) fn demand_unimplemented(
        &mut self,
        capability: TypecheckCapability,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) {
        let supported = self.recorder.require(
            CompileCapability::Typecheck(capability),
            SemanticLocation::origin(source.file(), origin),
        );
        debug_assert!(
            !supported,
            "an unimplemented body-typing capability was selected: {}",
            capability.stable_key()
        );
    }

    pub(super) fn unsupported<T>(
        &mut self,
        capability: TypecheckCapability,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Outcome<T> {
        self.demand_unimplemented(capability, source, origin);
        Outcome::Unperformed
    }

    pub(super) fn format_type(&self, ty: TypeId) -> Result<String, BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        Ok(DiagnosticTypeRenderer::new(
            self.collection,
            self.signatures.diagnostic_type_names(),
            &*self.signatures,
            owner,
        )
        .render_type(ty)?)
    }

    pub(super) fn format_trait_ref(&self, trait_ref: &TraitRef) -> Result<String, BodyTypingError> {
        let owner = self
            .current_owner
            .ok_or(BodyTypingError::MissingDiagnosticOwner)?;
        Ok(DiagnosticTypeRenderer::new(
            self.collection,
            self.signatures.diagnostic_type_names(),
            &*self.signatures,
            owner,
        )
        .render_trait_ref(trait_ref)?)
    }

    pub(super) fn target_label(&self, target: &CanonicalTarget) -> String {
        match target {
            CanonicalTarget::Source(declaration) => self
                .collection
                .declaration(*declaration)
                .and_then(|declaration| declaration.name())
                .map_or_else(|| format!("{declaration:?}"), NameKey::spelling),
            CanonicalTarget::External(declaration) => declaration.as_str().to_owned(),
        }
    }

    pub(super) fn local_binding(
        &self,
        module: ModuleId,
        body: BodyId,
        origin: OriginId,
    ) -> Result<LocalBindingId, BodyTypingError> {
        self.names
            .local_binding(module, body, origin)
            .ok_or(BodyTypingError::MissingLocalOrigin {
                module,
                body,
                origin,
            })
    }
}
