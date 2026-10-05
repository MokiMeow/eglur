use std::cell::Cell;
use std::collections::BTreeSet;

use super::*;
use crate::fingerprint::identity::tests::generic_ordinals::{headers, owner};
use crate::{CanonicalTarget, DeclarationKind, ExternalDefId, FactState, ModuleId};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Work {
    queries: usize,
    owners: usize,
    parameters: usize,
    lookups: usize,
    frames: usize,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work {
        queries: 0, owners: 0, parameters: 0, lookups: 0, frames: 0,
    }) };
}

pub(super) fn observe_query() {
    let mut work = WORK.get();
    work.queries += 1;
    WORK.set(work);
}

pub(super) fn observe_owner() {
    let mut work = WORK.get();
    work.owners += 1;
    WORK.set(work);
}

pub(super) fn observe_parameter() {
    let mut work = WORK.get();
    work.parameters += 1;
    WORK.set(work);
}

pub(super) fn observe_lookup() {
    let mut work = WORK.get();
    work.lookups += 1;
    WORK.set(work);
}

pub(super) fn observe_frame() {
    let mut work = WORK.get();
    work.frames += 1;
    WORK.set(work);
}

fn legacy_positions(
    collection: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    declaration: DefId,
) -> Result<BTreeMap<GenericParameterId, u32>, StableDeclarationError> {
    let mut owners = Vec::new();
    let mut cursor = Some(declaration);
    while let Some(owner) = cursor {
        let declaration = collection
            .declaration(owner)
            .ok_or(StableDeclarationError::UnknownDeclaration(owner))?;
        owners.push(owner);
        cursor = declaration.parent();
    }
    owners.reverse();

    let count = owners.iter().try_fold(0_usize, |count, &owner| {
        direct_generic_parameters(signatures, owner).map(|parameters| count + parameters.len())
    })?;
    let mut positions = BTreeMap::new();
    for owner in owners {
        for parameter in direct_generic_parameters(signatures, owner)? {
            let index = u32::try_from(positions.len())
                .map_err(|_| StableDeclarationError::GenericParameterOverflow(count))?;
            positions.insert(parameter.clone(), index);
        }
    }
    Ok(positions)
}

fn assert_positions(context: &BinderContext, expected: &BTreeMap<GenericParameterId, u32>) {
    assert_eq!(context.distinct_count, expected.len());
    for (parameter, &index) in expected {
        assert_eq!(context.ordinal(parameter), Some(index));
    }
}

fn retained_bindings(contexts: &DeclarationBinderContexts<'_>) -> (usize, usize) {
    let mut seen = BTreeSet::new();
    let mut rows = 0;
    for context in contexts.completed.values() {
        let mut cursor = Some(context);
        while let Some(context) = cursor {
            if !seen.insert(Rc::as_ptr(context)) {
                break;
            }
            rows += context.local.len();
            cursor = context.parent.as_ref();
        }
    }
    (seen.len(), rows)
}

#[test]
fn generic_children_share_one_completed_owner_context() {
    for public in [false, true] {
        for count in [8_usize, 32, 128] {
            let visibility = if public { "pub " } else { "" };
            let parameters = (0..count)
                .map(|index| format!("T{index:03}"))
                .collect::<Vec<_>>()
                .join(", ");
            let source =
                format!("{visibility}fn wide[{parameters}](value: Bool) -> Bool = value\n");
            let (collection, signatures, diagnostics) = headers(&source);
            assert!(diagnostics.diagnostics().is_empty());
            let root = owner(&collection, "wide");
            let parameters = direct_generic_parameters(&signatures, root).unwrap();
            let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);
            WORK.set(Work::default());
            let context = contexts.context(root).unwrap();
            assert_eq!(context.distinct_count, count);
            for reverse in [false, true, false] {
                for index in 0..count {
                    let index = if reverse { count - index - 1 } else { index };
                    let GenericParameterId::Source(parameter) = parameters[index] else {
                        panic!("native source parameter");
                    };
                    let inherited = contexts.context(parameter).unwrap();
                    assert!(Rc::ptr_eq(&context, &inherited));
                    assert_eq!(inherited.ordinal(&parameters[index]), Some(index as u32));
                }
            }
            assert!(Rc::ptr_eq(&context, &contexts.context(root).unwrap()));
            assert_eq!(
                WORK.get(),
                Work {
                    queries: 3 * count + 2,
                    owners: count + 1,
                    parameters: count,
                    lookups: 4 * count,
                    frames: 4 * count,
                }
            );
            assert_eq!(contexts.completed.len(), count + 1);
            assert_eq!(retained_bindings(&contexts), (2, count));
        }
    }
}

