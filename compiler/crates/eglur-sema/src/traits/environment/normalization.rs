use super::*;
use crate::{NominalHead, RawPointerMutability, ReferenceMutability};

enum Rebuild {
    Nominal(NominalHead),
    Tuple,
    CallableParameters(TypeId),
    ErrorUnion,
    ProjectionArguments {
        projection: AssociatedProjection,
        base: TypeId,
    },
}

struct Children {
    remaining: std::vec::IntoIter<TypeId>,
    normalized: Vec<TypeId>,
    symbolic: bool,
}

impl Children {
    fn new(values: Vec<TypeId>) -> Self {
        Self {
            normalized: Vec::with_capacity(values.len()),
            remaining: values.into_iter(),
            symbolic: false,
        }
    }
}

enum Frame {
    Children(Rebuild, Children),
    Reference(ReferenceMutability),
    RawPointer(RawPointerMutability),
    CallableResult {
        parameters: Vec<TypeId>,
        symbolic: bool,
    },
    ProjectionBase(AssociatedProjection),
    ProjectionValue(TypeId),
}

enum Step {
    Normalize(TypeId),
    Children(Rebuild, Children),
    Complete(TraitNormalization),
}

impl TraitEnvironment {
    pub(super) fn normalize_inner(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        root: TypeId,
        assumptions: &[TraitAssumption],
        active_projections: &mut BTreeSet<TypeId>,
        active_obligations: &mut BTreeSet<TraitObligation>,
    ) -> Result<TraitNormalization, TraitEnvironmentError> {
        let mut frames = Vec::new();
        let mut step = Step::Normalize(root);
        loop {
            step = match step {
                Step::Normalize(ty) => match signatures.types().kind(ty)?.clone() {
                    TypeKind::Never
                    | TypeKind::Primitive(_)
                    | TypeKind::GenericParameter(_)
                    | TypeKind::SelfType(_) => Step::Complete(TraitNormalization::Normalized(ty)),
                    TypeKind::Nominal { head, substitution } => Step::Children(
                        Rebuild::Nominal(head),
                        Children::new(signatures.types().substitution(substitution)?.to_vec()),
                    ),
                    TypeKind::Reference {
                        mutability,
                        referent,
                    } => {
                        frames.push(Frame::Reference(mutability));
                        Step::Normalize(referent)
                    }
                    TypeKind::RawPointer {
                        mutability,
                        referent,
                    } => {
                        frames.push(Frame::RawPointer(mutability));
                        Step::Normalize(referent)
                    }
                    TypeKind::Tuple(elements) => {
                        Step::Children(Rebuild::Tuple, Children::new(elements.into_vec()))
                    }
                    TypeKind::Callable { parameters, result } => Step::Children(
                        Rebuild::CallableParameters(result),
                        Children::new(parameters.into_vec()),
                    ),
                    TypeKind::ErrorUnion(members) => {
                        Step::Children(Rebuild::ErrorUnion, Children::new(members.into_vec()))
                    }
                    TypeKind::AssociatedProjection(projection) => {
                        let base = projection.base();
                        frames.push(Frame::ProjectionBase(projection));
                        Step::Normalize(base)
                    }
                },
                Step::Children(rebuild, mut children) => {
                    if let Some(child) = children.remaining.next() {
                        frames.push(Frame::Children(rebuild, children));
                        step = Step::Normalize(child);
                        continue;
                    }

                    let Children {
                        normalized,
                        symbolic,
                        ..
                    } = children;
                    match rebuild {
                        Rebuild::Nominal(head) => Step::Complete(classify_normalized(
                            signatures.types_mut().nominal(head, normalized)?,
                            symbolic,
                        )),
                        Rebuild::Tuple => Step::Complete(classify_normalized(
                            signatures.types_mut().tuple(normalized)?,
                            symbolic,
                        )),
                        Rebuild::CallableParameters(result) => {
                            frames.push(Frame::CallableResult {
                                parameters: normalized,
                                symbolic,
                            });
                            Step::Normalize(result)
                        }
                        Rebuild::ErrorUnion => Step::Complete(classify_normalized(
                            signatures.types_mut().error_union(normalized)?,
                            symbolic,
                        )),
                        Rebuild::ProjectionArguments { projection, base } => {
                            let substitution =
                                signatures.types_mut().intern_substitution(normalized)?;
                            let trait_ref = TraitRef::new(
                                projection.trait_ref().declaration().clone(),
                                substitution,
                            );
                            let normalized_projection = AssociatedProjection::new(
                                base,
                                trait_ref.clone(),
                                projection.associated().clone(),
                            );
                            let projection_ty = signatures
                                .types_mut()
                                .associated_projection(normalized_projection.clone())?;
                            let obligation = TraitObligation {
                                subject: base,
                                trait_ref,
                            };
                            let has_current_impl_evidence = assumptions.iter().any(|assumption| {
                                assumption.obligation == obligation
                                    && matches!(
                                        &assumption.evidence,
                                        TraitAssumptionEvidence::Implementation(_)
                                    )
                            });
                            if matches!(
                                signatures.types().kind(base)?,
                                TypeKind::GenericParameter(_) | TypeKind::SelfType(_)
                            ) && !has_current_impl_evidence
                            {
                                step = Step::Complete(TraitNormalization::Symbolic(projection_ty));
                                continue;
                            }
                            if let Some(owner) = owner
                                && let Some(cached) =
                                    self.projection_cache.get(&(owner, projection_ty))
                            {
                                step = Step::Complete(cached.clone());
                                continue;
                            }
                            if owner.is_none()
                                && let Some(cached) = self
                                    .local_query_cache
                                    .as_ref()
                                    .and_then(|cache| cache.projections.get(&projection_ty))
                            {
                                step = Step::Complete(cached.clone());
                                continue;
                            }
                            if !active_projections.insert(projection_ty) {
                                step = Step::Complete(TraitNormalization::Cyclic(
                                    normalized_projection,
                                ));
                                continue;
                            }

                            let selected = self.select_impl(
                                owner,
                                signatures,
                                obligation.clone(),
                                assumptions,
                                active_obligations,
                            )?;
                            // Only selection/value completion owns guard removal and caching.
                            // Infrastructure errors still leave the active guard untouched,
                            // except for the existing missing-binding error below.
                            frames.push(Frame::ProjectionValue(projection_ty));
                            match selected {
                                SelectionOutcome::Selected(selection) => {
                                    let implementation = self
                                        .implementations
                                        .get(&selection.implementation)
                                        .cloned()
                                        .ok_or_else(|| {
                                            TraitEnvironmentError::UnknownImplementation(
                                                selection.implementation.clone(),
                                            )
                                        })?;
                                    let Some(binding) =
                                        implementation.bindings.get(projection.associated())
                                    else {
                                        active_projections.remove(&projection_ty);
                                        return Err(
                                            TraitEnvironmentError::MissingAssociatedBinding {
                                                implementation: selection.implementation,
                                                associated: projection.associated().clone(),
                                            },
                                        );
                                    };
                                    let value = signatures
                                        .types_mut()
                                        .substitute(binding.value(), &selection.replacements)?;
                                    Step::Normalize(value)
                                }
                                SelectionOutcome::Poisoned(poison) => {
                                    Step::Complete(TraitNormalization::Poisoned(poison))
                                }
                                SelectionOutcome::InvalidArgument(violation) => {
                                    Step::Complete(TraitNormalization::InvalidArgument(violation))
                                }
                                SelectionOutcome::Unsatisfied(obligation) => {
                                    Step::Complete(TraitNormalization::Unsatisfied(obligation))
                                }
                                SelectionOutcome::Ambiguous(obligation) => {
                                    Step::Complete(TraitNormalization::Ambiguous(obligation))
                                }
                                SelectionOutcome::Assumed(_) | SelectionOutcome::TrustedBound => {
                                    Step::Complete(TraitNormalization::Unsatisfied(obligation))
                                }
                            }
                        }
                    }
                }
                Step::Complete(result) => {
                    let Some(frame) = frames.pop() else {
                        return Ok(result);
                    };
                    if let Frame::ProjectionValue(projection_ty) = frame {
                        active_projections.remove(&projection_ty);
                        if let Some(owner) = owner {
                            self.projection_cache
                                .insert((owner, projection_ty), result.clone());
                        } else if let Some(cache) = &mut self.local_query_cache {
                            cache.projections.insert(projection_ty, result.clone());
                        }
                        step = Step::Complete(result);
                        continue;
                    }

                    let (ty, symbolic) = match result {
                        TraitNormalization::Normalized(ty) => (ty, false),
                        TraitNormalization::Symbolic(ty) => (ty, true),
                        failure => {
                            // Discard pending siblings without visiting them, but still unwind
                            // selected projection values through their completion frames.
                            step = Step::Complete(failure);
                            continue;
                        }
                    };
                    match frame {
                        Frame::Children(rebuild, mut children) => {
                            children.normalized.push(ty);
                            children.symbolic |= symbolic;
                            Step::Children(rebuild, children)
                        }
                        Frame::Reference(mutability) => Step::Complete(classify_normalized(
                            signatures.types_mut().reference(mutability, ty)?,
                            symbolic,
                        )),
                        Frame::RawPointer(mutability) => Step::Complete(classify_normalized(
                            signatures.types_mut().raw_pointer(mutability, ty)?,
                            symbolic,
                        )),
                        Frame::CallableResult {
                            parameters,
                            symbolic: parameters_symbolic,
                        } => Step::Complete(classify_normalized(
                            signatures.types_mut().callable(parameters, ty)?,
                            parameters_symbolic || symbolic,
                        )),
                        Frame::ProjectionBase(projection) => {
                            let arguments = signatures
                                .types()
                                .substitution(projection.trait_ref().substitution())?
                                .to_vec();
                            Step::Children(
                                Rebuild::ProjectionArguments {
                                    projection,
                                    base: ty,
                                },
                                Children::new(arguments),
                            )
                        }
                        Frame::ProjectionValue(_) => unreachable!("projection value was completed"),
                    }
                }
            };
        }
    }

