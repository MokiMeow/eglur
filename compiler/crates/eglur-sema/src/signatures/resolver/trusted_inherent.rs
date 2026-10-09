use super::*;

impl SignatureResolver<'_, '_> {
    pub(super) fn resolve_trusted_inherent_owners(
        &mut self,
    ) -> Result<(), SignatureResolutionError> {
        let Some(trusted) = self.trusted else {
            return Ok(());
        };
        for owner in trusted.inherent_owners() {
            let mut families = BTreeMap::<NameKey, Vec<InherentMemberSignature>>::new();
            for member in &owner.members {
                let target = CanonicalTarget::External(member.callable.clone());
                let FactState::Known(DeclarationSignature::Callable(id)) =
                    self.declarations.state(&target)
                else {
                    return Err(SignatureResolutionError::MissingTrustedSignature(target));
                };
                let id = *id;
                let pattern = self.resolve_trusted_type(
                    &member.callable,
                    member.owner_parameter_count,
                    &member.owner_pattern,
                    TypePosition::CallableBoundary,
                )?;
                let callable = &self.callables[id.index()];
                let receiver_matches = match member.form {
                    InherentMemberForm::Associated => true,
                    InherentMemberForm::ValueReceiver => callable
                        .parameters()
                        .first()
                        .is_some_and(|parameter| parameter.ty() == pattern),
                    InherentMemberForm::SharedReceiver | InherentMemberForm::MutableReceiver => {
                        let expected = if member.form == InherentMemberForm::SharedReceiver {
                            ReferenceMutability::Shared
                        } else {
                            ReferenceMutability::Mutable
                        };
                        if let Some(parameter) = callable.parameters().first() {
                            matches!(self.types.kind(parameter.ty())?, TypeKind::Reference { mutability, referent } if *mutability == expected && *referent == pattern)
                        } else {
                            false
                        }
                    }
                };
                if !receiver_matches {
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::InvalidInherentMember {
                            callable: member.callable.clone(),
                            reason: "receiver type differs from the complete owner pattern",
                        },
                    ));
                }
                families.entry(member.name.clone()).or_default().push(
                    InherentMemberSignature::new(
                        id,
                        pattern,
                        member.owner_parameter_count,
                        member.form,
                        member.visibility,
                    ),
                );
            }
            self.trusted_inherent_owners.insert(
                owner.head.canonical(),
                InherentMethodInventory::new(
                    families
                        .into_iter()
                        .map(|(name, members)| (name, InherentFamilySignature::new(members)))
                        .collect(),
                ),
            );
        }
        Ok(())
    }
}