#[test]
fn many_generic_methods_share_the_parent_without_retaining_inherited_rows() {
    for count in [8_usize, 32, 128] {
        let parameters = (0..count)
            .map(|index| format!("T{index:03}"))
            .collect::<Vec<_>>()
            .join(", ");
        let methods = (0..count)
            .map(|index| {
                format!(
                    "  pub fn method_{index:03}[S{index:03}](self: &Self, value: Bool) -> Bool = value\n"
                )
            })
            .collect::<String>();
        let source = format!("pub struct Holder[{parameters}] {{\n{methods}}}\n");
        let (collection, signatures, diagnostics) = headers(&source);
        assert!(diagnostics.diagnostics().is_empty());
        let declarations = collection
            .modules()
            .flat_map(|module| module.declarations())
            .map(|declaration| declaration.id())
            .collect::<Vec<_>>();
        let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);

        WORK.set(Work::default());
        for declaration in declarations
            .iter()
            .rev()
            .chain(&declarations)
            .chain(&declarations)
        {
            contexts.context(*declaration).unwrap();
        }
        assert_eq!(
            WORK.get(),
            Work {
                queries: 3 * declarations.len(),
                owners: declarations.len(),
                parameters: 2 * count,
                lookups: 2 * count,
                frames: 3 * count,
            }
        );
        assert_eq!(contexts.completed.len(), declarations.len());
        assert_eq!(retained_bindings(&contexts), (count + 2, 2 * count));

        let parent = contexts.context(owner(&collection, "Holder")).unwrap();
        for index in 0..count {
            let method = owner(&collection, &format!("method_{index:03}"));
            let context = contexts.context(method).unwrap();
            assert!(Rc::ptr_eq(context.parent.as_ref().unwrap(), &parent));
            assert_eq!(context.local.len(), 1);
            assert_positions(
                &context,
                &legacy_positions(&collection, &signatures, method).unwrap(),
            );
        }
    }
}

#[test]
fn cold_reverse_queries_preserve_all_owner_families() {
    let (collection, signatures, diagnostics) = headers(concat!(
        "pub trait Echo[A] { fn echo[B](self: &Self, value: B) -> B }\n",
        "pub struct Wrap[X, Y] {\n",
        "  pub field value: X\n",
        "  pub fn echo[C](self: &Self, value: C) -> C = value\n",
        "  pub fn inherited(self: &Self, value: X) -> X = value\n",
        "}\n",
        "impl[X, Y] Echo[X] for Wrap[X, Y] {\n",
        "  fn echo[D](self: &Self, value: D) -> D = value\n",
        "}\n",
        "pub enum Either[L, R] { Left(L)\n Right(R) }\n",
        "pub type Pair[P, Q] = (P, Q)\n",
        "pub fn identity[T](value: T) -> T = value\n",
    ));
    assert!(diagnostics.diagnostics().is_empty());
    let declarations = collection
        .modules()
        .flat_map(|module| module.declarations())
        .map(|declaration| declaration.id())
        .collect::<Vec<_>>();
    let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);
    for declaration in declarations.iter().rev().chain(&declarations) {
        assert_positions(
            &contexts.context(*declaration).unwrap(),
            &legacy_positions(&collection, &signatures, *declaration).unwrap(),
        );
    }

    let parent = owner(&collection, "Wrap");
    let parent_context = contexts.context(parent).unwrap();
    let inherited = owner(&collection, "inherited");
    assert!(Rc::ptr_eq(
        &parent_context,
        &contexts.context(inherited).unwrap()
    ));
    let methods = collection
        .modules()
        .flat_map(|module| module.declarations())
        .filter(|declaration| declaration.name().is_some_and(|name| name.text() == "echo"))
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 3);
    let mut ordinals = Vec::new();
    for method in methods {
        let parameters = direct_generic_parameters(&signatures, method.id()).unwrap();
        assert_eq!(parameters.len(), 1);
        let context = contexts.context(method.id()).unwrap();
        let index = context.ordinal(&parameters[0]).unwrap();
        let owner = collection.declaration(method.parent().unwrap()).unwrap();
        assert_eq!(
            index,
            if owner.kind() == DeclarationKind::Trait {
                1
            } else {
                2
            }
        );
        ordinals.push((parameters[0].clone(), index));
    }
    assert!(ordinals.iter().enumerate().all(|(index, (parameter, _))| {
        ordinals
            .iter()
            .skip(index + 1)
            .all(|(other, _)| parameter != other)
    }));
}

