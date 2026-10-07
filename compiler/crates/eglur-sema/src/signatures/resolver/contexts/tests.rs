use std::cell::Cell;

use super::super::tests::with_resolver;
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    lookups: usize,
    hits: usize,
    builds: usize,
    queries: usize,
    arguments: usize,
    prepared: usize,
    peak_arguments: usize,
    rows: usize,
    peak_slots: usize,
    nominal: usize,
    trait_substitution: usize,
    self_type: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work {
        lookups: 0, hits: 0, builds: 0, queries: 0, arguments: 0, prepared: 0,
        peak_arguments: 0, rows: 0, peak_slots: 0, nominal: 0, trait_substitution: 0,
        self_type: 0,
    }) };
}

pub(super) fn observe_lookup() {
    WORK.update(|mut work| {
        work.lookups += 1;
        work
    });
}

pub(super) fn observe_hit() {
    WORK.update(|mut work| {
        work.hits += 1;
        work
    });
}

pub(super) fn observe_build() {
    WORK.update(|mut work| {
        work.builds += 1;
        work
    });
}

pub(super) fn observe_query() {
    WORK.update(|mut work| {
        work.queries += 1;
        work
    });
}

pub(super) fn observe_argument() {
    WORK.update(|mut work| {
        work.arguments += 1;
        work
    });
}

pub(super) fn observe_prepared(rows: usize) {
    WORK.update(|mut work| {
        work.prepared += rows;
        work.peak_arguments = work.peak_arguments.max(rows);
        work
    });
}

pub(super) fn observe_row(rows: usize) {
    WORK.update(|mut work| {
        work.rows += 1;
        work.peak_slots = work.peak_slots.max(rows);
        work
    });
}

pub(super) fn observe_nominal() {
    WORK.update(|mut work| {
        work.nominal += 1;
        work
    });
}

pub(super) fn observe_trait_substitution() {
    WORK.update(|mut work| {
        work.trait_substitution += 1;
        work
    });
}

pub(super) fn observe_self_type() {
    WORK.update(|mut work| {
        work.self_type += 1;
        work
    });
}

fn image(binding: &SelfBinding) -> (TypeId, Option<TraitRef>) {
    (binding.ty, binding.trait_ref.clone())
}

#[test]
fn static_self_is_lazy_and_preserves_graph_identity_and_request_flags() {
    let cases = [
        (
            "struct Host[T] { fn method[U](self:&Self,value:U)->U=value }\n",
            false,
            1,
        ),
        (
            "trait Marker[T] { fn method[U](self:&Self,value:U)->U=value }\n",
            true,
            1,
        ),
        (
            "struct Host { fn method[U](self:&Self,value:U)->U=value }\n",
            false,
            0,
        ),
        (
            "trait Marker { fn method[U](self:&Self,value:U)->U=value }\n",
            true,
            0,
        ),
    ];
    for (text, is_trait, generics) in cases {
        WORK.set(Work::default());
        with_resolver(&[text], |resolver, _| {
            let owner = resolver
                .collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .find(|d| d.kind() == DeclarationKind::Function)
                .unwrap()
                .id();
            assert!(resolver.static_self_bindings.is_empty());
            let registry = resolver.generic_types.clone();
            let before = (
                resolver.types.type_count(),
                resolver.types.substitution_count(),
            );
            assert!(
                resolver
                    .source_context(owner, false)
                    .unwrap()
                    .self_binding
                    .is_none()
            );
            assert_eq!(
                (
                    resolver.types.type_count(),
                    resolver.types.substitution_count()
                ),
                before
            );
            assert_eq!(WORK.get(), Work::default());

            let first = resolver.source_context(owner, true).unwrap();
            let binding = image(first.self_binding.as_ref().unwrap());
            let after = (
                resolver.types.type_count(),
                resolver.types.substitution_count(),
            );
            assert_eq!(after.0, before.0 + 1);
            assert_eq!(after.1, before.1 + usize::from(generics != 0));
            assert_eq!(resolver.static_self_bindings.len(), 1);
            assert_eq!(
                WORK.get(),
                Work {
                    lookups: 1,
                    builds: 1,
                    queries: 1,
                    arguments: generics,
                    prepared: generics,
                    peak_arguments: generics,
                    rows: 1,
                    peak_slots: 1,
                    nominal: usize::from(!is_trait),
                    trait_substitution: usize::from(is_trait),
                    self_type: usize::from(is_trait),
                    ..Work::default()
                }
            );

            let mut edited = first.clone();
            edited.structure_validation = TypeStructureValidation::AfterBindingCycles;
            edited.application_region =
                super::super::super::applications::TypeApplicationRegion::ImplMethodBody;
            let again = resolver.source_context(owner, true).unwrap();
            assert_eq!(image(again.self_binding.as_ref().unwrap()), binding);
            assert!(again.structure_validation == TypeStructureValidation::Immediate);
            assert_eq!(again.application_region, first.application_region);
            resolver.resolve_source_generics().unwrap();
            let resolved = resolver.source_context(owner, true).unwrap();
            assert_eq!(image(resolved.self_binding.as_ref().unwrap()), binding);
            assert_eq!(resolver.generic_types, registry);
            assert_eq!(
                (
                    resolver.types.type_count(),
                    resolver.types.substitution_count()
                ),
                after
            );
            assert!(
                resolver
                    .source_context(owner, false)
                    .unwrap()
                    .self_binding
                    .is_none()
            );
            let work = WORK.get();
            assert_eq!(
                (work.lookups, work.hits, work.builds, work.rows),
                (3, 2, 1, 1)
            );
        });
    }
    for text in [
        "fn plain[T](value:T)->T=value\n",
        "type Alias[T]=T\n",
        "enum Choice[T] { Empty }\n",
    ] {
        WORK.set(Work::default());
        with_resolver(&[text], |resolver, _| {
            let owner = resolver
                .collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .next()
                .unwrap()
                .id();
            for allow_self in [true, false, true] {
                assert!(
                    resolver
                        .source_context(owner, allow_self)
                        .unwrap()
                        .self_binding
                        .is_none()
                );
            }
            assert!(resolver.static_self_bindings.is_empty());
            assert_eq!(WORK.get(), Work::default());
        });
    }
}

