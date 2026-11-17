use std::cell::Cell;

use super::super::tests::with_resolver;
use super::super::{SignatureResolver, TypeStructureValidation};
use super::*;
use crate::{Declaration, FactState};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    declarations: usize,
    entries: usize,
    rows: usize,
    views: usize,
    lookups: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work {
        declarations: 0, entries: 0, rows: 0, views: 0, lookups: 0,
    }) };
}

pub(super) fn observe_declaration() {
    WORK.update(|mut work| {
        work.declarations += 1;
        work
    });
}

pub(super) fn observe_entry() {
    WORK.update(|mut work| {
        work.entries += 1;
        work
    });
}

pub(super) fn observe_row() {
    WORK.update(|mut work| {
        work.rows += 1;
        work
    });
}

pub(super) fn observe_view() {
    WORK.update(|mut work| {
        work.views += 1;
        work
    });
}

pub(super) fn observe_lookup() {
    WORK.update(|mut work| {
        work.lookups += 1;
        work
    });
}

fn flat_names(
    resolver: &SignatureResolver<'_, '_>,
    owner: DefId,
) -> BTreeMap<Box<str>, GenericParameterId> {
    let mut chain = Vec::new();
    let mut cursor = Some(owner);
    while let Some(current) = cursor {
        chain.push(current);
        cursor = resolver
            .collection
            .declaration(current)
            .and_then(Declaration::parent);
    }
    chain.reverse();

    let mut names = BTreeMap::new();
    for declaration in chain {
        for generic in resolver.child_declarations(declaration, DeclarationKind::GenericParameter) {
            if resolver.source_generic_redeclarations.contains(&generic) {
                continue;
            }
            if let Some(name) = resolver.collection.declaration(generic).unwrap().name() {
                names
                    .entry(Box::<str>::from(name.text()))
                    .or_insert(GenericParameterId::Source(generic));
            }
        }
    }
    names
}

#[test]
fn indexed_views_match_flattened_name_recovery_and_share_clones() {
    with_resolver(
        &[
            "struct Host[A, Box] { fn method[A, B, Box](self: &Self, value: B) -> B = value }\n\
         fn other[C, C, D](value: D) -> D = value\n\
         type Alias[E] = E\n enum Choice[F] { One { first: F } }\n",
        ],
        |resolver, _| {
            assert_eq!(
                resolver
                    .source_generic_names
                    .by_owner
                    .values()
                    .map(BTreeMap::len)
                    .sum::<usize>(),
                8
            );
            let declarations = resolver
                .collection
                .modules()
                .flat_map(|module| module.declarations())
                .collect::<Vec<_>>();
            let owners = declarations
                .iter()
                .map(|declaration| declaration.id())
                .collect::<Vec<_>>();
            let mut queries = declarations
                .iter()
                .filter_map(|declaration| declaration.name().map(|name| name.text().to_owned()))
                .collect::<Vec<_>>();
            queries.push("absent".into());

            for owner in owners {
                let expected = flat_names(resolver, owner);
                let context = resolver.source_context(owner, false).unwrap();
                for query in &queries {
                    assert_eq!(context.generics.get(query), expected.get(query.as_str()));
                    assert_eq!(
                        context.generics.contains_key(query),
                        expected.contains_key(query.as_str())
                    );
                }

                let cloned = context.clone();
                assert!(Rc::ptr_eq(&context.generics.index, &cloned.generics.index));
                assert!(Rc::ptr_eq(&context.generics.chain, &cloned.generics.chain));
            }
        },
    );
}