#[test]
fn source_trusted_duplicates_keep_last_insert_and_inherited_positions() {
    let module = ModuleId::from_index(0).unwrap();
    let first = GenericParameterId::Source(DefId::from_index(module, 1).unwrap());
    let second = GenericParameterId::Source(DefId::from_index(module, 2).unwrap());
    let trusted = GenericParameterId::Trusted {
        owner: ExternalDefId::new("probe::Owner").unwrap(),
        index: 7,
    };
    let empty = Rc::new(BinderContext::empty());
    let parameters = [
        trusted.clone(),
        first.clone(),
        second.clone(),
        first.clone(),
    ];
    let context = empty.extend(&parameters, 6).unwrap();
    assert_positions(
        &context,
        &BTreeMap::from([
            (trusted.clone(), 0),
            (first.clone(), 3),
            (second.clone(), 2),
        ]),
    );
    assert_eq!(context.parameter_count, 4);
    let inherited = context.extend(&[], 6).unwrap();
    assert!(Rc::ptr_eq(&context, &inherited));
    let next = context
        .extend(&[first.clone(), trusted.clone()], 6)
        .unwrap();
    assert_positions(
        &next,
        &BTreeMap::from([
            (trusted.clone(), 3),
            (first.clone(), 3),
            (second.clone(), 2),
        ]),
    );
    assert_eq!(next.parameter_count, 6);
    assert_eq!(next.local.len(), 2);
    assert_eq!(context.ordinal(&trusted), Some(0));
    assert_eq!(empty.ordinal(&first), None);
    let other = GenericParameterId::Trusted {
        owner: ExternalDefId::new("probe::Other").unwrap(),
        index: 7,
    };
    assert_eq!(next.ordinal(&other), None);
}

#[test]
fn incomplete_requests_preserve_errors_without_publishing_prefixes() {
    let (collection, signatures, diagnostics) = headers(concat!(
        "fn broken[A](value: Missing) -> Bool = false\n",
        "fn healthy[B](value: Bool) -> Bool = value\n",
    ));
    assert!(!diagnostics.diagnostics().is_empty());
    let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);
    let broken = owner(&collection, "broken");
    let parameter = owner(&collection, "A");
    for id in [broken, parameter, broken] {
        assert_eq!(
            contexts.context(id).unwrap_err(),
            legacy_positions(&collection, &signatures, id).unwrap_err()
        );
        assert!(contexts.completed.is_empty());
    }
    let healthy = owner(&collection, "healthy");
    let context = contexts.context(healthy).unwrap();
    assert_eq!(context.distinct_count, 1);
    assert_eq!(contexts.completed.len(), 1);
    let unknown = DefId::from_index(healthy.module(), 10_000).unwrap();
    assert_eq!(
        contexts.context(unknown).unwrap_err(),
        StableDeclarationError::UnknownDeclaration(unknown)
    );
    assert_eq!(contexts.completed.len(), 1);
    assert!(Rc::ptr_eq(&context, &contexts.context(healthy).unwrap()));
}

