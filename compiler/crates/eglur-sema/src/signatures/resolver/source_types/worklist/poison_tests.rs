use std::cell::Cell;

use super::super::poison_tests::{Step, Work, aggregate, assert_roots, roots};
use super::*;
use crate::signatures::resolver::tests::with_resolver;

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { inputs: 0, completed: 0 }) };
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Input => work.inputs += 1,
            Step::Complete => work.completed += 1,
        }
        work
    });
}

fn with_children(count: usize, inspect: impl FnOnce(TypeChildren, [TypeId; 3])) {
    with_resolver(&["type Seed = (i64, Bool)\n"], |resolver, sources| {
        let source = &sources[0];
        let declaration = resolver
            .collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .next()
            .unwrap();
        let owner = declaration.id();
        let ItemKind::TypeAlias(alias) = source
            .hir()
            .unwrap()
            .item(declaration.item().unwrap())
            .kind()
        else {
            panic!("seed alias");
        };
        let request = TypeRequest {
            site: Rc::new(SourceSite {
                owner,
                file: source.file(),
                context: resolver.source_context(owner, false).unwrap(),
            }),
            type_ref: alias.value,
            position: TypePosition::Ordinary,
        };
        let values = [
            resolver.types.primitive(crate::PrimitiveType::I64).unwrap(),
            resolver
                .types
                .primitive(crate::PrimitiveType::Bool)
                .unwrap(),
            resolver
                .types
                .primitive(crate::PrimitiveType::Unit)
                .unwrap(),
        ];
        WORK.set(Work::default());
        inspect(TypeChildren::new(vec![request; count]), values);
    });
}

fn complete(children: TypeChildren) -> Poison {
    let result = children.finish();
    assert!(matches!(&result, Resolution::Poisoned(_)));
    let Resolution::Poisoned(poison) = result else {
        panic!("poisoned child results");
    };
    poison
}

#[test]
fn known_empty_and_unperformed_children_preserve_values_and_cursor() {
    with_children(3, |mut children, values| {
        for value in values {
            assert!(children.next().is_some());
            children.accept(Resolution::Known(value)).unwrap();
        }
        assert!(children.next().is_none());
        let Resolution::Known(actual) = children.finish() else {
            panic!("known values");
        };
        assert_eq!(actual, values);
        assert_eq!(WORK.get(), Work::default());
    });
    with_children(0, |children, _| {
        assert!(children.next().is_none());
        let Resolution::Known(values) = children.finish() else {
            panic!("empty values");
        };
        assert!(values.is_empty());
        assert_eq!(WORK.get(), Work::default());
    });
    with_children(2, |mut children, values| {
        children.accept(Resolution::Known(values[0])).unwrap();
        children.accept(Resolution::Unperformed).unwrap();
        assert_eq!(children.next, 2);
        assert_eq!(children.values, [values[0]]);
        assert!(matches!(children.finish(), Resolution::Unperformed));
        assert_eq!(WORK.get(), Work::default());
    });
}

#[test]
fn distinct_children_complete_once_in_either_order() {
    for count in [8, 32, 128] {
        for reverse in [false, true] {
            with_children(count, |mut children, _| {
                let mut store = DiagnosticStore::new();
                let inputs = roots(&mut store, count);
                for index in 0..count {
                    let index = if reverse { count - index - 1 } else { index };
                    children
                        .accept(Resolution::Poisoned(inputs[index].clone()))
                        .unwrap();
                    assert_eq!(WORK.get().completed, 0);
                }
                assert!(children.next().is_none());
                let poison = complete(children);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: count,
                        completed: 1
                    }
                );
                assert_roots(store, &inputs, &poison, 0..count);
            });
        }
    }
}

#[test]
fn all_nonempty_subset_pairs_keep_exact_child_roots() {
    for left in 1..32 {
        for right in 1..32 {
            with_children(2, |mut children, _| {
                let mut store = DiagnosticStore::new();
                let inputs = roots(&mut store, 5);
                for mask in [left, right] {
                    children
                        .accept(Resolution::Poisoned(aggregate(
                            &inputs,
                            (0..5).filter(|index| mask & (1 << index) != 0),
                        )))
                        .unwrap();
                }
                let poison = complete(children);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: 2,
                        completed: 1
                    }
                );
                assert_roots(
                    store,
                    &inputs,
                    &poison,
                    (0..5).filter(|index| (left | right) & (1 << index) != 0),
                );
            });
        }
    }
}

#[test]
fn repeated_wide_and_overlapping_inputs_preserve_poison_precedence() {
    for count in [8, 32, 128] {
        for mode in ["single", "wide", "overlap"] {
            with_children(count + 3, |mut children, values| {
                let mut store = DiagnosticStore::new();
                let inputs = roots(&mut store, count);
                children.accept(Resolution::Unperformed).unwrap();
                children.accept(Resolution::Known(values[1])).unwrap();
                for index in 0..count {
                    let poison = match mode {
                        "single" => inputs[0].clone(),
                        "wide" => aggregate(&inputs, 0..count),
                        "overlap" => aggregate(&inputs, [index, (index + 1) % count]),
                        _ => unreachable!(),
                    };
                    children.accept(Resolution::Poisoned(poison)).unwrap();
                    assert_eq!(WORK.get().completed, 0);
                }
                children.accept(Resolution::Unperformed).unwrap();
                assert_eq!(children.values, [values[1]]);
                let poison = complete(children);
                assert_eq!(
                    WORK.get(),
                    Work {
                        inputs: count,
                        completed: 1
                    }
                );
                let expected = if mode == "single" { 0..1 } else { 0..count };
                assert_roots(store, &inputs, &poison, expected);
            });
        }
    }
}

#[test]
fn foreign_child_error_retains_prior_roots_and_does_not_advance_cursor() {
    with_children(3, |mut children, values| {
        let mut store = DiagnosticStore::new();
        let inputs = roots(&mut store, 2);
        let mut foreign_store = DiagnosticStore::new();
        let foreign = roots(&mut foreign_store, 2);
        children.accept(Resolution::Known(values[0])).unwrap();
        children
            .accept(Resolution::Poisoned(inputs[0].clone()))
            .unwrap();
        let error = children
            .accept(Resolution::Poisoned(foreign[0].clone()))
            .unwrap_err();
        assert_eq!(
            error,
            SignatureResolutionError::DiagnosticStore(DiagnosticStoreError::ForeignPoison)
        );
        assert_eq!(children.next, 2);
        assert!(children.next().is_some());
        assert_eq!(children.values, [values[0]]);
        children
            .accept(Resolution::Poisoned(inputs[1].clone()))
            .unwrap();
        assert!(children.next().is_none());
        let poison = complete(children);
        assert_eq!(
            WORK.get(),
            Work {
                inputs: 3,
                completed: 1
            }
        );
        assert_roots(store, &inputs, &poison, 0..2);
    });
}

#[test]
fn first_poison_can_come_from_any_store() {
    with_children(1, |mut children, _| {
        let mut store = DiagnosticStore::new();
        let inputs = roots(&mut store, 1);
        children
            .accept(Resolution::Poisoned(inputs[0].clone()))
            .unwrap();
        let poison = complete(children);
        assert_eq!(
            WORK.get(),
            Work {
                inputs: 1,
                completed: 1
            }
        );
        assert_roots(store, &inputs, &poison, [0]);
    });
}
