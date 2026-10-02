use super::{
    DependencyCounts, FactTransition, Inhabitance, NodeFormula, PropagationStep, QueueDiscipline,
    least_fixed_point, propagate,
};

#[derive(Default)]
struct Work {
    dependency_inspections: usize,
    evaluations: usize,
    advances: usize,
    notifications: usize,
}

impl Work {
    fn record(&mut self, step: PropagationStep) {
        match step {
            PropagationStep::DependencyInspected => self.dependency_inspections += 1,
            PropagationStep::NodeEvaluated => self.evaluations += 1,
            PropagationStep::NodeAdvanced => self.advances += 1,
            PropagationStep::EdgeNotified => self.notifications += 1,
        }
    }

    fn verify(&self, nodes: &[NodeFormula]) {
        let edges = nodes.iter().map(NodeFormula::degree).sum::<usize>();
        assert_eq!(self.dependency_inspections, edges);
        assert!(self.advances <= 2 * nodes.len());
        assert!(self.notifications <= 2 * edges);
        assert!(self.evaluations <= nodes.len() + self.notifications);
        assert!(
            self.dependency_inspections + self.evaluations + self.advances + self.notifications
                <= 3 * nodes.len() + 5 * edges
        );
    }
}

fn oracle(nodes: &[NodeFormula]) -> Vec<Inhabitance> {
    let mut values = vec![Inhabitance::Uninhabited; nodes.len()];
    for _ in 0..=2 * nodes.len() {
        let next = nodes
            .iter()
            .map(|formula| match formula {
                NodeFormula::Constant(value) => *value,
                NodeFormula::Product(dependencies) => dependencies
                    .iter()
                    .map(|&node| values[node])
                    .min()
                    .unwrap_or(Inhabitance::Inhabited),
                NodeFormula::Sum(dependencies) => dependencies
                    .iter()
                    .map(|&node| values[node])
                    .max()
                    .unwrap_or(Inhabitance::Uninhabited),
            })
            .collect::<Vec<_>>();
        if next == values {
            return values;
        }
        assert!(
            next.iter()
                .zip(&values)
                .all(|(next, previous)| next >= previous)
        );
        values = next;
    }
    panic!("the finite tri-state oracle did not converge");
}

fn check_schedules(nodes: &[NodeFormula], expected: &[Inhabitance]) {
    assert_eq!(least_fixed_point(nodes), expected);
    for reverse_initial in [false, true] {
        for discipline in [QueueDiscipline::Fifo, QueueDiscipline::Lifo] {
            let mut work = Work::default();
            let values = propagate(nodes, reverse_initial, discipline, |step| work.record(step));
            assert_eq!(values, expected);
            work.verify(nodes);
        }
    }
}

#[test]
fn product_and_sum_formulas_use_the_closed_lattice() {
    let leaves = [
        NodeFormula::Constant(Inhabitance::Uninhabited),
        NodeFormula::Constant(Inhabitance::Unknown),
        NodeFormula::Constant(Inhabitance::Inhabited),
    ];
    for (formula, expected) in [
        (NodeFormula::Product(Box::new([])), Inhabitance::Inhabited),
        (NodeFormula::Sum(Box::new([])), Inhabitance::Uninhabited),
        (NodeFormula::Product(Box::new([1, 2])), Inhabitance::Unknown),
        (
            NodeFormula::Product(Box::new([0, 2])),
            Inhabitance::Uninhabited,
        ),
        (NodeFormula::Sum(Box::new([0, 1])), Inhabitance::Unknown),
        (NodeFormula::Sum(Box::new([0, 2])), Inhabitance::Inhabited),
    ] {
        let mut nodes = leaves.to_vec();
        nodes.push(formula);
        check_schedules(
            &nodes,
            &[
                Inhabitance::Uninhabited,
                Inhabitance::Unknown,
                Inhabitance::Inhabited,
                expected,
            ],
        );
    }
}

