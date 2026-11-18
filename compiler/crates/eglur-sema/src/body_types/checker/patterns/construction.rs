use super::*;

pub(super) struct PatternConstructorContract {
    pub(super) declaration: CanonicalTarget,
    pub(super) generic_parameters: Vec<GenericParameterId>,
    pub(super) result: TypeId,
    pub(super) shape: PatternConstructorShape,
}

#[derive(Clone, Copy)]
pub(super) enum PatternConstructorShape {
    Absent,
    Tuple(usize),
    Named,
}

impl PatternConstructorShape {
    pub(super) const fn form(self) -> ConstructorPayloadForm {
        match self {
            Self::Absent => ConstructorPayloadForm::Unit,
            Self::Tuple(_) => ConstructorPayloadForm::Positional,
            Self::Named => ConstructorPayloadForm::Named,
        }
    }
}

impl BodyChecker<'_, '_, '_> {
    pub(super) fn pattern_constructor_target(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        pattern: PatternId,
        origin: OriginId,
    ) -> Result<Outcome<(CanonicalTarget, ResolvedTargetKind)>, BodyFlow> {
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
                return Err(BodyFlow::Internal(
                    BodyTypingError::UnexpectedPatternResolution {
                        module,
                        pattern,
                        fact: "local binding",
                    },
                ));
            }
            FactState::Known(ResolvedName::TypeMemberOwner(_)) => {
                return Err(BodyFlow::Internal(
                    BodyTypingError::UnexpectedPatternResolution {
                        module,
                        pattern,
                        fact: "type member owner",
                    },
                ));
            }
            FactState::NotApplicable => {
                return Err(BodyFlow::Internal(
                    BodyTypingError::UnexpectedPatternResolution {
                        module,
                        pattern,
                        fact: "not-applicable name",
                    },
                ));
            }
            FactState::Unperformed => return Ok(Outcome::Unperformed),
        };
        let signature = match self.signatures.declaration(&target) {
            FactState::Known(signature) => signature,
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed => return Ok(Outcome::Unperformed),
            FactState::NotApplicable => {
                return Err(BodyFlow::Internal(
                    BodyTypingError::UnexpectedSignatureKind {
                        target,
                        expected: "pattern constructor target",
                    },
                ));
            }
        };
        let kind = self.resolved_target_kind(&target, signature)?;
        Ok(Outcome::Known((target, kind)))
    }

    pub(super) fn pattern_constructor_contract(
        &mut self,
        target: &CanonicalTarget,
    ) -> Result<PatternConstructorContract, BodyFlow> {
        let (generic_parameters, result, shape) = match self.signatures.declaration(target) {
            FactState::Known(DeclarationSignature::Constructor(constructor)) => (
                constructor.generic_parameters().to_vec(),
                constructor.result(),
                match constructor.payload() {
                    ConstructorPayloadSignature::Unit => PatternConstructorShape::Absent,
                    ConstructorPayloadSignature::Positional(types) => {
                        PatternConstructorShape::Tuple(types.len())
                    }
                },
            ),
            FactState::Known(DeclarationSignature::EnumVariant { payload, .. }) => {
                let CanonicalTarget::Source(variant) = target else {
                    return Err(BodyFlow::Internal(
                        BodyTypingError::UnexpectedSignatureKind {
                            target: target.clone(),
                            expected: "source enum variant",
                        },
                    ));
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
                        return Err(BodyFlow::Internal(
                            BodyTypingError::UnexpectedSignatureKind {
                                target: CanonicalTarget::Source(owner),
                                expected: "enum",
                            },
                        ));
                    }
                };
                let shape = match payload {
                    EnumVariantPayloadSignature::Absent => PatternConstructorShape::Absent,
                    EnumVariantPayloadSignature::Tuple(types) => {
                        PatternConstructorShape::Tuple(types.len())
                    }
                    EnumVariantPayloadSignature::Named(_) => PatternConstructorShape::Named,
                };
                let result =
                    self.nominal_template(CanonicalTarget::Source(owner), &generic_parameters)?;
                (generic_parameters, result, shape)
            }
            _ => {
                return Err(BodyFlow::Internal(
                    BodyTypingError::UnexpectedSignatureKind {
                        target: target.clone(),
                        expected: "variant constructor",
                    },
                ));
            }
        };
        Ok(PatternConstructorContract {
            declaration: target.clone(),
            generic_parameters,
            result,
            shape,
        })
    }

    pub(super) fn pattern_constructor_prefix(
        &self,
        target: &CanonicalTarget,
        explicit: usize,
    ) -> Result<Vec<TypeId>, BodyFlow> {
        let types = match (target, self.signatures.declaration(target)) {
            (_, FactState::Known(DeclarationSignature::Constructor(constructor))) => {
                match constructor.payload() {
                    ConstructorPayloadSignature::Positional(types) => types,
                    ConstructorPayloadSignature::Unit => {
                        return Err(BodyTypingError::UnexpectedSignatureKind {
                            target: target.clone(),
                            expected: "positional variant constructor",
                        }
                        .into());
                    }
                }
            }
            (
                CanonicalTarget::Source(_),
                FactState::Known(DeclarationSignature::EnumVariant {
                    payload: EnumVariantPayloadSignature::Tuple(types),
                    ..
                }),
            ) => types,
            _ => {
                return Err(BodyTypingError::UnexpectedSignatureKind {
                    target: target.clone(),
                    expected: "positional variant constructor",
                }
                .into());
            }
        };
        let types = types[..explicit].to_vec();

        #[cfg(test)]
        super::construction_tests::observe_copy(types.len());

        Ok(types)
    }
}
