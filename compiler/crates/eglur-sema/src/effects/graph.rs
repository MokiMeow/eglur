use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::DefId;

/// Returns source-callable SCCs in callee-first order.
pub(super) fn dependency_first_sccs(
    nodes: impl IntoIterator<Item = DefId>,
    dependencies: impl IntoIterator<Item = (DefId, DefId)>,
) -> Result<Box<[Box<[DefId]>]>, EffectGraphError> {
    let nodes = nodes.into_iter().collect::<BTreeSet<_>>();
    let node_by_index = nodes.iter().copied().collect::<Vec<_>>();
    let index_by_node = node_by_index
        .iter()
        .enumerate()
        .map(|(index, &node)| (node, index))
        .collect::<BTreeMap<_, _>>();
    let mut forward = vec![Vec::new(); node_by_index.len()];
    let mut reverse = vec![Vec::new(); node_by_index.len()];
    for (caller, callee) in dependencies {
        let caller_index = index_by_node
            .get(&caller)
            .copied()
            .ok_or(EffectGraphError::UnknownNode(caller))?;
        let callee_index = index_by_node
            .get(&callee)
            .copied()
            .ok_or(EffectGraphError::UnknownNode(callee))?;
        forward[caller_index].push(callee_index);
        reverse[callee_index].push(caller_index);
    }
    for edges in forward.iter_mut().chain(reverse.iter_mut()) {
        edges.sort_unstable();
        edges.dedup();
    }

    let finishing_order = finishing_order(&forward);
    let mut component_by_node = vec![usize::MAX; node_by_index.len()];
    let mut components = Vec::new();
    for &start in finishing_order.iter().rev() {
        if component_by_node[start] != usize::MAX {
            continue;
        }
        let component = components.len();
        let mut members = Vec::new();
        let mut stack = vec![start];
        component_by_node[start] = component;
        while let Some(node) = stack.pop() {
            members.push(node_by_index[node]);
            for &next in reverse[node].iter().rev() {
                if component_by_node[next] == usize::MAX {
                    component_by_node[next] = component;
                    stack.push(next);
                }
            }
        }
        members.sort_unstable();
        components.push(members);
    }

    let mut dependents = vec![BTreeSet::new(); components.len()];
    let mut dependency_count = vec![0_usize; components.len()];
    for (caller, callees) in forward.iter().enumerate() {
        let caller_component = component_by_node[caller];
        for &callee in callees {
            let callee_component = component_by_node[callee];
            if caller_component != callee_component
                && dependents[callee_component].insert(caller_component)
            {
                dependency_count[caller_component] += 1;
            }
        }
    }

    let mut ready = dependency_count
        .iter()
        .enumerate()
        .filter(|&(_, &count)| count == 0)
        .map(|(component, _)| (components[component][0], component))
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::with_capacity(components.len());
    while let Some(&(key, component)) = ready.first() {
        ready.remove(&(key, component));
        ordered.push(components[component].clone().into_boxed_slice());
        for &dependent in &dependents[component] {
            dependency_count[dependent] -= 1;
            if dependency_count[dependent] == 0 {
                ready.insert((components[dependent][0], dependent));
            }
        }
    }
    debug_assert_eq!(ordered.len(), components.len());
    Ok(ordered.into_boxed_slice())
}

fn finishing_order(adjacency: &[Vec<usize>]) -> Vec<usize> {
    let mut visited = vec![false; adjacency.len()];
    let mut order = Vec::with_capacity(adjacency.len());
    for start in 0..adjacency.len() {
        if visited[start] {
            continue;
        }
        visited[start] = true;
        let mut stack = vec![(start, 0_usize)];
        while let Some((node, edge_index)) = stack.last_mut() {
            if let Some(&next) = adjacency[*node].get(*edge_index) {
                *edge_index += 1;
                if !visited[next] {
                    visited[next] = true;
                    stack.push((next, 0));
                }
            } else {
                let (finished, _) = stack.pop().expect("active DFS stack is non-empty");
                order.push(finished);
            }
        }
    }
    order
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EffectGraphError {
    UnknownNode(DefId),
}

impl fmt::Display for EffectGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownNode(node) => {
                write!(
                    formatter,
                    "effect dependency references unknown callable {node:?}"
                )
            }
        }
    }
}

impl std::error::Error for EffectGraphError {}

#[cfg(test)]
mod tests {
    use crate::{DefId, ModuleId};

    use super::{EffectGraphError, dependency_first_sccs};

    fn node(local: usize) -> DefId {
        DefId::from_index(ModuleId::from_index(0).unwrap(), local).unwrap()
    }

    fn projection(
        nodes: impl IntoIterator<Item = DefId>,
        dependencies: impl IntoIterator<Item = (DefId, DefId)>,
    ) -> Vec<Vec<u32>> {
        dependency_first_sccs(nodes, dependencies)
            .unwrap()
            .iter()
            .map(|component| component.iter().map(|member| member.local()).collect())
            .collect()
    }

    #[test]
    fn cycles_and_isolated_callables_form_dependency_first_units() {
        let nodes = [node(0), node(1), node(2), node(3)];
        let edges = [
            (node(0), node(1)),
            (node(1), node(0)),
            (node(2), node(1)),
            (node(3), node(3)),
        ];

        assert_eq!(projection(nodes, edges), vec![vec![0, 1], vec![2], vec![3]]);
    }

    #[test]
    fn order_is_independent_of_input_enumeration_and_duplicate_edges() {
        let nodes = [node(0), node(1), node(2), node(3)];
        let edges = [
            (node(0), node(2)),
            (node(2), node(3)),
            (node(0), node(1)),
            (node(1), node(3)),
            (node(0), node(1)),
        ];
        let mut shuffled_nodes = nodes;
        shuffled_nodes.reverse();
        let mut shuffled_edges = edges;
        shuffled_edges.reverse();

        assert_eq!(
            projection(nodes, edges),
            vec![vec![3], vec![1], vec![2], vec![0]]
        );
        assert_eq!(
            projection(nodes, edges),
            projection(shuffled_nodes, shuffled_edges)
        );
    }

    #[test]
    fn unknown_dependency_endpoint_is_rejected() {
        assert_eq!(
            dependency_first_sccs([node(0)], [(node(0), node(1))]),
            Err(EffectGraphError::UnknownNode(node(1)))
        );
    }

    #[test]
    fn deep_call_chains_do_not_use_the_call_stack() {
        const CALLABLE_COUNT: usize = 20_000;
        let nodes = (0..CALLABLE_COUNT).map(node).collect::<Vec<_>>();
        let edges = (1..CALLABLE_COUNT)
            .map(|caller| (node(caller), node(caller - 1)))
            .collect::<Vec<_>>();
        let components = dependency_first_sccs(nodes, edges).unwrap();

        assert_eq!(components.len(), CALLABLE_COUNT);
        assert_eq!(components.first().unwrap().as_ref(), &[node(0)]);
        assert_eq!(
            components.last().unwrap().as_ref(),
            &[node(CALLABLE_COUNT - 1)]
        );
    }
}
