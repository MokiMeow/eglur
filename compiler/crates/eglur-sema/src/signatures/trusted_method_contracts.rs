use std::collections::{BTreeMap, BTreeSet};

use crate::ExternalDefId;

use super::trusted::{
    TrustedAssociatedBinding, TrustedCallableRequirement, TrustedDeclarationSignature,
    TrustedSignatureError, TrustedSignatureKind, TrustedTraitExpr, TrustedTypeExpr,
};

pub(super) fn validate_trusted_method_contracts(
    declarations: &[TrustedDeclarationSignature],
    by_id: &BTreeMap<ExternalDefId, usize>,
    method_owners: &BTreeMap<ExternalDefId, ExternalDefId>,
) -> Result<(), TrustedSignatureError> {
    for implementation in declarations {
        let TrustedSignatureKind::Impl {
            trait_ref,
            target,
            associated_bindings,
            ..
        } = implementation.kind()
        else {
            continue;
        };
        let Some(&trait_index) = by_id.get(&trait_ref.declaration) else {
            return Err(TrustedSignatureError::UnknownTypeHead(
                trait_ref.declaration.clone(),
            ));
        };
        let trait_declaration = &declarations[trait_index];
        for method in trait_declaration.trait_methods() {
            let selected = implementation
                .impl_methods()
                .iter()
                .find(|candidate| candidate.trait_method == method.declaration)
                .map(|candidate| &candidate.callable);
            let selected = match selected {
                Some(selected) => selected,
                None if method.has_default => continue,
                None => {
                    return Err(TrustedSignatureError::MissingRequiredMethod {
                        implementation: implementation.id().clone(),
                        method: method.declaration.clone(),
                    });
                }
            };
            let expected = declaration_by_id(declarations, by_id, &method.declaration);
            let actual = declaration_by_id(declarations, by_id, selected);
            if !trusted_callable_contract_matches(
                expected,
                actual,
                &trait_ref.declaration,
                target,
                associated_bindings,
            ) {
                return Err(TrustedSignatureError::MethodContractMismatch {
                    implementation: implementation.id().clone(),
                    method: method.declaration.clone(),
                });
            }
        }
    }
    for adapter in declarations {
        let Some(relation) = adapter.trait_adapter() else {
            continue;
        };
        let method = declaration_by_id(declarations, by_id, &relation.method);
        let owner = method_owners
            .get(&relation.method)
            .expect("adapter methods have a validated owner");
        if !trusted_adapter_contract_matches(adapter, method, owner, relation.argument) {
            return Err(TrustedSignatureError::AdapterContractMismatch(
                adapter.id().clone(),
            ));
        }
    }
    Ok(())
}

fn declaration_by_id<'a>(
    declarations: &'a [TrustedDeclarationSignature],
    by_id: &BTreeMap<ExternalDefId, usize>,
    id: &ExternalDefId,
) -> &'a TrustedDeclarationSignature {
    &declarations[*by_id
        .get(id)
        .expect("trusted relation identities were validated")]
}

fn trusted_callable_contract_matches(
    expected: &TrustedDeclarationSignature,
    actual: &TrustedDeclarationSignature,
    self_owner: &ExternalDefId,
    self_type: &TrustedTypeExpr,
    associated_bindings: &[TrustedAssociatedBinding],
) -> bool {
    let (
        TrustedSignatureKind::Callable {
            bang: expected_bang,
            generic_parameters: expected_generics,
            parameters: expected_parameters,
            result: expected_result,
            requirements: expected_requirements,
        },
        TrustedSignatureKind::Callable {
            bang: actual_bang,
            generic_parameters: actual_generics,
            parameters: actual_parameters,
            result: actual_result,
            requirements: actual_requirements,
        },
    ) = (expected.kind(), actual.kind())
    else {
        return false;
    };
    if expected_bang != actual_bang
        || expected_generics.len() != actual_generics.len()
        || expected_parameters.len() != actual_parameters.len()
        || expected_requirements.len() != actual_requirements.len()
    {
        return false;
    }
    let generic_bounds_match =
        expected_generics
            .iter()
            .zip(actual_generics)
            .all(|(expected, actual)| {
                unordered_equal(
                    &specialize_trusted_bounds(
                        &expected.bounds,
                        self_owner,
                        self_type,
                        associated_bindings,
                    ),
                    &actual.bounds,
                )
            });
    let parameters_match =
        expected_parameters
            .iter()
            .zip(actual_parameters)
            .all(|(expected, actual)| {
                specialize_trusted_type(&expected.ty, self_owner, self_type, associated_bindings)
                    == actual.ty
            });
    let specialized_requirements = expected_requirements
        .iter()
        .map(|expected| match expected {
            TrustedCallableRequirement::FixedWidthInteger(expected) => {
                TrustedCallableRequirement::FixedWidthInteger(specialize_trusted_type(
                    expected,
                    self_owner,
                    self_type,
                    associated_bindings,
                ))
            }
        })
        .collect::<Vec<_>>();
    let requirements_match = unordered_equal(&specialized_requirements, actual_requirements);
    generic_bounds_match
        && parameters_match
        && requirements_match
        && specialize_trusted_type(expected_result, self_owner, self_type, associated_bindings)
            == *actual_result
}

