use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn resolve_source_trait_ref(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        type_ref: TypeRefId,
        context: &SourceTypeContext,
    ) -> Result<Resolution<TraitRef>, SignatureResolutionError> {
        let occurrence = hir.type_ref(type_ref);
        let result = match occurrence.kind() {
            TypeRefKind::Path { path, arguments } => {
                match self.lookup_type_target(owner.module(), source, path)? {
                    Resolution::Known(target) => {
                        if self.target_kind(&target)? != DeclarationKind::Trait {
                            let poison = self.source_error(
                                source,
                                occurrence.origin(),
                                "TRAIT_BOUND_INVALID",
                                "trait bound is invalid because the path does not name a trait",
                                BTreeMap::from([(
                                    Box::<str>::from("reason"),
                                    DiagnosticDataValue::from("not_trait"),
                                )]),
                            )?;
                            return self.record_trait_occurrence(
                                source,
                                type_ref,
                                occurrence.origin(),
                                None,
                                Resolution::Poisoned(poison),
                            );
                        }
                        match self.source_head_state(&target)? {
                            Resolution::Known(()) => {}
                            Resolution::Poisoned(poison) => {
                                return self.record_trait_occurrence(
                                    source,
                                    type_ref,
                                    occurrence.origin(),
                                    Some(target),
                                    Resolution::Poisoned(poison),
                                );
                            }
                            Resolution::Unperformed => {
                                return self.record_trait_occurrence(
                                    source,
                                    type_ref,
                                    occurrence.origin(),
                                    Some(target),
                                    Resolution::Unperformed,
                                );
                            }
                        }
                        let Some(info) = self.trait_info.get(&target).cloned() else {
                            return match &target {
                                CanonicalTarget::External(_) => self.record_trait_occurrence(
                                    source,
                                    type_ref,
                                    occurrence.origin(),
                                    Some(target),
                                    Resolution::Unperformed,
                                ),
                                CanonicalTarget::Source(_) => {
                                    Err(SignatureResolutionError::MissingTrustedSignature(target))
                                }
                            };
                        };
                        if arguments.len() != info.arity {
                            let poison = self.record_arity_error(
                                source,
                                occurrence.origin(),
                                info.arity,
                                arguments.len(),
                            )?;
                            Resolution::Poisoned(poison)
                        } else {
                            match self.resolve_source_types(
                                owner,
                                source,
                                hir,
                                arguments,
                                TypePosition::Ordinary,
                                context,
                            )? {
                                Resolution::Known(arguments) => match self
                                    .validate_source_abstraction_arguments(
                                        source,
                                        occurrence.origin(),
                                        arguments,
                                    )? {
                                    Resolution::Known(arguments) => {
                                        let substitution =
                                            self.types.intern_substitution(arguments)?;
                                        Resolution::Known(TraitRef::new(
                                            target.clone(),
                                            substitution,
                                        ))
                                    }
                                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                                    Resolution::Unperformed => unreachable!(),
                                },
                                Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                                Resolution::Unperformed => Resolution::Unperformed,
                            }
                        }
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                }
            }
            TypeRefKind::Invalid => self.invalid_resolution(source, occurrence.origin())?,
            _ => {
                let poison = self.source_error(
                    source,
                    occurrence.origin(),
                    "TRAIT_BOUND_INVALID",
                    "trait bound is invalid because it is not a nominal trait path",
                    BTreeMap::from([(
                        Box::<str>::from("reason"),
                        DiagnosticDataValue::from("not_trait_path"),
                    )]),
                )?;
                Resolution::Poisoned(poison)
            }
        };
        let written = match &result {
            Resolution::Known(reference) => Some(reference.declaration().clone()),
            _ => None,
        };
        self.record_trait_occurrence(source, type_ref, occurrence.origin(), written, result)
    }

    pub(super) fn resolve_source_type(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        type_ref: TypeRefId,
        position: TypePosition,
        context: &SourceTypeContext,
    ) -> Result<Resolution<TypeId>, SignatureResolutionError> {
        let occurrence = hir.type_ref(type_ref);
        let (result, written) =
            self.resolve_source_type_inner(owner, source, hir, type_ref, position, context)?;
        self.record_type_occurrence(source, type_ref, occurrence.origin(), written, result)
    }

    pub(super) fn resolve_source_type_inner(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        type_ref: TypeRefId,
        position: TypePosition,
        context: &SourceTypeContext,
    ) -> Result<(Resolution<TypeId>, Option<CanonicalTarget>), SignatureResolutionError> {
        let occurrence = hir.type_ref(type_ref);
        let mut written = None;
        let result = match occurrence.kind() {
            TypeRefKind::Reference {
                mutability,
                referent,
            } => self
                .resolve_source_type(
                    owner,
                    source,
                    hir,
                    *referent,
                    position.reference_referent(),
                    context,
                )?
                .map(|referent| {
                    self.types
                        .reference(reference_mutability(*mutability), referent)
                })
                .transpose()?,
            TypeRefKind::Pointer {
                mutability,
                referent,
            } => {
                if !position.is_extern() {
                    Resolution::Poisoned(self.source_error(
                        source,
                        occurrence.origin(),
                        "RAW_POINTER_TYPE_UNSUPPORTED",
                        "raw pointer type is unsupported in this type position",
                        BTreeMap::new(),
                    )?)
                } else {
                    self.resolve_source_type(
                        owner,
                        source,
                        hir,
                        *referent,
                        position.nested(),
                        context,
                    )?
                    .map(|referent| {
                        self.types
                            .raw_pointer(raw_pointer_mutability(*mutability), referent)
                    })
                    .transpose()?
                }
            }
            TypeRefKind::Path { path, arguments } => {
                let path_result = self.resolve_source_path_type(
                    SourceTypeSite {
                        owner,
                        source,
                        hir,
                        context,
                    },
                    path,
                    arguments,
                    position,
                )?;
                written = path_result.1;
                path_result.0
            }
            TypeRefKind::Dynamic { trait_ref } => {
                let nested =
                    self.resolve_source_trait_ref(owner, source, hir, *trait_ref, context)?;
                let (code, message) = if position.is_extern() {
                    (
                        "EXTERN_SIGNATURE_UNSUPPORTED",
                        "dyn trait type is unsupported in an extern signature",
                    )
                } else {
                    ("DYN_TRAIT_UNSUPPORTED", "dyn trait type is unsupported")
                };
                let poison =
                    self.source_error(source, occurrence.origin(), code, message, BTreeMap::new())?;
                match nested {
                    Resolution::Poisoned(nested) => Resolution::Poisoned(nested.merge(&poison)?),
                    Resolution::Unperformed => Resolution::Unperformed,
                    Resolution::Known(_) => Resolution::Poisoned(poison),
                }
            }
            TypeRefKind::Tuple(elements) => self
                .resolve_source_types(owner, source, hir, elements, position.nested(), context)?
                .map(|elements| self.types.tuple(elements))
                .transpose()?,
            TypeRefKind::Function { parameters, result } => {
                let parameters = self.resolve_source_types(
                    owner,
                    source,
                    hir,
                    parameters,
                    position.nested(),
                    context,
                )?;
                let result = self.resolve_source_type(
                    owner,
                    source,
                    hir,
                    *result,
                    position.nested(),
                    context,
                )?;
                combine_types(parameters, result, |parameters, result| {
                    self.types.callable(parameters, result)
                })?
            }
            TypeRefKind::Group(inner) => {
                return self
                    .resolve_source_type(owner, source, hir, *inner, position, context)
                    .map(|result| (result, None));
            }
            TypeRefKind::Invalid => self.invalid_resolution(source, occurrence.origin())?,
        };
        let result = match result {
            Resolution::Known(ty) => {
                self.validate_source_position(source, occurrence.origin(), ty, position)?
            }
            other => other,
        };
        Ok((result, written))
    }

    pub(super) fn resolve_source_path_type(
        &mut self,
        site: SourceTypeSite<'_, 'hir>,
        path: &eglur_hir::Path,
        arguments: &[TypeRefId],
        position: TypePosition,
    ) -> Result<(Resolution<TypeId>, Option<CanonicalTarget>), SignatureResolutionError> {
        let SourceTypeSite {
            owner,
            source,
            hir,
            context,
        } = site;
        let Some(path_text) = path.text() else {
            return Ok((self.invalid_path_resolution(source, path)?, None));
        };
        let segments = path.segments();
        let generic_projection = segments.len() > 1
            && segments[0]
                .text()
                .is_some_and(|name| context.generics.contains_key(name))
            && !self.has_known_module_prefix(owner.module(), path);
        if segments.len() > 1 && (segments[0].text() == Some("Self") || generic_projection) {
            return self.resolve_source_projection(source, hir, path, arguments, context);
        }
        if segments.len() == 1 {
            if path_text == "Never" {
                if !arguments.is_empty() {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            0,
                            arguments.len(),
                        )?),
                        None,
                    ));
                }
                return Ok((Resolution::Known(self.types.never()), None));
            }
            if let Some(primitive) = crate::PrimitiveType::from_spelling(&path_text) {
                if !arguments.is_empty() {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            0,
                            arguments.len(),
                        )?),
                        None,
                    ));
                }
                return Ok((Resolution::Known(self.types.primitive(primitive)?), None));
            }
            if let Some(intrinsic) = IntrinsicNominal::from_spelling(&path_text) {
                if arguments.len() != intrinsic.arity() {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            intrinsic.arity(),
                            arguments.len(),
                        )?),
                        None,
                    ));
                }
                let argument_positions =
                    (0..arguments.len()).map(|index| position.intrinsic_argument(intrinsic, index));
                let arguments = self.resolve_source_types_with_positions(
                    owner,
                    source,
                    hir,
                    arguments,
                    argument_positions,
                    context,
                )?;
                let result = match arguments {
                    Resolution::Known(arguments) => {
                        let ty = self
                            .types
                            .nominal(NominalHead::Intrinsic(intrinsic), arguments)?;
                        self.validate_source_slot(source, path.origin(), ty)?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                return Ok((result, None));
            }
            if path_text == "Union" {
                if arguments.len() < 2 {
                    let poison = self.source_error(
                        source,
                        path.origin(),
                        "UNION_TYPE_ARITY_MISMATCH",
                        format!(
                            "Union type argument arity mismatch: expected at least 2, found {}",
                            arguments.len()
                        ),
                        BTreeMap::from([
                            (
                                Box::<str>::from("minimum_count"),
                                DiagnosticDataValue::from(2_i64),
                            ),
                            (
                                Box::<str>::from("actual_count"),
                                diagnostic_count(arguments.len()),
                            ),
                        ]),
                    )?;
                    return Ok((Resolution::Poisoned(poison), None));
                }
                if position != TypePosition::ErrorChannel {
                    let (code, message) = if position.is_extern() {
                        (
                            "EXTERN_SIGNATURE_UNSUPPORTED",
                            "Union type is unsupported in an extern signature",
                        )
                    } else {
                        (
                            "UNION_TYPE_UNSUPPORTED",
                            "Union type is unsupported outside a Res error channel",
                        )
                    };
                    let poison =
                        self.source_error(source, path.origin(), code, message, BTreeMap::new())?;
                    return Ok((Resolution::Poisoned(poison), None));
                }
                let members = self.resolve_source_types(
                    owner,
                    source,
                    hir,
                    arguments,
                    position.nested(),
                    context,
                )?;
                let result = match members {
                    Resolution::Known(members) => {
                        let ty = self.types.error_union(members)?;
                        self.validate_source_position(source, path.origin(), ty, position)?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                return Ok((result, None));
            }
            if path_text == "Self" {
                if !arguments.is_empty() {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            0,
                            arguments.len(),
                        )?),
                        None,
                    ));
                }
                let Some(binding) = &context.self_binding else {
                    let poison = self.source_error(
                        source,
                        path.origin(),
                        "SELF_TYPE_INVALID",
                        "Self type is invalid outside an owning method or associated binding",
                        BTreeMap::from([(
                            Box::<str>::from("position"),
                            DiagnosticDataValue::from("declaration_signature"),
                        )]),
                    )?;
                    return Ok((Resolution::Poisoned(poison), None));
                };
                return Ok((Resolution::Known(binding.ty), None));
            }
            if let Some(parameter) = context.generics.get(path_text.as_str()) {
                if !arguments.is_empty() {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            0,
                            arguments.len(),
                        )?),
                        Some(generic_target(parameter)),
                    ));
                }
                return Ok((
                    Resolution::Known(self.generic_types[parameter]),
                    Some(generic_target(parameter)),
                ));
            }
        }

        if segments.len() > 1 {
            match self.is_concrete_projection(owner.module(), source, path)? {
                Resolution::Known(true) => {
                    return Ok((
                        Resolution::Poisoned(self.projection_error(
                            source,
                            path.origin(),
                            "concrete base",
                        )?),
                        None,
                    ));
                }
                Resolution::Known(false) => {}
                Resolution::Poisoned(poison) => {
                    return Ok((Resolution::Poisoned(poison), None));
                }
                Resolution::Unperformed => return Ok((Resolution::Unperformed, None)),
            }
        }

        let target = match self.lookup_type_target(owner.module(), source, path)? {
            Resolution::Known(target) => target,
            Resolution::Poisoned(poison) => {
                return Ok((Resolution::Poisoned(poison), None));
            }
            Resolution::Unperformed => return Ok((Resolution::Unperformed, None)),
        };
        let kind = self.target_kind(&target)?;
        match self.source_head_state(&target)? {
            Resolution::Known(()) => {}
            Resolution::Poisoned(poison) => {
                return Ok((Resolution::Poisoned(poison), Some(target)));
            }
            Resolution::Unperformed => {
                return Ok((Resolution::Unperformed, Some(target)));
            }
        }
        match kind {
            DeclarationKind::Struct | DeclarationKind::Enum | DeclarationKind::OpaqueType => {
                let head = NominalHead::Declaration(target.clone());
                let Some(expected) = self.types.nominal_arity(&head) else {
                    return match target {
                        CanonicalTarget::External(_) => Ok((Resolution::Unperformed, Some(target))),
                        CanonicalTarget::Source(_) => {
                            Err(SignatureResolutionError::MissingTrustedSignature(target))
                        }
                    };
                };
                if arguments.len() != expected {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            expected,
                            arguments.len(),
                        )?),
                        Some(target),
                    ));
                }
                let arguments = self.resolve_source_types(
                    owner,
                    source,
                    hir,
                    arguments,
                    position.nested(),
                    context,
                )?;
                let result = match arguments {
                    Resolution::Known(arguments) => {
                        let ty = self.types.nominal(head, arguments)?;
                        self.validate_source_slot(source, path.origin(), ty)?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                Ok((result, Some(target)))
            }
            DeclarationKind::TypeAlias => {
                let CanonicalTarget::Source(alias) = target else {
                    return Ok((Resolution::Unperformed, Some(target)));
                };
                let expected = self.alias_arities[&alias];
                if arguments.len() != expected {
                    return Ok((
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            path.origin(),
                            expected,
                            arguments.len(),
                        )?),
                        Some(CanonicalTarget::Source(alias)),
                    ));
                }
                let arguments = self.resolve_source_types(
                    owner,
                    source,
                    hir,
                    arguments,
                    position.nested(),
                    context,
                )?;
                let result = match arguments {
                    Resolution::Known(arguments) => {
                        let arguments = match self.validate_source_abstraction_arguments(
                            source,
                            path.origin(),
                            arguments,
                        )? {
                            Resolution::Known(arguments) => arguments,
                            Resolution::Poisoned(poison) => {
                                return Ok((
                                    Resolution::Poisoned(poison),
                                    Some(CanonicalTarget::Source(alias)),
                                ));
                            }
                            Resolution::Unperformed => unreachable!(),
                        };
                        match self.resolve_alias(alias, Some((source, path.origin())))? {
                            Resolution::Known(template) => {
                                let replacements = template
                                    .generic_parameters
                                    .iter()
                                    .cloned()
                                    .zip(arguments)
                                    .collect::<BTreeMap<_, _>>();
                                let ty = self.types.substitute(template.value, &replacements)?;
                                self.validate_source_position(source, path.origin(), ty, position)?
                            }
                            Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                            Resolution::Unperformed => Resolution::Unperformed,
                        }
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                Ok((result, Some(CanonicalTarget::Source(alias))))
            }
            DeclarationKind::Trait => Ok((
                Resolution::Poisoned(self.source_error(
                    source,
                    path.origin(),
                    "DYN_MUST_BE_EXPLICIT",
                    format!("dyn must be explicit when using trait `{path_text}` as a type"),
                    BTreeMap::new(),
                )?),
                Some(target),
            )),
            _ => Ok((
                Resolution::Poisoned(self.source_error(
                    source,
                    path.origin(),
                    "NAME_NOT_FOUND",
                    format!("type name `{path_text}` not found"),
                    BTreeMap::from([(
                        Box::<str>::from("name"),
                        DiagnosticDataValue::from(path_text),
                    )]),
                )?),
                Some(target),
            )),
        }
    }

    pub(super) fn resolve_source_projection(
        &mut self,
        source: &SemanticSource<'hir>,
        _hir: &HirFile,
        path: &eglur_hir::Path,
        arguments: &[TypeRefId],
        context: &SourceTypeContext,
    ) -> Result<(Resolution<TypeId>, Option<CanonicalTarget>), SignatureResolutionError> {
        let segments = path.segments();
        if segments.len() != 2 {
            return Ok((
                Resolution::Poisoned(self.projection_error(source, path.origin(), "chained")?),
                None,
            ));
        }
        let base_name = segments[0].text().unwrap_or("<invalid>");
        let associated_name = segments[1].text().unwrap_or("<invalid>");
        let (base, trait_refs) = if base_name == "Self" {
            let Some(binding) = &context.self_binding else {
                let poison = self.source_error(
                    source,
                    path.origin(),
                    "SELF_TYPE_INVALID",
                    "Self type is invalid outside an owning method or associated binding",
                    BTreeMap::from([(
                        Box::<str>::from("position"),
                        DiagnosticDataValue::from("associated_projection"),
                    )]),
                )?;
                return Ok((Resolution::Poisoned(poison), None));
            };
            let Some(trait_ref) = &binding.trait_ref else {
                return Ok((
                    Resolution::Poisoned(self.projection_error(
                        source,
                        path.origin(),
                        "Self has no declaring trait",
                    )?),
                    None,
                ));
            };
            (binding.ty, vec![trait_ref.clone()])
        } else {
            let parameter = context
                .generics
                .get(base_name)
                .expect("projection classifier found generic base")
                .clone();
            let base = self.generic_types[&parameter];
            if !self.generic_bounds.contains_key(&parameter) {
                let GenericParameterId::Source(declaration) = &parameter else {
                    return Ok((Resolution::Unperformed, None));
                };
                if matches!(
                    self.source_generic_states.get(declaration),
                    Some(SourceGenericState::Resolving)
                ) && self.source_generic_stack.last() != Some(declaration)
                {
                    return Ok((Resolution::Unperformed, None));
                }
                match self.resolve_source_generic(*declaration)? {
                    Resolution::Known(()) => {}
                    Resolution::Poisoned(poison) => {
                        return Ok((Resolution::Poisoned(poison), None));
                    }
                    Resolution::Unperformed => return Ok((Resolution::Unperformed, None)),
                }
            }
            let Some(bounds) = self.generic_bounds.get(&parameter) else {
                return match self.generic_parameters.state(&parameter) {
                    FactState::Poisoned(poison) => Ok((Resolution::Poisoned(poison.clone()), None)),
                    _ => Ok((Resolution::Unperformed, None)),
                };
            };
            (base, bounds.to_vec())
        };
        let mut candidates = Vec::new();
        for trait_ref in trait_refs {
            let Some(info) = self.trait_info.get(trait_ref.declaration()) else {
                continue;
            };
            if let Some(associated) = info.associated.get(associated_name) {
                for target in associated {
                    candidates.push((trait_ref.clone(), target.clone()));
                }
            }
        }
        if candidates.len() != 1 {
            let reason = if candidates.is_empty() {
                "no declaring bound"
            } else {
                "ambiguous declaring bounds"
            };
            return Ok((
                Resolution::Poisoned(self.projection_error(source, path.origin(), reason)?),
                None,
            ));
        }
        if !arguments.is_empty() {
            return Ok((
                Resolution::Poisoned(self.record_arity_error(
                    source,
                    path.origin(),
                    0,
                    arguments.len(),
                )?),
                Some(candidates[0].1.clone()),
            ));
        }
        let (trait_ref, associated) = candidates.pop().expect("one projection candidate");
        let ty = self.types.associated_projection(AssociatedProjection::new(
            base,
            trait_ref,
            associated.clone(),
        ))?;
        Ok((Resolution::Known(ty), Some(associated)))
    }

    pub(super) fn resolve_source_types(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        types: &[TypeRefId],
        position: TypePosition,
        context: &SourceTypeContext,
    ) -> Result<Resolution<Vec<TypeId>>, SignatureResolutionError> {
        self.resolve_source_types_with_positions(
            owner,
            source,
            hir,
            types,
            std::iter::repeat_n(position, types.len()),
            context,
        )
    }

    pub(super) fn resolve_source_types_with_positions(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        types: &[TypeRefId],
        positions: impl IntoIterator<Item = TypePosition>,
        context: &SourceTypeContext,
    ) -> Result<Resolution<Vec<TypeId>>, SignatureResolutionError> {
        let mut values = Vec::with_capacity(types.len());
        let mut poison = None;
        let mut unperformed = false;
        for (&type_ref, position) in types.iter().zip(positions) {
            match self.resolve_source_type(owner, source, hir, type_ref, position, context)? {
                Resolution::Known(ty) => values.push(ty),
                Resolution::Poisoned(found) => merge_poison(&mut poison, &found)?,
                Resolution::Unperformed => unperformed = true,
            }
        }
        Ok(if let Some(poison) = poison {
            Resolution::Poisoned(poison)
        } else if unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(values)
        })
    }

    pub(super) fn validate_source_position(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        ty: TypeId,
        position: TypePosition,
    ) -> Result<Resolution<TypeId>, SignatureResolutionError> {
        if !position.is_extern()
            && self.types.contains_ownership(ty)?
            && !self.ownership_allowed_at(ty, position)?
        {
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                origin,
                "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                "Own type position is forbidden here",
                BTreeMap::new(),
            )?));
        }
        if self.types.contains_raw_pointer(ty)? && !position.is_extern() {
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                origin,
                "RAW_POINTER_TYPE_UNSUPPORTED",
                "raw pointer type is unsupported in this type position",
                BTreeMap::new(),
            )?));
        }
        if matches!(self.types.kind(ty)?, TypeKind::ErrorUnion(_))
            && position != TypePosition::ErrorChannel
        {
            let (code, message) = if position.is_extern() {
                (
                    "EXTERN_SIGNATURE_UNSUPPORTED",
                    "Union type is unsupported in an extern signature",
                )
            } else {
                (
                    "UNION_TYPE_UNSUPPORTED",
                    "Union type is unsupported outside a Res error channel",
                )
            };
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                origin,
                code,
                message,
                BTreeMap::new(),
            )?));
        }
        if matches!(
            position,
            TypePosition::ExternParameter | TypePosition::ExternReturn
        ) && !self.is_supported_extern_type(ty, position == TypePosition::ExternReturn)?
        {
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                origin,
                "EXTERN_SIGNATURE_UNSUPPORTED",
                "extern signature type is unsupported",
                BTreeMap::new(),
            )?));
        }
        Ok(Resolution::Known(ty))
    }

    pub(super) fn ownership_allowed_at(
        &self,
        ty: TypeId,
        position: TypePosition,
    ) -> Result<bool, SignatureResolutionError> {
        match position {
            TypePosition::CallableBoundary | TypePosition::LocalBoundary => {
                if self.types.ownership_root(ty)?.is_some() {
                    return Ok(true);
                }
                let TypeKind::Reference { referent, .. } = self.types.kind(ty)? else {
                    return Ok(false);
                };
                Ok(self.types.ownership_root(*referent)?.is_some())
            }
            TypePosition::ConditionalOwnership => Ok(matches!(
                self.types.ownership_root(ty)?,
                Some(crate::OwnershipRoot::Direct { .. })
            )),
            TypePosition::BorrowedOwnership => Ok(self.types.ownership_root(ty)?.is_some()),
            TypePosition::Ordinary
            | TypePosition::ExternParameter
            | TypePosition::ExternReturn
            | TypePosition::ExternNested
            | TypePosition::ErrorChannel => Ok(false),
        }
    }

    pub(super) fn is_supported_extern_type(
        &self,
        ty: TypeId,
        allow_unit: bool,
    ) -> Result<bool, SignatureResolutionError> {
        match self.types.kind(ty)? {
            TypeKind::Primitive(primitive) => Ok(is_abi_numeric(*primitive)
                || *primitive == crate::PrimitiveType::Bool
                || (allow_unit && *primitive == crate::PrimitiveType::Unit)),
            TypeKind::RawPointer { referent, .. } => Ok(matches!(
                self.types.kind(*referent)?,
                TypeKind::Primitive(primitive) if is_abi_numeric(*primitive)
            )),
            _ => Ok(false),
        }
    }

    pub(super) fn validate_source_slot(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        ty: TypeId,
    ) -> Result<Resolution<TypeId>, SignatureResolutionError> {
        let TypeKind::Nominal { head, substitution } = self.types.kind(ty)? else {
            return Ok(Resolution::Known(ty));
        };
        let arguments = self.types.substitution(*substitution)?.to_vec();
        let ordinary = match head {
            NominalHead::Intrinsic(IntrinsicNominal::Own) => &arguments[..],
            NominalHead::Intrinsic(IntrinsicNominal::Opt)
                if self.types.ownership_root(ty)?.is_some() =>
            {
                return Ok(Resolution::Known(ty));
            }
            NominalHead::Intrinsic(IntrinsicNominal::Res)
                if self.types.ownership_root(ty)?.is_some() =>
            {
                &arguments[1..]
            }
            _ => &arguments[..],
        };
        for &argument in ordinary {
            if self.types.contains_ownership(argument)? {
                return Ok(Resolution::Poisoned(self.source_error(
                    source,
                    origin,
                    "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                    "ownership type is forbidden in this generic argument position",
                    BTreeMap::new(),
                )?));
            }
            if self.types.contains_reference(argument)? {
                return Ok(Resolution::Poisoned(self.source_error(
                    source,
                    origin,
                    "REFERENCE_TYPE_POSITION_FORBIDDEN",
                    "reference type is forbidden in this generic argument position",
                    BTreeMap::new(),
                )?));
            }
        }
        Ok(Resolution::Known(ty))
    }

    pub(super) fn validate_source_abstraction_arguments(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        arguments: Vec<TypeId>,
    ) -> Result<Resolution<Vec<TypeId>>, SignatureResolutionError> {
        if arguments
            .iter()
            .copied()
            .map(|argument| self.types.contains_ownership(argument))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|found| found)
        {
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                origin,
                "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                "ownership type is forbidden in this generic argument position",
                BTreeMap::new(),
            )?));
        }
        if arguments
            .iter()
            .copied()
            .map(|argument| self.types.contains_reference(argument))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(|found| found)
        {
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                origin,
                "REFERENCE_TYPE_POSITION_FORBIDDEN",
                "reference type is forbidden in this generic argument position",
                BTreeMap::new(),
            )?));
        }
        Ok(Resolution::Known(arguments))
    }

    pub(super) fn resolve_alias(
        &mut self,
        alias: DefId,
        cycle_site: Option<(&SemanticSource<'hir>, OriginId)>,
    ) -> Result<Resolution<AliasTemplate>, SignatureResolutionError> {
        if let Some(state) = self.alias_states.get(&alias).cloned() {
            return match state {
                AliasState::Resolving => {
                    let (source, origin) = if let Some(site) = cycle_site {
                        site
                    } else {
                        let declaration = self
                            .collection
                            .declaration(alias)
                            .expect("alias belongs to collection");
                        let module = self
                            .collection
                            .module(alias.module())
                            .expect("alias module belongs to collection");
                        (self.sources[&module.file()], declaration.origin())
                    };
                    Ok(Resolution::Poisoned(self.source_error(
                        source,
                        origin,
                        "TYPE_ALIAS_CYCLE",
                        "type alias expansion contains a cycle",
                        BTreeMap::new(),
                    )?))
                }
                AliasState::Resolved {
                    generic_parameters,
                    value,
                } => Ok(Resolution::Known(AliasTemplate {
                    generic_parameters,
                    value,
                })),
                AliasState::Poisoned(poison) => Ok(Resolution::Poisoned(poison)),
                AliasState::Unperformed => Ok(Resolution::Unperformed),
            };
        }
        self.alias_states.insert(alias, AliasState::Resolving);
        let declaration = self
            .collection
            .declaration(alias)
            .expect("alias belongs to collection");
        let module = self
            .collection
            .module(alias.module())
            .expect("alias module belongs to collection");
        let source = self.sources[&module.file()];
        let hir = self.source_hir(module.file())?;
        let item_id = declaration
            .item()
            .ok_or(SignatureResolutionError::MissingDeclarationItem(alias))?;
        let ItemKind::TypeAlias(item) = hir.item(item_id).kind() else {
            return Err(SignatureResolutionError::WrongDeclarationItem(alias));
        };
        let context = self.source_context(alias, false)?;
        let result = self.resolve_source_type(
            alias,
            source,
            hir,
            item.value,
            TypePosition::Ordinary,
            &context,
        )?;
        let generic_parameters = self.direct_generic_ids(alias);
        let state = match &result {
            Resolution::Known(value) => AliasState::Resolved {
                generic_parameters: generic_parameters.clone(),
                value: *value,
            },
            Resolution::Poisoned(poison) => AliasState::Poisoned(poison.clone()),
            Resolution::Unperformed => AliasState::Unperformed,
        };
        self.alias_states.insert(alias, state);
        Ok(result.map(|value| AliasTemplate {
            generic_parameters,
            value,
        }))
    }

    pub(super) fn lookup_type_target(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'hir>,
        path: &eglur_hir::Path,
    ) -> Result<Resolution<CanonicalTarget>, SignatureResolutionError> {
        let Some(text) = path.text() else {
            return self.invalid_path_resolution(source, path);
        };
        let keys = path
            .segments()
            .iter()
            .map(NameKey::from_hir)
            .collect::<Option<Vec<_>>>();
        let Some(keys) = keys else {
            return self.invalid_resolution(source, path.origin());
        };
        let lookup = if keys.len() == 1 {
            let name = &keys[0];
            let current = self
                .collection
                .module(module)
                .expect("lookup module belongs to collection");
            match lookup_source_module_type(self.collection, self.imports, current, name, false)? {
                TypeNameLookup::NotFound => self
                    .external_names
                    .map_or(TypeNameLookup::Unperformed, |external| {
                        lookup_prelude_type(external, name)
                    }),
                found => found,
            }
        } else if let Some(address) = self.qualified_module_address(module, &keys[..keys.len() - 1])
        {
            let name = &keys[keys.len() - 1];
            if let Some(source_module) = self.collection.module_for_address(&address) {
                let target = self
                    .collection
                    .module(source_module)
                    .expect("address lookup returned source module");
                lookup_source_module_type(
                    self.collection,
                    self.imports,
                    target,
                    name,
                    source_module != module,
                )?
            } else {
                self.external_names
                    .map_or(TypeNameLookup::Unperformed, |external| {
                        lookup_external_module_type(external, &address, name)
                    })
            }
        } else if self.external_names.is_none() {
            TypeNameLookup::Unperformed
        } else {
            TypeNameLookup::NotFound
        };
        self.finish_type_name_lookup(source, path.origin(), &text, lookup)
    }

    pub(super) fn finish_type_name_lookup(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        spelling: &str,
        lookup: TypeNameLookup,
    ) -> Result<Resolution<CanonicalTarget>, SignatureResolutionError> {
        match lookup {
            TypeNameLookup::Unique(target) => Ok(Resolution::Known(target)),
            TypeNameLookup::Ambiguous(candidates) => {
                Ok(Resolution::Poisoned(record_name_ambiguous(
                    self.diagnostics,
                    self.required_span(source, origin)?,
                    spelling,
                    candidates.iter().map(target_label),
                )?))
            }
            TypeNameLookup::Poisoned(poison) => Ok(Resolution::Poisoned(poison)),
            TypeNameLookup::Unperformed => Ok(Resolution::Unperformed),
            TypeNameLookup::VisibilityViolation => Ok(Resolution::Poisoned(
                record_visibility_violation(self.diagnostics, self.required_span(source, origin)?)?,
            )),
            TypeNameLookup::NotFound => Ok(Resolution::Poisoned(record_name_not_found(
                self.diagnostics,
                self.required_span(source, origin)?,
                spelling,
            )?)),
        }
    }

    pub(super) fn qualified_module_address(
        &self,
        module: ModuleId,
        prefix: &[NameKey],
    ) -> Option<ModuleAddress> {
        let current = self.collection.module(module)?.address();
        let mut components = Vec::<Box<str>>::new();
        let mut offset = 0;
        if prefix.first().is_some_and(|name| name.text() == "self") {
            components.extend(current.components().map(Box::<str>::from));
            offset = 1;
        } else if prefix.first().is_some_and(|name| name.text() == "super") {
            components.extend(current.components().map(Box::<str>::from));
            if components.len() == 1 {
                return None;
            }
            components.pop();
            offset = 1;
        }
        components.extend(
            prefix[offset..]
                .iter()
                .filter(|name| !name.is_bang())
                .map(|name| Box::<str>::from(name.text())),
        );
        ModuleAddress::new(components).ok()
    }

    pub(super) fn has_known_module_prefix(&self, module: ModuleId, path: &eglur_hir::Path) -> bool {
        let Some(keys) = path
            .segments()
            .iter()
            .map(NameKey::from_hir)
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        (1..keys.len()).rev().any(|prefix_len| {
            self.qualified_module_address(module, &keys[..prefix_len])
                .is_some_and(|address| {
                    self.collection.module_for_address(&address).is_some()
                        || self
                            .external_names
                            .is_some_and(|external| external.has_module(&address))
                })
        })
    }

    pub(super) fn is_concrete_projection(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'hir>,
        path: &eglur_hir::Path,
    ) -> Result<Resolution<bool>, SignatureResolutionError> {
        let Some(keys) = path
            .segments()
            .iter()
            .map(NameKey::from_hir)
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(Resolution::Known(false));
        };
        if keys.len() < 2 {
            return Ok(Resolution::Known(false));
        }
        if let Some(address) = self.qualified_module_address(module, &keys[..keys.len() - 1])
            && (self.collection.module_for_address(&address).is_some()
                || self
                    .external_names
                    .is_some_and(|external| external.has_module(&address)))
        {
            return Ok(Resolution::Known(false));
        }

        let base = &keys[keys.len() - 2];
        let lookup = if keys.len() == 2 {
            let current = self
                .collection
                .module(module)
                .expect("lookup module belongs to collection");
            match lookup_source_module_type(self.collection, self.imports, current, base, false)? {
                TypeNameLookup::NotFound => self
                    .external_names
                    .map_or(TypeNameLookup::Unperformed, |external| {
                        lookup_prelude_type(external, base)
                    }),
                found => found,
            }
        } else if let Some(address) = self.qualified_module_address(module, &keys[..keys.len() - 2])
        {
            if let Some(source_module) = self.collection.module_for_address(&address) {
                let target = self
                    .collection
                    .module(source_module)
                    .expect("address lookup returned source module");
                lookup_source_module_type(
                    self.collection,
                    self.imports,
                    target,
                    base,
                    source_module != module,
                )?
            } else {
                self.external_names
                    .map_or(TypeNameLookup::Unperformed, |external| {
                        lookup_external_module_type(external, &address, base)
                    })
            }
        } else if self.external_names.is_none() {
            TypeNameLookup::Unperformed
        } else {
            TypeNameLookup::NotFound
        };
        let candidate = match lookup {
            TypeNameLookup::Unique(candidate) => candidate,
            TypeNameLookup::NotFound => return Ok(Resolution::Known(false)),
            other => {
                let spelling = keys[..keys.len() - 1]
                    .iter()
                    .map(NameKey::spelling)
                    .collect::<Vec<_>>()
                    .join("::");
                return self
                    .finish_type_name_lookup(source, path.origin(), &spelling, other)
                    .map(|result| result.map(|_| false));
            }
        };
        Ok(Resolution::Known(matches!(
            self.target_kind(&candidate)?,
            DeclarationKind::Struct
                | DeclarationKind::Enum
                | DeclarationKind::OpaqueType
                | DeclarationKind::TypeAlias
                | DeclarationKind::Trait
        )))
    }

    pub(super) fn target_kind(
        &self,
        target: &CanonicalTarget,
    ) -> Result<DeclarationKind, SignatureResolutionError> {
        match target {
            CanonicalTarget::Source(id) => Ok(self
                .collection
                .declaration(*id)
                .expect("source target belongs to collection")
                .kind()),
            CanonicalTarget::External(id) => self
                .external_names
                .and_then(|names| names.declaration(id))
                .map(|declaration| declaration.kind())
                .ok_or_else(|| SignatureResolutionError::MissingTrustedSignature(target.clone())),
        }
    }

    pub(super) fn source_head_state(
        &mut self,
        target: &CanonicalTarget,
    ) -> Result<Resolution<()>, SignatureResolutionError> {
        let CanonicalTarget::Source(id) = target else {
            return Ok(Resolution::Known(()));
        };
        let declaration = self
            .collection
            .declaration(*id)
            .expect("source target belongs to collection");
        let reserved_name = declaration
            .name()
            .filter(|name| introduces_reserved_name(declaration.kind(), name.text()))
            .map(|name| Box::<str>::from(name.text()));
        if let Some(name) = reserved_name {
            return match declaration_fact_state(
                &self.declarations,
                &self.overload_family_poisons,
                target,
            ) {
                FactState::Poisoned(poison) => Ok(Resolution::Poisoned(poison.clone())),
                FactState::Unperformed => {
                    let module = self
                        .collection
                        .module(id.module())
                        .expect("source target module belongs to collection");
                    let source = self.sources[&module.file()];
                    let hir = self.source_hir(module.file())?;
                    let origin = declaration
                        .item()
                        .and_then(|item| item_declaration_name(hir.item(item).kind()))
                        .map_or(declaration.origin(), Name::origin);
                    let poison = self.source_error(
                        source,
                        origin,
                        "RESERVED_BUILTIN_NAME",
                        format!("reserved built-in name `{name}` cannot be declared"),
                        BTreeMap::new(),
                    )?;
                    self.declarations
                        .insert(target.clone(), SemanticFact::Poisoned(poison.clone()))?;
                    Ok(Resolution::Poisoned(poison))
                }
                FactState::Known(_) | FactState::NotApplicable => {
                    Err(SignatureResolutionError::InvalidSourceHeadState(*id))
                }
            };
        }
        Ok(Resolution::Known(()))
    }
}
