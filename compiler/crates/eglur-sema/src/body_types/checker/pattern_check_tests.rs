use std::cell::Cell;

use super::test_support::with_checker;
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    merges: usize,
    entries: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { merges: 0, entries: 0 }) };
}

pub(super) fn observe_merge() {
    WORK.update(|mut work| {
        work.merges += 1;
        work
    });
}

pub(super) fn observe_entry() {
    WORK.update(|mut work| {
        work.entries += 1;
        work
    });
}

fn with_bindings(count: usize, inspect: impl FnOnce(&[LocalBindingId], TypeId, TypeId)) {
    let parameters = (0..count)
        .map(|index| format!("v{index}: i64"))
        .collect::<Vec<_>>()
        .join(",");
    with_checker(&format!("fn seed({parameters}) -> Unit = ()"), |checker| {
        let bindings = checker
            .names
            .locals()
            .iter()
            .map(|local| local.id())
            .collect::<Vec<_>>();
        assert_eq!(bindings.len(), count);
        let integer = checker.primitive(PrimitiveType::I64).unwrap();
        let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
        inspect(&bindings, integer, boolean);
    });
}

fn check(
    bindings: impl IntoIterator<Item = (LocalBindingId, TypeId)>,
    total: bool,
) -> PatternCheck {
    let mut check = if total {
        PatternCheck::irrefutable()
    } else {
        PatternCheck::variant(
            CanonicalTarget::External(crate::ExternalDefId::new("fixture::Variant").unwrap()),
            PatternCheck::irrefutable(),
        )
    };
    check.bindings.extend(bindings);
    check
}

#[test]
fn all_small_subsets_preserve_left_bias_and_coverage_in_both_size_branches() {
    with_bindings(4, |bindings, integer, boolean| {
        for left in 0_usize..16 {
            for right in 0_usize..16 {
                for flags in 0..4 {
                    let left_type = |index| if index % 2 == 0 { integer } else { boolean };
                    let right_type = |index| if index % 2 == 0 { boolean } else { integer };
                    let mut first = check(
                        bindings.iter().enumerate().filter_map(|(index, &binding)| {
                            (left & (1 << index) != 0).then_some((binding, left_type(index)))
                        }),
                        flags & 1 != 0,
                    );
                    let second = check(
                        bindings.iter().enumerate().filter_map(|(index, &binding)| {
                            (right & (1 << index) != 0).then_some((binding, right_type(index)))
                        }),
                        flags & 2 != 0,
                    );
                    let expected = bindings
                        .iter()
                        .enumerate()
                        .filter_map(|(index, &binding)| {
                            if left & (1 << index) != 0 {
                                Some((binding, left_type(index)))
                            } else if right & (1 << index) != 0 {
                                Some((binding, right_type(index)))
                            } else {
                                None
                            }
                        })
                        .collect::<BTreeMap<_, _>>();

                    WORK.set(Work::default());
                    first.merge(second);
                    assert_eq!(first.bindings, expected, "{left}/{right}/{flags}");
                    assert_eq!(first.irrefutable, flags == 3);
                    assert_eq!(first.coverage.total, flags == 3);
                    assert!(first.coverage.variants.is_empty());
                    assert_eq!(
                        WORK.get(),
                        Work {
                            merges: 1,
                            entries: left.count_ones().min(right.count_ones()) as usize,
                        }
                    );
                }
            }
        }
    });
}

#[test]
fn alias_extension_keeps_inner_types_before_and_after_map_adoption() {
    with_bindings(3, |bindings, integer, boolean| {
        let inner = PatternCheck::binding(bindings[0], boolean)
            .with_binding(bindings[0], integer)
            .with_binding(bindings[1], boolean);
        let mut outer = PatternCheck::binding(bindings[1], integer);
        outer.merge(inner);
        let outer = outer
            .with_binding(bindings[0], integer)
            .with_binding(bindings[2], integer);
        assert_eq!(
            outer.bindings,
            BTreeMap::from([
                (bindings[0], boolean),
                (bindings[1], integer),
                (bindings[2], integer),
            ])
        );
    });
}

fn balanced(mut checks: Vec<PatternCheck>) -> PatternCheck {
    while checks.len() > 1 {
        let mut next = Vec::new();
        let mut pairs = checks.into_iter();
        while let Some(mut first) = pairs.next() {
            if let Some(second) = pairs.next() {
                first.merge(second);
            }
            next.push(first);
        }
        checks = next;
    }
    checks.pop().unwrap()
}

#[test]
fn balanced_disjoint_forest_has_input_logarithmic_entry_moves() {
    for count in [8_usize, 32, 128] {
        with_bindings(count, |bindings, integer, _| {
            let checks = bindings
                .iter()
                .map(|&binding| PatternCheck::binding(binding, integer))
                .collect();
            WORK.set(Work::default());
            let result = balanced(checks);
            assert_eq!(result.bindings.len(), count);
            assert!(result.bindings.values().all(|&ty| ty == integer));
            assert_eq!(
                WORK.get(),
                Work {
                    merges: count - 1,
                    entries: count / 2 * count.ilog2() as usize,
                }
            );
        });
    }
}

