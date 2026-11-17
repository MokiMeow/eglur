use super::*;
use std::rc::Rc;

mod aliases;
mod construction;
mod path_types;
mod trait_refs;
mod type_plans;

#[cfg(test)]
pub(super) mod custody_tests;
#[cfg(test)]
mod poison_tests;

struct SourceSite {
    owner: DefId,
    file: FileId,
    context: SourceTypeContext,
    custody: SourceCustody,
    construction: Option<source_bounds::ConstructionId>,
}

#[derive(Clone)]
struct TypeRequest {
    site: Rc<SourceSite>,
    type_ref: TypeRefId,
    position: TypePosition,
}

#[derive(Clone)]
struct TraitRequest {
    site: Rc<SourceSite>,
    type_ref: TypeRefId,
}

struct AliasRequest {
    alias: DefId,
    cycle_site: Option<(FileId, OriginId)>,
    custody: AliasCustody,
    construction: Option<source_bounds::ConstructionId>,
}

enum AliasCustody {
    Original,
    Inherited(SourceCustody),
}

enum Request {
    Type(TypeRequest),
    Trait(TraitRequest),
    Alias(AliasRequest),
    Bound {
        id: source_bounds::ConstructionId,
        custody: SourceCustody,
    },
    Cohort {
        id: source_bounds::ConstructionId,
        site: Rc<SourceSite>,
    },
}

enum Completed {
    Type(Resolution<TypeId>),
    Trait(Resolution<TraitRef>),
    Alias(Resolution<AliasTemplate>),
    Blocked(Option<Poison>),
    Cohort(Resolution<source_bounds::ReferenceCohort>),
}

struct TypeChildren {
    requests: Vec<TypeRequest>,
    next: usize,
    values: Vec<TypeId>,
    poison: PoisonAccumulator,
    unperformed: bool,
    blocked: bool,
}

impl TypeChildren {
    fn new(requests: Vec<TypeRequest>) -> Self {
        Self {
            values: Vec::with_capacity(requests.len()),
            requests,
            next: 0,
            poison: PoisonAccumulator::new(),
            unperformed: false,
            blocked: false,
        }
    }

    fn next(&self) -> Option<TypeRequest> {
        self.requests.get(self.next).cloned()
    }

    fn accept(&mut self, result: Resolution<TypeId>) -> Result<(), SignatureFlow> {
        match result {
            Resolution::Known(ty) => self.values.push(ty),
            Resolution::Poisoned(found) => {
                #[cfg(test)]
                poison_tests::observe(super::poison_tests::Step::Input);

                self.poison.include(&found)?;
            }
            Resolution::Unperformed => self.unperformed = true,
        }
        self.next += 1;
        Ok(())
    }

    fn finish(self) -> Resolution<Vec<TypeId>> {
        assert!(
            !self.blocked,
            "blocked construction has no canonical child values"
        );
        if let Some(poison) = self.poison.finish() {
            #[cfg(test)]
            poison_tests::observe(super::poison_tests::Step::Complete);

            Resolution::Poisoned(poison)
        } else if self.unperformed {
            Resolution::Unperformed
        } else {
            Resolution::Known(self.values)
        }
    }
}

enum TypeContinuation {
    Reference(Mutability),
    Pointer(PointerMutability),
    Group,
    Dynamic,
    Tuple(TypeChildren),
    Function(TypeChildren),
    Intrinsic(IntrinsicNominal, TypeChildren),
    Union(TypeChildren),
    Nominal {
        target: CanonicalTarget,
        children: TypeChildren,
    },
    AliasArguments {
        alias: DefId,
        children: TypeChildren,
    },
    AliasTemplate {
        alias: DefId,
        arguments: Vec<TypeId>,
    },
    ConstructionAliasTemplate {
        alias: DefId,
    },
    ConstructionAliasArguments {
        template: AliasTemplate,
        parameters: std::rc::Rc<[(usize, GenericParameterId)]>,
        children: TypeChildren,
    },
    Projection {
        base: TypeId,
        associated: CanonicalTarget,
        argument_count: usize,
    },
}

enum TypeCompletion {
    Ready {
        result: Resolution<TypeId>,
        written: Option<CanonicalTarget>,
    },
    ValidatePosition {
        result: Resolution<TypeId>,
        written: Option<CanonicalTarget>,
    },
}

enum TypePlan {
    Blocked(Option<Poison>),
    Complete(TypeCompletion),
    Await {
        request: Request,
        continuation: TypeContinuation,
    },
}

struct TypeFrame {
    parent: TypeRequest,
    continuation: TypeContinuation,
}

enum TraitContinuation {
    Arguments {
        target: CanonicalTarget,
        children: TypeChildren,
    },
}

