use std::collections::{BTreeMap, BTreeSet};

use crate::type_patterns::{
    AlphaPattern, ProjectionMode, TypePattern, alpha_pattern, type_pattern_with,
    unify_pattern_lists,
};
use crate::{
    CallableSignature, CanonicalTypeStore, GenericParameterId, TypeId, TypeKind, TypeStoreError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OverloadPairConflict {
    ReturnOnlyDifference,
    Overlap,
}

pub(crate) fn classify_overload_pair(
    types: &CanonicalTypeStore,
    left: &CallableSignature,
    right: &CallableSignature,
) -> Result<Option<OverloadPairConflict>, TypeStoreError> {
    if left.parameters().len() != right.parameters().len() {
        return Ok(None);
    }

    let left_shape = OverloadShape::new(types, left)?;
    let right_shape = OverloadShape::new(types, right)?;
    if left_shape.alpha_parameters == right_shape.alpha_parameters {
        return Ok(Some(
            if left_shape.alpha_result == right_shape.alpha_result {
                OverloadPairConflict::Overlap
            } else {
                OverloadPairConflict::ReturnOnlyDifference
            },
        ));
    }
    Ok(
        parameter_patterns_overlap(left, right, &left_shape.parameters, &right_shape.parameters)
            .then_some(OverloadPairConflict::Overlap),
    )
}

fn parameter_patterns_overlap(
    left: &CallableSignature,
    right: &CallableSignature,
    left_patterns: &[TypePattern],
    right_patterns: &[TypePattern],
) -> bool {
    (0..=left_patterns.len()).any(|positional| {
        let mut left_order = left_patterns[..positional].to_vec();
        let mut right_order = right_patterns[..positional].to_vec();
        let mut right_named = BTreeMap::new();
        for (index, parameter) in right.parameters()[positional..].iter().enumerate() {
            let Some(name) = parameter.name() else {
                return false;
            };
            if right_named.insert(name, positional + index).is_some() {
                return false;
            }
        }
        for (index, parameter) in left.parameters()[positional..].iter().enumerate() {
            let Some(name) = parameter.name() else {
                return false;
            };
            let Some(right_index) = right_named.remove(name) else {
                return false;
            };
            left_order.push(left_patterns[positional + index].clone());
            right_order.push(right_patterns[right_index].clone());
        }
        right_named.is_empty() && unify_pattern_lists(&left_order, &right_order)
    })
}

struct OverloadShape {
    parameters: Box<[TypePattern]>,
    alpha_parameters: Box<[AlphaPattern]>,
    alpha_result: AlphaPattern,
}

impl OverloadShape {
    fn new(
        types: &CanonicalTypeStore,
        callable: &CallableSignature,
    ) -> Result<Self, TypeStoreError> {
        let parameters = callable
            .generic_parameters()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let positions = callable
            .generic_parameters()
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, parameter)| (parameter, index))
            .collect::<BTreeMap<_, _>>();
        let structural = callable
            .parameters()
            .iter()
            .map(|parameter| {
                type_pattern_with(
                    types,
                    parameter.ty(),
                    &parameters,
                    ProjectionMode::Structural,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let alpha_parameters = structural
            .iter()
            .map(|parameter| alpha_pattern(parameter, &positions))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let result = type_pattern_with(
            types,
            callable.result(),
            &parameters,
            ProjectionMode::Structural,
        )?;
        let parameters = callable
            .parameters()
            .iter()
            .map(|parameter| {
                type_pattern_with(types, parameter.ty(), &parameters, ProjectionMode::Flexible)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        Ok(Self {
            parameters,
            alpha_parameters,
            alpha_result: alpha_pattern(&result, &positions),
        })
    }
}

pub(crate) fn exact_parameter_match(
    types: &CanonicalTypeStore,
    callable: &CallableSignature,
    actual: &[TypeId],
    replacements: &mut BTreeMap<GenericParameterId, TypeId>,
) -> Result<bool, TypeStoreError> {
    if callable.parameters().len() != actual.len() {
        return Ok(false);
    }
    let parameters = callable
        .generic_parameters()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for (parameter, &actual) in callable.parameters().iter().zip(actual) {
        if !exact_type_match(
            types,
            parameter.ty(),
            actual,
            &parameters,
            replacements,
            true,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn exact_type_match(
    types: &CanonicalTypeStore,
    template: TypeId,
    actual: TypeId,
    parameters: &BTreeSet<GenericParameterId>,
    replacements: &mut BTreeMap<GenericParameterId, TypeId>,
    parameter_root: bool,
) -> Result<bool, TypeStoreError> {
    if let TypeKind::GenericParameter(parameter) = types.kind(template)?
        && parameters.contains(parameter)
    {
        if let Some(&replacement) = replacements.get(parameter) {
            return Ok(replacement == actual);
        }
        if parameter_root && actual == types.never() {
            return Ok(false);
        }
        replacements.insert(parameter.clone(), actual);
        return Ok(true);
    }
    if template == actual {
        return Ok(true);
    }
    if actual == types.never() {
        return Ok(false);
    }

    let pairs = match (types.kind(template)?, types.kind(actual)?) {
        (
            TypeKind::Nominal {
                head: left_head,
                substitution: left_substitution,
            },
            TypeKind::Nominal {
                head: right_head,
                substitution: right_substitution,
            },
        ) if left_head == right_head => {
            let left = types.substitution(*left_substitution)?;
            let right = types.substitution(*right_substitution)?;
            if left.len() != right.len() {
                return Ok(false);
            }
            left.iter().copied().zip(right.iter().copied()).collect()
        }
        (
            TypeKind::Reference {
                mutability: left_mutability,
                referent: left_referent,
            },
            TypeKind::Reference {
                mutability: right_mutability,
                referent: right_referent,
            },
        ) if left_mutability == right_mutability => vec![(*left_referent, *right_referent)],
        (
            TypeKind::RawPointer {
                mutability: left_mutability,
                referent: left_referent,
            },
            TypeKind::RawPointer {
                mutability: right_mutability,
                referent: right_referent,
            },
        ) if left_mutability == right_mutability => vec![(*left_referent, *right_referent)],
        (TypeKind::Tuple(left), TypeKind::Tuple(right))
        | (TypeKind::ErrorUnion(left), TypeKind::ErrorUnion(right))
            if left.len() == right.len() =>
        {
            left.iter().copied().zip(right.iter().copied()).collect()
        }
        (
            TypeKind::Callable {
                parameters: left_parameters,
                result: left_result,
            },
            TypeKind::Callable {
                parameters: right_parameters,
                result: right_result,
            },
        ) if left_parameters.len() == right_parameters.len() => left_parameters
            .iter()
            .copied()
            .zip(right_parameters.iter().copied())
            .chain([(*left_result, *right_result)])
            .collect(),
        (TypeKind::AssociatedProjection(_), _) => return Ok(true),
        _ => return Ok(false),
    };

    for (template, actual) in pairs {
        if !exact_type_match(types, template, actual, parameters, replacements, false)? {
            return Ok(false);
        }
    }
    Ok(true)
}
