use super::*;
use crate::signatures::resolver::source_bounds::{
    ConstructedReference, ConstructionId, ConstructionKey, ConstructionState, DeclaringBound,
    DeclaringSelection, ProjectionSite, ReferenceCohort,
};

pub(super) struct CohortFrame {
    id: ConstructionId,
    site: Rc<SourceSite>,
    slots: Rc<[ConstructionId]>,
    next: usize,
    first: Option<TraitRef>,
    multiple: bool,
    poison: PoisonAccumulator,
    unperformed: bool,
    blocked: bool,
}

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(in crate::signatures::resolver) fn resolve_bound_worklist(
        &mut self,
        id: ConstructionId,
    ) -> Result<(), SignatureFlow> {
        let ConstructionKey::Bound(parameter, type_ref) = self.source_bounds.key(id) else {
            unreachable!("bound entry selects a bound node");
        };
        let (file, _) = self.source_generic_parameter(parameter)?;
        let custody = self.original_source_custody(TypeOccurrenceId::new(file, type_ref))?;
        match self.run_source_worklist(Request::Bound { id, custody })? {
            Completed::Trait(_) | Completed::Blocked(_) => Ok(()),
            _ => unreachable!("bound worklist returns a trait result"),
        }
    }

    pub(in crate::signatures::resolver) fn source_bound_head(
        &mut self,
        parameter: DefId,
        type_ref: TypeRefId,
    ) -> Result<Resolution<CanonicalTarget>, SignatureFlow> {
        let owner = self
            .collection
            .declaration(parameter)
            .and_then(Declaration::parent)
            .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                parameter,
            ))?;
        let (file, _) = self.source_generic_parameter(parameter)?;
        let custody = self.original_source_custody(TypeOccurrenceId::new(file, type_ref))?;
        self.claim_source_custody(&custody)?;
        let context = self.source_context(owner, false)?;
        let request = TraitRequest {
            site: Rc::new(SourceSite {
                owner,
                file,
                context,
                custody,
                construction: None,
            }),
            type_ref,
        };
        self.prepare_trait_head(&request)
    }

    pub(super) fn enter_bound_construction(
        &mut self,
        id: ConstructionId,
        custody: SourceCustody,
        pending: &mut Vec<Step>,
        completed: &mut Option<Completed>,
    ) -> Result<(), SignatureFlow> {
        self.claim_source_custody(&custody)?;
        match self.source_bounds.state(id).clone() {
            ConstructionState::Complete(result) => {
                *completed = Some(Completed::Trait(result.map(|value| match value {
                    ConstructedReference::Bound(reference) => reference,
                    _ => unreachable!("bound node stores a bound reference"),
                })));
            }
            ConstructionState::Active => *completed = Some(Completed::Blocked(None)),
            ConstructionState::Blocked(poison) => *completed = Some(Completed::Blocked(poison)),
            ConstructionState::Pending => {
                #[cfg(test)]
                {
                    self.source_bounds.work.bound_starts += 1;
                }
                self.source_bounds.set_state(id, ConstructionState::Active);
                let ConstructionKey::Bound(parameter, type_ref) = self.source_bounds.key(id) else {
                    unreachable!("bound request selects a bound node");
                };
                let owner = self
                    .collection
                    .declaration(parameter)
                    .and_then(Declaration::parent)
                    .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                        parameter,
                    ))?;
                let (file, _) = self.source_generic_parameter(parameter)?;
                let context = self.source_context(owner, false)?;
                let request = TraitRequest {
                    site: Rc::new(SourceSite {
                        owner,
                        file,
                        context,
                        custody,
                        construction: Some(id),
                    }),
                    type_ref,
                };
                pending.push(Step::ResumeBound { id });
                pending.push(Step::Enter(Request::Trait(request)));
            }
        }
        Ok(())
    }

    pub(super) fn finish_bound_construction(&mut self, id: ConstructionId, result: &Completed) {
        let state = match result {
            Completed::Trait(result) => {
                ConstructionState::Complete(result.clone().map(ConstructedReference::Bound))
            }
            Completed::Blocked(poison) => ConstructionState::Blocked(poison.clone()),
            _ => unreachable!("bound construction completes as a trait reference"),
        };
        self.source_bounds.set_state(id, state);
    }

    pub(in crate::signatures::resolver) fn resolve_cohort_worklist(
        &mut self,
        id: ConstructionId,
    ) -> Result<(), SignatureFlow> {
        if !matches!(self.source_bounds.state(id), ConstructionState::Pending) {
            return Ok(());
        }
        let ConstructionKey::Cohort(index) = self.source_bounds.key(id) else {
            unreachable!("cohort key");
        };
        let first = self.source_bounds.cohorts[index][0];
        let ConstructionKey::Bound(parameter, type_ref) = self.source_bounds.key(first) else {
            unreachable!("cohort slot");
        };
        let owner = self
            .collection
            .declaration(parameter)
            .and_then(Declaration::parent)
            .ok_or(SignatureResolutionError::MissingGenericDeclaration(
                parameter,
            ))?;
        let (file, _) = self.source_generic_parameter(parameter)?;
        let context = self.source_context(owner, false)?;
        let custody = self.original_source_custody(TypeOccurrenceId::new(file, type_ref))?;
        let site = Rc::new(SourceSite {
            owner,
            file,
            context,
            custody,
            construction: Some(id),
        });
        match self.run_source_worklist(Request::Cohort { id, site })? {
            Completed::Cohort(_) | Completed::Blocked(_) => Ok(()),
            _ => unreachable!("cohort worklist returns a cohort"),
        }
    }

    pub(super) fn enter_cohort(
        &mut self,
        id: ConstructionId,
        site: Rc<SourceSite>,
        pending: &mut Vec<Step>,
        completed: &mut Option<Completed>,
    ) -> Result<(), SignatureFlow> {
        self.claim_source_custody(&site.custody)?;
        match self.source_bounds.state(id).clone() {
            ConstructionState::Complete(result) => {
                *completed = Some(Completed::Cohort(result.map(|value| match value {
                    ConstructedReference::Cohort(value) => value,
                    _ => unreachable!("cohort node stores a cohort"),
                })));
            }
            ConstructionState::Active => *completed = Some(Completed::Blocked(None)),
            ConstructionState::Blocked(poison) => *completed = Some(Completed::Blocked(poison)),
            ConstructionState::Pending => {
                #[cfg(test)]
                {
                    self.source_bounds.work.cohort_starts += 1;
                }
                self.source_bounds.set_state(id, ConstructionState::Active);
                let ConstructionKey::Cohort(index) = self.source_bounds.key(id) else {
                    unreachable!("cohort key");
                };
                let frame = CohortFrame {
                    id,
                    site,
                    slots: Rc::clone(&self.source_bounds.cohorts[index]),
                    next: 0,
                    first: None,
                    multiple: false,
                    poison: PoisonAccumulator::new(),
                    unperformed: false,
                    blocked: false,
                };
                self.advance_cohort(frame, pending, completed)?;
            }
        }
        Ok(())
    }

    pub(super) fn continue_cohort(
        &mut self,
        mut frame: CohortFrame,
        result: Completed,
        pending: &mut Vec<Step>,
        completed: &mut Option<Completed>,
    ) -> Result<(), SignatureFlow> {
        match result {
            Completed::Trait(Resolution::Known(reference)) => {
                if let Some(first) = &frame.first {
                    frame.multiple |= *first != reference;
                } else {
                    frame.first = Some(reference);
                }
            }
            Completed::Trait(Resolution::Poisoned(poison)) => frame.poison.include(&poison)?,
            Completed::Trait(Resolution::Unperformed) => frame.unperformed = true,
            Completed::Blocked(poison) => {
                frame.blocked = true;
                if let Some(poison) = poison {
                    frame.poison.include(&poison)?;
                }
            }
            _ => unreachable!("cohort child is a trait reference"),
        }
        frame.next += 1;
        self.advance_cohort(frame, pending, completed)
    }

    fn advance_cohort(
        &mut self,
        frame: CohortFrame,
        pending: &mut Vec<Step>,
        completed: &mut Option<Completed>,
    ) -> Result<(), SignatureFlow> {
        if let Some(&id) = frame.slots.get(frame.next) {
            self.source_bounds.depend(frame.id, id, None);
            let custody = self.inherited_source_custody(frame.site.file, &frame.site.custody)?;
            pending.push(Step::ResumeCohort(frame));
            pending.push(Step::Enter(Request::Bound { id, custody }));
            return Ok(());
        }
        if frame.blocked {
            let poison = frame.poison.finish();
            self.source_bounds
                .set_state(frame.id, ConstructionState::Blocked(poison.clone()));
            *completed = Some(Completed::Blocked(poison));
            return Ok(());
        }
        let result = if let Some(poison) = frame.poison.finish() {
            Resolution::Poisoned(poison)
        } else if frame.unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(if frame.multiple {
                ReferenceCohort::Multiple
            } else {
                ReferenceCohort::Unique(frame.first.expect("cohort is nonempty"))
            })
        };
        self.source_bounds.set_state(
            frame.id,
            ConstructionState::Complete(result.clone().map(ConstructedReference::Cohort)),
        );
        *completed = Some(Completed::Cohort(result));
        Ok(())
    }

    pub(super) fn prepare_projection_construction(
        &mut self,
        request: &TypeRequest,
        path: &eglur_hir::Path,
        arguments: &[TypeRefId],
        origin: OriginId,
    ) -> Result<TypePlan, SignatureFlow> {
        let source = self.sources[&request.site.file];
        if path.segments().len() != 2 {
            return Ok(Self::position_type(
                Resolution::Poisoned(self.projection_error(source, origin, "chained")?),
                None,
            ));
        }
        let name = path.segments()[1].text().unwrap_or("<invalid>");
        let parameter = request
            .site
            .context
            .generics
            .get(path.segments()[0].text().unwrap_or("<invalid>"))
            .expect("classified generic projection")
            .clone();
        let GenericParameterId::Source(parameter_id) = parameter else {
            return Ok(Self::position_type(Resolution::Unperformed, None));
        };
        let selected =
            match self.select_source_declaring_bound(parameter_id, source, origin, name)? {
                Resolution::Known(selected) => selected,
                Resolution::Poisoned(poison) => {
                    return Ok(Self::position_type(Resolution::Poisoned(poison), None));
                }
                Resolution::Unperformed => {
                    return Ok(Self::position_type(Resolution::Unperformed, None));
                }
            };
        self.source_bounds.depend(
            request.site.construction.expect("construction owner"),
            selected.cohort,
            Some(ProjectionSite {
                file: request.site.file,
                origin,
            }),
        );
        Ok(TypePlan::Await {
            request: Request::Cohort {
                id: selected.cohort,
                site: Rc::clone(&request.site),
            },
            continuation: TypeContinuation::Projection {
                base: self.generic_types[&parameter],
                associated: selected.associated,
                argument_count: arguments.len(),
            },
        })
    }

    pub(in crate::signatures::resolver) fn select_source_declaring_bound(
        &mut self,
        parameter: DefId,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        name: &str,
    ) -> Result<Resolution<DeclaringBound>, SignatureFlow> {
        Ok(
            match self
                .source_bounds
                .declaring_bounds(parameter, name, &self.trait_info)
            {
                Resolution::Known(DeclaringSelection::Unique(selected)) => {
                    Resolution::Known(selected)
                }
                Resolution::Known(selection) => Resolution::Poisoned(self.projection_error(
                    source,
                    origin,
                    if matches!(selection, DeclaringSelection::Missing) {
                        "no declaring bound"
                    } else {
                        "ambiguous declaring bounds"
                    },
                )?),
                Resolution::Poisoned(poison) => Resolution::Poisoned(poison),
                Resolution::Unperformed => Resolution::Unperformed,
            },
        )
    }

    pub(in crate::signatures::resolver) fn complete_bound_projection(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        base: TypeId,
        associated: CanonicalTarget,
        argument_count: usize,
        result: Resolution<ReferenceCohort>,
    ) -> Result<(Resolution<TypeId>, Option<CanonicalTarget>), SignatureFlow> {
        let reference = match result {
            Resolution::Known(ReferenceCohort::Unique(reference)) => reference,
            Resolution::Known(ReferenceCohort::Multiple) => {
                return Ok((
                    Resolution::Poisoned(self.projection_error(
                        source,
                        origin,
                        "ambiguous declaring bounds",
                    )?),
                    None,
                ));
            }
            Resolution::Poisoned(poison) => return Ok((Resolution::Poisoned(poison), None)),
            Resolution::Unperformed => return Ok((Resolution::Unperformed, None)),
        };
        if argument_count != 0 {
            return Ok((
                Resolution::Poisoned(self.record_arity_error(source, origin, 0, argument_count)?),
                Some(associated),
            ));
        }
        let ty = self.types.associated_projection(AssociatedProjection::new(
            base,
            reference,
            associated.clone(),
        ))?;
        Ok((Resolution::Known(ty), Some(associated)))
    }

    pub(super) fn complete_projection_construction(
        &mut self,
        request: &TypeRequest,
        base: TypeId,
        associated: CanonicalTarget,
        argument_count: usize,
        result: Resolution<ReferenceCohort>,
    ) -> Result<TypePlan, SignatureFlow> {
        let source = self.sources[&request.site.file];
        let origin = self
            .source_hir(request.site.file)?
            .type_ref(request.type_ref)
            .origin();
        let (result, written) = self.complete_bound_projection(
            source,
            origin,
            base,
            associated,
            argument_count,
            result,
        )?;
        Ok(Self::position_type(result, written))
    }

    pub(super) fn prepare_construction_alias(
        &mut self,
        request: &TypeRequest,
        alias: DefId,
        origin: OriginId,
    ) -> Result<TypePlan, SignatureFlow> {
        let id = self.source_bounds.node(ConstructionKey::Alias(alias));
        self.source_bounds.depend(
            request
                .site
                .construction
                .expect("construction alias has an owner"),
            id,
            None,
        );
        Ok(TypePlan::Await {
            request: Request::Alias(AliasRequest {
                alias,
                cycle_site: Some((request.site.file, origin)),
                custody: AliasCustody::Inherited(
                    self.inherited_source_custody(request.site.file, &request.site.custody)?,
                ),
                construction: Some(id),
            }),
            continuation: TypeContinuation::ConstructionAliasTemplate { alias },
        })
    }

    pub(super) fn construct_alias_arguments(
        &mut self,
        parent: &TypeRequest,
        alias: DefId,
        result: Resolution<AliasTemplate>,
    ) -> Result<TypePlan, SignatureFlow> {
        let template = match result {
            Resolution::Known(template) => template,
            Resolution::Poisoned(poison) => {
                return Ok(Self::position_type(Resolution::Poisoned(poison), None));
            }
            Resolution::Unperformed => {
                return Ok(Self::position_type(Resolution::Unperformed, None));
            }
        };
        let parameters = self.construction_alias_parameters(alias, &template)?;
        let hir = self.source_hir(parent.site.file)?;
        let TypeRefKind::Path { arguments, .. } = hir.type_ref(parent.type_ref).kind() else {
            unreachable!("alias application is a source path");
        };
        let requests = parameters
            .iter()
            .map(|(ordinal, _)| {
                Self::type_child(parent, arguments[*ordinal], parent.position.nested())
            })
            .collect();
        self.advance_type_children(
            parent,
            TypeContinuation::ConstructionAliasArguments {
                template,
                parameters,
                children: TypeChildren::new(requests),
            },
        )
    }

    pub(super) fn complete_alias_frame(
        &mut self,
        frame: AliasFrame,
        result: Completed,
    ) -> Result<Completed, SignatureFlow> {
        if let Some(id) = frame.site.construction {
            let (state, complete) = match result {
                Completed::Type(result) => {
                    let result = result.map(|value| AliasTemplate {
                        generic_parameters: frame.generic_parameters,
                        value,
                    });
                    (
                        ConstructionState::Complete(
                            result.clone().map(ConstructedReference::Alias),
                        ),
                        Completed::Alias(result),
                    )
                }
                Completed::Blocked(poison) => (
                    ConstructionState::Blocked(poison.clone()),
                    Completed::Blocked(poison),
                ),
                _ => unreachable!("alias body completes as a type"),
            };
            self.source_bounds.set_state(id, state);
            Ok(complete)
        } else {
            let Completed::Type(result) = result else {
                unreachable!("validated alias body completes as a type");
            };
            Ok(Completed::Alias(self.finish_alias_plan(frame, result)?))
        }
    }
}
