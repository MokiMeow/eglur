use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn complete_trait_plan(
        &mut self,
        request: &TraitRequest,
        result: Resolution<TraitRef>,
        written: Option<CanonicalTarget>,
    ) -> Result<Resolution<TraitRef>, SignatureResolutionError> {
        let source = self.sources[&request.site.file];
        let hir = self.source_hir(request.site.file)?;
        let origin = hir.type_ref(request.type_ref).origin();
        self.record_trait_occurrence(source, request.type_ref, origin, written, result)
    }

    pub(super) fn prepare_trait_plan(
        &mut self,
        request: &TraitRequest,
    ) -> Result<TraitPlan, SignatureResolutionError> {
        let site = &request.site;
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
            return Ok(TraitPlan::complete(result, None));
        };
        let target = match self.lookup_type_target(site.owner.module(), source, path)? {
            Resolution::Known(target) => target,
            Resolution::Poisoned(poison) => {
                return Ok(TraitPlan::complete(Resolution::Poisoned(poison), None));
            }
            Resolution::Unperformed => {
                return Ok(TraitPlan::complete(Resolution::Unperformed, None));
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
            return Ok(TraitPlan::complete(Resolution::Poisoned(poison), None));
        }
        match self.source_head_state(&target)? {
            Resolution::Known(()) => {}
            Resolution::Poisoned(poison) => {
                return Ok(TraitPlan::complete(
                    Resolution::Poisoned(poison),
                    Some(target),
                ));
            }
            Resolution::Unperformed => {
                return Ok(TraitPlan::complete(Resolution::Unperformed, Some(target)));
            }
        }
        let Some(info) = self.trait_info.get(&target).cloned() else {
            return match &target {
                CanonicalTarget::External(_) => {
                    Ok(TraitPlan::complete(Resolution::Unperformed, Some(target)))
                }
                CanonicalTarget::Source(_) => {
                    Err(SignatureResolutionError::MissingTrustedSignature(target))
                }
            };
        };
        if arguments.len() != info.arity {
            let poison =
                self.record_arity_error(source, occurrence.origin(), info.arity, arguments.len())?;
            return Ok(TraitPlan::complete(Resolution::Poisoned(poison), None));
        }
        let parent = TypeRequest {
            site: Rc::clone(&request.site),
            type_ref: request.type_ref,
            position: TypePosition::Ordinary,
        };
        let children = Self::type_children(&parent, arguments, TypePosition::Ordinary);
        self.advance_trait_children(request, TraitContinuation::Arguments { target, children })
    }

    pub(super) fn continue_trait_plan(
        &mut self,
        frame: TraitFrame,
        result: Resolution<TypeId>,
    ) -> Result<TraitPlan, SignatureResolutionError> {
        match frame.continuation {
            TraitContinuation::Arguments {
                target,
                mut children,
            } => {
                children.accept(result)?;
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
    ) -> Result<TraitPlan, SignatureResolutionError> {
        let TraitContinuation::Arguments { target, children } = continuation;
        if let Some(request) = children.next() {
            return Ok(TraitPlan::await_type(
                request,
                TraitContinuation::Arguments { target, children },
            ));
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
                    self.retain_source_application(
                        parent.site.owner,
                        source,
                        parent.type_ref,
                        target.clone(),
                        arguments.clone(),
                        &parent.site.context,
                    );
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
