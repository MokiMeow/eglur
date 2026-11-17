use super::test_support::{Work, reset, work};
use super::*;
use eglur_diagnostics::{
    BoundaryRootKey, Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStore, Poison, PrimarySpan, Severity,
};
use std::collections::BTreeSet;

fn roots(store: &mut DiagnosticStore, count: usize) -> Vec<Poison> {
    (0..count)
        .map(|index| {
            let diagnostic = Diagnostic::new(
                DiagnosticCode::new(format!("ROOT_{index:04}")).unwrap(),
                Severity::Error,
                DiagnosticPhase::new("type").unwrap(),
                "independent failure",
                PrimarySpan::Unavailable,
            );
            let root = DiagnosticRoot::boundary(
                BoundaryRootKey::new(format!("root-{index}")).unwrap(),
                diagnostic,
            )
            .unwrap();
            store.record_failure(DiagnosticFailure::Root(root)).unwrap()
        })
        .collect()
}

fn union(poisons: &[Poison]) -> Poison {
    let mut result = poisons[0].clone();
    for poison in &poisons[1..] {
        result = result.merge(poison).unwrap();
    }
    result
}

fn assert_roots(
    mut store: DiagnosticStore,
    roots: &[Poison],
    poison: &Poison,
    expected: &BTreeSet<usize>,
) {
    store.select_preceding_roots(&roots[0], poison).unwrap();
    let retained = store
        .finalize(None)
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.code().as_str().to_owned())
        .collect::<BTreeSet<_>>();
    let expected = (0..roots.len())
        .filter(|index| !expected.contains(index))
        .map(|index| format!("ROOT_{index:04}"))
        .collect();
    assert_eq!(retained, expected);
}

#[test]
fn staged_groups_preserve_exact_distinct_shared_and_overlapping_roots() {
    for count in [1, 2, 3, 8, 32, 128] {
        for family in 0..3 {
            let mut store = DiagnosticStore::new();
            let roots = roots(&mut store, count + 3);
            let inputs = (0..count)
                .map(|index| match family {
                    0 => roots[index + 1].clone(),
                    1 => roots[1].clone(),
                    2 => roots[index + 1].merge(&roots[index + 2]).unwrap(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            let expected = match family {
                0 => (1..=count).collect(),
                1 => BTreeSet::from([1]),
                2 => (1..=count + 1).collect(),
                _ => unreachable!(),
            };

            reset();
            let mut group = OwnedPoisonUnion::new(inputs[0].clone());
            for poison in &inputs[1..] {
                group = group.include(poison.clone()).unwrap();
            }
            let poison = group.finish();
            let work = work();
            assert!(work.pairs <= 1);
            assert!(work.includes <= inputs.len());
            assert!(work.finishes <= 1);
            assert_eq!(work.pairs, usize::from(inputs.len() > 1));
            assert_eq!(work.finishes, usize::from(inputs.len() > 2));

            assert_roots(store, &roots, &poison, &expected);
        }
    }
}

#[test]
fn wide_singletons_and_pairs_keep_the_pairwise_kernel() {
    for count in [1, 2] {
        let mut store = DiagnosticStore::new();
        let roots = roots(&mut store, 129);
        let wide = union(&roots[1..]);

        reset();
        let mut group = OwnedPoisonUnion::new(wide.clone());
        for _ in 1..count {
            group = group.include(wide.clone()).unwrap();
        }
        let poison = group.finish();
        assert_eq!(
            work(),
            Work {
                pairs: count - 1,
                includes: 0,
                finishes: 0,
            }
        );
        assert_roots(store, &roots, &poison, &(1..roots.len()).collect());
    }
}
