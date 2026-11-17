use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn prepare_path_plan(
        &mut self,
        request: &TypeRequest,
        path: &eglur_hir::Path,
        arguments: &[TypeRefId],
        origin: OriginId,
    ) -> Result<TypePlan, SignatureResolutionError> {
        let site = &request.site;
        let source = self.sources[&site.file];
        let context = &site.context;
        let position = request.position;
        let Some(path_text) = path.text() else {
            return Ok(Self::position_type(
                self.invalid_path_resolution(source, path)?,
                None,
            ));
        };
        let segments = path.segments();
        let generic_projection = segments.len() > 1
            && segments[0]
                .text()
                .is_some_and(|name| context.generics.contains_key(name))
            && !self.has_known_module_prefix(site.owner.module(), path);
        if segments.len() > 1 && (segments[0].text() == Some("Self") || generic_projection) {
            let (result, written) =
                self.resolve_source_projection(source, origin, path, arguments, context)?;
            return Ok(Self::position_type(result, written));
        }
        if segments.len() == 1 {
            if path_text == "Never" {
                let result = if arguments.is_empty() {
                    Resolution::Known(self.types.never())
                } else {
                    Resolution::Poisoned(self.record_arity_error(
                        source,
                        origin,
                        0,
                        arguments.len(),
                    )?)
                };
                return Ok(Self::position_type(result, None));
            }
            if let Some(intrinsic) = IntrinsicNominal::from_spelling(&path_text) {
                return self.prepare_intrinsic_plan(request, intrinsic, arguments, origin);
            }
            if path_text == "Union" {
                if arguments.len() < 2 {
                    let poison = self.source_error(
                        source,
                        origin,
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
                    return Ok(Self::position_type(Resolution::Poisoned(poison), None));
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
                        self.source_error(source, origin, code, message, BTreeMap::new())?;
                    return Ok(Self::position_type(Resolution::Poisoned(poison), None));
                }
                let children = Self::type_children(request, arguments, position.nested());
                return self.advance_type_children(request, TypeContinuation::Union(children));
            }
            if path_text == "Self" {
                if !arguments.is_empty() {
                    return Ok(Self::position_type(
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            origin,
                            0,
                            arguments.len(),
                        )?),
                        None,
                    ));
                }
                let Some(binding) = &context.self_binding else {
                    let poison = self.source_error(
                        source,
                        origin,
                        "SELF_TYPE_INVALID",
                        "Self type is invalid outside an owning method or associated binding",
                        BTreeMap::from([(
                            Box::<str>::from("position"),
                            DiagnosticDataValue::from("declaration_signature"),
                        )]),
                    )?;
                    return Ok(Self::position_type(Resolution::Poisoned(poison), None));
                };
                return Ok(Self::position_type(Resolution::Known(binding.ty), None));
            }
            if let Some(parameter) = context.generics.get(path_text.as_str()) {
                let written = Some(generic_target(parameter));
                let result = if arguments.is_empty() {
                    Resolution::Known(self.generic_types[parameter])
                } else {
                    Resolution::Poisoned(self.record_arity_error(
                        source,
                        origin,
                        0,
                        arguments.len(),
                    )?)
                };
                return Ok(Self::position_type(result, written));
            }
        }

        if segments.len() > 1 {
            match self.is_concrete_projection(site.owner.module(), source, path)? {
                Resolution::Known(true) => {
                    return Ok(Self::position_type(
                        Resolution::Poisoned(self.projection_error(
                            source,
                            origin,
                            "concrete base",
                        )?),
                        None,
                    ));
                }
                Resolution::Known(false) => {}
                Resolution::Poisoned(poison) => {
                    return Ok(Self::position_type(Resolution::Poisoned(poison), None));
                }
                Resolution::Unperformed => {
                    return Ok(Self::position_type(Resolution::Unperformed, None));
                }
            }
        }
        let target = match self.lookup_type_target(site.owner.module(), source, path)? {
            Resolution::Known(target) => target,
            Resolution::Poisoned(poison) => {
                return Ok(Self::position_type(Resolution::Poisoned(poison), None));
            }
            Resolution::Unperformed => {
                return Ok(Self::position_type(Resolution::Unperformed, None));
            }
        };
        if let CanonicalTarget::Primitive(primitive) = target {
            let result = if arguments.is_empty() {
                Resolution::Known(self.types.primitive(primitive)?)
            } else {
                Resolution::Poisoned(self.record_arity_error(source, origin, 0, arguments.len())?)
            };
            return Ok(Self::position_type(result, Some(target)));
        }
        if let CanonicalTarget::Intrinsic(intrinsic) = target {
            return self.prepare_intrinsic_plan(request, intrinsic, arguments, origin);
        }
        let kind = self.target_kind(&target)?;
        match self.source_head_state(&target)? {
            Resolution::Known(()) => {}
            Resolution::Poisoned(poison) => {
                return Ok(Self::position_type(
                    Resolution::Poisoned(poison),
                    Some(target),
                ));
            }
            Resolution::Unperformed => {
                return Ok(Self::position_type(Resolution::Unperformed, Some(target)));
            }
        }
        match kind {
            DeclarationKind::Struct | DeclarationKind::Enum | DeclarationKind::OpaqueType => {
                let head = NominalHead::Declaration(target.clone());
                let Some(expected) = self.types.nominal_arity(&head) else {
                    return match target {
                        CanonicalTarget::External(_) => {
                            Ok(Self::position_type(Resolution::Unperformed, Some(target)))
                        }
                        CanonicalTarget::Primitive(primitive) => {
                            Err(SignatureResolutionError::UnexpectedPrimitiveTarget {
                                primitive,
                                expected: "a nominal declaration",
                            })
                        }
                        CanonicalTarget::Intrinsic(intrinsic) => {
                            Err(SignatureResolutionError::UnexpectedIntrinsicTarget {
                                intrinsic,
                                expected: "a nominal declaration",
                            })
                        }
                        CanonicalTarget::Source(_) => {
                            Err(SignatureResolutionError::MissingTrustedSignature(target))
                        }
                    };
                };
                if arguments.len() != expected {
                    return Ok(Self::position_type(
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            origin,
                            expected,
                            arguments.len(),
                        )?),
                        Some(target),
                    ));
                }
                let children = Self::type_children(request, arguments, position.nested());
                self.advance_type_children(request, TypeContinuation::Nominal { target, children })
            }
            DeclarationKind::TypeAlias => {
                let CanonicalTarget::Source(alias) = target else {
                    return Ok(Self::position_type(Resolution::Unperformed, Some(target)));
                };
                let expected = self.alias_arities[&alias];
                if arguments.len() != expected {
                    return Ok(Self::position_type(
                        Resolution::Poisoned(self.record_arity_error(
                            source,
                            origin,
                            expected,
                            arguments.len(),
                        )?),
                        Some(CanonicalTarget::Source(alias)),
                    ));
                }
                let children = Self::type_children(request, arguments, position.nested());
                self.advance_type_children(
                    request,
                    TypeContinuation::AliasArguments { alias, children },
                )
            }
            DeclarationKind::Trait => Ok(Self::position_type(
                Resolution::Poisoned(self.source_error(
                    source,
                    origin,
                    "DYN_MUST_BE_EXPLICIT",
                    format!("dyn must be explicit when using trait `{path_text}` as a type"),
                    BTreeMap::new(),
                )?),
                Some(target),
            )),
            _ => Ok(Self::position_type(
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

    fn prepare_intrinsic_plan(
        &mut self,
        request: &TypeRequest,
        intrinsic: IntrinsicNominal,
        arguments: &[TypeRefId],
        origin: OriginId,
    ) -> Result<TypePlan, SignatureResolutionError> {
        if arguments.len() != intrinsic.arity() {
            let source = self.sources[&request.site.file];
            return Ok(Self::position_type(
                Resolution::Poisoned(self.record_arity_error(
                    source,
                    origin,
                    intrinsic.arity(),
                    arguments.len(),
                )?),
                None,
            ));
        }
        let children = TypeChildren::new(
            arguments
                .iter()
                .enumerate()
                .map(|(index, &id)| {
                    Self::type_child(
                        request,
                        id,
                        request.position.intrinsic_argument(intrinsic, index),
                    )
                })
                .collect(),
        );
        self.advance_type_children(request, TypeContinuation::Intrinsic(intrinsic, children))
    }
}
