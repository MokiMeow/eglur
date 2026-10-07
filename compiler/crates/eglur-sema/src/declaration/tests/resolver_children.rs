use super::*;

#[test]
fn role_queries_preserve_every_collected_child_and_hir_generic_slot() {
    let text = "\
        fn top[A, A, B](value: B) -> B = value\n\
        type Alias[A, B] = B\n\
        trait Marker[A, B] { fn method[C, D](self: &Self, value: D) -> D }\n\
        struct Host[A, B] {\n\
          field value: A\n\
          fn method[C, D](self: &Self, value: D) -> D = value\n\
        }\n\
        enum Choice[A, B] { Empty\n Pair { first: A, second: B } }\n\
        impl[A, B] Marker[A, B] for Host[A, B] {}\n";
    with_sources(&[("first", text), ("second", text)], |sources| {
        let collection = performed(collect_declarations(sources, &Support).unwrap());
        for module in collection.modules() {
            let hir = sources
                .iter()
                .find(|source| source.file() == module.file())
                .unwrap()
                .hir()
                .unwrap();
            for owner in module.declarations() {
                let children = module.children(owner.id()).collect::<Vec<_>>();
                for child in &children {
                    let expected = children
                        .iter()
                        .filter(|candidate| candidate.kind() == child.kind())
                        .map(|candidate| candidate.id())
                        .collect::<Vec<_>>();
                    assert_eq!(module.child_ids(owner.id(), child.kind()), expected);
                }

                let Some(item) = owner.item() else { continue };
                let generics = match hir.item(item).kind() {
                    ItemKind::Function(value) => &value.generic_parameters,
                    ItemKind::Trait(value) => &value.generic_parameters,
                    ItemKind::Impl(value) => &value.generic_parameters,
                    ItemKind::Struct(value) => &value.generic_parameters,
                    ItemKind::Enum(value) => &value.generic_parameters,
                    ItemKind::TypeAlias(value) => &value.generic_parameters,
                    _ => continue,
                };
                let ids = module.child_ids(owner.id(), DeclarationKind::GenericParameter);
                assert_eq!(ids.len(), generics.len());
                for (index, (&id, parameter)) in ids.iter().zip(generics.iter()).enumerate() {
                    assert_eq!(module.source_generic_index(id), Some(index));
                    assert_eq!(module.declaration(id).unwrap().origin(), parameter.origin);
                }
                assert_eq!(module.source_generic_index(owner.id()), None);
            }
            let foreign = collection
                .modules()
                .find(|other| other.id() != module.id())
                .unwrap()
                .declarations()
                .next()
                .unwrap()
                .id();
            assert!(
                module
                    .child_ids(foreign, DeclarationKind::GenericParameter)
                    .is_empty()
            );
            assert_eq!(module.source_generic_index(foreign), None);
            let missing = DefId::from_index(module.id(), module.declarations.len()).unwrap();
            assert!(
                module
                    .child_ids(missing, DeclarationKind::Function)
                    .is_empty()
            );
            assert_eq!(module.source_generic_index(missing), None);
        }
    });
}

#[test]
fn role_and_generic_index_work_tracks_only_prepared_rows_and_queries() {
    for count in [8, 32, 128] {
        let parameters = (0..count)
            .map(|index| format!("T{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!(
            "struct Host[A] {{ fn method[{parameters}](self: &Self, value: Bool) -> Bool = value }}\n"
        );
        ROLE_WORK.set(RoleWork::default());
        let collection = collect(&[("first", &text), ("second", &text)]);
        let edges = 2 * (count + 2);
        let generics = 2 * (count + 1);
        assert_eq!(
            ROLE_WORK.get(),
            RoleWork {
                insertions: edges,
                generic_index_insertions: generics,
                ..RoleWork::default()
            }
        );
        let stored_edges = collection
            .modules()
            .map(|module| {
                module
                    .children_by_kind
                    .values()
                    .map(Vec::len)
                    .sum::<usize>()
            })
            .sum::<usize>();
        let stored_generics = collection
            .modules()
            .map(|module| module.source_generic_indices.len())
            .sum::<usize>();
        assert_eq!(stored_edges, edges);
        assert_eq!(stored_generics, generics);

        let owners = collection
            .modules()
            .flat_map(|module| {
                module
                    .declarations()
                    .map(move |declaration| (module, declaration.id()))
            })
            .collect::<Vec<_>>();
        DECLARATION_VISITS.set(0);
        for (module, owner) in &owners {
            let ids = module.child_ids(*owner, DeclarationKind::GenericParameter);
            for (index, &id) in ids.iter().enumerate() {
                assert_eq!(module.source_generic_index(id), Some(index));
            }
            assert_eq!(
                module
                    .child_ids(*owner, DeclarationKind::GenericParameter)
                    .as_ptr(),
                ids.as_ptr()
            );
        }
        assert_eq!(DECLARATION_VISITS.get(), 0);
        assert_eq!(
            ROLE_WORK.get(),
            RoleWork {
                insertions: edges,
                queries: 2 * owners.len(),
                generic_index_insertions: generics,
                generic_index_queries: generics
            }
        );
    }
}
