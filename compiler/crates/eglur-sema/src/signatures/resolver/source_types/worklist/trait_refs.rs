use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn complete_trait_plan(
        &mut self,
        request: &TraitRequest,
        result: Resolution<TraitRef>,
        written: Option<CanonicalTarget>,
    ) -> Result<Resolution<TraitRef>, SignatureFlow> {
        let source = self.sources[&request.site.file];
        let hir = self.source_hir(request.site.file)?;
        let origin = hir.type_ref(request.type_ref).origin();
        if request.site.construction.is_some() {
            Ok(result)
        } else {
            self.record_trait_occurrence(source, request.type_ref, origin, written, result)
        }
    }

    pub(super) fn prepare_trait_plan(
        &mut self,
        request: &TraitRequest,
    ) -> Result<TraitPlan, SignatureFlow> {
        let target = match self.prepare_trait_head(request)? {
            Resolution::Known(target) => target,
            Resolution::Poisoned(poison) => {
                return Ok(TraitPlan::complete(Resolution::Poisoned(poison), None));
            }
            Resolution::Unperformed => {
                return Ok(TraitPlan::complete(Resolution::Unperformed, None));
            }
        };
        let hir = self.source_hir(request.site.file)?;
        let TypeRefKind::Path { arguments, .. } = hir.type_ref(request.type_ref).kind() else {
            unreachable!("prepared trait head is a path");
        };
        let parent = TypeRequest {
            site: Rc::clone(&request.site),
            type_ref: request.type_ref,
            position: TypePosition::Ordinary,
        };
        let children = Self::type_children(&parent, arguments, TypePosition::Ordinary);
        self.advance_trait_children(request, TraitContinuation::Arguments { target, children })
    }

    pub(super) fn prepare_trait_head(
        &mut self,
        request: &TraitRequest,
    ) -> Result<Resolution<CanonicalTarget>, SignatureFlow> {
        let site = &request.site;
        if let Some(result) = self
            .source_bounds
            .head_results
            .get(&(site.file, request.type_ref))
        {
            return Ok(result.clone());
        }
        let source = self.sources[&site.file];
        let hir = self.source_hir(site.file)?;
        let occurrence = hir.type_ref(request.type_ref);
        let TypeRefKind::Path { path, arguments } = occurrence.kind() else {
            let result = if matches!(occurrence.kind(), TypeRefKind::Invalid) {
                self.invalid_resolution(source, occurrence.origin())?
            } else {
                Resolution::Poisoned(self.source_error(
                    source,
                    occurrence.origin(),
                    "TRAIT_BOUND_INVALID",
                    "trait bound is invalid because it is not a nominal trait path",
                    BTreeMap::from([(
                        Box::<str>::from("reason"),
                        DiagnosticDataValue::from("not_trait_path"),
                    )]),
                )?)
            };
            return Ok(result);
        };
        let target = match self.lookup_type_target(site.owner.module(), source, path)? {
            Resolution::Known(target) => target,
            Resolution::Poisoned(poison) => {
                return Ok(Resolution::Poisoned(poison));
            }
            Resolution::Unperformed => {
                return Ok(Resolution::Unperformed);
            }
        };
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
            return Ok(Resolution::Poisoned(poison));
        }
        match self.source_head_state(&target)? {
            Resolution::Known(()) => {}
            Resolution::Poisoned(poison) => {
                return Ok(Resolution::Poisoned(poison));
            }
            Resolution::Unperformed => {
                return Ok(Resolution::Unperformed);
            }
        }
        let Some(info) = self.trait_info.get(&target) else {
            return match &target {
                CanonicalTarget::External(_) => Ok(Resolution::Unperformed),
                CanonicalTarget::Primitive(primitive) => Err(SignatureFlow::Internal(
                    SignatureResolutionError::UnexpectedPrimitiveTarget {
                        primitive: *primitive,
                        expected: "a trait declaration",
                    },
                )),
                CanonicalTarget::Intrinsic(intrinsic) => Err(SignatureFlow::Internal(
                    SignatureResolutionError::UnexpectedIntrinsicTarget {
                        intrinsic: *intrinsic,
                        expected: "a trait declaration",
                    },
                )),
                CanonicalTarget::Source(_) => Err(SignatureFlow::Internal(
                    SignatureResolutionError::MissingTrustedSignature(target),
                )),
            };
        };
        if arguments.len() != info.arity {
            let poison =
                self.record_arity_error(source, occurrence.origin(), info.arity, arguments.len())?;
            return Ok(Resolution::Poisoned(poison));
        }
        Ok(Resolution::Known(target))
    }

    pub(super) fn continue_trait_plan(
        &mut self,
        frame: TraitFrame,
        result: Completed,
    ) -> Result<TraitPlan, SignatureFlow> {
        match frame.continuation {
            TraitContinuation::Arguments {
                target,
                mut children,
            } => {
                match result {
                    Completed::Type(result) => children.accept(result)?,
                    Completed::Blocked(poison) => {
                        children.blocked = true;
                        if let Some(poison) = poison {
                            children.poison.include(&poison)?;
                        }
                        children.next += 1;
                    }
                    _ => unreachable!("trait argument completes as a type"),
                }
                self.advance_trait_children(
                    &frame.parent,
                    TraitContinuation::Arguments { target, children },
                )
            }
        }
    }

    fn advance_trait_children(
        &mut self,
        parent: &TraitRequest,
        continuation: TraitContinuation,
    ) -> Result<TraitPlan, SignatureFlow> {
        let TraitContinuation::Arguments { target, children } = continuation;
        if let Some(request) = children.next() {
            return Ok(TraitPlan::await_type(
                request,
                TraitContinuation::Arguments { target, children },
            ));
        }
        if children.blocked {
            return Ok(TraitPlan::Blocked(children.poison.finish()));
        }
        let source = self.sources[&parent.site.file];
        let hir = self.source_hir(parent.site.file)?;
        let origin = hir.type_ref(parent.type_ref).origin();
        let result = match children.finish() {
            Resolution::Known(arguments) => match self.validate_source_abstraction_arguments(
                source,
                origin,
                arguments,
                parent.site.context.structure_validation,
            )? {
                Resolution::Known(arguments) => {
                    if parent.site.construction.is_none() {
                        self.retain_source_application(
                            parent.site.owner,
                            source,
                            parent.type_ref,
                            target.clone(),
                            arguments.clone(),
                            &parent.site.context,
                        );
                    }
                    let substitution = self.types.intern_substitution(arguments)?;
                    Resolution::Known(TraitRef::new(target.clone(), substitution))
                }
                Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                Resolution::Unperformed => unreachable!(),
            },
            Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
            Resolution::Unperformed => Resolution::Unperformed,
        };
        let written = match &result {
            Resolution::Known(reference) => Some(reference.declaration().clone()),
            _ => None,
        };
        Ok(TraitPlan::complete(result, written))
    }
}
