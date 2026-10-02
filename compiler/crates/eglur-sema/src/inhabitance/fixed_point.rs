use std::collections::VecDeque;

use super::Inhabitance;

pub(super) type NodeId = usize;

#[derive(Clone)]
pub(super) enum NodeFormula {
    Constant(Inhabitance),
    Product(Box<[NodeId]>),
    Sum(Box<[NodeId]>),
}

impl NodeFormula {
    fn degree(&self) -> usize {
        match self {
            Self::Constant(_) => 0,
            Self::Product(dependencies) | Self::Sum(dependencies) => dependencies.len(),
        }
    }

    fn inspect_dependencies<'a>(
        &'a self,
        observe: &'a mut impl FnMut(PropagationStep),
    ) -> impl Iterator<Item = NodeId> + 'a {
        let dependencies = match self {
            Self::Constant(_) => &[],
            Self::Product(dependencies) | Self::Sum(dependencies) => dependencies.as_ref(),
        };
        dependencies
            .iter()
            .copied()
            .inspect(|_| observe(PropagationStep::DependencyInspected))
    }

    fn initial_state(&self) -> NodeState {
        let aggregate = match self {
            Self::Constant(value) => FormulaState::Constant(*value),
            Self::Product(_) => FormulaState::Product(DependencyCounts::at_bottom(self.degree())),
            Self::Sum(_) => FormulaState::Sum(DependencyCounts::at_bottom(self.degree())),
        };
        NodeState {
            aggregate,
            value: Inhabitance::Uninhabited,
        }
    }
}

#[derive(Clone, Copy)]
enum FactTransition {
    UninhabitedToUnknown,
    UninhabitedToInhabited,
    UnknownToInhabited,
}

impl FactTransition {
    fn between(previous: Inhabitance, next: Inhabitance) -> Option<Self> {
        match (previous, next) {
            (Inhabitance::Uninhabited, Inhabitance::Unknown) => Some(Self::UninhabitedToUnknown),
            (Inhabitance::Uninhabited, Inhabitance::Inhabited) => {
                Some(Self::UninhabitedToInhabited)
            }
            (Inhabitance::Unknown, Inhabitance::Inhabited) => Some(Self::UnknownToInhabited),
            _ => None,
        }
    }

    fn endpoints(self) -> (Inhabitance, Inhabitance) {
        match self {
            Self::UninhabitedToUnknown => (Inhabitance::Uninhabited, Inhabitance::Unknown),
            Self::UninhabitedToInhabited => (Inhabitance::Uninhabited, Inhabitance::Inhabited),
            Self::UnknownToInhabited => (Inhabitance::Unknown, Inhabitance::Inhabited),
        }
    }
}

struct DependencyCounts([usize; 3]);

impl DependencyCounts {
    fn at_bottom(degree: usize) -> Self {
        Self([degree, 0, 0])
    }

    fn advance(&mut self, transition: FactTransition) {
        let (previous, next) = transition.endpoints();
        debug_assert!(self.0[previous as usize] > 0);
        self.0[previous as usize] -= 1;
        self.0[next as usize] += 1;
    }

    fn product(&self) -> Inhabitance {
        if self.0[Inhabitance::Uninhabited as usize] > 0 {
            Inhabitance::Uninhabited
        } else if self.0[Inhabitance::Unknown as usize] > 0 {
            Inhabitance::Unknown
        } else {
            Inhabitance::Inhabited
        }
    }

    fn sum(&self) -> Inhabitance {
        if self.0[Inhabitance::Inhabited as usize] > 0 {
            Inhabitance::Inhabited
        } else if self.0[Inhabitance::Unknown as usize] > 0 {
            Inhabitance::Unknown
        } else {
            Inhabitance::Uninhabited
        }
    }
}

enum FormulaState {
    Constant(Inhabitance),
    Product(DependencyCounts),
    Sum(DependencyCounts),
}

impl FormulaState {
    fn value(&self) -> Inhabitance {
        match self {
            Self::Constant(value) => *value,
            Self::Product(counts) => counts.product(),
            Self::Sum(counts) => counts.sum(),
        }
    }

    fn dependency_advanced(&mut self, transition: FactTransition) {
        match self {
            Self::Constant(_) => unreachable!("constant formulas have no dependency edges"),
            Self::Product(counts) | Self::Sum(counts) => counts.advance(transition),
        }
    }
}

struct NodeState {
    aggregate: FormulaState,
    value: Inhabitance,
}

impl NodeState {
    fn advance(&mut self) -> Option<FactTransition> {
        let next = self.aggregate.value();
        debug_assert!(next >= self.value);
        let transition = FactTransition::between(self.value, next)?;
        self.value = next;
        Some(transition)
    }
}

enum QueueDiscipline {
    Fifo,
    #[cfg(test)]
    Lifo,
}

impl QueueDiscipline {
    fn pop(&self, pending: &mut VecDeque<NodeId>) -> Option<NodeId> {
        match self {
            Self::Fifo => pending.pop_front(),
            #[cfg(test)]
            Self::Lifo => pending.pop_back(),
        }
    }
}

enum PropagationStep {
    DependencyInspected,
    NodeEvaluated,
    NodeAdvanced,
    EdgeNotified,
}

pub(super) fn least_fixed_point(nodes: &[NodeFormula]) -> Vec<Inhabitance> {
    propagate(nodes, false, QueueDiscipline::Fifo, |_| {})
}

fn propagate(
    nodes: &[NodeFormula],
    reverse_initial: bool,
    discipline: QueueDiscipline,
    mut observe: impl FnMut(PropagationStep),
) -> Vec<Inhabitance> {
    let mut states = nodes
        .iter()
        .map(NodeFormula::initial_state)
        .collect::<Vec<_>>();
    let mut reverse = vec![Vec::new(); nodes.len()];
    for (node, formula) in nodes.iter().enumerate() {
        for dependency in formula.inspect_dependencies(&mut observe) {
            // Each occurrence contributes to the parent's degree, including parallel edges.
            reverse[dependency].push(node);
        }
    }

    let mut pending = if reverse_initial {
        (0..nodes.len()).rev().collect::<VecDeque<_>>()
    } else {
        (0..nodes.len()).collect::<VecDeque<_>>()
    };
    let mut queued = vec![true; nodes.len()];

    while let Some(node) = discipline.pop(&mut pending) {
        queued[node] = false;
        observe(PropagationStep::NodeEvaluated);
        let Some(transition) = states[node].advance() else {
            continue;
        };
        observe(PropagationStep::NodeAdvanced);

        for &dependent in &reverse[node] {
            states[dependent].aggregate.dependency_advanced(transition);
            observe(PropagationStep::EdgeNotified);
            if !queued[dependent] {
                queued[dependent] = true;
                pending.push_back(dependent);
            }
        }
    }

    states.into_iter().map(|state| state.value).collect()
}

#[cfg(test)]
mod tests;
