use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn resolve_source_declarations(&mut self) -> Result<(), SignatureResolutionError> {
        let declarations = self
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .map(Declaration::id)
            .collect::<Vec<_>>();
        for declaration_id in declarations {
            let declaration = self
                .collection
                .declaration(declaration_id)
                .expect("declaration belongs to collection");
            if declaration.kind() == DeclarationKind::GenericParameter
                || !matches!(
                    self.declarations
                        .state(&CanonicalTarget::Source(declaration_id)),
                    FactState::Unperformed
                )
            {
                continue;
            }
            let fact = self.resolve_source_declaration(declaration_id)?;
            if let Some(fact) = fact {
                self.declarations
                    .insert(CanonicalTarget::Source(declaration_id), fact)?;
            }
        }
        Ok(())
    }

    pub(super) fn validate_source_overload_families(
        &mut self,
    ) -> Result<(), SignatureResolutionError> {
        let mut families = BTreeMap::<(ModuleId, Option<DefId>, NameKey), Vec<DefId>>::new();
        for module in self.collection.modules() {
            for declaration in module
                .declarations()
                .filter(|declaration| declaration.kind() == DeclarationKind::Function)
            {
                let Some(name) = declaration.name().cloned() else {
                    continue;
                };
                families
                    .entry((module.id(), declaration.parent(), name))
                    .or_default()
                    .push(declaration.id());
            }
        }

        for ((module, parent, name), family) in families {
            if family.len() < 2 {
                continue;
            }
            if parent.is_none()
                && name.text() == "main"
                && name.is_bang()
                && self
                    .collection
                    .module(module)
                    .is_some_and(|module| Some(module.file()) == self.deferred_entry_source)
            {
                continue;
            }
            if let Some(primary) = family.iter().copied().find(|declaration| {
                !self
                    .collection
                    .declaration(*declaration)
                    .expect("overload family declaration belongs to collection")
                    .is_overload()
            }) {
                self.reject_source_overload_family(
                    &family,
                    primary,
                    (primary != family[0]).then_some(family[0]),
                    "OVERLOAD_MARKER_REQUIRED",
                    format!("overload marker required for callable `{name}`"),
                )?;
                continue;
            }

            let checks_visibility = parent.is_none()
                || parent.is_some_and(|parent| {
                    self.collection
                        .declaration(parent)
                        .is_some_and(|owner| owner.kind() == DeclarationKind::Struct)
                });
            if checks_visibility {
                let first_visibility = self
                    .collection
                    .declaration(family[0])
                    .expect("overload family declaration belongs to collection")
                    .visibility();
                if let Some(primary) = family.iter().copied().skip(1).find(|declaration| {
                    self.collection
                        .declaration(*declaration)
                        .expect("overload family declaration belongs to collection")
                        .visibility()
                        != first_visibility
                }) {
                    self.reject_source_overload_family(
                        &family,
                        primary,
                        Some(family[0]),
                        "OVERLOAD_VISIBILITY_MISMATCH",
                        format!("overload visibility mismatch for callable `{name}`"),
                    )?;
                    continue;
                }
            }

            let callables = family
                .iter()
                .copied()
                .filter_map(|declaration| {
                    match declaration_fact_state(
                        &self.declarations,
                        &self.overload_family_poisons,
                        &CanonicalTarget::Source(declaration),
                    ) {
                        FactState::Known(DeclarationSignature::Callable(callable)) => self
                            .callables
                            .get(callable.index())
                            .cloned()
                            .map(|callable| (declaration, callable)),
                        FactState::Known(_)
                        | FactState::Poisoned(_)
                        | FactState::Unperformed
                        | FactState::NotApplicable => None,
                    }
                })
                .collect::<Vec<_>>();
            let mut return_only = None;
            let mut overlap = None;
            for left in 0..callables.len() {
                for right in (left + 1)..callables.len() {
                    match classify_overload_pair(
                        &self.types,
                        &callables[left].1,
                        &callables[right].1,
                    )? {
                        Some(OverloadPairConflict::ReturnOnlyDifference) => {
                            return_only.get_or_insert((callables[left].0, callables[right].0));
                        }
                        Some(OverloadPairConflict::Overlap) => {
                            overlap.get_or_insert((callables[left].0, callables[right].0));
                        }
                        None => {}
                    }
                }
            }
            if let Some((related, primary)) = return_only {
                self.reject_source_overload_family(
                    &family,
                    primary,
                    Some(related),
                    "OVERLOAD_RETURN_ONLY_DIFFERENCE",
                    format!("overload return type difference is forbidden for callable `{name}`"),
                )?;
            } else if let Some((related, primary)) = overlap {
                self.reject_source_overload_family(
                    &family,
                    primary,
                    Some(related),
                    "OVERLOAD_OVERLAP",
                    format!("overload overlap for callable `{name}`"),
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn reject_source_overload_family(
        &mut self,
        family: &[DefId],
        primary: DefId,
        related: Option<DefId>,
        code: &'static str,
        message: String,
    ) -> Result<(), SignatureResolutionError> {
        let poison = self.source_overload_error(primary, related, code, message)?;
        for &declaration in family {
            self.overload_family_poisons
                .insert(declaration, poison.clone());
        }
        Ok(())
    }

    pub(super) fn source_overload_error(
        &mut self,
        declaration: DefId,
        related: Option<DefId>,
        code: &'static str,
        message: String,
    ) -> Result<Poison, SignatureResolutionError> {
        let declaration = self
            .collection
            .declaration(declaration)
            .expect("overload declaration belongs to collection");
        let module = self
            .collection
            .module(declaration.id().module())
            .expect("overload declaration module belongs to collection");
        let source = self.sources[&module.file()];
        let span = self.required_span(source, declaration.origin())?;
        let related = related
            .map(|related| {
                let declaration = self
                    .collection
                    .declaration(related)
                    .expect("related overload declaration belongs to collection");
                let module = self
                    .collection
                    .module(declaration.id().module())
                    .expect("related overload declaration module belongs to collection");
                let source = self.sources[&module.file()];
                Ok::<RelatedSpan, SignatureResolutionError>(
                    RelatedSpan::new(
                        self.required_span(source, declaration.origin())?,
                        "overload family declaration established here",
                    )
                    .expect("static related-span label is non-empty"),
                )
            })
            .transpose()?;
        Ok(if let Some(related) = related {
            record_type_error_with_related(
                self.diagnostics,
                code,
                message,
                span,
                [related],
                BTreeMap::new(),
            )?
        } else {
            record_type_error(self.diagnostics, code, message, span, BTreeMap::new())?
        })
    }

    pub(super) fn resolve_body_type_occurrences(&mut self) -> Result<(), SignatureResolutionError> {
        let modules = self
            .collection
            .modules()
            .map(|module| module.id())
            .collect::<Vec<_>>();
        for module_id in modules {
            let module = self
                .collection
                .module(module_id)
                .expect("collected module identity remains valid");
            let source = self.sources[&module.file()];
            let hir = self.source_hir(module.file())?;
            let owners = body_owners(module, hir)?;
            for (body_id, owner) in owners {
                if !matches!(
                    declaration_fact_state(
                        &self.declarations,
                        &self.overload_family_poisons,
                        &CanonicalTarget::Source(owner.declaration),
                    ),
                    FactState::Known(_)
                ) {
                    continue;
                }
                let body = hir.body(body_id);
                let mut roots = BTreeMap::<TypeRefId, TypePosition>::new();
                for (_, statement) in body.statements() {
                    if let StmtKind::Let { ty: Some(ty), .. } = statement.kind() {
                        roots.insert(*ty, TypePosition::LocalBoundary);
                    }
                }
                for (_, expression) in body.expressions() {
                    match expression.kind() {
                        ExprKind::Call(call) => {
                            roots.extend(
                                call.type_arguments
                                    .iter()
                                    .copied()
                                    .map(|ty| (ty, TypePosition::Ordinary)),
                            );
                        }
                        ExprKind::CatalogCall(call) => {
                            roots.extend(
                                call.owner_arguments
                                    .iter()
                                    .copied()
                                    .map(|ty| (ty, TypePosition::Ordinary)),
                            );
                        }
                        ExprKind::DTypeCall(call) => {
                            roots.insert(call.dtype, TypePosition::ConstructorDType);
                        }
                        ExprKind::Lambda(lambda) => {
                            roots.extend(lambda.parameters.iter().filter_map(|parameter| {
                                parameter.ty.map(|ty| (ty, TypePosition::CallableBoundary))
                            }));
                        }
                        _ => {}
                    }
                }
                if roots.is_empty() {
                    continue;
                }
                let context = self.source_context(owner.declaration, owner.allow_self)?;
                for (type_ref, position) in roots {
                    self.resolve_source_type(
                        owner.declaration,
                        source,
                        hir,
                        type_ref,
                        position,
                        &context,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn resolve_source_declaration(
        &mut self,
        declaration_id: DefId,
    ) -> Result<Option<SemanticFact<DeclarationSignature>>, SignatureResolutionError> {
        let declaration = self
            .collection
            .declaration(declaration_id)
            .expect("declaration belongs to collection");
        let module = self
            .collection
            .module(declaration_id.module())
            .expect("declaration module belongs to collection");
        let source = self.sources[&module.file()];
        let hir = self.source_hir(module.file())?;
        let declaration_kind = declaration.kind();
        let parent_kind = declaration
            .parent()
            .and_then(|parent| self.collection.declaration(parent))
            .map(Declaration::kind);
        let item = declaration.item().map(|item| hir.item(item).kind());
        if let Some(poison) = module.owner_poison(declaration_id) {
            return Ok(Some(SemanticFact::Poisoned(poison.clone())));
        }
        match self.declaration_context_state(declaration_id)? {
            Resolution::Known(()) => {}
            Resolution::Poisoned(poison) => {
                return Ok(Some(SemanticFact::Poisoned(poison)));
            }
            Resolution::Unperformed => return Ok(None),
        }
        if let Some(name) = item.and_then(item_declaration_name)
            && !name.is_valid()
        {
            let poison = source.invalid_poison(name.origin()).cloned().ok_or(
                SignatureResolutionError::MissingInvalidPoison {
                    file: source.file(),
                    origin: name.origin(),
                },
            )?;
            return Ok(Some(SemanticFact::Poisoned(poison)));
        }
        if let Some(name) = declaration.name()
            && introduces_reserved_name(declaration_kind, name.text())
        {
            let origin = item
                .and_then(item_declaration_name)
                .map_or(declaration.origin(), Name::origin);
            let poison = self.source_error(
                source,
                origin,
                "RESERVED_BUILTIN_NAME",
                format!(
                    "reserved built-in name `{}` cannot be declared",
                    name.text()
                ),
                BTreeMap::new(),
            )?;
            return Ok(Some(SemanticFact::Poisoned(poison)));
        }
        match self.generic_context_state(declaration_id)? {
            Resolution::Known(()) => {}
            Resolution::Poisoned(poison) => {
                return Ok(Some(SemanticFact::Poisoned(poison)));
            }
            Resolution::Unperformed => return Ok(None),
        }
        let resolved = match (declaration_kind, item) {
            (DeclarationKind::Function, Some(ItemKind::Function(function))) => self
                .resolve_source_callable(
                    declaration_id,
                    source,
                    hir,
                    SourceCallable::Function(function),
                )?,
            (DeclarationKind::ExternFunction, Some(ItemKind::Extern(function))) => {
                self.resolve_source_extern(declaration_id, source, hir, function)?
            }
            (DeclarationKind::Trait, Some(ItemKind::Trait(value))) => {
                Resolution::Known(DeclarationSignature::Trait {
                    closed: value.closed,
                    generic_parameters: self.direct_generic_ids(declaration_id),
                    associated_types: value
                        .items
                        .iter()
                        .filter_map(|&item| {
                            matches!(hir.item(item).kind(), ItemKind::AssociatedType(_))
                                .then(|| module.item_def(item))
                                .flatten()
                                .map(CanonicalTarget::Source)
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                })
            }
            (DeclarationKind::Impl, Some(ItemKind::Impl(value))) => {
                self.resolve_source_impl(declaration_id, source, hir, value)?
            }
            (DeclarationKind::Struct, Some(ItemKind::Struct(value))) => {
                self.resolve_source_struct(declaration_id, source, hir, value)?
            }
            (DeclarationKind::Enum, Some(ItemKind::Enum(_))) => {
                Resolution::Known(DeclarationSignature::Enum {
                    generic_parameters: self.direct_generic_ids(declaration_id),
                    variants: self
                        .child_declarations(declaration_id, DeclarationKind::EnumVariant)
                        .into_iter()
                        .map(CanonicalTarget::Source)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                })
            }
            (DeclarationKind::EnumVariant, None) => {
                self.resolve_source_enum_variant(declaration_id, source, hir)?
            }
            (DeclarationKind::EnumVariantField, None) => {
                self.resolve_source_enum_field(declaration_id, source, hir)?
            }
            (DeclarationKind::TypeAlias, Some(ItemKind::TypeAlias(_))) => {
                match self.resolve_alias(declaration_id, None)? {
                    Resolution::Known(alias) => {
                        Resolution::Known(DeclarationSignature::TypeAlias {
                            generic_parameters: alias.generic_parameters,
                            value: alias.value,
                        })
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                }
            }
            (DeclarationKind::Const, Some(ItemKind::Const(value))) => {
                let context = self.source_context(declaration_id, false)?;
                self.resolve_source_type(
                    declaration_id,
                    source,
                    hir,
                    value.ty,
                    TypePosition::Ordinary,
                    &context,
                )?
                .map(|ty| DeclarationSignature::Const { ty })
            }
            (DeclarationKind::AssociatedType, Some(ItemKind::AssociatedType(_))) => {
                let Some(parent) = declaration.parent() else {
                    return Err(SignatureResolutionError::WrongDeclarationItem(
                        declaration_id,
                    ));
                };
                Resolution::Known(DeclarationSignature::AssociatedType {
                    owner_trait: CanonicalTarget::Source(parent),
                })
            }
            (DeclarationKind::AssociatedTypeBinding, Some(ItemKind::AssociatedTypeBinding(_))) => {
                return Ok(None);
            }
            (DeclarationKind::StorageField, Some(ItemKind::StorageField(value))) => {
                let mut poison = None;
                if value.syntax == eglur_hir::StorageFieldSyntax::Var {
                    let found = self.source_error(
                        source,
                        value.name.origin(),
                        "VAR_DECL_FORBIDDEN",
                        "var declarations are forbidden in v0.1",
                        BTreeMap::new(),
                    )?;
                    merge_poison(&mut poison, &found)?;
                }
                if value.initializer.is_some() && parent_kind == Some(DeclarationKind::Struct) {
                    let found = self.source_error(
                        source,
                        value.name.origin(),
                        "STRUCT_FIELD_DEFAULT_FORBIDDEN",
                        "struct field default initializer is forbidden in v0.1",
                        BTreeMap::new(),
                    )?;
                    merge_poison(&mut poison, &found)?;
                }
                if parent_kind == Some(DeclarationKind::Trait) {
                    let found = self.source_error(
                        source,
                        value.name.origin(),
                        "TRAIT_STATE_FORBIDDEN",
                        "trait state declarations are forbidden",
                        BTreeMap::new(),
                    )?;
                    merge_poison(&mut poison, &found)?;
                }
                let context = self.source_context(declaration_id, false)?;
                let resolved = self.resolve_source_type(
                    declaration_id,
                    source,
                    hir,
                    value.ty,
                    TypePosition::Ordinary,
                    &context,
                )?;
                merge_resolution_poison(
                    resolved.map(|ty| DeclarationSignature::Field { ty }),
                    poison,
                )?
            }
            (DeclarationKind::Property, Some(ItemKind::Property(value))) => {
                let mut poison = None;
                if parent_kind == Some(DeclarationKind::Trait) {
                    poison = Some(self.source_error(
                        source,
                        value.name.origin(),
                        "TRAIT_STATE_FORBIDDEN",
                        "trait state declarations are forbidden",
                        BTreeMap::new(),
                    )?);
                }
                let context = self.source_context(declaration_id, false)?;
                let resolved = self.resolve_source_type(
                    declaration_id,
                    source,
                    hir,
                    value.ty,
                    TypePosition::Ordinary,
                    &context,
                )?;
                merge_resolution_poison(
                    resolved.map(|ty| DeclarationSignature::Property { ty }),
                    poison,
                )?
            }
            _ => {
                return Err(SignatureResolutionError::WrongDeclarationItem(
                    declaration_id,
                ));
            }
        };
        Ok(resolved.into_fact())
    }

    pub(super) fn resolve_source_extern<'source>(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &'source HirFile,
        declaration: &'source eglur_hir::ExternDecl,
    ) -> Result<Resolution<DeclarationSignature>, SignatureResolutionError> {
        let mut poison = None;
        let visibility = self
            .collection
            .declaration(declaration_id)
            .expect("extern declaration belongs to collection")
            .visibility();
        if visibility.is_public() {
            let found = self.source_error(
                source,
                declaration.name.origin(),
                "EXTERN_VISIBILITY_FORBIDDEN",
                "pub extern visibility is forbidden in v0.1",
                BTreeMap::new(),
            )?;
            merge_poison(&mut poison, &found)?;
        }
        if declaration.abi.kind() == LiteralKind::Invalid {
            let found = source
                .invalid_poison(declaration.abi.origin())
                .cloned()
                .ok_or(SignatureResolutionError::MissingInvalidPoison {
                    file: source.file(),
                    origin: declaration.abi.origin(),
                })?;
            merge_poison(&mut poison, &found)?;
        } else if declaration.abi.kind() != LiteralKind::String
            || decode_string_literal(declaration.abi.spelling()).as_deref() != Some("C")
        {
            let found = self.source_error(
                source,
                declaration.abi.origin(),
                "EXTERN_ABI_UNSUPPORTED",
                "extern ABI is unsupported in v0.1",
                BTreeMap::new(),
            )?;
            merge_poison(&mut poison, &found)?;
        }
        let resolved = self.resolve_source_callable(
            declaration_id,
            source,
            hir,
            SourceCallable::Extern(declaration),
        )?;
        Ok(merge_resolution_poison(resolved, poison)?)
    }

    pub(super) fn resolve_source_callable<'source>(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &'source HirFile,
        declaration: SourceCallable<'source>,
    ) -> Result<Resolution<DeclarationSignature>, SignatureResolutionError> {
        let context = self.source_context(declaration_id, true)?;
        let name = declaration.name();
        let parameters = declaration.parameters();
        let return_type = declaration.return_type();
        let body = declaration.body();
        let (parameter_position, result_position) = declaration.type_positions();
        let body_hir = hir.body(body);
        let mut resolved_parameters = Vec::with_capacity(parameters.len());
        let mut poison = None;
        let mut unperformed = false;
        for parameter in parameters {
            let parameter_name = match body_hir.pattern(parameter.pattern) {
                Some(pattern) => match pattern.kind() {
                    PatternKind::Binding(name) if name.is_valid() => {
                        name.text().map(Box::<str>::from)
                    }
                    PatternKind::Binding(name) => {
                        let found = source.invalid_poison(name.origin()).cloned().ok_or(
                            SignatureResolutionError::MissingInvalidPoison {
                                file: source.file(),
                                origin: name.origin(),
                            },
                        )?;
                        merge_poison(&mut poison, &found)?;
                        None
                    }
                    PatternKind::Invalid => {
                        let found = source.invalid_poison(pattern.origin()).cloned().ok_or(
                            SignatureResolutionError::MissingInvalidPoison {
                                file: source.file(),
                                origin: pattern.origin(),
                            },
                        )?;
                        merge_poison(&mut poison, &found)?;
                        None
                    }
                    _ => {
                        let found = self.source_error(
                            source,
                            pattern.origin(),
                            "PARAM_PATTERN_UNSUPPORTED",
                            "parameter pattern is unsupported in v0.1",
                            BTreeMap::new(),
                        )?;
                        merge_poison(&mut poison, &found)?;
                        None
                    }
                },
                None => None,
            };
            let Some(type_ref) = parameter.ty else {
                let found = source.invalid_poison(parameter.origin).cloned().ok_or(
                    SignatureResolutionError::MissingInvalidPoison {
                        file: source.file(),
                        origin: parameter.origin,
                    },
                )?;
                merge_poison(&mut poison, &found)?;
                continue;
            };
            match self.resolve_source_type(
                declaration_id,
                source,
                hir,
                type_ref,
                parameter_position,
                &context,
            )? {
                Resolution::Known(ty) => resolved_parameters.push(CallableParameter {
                    name: parameter_name,
                    ty,
                    source_pattern: Some(parameter.pattern),
                }),
                Resolution::Poisoned(found) => merge_poison(&mut poison, &found)?,
                Resolution::Unperformed => unperformed = true,
            }
        }
        let result = if let Some(return_type) = return_type {
            match self.resolve_source_type(
                declaration_id,
                source,
                hir,
                return_type,
                result_position,
                &context,
            )? {
                Resolution::Known(ty) => Some(ty),
                Resolution::Poisoned(found) => {
                    merge_poison(&mut poison, &found)?;
                    None
                }
                Resolution::Unperformed => {
                    unperformed = true;
                    None
                }
            }
        } else {
            let found = self.source_error(
                source,
                name.origin(),
                "FN_RETURN_TYPE_REQUIRED",
                "named function return type is required",
                BTreeMap::new(),
            )?;
            merge_poison(&mut poison, &found)?;
            None
        };

        let is_method = self.is_method(declaration_id);
        let mut receiver_kind = None;
        if poison.is_none() && !unperformed && is_method {
            receiver_kind = method_receiver_kind(
                resolved_parameters.first(),
                context.self_binding.as_ref(),
                &self.types,
            )?;
            if receiver_kind.is_none()
                || resolved_parameters
                    .iter()
                    .skip(1)
                    .any(|parameter| parameter.name() == Some("self"))
            {
                poison = Some(self.source_error(
                    source,
                    name.origin(),
                    "METHOD_RECEIVER_INVALID",
                    "method receiver is invalid",
                    BTreeMap::from([(
                        Box::<str>::from("position"),
                        DiagnosticDataValue::from(0_i64),
                    )]),
                )?);
            }
        }
        if poison.is_none()
            && let Some(receiver_kind) = receiver_kind
            && ((!name.is_bang() && receiver_kind == MethodReceiverKind::Mutable)
                || (name.is_bang() && receiver_kind == MethodReceiverKind::Shared))
        {
            poison = Some(self.source_error(
                source,
                name.origin(),
                "EFFECT_SIGNATURE",
                "method effect signature requires a compatible `Self` or `&mut Self` receiver",
                BTreeMap::new(),
            )?);
        }
        if poison.is_none() && !unperformed && !name.is_bang() {
            for (index, parameter) in resolved_parameters
                .iter()
                .enumerate()
                .skip(usize::from(is_method))
            {
                if !matches!(
                    self.types.kind(parameter.ty)?,
                    TypeKind::Reference {
                        mutability: ReferenceMutability::Mutable,
                        ..
                    }
                ) {
                    continue;
                }
                let type_ref = parameters[index]
                    .ty
                    .expect("resolved callable parameter has a type occurrence");
                let found = self.source_error(
                    source,
                    hir.type_ref(type_ref).origin(),
                    "NON_BANG_MUT_PARAM_FORBIDDEN",
                    "non-bang callable `&mut` parameter is forbidden",
                    BTreeMap::new(),
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
        let result = result.expect("performed callable has a return type");
        let id = CallableSignatureId::from_index(self.callables.len())?;
        self.callables.push(CallableSignature {
            id,
            declaration: CanonicalTarget::Source(declaration_id),
            bang: name.is_bang(),
            generic_parameters: self.direct_generic_ids(declaration_id),
            parameters: resolved_parameters.into_boxed_slice(),
            result,
            requirements: Box::new([]),
            body: Some(body),
        });
        Ok(Resolution::Known(DeclarationSignature::Callable(id)))
    }

    pub(super) fn resolve_source_struct(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        value: &eglur_hir::StructDecl,
    ) -> Result<Resolution<DeclarationSignature>, SignatureResolutionError> {
        let mut poison = None;
        let mut unperformed = false;
        if let Some(base) = value.base {
            let context = self.source_context(declaration_id, false)?;
            let resolved = self.resolve_source_type(
                declaration_id,
                source,
                hir,
                base,
                TypePosition::Ordinary,
                &context,
            )?;
            match resolved {
                Resolution::Known(_) => {
                    let found = self.source_error(
                        source,
                        hir.type_ref(value.base.expect("base is present")).origin(),
                        "INHERITANCE_FORBIDDEN",
                        "inheritance is forbidden",
                        BTreeMap::new(),
                    )?;
                    merge_poison(&mut poison, &found)?;
                }
                Resolution::Poisoned(found) => {
                    merge_poison(&mut poison, &found)?;
                }
                Resolution::Unperformed => {
                    unperformed = true;
                }
            }
        }
        if let Some(poison) = poison {
            return Ok(Resolution::Poisoned(poison));
        }
        if unperformed {
            return Ok(Resolution::Unperformed);
        }
        let storage_fields = value
            .items
            .iter()
            .filter_map(|&item| {
                matches!(hir.item(item).kind(), ItemKind::StorageField(_))
                    .then(|| {
                        self.collection
                            .module(declaration_id.module())
                            .and_then(|module| module.item_def(item))
                            .map(CanonicalTarget::Source)
                    })
                    .flatten()
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let properties = value
            .items
            .iter()
            .filter_map(|&item| {
                matches!(hir.item(item).kind(), ItemKind::Property(_))
                    .then(|| {
                        self.collection
                            .module(declaration_id.module())
                            .and_then(|module| module.item_def(item))
                            .map(CanonicalTarget::Source)
                    })
                    .flatten()
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Ok(Resolution::Known(DeclarationSignature::Struct {
            generic_parameters: self.direct_generic_ids(declaration_id),
            storage_fields,
            properties,
        }))
    }

    pub(super) fn resolve_source_impl(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
        value: &eglur_hir::ImplDecl,
    ) -> Result<Resolution<DeclarationSignature>, SignatureResolutionError> {
        if let Some(poison) = self.impl_header_poison.get(&declaration_id).cloned() {
            self.poison_impl_bindings(declaration_id, &poison)?;
            return Ok(Resolution::Poisoned(poison));
        }
        let Some(header) = self.impl_headers.get(&declaration_id).cloned() else {
            return Ok(Resolution::Unperformed);
        };
        let bindings = self.resolve_source_impl_bindings(
            declaration_id,
            source,
            hir,
            value,
            header.trait_ref(),
            header.target(),
        )?;
        match bindings {
            Resolution::Known(bindings) => Ok(Resolution::Known(DeclarationSignature::Impl {
                associated_bindings: bindings.into_boxed_slice(),
            })),
            Resolution::Poisoned(poison) => Ok(Resolution::Poisoned(poison)),
            Resolution::Unperformed => Ok(Resolution::Unperformed),
        }
    }

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
        let mut pending_bindings = Vec::new();
        let mut poison = None;
        let mut unperformed = false;
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
            let associated = candidates[0].clone();
            let context = self.source_context(binding_id, true)?;
            match self.resolve_source_type(
                binding_id,
                source,
                hir,
                binding.value,
                TypePosition::Ordinary,
                &context,
            )? {
                Resolution::Known(ty) => {
                    let slot_error = if self.types.contains_ownership(ty)? {
                        Some((
                            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
                            "ownership type is forbidden in an associated type binding",
                        ))
                    } else if self.types.contains_reference(ty)? {
                        Some((
                            "REFERENCE_TYPE_POSITION_FORBIDDEN",
                            "reference type is forbidden in an associated type binding",
                        ))
                    } else {
                        None
                    };
                    if let Some((code, message)) = slot_error {
                        let found = self.source_error(
                            source,
                            hir.type_ref(binding.value).origin(),
                            code,
                            message,
                            BTreeMap::new(),
                        )?;
                        self.declarations.insert(
                            CanonicalTarget::Source(binding_id),
                            SemanticFact::Poisoned(found.clone()),
                        )?;
                        merge_poison(&mut poison, &found)?;
                    } else {
                        let signature = AssociatedBindingSignature {
                            declaration: CanonicalTarget::Source(binding_id),
                            associated,
                            value: ty,
                        };
                        pending_bindings.push((signature, hir.type_ref(binding.value).origin()));
                    }
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
        let cycle = self.associated_binding_cycle(source, trait_ref, target, &pending_bindings)?;
        let cycle_affected = if let Some((found, affected)) = cycle {
            merge_poison(&mut poison, &found)?;
            Some((found, affected))
        } else {
            None
        };
        let mut bindings = Vec::with_capacity(pending_bindings.len());
        for (signature, _) in pending_bindings {
            let fact = if let Some((found, affected)) = &cycle_affected
                && affected.contains(&signature.associated)
            {
                SemanticFact::Poisoned(found.clone())
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

    pub(super) fn associated_binding_cycle(
        &mut self,
        source: &SemanticSource<'hir>,
        trait_ref: &TraitRef,
        target: TypeId,
        bindings: &[(AssociatedBindingSignature, OriginId)],
    ) -> Result<Option<(Poison, BTreeSet<CanonicalTarget>)>, SignatureResolutionError> {
        let signatures = bindings
            .iter()
            .map(|(binding, _)| binding.clone())
            .collect::<Vec<_>>();
        let Some(cycles) =
            associated_binding_cycle_sets(&self.types, trait_ref, target, &signatures)?
        else {
            return Ok(None);
        };
        let (_, origin) = bindings
            .iter()
            .find(|(binding, _)| cycles.cyclic.contains(&binding.associated))
            .expect("cyclic associated target has a source binding");
        let poison = self.source_error(
            source,
            *origin,
            "ASSOCIATED_TYPE_CYCLE",
            "associated type binding contains a cycle",
            BTreeMap::new(),
        )?;
        Ok(Some((poison, cycles.affected)))
    }

    pub(super) fn poison_impl_bindings(
        &mut self,
        declaration_id: DefId,
        poison: &Poison,
    ) -> Result<(), SignatureResolutionError> {
        for binding in
            self.child_declarations(declaration_id, DeclarationKind::AssociatedTypeBinding)
        {
            self.declarations.insert(
                CanonicalTarget::Source(binding),
                SemanticFact::Poisoned(poison.clone()),
            )?;
        }
        Ok(())
    }

    pub(super) fn associated_mismatch_error(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        owner: DefId,
        trait_ref: &TraitRef,
        associated: &str,
        reason: &'static str,
    ) -> Result<Poison, SignatureResolutionError> {
        let trait_name =
            DiagnosticTypeRenderer::new(self.collection, &self.diagnostic_type_names, self, owner)
                .render_trait_ref(trait_ref)?;
        self.source_error(
            source,
            origin,
            "TRAIT_IMPL_ASSOC_TYPE_MISMATCH",
            format!("trait impl associated type mismatch: `{associated}` is {reason}"),
            BTreeMap::from([
                (
                    Box::<str>::from("trait"),
                    DiagnosticDataValue::from(trait_name),
                ),
                (
                    Box::<str>::from("associated_type"),
                    DiagnosticDataValue::from(associated),
                ),
                (
                    Box::<str>::from("reason"),
                    DiagnosticDataValue::from(reason),
                ),
            ]),
        )
    }

    pub(super) fn resolve_source_enum_variant(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
    ) -> Result<Resolution<DeclarationSignature>, SignatureResolutionError> {
        let declaration = self
            .collection
            .declaration(declaration_id)
            .expect("enum variant belongs to collection");
        let parent = declaration
            .parent()
            .ok_or(SignatureResolutionError::WrongDeclarationItem(
                declaration_id,
            ))?;
        let parent_declaration = self
            .collection
            .declaration(parent)
            .expect("enum variant parent belongs to collection");
        let item = parent_declaration
            .item()
            .ok_or(SignatureResolutionError::MissingDeclarationItem(parent))?;
        let ItemKind::Enum(parent_item) = hir.item(item).kind() else {
            return Err(SignatureResolutionError::WrongDeclarationItem(parent));
        };
        let variant = parent_item
            .variants
            .iter()
            .find(|variant| variant.origin == declaration.origin())
            .ok_or(SignatureResolutionError::WrongDeclarationItem(
                declaration_id,
            ))?;
        if !variant.name.is_valid() {
            return self.invalid_resolution(source, variant.name.origin());
        }
        if variant.visibility == eglur_hir::Visibility::Public {
            return Ok(Resolution::Poisoned(self.source_error(
                source,
                variant.origin,
                "ENUM_VARIANT_VISIBILITY_FORBIDDEN",
                "enum variant visibility modifiers are forbidden in v0.1",
                BTreeMap::new(),
            )?));
        }
        let context = self.source_context(parent, false)?;
        match &variant.payload {
            EnumVariantPayload::Absent => {
                Ok(Resolution::Known(DeclarationSignature::EnumVariant {
                    payload: EnumVariantPayloadSignature::Absent,
                }))
            }
            EnumVariantPayload::Tuple(types) => Ok(self
                .resolve_source_types(parent, source, hir, types, TypePosition::Ordinary, &context)?
                .map(|fields| DeclarationSignature::EnumVariant {
                    payload: EnumVariantPayloadSignature::Tuple(fields.into_boxed_slice()),
                })),
            EnumVariantPayload::Named(fields) => {
                let declarations =
                    self.child_declarations(declaration_id, DeclarationKind::EnumVariantField);
                let mut resolved = Vec::with_capacity(fields.len());
                let mut poison = None;
                let mut unperformed = false;
                for field in fields {
                    let field_id = declarations
                        .iter()
                        .copied()
                        .find(|&candidate| {
                            self.collection
                                .declaration(candidate)
                                .is_some_and(|declaration| declaration.origin() == field.origin)
                        })
                        .ok_or(SignatureResolutionError::WrongDeclarationItem(
                            declaration_id,
                        ))?;
                    match self.resolve_source_type(
                        parent,
                        source,
                        hir,
                        field.ty,
                        TypePosition::Ordinary,
                        &context,
                    )? {
                        Resolution::Known(ty) => {
                            resolved.push(CanonicalTarget::Source(field_id));
                            self.declarations.insert(
                                CanonicalTarget::Source(field_id),
                                SemanticFact::Known(DeclarationSignature::Field { ty }),
                            )?;
                        }
                        Resolution::Poisoned(found) => {
                            self.declarations.insert(
                                CanonicalTarget::Source(field_id),
                                SemanticFact::Poisoned(found.clone()),
                            )?;
                            merge_poison(&mut poison, &found)?;
                        }
                        Resolution::Unperformed => unperformed = true,
                    }
                }
                Ok(if let Some(poison) = poison {
                    Resolution::Poisoned(poison)
                } else if unperformed {
                    Resolution::Unperformed
                } else {
                    Resolution::Known(DeclarationSignature::EnumVariant {
                        payload: EnumVariantPayloadSignature::Named(resolved.into_boxed_slice()),
                    })
                })
            }
        }
    }

    pub(super) fn resolve_source_enum_field(
        &mut self,
        declaration_id: DefId,
        source: &SemanticSource<'hir>,
        hir: &HirFile,
    ) -> Result<Resolution<DeclarationSignature>, SignatureResolutionError> {
        let declaration = self
            .collection
            .declaration(declaration_id)
            .expect("enum field belongs to collection");
        let variant_id =
            declaration
                .parent()
                .ok_or(SignatureResolutionError::WrongDeclarationItem(
                    declaration_id,
                ))?;
        let variant = self
            .collection
            .declaration(variant_id)
            .expect("field variant belongs to collection");
        let enum_id = variant
            .parent()
            .ok_or(SignatureResolutionError::WrongDeclarationItem(variant_id))?;
        let enum_declaration = self
            .collection
            .declaration(enum_id)
            .expect("variant enum belongs to collection");
        let item = enum_declaration
            .item()
            .ok_or(SignatureResolutionError::MissingDeclarationItem(enum_id))?;
        let ItemKind::Enum(enum_item) = hir.item(item).kind() else {
            return Err(SignatureResolutionError::WrongDeclarationItem(enum_id));
        };
        let field = enum_item
            .variants
            .iter()
            .find(|candidate| candidate.origin == variant.origin())
            .and_then(|candidate| match &candidate.payload {
                EnumVariantPayload::Named(fields) => fields
                    .iter()
                    .find(|field| field.origin == declaration.origin()),
                _ => None,
            })
            .ok_or(SignatureResolutionError::WrongDeclarationItem(
                declaration_id,
            ))?;
        if !field.name.is_valid() {
            return self.invalid_resolution(source, field.name.origin());
        }
        let context = self.source_context(enum_id, false)?;
        self.resolve_source_type(
            enum_id,
            source,
            hir,
            field.ty,
            TypePosition::Ordinary,
            &context,
        )
        .map(|result| result.map(|ty| DeclarationSignature::Field { ty }))
    }

    pub(super) fn is_method(&self, declaration_id: DefId) -> bool {
        let mut parent = self
            .collection
            .declaration(declaration_id)
            .and_then(Declaration::parent);
        while let Some(candidate) = parent {
            let declaration = self
                .collection
                .declaration(candidate)
                .expect("method ancestor belongs to collection");
            if matches!(
                declaration.kind(),
                DeclarationKind::Struct | DeclarationKind::Trait | DeclarationKind::Impl
            ) {
                return true;
            }
            parent = declaration.parent();
        }
        false
    }

    pub(super) fn source_generic_parameter(
        &self,
        declaration_id: DefId,
    ) -> Result<(FileId, &'hir GenericParameter), SignatureResolutionError> {
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
        let owner_declaration = self
            .collection
            .declaration(owner)
            .expect("generic owner belongs to collection");
        let module = self
            .collection
            .module(owner.module())
            .expect("generic owner module belongs to collection");
        let hir = self.source_hir(module.file())?;
        let item = owner_declaration
            .item()
            .ok_or(SignatureResolutionError::MissingDeclarationItem(owner))?;
        let generics = item_generic_parameters(hir.item(item).kind())
            .ok_or(SignatureResolutionError::WrongDeclarationItem(owner))?;
        let declarations = self.child_declarations(owner, DeclarationKind::GenericParameter);
        let index = declarations
            .iter()
            .position(|&candidate| candidate == declaration_id)
            .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                declaration_id,
            ))?;
        let parameter =
            generics
                .get(index)
                .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                    declaration_id,
                ))?;
        Ok((module.file(), parameter))
    }

    pub(super) fn direct_generic_ids(&self, owner: DefId) -> Box<[GenericParameterId]> {
        self.child_declarations(owner, DeclarationKind::GenericParameter)
            .into_iter()
            .map(GenericParameterId::Source)
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub(super) fn child_declarations(&self, owner: DefId, kind: DeclarationKind) -> Vec<DefId> {
        self.collection
            .module(owner.module())
            .into_iter()
            .flat_map(|module| module.declarations())
            .filter(|declaration| declaration.parent() == Some(owner) && declaration.kind() == kind)
            .map(Declaration::id)
            .collect()
    }
}
