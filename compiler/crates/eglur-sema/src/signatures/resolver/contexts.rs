use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn generic_context_state(
        &mut self,
        owner: DefId,
    ) -> Result<Resolution<()>, SignatureResolutionError> {
        let mut chain = Vec::new();
        let mut cursor = Some(owner);
        while let Some(current) = cursor {
            chain.push(current);
            cursor = self
                .collection
                .declaration(current)
                .and_then(Declaration::parent);
        }
        let mut poison = PoisonAccumulator::new();
        let mut unperformed = false;
        for declaration in chain {
            match self.direct_generic_state(declaration)? {
                Resolution::Poisoned(found) => poison.include(&found)?,
                Resolution::Unperformed => unperformed = true,
                Resolution::Known(()) => {}
            }
        }
        Ok(if let Some(poison) = poison.finish() {
            Resolution::Poisoned(poison)
        } else if unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(())
        })
    }

    fn direct_generic_state(
        &mut self,
        owner: DefId,
    ) -> Result<Resolution<()>, SignatureResolutionError> {
        #[cfg(test)]
        generic_state_tests::observe(generic_state_tests::Step::Lookup);

        if let Some(state) = self
            .frozen_source_generic_scopes
            .as_ref()
            .and_then(|scopes| scopes.get(&owner))
        {
            #[cfg(test)]
            generic_state_tests::observe(generic_state_tests::Step::Hit);

            return Ok(state.clone());
        }
        #[cfg(test)]
        generic_state_tests::observe(generic_state_tests::Step::Query);

        let generics = self.child_declarations(owner, DeclarationKind::GenericParameter);
        if generics.len() == 0 {
            return Ok(Resolution::Known(()));
        }
        #[cfg(test)]
        generic_state_tests::observe(generic_state_tests::Step::Build);

        let mut poison = PoisonAccumulator::new();
        let mut unperformed = false;
        for generic in generics {
            #[cfg(test)]
            generic_state_tests::observe(generic_state_tests::Step::Read);

            match self
                .generic_parameters
                .state(&GenericParameterId::Source(generic))
            {
                FactState::Poisoned(found) => {
                    #[cfg(test)]
                    generic_state_tests::observe(generic_state_tests::Step::Poisoned);
                    #[cfg(test)]
                    generic_state_tests::observe(generic_state_tests::Step::Include);

                    poison.include(found)?;
                }
                FactState::Unperformed => {
                    #[cfg(test)]
                    generic_state_tests::observe(generic_state_tests::Step::Unperformed);

                    unperformed = true;
                }
                FactState::Known(_) | FactState::NotApplicable => {
                    #[cfg(test)]
                    generic_state_tests::observe(generic_state_tests::Step::Neutral);
                }
            }
        }
        let state = if let Some(poison) = poison.finish() {
            Resolution::Poisoned(poison)
        } else if unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(())
        };
        if let Some(scopes) = &mut self.frozen_source_generic_scopes {
            scopes.insert(owner, state.clone());
            #[cfg(test)]
            generic_state_tests::observe(generic_state_tests::Step::Row(scopes.len()));
        }
        Ok(state)
    }

    pub(super) fn declaration_context_state(
        &self,
        owner: DefId,
    ) -> Result<Resolution<()>, SignatureResolutionError> {
        let mut cursor = self
            .collection
            .declaration(owner)
            .and_then(Declaration::parent);
        let mut poison = PoisonAccumulator::new();
        let mut unperformed = false;
        while let Some(current) = cursor {
            match declaration_fact_state(
                &self.declarations,
                &self.overload_family_poisons,
                &CanonicalTarget::Source(current),
            ) {
                FactState::Poisoned(found) => poison.include(found)?,
                FactState::Unperformed => unperformed = true,
                FactState::Known(_) | FactState::NotApplicable => {}
            }
            cursor = self
                .collection
                .declaration(current)
                .and_then(Declaration::parent);
        }
        Ok(if let Some(poison) = poison.finish() {
            Resolution::Poisoned(poison)
        } else if unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(())
        })
    }

    pub(super) fn source_context(
        &mut self,
        owner: DefId,
        allow_self: bool,
    ) -> Result<SourceTypeContext, SignatureResolutionError> {
        let mut chain = Vec::new();
        let mut cursor = Some(owner);
        while let Some(current) = cursor {
            chain.push(current);
            cursor = self
                .collection
                .declaration(current)
                .and_then(Declaration::parent);
        }
        chain.reverse();
        let self_binding = if allow_self {
            self.self_binding(&chain)?
        } else {
            None
        };
        Ok(SourceTypeContext {
            generics: self.source_generic_names.for_chain(chain),
            self_binding,
            structure_validation: TypeStructureValidation::Immediate,
            application_region: self.application_region(owner),
        })
    }

    fn application_region(
        &self,
        owner: DefId,
    ) -> super::super::applications::TypeApplicationRegion {
        use super::super::applications::TypeApplicationRegion;

        let Some(declaration) = self.collection.declaration(owner) else {
            return TypeApplicationRegion::Declaration;
        };
        match declaration.kind() {
            DeclarationKind::Impl => TypeApplicationRegion::ImplHeader,
            DeclarationKind::AssociatedTypeBinding => TypeApplicationRegion::ImplBinding,
            DeclarationKind::Function
                if declaration.parent().is_some_and(|parent| {
                    self.collection
                        .declaration(parent)
                        .is_some_and(|parent| parent.kind() == DeclarationKind::Impl)
                }) =>
            {
                TypeApplicationRegion::ImplMethodSignature
            }
            _ => TypeApplicationRegion::Declaration,
        }
    }

    pub(super) fn self_binding(
        &mut self,
        chain: &[DefId],
    ) -> Result<Option<SelfBinding>, SignatureResolutionError> {
        for &candidate in chain.iter().rev() {
            let declaration = self
                .collection
                .declaration(candidate)
                .expect("context declaration belongs to collection");
            match declaration.kind() {
                DeclarationKind::Trait | DeclarationKind::Struct => {
                    #[cfg(test)]
                    tests::observe_lookup();

                    if let Some(binding) = self.static_self_bindings.get(&candidate) {
                        #[cfg(test)]
                        tests::observe_hit();

                        return Ok(Some(binding.clone()));
                    }
                    #[cfg(test)]
                    tests::observe_build();
                    #[cfg(test)]
                    tests::observe_query();

                    let arguments = self
                        .direct_generic_ids(candidate)
                        .iter()
                        .map(|id| {
                            #[cfg(test)]
                            tests::observe_argument();

                            self.generic_types[id]
                        })
                        .collect::<Vec<_>>();
                    #[cfg(test)]
                    tests::observe_prepared(arguments.len());

                    let binding = if declaration.kind() == DeclarationKind::Trait {
                        let target = CanonicalTarget::Source(candidate);
                        #[cfg(test)]
                        tests::observe_trait_substitution();

                        let substitution = self.types.intern_substitution(arguments)?;
                        #[cfg(test)]
                        tests::observe_self_type();

                        let ty = self.types.self_type(target.clone())?;
                        SelfBinding {
                            ty,
                            trait_ref: Some(TraitRef::new(target, substitution)),
                        }
                    } else {
                        #[cfg(test)]
                        tests::observe_nominal();

                        let ty = self.types.nominal(
                            NominalHead::Declaration(CanonicalTarget::Source(candidate)),
                            arguments,
                        )?;
                        SelfBinding {
                            ty,
                            trait_ref: None,
                        }
                    };
                    self.static_self_bindings.insert(candidate, binding.clone());
                    #[cfg(test)]
                    tests::observe_row(self.static_self_bindings.len());

                    return Ok(Some(binding));
                }
                DeclarationKind::Impl => {
                    return Ok(self.impl_headers.get(&candidate).map(|header| SelfBinding {
                        ty: header.target(),
                        trait_ref: Some(header.trait_ref().clone()),
                    }));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod generic_state_tests;
