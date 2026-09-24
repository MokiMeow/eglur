use super::*;
use crate::signatures::public_contracts::{
    conformance_is_externally_reachable, first_unnameable_trait, first_unnameable_type,
};

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn resolve_source_generics(&mut self) -> Result<(), SignatureResolutionError> {
        let generic_ids = self
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .filter(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
            .map(Declaration::id)
            .collect::<Vec<_>>();
        self.prime_source_generic_bounds(&generic_ids)?;
        for declaration_id in generic_ids {
            self.resolve_source_generic(declaration_id)?;
        }
        Ok(())
    }

    pub(super) fn prime_source_impl_headers(&mut self) -> Result<(), SignatureResolutionError> {
        let implementations = self
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .filter(|declaration| declaration.kind() == DeclarationKind::Impl)
            .map(Declaration::id)
            .collect::<Vec<_>>();
        for declaration_id in implementations {
            let declaration = self
                .collection
                .declaration(declaration_id)
                .expect("impl belongs to collection");
            let module = self
                .collection
                .module(declaration_id.module())
                .expect("impl module belongs to collection");
            if let Some(poison) = module.owner_poison(declaration_id) {
                self.impl_header_poison
                    .insert(declaration_id, poison.clone());
                continue;
            }
            match self.generic_context_state(declaration_id)? {
                Resolution::Known(()) => {}
                Resolution::Poisoned(poison) => {
                    self.impl_header_poison.insert(declaration_id, poison);
                    continue;
                }
                Resolution::Unperformed => continue,
            }
            let source = self.sources[&module.file()];
            let hir = self.source_hir(module.file())?;
            let item =
                declaration
                    .item()
                    .ok_or(SignatureResolutionError::MissingDeclarationItem(
                        declaration_id,
                    ))?;
            let ItemKind::Impl(value) = hir.item(item).kind() else {
                return Err(SignatureResolutionError::WrongDeclarationItem(
                    declaration_id,
                ));
            };
            match self.resolve_source_impl_header(declaration_id, source, hir, value)? {
                Resolution::Known(header) => {
                    self.impl_headers.insert(declaration_id, header);
                }
                Resolution::Poisoned(poison) => {
                    self.impl_header_poison.insert(declaration_id, poison);
                }
                Resolution::Unperformed => {}
            }
        }
        Ok(())
    }

    pub(super) fn resolve_source_impl_header(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        value: &eglur_hir::ImplDecl,
    ) -> Result<Resolution<TraitImplHeader>, SignatureResolutionError> {
        let mut poison = None;
        let generic_parameters = self.direct_generic_ids(declaration_id);
        if value.negative {
            let declaration = self
                .collection
                .declaration(declaration_id)
                .expect("impl belongs to collection");
            poison = Some(self.source_error(
                source,
                declaration.origin(),
                "NEGATIVE_IMPL_UNSUPPORTED",
                "negative impl is unsupported in v0.1",
                BTreeMap::new(),
            )?);
        }
        let context = self.source_context(declaration_id, false)?;
        let trait_ref =
            self.resolve_source_trait_ref(declaration_id, source, hir, value.trait_ref, &context)?;
        let target = self.resolve_source_type(
            declaration_id,
            source,
            hir,
            value.target,
            TypePosition::Ordinary,
            &context,
        )?;
        let mut unperformed = false;
        let trait_ref = match trait_ref {
            Resolution::Known(trait_ref) => Some(trait_ref),
            Resolution::Poisoned(found) => {
                merge_poison(&mut poison, &found)?;
                None
            }
            Resolution::Unperformed => {
                unperformed = true;
                None
            }
        };
        let target = match target {
            Resolution::Known(target) => Some(target),
            Resolution::Poisoned(found) => {
                merge_poison(&mut poison, &found)?;
                None
            }
            Resolution::Unperformed => {
                unperformed = true;
                None
            }
        };
        if let (Some(trait_ref), Some(target)) = (&trait_ref, target) {
            let trait_has_projection = self
                .types
                .substitution(trait_ref.substitution())?
                .iter()
                .copied()
                .map(|argument| self.types.contains_associated_projection(argument))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|found| found);
            if trait_has_projection || self.types.contains_associated_projection(target)? {
                let found = self.projection_error(
                    source,
                    hir.type_ref(value.trait_ref).origin(),
                    "projection in impl header",
                )?;
                merge_poison(&mut poison, &found)?;
            }
            let header = TraitImplHeader {
                declaration: CanonicalTarget::Source(declaration_id),
                generic_parameters: generic_parameters.clone(),
                trait_ref: trait_ref.clone(),
                target,
            };
            if let Some(parameter) = header_parameter_below_error_union(&self.types, &header)? {
                let parameter_name = match self.generic_parameters.state(&parameter) {
                    FactState::Known(signature) => signature.name().to_owned(),
                    FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                        unreachable!("performed impl header has known generic parameters")
                    }
                };
                let origin = self
                    .collection
                    .declaration(declaration_id)
                    .expect("impl belongs to collection")
                    .origin();
                let found = self.source_error(
                    source,
                    origin,
                    "IMPL_GENERIC_ERROR_UNION_UNSUPPORTED",
                    format!(
                        "impl generic parameter `{parameter_name}` is unsupported inside an error union"
                    ),
                    BTreeMap::from([(
                        Box::<str>::from("parameter"),
                        DiagnosticDataValue::from(parameter_name),
                    )]),
                )?;
                merge_poison(&mut poison, &found)?;
            }
        }
        if let Some(poison) = poison {
            return Ok(Resolution::Poisoned(poison));
        }
        if unperformed {
            return Ok(Resolution::Unperformed);
        }
        Ok(Resolution::Known(TraitImplHeader {
            declaration: CanonicalTarget::Source(declaration_id),
            generic_parameters,
            trait_ref: trait_ref.expect("performed impl has trait ref"),
            target: target.expect("performed impl has target"),
        }))
    }

    pub(super) fn validate_source_impl_headers(&mut self) -> Result<(), SignatureResolutionError> {
        let mut accepted = self
            .retained_impl_headers
            .iter()
            .filter_map(|(target, fact)| {
                matches!(target, CanonicalTarget::External(_))
                    .then_some(fact)
                    .and_then(|fact| match fact {
                        SemanticFact::Known(header) => Some(header.clone()),
                        SemanticFact::Poisoned(_) | SemanticFact::NotApplicable => None,
                    })
            })
            .collect::<Vec<_>>();

        let implementations = self
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .filter(|declaration| declaration.kind() == DeclarationKind::Impl)
            .map(Declaration::id)
            .collect::<Vec<_>>();
        for declaration_id in implementations {
            let target = CanonicalTarget::Source(declaration_id);
            if let Some(poison) = self.impl_header_poison.get(&declaration_id) {
                self.retained_impl_headers
                    .insert(target, SemanticFact::Poisoned(poison.clone()))?;
                continue;
            }
            let Some(header) = self.impl_headers.get(&declaration_id).cloned() else {
                continue;
            };
            if let Some((code, message)) = self.source_impl_authority_violation(&header)? {
                let poison = self.source_impl_header_error(declaration_id, code, message, None)?;
                self.reject_source_impl_header(declaration_id, poison)?;
                continue;
            }
            let conflict = accepted
                .iter()
                .find_map(|candidate| {
                    classify_overlap(self, candidate, &header)
                        .transpose()
                        .map(|conflict| conflict.map(|kind| (candidate.clone(), kind)))
                })
                .transpose()?;
            if let Some((previous, kind)) = conflict {
                let (code, message) = match kind {
                    HeaderConflict::Duplicate => (
                        "TRAIT_IMPL_DUPLICATE",
                        "trait impl is a duplicate of an existing impl",
                    ),
                    HeaderConflict::Overlap => {
                        ("TRAIT_IMPL_OVERLAP", "trait impl overlaps an existing impl")
                    }
                    HeaderConflict::Specialization => (
                        "SPECIALIZATION_FORBIDDEN",
                        "trait impl specialization is forbidden",
                    ),
                };
                let related = match previous.declaration() {
                    CanonicalTarget::Source(previous) => Some(*previous),
                    CanonicalTarget::External(_) => None,
                };
                let poison =
                    self.source_impl_header_error(declaration_id, code, message, related)?;
                self.reject_source_impl_header(declaration_id, poison)?;
                continue;
            }
            let mut unconstrained = false;
            for parameter in header.generic_parameters() {
                if !header_mentions_parameter(&self.types, &header, parameter)? {
                    unconstrained = true;
                    break;
                }
            }
            if unconstrained {
                let poison = self.source_impl_header_error(
                    declaration_id,
                    "IMPL_UNCONSTRAINED_TYPE_PARAM",
                    "impl type parameter is unconstrained by its trait or target type",
                    None,
                )?;
                self.reject_source_impl_header(declaration_id, poison)?;
                continue;
            }
            accepted.push(header.clone());
            if self.public_impl_header_exposes_private_type(&header)? {
                let poison = self.source_impl_header_error(
                    declaration_id,
                    "VISIBILITY_VIOLATION",
                    "visibility violation: public conformance exposes a private declaration",
                    None,
                )?;
                self.reject_source_impl_header(declaration_id, poison)?;
                continue;
            }
            self.retained_impl_headers
                .insert(target, SemanticFact::Known(header))?;
        }
        Ok(())
    }

    fn public_impl_header_exposes_private_type(
        &self,
        header: &TraitImplHeader,
    ) -> Result<bool, SignatureResolutionError> {
        if !conformance_is_externally_reachable(self.collection, &self.types, header)? {
            return Ok(false);
        }
        for parameter in header.generic_parameters() {
            let FactState::Known(signature) = self.generic_parameters.state(parameter) else {
                unreachable!("checked impl header has known generic parameters")
            };
            for bound in signature.declared_bounds() {
                if first_unnameable_trait(self.collection, &self.types, bound.clone())?.is_some() {
                    return Ok(true);
                }
            }
        }
        Ok(
            first_unnameable_trait(self.collection, &self.types, header.trait_ref().clone())?
                .is_some()
                || first_unnameable_type(self.collection, &self.types, header.target())?.is_some(),
        )
    }

    pub(super) fn reject_source_impl_header(
        &mut self,
        declaration_id: DefId,
        poison: Poison,
    ) -> Result<(), SignatureResolutionError> {
        self.impl_headers.remove(&declaration_id);
        self.impl_header_poison
            .insert(declaration_id, poison.clone());
        self.retained_impl_headers.insert(
            CanonicalTarget::Source(declaration_id),
            SemanticFact::Poisoned(poison),
        )?;
        Ok(())
    }

    pub(super) fn source_impl_authority_violation(
        &self,
        header: &TraitImplHeader,
    ) -> Result<Option<(&'static str, &'static str)>, SignatureResolutionError> {
        let trait_info = self
            .trait_info
            .get(header.trait_ref().declaration())
            .ok_or_else(|| {
                SignatureResolutionError::MissingTrustedSignature(
                    header.trait_ref().declaration().clone(),
                )
            })?;
        if trait_info.closed
            && matches!(
                header.trait_ref().declaration(),
                CanonicalTarget::External(_)
            )
        {
            return Ok(Some((
                "CLOSED_TRAIT_IMPL_FORBIDDEN",
                "closed trait impl is forbidden outside its declaring package",
            )));
        }
        if matches!(
            header.trait_ref().declaration(),
            CanonicalTarget::External(_)
        ) && !matches!(
            self.types.kind(header.target())?,
            TypeKind::Nominal {
                head: NominalHead::Declaration(CanonicalTarget::Source(_)),
                ..
            }
        ) {
            return Ok(Some((
                "ORPHAN_RULE_VIOLATION",
                "orphan impl requires a local trait or local nominal target type",
            )));
        }
        Ok(None)
    }

    pub(super) fn source_impl_header_error(
        &mut self,
        declaration_id: DefId,
        code: &'static str,
        message: &'static str,
        related: Option<DefId>,
    ) -> Result<Poison, SignatureResolutionError> {
        let declaration = self
            .collection
            .declaration(declaration_id)
            .expect("impl belongs to collection");
        let module = self
            .collection
            .module(declaration_id.module())
            .expect("impl module belongs to collection");
        let source = self.sources[&module.file()];
        let span = self.required_span(source, declaration.origin())?;
        if let Some(related) = related {
            let declaration = self
                .collection
                .declaration(related)
                .expect("related impl belongs to collection");
            let module = self
                .collection
                .module(related.module())
                .expect("related impl module belongs to collection");
            let source = self.sources[&module.file()];
            let related = RelatedSpan::new(
                self.required_span(source, declaration.origin())?,
                "conflicting impl declared here",
            )
            .expect("static related-span label is non-empty");
            Ok(record_type_error_with_related(
                self.diagnostics,
                code,
                message,
                span,
                [related],
                BTreeMap::new(),
            )?)
        } else {
            Ok(record_type_error(
                self.diagnostics,
                code,
                message,
                span,
                BTreeMap::new(),
            )?)
        }
    }

    pub(super) fn classify_source_generic_redeclarations(&self) -> BTreeSet<DefId> {
        let mut by_owner = BTreeMap::<DefId, Vec<(DefId, Box<str>)>>::new();
        for declaration in self
            .collection
            .modules()
            .flat_map(|module| module.declarations())
        {
            if declaration.kind() != DeclarationKind::GenericParameter {
                continue;
            }
            let (Some(owner), Some(name)) = (declaration.parent(), declaration.name()) else {
                continue;
            };
            if is_reserved_type_name(name.text()) {
                continue;
            }
            by_owner
                .entry(owner)
                .or_default()
                .push((declaration.id(), Box::<str>::from(name.text())));
        }

        let mut redeclarations = BTreeSet::new();
        for (&owner, direct_parameters) in &by_owner {
            let mut enclosing_names = BTreeSet::new();
            let mut ancestor = self
                .collection
                .declaration(owner)
                .and_then(Declaration::parent);
            while let Some(current) = ancestor {
                if let Some(parameters) = by_owner.get(&current) {
                    enclosing_names.extend(parameters.iter().map(|(_, name)| name.clone()));
                }
                ancestor = self
                    .collection
                    .declaration(current)
                    .and_then(Declaration::parent);
            }

            let mut direct_names = BTreeSet::new();
            for (generic, name) in direct_parameters {
                if !direct_names.insert(name.clone()) || enclosing_names.contains(name.as_ref()) {
                    redeclarations.insert(*generic);
                }
            }
        }

        redeclarations
    }

    pub(super) fn prime_source_generic_bounds(
        &mut self,
        generic_ids: &[DefId],
    ) -> Result<(), SignatureResolutionError> {
        for &declaration_id in generic_ids {
            if self.source_generic_redeclarations.contains(&declaration_id) {
                continue;
            }
            let declaration = self
                .collection
                .declaration(declaration_id)
                .expect("generic declaration belongs to collection");
            let owner =
                declaration
                    .parent()
                    .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                        declaration_id,
                    ))?;
            let (file, parameter) = self.source_generic_parameter(declaration_id)?;
            let parameter = parameter.clone();
            if !parameter.name.is_valid()
                || parameter.name.text().is_some_and(is_reserved_type_name)
            {
                continue;
            }

            let source = self.sources[&file];
            let hir = self.source_hir(file)?;
            let context = self.source_context(owner, false)?;
            let parameter_id = GenericParameterId::Source(declaration_id);
            let mut available = BTreeSet::new();
            self.generic_bounds
                .insert(parameter_id.clone(), Box::new([]));
            for &bound_id in &parameter.bounds {
                if type_ref_contains_generic_projection(hir, bound_id, &context.generics) {
                    continue;
                }
                let result =
                    self.resolve_source_trait_ref(owner, source, hir, bound_id, &context)?;
                if let Resolution::Known(bound) = &result {
                    available.insert(bound.clone());
                    self.generic_bounds.insert(
                        parameter_id.clone(),
                        available
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                }
                self.source_generic_bound_seeds
                    .insert((declaration_id, bound_id), result);
            }
        }
        Ok(())
    }

    pub(super) fn resolve_source_generic(
        &mut self,
        declaration_id: DefId,
    ) -> Result<Resolution<()>, SignatureResolutionError> {
        if let Some(state) = self.source_generic_states.get(&declaration_id) {
            return Ok(match state {
                SourceGenericState::Resolving | SourceGenericState::Unperformed => {
                    Resolution::Unperformed
                }
                SourceGenericState::Resolved => Resolution::Known(()),
                SourceGenericState::Poisoned(poison) => Resolution::Poisoned(poison.clone()),
            });
        }
        self.source_generic_states
            .insert(declaration_id, SourceGenericState::Resolving);
        self.source_generic_stack.push(declaration_id);

        let declaration = self
            .collection
            .declaration(declaration_id)
            .expect("generic declaration belongs to collection");
        let owner =
            declaration
                .parent()
                .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                    declaration_id,
                ))?;
        let (file, parameter) = self.source_generic_parameter(declaration_id)?;
        let parameter = parameter.clone();
        let source = self.sources[&file];
        let hir = self.source_hir(file)?;
        let id = GenericParameterId::Source(declaration_id);
        let result = if !parameter.name.is_valid() {
            let poison = source
                .invalid_poison(parameter.name.origin())
                .cloned()
                .ok_or(SignatureResolutionError::MissingInvalidPoison {
                    file,
                    origin: parameter.name.origin(),
                })?;
            Resolution::Poisoned(poison)
        } else if parameter.name.text().is_some_and(is_reserved_type_name) {
            let name = parameter.name.text().expect("valid name has text");
            Resolution::Poisoned(self.source_error(
                source,
                parameter.name.origin(),
                "RESERVED_BUILTIN_NAME",
                format!("reserved built-in name `{name}` cannot be declared"),
                BTreeMap::new(),
            )?)
        } else if self.source_generic_redeclarations.contains(&declaration_id) {
            let name = parameter.name.text().expect("valid generic name has text");
            Resolution::Poisoned(self.source_error(
                source,
                parameter.name.origin(),
                "GENERIC_PARAMETER_REDECLARATION",
                format!("generic parameter `{name}` redeclares an in-scope generic parameter"),
                BTreeMap::from([(Box::<str>::from("name"), DiagnosticDataValue::from(name))]),
            )?)
        } else {
            let context = self.source_context(owner, false)?;
            let mut bounds = Vec::new();
            let mut poison = None;
            let mut unperformed = false;
            let mut seen = BTreeSet::new();
            let parameter_name = parameter.name.text().unwrap_or("<invalid>");
            for &bound_id in &parameter.bounds {
                let bound = match self
                    .source_generic_bound_seeds
                    .get(&(declaration_id, bound_id))
                {
                    Some(bound) => bound.clone(),
                    None => {
                        self.resolve_source_trait_ref(owner, source, hir, bound_id, &context)?
                    }
                };
                match bound {
                    Resolution::Known(bound) => {
                        if !seen.insert(bound.clone()) {
                            let duplicate = self.source_error(
                                source,
                                hir.type_ref(bound_id).origin(),
                                "TRAIT_BOUND_INVALID",
                                "trait bound is invalid because it is repeated",
                                BTreeMap::from([
                                    (
                                        Box::<str>::from("parameter"),
                                        DiagnosticDataValue::from(parameter_name),
                                    ),
                                    (
                                        Box::<str>::from("reason"),
                                        DiagnosticDataValue::from("duplicate"),
                                    ),
                                ]),
                            )?;
                            merge_poison(&mut poison, &duplicate)?;
                        } else {
                            bounds.push(bound);
                            let mut available = bounds.clone();
                            available.sort();
                            self.generic_bounds
                                .insert(id.clone(), available.into_boxed_slice());
                        }
                    }
                    Resolution::Poisoned(found) => merge_poison(&mut poison, &found)?,
                    Resolution::Unperformed => unperformed = true,
                }
            }
            if let Some(poison) = poison {
                Resolution::Poisoned(poison)
            } else if unperformed {
                Resolution::Unperformed
            } else {
                Resolution::Known(GenericParameterSignature::new(
                    id.clone(),
                    parameter.name.text().unwrap_or("<invalid>").into(),
                    bounds,
                ))
            }
        };

        let state = match result {
            Resolution::Known(signature) => {
                self.generic_bounds
                    .insert(id.clone(), signature.bounds().into());
                self.generic_parameters
                    .insert(id, SemanticFact::Known(signature.clone()))?;
                self.declarations.insert(
                    CanonicalTarget::Source(declaration_id),
                    SemanticFact::Known(DeclarationSignature::GenericParameter(signature)),
                )?;
                SourceGenericState::Resolved
            }
            Resolution::Poisoned(poison) => {
                self.generic_bounds.remove(&id);
                self.generic_parameters
                    .insert(id, SemanticFact::Poisoned(poison.clone()))?;
                self.declarations.insert(
                    CanonicalTarget::Source(declaration_id),
                    SemanticFact::Poisoned(poison.clone()),
                )?;
                SourceGenericState::Poisoned(poison)
            }
            Resolution::Unperformed => {
                self.generic_bounds.remove(&id);
                SourceGenericState::Unperformed
            }
        };
        let result = match &state {
            SourceGenericState::Resolved => Resolution::Known(()),
            SourceGenericState::Poisoned(poison) => Resolution::Poisoned(poison.clone()),
            SourceGenericState::Resolving | SourceGenericState::Unperformed => {
                Resolution::Unperformed
            }
        };
        self.source_generic_states.insert(declaration_id, state);
        let completed = self
            .source_generic_stack
            .pop()
            .expect("resolving source generic is on the resolution stack");
        debug_assert_eq!(completed, declaration_id);
        Ok(result)
    }
}
