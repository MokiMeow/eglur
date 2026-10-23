use super::*;

impl SourceBoundConstruction {
    pub(in crate::signatures::resolver) fn declaring_bounds(
        &mut self,
        parameter: DefId,
        name: &str,
        traits: &BTreeMap<CanonicalTarget, TraitInfo>,
    ) -> Resolution<DeclaringSelection> {
        let key = (parameter, Box::<str>::from(name));
        if let Some(candidates) = self.candidates.get(&key) {
            return candidates.clone();
        }
        let result = match self.head_states.get(&parameter) {
            Some(Resolution::Known(())) => {
                let heads = self.heads.get(&parameter);
                let associated_heads = self.associated_heads.get(name);
                let mut selected = None;
                let mut ambiguous = false;
                if let (Some(heads), Some(associated_heads)) = (heads, associated_heads) {
                    let mut visit = |head: &CanonicalTarget| {
                        #[cfg(test)]
                        {
                            self.work.head_probes += 1;
                        }
                        let Some(&cohort) = heads.get(head) else {
                            return false;
                        };
                        let Some(associated) =
                            traits.get(head).and_then(|info| info.associated.get(name))
                        else {
                            return false;
                        };
                        if selected.is_some() || associated.len() != 1 {
                            ambiguous = true;
                        } else {
                            selected = Some(DeclaringBound {
                                cohort,
                                associated: associated[0].clone(),
                            });
                        }
                        ambiguous
                    };
                    if associated_heads.len() < heads.len() {
                        for head in associated_heads {
                            if visit(head) {
                                break;
                            }
                        }
                    } else {
                        for head in heads.keys() {
                            if visit(head) {
                                break;
                            }
                        }
                    }
                }
                Resolution::Known(if ambiguous {
                    DeclaringSelection::Ambiguous
                } else if let Some(selected) = selected {
                    DeclaringSelection::Unique(selected)
                } else {
                    DeclaringSelection::Missing
                })
            }
            Some(Resolution::Poisoned(poison)) => Resolution::Poisoned(poison.clone()),
            Some(Resolution::Unperformed) | None => Resolution::Unperformed,
        };
        self.candidates.insert(key, result.clone());
        result
    }
}

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(in crate::signatures::resolver) fn construct_source_bound_inventory(
        &mut self,
        generic_ids: &[DefId],
    ) -> Result<(), SignatureFlow> {
        if self.source_bounds.initialized {
            return Ok(());
        }
        let mut recipes = Vec::new();
        for (head, info) in &self.trait_info {
            let head = Rc::new(head.clone());
            for name in info.associated.keys() {
                self.source_bounds
                    .associated_heads
                    .entry(name.clone())
                    .or_default()
                    .push(Rc::clone(&head));
            }
        }
        for &parameter_id in generic_ids {
            if self.source_generic_redeclarations.contains(&parameter_id) {
                continue;
            }
            let (file, parameter) = self.source_generic_parameter(parameter_id)?;
            if !parameter.name.is_valid()
                || parameter.name.text().is_some_and(is_reserved_type_name)
            {
                continue;
            }
            let bounds = parameter.bounds.clone();
            recipes.push((parameter_id, bounds.clone()));
            let mut poison = PoisonAccumulator::new();
            let mut unperformed = false;
            let mut head_slots = BTreeMap::<CanonicalTarget, Vec<ConstructionId>>::new();
            for &bound in &bounds {
                let id = self
                    .source_bounds
                    .node(ConstructionKey::Bound(parameter_id, bound));
                let head = self.source_bound_head(parameter_id, bound)?;
                self.source_bounds
                    .head_results
                    .insert((file, bound), head.clone());
                match head {
                    Resolution::Known(target) => {
                        head_slots.entry(target).or_default().push(id);
                    }
                    Resolution::Poisoned(found) => {
                        poison.include(&found)?;
                        self.source_bounds.set_state(
                            id,
                            ConstructionState::Complete(Resolution::Poisoned(found)),
                        );
                    }
                    Resolution::Unperformed => {
                        unperformed = true;
                        self.source_bounds
                            .set_state(id, ConstructionState::Complete(Resolution::Unperformed));
                    }
                }
            }
            let state = if let Some(poison) = poison.finish() {
                Resolution::Poisoned(poison)
            } else if unperformed {
                Resolution::Unperformed
            } else {
                Resolution::Known(())
            };
            self.source_bounds.head_states.insert(parameter_id, state);
            for (head, slots) in head_slots {
                let index = self.source_bounds.cohorts.len();
                self.source_bounds.cohorts.push(Rc::from(slots));
                let id = self.source_bounds.node(ConstructionKey::Cohort(index));
                self.source_bounds
                    .heads
                    .entry(parameter_id)
                    .or_default()
                    .insert(head, id);
            }
        }
        self.source_bounds.initialized = true;
        let ids = self
            .source_bounds
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| matches!(node.key, ConstructionKey::Bound(_, _)).then_some(id))
            .collect::<Vec<_>>();
        for &id in &ids {
            self.resolve_bound_worklist(id)?;
        }
        let cohorts = self
            .source_bounds
            .heads
            .values()
            .flat_map(|heads| heads.values().copied())
            .collect::<Vec<_>>();
        for id in cohorts {
            self.resolve_cohort_worklist(id)?;
        }
        self.finish_bound_construction_cycles()?;

        for (parameter, bounds) in recipes {
            let mut available = BTreeSet::new();
            let mut poison = PoisonAccumulator::new();
            let mut unperformed = false;
            for bound in bounds {
                let Some(&id) = self
                    .source_bounds
                    .by_key
                    .get(&ConstructionKey::Bound(parameter, bound))
                else {
                    continue;
                };
                let result = match self.source_bounds.state(id).clone() {
                    ConstructionState::Complete(result) => result.map(|value| match value {
                        ConstructedReference::Bound(reference) => reference,
                        _ => unreachable!("bound inventory stores trait references"),
                    }),
                    _ => unreachable!("construction graph is terminal after cycle analysis"),
                };
                match &result {
                    Resolution::Known(reference) => {
                        available.insert(reference.clone());
                    }
                    Resolution::Poisoned(found) => poison.include(found)?,
                    Resolution::Unperformed => unperformed = true,
                }
                self.source_generic_bound_seeds
                    .entry((parameter, bound))
                    .or_insert(result);
            }
            let state = if let Some(poison) = poison.finish() {
                Resolution::Poisoned(poison)
            } else if unperformed {
                Resolution::Unperformed
            } else {
                Resolution::Known(())
            };
            if matches!(state, Resolution::Known(())) {
                self.generic_bounds
                    .insert(GenericParameterId::Source(parameter), available);
            }
            self.source_bounds.environments.insert(parameter, state);
        }
        Ok(())
    }

    pub(in crate::signatures::resolver) fn construction_alias_parameters(
        &mut self,
        alias: DefId,
        template: &AliasTemplate,
    ) -> Result<Rc<[(usize, GenericParameterId)]>, SignatureFlow> {
        if let Some(parameters) = self.source_bounds.alias_parameters.get(&alias) {
            return Ok(Rc::clone(parameters));
        }
        let mut pending = vec![template.value];
        let mut seen = BTreeSet::new();
        let mut parameters = BTreeSet::new();
        while let Some(ty) = pending.pop() {
            if !seen.insert(ty) {
                continue;
            }
            match self.types.kind(ty)? {
                TypeKind::GenericParameter(parameter) => {
                    parameters.insert(parameter.clone());
                }
                TypeKind::Nominal { substitution, .. } => {
                    pending.extend(self.types.substitution(*substitution)?)
                }
                TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                    pending.push(*referent)
                }
                TypeKind::Tuple(elements) | TypeKind::ErrorUnion(elements) => {
                    pending.extend(elements.iter().copied())
                }
                TypeKind::Callable { parameters, result } => {
                    pending.extend(parameters.iter().copied());
                    pending.push(*result);
                }
                TypeKind::AssociatedProjection(projection) => {
                    pending.push(projection.base());
                    pending.extend(
                        self.types
                            .substitution(projection.trait_ref().substitution())?,
                    );
                }
                TypeKind::Never | TypeKind::Primitive(_) | TypeKind::SelfType(_) => {}
            }
        }
        let parameters = Rc::from(
            template
                .generic_parameters
                .iter()
                .enumerate()
                .filter(|(_, id)| parameters.contains(*id))
                .map(|(ordinal, parameter)| (ordinal, parameter.clone()))
                .collect::<Vec<_>>(),
        );
        self.source_bounds
            .alias_parameters
            .insert(alias, Rc::clone(&parameters));
        Ok(parameters)
    }
}