enum TraitPlan {
    Blocked(Option<Poison>),
    Complete {
        result: Resolution<TraitRef>,
        written: Option<CanonicalTarget>,
    },
    Await {
        request: TypeRequest,
        continuation: TraitContinuation,
    },
}

impl TraitPlan {
    fn complete(result: Resolution<TraitRef>, written: Option<CanonicalTarget>) -> Self {
        Self::Complete { result, written }
    }

    fn await_type(request: TypeRequest, continuation: TraitContinuation) -> Self {
        Self::Await {
            request,
            continuation,
        }
    }
}

struct TraitFrame {
    parent: TraitRequest,
    continuation: TraitContinuation,
}

enum AliasPlan {
    Blocked(Option<Poison>),
    Complete(Resolution<AliasTemplate>),
    Body {
        request: TypeRequest,
        generic_parameters: Box<[GenericParameterId]>,
    },
}

struct AliasFrame {
    alias: DefId,
    generic_parameters: Box<[GenericParameterId]>,
    site: Rc<SourceSite>,
}

enum Step {
    Enter(Request),
    ResumeType(TypeFrame),
    ResumeTrait(TraitFrame),
    ResumeAlias(AliasFrame),
    ResumeBound { id: source_bounds::ConstructionId },
    ResumeCohort(construction::CohortFrame),
}

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn resolve_type_worklist(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        type_ref: TypeRefId,
        position: TypePosition,
        context: &SourceTypeContext,
    ) -> Result<Resolution<TypeId>, SignatureFlow> {
        self.ensure_analysis_active()?;
        let custody =
            self.original_source_custody(TypeOccurrenceId::new(source.file(), type_ref))?;
        let site = Rc::new(SourceSite {
            owner,
            file: source.file(),
            context: context.clone(),
            custody,
            construction: None,
        });
        let request = Request::Type(TypeRequest {
            site,
            type_ref,
            position,
        });
        match self.run_source_worklist(request)? {
            Completed::Type(result) => Ok(result),
            _ => unreachable!("type request completes with type result"),
        }
    }

    pub(super) fn resolve_trait_worklist(
        &mut self,
        owner: DefId,
        source: &SemanticSource<'hir>,
        type_ref: TypeRefId,
        context: &SourceTypeContext,
    ) -> Result<Resolution<TraitRef>, SignatureFlow> {
        self.ensure_analysis_active()?;
        let custody =
            self.original_source_custody(TypeOccurrenceId::new(source.file(), type_ref))?;
        let site = Rc::new(SourceSite {
            owner,
            file: source.file(),
            context: context.clone(),
            custody,
            construction: None,
        });
        let request = Request::Trait(TraitRequest { site, type_ref });
        match self.run_source_worklist(request)? {
            Completed::Trait(result) => Ok(result),
            _ => unreachable!("trait request completes with trait result"),
        }
    }

    pub(super) fn resolve_alias_worklist(
        &mut self,
        alias: DefId,
        cycle_site: Option<(&SemanticSource<'hir>, OriginId)>,
    ) -> Result<Resolution<AliasTemplate>, SignatureFlow> {
        self.ensure_analysis_active()?;
        let request = Request::Alias(AliasRequest {
            alias,
            cycle_site: cycle_site.map(|(source, origin)| (source.file(), origin)),
            custody: AliasCustody::Original,
            construction: None,
        });
        match self.run_source_worklist(request)? {
            Completed::Alias(result) => Ok(result),
            _ => unreachable!("alias request completes with alias result"),
        }
    }

    fn run_source_worklist(&mut self, root: Request) -> Result<Completed, SignatureFlow> {
        let mut pending = vec![Step::Enter(root)];
        let mut completed = None;
        while let Some(step) = pending.pop() {
            self.ensure_analysis_active()?;
            #[cfg(test)]
            match &step {
                Step::Enter(Request::Type(request)) => execution::tests::checkpoint(
                    self,
                    execution::tests::Checkpoint::Type(TypeOccurrenceId::new(
                        request.site.file,
                        request.type_ref,
                    )),
                )?,
                Step::Enter(Request::Trait(request)) => execution::tests::checkpoint(
                    self,
                    execution::tests::Checkpoint::Trait(TypeOccurrenceId::new(
                        request.site.file,
                        request.type_ref,
                    )),
                )?,
                Step::Enter(Request::Alias(request)) => execution::tests::checkpoint(
                    self,
                    execution::tests::Checkpoint::Alias(request.alias),
                )?,
                Step::Enter(Request::Bound { .. })
                | Step::Enter(Request::Cohort { .. })
                | Step::ResumeCohort(_)
                | Step::ResumeBound { .. }
                | Step::ResumeType(_)
                | Step::ResumeTrait(_)
                | Step::ResumeAlias(_) => {}
            }
            match step {
                Step::Enter(Request::Type(request)) => {
                    self.claim_source_custody(&request.site.custody)?;
                    #[cfg(test)]
                    custody_tests::observe(
                        self,
                        TypeOccurrenceId::new(request.site.file, request.type_ref),
                        &request.site.custody,
                    )?;
                    let plan = self.prepare_type_plan(&request)?;
                    self.schedule_type_plan(request, plan, &mut pending, &mut completed)?;
                }
                Step::Enter(Request::Trait(request)) => {
                    self.claim_source_custody(&request.site.custody)?;
                    #[cfg(test)]
                    custody_tests::observe(
                        self,
                        TypeOccurrenceId::new(request.site.file, request.type_ref),
                        &request.site.custody,
                    )?;
                    let plan = self.prepare_trait_plan(&request)?;
                    self.schedule_trait_plan(request, plan, &mut pending, &mut completed)?;
                }
                Step::Enter(Request::Alias(request)) => {
                    let alias = request.alias;
                    match self.prepare_alias_plan(request)? {
                        AliasPlan::Blocked(poison) => completed = Some(Completed::Blocked(poison)),
                        AliasPlan::Complete(result) => completed = Some(Completed::Alias(result)),
                        AliasPlan::Body {
                            request,
                            generic_parameters,
                        } => {
                            pending.push(Step::ResumeAlias(AliasFrame {
                                alias,
                                generic_parameters,
                                site: Rc::clone(&request.site),
                            }));
                            pending.push(Step::Enter(Request::Type(request)));
                        }
                    }
                }
                Step::Enter(Request::Bound { id, custody }) => {
                    self.enter_bound_construction(id, custody, &mut pending, &mut completed)?;
                }
                Step::Enter(Request::Cohort { id, site }) => {
                    self.enter_cohort(id, site, &mut pending, &mut completed)?;
                }
                Step::ResumeCohort(frame) => {
                    let result = completed.take().expect("cohort child completed");
                    self.continue_cohort(frame, result, &mut pending, &mut completed)?;
                }
                Step::ResumeBound { id } => {
                    let result = completed.take().expect("bound reference completed");
                    self.finish_bound_construction(id, &result);
                    completed = Some(result);
                }
                Step::ResumeType(frame) => {
                    self.claim_source_custody(&frame.parent.site.custody)?;
                    let Some(complete) = completed.take() else {
                        unreachable!("type dependency completed")
                    };
                    let parent = frame.parent.clone();
                    let plan = self.continue_type_plan(frame, complete)?;
                    self.schedule_type_plan(parent, plan, &mut pending, &mut completed)?;
                }
                Step::ResumeTrait(frame) => {
                    self.claim_source_custody(&frame.parent.site.custody)?;
                    let result = completed.take().expect("trait argument completed");
                    let parent = frame.parent.clone();
                    let plan = self.continue_trait_plan(frame, result)?;
                    self.schedule_trait_plan(parent, plan, &mut pending, &mut completed)?;
                }
                Step::ResumeAlias(frame) => {
                    let result = completed.take().expect("alias body completed");
                    completed = Some(self.complete_alias_frame(frame, result)?);
                }
            }
        }
        Ok(completed.expect("root source request completed"))
    }

    fn schedule_type_plan(
        &mut self,
        request: TypeRequest,
        plan: TypePlan,
        pending: &mut Vec<Step>,
        completed: &mut Option<Completed>,
    ) -> Result<(), SignatureFlow> {
        match plan {
            TypePlan::Blocked(poison) => *completed = Some(Completed::Blocked(poison)),
            TypePlan::Complete(completion) => {
                *completed = Some(Completed::Type(
                    self.complete_type_plan(&request, completion)?,
                ));
            }
            TypePlan::Await {
                request: child,
                continuation,
            } => {
                pending.push(Step::ResumeType(TypeFrame {
                    parent: request,
                    continuation,
                }));
                pending.push(Step::Enter(child));
            }
        }
        Ok(())
    }

    fn schedule_trait_plan(
        &mut self,
        request: TraitRequest,
        plan: TraitPlan,
        pending: &mut Vec<Step>,
        completed: &mut Option<Completed>,
    ) -> Result<(), SignatureFlow> {
        match plan {
            TraitPlan::Blocked(poison) => *completed = Some(Completed::Blocked(poison)),
            TraitPlan::Await {
                request: child,
                continuation,
            } => {
                pending.push(Step::ResumeTrait(TraitFrame {
                    parent: request,
                    continuation,
                }));
                pending.push(Step::Enter(Request::Type(child)));
            }
            TraitPlan::Complete { result, written } => {
                *completed = Some(Completed::Trait(
                    self.complete_trait_plan(&request, result, written)?,
                ));
            }
        }
        Ok(())
    }
}