#[test]
fn dependency_transitions_preserve_multiplicity_and_empty_identities() {
    let mut counts = DependencyCounts::at_bottom(3);
    for (transition, expected) in [
        (FactTransition::UninhabitedToUnknown, [2, 1, 0]),
        (FactTransition::UninhabitedToInhabited, [1, 1, 1]),
        (FactTransition::UnknownToInhabited, [1, 0, 2]),
        (FactTransition::UninhabitedToInhabited, [0, 0, 3]),
    ] {
        counts.advance(transition);
        assert_eq!(counts.0, expected);
        assert_eq!(counts.0.iter().sum::<usize>(), 3);
    }
    assert_eq!(counts.product(), Inhabitance::Inhabited);
    assert_eq!(counts.sum(), Inhabitance::Inhabited);
    assert_eq!(
        DependencyCounts::at_bottom(0).product(),
        Inhabitance::Inhabited
    );
    assert_eq!(
        DependencyCounts::at_bottom(0).sum(),
        Inhabitance::Uninhabited
    );

    for value in [
        Inhabitance::Uninhabited,
        Inhabitance::Unknown,
        Inhabitance::Inhabited,
    ] {
        assert!(FactTransition::between(value, value).is_none());
    }
    assert!(FactTransition::between(Inhabitance::Inhabited, Inhabitance::Unknown).is_none());
    assert!(FactTransition::between(Inhabitance::Unknown, Inhabitance::Uninhabited).is_none());
}

#[test]
fn queued_parents_receive_both_stages_and_parallel_incidence_updates() {
    let nodes = [
        NodeFormula::Constant(Inhabitance::Unknown),
        NodeFormula::Sum(Box::new([0, 2, 2])),
        NodeFormula::Product(Box::new([3])),
        NodeFormula::Constant(Inhabitance::Inhabited),
        NodeFormula::Product(Box::new([1, 1])),
    ];
    let mut work = Work::default();
    let result = propagate(&nodes, false, QueueDiscipline::Fifo, |step| {
        work.record(step)
    });
    assert_eq!(
        result,
        [
            Inhabitance::Unknown,
            Inhabitance::Inhabited,
            Inhabitance::Inhabited,
            Inhabitance::Inhabited,
            Inhabitance::Inhabited
        ]
    );
    assert_eq!(work.advances, 7);
    work.verify(&nodes);
    check_schedules(&nodes, &oracle(&nodes));
}

#[test]
fn self_cycles_and_multiple_components_keep_the_least_fixed_point() {
    let nodes = [
        NodeFormula::Product(Box::new([0])),
        NodeFormula::Sum(Box::new([1, 2])),
        NodeFormula::Constant(Inhabitance::Unknown),
        NodeFormula::Sum(Box::new([3, 4, 4])),
        NodeFormula::Constant(Inhabitance::Inhabited),
        NodeFormula::Product(Box::new([1, 3, 3])),
        NodeFormula::Product(Box::new([6, 4])),
        NodeFormula::Sum(Box::new([])),
        NodeFormula::Product(Box::new([])),
        NodeFormula::Product(Box::new([7, 8])),
    ];
    let expected = [
        Inhabitance::Uninhabited,
        Inhabitance::Unknown,
        Inhabitance::Unknown,
        Inhabitance::Inhabited,
        Inhabitance::Inhabited,
        Inhabitance::Unknown,
        Inhabitance::Uninhabited,
        Inhabitance::Uninhabited,
        Inhabitance::Inhabited,
        Inhabitance::Uninhabited,
    ];
    assert_eq!(oracle(&nodes), expected);
    check_schedules(&nodes, &expected);
}

#[test]
fn all_three_node_graphs_of_degree_at_most_two_match_the_synchronous_oracle() {
    let mut checked = 0;
    for size in 0..=3usize {
        let mut choices = vec![
            NodeFormula::Constant(Inhabitance::Uninhabited),
            NodeFormula::Constant(Inhabitance::Unknown),
            NodeFormula::Constant(Inhabitance::Inhabited),
            NodeFormula::Product(Box::new([])),
            NodeFormula::Sum(Box::new([])),
        ];
        for first in 0..size {
            choices.push(NodeFormula::Product(Box::new([first])));
            choices.push(NodeFormula::Sum(Box::new([first])));
            for second in 0..size {
                choices.push(NodeFormula::Product(Box::new([first, second])));
                choices.push(NodeFormula::Sum(Box::new([first, second])));
            }
        }
        for mut code in 0..choices.len().pow(size as u32) {
            let nodes = (0..size)
                .map(|_| {
                    let formula = choices[code % choices.len()].clone();
                    code /= choices.len();
                    formula
                })
                .collect::<Vec<_>>();
            check_schedules(&nodes, &oracle(&nodes));
            checked += 1;
        }
    }
    assert_eq!(checked, 24_688);
}

