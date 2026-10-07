use std::cell::Cell;

use super::super::tests::{with_modules, with_resolver};
use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    lookups: usize,
    hits: usize,
    queries: usize,
    builds: usize,
    reads: usize,
    neutral: usize,
    poisoned: usize,
    unperformed: usize,
    merges: usize,
    rows: usize,
    peak_slots: usize,
}

thread_local! {
    static WORK: Cell<Work> = Cell::new(Work::default());
}

pub(super) enum Step {
    Lookup,
    Hit,
    Query,
    Build,
    Read,
    Neutral,
    Poisoned,
    Unperformed,
    Merge,
    Row(usize),
}

pub(super) fn observe(step: Step) {
    WORK.update(|mut work| {
        match step {
            Step::Lookup => work.lookups += 1,
            Step::Hit => work.hits += 1,
            Step::Query => work.queries += 1,
            Step::Build => work.builds += 1,
            Step::Read => work.reads += 1,
            Step::Neutral => work.neutral += 1,
            Step::Poisoned => work.poisoned += 1,
            Step::Unperformed => work.unperformed += 1,
            Step::Merge => work.merges += 1,
            Step::Row(slots) => {
                work.rows += 1;
                work.peak_slots = work.peak_slots.max(slots);
            }
        }
        work
    });
}

fn owners(resolver: &SignatureResolver<'_, '_>, kind: DeclarationKind) -> Vec<DefId> {
    resolver
        .collection
        .modules()
        .flat_map(|module| module.declarations())
        .filter(|declaration| declaration.kind() == kind)
        .map(Declaration::id)
        .collect()
}

fn flat_state(
    resolver: &SignatureResolver<'_, '_>,
    owner: DefId,
) -> Result<Resolution<()>, SignatureResolutionError> {
    let mut cursor = Some(owner);
    let mut poison = None;
    let mut unperformed = false;
    while let Some(current) = cursor {
        for generic in resolver.child_declarations(current, DeclarationKind::GenericParameter) {
            match resolver
                .generic_parameters
                .state(&GenericParameterId::Source(generic))
            {
                FactState::Poisoned(found) => merge_poison(&mut poison, found)?,
                FactState::Unperformed => unperformed = true,
                FactState::Known(_) | FactState::NotApplicable => {}
            }
        }
        cursor = resolver
            .collection
            .declaration(current)
            .and_then(Declaration::parent);
    }
    Ok(if let Some(poison) = poison {
        Resolution::Poisoned(poison)
    } else if unperformed {
        Resolution::Unperformed
    } else {
        Resolution::Known(())
    })
}

fn assert_same_state(actual: Resolution<()>, expected: Resolution<()>) {
    match (actual, expected) {
        (Resolution::Known(()), Resolution::Known(()))
        | (Resolution::Unperformed, Resolution::Unperformed) => {}
        (Resolution::Poisoned(actual), Resolution::Poisoned(expected)) => {
            let image = format!("{expected:?}");
            assert_eq!(format!("{actual:?}"), image);
            // Equal finite cardinalities and an unchanged union prove identical root sets.
            assert_eq!(format!("{:?}", actual.merge(&expected).unwrap()), image);
        }
        _ => panic!("generic context state changed"),
    }
}