#[test]
fn failed_child_preflight_does_not_construct_or_publish_its_healthy_parent() {
    let (collection, signatures, diagnostics) = headers(concat!(
        "pub struct Holder[A] {\n",
        "  pub fn broken[B](self: &Self, value: Missing) -> Bool = false\n",
        "  pub fn healthy[C](self: &Self, value: C) -> C = value\n",
        "}\n",
    ));
    assert!(!diagnostics.diagnostics().is_empty());
    let parent = owner(&collection, "Holder");
    assert!(matches!(
        signatures.source_declaration(parent),
        FactState::Known(_)
    ));
    let broken = owner(&collection, "broken");
    let parameter = owner(&collection, "B");
    let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);

    WORK.set(Work::default());
    for declaration in [broken, parameter] {
        assert_eq!(
            contexts.context(declaration).unwrap_err(),
            legacy_positions(&collection, &signatures, declaration).unwrap_err()
        );
        assert!(contexts.completed.is_empty());
        assert_eq!(WORK.get().parameters, 0);
    }

    let context = contexts.context(parent).unwrap();
    for declaration in [parameter, broken] {
        assert_eq!(
            contexts.context(declaration).unwrap_err(),
            StableDeclarationError::IncompleteSignature(broken)
        );
        assert_eq!(contexts.completed.len(), 1);
    }
    assert!(Rc::ptr_eq(&context, &contexts.context(parent).unwrap()));
    let healthy = contexts.context(owner(&collection, "healthy")).unwrap();
    assert!(Rc::ptr_eq(healthy.parent.as_ref().unwrap(), &context));
    assert_eq!(healthy.distinct_count, 2);
}

#[test]
fn known_impl_header_retains_authority_after_binding_failure() {
    let (collection, signatures, diagnostics) = headers(concat!(
        "pub trait Echo[A] { type Item }\n",
        "pub struct Wrap[X] {}\n",
        "impl[X] Echo[X] for Wrap[X] {}\n",
    ));
    assert!(!diagnostics.diagnostics().is_empty());
    let implementation = collection
        .modules()
        .flat_map(|module| module.declarations())
        .find(|declaration| declaration.kind() == DeclarationKind::Impl)
        .unwrap()
        .id();
    assert!(matches!(
        signatures.source_declaration(implementation),
        FactState::Poisoned(_)
    ));
    let FactState::Known(header) = signatures.impl_header(&CanonicalTarget::Source(implementation))
    else {
        panic!("retained known header");
    };
    let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);
    let context = contexts.context(implementation).unwrap();
    assert_eq!(context.distinct_count, 1);
    assert_eq!(context.ordinal(&header.generic_parameters()[0]), Some(0));
}

#[test]
fn different_snapshots_keep_their_own_parameter_order() {
    let mut ordinals = Vec::new();
    for parameters in ["A, B", "B, A"] {
        let source = format!("pub fn order[{parameters}](value: Bool) -> Bool = value\n");
        let (collection, signatures, diagnostics) = headers(&source);
        assert!(diagnostics.diagnostics().is_empty());
        let mut contexts = DeclarationBinderContexts::new(&collection, &signatures);
        let context = contexts.context(owner(&collection, "order")).unwrap();
        ordinals.push(
            context
                .ordinal(&GenericParameterId::Source(owner(&collection, "A")))
                .unwrap(),
        );
    }
    assert_eq!(ordinals, [0, 1]);
}

#[test]
fn overflow_reports_the_complete_request_parameter_count_before_duplicate_checks() {
    assert_eq!(binder_index(u32::MAX as usize, 123), Ok(u32::MAX));
    if usize::BITS > u32::BITS {
        // Numeric boundary probe, not a source with billions of parameters.
        let parameter = GenericParameterId::Trusted {
            owner: ExternalDefId::new("probe::Owner").unwrap(),
            index: 7,
        };
        let context = Rc::new(BinderContext {
            parent: None,
            local: BTreeMap::from([(parameter.clone(), 0)]),
            distinct_count: u32::MAX as usize + 1,
            parameter_count: 123,
        });
        assert_eq!(
            context.extend(&[parameter], 456).unwrap_err(),
            StableDeclarationError::GenericParameterOverflow(456)
        );
        assert!(Rc::ptr_eq(&context, &context.extend(&[], 456).unwrap()));
    }
}