fn staggered(width: usize, product: bool) -> Vec<NodeFormula> {
    let mut nodes = vec![NodeFormula::Constant(if product {
        Inhabitance::Inhabited
    } else {
        Inhabitance::Unknown
    })];
    for node in 1..=width {
        nodes.push(NodeFormula::Product(Box::new([if node == width {
            0
        } else {
            node + 1
        }])));
    }
    nodes.push(if product {
        NodeFormula::Product((1..=width).rev().collect::<Vec<_>>().into_boxed_slice())
    } else {
        NodeFormula::Sum((1..=width).collect::<Vec<_>>().into_boxed_slice())
    });
    nodes
}

#[test]
fn wide_staggered_sum_and_product_propagation_have_linear_work() {
    for width in [1, 8, 32, 128, 512, 2048] {
        for product in [false, true] {
            let nodes = staggered(width, product);
            let expected = if product {
                Inhabitance::Inhabited
            } else {
                Inhabitance::Unknown
            };
            check_schedules(&nodes, &vec![expected; nodes.len()]);
        }
    }
}

#[test]
fn fanout_reconvergence_and_wide_cycles_have_linear_work() {
    for width in [1, 8, 32, 128, 512, 2048] {
        let mut fanout = vec![
            NodeFormula::Constant(Inhabitance::Unknown),
            NodeFormula::Constant(Inhabitance::Inhabited),
        ];
        for _ in 0..width {
            fanout.push(NodeFormula::Sum(Box::new([0, 1, 0])));
        }
        fanout.push(NodeFormula::Product(
            (2..2 + width).collect::<Vec<_>>().into_boxed_slice(),
        ));
        let mut expected = vec![Inhabitance::Inhabited; fanout.len()];
        expected[0] = Inhabitance::Unknown;
        check_schedules(&fanout, &expected);

        let mut reconvergent = vec![
            NodeFormula::Constant(Inhabitance::Unknown),
            NodeFormula::Constant(Inhabitance::Inhabited),
        ];
        let mut previous = 0;
        for _ in 0..width {
            let left = reconvergent.len();
            reconvergent.push(NodeFormula::Product(Box::new([previous, 1])));
            let right = reconvergent.len();
            reconvergent.push(NodeFormula::Sum(Box::new([previous, 0])));
            previous = reconvergent.len();
            reconvergent.push(NodeFormula::Product(Box::new([left, right, left])));
        }
        let mut expected = vec![Inhabitance::Unknown; reconvergent.len()];
        expected[1] = Inhabitance::Inhabited;
        check_schedules(&reconvergent, &expected);

        for base in [
            Inhabitance::Uninhabited,
            Inhabitance::Unknown,
            Inhabitance::Inhabited,
        ] {
            let mut cyclic = vec![NodeFormula::Sum(
                (1..=width + 1).collect::<Vec<_>>().into_boxed_slice(),
            )];
            for _ in 0..width {
                cyclic.push(NodeFormula::Product(Box::new([0, 0])));
            }
            cyclic.push(NodeFormula::Constant(base));
            check_schedules(&cyclic, &vec![base; cyclic.len()]);
        }
    }
}

#[test]
fn node_and_dependency_orders_do_not_change_facts() {
    let original = [
        NodeFormula::Constant(Inhabitance::Unknown),
        NodeFormula::Constant(Inhabitance::Inhabited),
        NodeFormula::Product(Box::new([3, 4, 3])),
        NodeFormula::Sum(Box::new([0, 1])),
        NodeFormula::Sum(Box::new([4, 0])),
    ];
    let expected = oracle(&original);
    for reverse_children in [false, true] {
        let mut reordered = vec![NodeFormula::Constant(Inhabitance::Uninhabited); original.len()];
        for (old, formula) in original.iter().enumerate() {
            let remap = |dependencies: &[usize]| {
                let mut mapped = dependencies
                    .iter()
                    .map(|&node| original.len() - 1 - node)
                    .collect::<Vec<_>>();
                if reverse_children {
                    mapped.reverse();
                }
                mapped.into_boxed_slice()
            };
            reordered[original.len() - 1 - old] = match formula {
                NodeFormula::Constant(value) => NodeFormula::Constant(*value),
                NodeFormula::Product(dependencies) => NodeFormula::Product(remap(dependencies)),
                NodeFormula::Sum(dependencies) => NodeFormula::Sum(remap(dependencies)),
            };
        }
        let mapped_expected = expected.iter().rev().copied().collect::<Vec<_>>();
        check_schedules(&reordered, &mapped_expected);
    }
}
