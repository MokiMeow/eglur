use super::*;

#[cfg(test)]
mod poison_tests;

impl SignatureResolver<'_, '_> {
    pub(super) fn resolve_trusted_enum(
        &mut self,
        owner: &ExternalDefId,
        generic_parameters: &[GenericParameterId],
        variants: &[TrustedEnumVariant],
    ) -> Result<DeclarationSignature, SignatureResolutionError> {
        let owner_target = CanonicalTarget::External(owner.clone());
        let mut members = Vec::with_capacity(variants.len());
        for variant in variants {
            let payload = match &variant.payload {
                TrustedEnumPayload::Absent => EnumVariantPayloadSignature::Absent,
                TrustedEnumPayload::Tuple(types) => {
                    let types = types
                        .iter()
                        .map(|ty| {
                            self.resolve_trusted_type(
                                owner,
                                generic_parameters.len(),
                                ty,
                                TypePosition::Ordinary,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    EnumVariantPayloadSignature::Tuple(types.into_boxed_slice())
                }
                TrustedEnumPayload::Named(fields) => {
                    let mut members = Vec::with_capacity(fields.len());
                    for field in fields {
                        let ty = self.resolve_trusted_type(
                            owner,
                            generic_parameters.len(),
                            &field.ty,
                            TypePosition::Ordinary,
                        )?;
                        let target = CanonicalTarget::External(field.id.clone());
                        self.declarations.insert(
                            target.clone(),
                            SemanticFact::Known(DeclarationSignature::Field {
                                name: field.name.clone(),
                                ty,
                            }),
                        )?;
                        members.push(target);
                    }
                    EnumVariantPayloadSignature::Named(members.into_boxed_slice())
                }
            };
            let target = CanonicalTarget::External(variant.id.clone());
            self.declarations.insert(
                target.clone(),
                SemanticFact::Known(DeclarationSignature::EnumVariant {
                    owner: owner_target.clone(),
                    name: variant.name.clone(),
                    payload,
                }),
            )?;
            members.push(target);
        }
        Ok(DeclarationSignature::Enum {
            generic_parameters: generic_parameters.to_vec().into_boxed_slice(),
            variants: members.into_boxed_slice(),
        })
    }

    pub(super) fn index_enum_variant_names(&mut self) -> Result<(), SignatureResolutionError> {
        for (target, fact) in self.declarations.iter() {
            let signature = match fact {
                SemanticFact::Known(signature) => signature,
                SemanticFact::Poisoned(poison) => {
                    self.enum_variant_names
                        .insert(target.clone(), SemanticFact::Poisoned(poison.clone()));
                    continue;
                }
                SemanticFact::NotApplicable => continue,
            };
            let DeclarationSignature::Enum { variants, .. } = signature else {
                self.enum_variant_names
                    .insert(target.clone(), SemanticFact::NotApplicable);
                continue;
            };

            let mut names = HashSet::with_capacity(variants.len());
            let mut poison = PoisonAccumulator::new();
            let mut unperformed = false;
            for variant in variants {
                match self.declarations.state(variant) {
                    FactState::Known(DeclarationSignature::EnumVariant { name, .. }) => {
                        names.insert(name.clone());
                    }
                    FactState::Poisoned(found) => {
                        #[cfg(test)]
                        poison_tests::observe(poison_tests::Step::Input);

                        poison.include(found)?;
                    }
                    FactState::Unperformed | FactState::NotApplicable => unperformed = true,
                    FactState::Known(_) => unreachable!("enum members are variant declarations"),
                }
            }
            let fact = if let Some(poison) = poison.finish() {
                #[cfg(test)]
                poison_tests::observe(poison_tests::Step::Complete);

                SemanticFact::Poisoned(poison)
            } else if unperformed {
                continue;
            } else {
                SemanticFact::Known(names)
            };
            self.enum_variant_names.insert(target.clone(), fact);
        }
        Ok(())
    }
}