#[test]
fn scope_results_stay_live_until_all_parameter_attempts_complete() {
    for (text, count) in [
        ("struct Host[A] { fn method[B](self:&Self)->Unit {} }\n", 2),
        ("trait Host[A] { fn method[B](self:&Self)->Unit }\n", 2),
        (
            "struct Host[Box] { fn method[B](self:&Self)->Unit {} }\n",
            2,
        ),
        ("struct Host[A] { fn method[A](self:&Self)->Unit {} }\n", 2),
        (
            "struct Host[Box,A] { fn method[B](self:&Self)->Unit {} }\n",
            3,
        ),
        ("trait Host[Box,A] { fn method[B](self:&Self)->Unit }\n", 3),
    ] {
        with_resolver(&[text], |resolver, _| {
            let owner = owners(resolver, DeclarationKind::Function)[0];
            let parameters = owners(resolver, DeclarationKind::GenericParameter);
            assert_eq!(parameters.len(), count);
            WORK.set(Work::default());
            assert!(matches!(
                resolver.generic_context_state(owner).unwrap(),
                Resolution::Unperformed
            ));
            assert_eq!(WORK.get().unperformed, count);
            assert_eq!(WORK.get().rows, 0);
            assert!(resolver.frozen_source_generic_scopes.is_none());

            resolver.resolve_source_generic(parameters[0]).unwrap();
            assert_same_state(
                resolver.generic_context_state(owner).unwrap(),
                flat_state(resolver, owner).unwrap(),
            );
            assert!(resolver.frozen_source_generic_scopes.is_none());
            assert_eq!(WORK.get().rows, 0);

            resolver.resolve_source_generics().unwrap();
            assert!(
                resolver
                    .frozen_source_generic_scopes
                    .as_ref()
                    .unwrap()
                    .is_empty()
            );
            assert_same_state(
                resolver.generic_context_state(owner).unwrap(),
                flat_state(resolver, owner).unwrap(),
            );
            assert_eq!(
                resolver
                    .frozen_source_generic_scopes
                    .as_ref()
                    .unwrap()
                    .len(),
                2
            );
            let before = WORK.get();
            resolver.resolve_source_generics().unwrap();
            assert_same_state(
                resolver.generic_context_state(owner).unwrap(),
                flat_state(resolver, owner).unwrap(),
            );
            assert_eq!(WORK.get().reads, before.reads);
            assert_eq!(WORK.get().rows, before.rows);
            assert_eq!(WORK.get().hits - before.hits, 2);
        });
    }
}

