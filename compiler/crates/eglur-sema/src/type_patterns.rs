use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CanonicalTarget, CanonicalTypeStore, GenericParameterId, NominalHead, PrimitiveType,
    RawPointerMutability, ReferenceMutability, TypeId, TypeKind, TypeStoreError,
};

#[derive(Clone, Copy)]
pub(crate) enum ProjectionMode {
    Structural,
    Flexible,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum PatternVariable {
    Generic(GenericParameterId),
    Projection(TypeId),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum TypePattern {
    Variable(PatternVariable),
    Never,
    Primitive(PrimitiveType),
    Nominal(NominalHead, Box<[TypePattern]>),
    RigidGeneric(GenericParameterId),
    SelfType(CanonicalTarget),
    Reference(ReferenceMutability, Box<TypePattern>),
    RawPointer(RawPointerMutability, Box<TypePattern>),
    Tuple(Box<[TypePattern]>),
    Callable(Box<[TypePattern]>, Box<TypePattern>),
    ErrorUnion(Box<[TypePattern]>),
    Projection {
        base: Box<TypePattern>,
        trait_declaration: CanonicalTarget,
        trait_arguments: Box<[TypePattern]>,
        associated: CanonicalTarget,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum AlphaPattern {
    Variable(usize),
    Never,
    Primitive(PrimitiveType),
    Nominal(NominalHead, Box<[AlphaPattern]>),
    RigidGeneric(GenericParameterId),
    SelfType(CanonicalTarget),
    Reference(ReferenceMutability, Box<AlphaPattern>),
    RawPointer(RawPointerMutability, Box<AlphaPattern>),
    Tuple(Box<[AlphaPattern]>),
    Callable(Box<[AlphaPattern]>, Box<AlphaPattern>),
    ErrorUnion(Box<[AlphaPattern]>),
    Projection {
        base: Box<AlphaPattern>,
        trait_declaration: CanonicalTarget,
        trait_arguments: Box<[AlphaPattern]>,
        associated: CanonicalTarget,
    },
    FlexibleProjection(TypeId),
}

pub(crate) fn type_pattern(
    types: &CanonicalTypeStore,
    root: TypeId,
    parameters: &BTreeSet<GenericParameterId>,
) -> Result<TypePattern, TypeStoreError> {
    type_pattern_with(types, root, parameters, ProjectionMode::Structural)
}

pub(crate) fn type_pattern_with(
    types: &CanonicalTypeStore,
    root: TypeId,
    parameters: &BTreeSet<GenericParameterId>,
    projections: ProjectionMode,
) -> Result<TypePattern, TypeStoreError> {
    Ok(match types.kind(root)? {
        TypeKind::Never => TypePattern::Never,
        TypeKind::Primitive(primitive) => TypePattern::Primitive(*primitive),
        TypeKind::Nominal { head, substitution } => TypePattern::Nominal(
            head.clone(),
            types
                .substitution(*substitution)?
                .iter()
                .map(|&argument| type_pattern_with(types, argument, parameters, projections))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        ),
        TypeKind::GenericParameter(parameter) if parameters.contains(parameter) => {
            TypePattern::Variable(PatternVariable::Generic(parameter.clone()))
        }
        TypeKind::GenericParameter(parameter) => TypePattern::RigidGeneric(parameter.clone()),
        TypeKind::SelfType(owner) => TypePattern::SelfType(owner.clone()),
        TypeKind::Reference {
            mutability,
            referent,
        } => TypePattern::Reference(
            *mutability,
            Box::new(type_pattern_with(
                types,
                *referent,
                parameters,
                projections,
            )?),
        ),
        TypeKind::RawPointer {
            mutability,
            referent,
        } => TypePattern::RawPointer(
            *mutability,
            Box::new(type_pattern_with(
                types,
                *referent,
                parameters,
                projections,
            )?),
        ),
        TypeKind::Tuple(elements) => TypePattern::Tuple(
            elements
                .iter()
                .map(|&element| type_pattern_with(types, element, parameters, projections))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        ),
        TypeKind::Callable {
            parameters: values,
            result,
        } => TypePattern::Callable(
            values
                .iter()
                .map(|&parameter| type_pattern_with(types, parameter, parameters, projections))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
            Box::new(type_pattern_with(types, *result, parameters, projections)?),
        ),
        TypeKind::ErrorUnion(members) => TypePattern::ErrorUnion(
            members
                .iter()
                .map(|&member| type_pattern_with(types, member, parameters, projections))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
        ),
        TypeKind::AssociatedProjection(_) if matches!(projections, ProjectionMode::Flexible) => {
            TypePattern::Variable(PatternVariable::Projection(root))
        }
        TypeKind::AssociatedProjection(projection) => TypePattern::Projection {
            base: Box::new(type_pattern_with(
                types,
                projection.base(),
                parameters,
                projections,
            )?),
            trait_declaration: projection.trait_ref().declaration().clone(),
            trait_arguments: types
                .substitution(projection.trait_ref().substitution())?
                .iter()
                .map(|&argument| type_pattern_with(types, argument, parameters, projections))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice(),
            associated: projection.associated().clone(),
        },
    })
}

pub(crate) fn alpha_pattern(
    pattern: &TypePattern,
    positions: &BTreeMap<GenericParameterId, usize>,
) -> AlphaPattern {
    match pattern {
        TypePattern::Variable(PatternVariable::Generic(parameter)) => {
            AlphaPattern::Variable(positions[parameter])
        }
        TypePattern::Variable(PatternVariable::Projection(projection)) => {
            AlphaPattern::FlexibleProjection(*projection)
        }
        TypePattern::Never => AlphaPattern::Never,
        TypePattern::Primitive(primitive) => AlphaPattern::Primitive(*primitive),
        TypePattern::Nominal(head, arguments) => {
            AlphaPattern::Nominal(head.clone(), alpha_patterns(arguments, positions))
        }
        TypePattern::RigidGeneric(parameter) => AlphaPattern::RigidGeneric(parameter.clone()),
        TypePattern::SelfType(owner) => AlphaPattern::SelfType(owner.clone()),
        TypePattern::Reference(mutability, referent) => {
            AlphaPattern::Reference(*mutability, Box::new(alpha_pattern(referent, positions)))
        }
        TypePattern::RawPointer(mutability, referent) => {
            AlphaPattern::RawPointer(*mutability, Box::new(alpha_pattern(referent, positions)))
        }
        TypePattern::Tuple(elements) => AlphaPattern::Tuple(alpha_patterns(elements, positions)),
        TypePattern::Callable(parameters, result) => AlphaPattern::Callable(
            alpha_patterns(parameters, positions),
            Box::new(alpha_pattern(result, positions)),
        ),
        TypePattern::ErrorUnion(members) => {
            let mut members = alpha_patterns(members, positions).into_vec();
            members.sort();
            AlphaPattern::ErrorUnion(members.into_boxed_slice())
        }
        TypePattern::Projection {
            base,
            trait_declaration,
            trait_arguments,
            associated,
        } => AlphaPattern::Projection {
            base: Box::new(alpha_pattern(base, positions)),
            trait_declaration: trait_declaration.clone(),
            trait_arguments: alpha_patterns(trait_arguments, positions),
            associated: associated.clone(),
        },
    }
}

fn alpha_patterns(
    patterns: &[TypePattern],
    positions: &BTreeMap<GenericParameterId, usize>,
) -> Box<[AlphaPattern]> {
    patterns
        .iter()
        .map(|pattern| alpha_pattern(pattern, positions))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

pub(crate) fn one_way_pattern_match(
    general: &TypePattern,
    specific: &TypePattern,
    bindings: &mut BTreeMap<GenericParameterId, TypePattern>,
) -> bool {
    if let TypePattern::Variable(PatternVariable::Generic(variable)) = general {
        return match bindings.get(variable) {
            Some(existing) => existing == specific,
            None => {
                bindings.insert(variable.clone(), specific.clone());
                true
            }
        };
    }
    match (general, specific) {
        (TypePattern::Never, TypePattern::Never)
        | (TypePattern::Primitive(_), TypePattern::Primitive(_))
        | (TypePattern::RigidGeneric(_), TypePattern::RigidGeneric(_))
        | (TypePattern::SelfType(_), TypePattern::SelfType(_)) => general == specific,
        (TypePattern::Nominal(left_head, left), TypePattern::Nominal(right_head, right))
            if left_head == right_head =>
        {
            one_way_pattern_lists(left, right, bindings)
        }
        (TypePattern::Reference(left_mut, left), TypePattern::Reference(right_mut, right))
            if left_mut == right_mut =>
        {
            one_way_pattern_match(left, right, bindings)
        }
        (TypePattern::RawPointer(left_mut, left), TypePattern::RawPointer(right_mut, right))
            if left_mut == right_mut =>
        {
            one_way_pattern_match(left, right, bindings)
        }
        (TypePattern::Tuple(left), TypePattern::Tuple(right))
        | (TypePattern::ErrorUnion(left), TypePattern::ErrorUnion(right)) => {
            one_way_pattern_lists(left, right, bindings)
        }
        (
            TypePattern::Callable(left_parameters, left_result),
            TypePattern::Callable(right_parameters, right_result),
        ) => {
            one_way_pattern_lists(left_parameters, right_parameters, bindings)
                && one_way_pattern_match(left_result, right_result, bindings)
        }
        (
            TypePattern::Projection {
                base: left_base,
                trait_declaration: left_trait,
                trait_arguments: left_arguments,
                associated: left_associated,
            },
            TypePattern::Projection {
                base: right_base,
                trait_declaration: right_trait,
                trait_arguments: right_arguments,
                associated: right_associated,
            },
        ) if left_trait == right_trait && left_associated == right_associated => {
            one_way_pattern_match(left_base, right_base, bindings)
                && one_way_pattern_lists(left_arguments, right_arguments, bindings)
        }
        _ => false,
    }
}

pub(crate) fn one_way_pattern_lists(
    left: &[TypePattern],
    right: &[TypePattern],
    bindings: &mut BTreeMap<GenericParameterId, TypePattern>,
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| one_way_pattern_match(left, right, bindings))
}

pub(crate) fn unify_pattern_lists(left: &[TypePattern], right: &[TypePattern]) -> bool {
    left.len() == right.len() && {
        let mut unifier = PatternUnifier::default();
        left.iter()
            .cloned()
            .zip(right.iter().cloned())
            .all(|(left, right)| unifier.unify(left, right))
    }
}

#[derive(Default)]
struct PatternUnifier {
    bindings: BTreeMap<PatternVariable, TypePattern>,
}

impl PatternUnifier {
    fn unify(&mut self, left: TypePattern, right: TypePattern) -> bool {
        let left = self.resolve(left);
        let right = self.resolve(right);
        if left == right {
            return true;
        }
        match (left, right) {
            (TypePattern::Variable(variable), value) | (value, TypePattern::Variable(variable)) => {
                self.bind(variable, value)
            }
            (TypePattern::Nominal(left_head, left), TypePattern::Nominal(right_head, right))
                if left_head == right_head =>
            {
                self.unify_lists(left, right)
            }
            (TypePattern::Reference(left_mut, left), TypePattern::Reference(right_mut, right))
                if left_mut == right_mut =>
            {
                self.unify(*left, *right)
            }
            (
                TypePattern::RawPointer(left_mut, left),
                TypePattern::RawPointer(right_mut, right),
            ) if left_mut == right_mut => self.unify(*left, *right),
            (TypePattern::Tuple(left), TypePattern::Tuple(right))
            | (TypePattern::ErrorUnion(left), TypePattern::ErrorUnion(right)) => {
                self.unify_lists(left, right)
            }
            (
                TypePattern::Callable(left_parameters, left_result),
                TypePattern::Callable(right_parameters, right_result),
            ) => {
                self.unify_lists(left_parameters, right_parameters)
                    && self.unify(*left_result, *right_result)
            }
            (
                TypePattern::Projection {
                    base: left_base,
                    trait_declaration: left_trait,
                    trait_arguments: left_arguments,
                    associated: left_associated,
                },
                TypePattern::Projection {
                    base: right_base,
                    trait_declaration: right_trait,
                    trait_arguments: right_arguments,
                    associated: right_associated,
                },
            ) if left_trait == right_trait && left_associated == right_associated => {
                self.unify(*left_base, *right_base)
                    && self.unify_lists(left_arguments, right_arguments)
            }
            _ => false,
        }
    }

    fn unify_lists(&mut self, left: Box<[TypePattern]>, right: Box<[TypePattern]>) -> bool {
        left.len() == right.len()
            && left
                .into_vec()
                .into_iter()
                .zip(right.into_vec())
                .all(|(left, right)| self.unify(left, right))
    }

    fn bind(&mut self, variable: PatternVariable, value: TypePattern) -> bool {
        let value = self.resolve(value);
        if value == TypePattern::Variable(variable.clone()) {
            return true;
        }
        if pattern_contains_variable(&value, &variable) {
            return false;
        }
        self.bindings.insert(variable, value);
        true
    }

    fn resolve(&self, mut value: TypePattern) -> TypePattern {
        let mut seen = BTreeSet::new();
        while let TypePattern::Variable(variable) = &value {
            if !seen.insert(variable.clone()) {
                break;
            }
            let Some(found) = self.bindings.get(variable) else {
                break;
            };
            value = found.clone();
        }
        match value {
            TypePattern::Nominal(head, arguments) => {
                TypePattern::Nominal(head, self.resolve_many(arguments))
            }
            TypePattern::Reference(mutability, referent) => {
                TypePattern::Reference(mutability, Box::new(self.resolve(*referent)))
            }
            TypePattern::RawPointer(mutability, referent) => {
                TypePattern::RawPointer(mutability, Box::new(self.resolve(*referent)))
            }
            TypePattern::Tuple(elements) => TypePattern::Tuple(self.resolve_many(elements)),
            TypePattern::Callable(parameters, result) => TypePattern::Callable(
                self.resolve_many(parameters),
                Box::new(self.resolve(*result)),
            ),
            TypePattern::ErrorUnion(members) => TypePattern::ErrorUnion(self.resolve_many(members)),
            TypePattern::Projection {
                base,
                trait_declaration,
                trait_arguments,
                associated,
            } => TypePattern::Projection {
                base: Box::new(self.resolve(*base)),
                trait_declaration,
                trait_arguments: self.resolve_many(trait_arguments),
                associated,
            },
            terminal => terminal,
        }
    }

    fn resolve_many(&self, values: Box<[TypePattern]>) -> Box<[TypePattern]> {
        values
            .into_vec()
            .into_iter()
            .map(|value| self.resolve(value))
            .collect::<Vec<_>>()
            .into_boxed_slice()
    }
}

fn pattern_contains_variable(pattern: &TypePattern, needle: &PatternVariable) -> bool {
    match pattern {
        TypePattern::Variable(variable) => variable == needle,
        TypePattern::Nominal(_, arguments)
        | TypePattern::Tuple(arguments)
        | TypePattern::ErrorUnion(arguments) => arguments
            .iter()
            .any(|argument| pattern_contains_variable(argument, needle)),
        TypePattern::Reference(_, referent) | TypePattern::RawPointer(_, referent) => {
            pattern_contains_variable(referent, needle)
        }
        TypePattern::Callable(parameters, result) => {
            parameters
                .iter()
                .any(|parameter| pattern_contains_variable(parameter, needle))
                || pattern_contains_variable(result, needle)
        }
        TypePattern::Projection {
            base,
            trait_arguments,
            ..
        } => {
            pattern_contains_variable(base, needle)
                || trait_arguments
                    .iter()
                    .any(|argument| pattern_contains_variable(argument, needle))
        }
        TypePattern::Never
        | TypePattern::Primitive(_)
        | TypePattern::RigidGeneric(_)
        | TypePattern::SelfType(_) => false,
    }
}
