use super::*;

impl SourceBoundConstruction {
    pub(super) fn components(&self) -> Vec<Vec<ConstructionId>> {
        let mut reverse = vec![Vec::new(); self.nodes.len()];
        for (caller, node) in self.nodes.iter().enumerate() {
            for edge in &node.dependencies {
                reverse[edge.target].push(caller);
            }
        }

        let mut seen = vec![false; self.nodes.len()];
        let mut order = Vec::with_capacity(self.nodes.len());
        for start in 0..self.nodes.len() {
            if seen[start] {
                continue;
            }
            seen[start] = true;
            let mut pending = vec![(start, 0)];
            while let Some((id, next)) = pending.last_mut() {
                if let Some(edge) = self.nodes[*id].dependencies.get(*next) {
                    *next += 1;
                    if !seen[edge.target] {
                        seen[edge.target] = true;
                        pending.push((edge.target, 0));
                    }
                } else {
                    order.push(pending.pop().expect("active graph frame").0);
                }
            }
        }

        seen.fill(false);
        let mut cycles = Vec::new();
        for start in order.into_iter().rev() {
            if seen[start] {
                continue;
            }
            seen[start] = true;
            let mut members = Vec::new();
            let mut pending = vec![start];
            while let Some(id) = pending.pop() {
                members.push(id);
                for &caller in &reverse[id] {
                    if !seen[caller] {
                        seen[caller] = true;
                        pending.push(caller);
                    }
                }
            }
            cycles.push(members);
        }
        cycles
    }
}

impl SignatureResolver<'_, '_> {
    pub(in crate::signatures::resolver) fn finish_bound_construction_cycles(
        &mut self,
    ) -> Result<(), SignatureFlow> {
        let components = self.source_bounds.components();
        let mut component_by_node = vec![usize::MAX; self.source_bounds.nodes.len()];
        for (index, component) in components.iter().enumerate() {
            for &id in component {
                component_by_node[id] = index;
            }
        }

        let mut selected = Vec::new();
        for (index, component) in components.iter().enumerate() {
            let cyclic = component.len() > 1
                || self.source_bounds.nodes[component[0]]
                    .dependencies
                    .iter()
                    .any(|edge| edge.target == component[0]);
            if !cyclic {
                continue;
            }
            let mut primary = None;
            for &id in component {
                for edge in &self.source_bounds.nodes[id].dependencies {
                    if component_by_node[edge.target] != index {
                        continue;
                    }
                    if let Some(site) = edge.projection {
                        let span = self.required_span(self.sources[&site.file], site.origin)?;
                        let key = (span.source().clone(), span.file_span().range().start());
                        if primary.as_ref().is_none_or(|(current, _)| &key < current) {
                            primary = Some((key, site));
                        }
                    }
                }
            }
            if let Some((key, site)) = primary {
                selected.push((key, site, index, true));
            } else {
                let mut aliases = Vec::new();
                for &id in component {
                    if let ConstructionKey::Alias(alias) = self.source_bounds.key(id) {
                        let declaration = self.collection.declaration(alias).expect("source alias");
                        let file = self
                            .collection
                            .module(alias.module())
                            .expect("alias module")
                            .file();
                        let site = ProjectionSite {
                            file,
                            origin: declaration.origin(),
                        };
                        let span = self.required_span(self.sources[&file], site.origin)?;
                        aliases.push((
                            (span.source().clone(), span.file_span().range().start()),
                            site,
                        ));
                    }
                }
                let (key, site) = aliases
                    .into_iter()
                    .min_by(|left, right| left.0.cmp(&right.0))
                    .expect("alias-only construction cycle");
                selected.push((key, site, index, false));
            }
        }
        selected.sort_by(|left, right| left.0.cmp(&right.0));

        let mut cycle_poisons = vec![None; components.len()];
        for (_, site, component, bound_cycle) in selected {
            self.ensure_analysis_active()?;
            cycle_poisons[component] = Some(self.source_error(
                self.sources[&site.file],
                site.origin,
                if bound_cycle {
                    "GENERIC_BOUND_REFERENCE_CYCLE"
                } else {
                    "TYPE_ALIAS_CYCLE"
                },
                if bound_cycle {
                    "generic bound reference construction contains a cycle"
                } else {
                    "type alias expansion contains a cycle"
                },
                BTreeMap::new(),
            )?);
        }

        // Kosaraju returns caller-first components; finish dependencies first.
        for (index, members) in components.iter().enumerate().rev() {
            self.ensure_analysis_active()?;
            let mut evidence = PoisonAccumulator::new();
            if let Some(poison) = &cycle_poisons[index] {
                evidence.include(poison)?;
            }
            for &id in members {
                match self.source_bounds.state(id) {
                    ConstructionState::Blocked(Some(poison))
                    | ConstructionState::Complete(Resolution::Poisoned(poison)) => {
                        evidence.include(poison)?
                    }
                    _ => {}
                }
                for edge in &self.source_bounds.nodes[id].dependencies {
                    if component_by_node[edge.target] != index
                        && let ConstructionState::Complete(Resolution::Poisoned(poison)) =
                            self.source_bounds.state(edge.target)
                    {
                        evidence.include(poison)?;
                    }
                }
            }
            let evidence = evidence.finish();
            for &id in members {
                if matches!(
                    self.source_bounds.state(id),
                    ConstructionState::Blocked(_) | ConstructionState::Active
                ) {
                    let poison = evidence
                        .as_ref()
                        .expect("blocked component has a cycle or failed dependency");
                    self.source_bounds.set_state(
                        id,
                        ConstructionState::Complete(Resolution::Poisoned(poison.clone())),
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_graphs_through_four_nodes_match_mutual_positive_reachability() {
        for count in 0..=4 {
            for mask in 0_u32..(1 << (count * count)) {
                let mut graph = SourceBoundConstruction::default();
                for id in 0..count {
                    graph.node(ConstructionKey::Cohort(id));
                }
                let mut reach = vec![vec![false; count]; count];
                for (from, row) in reach.iter_mut().enumerate() {
                    for (to, reachable) in row.iter_mut().enumerate() {
                        if mask & (1 << (from * count + to)) != 0 {
                            *reachable = true;
                            graph.depend(from, to, None);
                        }
                    }
                }
                for via in 0..count {
                    for from in 0..count {
                        for to in 0..count {
                            reach[from][to] |= reach[from][via] && reach[via][to];
                        }
                    }
                }
                let components = graph.components();
                let mut component_of = vec![usize::MAX; count];
                for (component, members) in components.iter().enumerate() {
                    for &member in members {
                        component_of[member] = component;
                    }
                }
                for from in 0..count {
                    for to in 0..count {
                        assert_eq!(
                            component_of[from] == component_of[to],
                            from == to || (reach[from][to] && reach[to][from])
                        );
                        if reach[from][to] {
                            assert!(component_of[from] <= component_of[to]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn long_construction_graphs_use_explicit_traversal_stacks() {
        let count = 32_768;
        let mut graph = SourceBoundConstruction::default();
        for id in 0..count {
            graph.node(ConstructionKey::Cohort(id));
        }
        for id in 1..count {
            graph.depend(id - 1, id, None);
        }
        assert_eq!(graph.components().len(), count);
        graph.depend(count - 1, 0, None);
        let components = graph.components();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].len(), count);
    }
}
