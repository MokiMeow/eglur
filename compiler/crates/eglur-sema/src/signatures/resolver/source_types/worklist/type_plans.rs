use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn type_child(
        parent: &TypeRequest,
        type_ref: TypeRefId,
        position: TypePosition,
    ) -> TypeRequest {
        TypeRequest {
            site: Rc::clone(&parent.site),
            type_ref,
            position,
        }
    }

    pub(super) fn type_children(
        parent: &TypeRequest,
        type_refs: &[TypeRefId],
        position: TypePosition,
    ) -> TypeChildren {
        TypeChildren::new(
            type_refs
                .iter()
                .map(|&type_ref| Self::type_child(parent, type_ref, position))
                .collect(),
        )
    }

    pub(super) fn await_type(child: TypeRequest, continuation: TypeContinuation) -> TypePlan {
        TypePlan::Await {
            request: Request::Type(child),
            continuation,
        }
    }

    pub(super) fn ready_type(
        result: Resolution<TypeId>,
        written: Option<CanonicalTarget>,
    ) -> TypePlan {
        TypePlan::Complete(TypeCompletion::Ready { result, written })
    }

    pub(super) fn position_type(
        result: Resolution<TypeId>,
        written: Option<CanonicalTarget>,
    ) -> TypePlan {
        TypePlan::Complete(TypeCompletion::ValidatePosition { result, written })
    }

    pub(super) fn complete_type_plan(
        &mut self,
        request: &TypeRequest,
        completion: TypeCompletion,
    ) -> Result<Resolution<TypeId>, SignatureResolutionError> {
        let source = self.sources[&request.site.file];
        let hir = self.source_hir(request.site.file)?;
        let origin = hir.type_ref(request.type_ref).origin();
        let (result, written) = match completion {
            TypeCompletion::Ready { result, written } => (result, written),
            TypeCompletion::ValidatePosition { result, written } => {
                let result = match result {
                    Resolution::Known(ty) => self.validate_source_position(
                        source,
                        origin,
                        ty,
                        request.position,
                        request.site.context.structure_validation,
                    )?,
                    other => other,
                };
                (result, written)
            }
        };
        self.record_type_occurrence(source, request.type_ref, origin, written, result)
    }

    pub(super) fn prepare_type_plan(
        &mut self,
        request: &TypeRequest,
    ) -> Result<TypePlan, SignatureResolutionError> {
        let source = self.sources[&request.site.file];
        let hir = self.source_hir(request.site.file)?;
        let occurrence = hir.type_ref(request.type_ref);
        let position = request.position;
        if position == TypePosition::ConstructorDType
            && let TypeRefKind::Path { path, arguments } = occurrence.kind()
            && arguments.is_empty()
            && let [name] = path.segments()
            && name.text() == Some("bool")
        {
            return Ok(Self::ready_type(
                Resolution::Known(self.types.primitive(crate::PrimitiveType::Bool)?),
                None,
            ));
        }
        match occurrence.kind() {
            TypeRefKind::Reference {
                mutability,
                referent,
            } => Ok(Self::await_type(
                Self::type_child(request, *referent, position.reference_referent()),
                TypeContinuation::Reference(*mutability),
            )),
            TypeRefKind::Pointer {
                mutability,
                referent,
            } => {
                if !position.is_extern() {
                    Ok(Self::position_type(
                        Resolution::Poisoned(self.source_error(
                            source,
                            occurrence.origin(),
                            "RAW_POINTER_TYPE_UNSUPPORTED",
                            "raw pointer type is unsupported in this type position",
                            BTreeMap::new(),
                        )?),
                        None,
                    ))
                } else {
                    Ok(Self::await_type(
                        Self::type_child(request, *referent, position.nested()),
                        TypeContinuation::Pointer(*mutability),
                    ))
                }
            }
            TypeRefKind::Group(inner) => Ok(Self::await_type(
                Self::type_child(request, *inner, position),
                TypeContinuation::Group,
            )),
            TypeRefKind::Tuple(elements) => {
                let children = Self::type_children(request, elements, position.nested());
                self.advance_type_children(request, TypeContinuation::Tuple(children))
            }
            TypeRefKind::Function { parameters, result } => {
                let mut children = parameters
                    .iter()
                    .map(|&id| Self::type_child(request, id, position.nested()))
                    .collect::<Vec<_>>();
                children.push(Self::type_child(request, *result, position.nested()));
                self.advance_type_children(
                    request,
                    TypeContinuation::Function(TypeChildren::new(children)),
                )
            }
            TypeRefKind::Dynamic { trait_ref } => Ok(TypePlan::Await {
                request: Request::Trait(TraitRequest {
                    site: Rc::clone(&request.site),
                    type_ref: *trait_ref,
                }),
                continuation: TypeContinuation::Dynamic,
            }),
            TypeRefKind::Path { path, arguments } => {
                self.prepare_path_plan(request, path, arguments, occurrence.origin())
            }
            TypeRefKind::Invalid => Ok(Self::position_type(
                self.invalid_resolution(source, occurrence.origin())?,
                None,
            )),
        }
    }

    pub(super) fn continue_type_plan(
        &mut self,
        frame: TypeFrame,
        complete: Completed,
    ) -> Result<TypePlan, SignatureResolutionError> {
        let source = self.sources[&frame.parent.site.file];
        let hir = self.source_hir(frame.parent.site.file)?;
        let origin = hir.type_ref(frame.parent.type_ref).origin();
        let position = frame.parent.position;
        match (frame.continuation, complete) {
            (TypeContinuation::Reference(mutability), Completed::Type(result)) => {
                Ok(Self::position_type(
                    result
                        .map(|referent| {
                            self.types
                                .reference(reference_mutability(mutability), referent)
                        })
                        .transpose()?,
                    None,
                ))
            }
            (TypeContinuation::Pointer(mutability), Completed::Type(result)) => {
                Ok(Self::position_type(
                    result
                        .map(|referent| {
                            self.types
                                .raw_pointer(raw_pointer_mutability(mutability), referent)
                        })
                        .transpose()?,
                    None,
                ))
            }
            (TypeContinuation::Group, Completed::Type(result)) => {
                Ok(Self::ready_type(result, None))
            }
            (TypeContinuation::Dynamic, Completed::Trait(nested)) => {
                let (code, message) = if position.is_extern() {
                    (
                        "EXTERN_SIGNATURE_UNSUPPORTED",
                        "dyn trait type is unsupported in an extern signature",
                    )
                } else {
                    ("DYN_TRAIT_UNSUPPORTED", "dyn trait type is unsupported")
                };
                let poison = self.source_error(source, origin, code, message, BTreeMap::new())?;
                let result = match nested {
                    Resolution::Poisoned(nested) => Resolution::Poisoned(nested.merge(&poison)?),
                    Resolution::Unperformed => Resolution::Unperformed,
                    Resolution::Known(_) => Resolution::Poisoned(poison),
                };
                Ok(Self::position_type(result, None))
            }
            (TypeContinuation::AliasTemplate { alias, arguments }, Completed::Alias(template)) => {
                let target = CanonicalTarget::Source(alias);
                let result = match template {
                    Resolution::Known(template) => {
                        self.retain_source_application(
                            frame.parent.site.owner,
                            source,
                            frame.parent.type_ref,
                            target.clone(),
                            arguments.clone(),
                            &frame.parent.site.context,
                        );
                        let replacements = template
                            .generic_parameters
                            .iter()
                            .cloned()
                            .zip(arguments)
                            .collect::<BTreeMap<_, _>>();
                        let ty = self.types.substitute(template.value, &replacements)?;
                        self.validate_source_position(
                            source,
                            origin,
                            ty,
                            position,
                            frame.parent.site.context.structure_validation,
                        )?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                Ok(Self::position_type(result, Some(target)))
            }
            (continuation, Completed::Type(result)) => {
                let mut continuation = continuation;
                match &mut continuation {
                    TypeContinuation::Tuple(children)
                    | TypeContinuation::Function(children)
                    | TypeContinuation::Intrinsic(_, children)
                    | TypeContinuation::Union(children)
                    | TypeContinuation::Nominal { children, .. }
                    | TypeContinuation::AliasArguments { children, .. } => {
                        children.accept(result)?
                    }
                    _ => unreachable!("type child has type-family continuation"),
                }
                self.advance_type_children(&frame.parent, continuation)
            }
            _ => unreachable!("continuation receives its requested result family"),
        }
    }

    pub(super) fn advance_type_children(
        &mut self,
        parent: &TypeRequest,
        continuation: TypeContinuation,
    ) -> Result<TypePlan, SignatureResolutionError> {
        let children = match &continuation {
            TypeContinuation::Tuple(children)
            | TypeContinuation::Function(children)
            | TypeContinuation::Intrinsic(_, children)
            | TypeContinuation::Union(children)
            | TypeContinuation::Nominal { children, .. }
            | TypeContinuation::AliasArguments { children, .. } => children,
            _ => unreachable!("only type-child families advance"),
        };
        if let Some(child) = children.next() {
            return Ok(Self::await_type(child, continuation));
        }
        let source = self.sources[&parent.site.file];
        let hir = self.source_hir(parent.site.file)?;
        let origin = hir.type_ref(parent.type_ref).origin();
        match continuation {
            TypeContinuation::Tuple(children) => Ok(Self::position_type(
                children
                    .finish()
                    .map(|elements| self.types.tuple(elements))
                    .transpose()?,
                None,
            )),
            TypeContinuation::Function(children) => Ok(Self::position_type(
                children
                    .finish()
                    .map(|mut values| {
                        let result = values.pop().expect("function result child");
                        self.types.callable(values, result)
                    })
                    .transpose()?,
                None,
            )),
            TypeContinuation::Intrinsic(head, children) => {
                let result = match children.finish() {
                    Resolution::Known(arguments) => {
                        let ty = self
                            .types
                            .nominal(NominalHead::Intrinsic(head), arguments)?;
                        self.validate_source_slot(
                            source,
                            origin,
                            ty,
                            parent.site.context.structure_validation,
                        )?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                Ok(Self::position_type(result, None))
            }
            TypeContinuation::Union(children) => {
                let result = match children.finish() {
                    Resolution::Known(members) => {
                        let ty = self.types.error_union(members)?;
                        self.validate_source_position(
                            source,
                            origin,
                            ty,
                            parent.position,
                            parent.site.context.structure_validation,
                        )?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                Ok(Self::position_type(result, None))
            }
            TypeContinuation::Nominal { target, children } => {
                let result = match children.finish() {
                    Resolution::Known(arguments) => {
                        self.retain_source_application(
                            parent.site.owner,
                            source,
                            parent.type_ref,
                            target.clone(),
                            arguments.clone(),
                            &parent.site.context,
                        );
                        let ty = self
                            .types
                            .nominal(NominalHead::Declaration(target.clone()), arguments)?;
                        self.validate_source_slot(
                            source,
                            origin,
                            ty,
                            parent.site.context.structure_validation,
                        )?
                    }
                    Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                    Resolution::Unperformed => Resolution::Unperformed,
                };
                Ok(Self::position_type(result, Some(target)))
            }
            TypeContinuation::AliasArguments { alias, children } => {
                let arguments = match children.finish() {
                    Resolution::Known(arguments) => {
                        match self.validate_source_abstraction_arguments(
                            source,
                            origin,
                            arguments,
                            parent.site.context.structure_validation,
                        )? {
                            Resolution::Known(arguments) => arguments,
                            Resolution::Poisoned(poison) => {
                                return Ok(Self::position_type(
                                    Resolution::Poisoned(poison),
                                    Some(CanonicalTarget::Source(alias)),
                                ));
                            }
                            Resolution::Unperformed => unreachable!(),
                        }
                    }
                    Resolution::Poisoned(poison) => {
                        return Ok(Self::position_type(
                            Resolution::Poisoned(poison),
                            Some(CanonicalTarget::Source(alias)),
                        ));
                    }
                    Resolution::Unperformed => {
                        return Ok(Self::position_type(
                            Resolution::Unperformed,
                            Some(CanonicalTarget::Source(alias)),
                        ));
                    }
                };
                Ok(TypePlan::Await {
                    request: Request::Alias(AliasRequest {
                        alias,
                        cycle_site: Some((parent.site.file, origin)),
                    }),
                    continuation: TypeContinuation::AliasTemplate { alias, arguments },
                })
            }
            _ => unreachable!("only type-child families advance"),
        }
    }
}
