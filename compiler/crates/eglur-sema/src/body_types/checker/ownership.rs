use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_own(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        origin: OriginId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let (binding, source_ty) = match self.ownership_local(module, expression)? {
            Outcome::Known(Some(operand)) => operand,
            Outcome::Known(None) | Outcome::NotApplicable => {
                return self
                    .type_error(
                        source,
                        origin,
                        "OWN_REQUIRES_LOCAL_BINDING",
                        "own requires a local binding or parameter",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Unperformed | Outcome::Deferred => return Ok(Outcome::Unperformed),
        };
        if self.signatures.types().ownership_root(source_ty)?.is_some() {
            return self
                .type_error(
                    source,
                    origin,
                    "OWN_REQUIRES_NON_OWN",
                    "own requires a non-Own operand",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        if self.signatures.types().contains_reference(source_ty)? {
            return self
                .type_error(
                    source,
                    origin,
                    "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                    "Own payload type must be reference-free",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        let result = self
            .signatures
            .types_mut()
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Own), [source_ty])?;
        self.record_ownership_operation(
            ExpressionKey::new(module, expression),
            TypedOwnershipOperationKind::Own,
            binding,
        )?;
        Ok(Outcome::Known(self.value_expression(result)))
    }

    pub(super) fn type_move(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        origin: OriginId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let (binding, source_ty) = match self.ownership_local(module, expression)? {
            Outcome::Known(Some(operand)) => operand,
            Outcome::Known(None) | Outcome::NotApplicable => {
                return self
                    .type_error(
                        source,
                        origin,
                        "MOVE_REQUIRES_OWN",
                        "move requires an Own operand",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Unperformed | Outcome::Deferred => return Ok(Outcome::Unperformed),
        };
        if self.signatures.types().ownership_root(source_ty)?.is_none() {
            return self
                .type_error(
                    source,
                    origin,
                    "MOVE_REQUIRES_OWN",
                    "move requires an Own operand",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        self.record_ownership_operation(
            ExpressionKey::new(module, expression),
            TypedOwnershipOperationKind::Move,
            binding,
        )?;
        Ok(Outcome::Known(self.value_expression(source_ty)))
    }

    pub(super) fn type_drop_own(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        expression: ExprId,
        origin: OriginId,
        explicit_move: bool,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let operand = self.ownership_local(module, expression)?;
        if let Outcome::Poisoned(poison) = operand {
            return Ok(Outcome::Poisoned(poison));
        }
        if matches!(operand, Outcome::Unperformed | Outcome::Deferred) {
            return Ok(Outcome::Unperformed);
        }
        if !explicit_move {
            return self
                .type_error(
                    source,
                    origin,
                    "DROP_OWN_REQUIRES_MOVE",
                    "drop_own requires an explicit move operand",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        let (binding, source_ty) = match operand {
            Outcome::Known(Some(operand)) => operand,
            Outcome::Known(None) | Outcome::NotApplicable => {
                return self
                    .type_error(
                        source,
                        origin,
                        "MOVE_REQUIRES_OWN",
                        "drop_own requires an Own operand",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            Outcome::Poisoned(_) | Outcome::Unperformed | Outcome::Deferred => {
                unreachable!("non-value ownership operand returned above")
            }
        };
        if self.signatures.types().ownership_root(source_ty)?.is_none() {
            return self
                .type_error(
                    source,
                    origin,
                    "MOVE_REQUIRES_OWN",
                    "drop_own requires an Own operand",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned);
        }
        self.record_ownership_operation(
            ExpressionKey::new(module, expression),
            TypedOwnershipOperationKind::DropOwn,
            binding,
        )?;
        let unit = self.primitive(PrimitiveType::Unit)?;
        Ok(Outcome::Known(self.value_expression(unit)))
    }

    fn ownership_local(
        &self,
        module: ModuleId,
        expression: ExprId,
    ) -> Result<Outcome<Option<(LocalBindingId, TypeId)>>, BodyTypingError> {
        let key = ResolutionKey::Expression { module, expression };
        match self.names.fact(key) {
            FactState::Known(ResolvedName::Local(binding)) => match self.locals.state(binding) {
                FactState::Known(local) => Ok(Outcome::Known(Some((*binding, local.ty)))),
                FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => Ok(Outcome::Unperformed),
                FactState::NotApplicable => Ok(Outcome::NotApplicable),
            },
            FactState::Known(ResolvedName::Declarations(_) | ResolvedName::TypeMemberOwner(_)) => {
                Ok(Outcome::Known(None))
            }
            FactState::Poisoned(poison) => Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => Ok(Outcome::Unperformed),
            FactState::NotApplicable => Ok(Outcome::NotApplicable),
        }
    }

    fn record_ownership_operation(
        &mut self,
        key: ExpressionKey,
        kind: TypedOwnershipOperationKind,
        source: LocalBindingId,
    ) -> Result<(), BodyTypingError> {
        self.ownership_operations.insert(
            key,
            SemanticFact::Known(TypedOwnershipOperation { kind, source }),
        )?;
        Ok(())
    }
}