    pub(super) fn normalize_many(
        &mut self,
        owner: Option<DefId>,
        signatures: &mut DeclarationSignatures,
        values: &[TypeId],
        assumptions: &[TraitAssumption],
        active_projections: &mut BTreeSet<TypeId>,
        active_obligations: &mut BTreeSet<TraitObligation>,
    ) -> Result<Result<(Vec<TypeId>, bool), TraitNormalization>, TraitEnvironmentError> {
        let mut normalized = Vec::with_capacity(values.len());
        let mut symbolic = false;
        for &value in values {
            match self.normalize_inner(
                owner,
                signatures,
                value,
                assumptions,
                active_projections,
                active_obligations,
            )? {
                TraitNormalization::Normalized(ty) => normalized.push(ty),
                TraitNormalization::Symbolic(ty) => {
                    symbolic = true;
                    normalized.push(ty);
                }
                failure => return Ok(Err(failure)),
            }
        }
        Ok(Ok((normalized, symbolic)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompileCapability, CompileSupport, ImportAnalysisUnit, IntrinsicNominal, ModuleAddress,
        PhaseProduct, PrimitiveType, SemanticSource, SignatureResolutionInputs,
        collect_declarations, plan_imports, resolve_declaration_signatures, resolve_imports,
    };
    use eglur_diagnostics::{DiagnosticStore, SourceLabel, SourceSpan};
    use eglur_hir::lower;
    use eglur_source::{FileId, LineIndex, SourceFile};
    use eglur_syntax::parse_validated;

    struct AllCapabilities;

    impl CompileSupport for AllCapabilities {
        fn supports(&self, _: CompileCapability) -> bool {
            true
        }
    }

    struct Fixture {
        signatures: DeclarationSignatures,
        environment: TraitEnvironment,
        owner: DefId,
        projection: AssociatedProjection,
    }

    fn performed<T>(product: PhaseProduct<T>) -> T {
        match product {
            PhaseProduct::Performed(value) => value,
            PhaseProduct::Unperformed => panic!("fixture phase unavailable"),
        }
    }

    fn fixture(projected_binding: bool) -> Fixture {
        let binding = if projected_binding {
            "Item[Other]"
        } else {
            "i64"
        };
        let other = if projected_binding {
            "struct Other {}\nimpl Carries for Other { type Item = i64 }\n"
        } else {
            ""
        };
        let text = format!(
            "trait Carries {{ type Item }}\nstruct Carrier {{}}\n\
             type Item[T: Carries] = T::Item\n\
             impl Carries for Carrier {{ type Item = {binding} }}\n\
             fn direct() -> Unit = ()\n{other}"
        );
        let file = FileId::new(0);
        let input = SourceFile::new(file, text.into_bytes());
        let validated = input.validate().unwrap();
        let lines = LineIndex::new(validated);
        let parse = parse_validated(validated);
        assert!(parse.diagnostics().is_empty());
        let hir = lower(&parse);
        assert!(hir.invalid_occurrences().is_empty());
        let label = SourceLabel::new("normalization.eglur").unwrap();
        let spans = hir
            .origins()
            .map(|(origin, value)| {
                (
                    origin,
                    SourceSpan::from_origin(label.clone(), value, &lines).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let sources = [SemanticSource::new(
            file,
            ModuleAddress::new([Box::<str>::from("main")]).unwrap(),
            Some(&hir),
            spans,
            [],
            None,
        )
        .unwrap()];
        let mut diagnostics = DiagnosticStore::new();
        let declarations = performed(
            collect_declarations(&sources, &AllCapabilities)
                .unwrap()
                .into_parts()
                .0,
        );
        let plan = performed(
            plan_imports(
                &declarations,
                &sources,
                &AllCapabilities,
                None,
                &mut diagnostics,
            )
            .unwrap()
            .into_parts()
            .0,
        );
        let imports = performed(
            resolve_imports(
                &declarations,
                &plan,
                &[ImportAnalysisUnit::new([file]).unwrap()],
                &sources,
                &AllCapabilities,
                None,
                &mut diagnostics,
            )
            .unwrap()
            .into_parts()
            .0,
        );
        let resolved = performed(
            resolve_declaration_signatures(
                &declarations,
                &imports,
                &sources,
                &AllCapabilities,
                SignatureResolutionInputs::new(None, None),
                &mut diagnostics,
            )
            .unwrap()
            .into_parts()
            .0,
        );
        let (mut signatures, _) = resolved.into_parts();
        let mut environment = performed(
            crate::build_trait_environment(
                &declarations,
                &sources,
                &AllCapabilities,
                &mut signatures,
                &mut diagnostics,
            )
            .unwrap()
            .into_parts()
            .0,
        );
        let owner = declarations
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| {
                declaration
                    .name()
                    .is_some_and(|name| name.spelling() == "direct")
            })
            .unwrap()
            .id();
        let implementation = environment.implementations.values().next().unwrap();
        let projection = AssociatedProjection::new(
            implementation.header.target(),
            implementation.header.trait_ref().clone(),
            implementation.bindings.keys().next().unwrap().clone(),
        );
        environment.projection_cache.clear();
        environment.selection_cache.clear();
        Fixture {
            signatures,
            environment,
            owner,
            projection,
        }
    }

    impl Fixture {
        fn normalize(&mut self, root: TypeId) -> TraitNormalization {
            self.environment
                .normalize_inner(
                    Some(self.owner),
                    &mut self.signatures,
                    root,
                    &[],
                    &mut BTreeSet::new(),
                    &mut BTreeSet::new(),
                )
                .unwrap()
        }

        fn projection_type(&mut self, base: TypeId, arguments: &[TypeId]) -> TypeId {
            let substitution = self
                .signatures
                .types_mut()
                .intern_substitution(arguments.iter().copied())
                .unwrap();
            self.signatures
                .types_mut()
                .associated_projection(AssociatedProjection::new(
                    base,
                    TraitRef::new(
                        self.projection.trait_ref().declaration().clone(),
                        substitution,
                    ),
                    self.projection.associated().clone(),
                ))
                .unwrap()
        }
    }

    #[test]
    fn deeply_mixed_structures_preserve_ids_and_symbolic_classification() {
        for symbolic in [false, true] {
            let mut fixture = fixture(false);
            let integer = fixture
                .signatures
                .types_mut()
                .primitive(PrimitiveType::I64)
                .unwrap();
            let mut root = if symbolic {
                let base = fixture
                    .signatures
                    .types_mut()
                    .self_type(CanonicalTarget::Source(fixture.owner))
                    .unwrap();
                fixture.projection_type(base, &[])
            } else {
                integer
            };
            for depth in 0..2048 {
                let types = fixture.signatures.types_mut();
                root = match depth % 6 {
                    0 => types.tuple([root]).unwrap(),
                    1 => types
                        .nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [root])
                        .unwrap(),
                    2 => types.reference(ReferenceMutability::Shared, root).unwrap(),
                    3 => types
                        .raw_pointer(RawPointerMutability::Const, root)
                        .unwrap(),
                    4 => types.callable([root], integer).unwrap(),
                    _ => types.error_union([root, integer]).unwrap(),
                };
            }
            let count = fixture.signatures.types().type_count();
            let result = fixture.normalize(root);
            match result {
                TraitNormalization::Normalized(ty) => {
                    assert!(!symbolic);
                    assert_eq!(ty, root);
                }
                TraitNormalization::Symbolic(ty) => {
                    assert!(symbolic);
                    assert_eq!(ty, root);
                }
                failure => panic!("unexpected normalization failure: {failure:?}"),
            }
            assert_eq!(fixture.signatures.types().type_count(), count);
        }
    }

    #[test]
    fn deeply_nested_projection_arguments_do_not_recurse() {
        let mut fixture = fixture(false);
        let base = fixture
            .signatures
            .types_mut()
            .self_type(CanonicalTarget::Source(fixture.owner))
            .unwrap();
        let mut root = base;
        for _ in 0..2048 {
            root = fixture.projection_type(base, &[root]);
        }
        assert!(matches!(fixture.normalize(root), TraitNormalization::Symbolic(ty) if ty == root));
        assert!(fixture.environment.projection_cache.is_empty());
    }

    #[test]
    fn deeply_nested_projection_bases_short_circuit_after_innermost_failure() {
        let mut fixture = fixture(false);
        let base = fixture
            .signatures
            .types_mut()
            .self_type(CanonicalTarget::Source(fixture.owner))
            .unwrap();
        let symbolic = fixture.projection_type(base, &[]);
        let failed = fixture.projection_type(symbolic, &[]);
        let mut root = failed;
        for _ in 0..2048 {
            root = fixture.projection_type(root, &[]);
        }
        assert!(
            matches!(fixture.normalize(root), TraitNormalization::Unsatisfied(obligation) if obligation.subject() == symbolic)
        );
        assert_eq!(fixture.environment.projection_cache.len(), 1);
        assert!(
            fixture
                .environment
                .projection_cache
                .contains_key(&(fixture.owner, failed))
        );
    }

    #[test]
    fn structural_rebuild_interns_in_source_postorder() {
        let mut fixture = fixture(false);
        let integer = fixture
            .signatures
            .types_mut()
            .primitive(PrimitiveType::I64)
            .unwrap();
        let projection = fixture.projection_type(fixture.projection.base(), &[]);
        let first = fixture.signatures.types_mut().tuple([projection]).unwrap();
        let second = fixture
            .signatures
            .types_mut()
            .callable([projection], projection)
            .unwrap();
        let root = fixture
            .signatures
            .types_mut()
            .tuple([first, second])
            .unwrap();
        let start = fixture.signatures.types().type_count();
        let first_normalized = TypeId::from_index(start).unwrap();
        let second_normalized = TypeId::from_index(start + 1).unwrap();
        let root_normalized = TypeId::from_index(start + 2).unwrap();

        assert!(
            matches!(fixture.normalize(root), TraitNormalization::Normalized(ty) if ty == root_normalized)
        );
        assert_eq!(fixture.signatures.types().type_count(), start + 3);
        assert_eq!(
            fixture.signatures.types().kind(first_normalized).unwrap(),
            &TypeKind::Tuple(Box::new([integer]))
        );
        assert_eq!(
            fixture.signatures.types().kind(second_normalized).unwrap(),
            &TypeKind::Callable {
                parameters: Box::new([integer]),
                result: integer
            }
        );
        assert_eq!(
            fixture.signatures.types().kind(root_normalized).unwrap(),
            &TypeKind::Tuple(Box::new([first_normalized, second_normalized]))
        );
    }

    #[test]
    fn symbolic_generic_base_precedes_projection_cache_and_cycle_guard() {
        let mut fixture = fixture(false);
        let base = fixture
            .signatures
            .types_mut()
            .generic_parameter(GenericParameterId::Source(fixture.owner))
            .unwrap();
        let root = fixture.projection_type(base, &[]);
        fixture.environment.projection_cache.insert(
            (fixture.owner, root),
            TraitNormalization::Cyclic(fixture.projection.clone()),
        );
        let mut active = BTreeSet::from([root]);
        let result = fixture
            .environment
            .normalize_inner(
                Some(fixture.owner),
                &mut fixture.signatures,
                root,
                &[],
                &mut active,
                &mut BTreeSet::new(),
            )
            .unwrap();
        assert!(matches!(result, TraitNormalization::Symbolic(ty) if ty == root));
        assert_eq!(active, BTreeSet::from([root]));
        assert!(matches!(
            fixture.environment.projection_cache[&(fixture.owner, root)],
            TraitNormalization::Cyclic(_)
        ));
    }

    #[test]
    fn failed_base_skips_arguments_and_failed_child_skips_later_siblings() {
        let mut fixture = fixture(false);
        let integer = fixture
            .signatures
            .types_mut()
            .primitive(PrimitiveType::I64)
            .unwrap();
        let failed = fixture.projection_type(integer, &[]);
        let successful = fixture.projection_type(fixture.projection.base(), &[]);
        let outer = fixture.projection_type(failed, &[successful]);
        let root = fixture
            .signatures
            .types_mut()
            .callable([outer, successful], successful)
            .unwrap();
        assert!(
            matches!(fixture.normalize(root), TraitNormalization::Unsatisfied(obligation) if obligation.subject() == integer)
        );
        assert_eq!(fixture.environment.projection_cache.len(), 1);
        assert!(
            fixture
                .environment
                .projection_cache
                .contains_key(&(fixture.owner, failed))
        );
        assert!(
            !fixture
                .environment
                .projection_cache
                .contains_key(&(fixture.owner, outer))
        );
        assert!(
            !fixture
                .environment
                .projection_cache
                .contains_key(&(fixture.owner, successful))
        );
    }

    #[test]
    fn failed_argument_leaves_enclosing_projection_uncached() {
        let mut fixture = fixture(false);
        let integer = fixture
            .signatures
            .types_mut()
            .primitive(PrimitiveType::I64)
            .unwrap();
        let failed = fixture.projection_type(integer, &[]);
        let successful = fixture.projection_type(fixture.projection.base(), &[]);
        let outer = fixture.projection_type(fixture.projection.base(), &[failed, successful]);
        assert!(matches!(
            fixture.normalize(outer),
            TraitNormalization::Unsatisfied(_)
        ));
        assert_eq!(fixture.environment.projection_cache.len(), 1);
        assert!(
            fixture
                .environment
                .projection_cache
                .contains_key(&(fixture.owner, failed))
        );
    }

    #[test]
    fn selected_value_cycle_unwinds_its_guard_and_caches_only_in_query_scope() {
        for owner_scope in [false, true] {
            let mut fixture = fixture(true);
            let root = fixture.projection_type(fixture.projection.base(), &[]);
            let owner = owner_scope.then_some(fixture.owner);
            fixture.environment.local_query_cache = Some(LocalQueryCache::default());
            let value = fixture
                .environment
                .implementations
                .values()
                .next()
                .unwrap()
                .bindings
                .values()
                .next()
                .unwrap()
                .value();
            let TypeKind::AssociatedProjection(value_projection) =
                fixture.signatures.types().kind(value).unwrap()
            else {
                panic!("expected projected binding value");
            };
            let value_projection = value_projection.clone();
            let mut active = BTreeSet::from([value]);
            let result = fixture
                .environment
                .normalize_inner(
                    owner,
                    &mut fixture.signatures,
                    root,
                    &[],
                    &mut active,
                    &mut BTreeSet::new(),
                )
                .unwrap();
            assert!(
                matches!(result, TraitNormalization::Cyclic(ref projection) if projection == &value_projection)
            );
            assert_eq!(active, BTreeSet::from([value]));
            assert_eq!(
                fixture.environment.projection_cache.len(),
                usize::from(owner_scope)
            );
            let local = fixture.environment.local_query_cache.as_ref().unwrap();
            assert_eq!(local.projections.len(), usize::from(!owner_scope));
            let cached = if owner_scope {
                &fixture.environment.projection_cache[&(fixture.owner, root)]
            } else {
                &local.projections[&root]
            };
            assert!(
                matches!(cached, TraitNormalization::Cyclic(projection) if projection == &value_projection)
            );
        }
    }

    #[test]
    fn missing_binding_removes_only_its_guard_and_does_not_cache_failure() {
        let mut fixture = fixture(false);
        let root = fixture.projection_type(fixture.projection.base(), &[]);
        fixture
            .environment
            .implementations
            .values_mut()
            .next()
            .unwrap()
            .bindings
            .clear();
        let unrelated = fixture
            .signatures
            .types_mut()
            .primitive(PrimitiveType::Bool)
            .unwrap();
        let mut active = BTreeSet::from([unrelated]);
        let result = fixture.environment.normalize_inner(
            Some(fixture.owner),
            &mut fixture.signatures,
            root,
            &[],
            &mut active,
            &mut BTreeSet::new(),
        );
        assert!(matches!(
            result,
            Err(TraitEnvironmentError::MissingAssociatedBinding { .. })
        ));
        assert_eq!(active, BTreeSet::from([unrelated]));
        assert!(fixture.environment.projection_cache.is_empty());
    }

    #[test]
    fn infrastructure_error_preserves_active_guard_without_caching() {
        let mut fixture = fixture(false);
        let root = fixture.projection_type(fixture.projection.base(), &[]);
        let implementation = fixture
            .environment
            .implementations
            .keys()
            .next()
            .unwrap()
            .clone();
        fixture.environment.selection_cache.insert(
            (
                fixture.owner,
                TraitObligation::new(
                    fixture.projection.base(),
                    fixture.projection.trait_ref().clone(),
                ),
            ),
            SelectionOutcome::Selected(ImplSelection {
                implementation: implementation.clone(),
                replacements: BTreeMap::new(),
            }),
        );
        fixture.environment.implementations.remove(&implementation);
        let mut active = BTreeSet::new();
        let result = fixture.environment.normalize_inner(
            Some(fixture.owner),
            &mut fixture.signatures,
            root,
            &[],
            &mut active,
            &mut BTreeSet::new(),
        );
        assert!(
            matches!(result, Err(TraitEnvironmentError::UnknownImplementation(found)) if found == implementation)
        );
        assert_eq!(active, BTreeSet::from([root]));
        assert!(fixture.environment.projection_cache.is_empty());
    }
}