#[test]
fn scope_preparation_tracks_parameters_not_width_times_methods() {
    for count in [8_usize, 32, 128] {
        let parameters = (0..count)
            .map(|index| format!("T{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        for is_trait in [false, true] {
            for poisoned in [false, true] {
                let methods = (0..count)
                    .map(|index| {
                        let parameter = if poisoned { "Box" } else { "U" };
                        let body = if is_trait { "" } else { " {}" };
                        format!("fn method_{index:03}[{parameter}](self:&Self)->Unit{body}\n")
                    })
                    .collect::<String>();
                let kind = if is_trait { "trait" } else { "struct" };
                let text = format!("{kind} Host[{parameters}] {{ {methods} }}\n");
                with_resolver(&[&text], |resolver, _| {
                    resolver.resolve_trusted().unwrap();
                    resolver.validate_language_items().unwrap();
                    resolver.resolve_source_generics().unwrap();
                    WORK.set(Work::default());
                    resolver.prime_source_impl_headers().unwrap();
                    resolver.validate_source_impl_headers().unwrap();
                    resolver.resolve_source_declarations().unwrap();
                    resolver.validate_source_overload_families().unwrap();
                    resolver.resolve_body_type_occurrences().unwrap();
                    resolver.index_enum_variant_names().unwrap();
                    assert_eq!(
                        WORK.get(),
                        Work {
                            lookups: 2 * count + 1,
                            hits: count,
                            queries: count + 1,
                            builds: count + 1,
                            reads: 2 * count,
                            neutral: if poisoned { count } else { 2 * count },
                            poisoned: if poisoned { count } else { 0 },
                            merges: if poisoned { count } else { 0 },
                            rows: count + 1,
                            peak_slots: count + 1,
                            unperformed: 0,
                        }
                    );
                    assert_eq!(
                        resolver
                            .frozen_source_generic_scopes
                            .as_ref()
                            .unwrap()
                            .len(),
                        count + 1
                    );
                    for owner in owners(resolver, DeclarationKind::Function) {
                        let expected = flat_state(resolver, owner).unwrap();
                        let actual = resolver.generic_context_state(owner).unwrap();
                        if poisoned {
                            let Resolution::Poisoned(poison) = &actual else {
                                panic!("method lost its generic poison")
                            };
                            assert!(format!("{poison:?}").contains("root_count: 1"));
                        }
                        assert_same_state(actual, expected);
                    }
                    assert_eq!(WORK.get().reads, 2 * count);
                });
            }
        }
    }
}

#[test]
fn empty_scopes_are_not_retained_and_full_owner_ids_separate_modules() {
    let text = "struct Host[A] { fn method[B](self:&Self)->Unit {} fn plain(self:&Self)->Unit {} }\nfn empty()->Unit {}\n";
    with_modules(&[("first", text), ("second", text)], |resolver| {
        resolver.resolve_source_generics().unwrap();
        let functions = owners(resolver, DeclarationKind::Function);
        assert_eq!(functions.len(), 6);
        WORK.set(Work::default());
        for _ in 0..2 {
            for &owner in &functions {
                assert_same_state(
                    resolver.generic_context_state(owner).unwrap(),
                    flat_state(resolver, owner).unwrap(),
                );
            }
        }
        let summaries = resolver.frozen_source_generic_scopes.as_ref().unwrap();
        assert_eq!(summaries.len(), 4);
        assert_eq!(WORK.get().reads, 4);
        assert_eq!(WORK.get().rows, 4);
        assert_eq!(WORK.get().peak_slots, 4);
        for module in resolver.collection.modules() {
            let retained = summaries
                .keys()
                .filter(|owner| owner.module() == module.id())
                .count();
            assert_eq!(retained, 2);
            for declaration in module.declarations() {
                assert_eq!(
                    summaries.contains_key(&declaration.id()),
                    resolver
                        .child_declarations(declaration.id(), DeclarationKind::GenericParameter)
                        .len()
                        != 0
                );
            }
        }
    });
}

#[test]
fn summaries_preserve_root_sets_and_later_declaration_state_stays_live() {
    with_resolver(
        &["struct Host[Box,Own] { fn method[Opt,Res](self:&Self)->Unit {} }\n"],
        |resolver, _| {
            let method = owners(resolver, DeclarationKind::Function)[0];
            resolver.resolve_source_generics().unwrap();
            let expected = flat_state(resolver, method).unwrap();
            let Resolution::Poisoned(poison) = &expected else {
                panic!("reserved names must retain roots")
            };
            assert!(format!("{poison:?}").contains("root_count: 4"));
            for _ in 0..3 {
                assert_same_state(
                    resolver.generic_context_state(method).unwrap(),
                    expected.clone(),
                );
            }
            assert!(matches!(
                resolver.declaration_context_state(method).unwrap(),
                Resolution::Unperformed
            ));
            resolver.resolve_source_declarations().unwrap();
            let Resolution::Poisoned(parent) = resolver.declaration_context_state(method).unwrap()
            else {
                panic!("parent declaration must now carry its own generic roots")
            };
            assert!(format!("{parent:?}").contains("root_count: 2"));
            assert_same_state(resolver.generic_context_state(method).unwrap(), expected);
        },
    );
}

#[test]
fn equal_width_poison_summaries_keep_each_owners_exact_roots() {
    with_resolver(
        &["fn first[Box]()->Unit {}\nfn second[Own]()->Unit {}\n"],
        |resolver, _| {
            let functions = owners(resolver, DeclarationKind::Function);
            assert_eq!(functions.len(), 2);
            resolver.resolve_source_generics().unwrap();
            for _ in 0..2 {
                for &owner in &functions {
                    let expected = flat_state(resolver, owner).unwrap();
                    let Resolution::Poisoned(poison) = &expected else {
                        panic!("reserved generic must be poisoned")
                    };
                    assert!(format!("{poison:?}").contains("root_count: 1"));
                    assert_same_state(resolver.generic_context_state(owner).unwrap(), expected);
                }
            }
        },
    );
}

#[test]
fn failed_pass_does_not_seal_and_terminal_absence_remains_distinct() {
    with_resolver(&["fn header[A,B]()->Unit {}\n"], |resolver, sources| {
        let owner = owners(resolver, DeclarationKind::Function)[0];
        resolver.sources.clear();
        assert_eq!(
            resolver.resolve_source_generics().unwrap_err(),
            SignatureResolutionError::MissingHir(sources[0].file())
        );
        assert!(resolver.frozen_source_generic_scopes.is_none());
        assert!(matches!(
            resolver.generic_context_state(owner).unwrap(),
            Resolution::Unperformed
        ));
        resolver.sources.insert(sources[0].file(), &sources[0]);
        let parameters = owners(resolver, DeclarationKind::GenericParameter);
        resolver
            .source_generic_states
            .insert(parameters[0], SourceGenericState::Unperformed);
        resolver
            .source_generic_states
            .insert(parameters[1], SourceGenericState::Resolving);
        resolver.resolve_source_generics().unwrap();
        WORK.set(Work::default());
        for _ in 0..2 {
            assert!(matches!(
                resolver.generic_context_state(owner).unwrap(),
                Resolution::Unperformed
            ));
        }
        assert_eq!(WORK.get().reads, 2);
        assert_eq!(WORK.get().unperformed, 2);
        assert_eq!(WORK.get().hits, 1);
        assert_eq!(WORK.get().rows, 1);
        for parameter in parameters {
            assert!(matches!(
                resolver.resolve_source_generic(parameter).unwrap(),
                Resolution::Unperformed
            ));
            assert!(matches!(
                resolver
                    .generic_parameters
                    .state(&GenericParameterId::Source(parameter)),
                FactState::Unperformed
            ));
        }
        assert!(matches!(
            resolver.generic_context_state(owner).unwrap(),
            Resolution::Unperformed
        ));
        assert_eq!(WORK.get().reads, 2);
    });
}

#[test]
fn foreign_poison_errors_do_not_publish_failed_scope_summaries() {
    let mut poisons = Vec::new();
    for _ in 0..2 {
        with_resolver(&["fn root[Box]()->Unit {}\n"], |resolver, _| {
            let owner = owners(resolver, DeclarationKind::Function)[0];
            resolver.resolve_source_generics().unwrap();
            let Resolution::Poisoned(poison) = resolver.generic_context_state(owner).unwrap()
            else {
                panic!("reserved generic must be poisoned")
            };
            poisons.push(poison);
        });
    }
    with_resolver(
        &["fn header[A,B]()->Unit {}\nfn empty()->Unit {}\n"],
        |resolver, _| {
            let owner = owners(resolver, DeclarationKind::Function)[0];
            let parameters = owners(resolver, DeclarationKind::GenericParameter);
            for (&parameter, poison) in parameters.iter().zip(&poisons) {
                resolver
                    .generic_parameters
                    .insert(
                        GenericParameterId::Source(parameter),
                        SemanticFact::Poisoned(poison.clone()),
                    )
                    .unwrap();
                resolver
                    .source_generic_states
                    .insert(parameter, SourceGenericState::Poisoned(poison.clone()));
            }
            resolver.resolve_source_generics().unwrap();
            WORK.set(Work::default());
            for _ in 0..2 {
                assert!(matches!(
                    flat_state(resolver, owner),
                    Err(SignatureResolutionError::DiagnosticStore(
                        DiagnosticStoreError::ForeignPoison
                    ))
                ));
                assert!(matches!(
                    resolver.generic_context_state(owner),
                    Err(SignatureResolutionError::DiagnosticStore(
                        DiagnosticStoreError::ForeignPoison
                    ))
                ));
                assert!(
                    resolver
                        .frozen_source_generic_scopes
                        .as_ref()
                        .unwrap()
                        .is_empty()
                );
            }
            assert_eq!(WORK.get().builds, 2);
            assert_eq!(WORK.get().reads, 4);
            assert_eq!(WORK.get().poisoned, 4);
            assert_eq!(WORK.get().merges, 4);
            assert_eq!(WORK.get().hits, 0);
            assert_eq!(WORK.get().rows, 0);
            let empty = owners(resolver, DeclarationKind::Function)[1];
            assert!(matches!(
                resolver.generic_context_state(empty).unwrap(),
                Resolution::Known(())
            ));
            assert!(
                resolver
                    .frozen_source_generic_scopes
                    .as_ref()
                    .unwrap()
                    .is_empty()
            );
        },
    );

    with_resolver(
        &["struct Host[A] { fn method[B](self:&Self)->Unit {} }\n"],
        |resolver, _| {
            let method = owners(resolver, DeclarationKind::Function)[0];
            for (parameter, poison) in owners(resolver, DeclarationKind::GenericParameter)
                .into_iter()
                .zip(&poisons)
            {
                resolver
                    .generic_parameters
                    .insert(
                        GenericParameterId::Source(parameter),
                        SemanticFact::Poisoned(poison.clone()),
                    )
                    .unwrap();
                resolver
                    .source_generic_states
                    .insert(parameter, SourceGenericState::Poisoned(poison.clone()));
            }
            resolver.resolve_source_generics().unwrap();
            WORK.set(Work::default());
            for _ in 0..2 {
                assert!(matches!(
                    flat_state(resolver, method),
                    Err(SignatureResolutionError::DiagnosticStore(
                        DiagnosticStoreError::ForeignPoison
                    ))
                ));
                assert!(matches!(
                    resolver.generic_context_state(method),
                    Err(SignatureResolutionError::DiagnosticStore(
                        DiagnosticStoreError::ForeignPoison
                    ))
                ));
            }
            assert_eq!(WORK.get().reads, 2);
            assert_eq!(WORK.get().rows, 2);
            assert_eq!(WORK.get().hits, 2);
            assert_eq!(
                resolver
                    .frozen_source_generic_scopes
                    .as_ref()
                    .unwrap()
                    .len(),
                2
            );
        },
    );
}