fn unordered_equal<T: Eq>(left: &[T], right: &[T]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut matched = vec![false; right.len()];
    left.iter().all(|value| {
        right
            .iter()
            .enumerate()
            .find(|(index, candidate)| !matched[*index] && *candidate == value)
            .is_some_and(|(index, _)| {
                matched[index] = true;
                true
            })
    })
}

fn trusted_adapter_contract_matches(
    adapter: &TrustedDeclarationSignature,
    method: &TrustedDeclarationSignature,
    method_owner: &ExternalDefId,
    argument: u32,
) -> bool {
    let (
        TrustedSignatureKind::Callable {
            bang: adapter_bang,
            generic_parameters,
            parameters: adapter_parameters,
            result: adapter_result,
            ..
        },
        TrustedSignatureKind::Callable {
            bang: method_bang,
            generic_parameters: method_generics,
            parameters: method_parameters,
            result: method_result,
            ..
        },
    ) = (adapter.kind(), method.kind())
    else {
        return false;
    };
    let Ok(argument) = usize::try_from(argument) else {
        return false;
    };
    let Some(adapter_argument) = adapter_parameters.get(argument).map(|value| &value.ty) else {
        return false;
    };
    if !method_generics.is_empty() || method_parameters.len() != adapter_parameters.len() {
        return false;
    }
    let receiver = match &method_parameters[argument].ty {
        TrustedTypeExpr::SelfType(owner) if owner == method_owner => adapter_argument,
        TrustedTypeExpr::Reference { referent, .. } if matches!(referent.as_ref(), TrustedTypeExpr::SelfType(owner) if owner == method_owner) => {
            adapter_argument
        }
        _ => return false,
    };
    let parameters_match = method_parameters
        .iter()
        .zip(adapter_parameters)
        .enumerate()
        .all(|(index, (method_parameter, adapter_parameter))| {
            index == argument
                || specialize_trusted_type(&method_parameter.ty, method_owner, receiver, &[])
                    == adapter_parameter.ty
        });
    let has_bound = match receiver {
        TrustedTypeExpr::GenericParameter(index) => usize::try_from(*index)
            .ok()
            .and_then(|index| generic_parameters.get(index))
            .is_some_and(|parameter| {
                parameter
                    .bounds
                    .iter()
                    .any(|bound| bound.declaration == *method_owner && bound.arguments.is_empty())
            }),
        _ => false,
    };
    adapter_bang == method_bang
        && has_bound
        && parameters_match
        && specialize_trusted_type(method_result, method_owner, receiver, &[]) == *adapter_result
}