#[test]
fn signature_preparation_stores_each_generic_name_once() {
    for count in [8_usize, 32, 128] {
        let parameters = (0..count)
            .map(|index| format!("T{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let last = format!("T{:03}", count - 1);
        let methods = (0..count)
            .map(|index| format!("fn method_{index:03}[U](self: &Self, value: U) -> U = value\n"))
            .collect::<String>();
        let cases = [
            (
                format!("fn wide[{parameters}](value: {last}) -> {last} = value\n"),
                count,
                count + 1,
                2 * count + 1,
                2,
            ),
            (
                format!("struct Host[{parameters}] {{ {methods} }}\n"),
                2 * count,
                3 * count + 1,
                5 * count,
                4 * count,
            ),
            (
                format!("trait Marker[{parameters}] {{ {methods} }}\n"),
                2 * count,
                3 * count + 1,
                5 * count,
                4 * count,
            ),
        ];

        for (text, rows, declarations, views, lookups) in cases {
            WORK.set(Work::default());
            with_resolver(&[&text], |resolver, _| {
                resolver.resolve_trusted().unwrap();
                resolver.validate_language_items().unwrap();
                resolver.resolve_source_generics().unwrap();
                resolver.prime_source_impl_headers().unwrap();
                resolver.validate_source_impl_headers().unwrap();
                resolver.resolve_source_declarations().unwrap();
                resolver.validate_source_overload_families().unwrap();
                resolver.resolve_body_type_occurrences().unwrap();
                resolver.index_enum_variant_names().unwrap();

                assert_eq!(
                    WORK.get(),
                    Work {
                        declarations,
                        entries: rows,
                        rows,
                        views,
                        lookups
                    }
                );
                assert_eq!(
                    resolver
                        .source_generic_names
                        .by_owner
                        .values()
                        .map(BTreeMap::len)
                        .sum::<usize>(),
                    rows
                );
                for declaration in resolver
                    .collection
                    .modules()
                    .flat_map(|module| module.declarations())
                {
                    assert!(matches!(
                        resolver
                            .declarations
                            .state(&crate::CanonicalTarget::Source(declaration.id())),
                        FactState::Known(_)
                    ));
                }
            });
        }
    }
}

#[test]
fn shared_names_do_not_freeze_impl_self_or_request_flags() {
    use super::super::super::applications::TypeApplicationRegion;

    let mut retained = None;
    let mut weak = None;
    with_resolver(
        &[
            "trait Link { type Item\n fn pick(self: &Self, value: Self::Item) -> Self::Item }\n\
         struct Host[T] {}\n\
         impl[T] Link for Host[T] { type Item = T\n fn pick(self: &Self, value: Self::Item) -> Self::Item = value }\n",
        ],
        |resolver, _| {
            let module = resolver.collection.modules().next().unwrap();
            let binding = module
                .declarations()
                .find(|declaration| declaration.kind() == DeclarationKind::AssociatedTypeBinding)
                .unwrap()
                .id();
            let method = module
                .declarations()
                .find(|declaration| {
                    declaration.kind() == DeclarationKind::Function
                        && declaration.parent().is_some_and(|parent| {
                            module.declaration(parent).unwrap().kind() == DeclarationKind::Impl
                        })
                })
                .unwrap()
                .id();

            let before = resolver.source_context(binding, true).unwrap();
            assert!(before.self_binding.is_none());
            assert_eq!(
                before.application_region,
                TypeApplicationRegion::ImplBinding
            );
            resolver.resolve_source_generics().unwrap();
            resolver.prime_source_impl_headers().unwrap();
            let after = resolver.source_context(binding, true).unwrap();
            assert!(after.self_binding.is_some());
            assert_eq!(before.generics.get("T"), after.generics.get("T"));
            assert!(Rc::ptr_eq(&before.generics.index, &after.generics.index));
            assert!(
                resolver
                    .source_context(binding, false)
                    .unwrap()
                    .self_binding
                    .is_none()
            );

            let original = resolver.source_context(method, true).unwrap();
            let mut body = original.clone();
            body.application_region = TypeApplicationRegion::ImplMethodBody;
            body.structure_validation = TypeStructureValidation::AfterBindingCycles;
            let fresh = resolver.source_context(method, true).unwrap();
            assert_eq!(
                original.application_region,
                TypeApplicationRegion::ImplMethodSignature
            );
            assert_eq!(fresh.application_region, original.application_region);
            assert!(original.structure_validation == TypeStructureValidation::Immediate);
            assert!(fresh.structure_validation == TypeStructureValidation::Immediate);

            weak = Some(Rc::downgrade(&after.generics.index));
            retained = Some(after.generics.clone());
        },
    );
    let names = retained.unwrap();
    assert!(names.contains_key("T"));
    assert!(weak.as_ref().unwrap().upgrade().is_some());
    drop(names);
    assert!(weak.unwrap().upgrade().is_none());
}
