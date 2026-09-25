use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn generic_context_state(
        &self,
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
        let mut poison = None;
        let mut unperformed = false;
        for declaration in chain {
            for generic in self.child_declarations(declaration, DeclarationKind::GenericParameter) {
                match self
                    .generic_parameters
                    .state(&GenericParameterId::Source(generic))
                {
                    FactState::Poisoned(found) => merge_poison(&mut poison, found)?,
                    FactState::Unperformed => unperformed = true,
                    FactState::Known(_) | FactState::NotApplicable => {}
                }
            }
        }
        Ok(if let Some(poison) = poison {
            Resolution::Poisoned(poison)
        } else if unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(())
        })
    }

    pub(super) fn declaration_context_state(
        &self,
        owner: DefId,
    ) -> Result<Resolution<()>, SignatureResolutionError> {
        let mut cursor = self
            .collection
            .declaration(owner)
            .and_then(Declaration::parent);
        let mut poison = None;
        let mut unperformed = false;
        while let Some(current) = cursor {
            match declaration_fact_state(
                &self.declarations,
                &self.overload_family_poisons,
                &CanonicalTarget::Source(current),
            ) {
                FactState::Poisoned(found) => merge_poison(&mut poison, found)?,
                FactState::Unperformed => unperformed = true,
                FactState::Known(_) | FactState::NotApplicable => {}
            }
            cursor = self
                .collection
                .declaration(current)
                .and_then(Declaration::parent);
        }
        Ok(if let Some(poison) = poison {
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
        let mut generics = BTreeMap::new();
        for declaration in &chain {
            for generic in self.child_declarations(*declaration, DeclarationKind::GenericParameter)
            {
                if self.source_generic_redeclarations.contains(&generic) {
                    continue;
                }
                let generic_declaration = self
                    .collection
                    .declaration(generic)
                    .expect("generic child belongs to collection");
                if let Some(name) = generic_declaration.name() {
                    generics
                        .entry(Box::<str>::from(name.text()))
                        .or_insert(GenericParameterId::Source(generic));
                }
            }
        }
        let self_binding = if allow_self {
            self.self_binding(&chain)?
        } else {
            None
        };
        Ok(SourceTypeContext {
            generics,
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
                DeclarationKind::Trait => {
                    let target = CanonicalTarget::Source(candidate);
                    let arguments = self
                        .direct_generic_ids(candidate)
                        .iter()
                        .map(|id| self.generic_types[id])
                        .collect::<Vec<_>>();
                    let substitution = self.types.intern_substitution(arguments)?;
                    let ty = self.types.self_type(target.clone())?;
                    return Ok(Some(SelfBinding {
                        ty,
                        trait_ref: Some(TraitRef::new(target, substitution)),
                    }));
                }
                DeclarationKind::Impl => {
                    return Ok(self.impl_headers.get(&candidate).map(|header| SelfBinding {
                        ty: header.target(),
                        trait_ref: Some(header.trait_ref().clone()),
                    }));
                }
                DeclarationKind::Struct => {
                    let arguments = self
                        .direct_generic_ids(candidate)
                        .iter()
                        .map(|id| self.generic_types[id])
                        .collect::<Vec<_>>();
                    let ty = self.types.nominal(
                        NominalHead::Declaration(CanonicalTarget::Source(candidate)),
                        arguments,
                    )?;
                    return Ok(Some(SelfBinding {
                        ty,
                        trait_ref: None,
                    }));
                }
                _ => {}
            }
        }
        Ok(None)
    }
}
