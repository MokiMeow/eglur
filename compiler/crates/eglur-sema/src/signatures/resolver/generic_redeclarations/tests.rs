use std::cell::Cell;

use super::super::tests::with_resolver;
use super::*;
use crate::Declaration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    inventory: usize,
    raw_rows: usize,
    enters: usize,
    leaves: usize,
    child_reads: usize,
    direct: usize,
    membership: usize,
    activated: usize,
    deactivated: usize,
    peak_names: usize,
    output: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work {
        inventory: 0, raw_rows: 0, enters: 0, leaves: 0, child_reads: 0,
        direct: 0, membership: 0, activated: 0, deactivated: 0, peak_names: 0, output: 0,
    }) };
}

pub(super) fn observe_inventory() {
    WORK.update(|mut work| {
        work.inventory += 1;
        work
    });
}

pub(super) fn observe_raw_row() {
    WORK.update(|mut work| {
        work.raw_rows += 1;
        work
    });
}

pub(super) fn observe_enter() {
    WORK.update(|mut work| {
        work.enters += 1;
        work
    });
}

pub(super) fn observe_leave() {
    WORK.update(|mut work| {
        work.leaves += 1;
        work
    });
}

pub(super) fn observe_child() {
    WORK.update(|mut work| {
        work.child_reads += 1;
        work
    });
}

pub(super) fn observe_direct() {
    WORK.update(|mut work| {
        work.direct += 1;
        work
    });
}

pub(super) fn observe_membership() {
    WORK.update(|mut work| {
        work.membership += 1;
        work
    });
}

pub(super) fn observe_activation(live: usize) {
    WORK.update(|mut work| {
        work.activated += 1;
        work.peak_names = work.peak_names.max(live);
        work
    });
}

pub(super) fn observe_deactivation() {
    WORK.update(|mut work| {
        work.deactivated += 1;
        work
    });
}

pub(super) fn observe_output() {
    WORK.update(|mut work| {
        work.output += 1;
        work
    });
}

fn ancestor_union_classification(collection: &DeclarationCollection) -> BTreeSet<DefId> {
    let mut by_owner = BTreeMap::<DefId, Vec<(DefId, Box<str>)>>::new();
    for declaration in collection
        .modules()
        .flat_map(|module| module.declarations())
    {
        if declaration.kind() != DeclarationKind::GenericParameter {
            continue;
        }
        let (Some(owner), Some(name)) = (declaration.parent(), declaration.name()) else {
            continue;
        };
        if !is_reserved_type_name(name.text()) {
            by_owner
                .entry(owner)
                .or_default()
                .push((declaration.id(), Box::from(name.text())));
        }
    }

    let mut result = BTreeSet::new();
    for (&owner, parameters) in &by_owner {
        let mut ancestors = BTreeSet::new();
        let mut parent = collection.declaration(owner).and_then(Declaration::parent);
        while let Some(current) = parent {
            if let Some(parameters) = by_owner.get(&current) {
                ancestors.extend(parameters.iter().map(|(_, name)| name.clone()));
            }
            parent = collection
                .declaration(current)
                .and_then(Declaration::parent);
        }
        let mut local = BTreeSet::new();
        for (id, name) in parameters {
            if !local.insert(name.clone()) || ancestors.contains(name.as_ref()) {
                result.insert(*id);
            }
        }
    }
    result
}

