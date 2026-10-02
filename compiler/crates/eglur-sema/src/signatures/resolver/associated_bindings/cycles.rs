#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BindingCycleStatus {
    Independent,
    Cyclic,
    Dependent,
}

impl BindingCycleStatus {
    pub(super) fn is_cyclic(self) -> bool {
        self == Self::Cyclic
    }

    pub(super) fn is_affected(self) -> bool {
        self != Self::Independent
    }
}

enum GraphStep {
    EdgeInspected,
    NodeVisited,
}

struct BindingGraph<'a> {
    dependencies: &'a [Vec<usize>],
    dependents: Vec<Vec<usize>>,
    self_loops: Vec<bool>,
}

impl<'a> BindingGraph<'a> {
    fn new(dependencies: &'a [Vec<usize>], observe: &mut impl FnMut(GraphStep)) -> Self {
        let mut dependents = vec![Vec::new(); dependencies.len()];
        let mut self_loops = vec![false; dependencies.len()];
        for (node, edges) in dependencies.iter().enumerate() {
            for &dependency in edges {
                observe(GraphStep::EdgeInspected);
                dependents[dependency].push(node);
                self_loops[node] |= dependency == node;
            }
        }
        Self {
            dependencies,
            dependents,
            self_loops,
        }
    }

    fn dependency_at(
        &self,
        node: usize,
        index: usize,
        observe: &mut impl FnMut(GraphStep),
    ) -> Option<usize> {
        self.dependencies[node].get(index).copied().inspect(|_| {
            observe(GraphStep::EdgeInspected);
        })
    }

    fn dependents<'b>(
        &'b self,
        node: usize,
        observe: &'b mut impl FnMut(GraphStep),
    ) -> impl Iterator<Item = usize> + 'b {
        self.dependents[node].iter().copied().inspect(|_| {
            observe(GraphStep::EdgeInspected);
        })
    }

    fn finishing_order(&self, observe: &mut impl FnMut(GraphStep)) -> Vec<usize> {
        let mut visited = vec![false; self.dependencies.len()];
        let mut order = Vec::with_capacity(visited.len());
        for start in 0..visited.len() {
            if visited[start] {
                continue;
            }
            visited[start] = true;
            observe(GraphStep::NodeVisited);
            let mut stack = vec![(start, 0)];
            while let Some((node, cursor)) = stack.last_mut() {
                if let Some(next) = self.dependency_at(*node, *cursor, observe) {
                    *cursor += 1;
                    if !visited[next] {
                        visited[next] = true;
                        observe(GraphStep::NodeVisited);
                        stack.push((next, 0));
                    }
                } else {
                    order.push(stack.pop().expect("active DFS frame exists").0);
                }
            }
        }
        order
    }

    fn cycle_members(&self, observe: &mut impl FnMut(GraphStep)) -> Vec<BindingCycleStatus> {
        let order = self.finishing_order(observe);
        let mut assigned = vec![false; self.dependencies.len()];
        let mut statuses = vec![BindingCycleStatus::Independent; assigned.len()];
        for &start in order.iter().rev() {
            if assigned[start] {
                continue;
            }
            assigned[start] = true;
            let mut pending = vec![start];
            let mut members = Vec::new();
            while let Some(node) = pending.pop() {
                observe(GraphStep::NodeVisited);
                members.push(node);
                for next in self.dependents(node, observe) {
                    if !assigned[next] {
                        assigned[next] = true;
                        pending.push(next);
                    }
                }
            }
            if members.len() > 1 || self.self_loops[start] {
                for node in members {
                    statuses[node] = BindingCycleStatus::Cyclic;
                }
            }
        }
        statuses
    }

    fn propagate_affected(
        &self,
        statuses: &mut [BindingCycleStatus],
        observe: &mut impl FnMut(GraphStep),
    ) {
        let mut pending = statuses
            .iter()
            .enumerate()
            .filter_map(|(node, status)| status.is_cyclic().then_some(node))
            .collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            observe(GraphStep::NodeVisited);
            for dependent in self.dependents(node, observe) {
                if statuses[dependent] == BindingCycleStatus::Independent {
                    statuses[dependent] = BindingCycleStatus::Dependent;
                    pending.push(dependent);
                }
            }
        }
    }
}

pub(super) fn classify(dependencies: &[Vec<usize>]) -> Vec<BindingCycleStatus> {
    classify_observed(dependencies, |_| {})
}

fn classify_observed(
    dependencies: &[Vec<usize>],
    mut observe: impl FnMut(GraphStep),
) -> Vec<BindingCycleStatus> {
    let graph = BindingGraph::new(dependencies, &mut observe);
    let mut statuses = graph.cycle_members(&mut observe);
    graph.propagate_affected(&mut statuses, &mut observe);
    statuses
}

#[cfg(test)]
mod tests;
