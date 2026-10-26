use std::cell::Cell;

use super::*;
use crate::body_types::checker::test_support::with_checker;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    occurrences: usize,
    types: usize,
    resets: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { occurrences: 0, types: 0, resets: 0 }) };
}

pub(super) fn observe_occurrence() {
    WORK.update(|work| Work {
        occurrences: work.occurrences + 1,
        ..work
    });
}

pub(super) fn observe_type() {
    WORK.update(|work| Work {
        types: work.types + 1,
        ..work
    });
}

pub(super) fn observe_reset() {
    WORK.update(|work| Work {
        resets: work.resets + 1,
        ..work
    });
}

#[derive(Clone, Copy)]
struct Site {
    binding: LocalBindingId,
    pattern: PatternId,
    origin: OriginId,
}

fn with_sites(count: usize, inspect: impl FnOnce(ModuleId, PatternId, &[Site], TypeId, TypeId)) {
    let types = vec!["i64"; count].join(",");
    let names = (0..count)
        .map(|index| format!("v{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("fn f(input: ({types},)) -> i64 = match input {{ ({names},) => 0 }}");
    with_checker(&source, |checker| {
        let integer = checker.primitive(PrimitiveType::I64).unwrap();
        let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
        let declaration = checker
            .collection
            .modules()
            .flat_map(|module| module.declarations())
            .find(|declaration| declaration.name().is_some_and(|name| name.text() == "f"))
            .unwrap()
            .id();
        let (module, _, hir, item) = checker.source_item(declaration).unwrap();
        let ItemKind::Function(function) = item.kind() else {
            panic!("function")
        };
        let body = hir.body(function.body);
        let root = body
            .expressions()
            .find_map(|(_, expression)| match expression.kind() {
                ExprKind::Match(value) => Some(value.arms[0].pattern),
                _ => None,
            })
            .unwrap();
        let sites = body
            .patterns()
            .filter_map(|(pattern, value)| match value.kind() {
                PatternKind::Binding(name)
                    if name.text().is_some_and(|text| text.starts_with('v')) =>
                {
                    Some(Site {
                        binding: checker
                            .names
                            .local_binding(module, body.id(), name.origin())
                            .unwrap(),
                        pattern,
                        origin: name.origin(),
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sites.len(), count);
        inspect(module, root, &sites, integer, boolean);
    });
}

#[test]
fn all_small_overlaps_keep_first_typed_values_and_first_duplicate() {
    with_sites(4, |module, root, sites, integer, boolean| {
        let mut scratch = PatternBindingScratch::default();
        for left in 0usize..16 {
            for right in 0usize..16 {
                scratch.begin(module, root);
                let mut expected = BTreeMap::new();
                let mut duplicate = None;
                for (mask, ty) in [(left, integer), (right, boolean)] {
                    for (index, site) in sites.iter().enumerate() {
                        if mask & (1 << index) != 0 {
                            if expected.contains_key(&site.binding) {
                                duplicate.get_or_insert(site.origin);
                            }
                            expected.entry(site.binding).or_insert(ty);
                            scratch
                                .observe(site.binding, site.pattern, site.origin)
                                .unwrap();
                            scratch.type_binding(site.binding, ty).unwrap();
                        }
                    }
                }
                for site in sites {
                    match expected.get(&site.binding) {
                        Some(&ty) => assert_eq!(scratch.ty(site.binding), Ok(ty)),
                        None => assert_eq!(
                            scratch.ty(site.binding),
                            Err(BodyTypingError::InconsistentPatternBinding(site.binding))
                        ),
                    }
                }
                assert_eq!(
                    scratch.duplicate(module, root).map(|value| value.origin),
                    duplicate
                );
            }
        }
    });
}

#[test]
fn alias_occurrences_are_preorder_but_typed_values_are_child_first() {
    with_checker(
        "fn f(input: (i64, Bool)) -> i64 = match input { same @ (same, other) => 0 }",
        |checker| {
            let integer = checker.primitive(PrimitiveType::I64).unwrap();
            checker.check_all().unwrap();
            let same = checker
                .names
                .locals()
                .iter()
                .find(|local| local.name().text() == "same")
                .unwrap();
            assert_eq!(same.origins().len(), 2);
            assert_eq!(checker.pattern_bindings.ty(same.id()), Ok(integer));
            let row = checker
                .pattern_bindings
                .row(same.id().module(), same.id().body())
                .unwrap();
            assert_eq!(row.duplicate.unwrap().origin, same.origins()[1]);
            assert!(matches!(
                checker.locals.state(&same.id()),
                FactState::Poisoned(_)
            ));
            let snapshot =
                std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
            assert_eq!(
                snapshot
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| diagnostic.code().as_str())
                    .collect::<Vec<_>>(),
                ["PATTERN_DUPLICATE_BINDING"]
            );
        },
    );
}

#[test]
fn remembered_duplicates_are_not_diagnostics_after_structural_failure() {
    for pattern in ["same @ (true, same)", "same @ (same, true)"] {
        with_checker(
            &format!("fn f(input: (i64, i64)) -> i64 = match input {{ {pattern} => 0 }}"),
            |checker| {
                checker.check_all().unwrap();
                for local in checker
                    .names
                    .locals()
                    .iter()
                    .filter(|local| local.name().text() == "same")
                {
                    assert!(matches!(
                        checker.locals.state(&local.id()),
                        FactState::Poisoned(_)
                    ));
                }
                let snapshot =
                    std::mem::replace(checker.diagnostics, DiagnosticStore::new()).finalize(None);
                assert_eq!(
                    snapshot
                        .diagnostics()
                        .iter()
                        .map(|diagnostic| diagnostic.code().as_str())
                        .collect::<Vec<_>>(),
                    ["PATTERN_TYPE_MISMATCH"]
                );
            },
        );
    }
}

#[test]
fn repeated_roots_reset_only_touched_slots_and_keep_final_local_types() {
    for count in [1, 8, 64, 256] {
        let roots = (0..count)
            .map(|index| format!("let out{index} = match input {{ x{index} => 0 }}\n"))
            .collect::<String>();
        with_checker(
            &format!("fn f(input: i64) -> Unit {{ {roots}yield () }}"),
            |checker| {
                WORK.set(Work::default());
                checker.check_all().unwrap();
                assert!(!checker.diagnostics.has_errors());
                assert_eq!(
                    WORK.get(),
                    Work {
                        occurrences: count,
                        types: count,
                        resets: count - 1
                    }
                );
                let first = checker.names.locals().first().unwrap().id();
                let row = checker
                    .pattern_bindings
                    .row(first.module(), first.body())
                    .unwrap();
                let limit = checker
                    .names
                    .locals()
                    .iter()
                    .filter(|local| {
                        local.id().module() == first.module() && local.id().body() == first.body()
                    })
                    .map(|local| local.id().local() as usize + 1)
                    .max()
                    .unwrap();
                assert!(row.slots.len() <= limit);
                assert_eq!(row.touched.len(), 1);
                for local in checker.names.locals() {
                    assert!(matches!(
                        checker.locals.state(&local.id()),
                        FactState::Known(_)
                    ));
                }
                eprintln!(
                    "roots={count} slot_capacity={} touched_capacity={} row_size={} slot_size={}",
                    row.slots.capacity(),
                    row.touched.capacity(),
                    size_of::<BodyScratch>(),
                    size_of::<BindingSlot>()
                );
            },
        );
    }
}

#[test]
fn interleaved_body_owners_keep_their_rows_and_reject_missing_writes() {
    with_checker(
        "fn f(x: i64) -> i64 = match x { y => y }\nfn g(x: i64) -> i64 = match x { y => y }",
        |checker| {
            let integer = checker.primitive(PrimitiveType::I64).unwrap();
            let mut rows = Vec::new();
            for declaration in checker
                .collection
                .modules()
                .flat_map(|module| module.declarations())
            {
                if !declaration
                    .name()
                    .is_some_and(|name| matches!(name.text(), "f" | "g"))
                {
                    continue;
                }
                let (module, _, hir, item) = checker.source_item(declaration.id()).unwrap();
                let ItemKind::Function(function) = item.kind() else {
                    panic!("function")
                };
                let body = hir.body(function.body);
                for (pattern, value) in body.patterns() {
                    if let PatternKind::Binding(name) = value.kind()
                        && name.text() == Some("y")
                    {
                        let binding = checker
                            .names
                            .local_binding(module, body.id(), name.origin())
                            .unwrap();
                        rows.push((
                            module,
                            Site {
                                binding,
                                pattern,
                                origin: name.origin(),
                            },
                        ));
                    }
                }
            }
            assert_eq!(rows.len(), 2);
            let mut scratch = PatternBindingScratch::default();
            for &(module, site) in &rows {
                scratch.begin(module, site.pattern);
                scratch
                    .observe(site.binding, site.pattern, site.origin)
                    .unwrap();
                scratch.type_binding(site.binding, integer).unwrap();
            }
            let left = rows[0].1;
            let right = rows[1].1;
            let pointer = scratch
                .row(rows[0].0, left.pattern.body())
                .unwrap()
                .slots
                .as_ptr();
            scratch.begin(rows[0].0, left.pattern);
            assert_eq!(
                scratch
                    .row(rows[0].0, left.pattern.body())
                    .unwrap()
                    .slots
                    .as_ptr(),
                pointer
            );
            assert!(scratch.ty(left.binding).is_err());
            assert_eq!(scratch.ty(right.binding), Ok(integer));
            let boolean = checker.primitive(PrimitiveType::Bool).unwrap();
            scratch
                .observe(left.binding, left.pattern, left.origin)
                .unwrap();
            scratch.type_binding(left.binding, integer).unwrap();
            let other_module = ModuleId::from_index(2).unwrap();
            let other = LocalBindingId::from_index(
                other_module,
                left.binding.body(),
                left.binding.local() as usize,
            )
            .unwrap();
            scratch.begin(other_module, left.pattern);
            scratch.observe(other, left.pattern, left.origin).unwrap();
            scratch.type_binding(other, boolean).unwrap();
            assert_eq!(scratch.ty(left.binding), Ok(integer));
            assert_eq!(scratch.ty(other), Ok(boolean));
            let missing = LocalBindingId::from_index(
                ModuleId::from_index(u32::MAX as usize).unwrap(),
                left.pattern.body(),
                0,
            )
            .unwrap();
            let capacity = scratch.modules.capacity();
            assert!(scratch.type_binding(missing, integer).is_err());
            assert!(scratch.observe(missing, left.pattern, left.origin).is_err());
            assert_eq!(scratch.modules.capacity(), capacity);
            assert!(
                scratch
                    .observe(left.binding, right.pattern, right.origin)
                    .is_err()
            );
        },
    );
}