#[test]
fn static_self_preparation_tracks_owner_width_not_width_times_methods() {
    for count in [8_usize, 32, 128] {
        let parameters = (0..count)
            .map(|i| format!("T{i:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let methods = (0..count)
            .map(|i| format!("fn method_{i:03}[U](self:&Self,value:U)->U=value\n"))
            .collect::<String>();
        for (kind, is_trait) in [("struct Host", false), ("trait Marker", true)] {
            let text = format!("{kind}[{parameters}] {{ {methods} }}\n");
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
                assert_eq!(resolver.static_self_bindings.len(), 1);
                assert_eq!(
                    WORK.get(),
                    Work {
                        lookups: count,
                        hits: count - 1,
                        builds: 1,
                        queries: 1,
                        arguments: count,
                        prepared: count,
                        peak_arguments: count,
                        rows: 1,
                        peak_slots: 1,
                        nominal: usize::from(!is_trait),
                        trait_substitution: usize::from(is_trait),
                        self_type: usize::from(is_trait),
                    }
                );
            });
        }
    }
}

struct Support;

impl CompileSupport for Support {
    fn supports(&self, _: CompileCapability) -> bool {
        true
    }
}

fn with_modules(texts: &[(&str, &str)], inspect: impl FnOnce(&mut SignatureResolver<'_, '_>)) {
    crate::declaration::tests::with_sources(texts, |sources| {
        let PhaseProduct::Performed(collection) = crate::collect_declarations(sources, &Support)
            .unwrap()
            .into_parts()
            .0
        else {
            panic!("collection is supported")
        };
        assert_eq!(collection.modules().len(), texts.len());
        let external = ExternalNameEnvironment::new([], [], []).unwrap();
        let mut diagnostics = DiagnosticStore::new();
        let PhaseProduct::Performed(plan) = crate::plan_imports(
            &collection,
            sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0
        else {
            panic!("planning is supported")
        };
        let unit = crate::ImportAnalysisUnit::new(sources.iter().map(|s| s.file())).unwrap();
        let PhaseProduct::Performed(imports) = crate::resolve_imports(
            &collection,
            &plan,
            &[unit],
            sources,
            &Support,
            Some(&external),
            &mut diagnostics,
        )
        .unwrap()
        .into_parts()
        .0
        else {
            panic!("imports are supported")
        };
        let mut resolver = SignatureResolver::new(
            &collection,
            &imports,
            sources.iter().map(|s| (s.file(), s)).collect(),
            None,
            Some(&external),
            None,
            &mut diagnostics,
        )
        .unwrap();
        inspect(&mut resolver);
    });
}

#[test]
fn static_self_keys_include_module_and_owner_identity() {
    let source = "struct Host[T] { fn method[U](self:&Self,value:U)->U=value }\ntrait Marker[T] { fn method[U](self:&Self,value:U)->U=value }\n";
    WORK.set(Work::default());
    with_modules(&[("first", source), ("second", source)], |resolver| {
        let owners = resolver
            .collection
            .modules()
            .flat_map(|m| m.declarations())
            .filter(|d| d.kind() == DeclarationKind::Function)
            .map(|d| d.id())
            .collect::<Vec<_>>();
        assert_eq!(owners.len(), 4);
        let mut images = Vec::new();
        for &owner in &owners {
            images.push(image(
                resolver
                    .source_context(owner, true)
                    .unwrap()
                    .self_binding
                    .as_ref()
                    .unwrap(),
            ));
        }
        assert_eq!(images.iter().collect::<BTreeSet<_>>().len(), owners.len());
        let counts = (
            resolver.types.type_count(),
            resolver.types.substitution_count(),
        );
        for (&owner, expected) in owners.iter().zip(&images) {
            assert_eq!(
                &image(
                    resolver
                        .source_context(owner, true)
                        .unwrap()
                        .self_binding
                        .as_ref()
                        .unwrap()
                ),
                expected
            );
        }
        assert_eq!(
            (
                resolver.types.type_count(),
                resolver.types.substitution_count()
            ),
            counts
        );
        assert_eq!(resolver.static_self_bindings.len(), owners.len());
        assert_eq!(
            WORK.get(),
            Work {
                lookups: 8,
                hits: 4,
                builds: 4,
                queries: 4,
                arguments: 4,
                prepared: 4,
                peak_arguments: 1,
                rows: 4,
                peak_slots: 4,
                nominal: 2,
                trait_substitution: 2,
                self_type: 2,
            }
        );
    });
}

#[test]
fn impl_self_stays_dynamic_and_failed_static_creation_is_not_published() {
    with_resolver(
        &[
            "trait Link { type Item\n fn pick(self:&Self,value:Self::Item)->Self::Item }\nstruct Host[T] {}\nimpl[T] Link for Host[T] { type Item=T\n fn pick(self:&Self,value:Self::Item)->Self::Item=value }\n",
        ],
        |resolver, _| {
            let owner = resolver
                .collection
                .modules()
                .next()
                .unwrap()
                .declarations()
                .find(|d| d.kind() == DeclarationKind::AssociatedTypeBinding)
                .unwrap()
                .id();
            assert!(
                resolver
                    .source_context(owner, true)
                    .unwrap()
                    .self_binding
                    .is_none()
            );
            assert!(resolver.static_self_bindings.is_empty());
            resolver.resolve_source_generics().unwrap();
            resolver.prime_source_impl_headers().unwrap();
            let expected = image(
                resolver
                    .source_context(owner, true)
                    .unwrap()
                    .self_binding
                    .as_ref()
                    .unwrap(),
            );
            for allow_self in [false, true, true] {
                let context = resolver.source_context(owner, allow_self).unwrap();
                assert_eq!(
                    context.self_binding.as_ref().map(image),
                    allow_self.then(|| expected.clone())
                );
            }
            assert!(resolver.static_self_bindings.is_empty());
        },
    );
    WORK.set(Work::default());
    with_resolver(&["struct Host {}\n"], |resolver, _| {
        let owner = resolver
            .collection
            .modules()
            .next()
            .unwrap()
            .declarations()
            .next()
            .unwrap()
            .id();
        let head = NominalHead::Declaration(CanonicalTarget::Source(owner));
        resolver.types = CanonicalTypeStore::new().unwrap();
        for _ in 0..2 {
            assert_eq!(
                resolver.source_context(owner, true).err().unwrap(),
                SignatureResolutionError::TypeStore(
                    crate::TypeStoreError::UnregisteredNominalHead(head.clone())
                )
            );
            assert!(resolver.static_self_bindings.is_empty());
        }
        resolver.types.register_nominal_head(head, 0).unwrap();
        let binding = image(
            resolver
                .source_context(owner, true)
                .unwrap()
                .self_binding
                .as_ref()
                .unwrap(),
        );
        assert_eq!(
            image(
                resolver
                    .source_context(owner, true)
                    .unwrap()
                    .self_binding
                    .as_ref()
                    .unwrap()
            ),
            binding
        );
        assert_eq!(resolver.static_self_bindings.len(), 1);
        assert_eq!(
            WORK.get(),
            Work {
                lookups: 4,
                hits: 1,
                builds: 3,
                queries: 3,
                rows: 1,
                peak_slots: 1,
                nominal: 3,
                ..Work::default()
            }
        );
    });
}
