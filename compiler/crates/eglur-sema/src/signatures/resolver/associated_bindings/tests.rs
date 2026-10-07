use eglur_diagnostics::{DiagnosticFailure, DiagnosticSnapshot};

use super::super::tests::with_resolver;
use super::*;

#[test]
fn missing_obligations_complete_structural_poison_before_supplied_rhs() {
    for count in [1, 2, 8, 32] {
        for value in ["i64", "Missing", "&i64", "Self::Keep"] {
            let required = (0..count)
                .map(|index| format!("type A{index}\n"))
                .collect::<String>();
            let text = format!(
                "trait Mark {{ {required} type Keep }}\nstruct Host {{}}\nimpl Mark for Host {{ type Keep = {value} }}\n"
            );
            with_resolver(&[&text], |resolver, sources| {
                let source = &sources[0];
                let hir = source.hir().unwrap();
                let owner = resolver
                    .collection
                    .modules()
                    .next()
                    .unwrap()
                    .declarations()
                    .find(|declaration| declaration.kind() == DeclarationKind::Impl)
                    .unwrap()
                    .id();
                let declaration = resolver.collection.declaration(owner).unwrap();
                let span = source.origin_span(declaration.origin()).unwrap();
                let ItemKind::Impl(value) = hir.item(declaration.item().unwrap()).kind() else {
                    panic!("fixture contains an impl");
                };
                resolver.prime_source_impl_headers().unwrap();
                let header = resolver.impl_headers.get(&owner).unwrap().clone();
                let checkpoint = resolver.diagnostics.checkpoint();
                let Resolution::Poisoned(poison) = resolver
                    .resolve_source_impl_bindings(
                        owner,
                        source,
                        hir,
                        value,
                        header.trait_ref(),
                        header.target(),
                    )
                    .unwrap()
                else {
                    panic!("missing obligations poison the impl");
                };

                let module = resolver.collection.module(owner.module()).unwrap();
                for &item in &value.items {
                    let ItemKind::AssociatedTypeBinding(binding) = hir.item(item).kind() else {
                        continue;
                    };
                    assert!(matches!(
                        resolver
                            .declarations
                            .state(&CanonicalTarget::Source(module.item_def(item).unwrap())),
                        FactState::Poisoned(_)
                    ));
                    assert!(matches!(
                        resolver
                            .occurrences
                            .state(&TypeOccurrenceId::new(source.file(), binding.value)),
                        FactState::Unperformed
                    ));
                }
                let all = resolver
                    .diagnostics
                    .roots_since_within(&checkpoint, span)
                    .unwrap()
                    .unwrap();
                resolver
                    .diagnostics
                    .record_failure(DiagnosticFailure::Derivative(poison.clone()))
                    .unwrap();
                resolver
                    .diagnostics
                    .select_preceding_roots(&poison, &all)
                    .unwrap();
                let snapshot = std::mem::take(resolver.diagnostics).finalize(None);
                let DiagnosticSnapshot::Rejected(snapshot) = snapshot else {
                    panic!("structural poison retains an error");
                };
                assert_eq!(snapshot.diagnostics().len(), count);
                for diagnostic in snapshot.diagnostics() {
                    assert_eq!(diagnostic.code().as_str(), "TRAIT_IMPL_ASSOC_TYPE_MISMATCH");
                    assert_eq!(diagnostic.primary_span().as_source().unwrap(), span);
                    assert_eq!(
                        diagnostic.data().get("reason"),
                        Some(&DiagnosticDataValue::from("missing"))
                    );
                }
            });
        }
    }
}

#[test]
fn distinct_associated_targets_with_the_same_name_remain_distinct_obligations() {
    with_resolver(
        &["trait Mark { type Item\ntype Item }\nstruct Host {}\nimpl Mark for Host {}\n"],
        |resolver, sources| {
            let source = &sources[0];
            let hir = source.hir().unwrap();
            let owner = resolver
                .collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .find(|declaration| declaration.kind() == DeclarationKind::Impl)
                .unwrap()
                .id();
            let declaration = resolver.collection.declaration(owner).unwrap();
            let ItemKind::Impl(value) = hir.item(declaration.item().unwrap()).kind() else {
                panic!("fixture contains an impl");
            };
            resolver.prime_source_impl_headers().unwrap();
            let header = resolver.impl_headers.get(&owner).unwrap().clone();
            assert!(matches!(
                resolver
                    .resolve_source_impl_bindings(
                        owner,
                        source,
                        hir,
                        value,
                        header.trait_ref(),
                        header.target()
                    )
                    .unwrap(),
                Resolution::Poisoned(_)
            ));
            let snapshot = std::mem::take(resolver.diagnostics).finalize(None);
            assert_eq!(snapshot.diagnostics().len(), 2);
            assert_eq!(snapshot.diagnostics()[0], snapshot.diagnostics()[1]);
            assert_eq!(
                snapshot.diagnostics()[0].data().get("associated_type"),
                Some(&DiagnosticDataValue::from("Item"))
            );
        },
    );
}

#[test]
fn complete_binding_structures_publish_known_values_without_missing_roots() {
    with_resolver(
        &[
            "trait Mark { type A\ntype B }\nstruct Host {}\nimpl Mark for Host { type A = i64\ntype B = Bool }\n",
        ],
        |resolver, sources| {
            let source = &sources[0];
            let hir = source.hir().unwrap();
            let owner = resolver
                .collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .find(|declaration| declaration.kind() == DeclarationKind::Impl)
                .unwrap()
                .id();
            let ItemKind::Impl(value) = hir
                .item(
                    resolver
                        .collection
                        .declaration(owner)
                        .unwrap()
                        .item()
                        .unwrap(),
                )
                .kind()
            else {
                panic!("fixture contains an impl");
            };
            resolver.prime_source_impl_headers().unwrap();
            let header = resolver.impl_headers.get(&owner).unwrap().clone();
            let Resolution::Known(bindings) = resolver
                .resolve_source_impl_bindings(
                    owner,
                    source,
                    hir,
                    value,
                    header.trait_ref(),
                    header.target(),
                )
                .unwrap()
            else {
                panic!("complete bindings remain known");
            };
            assert_eq!(bindings.len(), 2);
            assert!(
                std::mem::take(resolver.diagnostics)
                    .finalize(None)
                    .diagnostics()
                    .is_empty()
            );
        },
    );
}
