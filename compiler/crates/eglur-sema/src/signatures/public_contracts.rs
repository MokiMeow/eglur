use crate::{
    CanonicalTarget, CanonicalTypeStore, DeclarationCollection, NominalHead, TraitRef, TypeId,
    TypeKind, TypeStoreError,
};

enum Component {
    Type(TypeId),
    Trait(TraitRef),
}

pub(crate) fn first_unnameable_type(
    collection: &DeclarationCollection,
    types: &CanonicalTypeStore,
    root: TypeId,
) -> Result<Option<CanonicalTarget>, TypeStoreError> {
    first_unnameable(collection, types, Component::Type(root))
}

pub(crate) fn first_unnameable_trait(
    collection: &DeclarationCollection,
    types: &CanonicalTypeStore,
    root: TraitRef,
) -> Result<Option<CanonicalTarget>, TypeStoreError> {
    first_unnameable(collection, types, Component::Trait(root))
}

fn first_unnameable(
    collection: &DeclarationCollection,
    types: &CanonicalTypeStore,
    root: Component,
) -> Result<Option<CanonicalTarget>, TypeStoreError> {
    let mut pending = vec![root];
    while let Some(component) = pending.pop() {
        match component {
            Component::Type(ty) => match types.kind(ty)? {
                TypeKind::Never
                | TypeKind::Primitive(_)
                | TypeKind::GenericParameter(_)
                | TypeKind::SelfType(_) => {}
                TypeKind::Nominal { head, substitution } => {
                    if let NominalHead::Declaration(target) = head
                        && !nameable(collection, target)
                    {
                        return Ok(Some(target.clone()));
                    }
                    push_types(&mut pending, types.substitution(*substitution)?);
                }
                TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                    pending.push(Component::Type(*referent));
                }
                TypeKind::Tuple(elements) | TypeKind::ErrorUnion(elements) => {
                    push_types(&mut pending, elements);
                }
                TypeKind::Callable { parameters, result } => {
                    pending.push(Component::Type(*result));
                    push_types(&mut pending, parameters);
                }
                TypeKind::AssociatedProjection(projection) => {
                    pending.push(Component::Trait(projection.trait_ref().clone()));
                    pending.push(Component::Type(projection.base()));
                }
            },
            Component::Trait(trait_ref) => {
                if !nameable(collection, trait_ref.declaration()) {
                    return Ok(Some(trait_ref.declaration().clone()));
                }
                push_types(&mut pending, types.substitution(trait_ref.substitution())?);
            }
        }
    }
    Ok(None)
}

fn push_types(pending: &mut Vec<Component>, types: &[TypeId]) {
    pending.extend(types.iter().rev().copied().map(Component::Type));
}

fn nameable(collection: &DeclarationCollection, target: &CanonicalTarget) -> bool {
    match target {
        CanonicalTarget::Source(id) => collection
            .declaration(*id)
            .is_some_and(|declaration| declaration.visibility().is_public()),
        // File-set trusted catalog declarations are externally accessible. Package bridges
        // have a separate contract and are not decided by this predicate.
        CanonicalTarget::External(_) => true,
    }
}