fn specialize_trusted_bounds(
    bounds: &[TrustedTraitExpr],
    self_owner: &ExternalDefId,
    self_type: &TrustedTypeExpr,
    associated_bindings: &[TrustedAssociatedBinding],
) -> Box<[TrustedTraitExpr]> {
    bounds
        .iter()
        .map(|bound| TrustedTraitExpr {
            declaration: bound.declaration.clone(),
            arguments: bound
                .arguments
                .iter()
                .map(|argument| {
                    specialize_trusted_type(argument, self_owner, self_type, associated_bindings)
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn specialize_trusted_type(
    ty: &TrustedTypeExpr,
    self_owner: &ExternalDefId,
    self_type: &TrustedTypeExpr,
    associated_bindings: &[TrustedAssociatedBinding],
) -> TrustedTypeExpr {
    specialize_trusted_type_inner(
        ty,
        self_owner,
        self_type,
        associated_bindings,
        &mut BTreeSet::new(),
    )
}

fn specialize_trusted_type_inner(
    ty: &TrustedTypeExpr,
    self_owner: &ExternalDefId,
    self_type: &TrustedTypeExpr,
    associated_bindings: &[TrustedAssociatedBinding],
    active_bindings: &mut BTreeSet<ExternalDefId>,
) -> TrustedTypeExpr {
    match ty {
        TrustedTypeExpr::SelfType(owner) if owner == self_owner => self_type.clone(),
        TrustedTypeExpr::Intrinsic { head, arguments } => TrustedTypeExpr::Intrinsic {
            head: *head,
            arguments: specialize_trusted_types_inner(
                arguments,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            ),
        },
        TrustedTypeExpr::Nominal { head, arguments } => TrustedTypeExpr::Nominal {
            head: head.clone(),
            arguments: specialize_trusted_types_inner(
                arguments,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            ),
        },
        TrustedTypeExpr::Reference {
            mutability,
            referent,
        } => TrustedTypeExpr::Reference {
            mutability: *mutability,
            referent: Box::new(specialize_trusted_type_inner(
                referent,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            )),
        },
        TrustedTypeExpr::RawPointer {
            mutability,
            referent,
        } => TrustedTypeExpr::RawPointer {
            mutability: *mutability,
            referent: Box::new(specialize_trusted_type_inner(
                referent,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            )),
        },
        TrustedTypeExpr::Tuple(elements) => TrustedTypeExpr::Tuple(specialize_trusted_types_inner(
            elements,
            self_owner,
            self_type,
            associated_bindings,
            active_bindings,
        )),
        TrustedTypeExpr::Callable { parameters, result } => TrustedTypeExpr::Callable {
            parameters: specialize_trusted_types_inner(
                parameters,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            ),
            result: Box::new(specialize_trusted_type_inner(
                result,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            )),
        },
        TrustedTypeExpr::ErrorUnion(members) => {
            TrustedTypeExpr::ErrorUnion(specialize_trusted_types_inner(
                members,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            ))
        }
        TrustedTypeExpr::AssociatedProjection {
            base,
            trait_declaration,
            trait_arguments,
            associated,
        } => {
            let base = specialize_trusted_type_inner(
                base,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            );
            if trait_declaration == self_owner
                && base == *self_type
                && let Some(binding) = associated_bindings
                    .iter()
                    .find(|binding| binding.associated == *associated)
                && active_bindings.insert(associated.clone())
            {
                let specialized = specialize_trusted_type_inner(
                    &binding.value,
                    self_owner,
                    self_type,
                    associated_bindings,
                    active_bindings,
                );
                active_bindings.remove(associated);
                return specialized;
            }
            TrustedTypeExpr::AssociatedProjection {
                base: Box::new(base),
                trait_declaration: trait_declaration.clone(),
                trait_arguments: specialize_trusted_types_inner(
                    trait_arguments,
                    self_owner,
                    self_type,
                    associated_bindings,
                    active_bindings,
                ),
                associated: associated.clone(),
            }
        }
        TrustedTypeExpr::Never
        | TrustedTypeExpr::Primitive(_)
        | TrustedTypeExpr::GenericParameter(_)
        | TrustedTypeExpr::SelfType(_) => ty.clone(),
    }
}

fn specialize_trusted_types_inner(
    types: &[TrustedTypeExpr],
    self_owner: &ExternalDefId,
    self_type: &TrustedTypeExpr,
    associated_bindings: &[TrustedAssociatedBinding],
    active_bindings: &mut BTreeSet<ExternalDefId>,
) -> Box<[TrustedTypeExpr]> {
    types
        .iter()
        .map(|ty| {
            specialize_trusted_type_inner(
                ty,
                self_owner,
                self_type,
                associated_bindings,
                active_bindings,
            )
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}
