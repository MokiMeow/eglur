use super::*;

mod cycles;
mod dependencies;

use dependencies::BindingDependencies;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn resolve_source_impl_bindings(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        value: &eglur_hir::ImplDecl,
        trait_ref: &TraitRef,
        target: TypeId,
    ) -> Result<Resolution<Vec<AssociatedBindingSignature>>, SignatureResolutionError> {
        let info = self
            .trait_info
            .get(trait_ref.declaration())
            .cloned()
            .expect("resolved trait ref has registered trait info");
        let mut expected_by_target = BTreeMap::new();
        for (name, targets) in &info.associated {
            for target in targets {
                expected_by_target.insert(target.clone(), name.clone());
            }
        }
        let mut seen = BTreeSet::new();
        let mut supplied = Vec::new();
        let mut poison = None;
        let module = self
            .collection
            .module(declaration_id.module())
            .expect("impl module belongs to collection");
        for &item_id in &value.items {
            let ItemKind::AssociatedTypeBinding(binding) = hir.item(item_id).kind() else {
                continue;
            };
            let binding_id = module
                .item_def(item_id)
                .expect("collected associated binding has declaration identity");
            let Some(name) = binding.name.text() else {
                let found = source
                    .invalid_poison(binding.name.origin())
                    .cloned()
                    .ok_or(SignatureResolutionError::MissingInvalidPoison {
                        file: source.file(),
                        origin: binding.name.origin(),
                    })?;
                self.declarations.insert(
                    CanonicalTarget::Source(binding_id),
                    SemanticFact::Poisoned(found.clone()),
                )?;
                merge_poison(&mut poison, &found)?;
                continue;
            };
            let candidates = info.associated.get(name).map_or(&[][..], AsRef::as_ref);
            let reason = if candidates.len() != 1 {
                Some("unknown")
            } else if !seen.insert(candidates[0].clone()) {
                Some("duplicate")
            } else {
                None
            };
            if let Some(reason) = reason {
                let found = self.associated_mismatch_error(
                    source,
                    binding.name.origin(),
                    binding_id,
                    trait_ref,
                    name,
                    reason,
                )?;
                self.declarations.insert(
                    CanonicalTarget::Source(binding_id),
                    SemanticFact::Poisoned(found.clone()),
                )?;
                merge_poison(&mut poison, &found)?;
                continue;
            }
            supplied.push((binding_id, binding.value, candidates[0].clone()));
        }
        for (associated, name) in expected_by_target {
            if seen.contains(&associated) {
                continue;
            }
            let declaration = self
                .collection
                .declaration(declaration_id)
                .expect("impl belongs to collection");
            let found = self.associated_mismatch_error(
                source,
                declaration.origin(),
                declaration_id,
                trait_ref,
                &name,
                "missing",
            )?;
            merge_poison(&mut poison, &found)?;
        }
        if let Some(found) = poison {
            for (binding_id, _, _) in supplied {
                self.declarations.insert(
                    CanonicalTarget::Source(binding_id),
                    SemanticFact::Poisoned(found.clone()),
                )?;
            }
            return Ok(Resolution::Poisoned(found));
        }

        let mut pending_bindings = Vec::new();
        let mut unperformed = false;
        for (binding_id, type_ref, associated) in supplied {
            let mut context = self.source_context(binding_id, true)?;
            context.structure_validation = TypeStructureValidation::AfterBindingCycles;
            match self.resolve_source_type(
                binding_id,
                source,
                hir,
                type_ref,
                TypePosition::Ordinary,
                &context,
            )? {
                Resolution::Known(ty) => {
                    pending_bindings.push((
                        AssociatedBindingSignature {
                            declaration: CanonicalTarget::Source(binding_id),
                            associated,
                            value: ty,
                        },
                        type_ref,
                    ));
                }
                Resolution::Poisoned(found) => {
                    self.declarations.insert(
                        CanonicalTarget::Source(binding_id),
                        SemanticFact::Poisoned(found.clone()),
                    )?;
                    merge_poison(&mut poison, &found)?;
                }
                Resolution::Unperformed => unperformed = true,
            }
        }
        let cycle =
            self.associated_binding_cycle(source, hir, trait_ref, target, &pending_bindings)?;
        let cycle_affected = if let Some((found, affected)) = cycle {
            merge_poison(&mut poison, &found)?;
            Some((found, affected))
        } else {
            None
        };
        let mut bindings = Vec::with_capacity(pending_bindings.len());
        for (signature, type_ref) in pending_bindings {
            let fact = if let Some((found, affected)) = &cycle_affected
                && affected.contains(&signature.associated)
            {
                SemanticFact::Poisoned(found.clone())
            } else if let Some((origin, code, message)) =
                self.source_binding_structure_error(source, hir, type_ref)?
            {
                let found = self.source_error(source, origin, code, message, BTreeMap::new())?;
                merge_poison(&mut poison, &found)?;
                SemanticFact::Poisoned(found)
            } else {
                bindings.push(signature.clone());
                SemanticFact::Known(DeclarationSignature::AssociatedTypeBinding(
                    signature.clone(),
                ))
            };
            self.declarations
                .insert(signature.declaration.clone(), fact)?;
        }
        Ok(if let Some(poison) = poison {
            Resolution::Poisoned(poison)
        } else if unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(bindings)
        })
    }

    fn source_binding_structure_error(
        &self,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        root: TypeRefId,
    ) -> Result<Option<(OriginId, &'static str, &'static str)>, SignatureResolutionError> {
        let mut pending = vec![root];
        let mut reference = None;
        while let Some(type_ref) = pending.pop() {
            let occurrence = hir.type_ref(type_ref);
            if let FactState::Known(resolved) = self
                .occurrences
                .state(&TypeOccurrenceId::new(source.file(), type_ref))
                && let Some(ty) = resolved.canonical_type()
            {
                if self.types.contains_ownership(ty)? {
                    return Ok(Some((
                        occurrence.origin(),
                        "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                        "Own type position is forbidden in an associated type binding",
                    )));
                }
                if reference.is_none() && self.types.contains_reference(ty)? {
                    reference = Some((
                        occurrence.origin(),
                        "REFERENCE_TYPE_POSITION_FORBIDDEN",
                        "reference type position is forbidden for an associated type binding",
                    ));
                }
            }
            // Written arguments still have slot obligations when an alias erases them.
            match occurrence.kind() {
                TypeRefKind::Path { arguments, .. } | TypeRefKind::Tuple(arguments) => {
                    pending.extend(arguments.iter().rev().copied());
                }
                TypeRefKind::Function { parameters, result } => {
                    pending.push(*result);
                    pending.extend(parameters.iter().rev().copied());
                }
                TypeRefKind::Reference { referent, .. } | TypeRefKind::Pointer { referent, .. } => {
                    pending.push(*referent)
                }
                TypeRefKind::Group(inner) => pending.push(*inner),
                TypeRefKind::Dynamic { trait_ref } => pending.push(*trait_ref),
                TypeRefKind::Invalid => {}
            }
        }
        Ok(reference)
    }

    fn associated_binding_cycle(
        &mut self,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        trait_ref: &TraitRef,
        target: TypeId,
        bindings: &[(AssociatedBindingSignature, TypeRefId)],
    ) -> Result<Option<(Poison, BTreeSet<CanonicalTarget>)>, SignatureResolutionError> {
        let signatures = bindings
            .iter()
            .map(|(binding, _)| binding.clone())
            .collect::<Vec<_>>();
        let Some(cycles) =
            BindingDependencies::new(&self.types, trait_ref, target, &signatures)?.cycle_sets()
        else {
            return Ok(None);
        };
        let (_, type_ref) = bindings
            .iter()
            .find(|(binding, _)| cycles.cyclic.contains(&binding.associated))
            .expect("cyclic associated target has a source binding");
        let poison = self.source_error(
            source,
            hir.type_ref(*type_ref).origin(),
            "ASSOCIATED_TYPE_CYCLE",
            "associated type binding contains a cycle",
            BTreeMap::new(),
        )?;
        Ok(Some((poison, cycles.affected)))
    }

    pub(super) fn resolve_trusted_impl_bindings(
        &mut self,
        owner: &ExternalDefId,
        generic_count: usize,
        trait_ref: &TraitRef,
        target: TypeId,
        supplied: &[TrustedAssociatedBinding],
    ) -> Result<Vec<AssociatedBindingSignature>, SignatureResolutionError> {
        let info = self
            .trait_info
            .get(trait_ref.declaration())
            .expect("trusted impl header has a resolved trait");
        let expected = info
            .associated
            .values()
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut seen = BTreeSet::new();
        for binding in supplied {
            let associated = CanonicalTarget::External(binding.associated.clone());
            if !expected.contains(&associated) || !seen.insert(associated) {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::UnknownAssociatedType(binding.associated.clone()),
                ));
            }
        }
        if let Some(CanonicalTarget::External(missing)) = expected.difference(&seen).next() {
            return Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::UnknownAssociatedType(missing.clone()),
            ));
        }

        let mut bindings = Vec::with_capacity(supplied.len());
        for binding in supplied {
            let value = self.resolve_trusted_type_with_validation(
                owner,
                generic_count,
                &binding.value,
                TypePosition::Ordinary,
                TypeStructureValidation::AfterBindingCycles,
            )?;
            bindings.push(AssociatedBindingSignature {
                declaration: CanonicalTarget::External(binding.declaration.clone()),
                associated: CanonicalTarget::External(binding.associated.clone()),
                value,
            });
        }
        if BindingDependencies::new(&self.types, trait_ref, target, &bindings)?
            .cycle_sets()
            .is_some()
        {
            return Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::InvalidTypePosition {
                    declaration: owner.clone(),
                    reason: "associated type binding cycle",
                },
            ));
        }
        for (binding, input) in bindings.iter().zip(supplied) {
            if self.types.contains_ownership(binding.value)? {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: owner.clone(),
                        reason: "ownership type outside a complete callable boundary",
                    },
                ));
            }
            self.resolve_trusted_type(owner, generic_count, &input.value, TypePosition::Ordinary)?;
            if self.types.contains_reference(binding.value)? {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: owner.clone(),
                        reason: "reference-bearing associated type binding",
                    },
                ));
            }
        }
        for binding in &bindings {
            self.declarations.insert(
                binding.declaration.clone(),
                SemanticFact::Known(DeclarationSignature::AssociatedTypeBinding(binding.clone())),
            )?;
        }
        Ok(bindings)
    }
}
