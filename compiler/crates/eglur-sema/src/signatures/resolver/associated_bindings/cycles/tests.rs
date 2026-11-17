use super::{BindingCycleStatus, GraphStep, classify, classify_observed};

#[derive(Default)]
struct Work {
    edges: usize,
    nodes: usize,
}

impl Work {
    fn record(&mut self, step: GraphStep) {
        match step {
            GraphStep::EdgeInspected => self.edges += 1,
            GraphStep::NodeVisited => self.nodes += 1,
        }
    }

    fn verify(&self, dependencies: &[Vec<usize>], statuses: &[BindingCycleStatus]) {
        let edges = dependencies.iter().map(Vec::len).sum::<usize>();
        assert!(self.edges >= 3 * edges);
        assert!(self.edges <= 4 * edges);
        assert_eq!(
            self.nodes,
            2 * dependencies.len()
                + statuses
                    .iter()
                    .filter(|status| status.is_affected())
                    .count()
        );
    }
}

fn check(dependencies: &[Vec<usize>], expected: &[BindingCycleStatus]) {
    assert_eq!(classify(dependencies), expected);
    let mut work = Work::default();
    let statuses = classify_observed(dependencies, |step| work.record(step));
    assert_eq!(statuses, expected);
    work.verify(dependencies, &statuses);
}

fn oracle(dependencies: &[Vec<usize>]) -> Vec<BindingCycleStatus> {
    let mut reachable = vec![vec![false; dependencies.len()]; dependencies.len()];
    for (node, edges) in dependencies.iter().enumerate() {
        for &dependency in edges {
            reachable[node][dependency] = true;
        }
    }
    for pivot in 0..dependencies.len() {
        for node in 0..dependencies.len() {
            for goal in 0..dependencies.len() {
                reachable[node][goal] |= reachable[node][pivot] && reachable[pivot][goal];
            }
        }
    }
    (0..dependencies.len())
        .map(|node| {
            if reachable[node][node] {
                BindingCycleStatus::Cyclic
            } else if (0..dependencies.len())
                .any(|goal| reachable[node][goal] && reachable[goal][goal])
            {
                BindingCycleStatus::Dependent
            } else {
                BindingCycleStatus::Independent
            }
        })
        .collect()
}

#[test]
fn all_directed_graphs_through_four_nodes_match_positive_path_closure() {
    let mut checked = 0;
    for size in 0..=4usize {
        for edges in 0..(1usize << (size * size)) {
            let dependencies = (0..size)
                .map(|node| {
                    (0..size)
                        .filter(|&dependency| edges & (1 << (node * size + dependency)) != 0)
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            check(&dependencies, &oracle(&dependencies));
            checked += 1;
        }
    }
    assert_eq!(checked, 66_067);
}

#[test]
fn independent_cycles_and_dependents_survive_identity_edge_order_and_duplicates() {
    let original = vec![
        vec![1, 1],
        vec![0],
        vec![0, 3],
        vec![3],
        vec![2, 3, 2],
        vec![6],
        vec![],
    ];
    let expected = vec![
        BindingCycleStatus::Cyclic,
        BindingCycleStatus::Cyclic,
        BindingCycleStatus::Dependent,
        BindingCycleStatus::Cyclic,
        BindingCycleStatus::Dependent,
        BindingCycleStatus::Independent,
        BindingCycleStatus::Independent,
    ];
    assert_eq!(oracle(&original), expected);
    check(&original, &expected);
    let reordered = original
        .iter()
        .rev()
        .map(|edges| {
            edges
                .iter()
                .rev()
                .map(|&node| original.len() - 1 - node)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let reversed_expected = expected.iter().rev().copied().collect::<Vec<_>>();
    check(&reordered, &reversed_expected);

    let duplicated = reordered
        .iter()
        .map(|edges| edges.iter().copied().chain(edges.iter().copied()).collect())
        .collect::<Vec<_>>();
    check(&duplicated, &reversed_expected);
}

fn chain(size: usize, cycle: bool) -> Vec<Vec<usize>> {
    (0..size)
        .map(|node| {
            if node + 1 < size {
                vec![node + 1]
            } else if cycle {
                vec![node]
            } else {
                vec![]
            }
        })
        .collect()
}

#[test]
fn deep_chains_and_tail_cycles_have_linear_stack_safe_work() {
    for size in [1, 32, 128, 512, 2048, 20_000] {
        check(
            &chain(size, false),
            &vec![BindingCycleStatus::Independent; size],
        );
        let mut expected = vec![BindingCycleStatus::Dependent; size];
        expected[size - 1] = BindingCycleStatus::Cyclic;
        check(&chain(size, true), &expected);
    }
}

#[test]
fn wide_fanout_and_reconvergence_admit_each_affected_node_once() {
    for width in [1, 32, 128, 512, 2048] {
        let mut fanout = vec![vec![0], vec![]];
        for _ in 0..width {
            fanout.push(vec![0, 1, 0]);
        }
        fanout.push((2..width + 2).collect());
        let mut expected = vec![BindingCycleStatus::Dependent; fanout.len()];
        expected[0] = BindingCycleStatus::Cyclic;
        expected[1] = BindingCycleStatus::Independent;
        check(&fanout, &expected);

        let mut reconvergent = vec![vec![0]];
        let mut previous = 0;
        for _ in 0..width {
            let left = reconvergent.len();
            reconvergent.push(vec![previous]);
            let right = reconvergent.len();
            reconvergent.push(vec![previous]);
            previous = reconvergent.len();
            reconvergent.push(vec![left, right, left]);
        }
        let mut expected = vec![BindingCycleStatus::Dependent; reconvergent.len()];
        expected[0] = BindingCycleStatus::Cyclic;
        check(&reconvergent, &expected);
    }
}
