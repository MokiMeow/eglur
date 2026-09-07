use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn new(
        collection: &'input DeclarationCollection,
        imports: &'input ResolvedImports,
        sources: BTreeMap<FileId, &'input SemanticSource<'hir>>,
        deferred_entry_source: Option<FileId>,
        external_names: Option<&'input ExternalNameEnvironment>,
        trusted_inputs: Option<TrustedDeclarationInputs<'input>>,
        diagnostics: &'input mut DiagnosticStore,
    ) -> Result<Self, SignatureResolutionError> {
        let (trusted, trusted_inhabitance, trusted_language_items) = trusted_inputs
            .map(|inputs| {
                (
                    Some(inputs.signatures),
                    inputs.inhabitance,
                    inputs.language_items,
                )
            })
            .unwrap_or((None, None, None));
        if trusted.is_some() && external_names.is_none() {
            return Err(SignatureResolutionError::MissingExternalNames);
        }
        debug_assert!(trusted_inhabitance.is_none() || trusted.is_some());
        debug_assert!(trusted_language_items.is_none() || trusted.is_some());
        let diagnostic_type_names =
            DiagnosticTypeNames::new(collection, imports, external_names, trusted)?;
        let mut resolver = Self {
            collection,
            imports,
            sources,
            deferred_entry_source,
            external_names,
            trusted,
            trusted_inhabitance,
            trusted_language_items,
            diagnostics,
            types: CanonicalTypeStore::new()?,
            diagnostic_type_names,
            declarations: FactTable::new(),
            overload_family_poisons: BTreeMap::new(),
            generic_parameters: FactTable::new(),
            occurrences: FactTable::new(),
            callables: Vec::new(),
            trait_methods: BTreeMap::new(),
            trait_method_owners: BTreeMap::new(),
            declared_impl_methods: BTreeMap::new(),
            trait_adapters: BTreeMap::new(),
            prelude_traits: BTreeSet::new(),
            generic_types: BTreeMap::new(),
            generic_bounds: BTreeMap::new(),
            source_generic_redeclarations: BTreeSet::new(),
            source_generic_bound_seeds: BTreeMap::new(),
            source_generic_states: BTreeMap::new(),
            source_generic_stack: Vec::new(),
            trait_info: BTreeMap::new(),
            alias_arities: BTreeMap::new(),
            alias_states: BTreeMap::new(),
            impl_headers: BTreeMap::new(),
            impl_header_poison: BTreeMap::new(),
            retained_impl_headers: FactTable::new(),
        };
        resolver.register_trusted_heads()?;
        resolver.register_source_heads()?;
        resolver.source_generic_redeclarations = resolver.classify_source_generic_redeclarations();
        resolver.register_generic_types()?;
        Ok(resolver)
    }

    pub(super) fn finish(self) -> DeclarationSignatures {
        DeclarationSignatures {
            types: self.types,
            diagnostic_type_names: self.diagnostic_type_names,
            declarations: self.declarations,
            overload_family_poisons: self.overload_family_poisons,
            generic_parameters: self.generic_parameters,
            occurrences: self.occurrences,
            callables: self.callables.into_boxed_slice(),
            impl_headers: self.retained_impl_headers,
            trait_methods: self.trait_methods,
            trait_method_owners: self.trait_method_owners,
            declared_impl_methods: self.declared_impl_methods,
            trait_adapters: self.trait_adapters,
            prelude_traits: self
                .prelude_traits
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            language_items: LanguageItems::from_trusted(self.trusted_language_items),
        }
    }

    pub(super) fn validate_language_items(&self) -> Result<(), SignatureResolutionError> {
        let Some(items) = self.trusted_language_items else {
            return Ok(());
        };
        for (item, declaration) in items.items() {
            let target = CanonicalTarget::External(declaration.clone());
            let FactState::Known(signature) = self.declarations.state(&target) else {
                return Err(SignatureResolutionError::MissingLanguageItemSignature {
                    item,
                    declaration: declaration.clone(),
                });
            };
            if !self.language_item_signature_matches(item, signature)? {
                return Err(SignatureResolutionError::InvalidLanguageItemSignature {
                    item,
                    declaration: declaration.clone(),
                });
            }
        }
        Ok(())
    }

    pub(super) fn language_item_signature_matches(
        &self,
        item: LanguageItem,
        signature: &DeclarationSignature,
    ) -> Result<bool, SignatureResolutionError> {
        if item == LanguageItem::NumericError {
            return Ok(matches!(
                signature,
                DeclarationSignature::Nominal { generic_parameters }
                    if generic_parameters.is_empty()
            ));
        }
        let DeclarationSignature::Constructor(constructor) = signature else {
            return Ok(false);
        };
        let expected_head = match item {
            LanguageItem::OptSome | LanguageItem::OptNone => IntrinsicNominal::Opt,
            LanguageItem::ResOk | LanguageItem::ResErr => IntrinsicNominal::Res,
            LanguageItem::NumericError => unreachable!("numeric error was handled above"),
        };
        let TypeKind::Nominal {
            head: NominalHead::Intrinsic(head),
            substitution,
        } = self.types.kind(constructor.result())?
        else {
            return Ok(false);
        };
        let parameters = constructor.generic_parameters();
        let result_arguments = self.types.substitution(*substitution)?;
        if *head != expected_head
            || parameters.len() != expected_head.arity()
            || result_arguments.len() != parameters.len()
        {
            return Ok(false);
        }
        for (argument, parameter) in result_arguments.iter().zip(parameters) {
            if !matches!(
                self.types.kind(*argument)?,
                TypeKind::GenericParameter(actual) if actual == parameter
            ) {
                return Ok(false);
            }
        }
        let payload_parameter = match item {
            LanguageItem::OptNone => None,
            LanguageItem::OptSome | LanguageItem::ResOk => Some(0),
            LanguageItem::ResErr => Some(1),
            LanguageItem::NumericError => unreachable!("numeric error was handled above"),
        };
        match (payload_parameter, constructor.payload()) {
            (None, ConstructorPayloadSignature::Unit) => Ok(true),
            (Some(index), ConstructorPayloadSignature::Positional(values)) if values.len() == 1 => {
                Ok(matches!(
                    self.types.kind(values[0])?,
                    TypeKind::GenericParameter(actual) if actual == &parameters[index]
                ))
            }
            _ => Ok(false),
        }
    }

    pub(super) fn resolve_trusted_inhabitance(
        &mut self,
    ) -> Result<Vec<(ExternalDefId, NominalInhabitanceRule)>, SignatureResolutionError> {
        let Some(environment) = self.trusted_inhabitance else {
            return Ok(Vec::new());
        };
        let signatures = self
            .trusted
            .expect("trusted inhabitance requires trusted signatures");
        let mut resolved = Vec::with_capacity(environment.facts().len());
        for fact in environment.facts() {
            let declaration = signatures.declaration(fact.declaration()).ok_or_else(|| {
                SignatureResolutionError::MissingTrustedInhabitanceSignature(
                    fact.declaration().clone(),
                )
            })?;
            let TrustedSignatureKind::Nominal { generic_parameters } = declaration.kind() else {
                return Err(SignatureResolutionError::TrustedInhabitanceOnNonNominal(
                    fact.declaration().clone(),
                ));
            };
            let generic_count = generic_parameters.len();
            let rule = match fact.rule() {
                TrustedNominalInhabitance::Unknown => NominalInhabitanceRule::Unknown,
                TrustedNominalInhabitance::Inhabited => NominalInhabitanceRule::Inhabited,
                TrustedNominalInhabitance::Product(members) => {
                    let members = members
                        .iter()
                        .map(|member| {
                            self.resolve_trusted_type(
                                fact.declaration(),
                                generic_count,
                                member,
                                TypePosition::Ordinary,
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    NominalInhabitanceRule::Product(members.into_boxed_slice())
                }
                TrustedNominalInhabitance::Sum(products) => {
                    let products = products
                        .iter()
                        .map(|product| {
                            product
                                .members()
                                .iter()
                                .map(|member| {
                                    self.resolve_trusted_type(
                                        fact.declaration(),
                                        generic_count,
                                        member,
                                        TypePosition::Ordinary,
                                    )
                                })
                                .collect::<Result<Vec<_>, _>>()
                                .map(NominalInhabitanceProduct::new)
                        })
                        .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                    NominalInhabitanceRule::Sum(products.into_boxed_slice())
                }
            };
            resolved.push((fact.declaration().clone(), rule));
        }
        Ok(resolved)
    }

    pub(super) fn register_trusted_heads(&mut self) -> Result<(), SignatureResolutionError> {
        let Some(trusted) = self.trusted else {
            return Ok(());
        };
        for declaration in trusted.declarations() {
            match declaration.kind() {
                TrustedSignatureKind::Nominal { generic_parameters } => {
                    self.require_external_kind(
                        declaration.id(),
                        &[
                            DeclarationKind::Struct,
                            DeclarationKind::Enum,
                            DeclarationKind::OpaqueType,
                        ],
                    )?;
                    self.types.register_nominal_head(
                        NominalHead::Declaration(CanonicalTarget::External(
                            declaration.id().clone(),
                        )),
                        generic_parameters.len(),
                    )?;
                }
                TrustedSignatureKind::Trait {
                    generic_parameters,
                    associated_types,
                    closed,
                } => {
                    self.require_external_kind(declaration.id(), &[DeclarationKind::Trait])?;
                    let associated = freeze_associated(associated_types.iter().map(|associated| {
                        (
                            associated.name.clone(),
                            CanonicalTarget::External(associated.id.clone()),
                        )
                    }));
                    self.trait_info.insert(
                        CanonicalTarget::External(declaration.id().clone()),
                        TraitInfo {
                            arity: generic_parameters.len(),
                            closed: *closed,
                            associated,
                        },
                    );
                    let trait_target = CanonicalTarget::External(declaration.id().clone());
                    let methods = declaration
                        .trait_methods()
                        .iter()
                        .map(|method| {
                            let method_target =
                                CanonicalTarget::External(method.declaration.clone());
                            self.trait_method_owners
                                .insert(method_target.clone(), trait_target.clone());
                            TraitMethodSignature::new(
                                method_target,
                                method.name.clone(),
                                method.has_default,
                            )
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice();
                    self.trait_methods.insert(trait_target.clone(), methods);
                    if let Some(names) = self.external_names {
                        for (namespace, name, declarations) in names.prelude_bindings() {
                            if namespace == Namespace::Type
                                && declarations.contains(declaration.id())
                            {
                                self.prelude_traits.insert(PreludeTraitSignature::new(
                                    name.clone(),
                                    trait_target.clone(),
                                ));
                            }
                        }
                    }
                }
                TrustedSignatureKind::Callable { .. } => {
                    if !trusted.is_hidden_callable(declaration.id()) {
                        self.require_external_kind(
                            declaration.id(),
                            &[DeclarationKind::Function, DeclarationKind::ExternFunction],
                        )?;
                    }
                }
                TrustedSignatureKind::Constructor { .. } => {
                    self.require_external_kind(
                        declaration.id(),
                        &[DeclarationKind::IntrinsicConstructor],
                    )?;
                }
                TrustedSignatureKind::Impl { .. } => {}
            }
        }
        Ok(())
    }

    pub(super) fn register_source_heads(&mut self) -> Result<(), SignatureResolutionError> {
        for module in self.collection.modules() {
            let hir = self.source_hir(module.file())?;
            for declaration in module.declarations() {
                let Some(item_id) = declaration.item() else {
                    continue;
                };
                if declaration
                    .name()
                    .is_some_and(|name| introduces_reserved_name(declaration.kind(), name.text()))
                {
                    continue;
                }
                match hir.item(item_id).kind() {
                    ItemKind::Struct(value) => self.types.register_nominal_head(
                        NominalHead::Declaration(CanonicalTarget::Source(declaration.id())),
                        value.generic_parameters.len(),
                    )?,
                    ItemKind::Enum(value) => self.types.register_nominal_head(
                        NominalHead::Declaration(CanonicalTarget::Source(declaration.id())),
                        value.generic_parameters.len(),
                    )?,
                    ItemKind::Trait(value) => {
                        let trait_target = CanonicalTarget::Source(declaration.id());
                        let associated =
                            freeze_associated(value.items.iter().filter_map(|&child_id| {
                                let child = hir.item(child_id);
                                let ItemKind::AssociatedType(associated) = child.kind() else {
                                    return None;
                                };
                                let name = associated.name.text()?;
                                let target = module.item_def(child_id)?;
                                Some((Box::<str>::from(name), CanonicalTarget::Source(target)))
                            }));
                        self.trait_info.insert(
                            trait_target.clone(),
                            TraitInfo {
                                arity: value.generic_parameters.len(),
                                closed: value.closed,
                                associated,
                            },
                        );
                        let methods = value
                            .items
                            .iter()
                            .filter_map(|&child_id| {
                                let ItemKind::Function(function) = hir.item(child_id).kind() else {
                                    return None;
                                };
                                let method = module.item_def(child_id)?;
                                let name = self.collection.declaration(method)?.name()?.clone();
                                let method_target = CanonicalTarget::Source(method);
                                self.trait_method_owners
                                    .insert(method_target.clone(), trait_target.clone());
                                Some(TraitMethodSignature::new(
                                    method_target,
                                    name,
                                    !matches!(function.body_form, FunctionBody::Absent),
                                ))
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice();
                        self.trait_methods.insert(trait_target, methods);
                    }
                    ItemKind::TypeAlias(value) => {
                        self.alias_arities
                            .insert(declaration.id(), value.generic_parameters.len());
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub(super) fn register_generic_types(&mut self) -> Result<(), SignatureResolutionError> {
        if let Some(trusted) = self.trusted {
            for declaration in trusted.declarations() {
                let count = trusted_generic_parameters(declaration.kind()).len();
                for index in 0..count {
                    let index = u32::try_from(index).map_err(|_| {
                        SignatureResolutionError::Trusted(
                            TrustedSignatureError::GenericParameterOutOfRange {
                                declaration: declaration.id().clone(),
                                index: u32::MAX,
                            },
                        )
                    })?;
                    let id = GenericParameterId::Trusted {
                        owner: declaration.id().clone(),
                        index,
                    };
                    let ty = self.types.generic_parameter(id.clone())?;
                    self.generic_types.insert(id, ty);
                }
            }
        }
        for module in self.collection.modules() {
            for declaration in module
                .declarations()
                .filter(|declaration| declaration.kind() == DeclarationKind::GenericParameter)
            {
                if self
                    .source_generic_redeclarations
                    .contains(&declaration.id())
                {
                    continue;
                }
                let id = GenericParameterId::Source(declaration.id());
                let ty = self.types.generic_parameter(id.clone())?;
                self.generic_types.insert(id, ty);
            }
        }
        Ok(())
    }

    pub(super) fn require_external_kind(
        &self,
        id: &ExternalDefId,
        accepted: &[DeclarationKind],
    ) -> Result<(), SignatureResolutionError> {
        let declaration = self
            .external_names
            .and_then(|names| names.declaration(id))
            .ok_or_else(|| {
                SignatureResolutionError::Trusted(TrustedSignatureError::MissingNameDeclaration(
                    id.clone(),
                ))
            })?;
        if accepted.contains(&declaration.kind()) {
            Ok(())
        } else {
            Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::DeclarationKindMismatch(id.clone()),
            ))
        }
    }

    pub(super) fn prime_trusted_impl_headers(&mut self) -> Result<(), SignatureResolutionError> {
        let Some(trusted) = self.trusted else {
            return Ok(());
        };
        for declaration in trusted.declarations() {
            let TrustedSignatureKind::Impl {
                negative,
                trait_ref,
                target,
                ..
            } = declaration.kind()
            else {
                continue;
            };
            if *negative {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::NegativeImplUnsupported(declaration.id().clone()),
                ));
            }
            let generic_parameters = self.trusted_generic_ids(declaration);
            let trait_ref = self.resolve_trusted_trait_ref(
                declaration.id(),
                generic_parameters.len(),
                trait_ref,
            )?;
            let target = self.resolve_trusted_type(
                declaration.id(),
                generic_parameters.len(),
                target,
                TypePosition::Ordinary,
            )?;
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
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: declaration.id().clone(),
                        reason: "projection in impl header",
                    },
                ));
            }
            let declaration = CanonicalTarget::External(declaration.id().clone());
            let header = TraitImplHeader {
                declaration: declaration.clone(),
                generic_parameters,
                trait_ref,
                target,
            };
            if header_parameter_below_error_union(&self.types, &header)?.is_some() {
                let CanonicalTarget::External(declaration) = header.declaration() else {
                    unreachable!("trusted header has an external declaration")
                };
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: declaration.clone(),
                        reason: "generic parameter below structural error union in impl header",
                    },
                ));
            }
            self.retained_impl_headers
                .insert(declaration.clone(), SemanticFact::Known(header))?;
        }
        Ok(())
    }

    pub(super) fn validate_trusted_impl_headers(&self) -> Result<(), SignatureResolutionError> {
        let headers = self
            .retained_impl_headers
            .iter()
            .filter_map(|(target, fact)| {
                matches!(target, CanonicalTarget::External(_))
                    .then_some(fact)
                    .and_then(|fact| match fact {
                        SemanticFact::Known(header) => Some(header),
                        SemanticFact::Poisoned(_) | SemanticFact::NotApplicable => None,
                    })
            })
            .collect::<Vec<_>>();
        for header in &headers {
            for parameter in header.generic_parameters() {
                if !header_mentions_parameter(&self.types, header, parameter)? {
                    let CanonicalTarget::External(declaration) = header.declaration() else {
                        unreachable!("trusted header has an external declaration")
                    };
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::UnconstrainedImplParameter {
                            declaration: declaration.clone(),
                            parameter: parameter.clone(),
                        },
                    ));
                }
            }
        }
        for (index, left) in headers.iter().enumerate() {
            for right in &headers[index + 1..] {
                if classify_overlap(self, left, right)?.is_some() {
                    let CanonicalTarget::External(left) = left.declaration() else {
                        unreachable!("trusted header has an external declaration")
                    };
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::CoherenceViolation(left.clone()),
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn resolve_trusted(&mut self) -> Result<(), SignatureResolutionError> {
        let Some(trusted) = self.trusted else {
            return Ok(());
        };
        for declaration in trusted.declarations() {
            self.resolve_trusted_generics(declaration)?;
        }
        self.prime_trusted_impl_headers()?;
        self.validate_trusted_impl_headers()?;
        for declaration in trusted.declarations() {
            let target = CanonicalTarget::External(declaration.id().clone());
            let generic_parameters = self.trusted_generic_ids(declaration);
            if let Some(adapter) = declaration.trait_adapter() {
                self.trait_adapters.insert(
                    target.clone(),
                    TraitAdapterSignature::new(
                        CanonicalTarget::External(adapter.method.clone()),
                        usize::try_from(adapter.argument)
                            .expect("trusted adapter arguments fit the host index"),
                    ),
                );
            }
            if !declaration.impl_methods().is_empty() {
                self.declared_impl_methods.insert(
                    target.clone(),
                    declaration
                        .impl_methods()
                        .iter()
                        .map(|method| {
                            DeclaredImplMethod::new(
                                CanonicalTarget::External(method.trait_method.clone()),
                                CanonicalTarget::External(method.callable.clone()),
                            )
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
            }
            let signature = match declaration.kind() {
                TrustedSignatureKind::Nominal { .. } => {
                    DeclarationSignature::Nominal { generic_parameters }
                }
                TrustedSignatureKind::Trait {
                    closed,
                    associated_types,
                    ..
                } => {
                    for associated in associated_types {
                        self.declarations.insert(
                            CanonicalTarget::External(associated.id.clone()),
                            SemanticFact::Known(DeclarationSignature::AssociatedType {
                                owner_trait: target.clone(),
                            }),
                        )?;
                    }
                    DeclarationSignature::Trait {
                        closed: *closed,
                        generic_parameters,
                        associated_types: associated_types
                            .iter()
                            .map(|associated| CanonicalTarget::External(associated.id.clone()))
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    }
                }
                TrustedSignatureKind::Constructor {
                    payload, result, ..
                } => {
                    let owner_count = result_owner_count(declaration.kind());
                    let payload = match payload {
                        TrustedConstructorPayload::Unit => ConstructorPayloadSignature::Unit,
                        TrustedConstructorPayload::Positional(types) => {
                            ConstructorPayloadSignature::Positional(
                                types
                                    .iter()
                                    .map(|ty| {
                                        self.resolve_trusted_type(
                                            declaration.id(),
                                            owner_count,
                                            ty,
                                            TypePosition::CallableBoundary,
                                        )
                                    })
                                    .collect::<Result<Vec<_>, SignatureResolutionError>>()?
                                    .into_boxed_slice(),
                            )
                        }
                    };
                    let result = self.resolve_trusted_type(
                        declaration.id(),
                        owner_count,
                        result,
                        TypePosition::CallableBoundary,
                    )?;
                    DeclarationSignature::Constructor(ConstructorSignature {
                        declaration: target.clone(),
                        generic_parameters,
                        payload,
                        result,
                    })
                }
                TrustedSignatureKind::Callable {
                    bang,
                    parameters,
                    result,
                    requirements,
                    ..
                } => {
                    let external_kind = self
                        .external_names
                        .and_then(|names| names.declaration(declaration.id()))
                        .map(|external| external.kind());
                    let parameter_position =
                        if external_kind == Some(DeclarationKind::ExternFunction) {
                            TypePosition::ExternParameter
                        } else {
                            TypePosition::CallableBoundary
                        };
                    let result_position = if external_kind == Some(DeclarationKind::ExternFunction)
                    {
                        TypePosition::ExternReturn
                    } else {
                        TypePosition::CallableBoundary
                    };
                    let parameters = parameters
                        .iter()
                        .map(|parameter| {
                            Ok(CallableParameter {
                                name: parameter.name.clone(),
                                ty: self.resolve_trusted_type(
                                    declaration.id(),
                                    result_owner_count(declaration.kind()),
                                    &parameter.ty,
                                    parameter_position,
                                )?,
                                source_pattern: None,
                            })
                        })
                        .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                    let result = self.resolve_trusted_type(
                        declaration.id(),
                        result_owner_count(declaration.kind()),
                        result,
                        result_position,
                    )?;
                    let requirements = requirements
                        .iter()
                        .map(|requirement| match requirement {
                            TrustedCallableRequirement::FixedWidthInteger(ty) => self
                                .resolve_trusted_type(
                                    declaration.id(),
                                    result_owner_count(declaration.kind()),
                                    ty,
                                    TypePosition::CallableBoundary,
                                )
                                .map(CallableRequirement::FixedWidthInteger),
                        })
                        .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                    let id = CallableSignatureId::from_index(self.callables.len())?;
                    self.callables.push(CallableSignature {
                        id,
                        declaration: target.clone(),
                        bang: *bang,
                        generic_parameters,
                        parameters: parameters.into_boxed_slice(),
                        result,
                        requirements: requirements.into_boxed_slice(),
                        body: None,
                    });
                    DeclarationSignature::Callable(id)
                }
                TrustedSignatureKind::Impl {
                    associated_bindings,
                    ..
                } => {
                    let header = match self.retained_impl_headers.state(&target) {
                        FactState::Known(header) => header.clone(),
                        FactState::Poisoned(_)
                        | FactState::Unperformed
                        | FactState::NotApplicable => {
                            return Err(SignatureResolutionError::MissingTrustedSignature(
                                target.clone(),
                            ));
                        }
                    };
                    let trait_ref = header.trait_ref().clone();
                    let impl_target = header.target();
                    let info = self
                        .trait_info
                        .get(trait_ref.declaration())
                        .cloned()
                        .ok_or_else(|| {
                            SignatureResolutionError::Trusted(TrustedSignatureError::UnknownTrait(
                                match trait_ref.declaration() {
                                    CanonicalTarget::External(id) => id.clone(),
                                    CanonicalTarget::Source(_) => unreachable!(
                                        "trusted trait references cannot target source declarations"
                                    ),
                                },
                            ))
                        })?;
                    let expected = info
                        .associated
                        .values()
                        .flatten()
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    let mut seen = BTreeSet::new();
                    let mut bindings = Vec::with_capacity(associated_bindings.len());
                    for binding in associated_bindings {
                        let associated = CanonicalTarget::External(binding.associated.clone());
                        if !expected.contains(&associated) || !seen.insert(associated.clone()) {
                            return Err(SignatureResolutionError::Trusted(
                                TrustedSignatureError::UnknownAssociatedType(
                                    binding.associated.clone(),
                                ),
                            ));
                        }
                        let value = self.resolve_trusted_type(
                            declaration.id(),
                            generic_parameters.len(),
                            &binding.value,
                            TypePosition::Ordinary,
                        )?;
                        if self.types.contains_reference(value)? {
                            return Err(SignatureResolutionError::Trusted(
                                TrustedSignatureError::InvalidTypePosition {
                                    declaration: declaration.id().clone(),
                                    reason: "reference-bearing associated type binding",
                                },
                            ));
                        }
                        let signature = AssociatedBindingSignature {
                            declaration: CanonicalTarget::External(binding.declaration.clone()),
                            associated,
                            value,
                        };
                        bindings.push(signature);
                    }
                    if seen != expected {
                        let missing = expected
                            .difference(&seen)
                            .next()
                            .expect("unequal associated sets have one missing member");
                        let CanonicalTarget::External(missing) = missing else {
                            unreachable!("trusted traits have portable associated identities")
                        };
                        return Err(SignatureResolutionError::Trusted(
                            TrustedSignatureError::UnknownAssociatedType(missing.clone()),
                        ));
                    }
                    if associated_binding_cycle_sets(
                        &self.types,
                        &trait_ref,
                        impl_target,
                        &bindings,
                    )?
                    .is_some()
                    {
                        return Err(SignatureResolutionError::Trusted(
                            TrustedSignatureError::InvalidTypePosition {
                                declaration: declaration.id().clone(),
                                reason: "associated type binding cycle",
                            },
                        ));
                    }
                    for binding in &bindings {
                        self.declarations.insert(
                            binding.declaration.clone(),
                            SemanticFact::Known(DeclarationSignature::AssociatedTypeBinding(
                                binding.clone(),
                            )),
                        )?;
                    }
                    DeclarationSignature::Impl {
                        associated_bindings: bindings.into_boxed_slice(),
                    }
                }
            };
            self.declarations
                .insert(target, SemanticFact::Known(signature))?;
        }
        Ok(())
    }

    pub(super) fn resolve_trusted_generics(
        &mut self,
        declaration: &TrustedDeclarationSignature,
    ) -> Result<(), SignatureResolutionError> {
        let parameters = trusted_generic_parameters(declaration.kind());
        for (index, parameter) in parameters.iter().enumerate() {
            let index = u32::try_from(index).expect("trusted generic count already fit u32");
            let id = GenericParameterId::Trusted {
                owner: declaration.id().clone(),
                index,
            };
            let mut bounds = parameter
                .bounds
                .iter()
                .map(|bound| {
                    self.resolve_trusted_trait_ref(declaration.id(), parameters.len(), bound)
                })
                .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
            bounds.sort();
            let before = bounds.len();
            bounds.dedup();
            if bounds.len() != before {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::DuplicateBound {
                        declaration: declaration.id().clone(),
                        parameter: index,
                    },
                ));
            }
            let signature = GenericParameterSignature {
                id: id.clone(),
                name: parameter.name.clone(),
                bounds: bounds.into_boxed_slice(),
            };
            self.generic_bounds
                .insert(id.clone(), signature.bounds.clone());
            self.generic_parameters
                .insert(id, SemanticFact::Known(signature))?;
        }
        Ok(())
    }

    pub(super) fn trusted_generic_ids(
        &self,
        declaration: &TrustedDeclarationSignature,
    ) -> Box<[GenericParameterId]> {
        (0..trusted_generic_parameters(declaration.kind()).len())
            .map(|index| GenericParameterId::Trusted {
                owner: declaration.id().clone(),
                index: u32::try_from(index).expect("trusted generic count fit during registration"),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }

    pub(super) fn resolve_trusted_trait_ref(
        &mut self,
        owner: &ExternalDefId,
        generic_count: usize,
        reference: &TrustedTraitExpr,
    ) -> Result<TraitRef, SignatureResolutionError> {
        let target = CanonicalTarget::External(reference.declaration.clone());
        let info = self.trait_info.get(&target).cloned().ok_or_else(|| {
            SignatureResolutionError::Trusted(TrustedSignatureError::UnknownTrait(
                reference.declaration.clone(),
            ))
        })?;
        if reference.arguments.len() != info.arity {
            return Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::Arity {
                    declaration: reference.declaration.clone(),
                    expected: info.arity,
                    actual: reference.arguments.len(),
                },
            ));
        }
        let arguments = reference
            .arguments
            .iter()
            .map(|argument| {
                self.resolve_trusted_type(owner, generic_count, argument, TypePosition::Ordinary)
            })
            .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
        for &argument in &arguments {
            if self.types.contains_ownership(argument)? {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: owner.clone(),
                        reason: "ownership-bearing trait argument",
                    },
                ));
            }
            if self.types.contains_reference(argument)? {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: owner.clone(),
                        reason: "reference-bearing trait argument",
                    },
                ));
            }
        }
        let substitution = self.types.intern_substitution(arguments)?;
        Ok(TraitRef::new(target, substitution))
    }

    pub(super) fn resolve_trusted_type(
        &mut self,
        owner: &ExternalDefId,
        generic_count: usize,
        expression: &TrustedTypeExpr,
        position: TypePosition,
    ) -> Result<TypeId, SignatureResolutionError> {
        let ty = match expression {
            TrustedTypeExpr::Never => self.types.never(),
            TrustedTypeExpr::Primitive(primitive) => self.types.primitive(*primitive)?,
            TrustedTypeExpr::Intrinsic { head, arguments } => {
                let arguments = arguments
                    .iter()
                    .enumerate()
                    .map(|(index, argument)| {
                        let argument_position = position.intrinsic_argument(*head, index);
                        self.resolve_trusted_type(owner, generic_count, argument, argument_position)
                    })
                    .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                self.types
                    .nominal(NominalHead::Intrinsic(*head), arguments)?
            }
            TrustedTypeExpr::Nominal { head, arguments } => {
                let nominal = NominalHead::Declaration(CanonicalTarget::External(head.clone()));
                if self.types.nominal_arity(&nominal).is_none() {
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::UnknownTypeHead(head.clone()),
                    ));
                }
                let arguments = arguments
                    .iter()
                    .map(|argument| {
                        self.resolve_trusted_type(owner, generic_count, argument, position.nested())
                    })
                    .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                self.types.nominal(nominal, arguments)?
            }
            TrustedTypeExpr::GenericParameter(index) => {
                if usize::try_from(*index).map_or(true, |index| index >= generic_count) {
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::GenericParameterOutOfRange {
                            declaration: owner.clone(),
                            index: *index,
                        },
                    ));
                }
                self.generic_types[&GenericParameterId::Trusted {
                    owner: owner.clone(),
                    index: *index,
                }]
            }
            TrustedTypeExpr::SelfType(self_owner) => self
                .types
                .self_type(CanonicalTarget::External(self_owner.clone()))?,
            TrustedTypeExpr::Reference {
                mutability,
                referent,
            } => {
                let referent = self.resolve_trusted_type(
                    owner,
                    generic_count,
                    referent,
                    position.reference_referent(),
                )?;
                self.types.reference(*mutability, referent)?
            }
            TrustedTypeExpr::RawPointer {
                mutability,
                referent,
            } => {
                if !position.is_extern() {
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::InvalidTypePosition {
                            declaration: owner.clone(),
                            reason: "raw pointer outside an extern signature",
                        },
                    ));
                }
                let referent =
                    self.resolve_trusted_type(owner, generic_count, referent, position.nested())?;
                self.types.raw_pointer(*mutability, referent)?
            }
            TrustedTypeExpr::Tuple(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| {
                        self.resolve_trusted_type(owner, generic_count, element, position.nested())
                    })
                    .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                self.types.tuple(elements)?
            }
            TrustedTypeExpr::Callable { parameters, result } => {
                let parameters = parameters
                    .iter()
                    .map(|parameter| {
                        self.resolve_trusted_type(
                            owner,
                            generic_count,
                            parameter,
                            position.nested(),
                        )
                    })
                    .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                let result =
                    self.resolve_trusted_type(owner, generic_count, result, position.nested())?;
                self.types.callable(parameters, result)?
            }
            TrustedTypeExpr::ErrorUnion(members) => {
                if position != TypePosition::ErrorChannel {
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::InvalidTypePosition {
                            declaration: owner.clone(),
                            reason: if position.is_extern() {
                                "error union in an extern signature"
                            } else {
                                "error union outside a Res error channel"
                            },
                        },
                    ));
                }
                let members = members
                    .iter()
                    .map(|member| {
                        self.resolve_trusted_type(owner, generic_count, member, position.nested())
                    })
                    .collect::<Result<Vec<_>, SignatureResolutionError>>()?;
                self.types.error_union(members)?
            }
            TrustedTypeExpr::AssociatedProjection {
                base,
                trait_declaration,
                trait_arguments,
                associated,
            } => {
                let base =
                    self.resolve_trusted_type(owner, generic_count, base, position.nested())?;
                let trait_ref = self.resolve_trusted_trait_ref(
                    owner,
                    generic_count,
                    &TrustedTraitExpr {
                        declaration: trait_declaration.clone(),
                        arguments: trait_arguments.clone(),
                    },
                )?;
                let associated_target = CanonicalTarget::External(associated.clone());
                let info = self
                    .trait_info
                    .get(trait_ref.declaration())
                    .expect("trusted trait ref was validated");
                if !info
                    .associated
                    .values()
                    .flatten()
                    .any(|candidate| candidate == &associated_target)
                {
                    return Err(SignatureResolutionError::Trusted(
                        TrustedSignatureError::UnknownAssociatedType(associated.clone()),
                    ));
                }
                self.types.associated_projection(AssociatedProjection::new(
                    base,
                    trait_ref,
                    associated_target,
                ))?
            }
        };
        self.validate_trusted_slot(owner, ty)?;
        if !position.is_extern()
            && self.types.contains_ownership(ty)?
            && !self.ownership_allowed_at(ty, position)?
        {
            return Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::InvalidTypePosition {
                    declaration: owner.clone(),
                    reason: "ownership type outside a complete callable boundary",
                },
            ));
        }
        if matches!(
            position,
            TypePosition::ExternParameter | TypePosition::ExternReturn
        ) && !self.is_supported_extern_type(ty, position == TypePosition::ExternReturn)?
        {
            return Err(SignatureResolutionError::Trusted(
                TrustedSignatureError::InvalidTypePosition {
                    declaration: owner.clone(),
                    reason: "unsupported extern signature type",
                },
            ));
        }
        Ok(ty)
    }

    pub(super) fn validate_trusted_slot(
        &self,
        owner: &ExternalDefId,
        ty: TypeId,
    ) -> Result<(), SignatureResolutionError> {
        let TypeKind::Nominal { head, substitution } = self.types.kind(ty)? else {
            return Ok(());
        };
        let arguments = self.types.substitution(*substitution)?;
        let ordinary = match head {
            NominalHead::Intrinsic(IntrinsicNominal::Own) => arguments,
            NominalHead::Intrinsic(IntrinsicNominal::Opt)
                if self.types.ownership_root(ty)?.is_some() =>
            {
                return Ok(());
            }
            NominalHead::Intrinsic(IntrinsicNominal::Res)
                if self.types.ownership_root(ty)?.is_some() =>
            {
                &arguments[1..]
            }
            _ => arguments,
        };
        for &argument in ordinary {
            if self.types.contains_ownership(argument)? {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: owner.clone(),
                        reason: "ownership-bearing generic argument",
                    },
                ));
            }
            if self.types.contains_reference(argument)? {
                return Err(SignatureResolutionError::Trusted(
                    TrustedSignatureError::InvalidTypePosition {
                        declaration: owner.clone(),
                        reason: "reference-bearing generic argument",
                    },
                ));
            }
        }
        Ok(())
    }
}