#[test]
fn raw_name_classification_matches_ancestor_union_and_isolates_siblings() {
    let cases = [
        "fn valid[A,B](value:B)->B=value\n",
        "fn duplicate[A,A,B](value:B)->B=value\n",
        "struct Host[A] { fn method[A,B](self:&Self,value:B)->B=value }\n",
        "struct Host[A,A] { fn method[A,B](self:&Self,value:B)->B=value }\n",
        "struct Host[Box] { fn method[Box,B](self:&Self,value:B)->B=value }\n",
        "struct Host[Bool] { fn method[Bool,B](self:&Self,value:B)->B=value }\n",
        "struct Host[T] { fn left[U](self:&Self,value:U)->U=value\n fn right[U](self:&Self,value:U)->U=value }\n",
        "struct Host[T] { fn left[T](self:&Self,value:T)->T=value\n fn right[T](self:&Self,value:T)->T=value }\n",
        "struct Host[U] { fn left[T,T](self:&Self,value:T)->T=value\n fn right[T](self:&Self,value:T)->T=value }\n",
        "fn reserved[Box,Box,A,A](value:A)->A=value\n",
        "type Alias[T,T]=T\n struct Other[T,T] {}\n",
        "trait Link { type Item }\n struct Host[T] {}\n impl[T] Link for Host[T] { type Item=T\n fn method[T,U](self:&Self,value:U)->U=value }\n",
        "trait Marker[T] { fn method[T,U](self:&Self,value:U)->U=value }\n",
        "enum Choice[T,T] { Empty }\n",
        "fn first[T](value:T)->T=value\n fn second[T,T](value:T)->T=value\n",
        "trait Marker[A] { fn left[A](self:&Self,value:A)->A=value\n fn right[B](self:&Self,value:B)->B=value }\n",
    ];
    for text in cases {
        WORK.set(Work::default());
        with_resolver(&[text], |resolver, _| {
            assert_eq!(
                resolver.source_generic_redeclarations,
                ancestor_union_classification(resolver.collection)
            );
            assert_eq!(
                WORK.get().output,
                resolver.source_generic_redeclarations.len()
            );
        });
    }
    for texts in [
        [cases[3], cases[9], cases[7]],
        [cases[7], cases[3], cases[9]],
    ] {
        WORK.set(Work::default());
        with_resolver(&texts, |resolver, _| {
            assert_eq!(
                resolver.source_generic_redeclarations,
                ancestor_union_classification(resolver.collection)
            );
            assert_eq!(
                WORK.get().output,
                resolver.source_generic_redeclarations.len()
            );
        });
    }
}

#[test]
fn scope_work_tracks_input_and_edges_not_parent_width_times_methods() {
    for count in [8_usize, 32, 128] {
        let parameters = (0..count)
            .map(|i| format!("T{i:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let methods = (0..count)
            .map(|i| format!("fn method_{i:03}[U](self:&Self,value:U)->U=value\n"))
            .collect::<String>();
        let cases = [
            (
                format!("fn wide[{parameters}](value:T000)->T000=value\n"),
                count,
                count + 1,
                count,
            ),
            (
                format!("struct Host[{parameters}] {{ {methods} }}\n"),
                2 * count,
                3 * count + 1,
                count + 1,
            ),
            (
                format!("trait Marker[{parameters}] {{ {methods} }}\n"),
                2 * count,
                3 * count + 1,
                count + 1,
            ),
        ];
        for (text, generics, declarations, peak_names) in cases {
            WORK.set(Work::default());
            with_resolver(&[&text], |resolver, _| {
                assert!(resolver.source_generic_redeclarations.is_empty());
                assert_eq!(
                    WORK.get(),
                    Work {
                        inventory: declarations,
                        raw_rows: generics,
                        enters: declarations,
                        leaves: declarations,
                        child_reads: declarations - 1,
                        direct: generics,
                        membership: generics,
                        activated: generics,
                        deactivated: generics,
                        peak_names,
                        output: 0,
                    }
                );
            });
        }
    }
    for (text, declarations) in [
        ("fn plain(value:Bool)->Bool=value\n", 1),
        ("fn reserved[Box](value:Bool)->Bool=value\n", 2),
    ] {
        WORK.set(Work::default());
        with_resolver(&[text], |resolver, _| {
            assert!(resolver.source_generic_redeclarations.is_empty());
            assert_eq!(
                WORK.get(),
                Work {
                    inventory: declarations,
                    ..Work::default()
                }
            );
        });
    }
}

#[test]
fn overlapping_raw_names_survive_every_inner_scope_exit() {
    let mut scopes = ScopeNames::default();
    let names = BTreeSet::from(["same"]);
    for _ in 0..4096 {
        scopes.enter(&names);
        assert!(scopes.contains("same"));
    }
    assert_eq!(scopes.counts.len(), 1);
    for _ in 0..4095 {
        scopes.leave(&names);
        assert!(scopes.contains("same"));
    }
    scopes.leave(&names);
    assert!(!scopes.contains("same"));
    assert!(scopes.counts.is_empty());
}