#[test]
fn overlapping_forests_preserve_earliest_values_and_an_input_derived_work_bound() {
    with_bindings(4, |bindings, integer, boolean| {
        for count in [8_usize, 32, 128] {
            for balanced_order in [false, true] {
                let masks = (0..count).map(|index| index % 15 + 1).collect::<Vec<_>>();
                let value = |index| if index % 2 == 0 { integer } else { boolean };
                let mut expected = BTreeMap::new();
                for (bit, &binding) in bindings.iter().enumerate() {
                    let first = masks
                        .iter()
                        .position(|mask| mask & (1 << bit) != 0)
                        .unwrap();
                    expected.insert(binding, value(first));
                }
                let checks = masks
                    .iter()
                    .enumerate()
                    .map(|(index, mask)| {
                        check(
                            bindings.iter().enumerate().filter_map(|(bit, &binding)| {
                                (mask & (1 << bit) != 0).then_some((binding, value(index)))
                            }),
                            true,
                        )
                    })
                    .collect::<Vec<_>>();
                let inputs = checks
                    .iter()
                    .map(|check| check.bindings.len())
                    .sum::<usize>();

                WORK.set(Work::default());
                let result = if balanced_order {
                    balanced(checks)
                } else {
                    let mut checks = checks.into_iter();
                    let mut result = checks.next().unwrap();
                    for next in checks {
                        result.merge(next);
                    }
                    result
                };
                assert_eq!(result.bindings, expected);
                assert_eq!(WORK.get().merges, count - 1);
                assert!(WORK.get().entries <= 2 * inputs * (1 + inputs.ilog2() as usize));
            }
        }
    });
}

#[test]
fn growing_overlapping_universes_keep_precedence_across_unbalanced_map_sizes() {
    for count in [8_usize, 32, 128] {
        with_bindings(count, |bindings, integer, boolean| {
            let inputs = (0..count)
                .map(|index| {
                    let width = match index % 4 {
                        0 => 1,
                        1 => count / 2,
                        2 => count / 4,
                        _ => count / 8,
                    };
                    (0..width)
                        .map(|offset| {
                            let key = (7 * index + offset) % count;
                            let ty = if (index + key) % 2 == 0 {
                                integer
                            } else {
                                boolean
                            };
                            (bindings[key], ty)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let entries = inputs.iter().map(Vec::len).sum::<usize>();
            let expected = bindings
                .iter()
                .map(|&binding| {
                    let (_, ty) = inputs
                        .iter()
                        .flat_map(|input| input.iter())
                        .find(|(key, _)| *key == binding)
                        .unwrap();
                    (binding, *ty)
                })
                .collect::<BTreeMap<_, _>>();

            for balanced_order in [false, true] {
                let checks = inputs
                    .iter()
                    .enumerate()
                    .map(|(index, input)| check(input.iter().copied(), index % 3 != 0))
                    .collect::<Vec<_>>();
                WORK.set(Work::default());
                let result = if balanced_order {
                    balanced(checks)
                } else {
                    let mut checks = checks.into_iter();
                    let mut result = checks.next().unwrap();
                    for next in checks {
                        result.merge(next);
                    }
                    result
                };
                assert_eq!(result.bindings, expected);
                assert!(!result.irrefutable);
                assert!(!result.coverage.total);
                assert!(result.coverage.variants.is_empty());
                assert_eq!(WORK.get().merges, count - 1);
                assert!(WORK.get().entries <= 2 * entries * (1 + entries.ilog2() as usize));
            }
        });
    }
}

#[test]
fn real_skewed_and_alias_patterns_do_not_reinsert_growing_binding_maps() {
    for form in ["left", "right", "alias", "single"] {
        for count in [8_usize, 32, if form == "alias" { 64 } else { 128 }] {
            let mut ty = "i64".to_owned();
            let mut pattern = "leaf".to_owned();
            for index in 0..count {
                match form {
                    "left" => {
                        ty = format!("({ty}, i64)");
                        pattern = format!("({pattern}, v{index})");
                    }
                    "right" => {
                        ty = format!("(i64, {ty})");
                        pattern = format!("(v{index}, {pattern})");
                    }
                    "alias" => {
                        ty = format!("({ty},)");
                        pattern = format!("alias_{index} @ ({pattern},)");
                    }
                    _ => {
                        ty = format!("({ty},)");
                        pattern = format!("({pattern},)");
                    }
                }
            }
            let text = format!("fn f(input: {ty}) -> i64 = match input {{ {pattern} => leaf }}");
            with_checker(&text, |checker| {
                WORK.set(Work::default());
                checker.check_all().unwrap();
                assert!(!checker.diagnostics.has_errors(), "{form}/{count}");
                assert_eq!(
                    WORK.get(),
                    Work {
                        merges: if matches!(form, "left" | "right") {
                            2 * count
                        } else {
                            count
                        },
                        entries: if matches!(form, "left" | "right") {
                            count
                        } else {
                            0
                        },
                    },
                    "{form}/{count}"
                );
                assert_eq!(
                    checker.names.locals().len(),
                    if form == "single" { 2 } else { count + 2 }
                );
                for local in checker.names.locals() {
                    let FactState::Known(fact) = checker.locals.state(&local.id()) else {
                        panic!("{} lost its installed type", local.name().spelling());
                    };
                    if local.name().spelling() != "input"
                        && !local.name().spelling().starts_with("alias_")
                    {
                        assert_eq!(
                            checker.signatures.types().kind(fact.ty()).unwrap(),
                            &TypeKind::Primitive(PrimitiveType::I64)
                        );
                    }
                }
            });
        }
    }
}
