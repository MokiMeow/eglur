use std::collections::{BTreeMap, BTreeSet};

use crate::type_patterns::{
    AlphaPattern, TypePattern, alpha_pattern, one_way_pattern_match, type_pattern,
    unify_pattern_lists,
};
use crate::{
    CanonicalTarget, CanonicalTypeStore, DeclarationSignatures, FactState, GenericParameterId,
    GenericParameterSignature, NominalHead, PrimitiveType, RawPointerMutability,
    ReferenceMutability, TraitImplHeader, TypeId, TypeKind, TypeStoreError,
};

use super::model::{TraitNormalization, TraitObligation};

pub(crate) trait TraitHeaderFacts {
    fn header_types(&self) -> &CanonicalTypeStore;

    fn header_generic_parameter(
        &self,
        parameter: &GenericParameterId,
    ) -> FactState<'_, GenericParameterSignature>;
}

impl TraitHeaderFacts for DeclarationSignatures {
    fn header_types(&self) -> &CanonicalTypeStore {
        self.types()
    }

    fn header_generic_parameter(
        &self,
        parameter: &GenericParameterId,
    ) -> FactState<'_, GenericParameterSignature> {
        self.generic_parameter(parameter)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ImplTargetBucket {
    Blanket,
    Never,
    Primitive(PrimitiveType),
    Nominal(NominalHead),
    SelfType(CanonicalTarget),
    Reference(ReferenceMutability),
    RawPointer(RawPointerMutability),
    Tuple(usize),
    Callable(usize),
    ErrorUnion(usize),
    Projection,
}

pub(super) type ImplSubstitution = BTreeMap<GenericParameterId, TypeId>;
pub(super) type ImplCandidate = (CanonicalTarget, ImplSubstitution);
pub(super) type ImplBuckets = BTreeMap<(CanonicalTarget, ImplTargetBucket), Box<[CanonicalTarget]>>;

pub(super) fn target_bucket(
    types: &CanonicalTypeStore,
    target: TypeId,
) -> Result<ImplTargetBucket, TypeStoreError> {
    Ok(match types.kind(target)? {
        TypeKind::Never => ImplTargetBucket::Never,
        TypeKind::Primitive(primitive) => ImplTargetBucket::Primitive(*primitive),
        TypeKind::Nominal { head, .. } => ImplTargetBucket::Nominal(head.clone()),
        TypeKind::GenericParameter(_) => ImplTargetBucket::Blanket,
        TypeKind::SelfType(owner) => ImplTargetBucket::SelfType(owner.clone()),
        TypeKind::Reference { mutability, .. } => ImplTargetBucket::Reference(*mutability),
        TypeKind::RawPointer { mutability, .. } => ImplTargetBucket::RawPointer(*mutability),
        TypeKind::Tuple(elements) => ImplTargetBucket::Tuple(elements.len()),
        TypeKind::Callable { parameters, .. } => ImplTargetBucket::Callable(parameters.len()),
        TypeKind::ErrorUnion(members) => ImplTargetBucket::ErrorUnion(members.len()),
        TypeKind::AssociatedProjection(_) => ImplTargetBucket::Projection,
    })
}

pub(crate) fn header_mentions_parameter(
    types: &CanonicalTypeStore,
    header: &TraitImplHeader,
    parameter: &GenericParameterId,
) -> Result<bool, TypeStoreError> {
    if type_mentions_parameter(types, header.target(), parameter)? {
        return Ok(true);
    }
    for argument in types.substitution(header.trait_ref().substitution())? {
        if type_mentions_parameter(types, *argument, parameter)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn header_parameter_below_error_union(
    types: &CanonicalTypeStore,
    header: &TraitImplHeader,
) -> Result<Option<GenericParameterId>, TypeStoreError> {
    let parameters = header
        .generic_parameters()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut pending = vec![(header.target(), false)];
    pending.extend(
        types
            .substitution(header.trait_ref().substitution())?
            .iter()
            .copied()
            .map(|argument| (argument, false)),
    );
    let mut seen = BTreeSet::new();
    let mut found = BTreeSet::new();
    while let Some((current, below_union)) = pending.pop() {
        if !seen.insert((current, below_union)) {
            continue;
        }
        match types.kind(current)? {
            TypeKind::GenericParameter(parameter)
                if below_union && parameters.contains(parameter) =>
            {
                found.insert(parameter.clone());
            }
            TypeKind::Nominal { substitution, .. } => {
                pending.extend(
                    types
                        .substitution(*substitution)?
                        .iter()
                        .copied()
                        .map(|argument| (argument, below_union)),
                );
            }
            TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                pending.push((*referent, below_union));
            }
            TypeKind::Tuple(elements) => {
                pending.extend(
                    elements
                        .iter()
                        .copied()
                        .map(|element| (element, below_union)),
                );
            }
            TypeKind::ErrorUnion(members) => {
                pending.extend(members.iter().copied().map(|member| (member, true)));
            }
            TypeKind::Callable { parameters, result } => {
                pending.extend(
                    parameters
                        .iter()
                        .copied()
                        .map(|parameter| (parameter, below_union)),
                );
                pending.push((*result, below_union));
            }
            TypeKind::AssociatedProjection(projection) => {
                pending.push((projection.base(), below_union));
                pending.extend(
                    types
                        .substitution(projection.trait_ref().substitution())?
                        .iter()
                        .copied()
                        .map(|argument| (argument, below_union)),
                );
            }
            TypeKind::Never
            | TypeKind::Primitive(_)
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_) => {}
        }
    }
    Ok(header
        .generic_parameters()
        .iter()
        .find(|parameter| found.contains(*parameter))
        .cloned())
}

fn type_mentions_parameter(
    types: &CanonicalTypeStore,
    root: TypeId,
    parameter: &GenericParameterId,
) -> Result<bool, TypeStoreError> {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        match types.kind(current)? {
            TypeKind::GenericParameter(found) if found == parameter => return Ok(true),
            TypeKind::Nominal { substitution, .. } => {
                pending.extend(types.substitution(*substitution)?.iter().copied());
            }
            TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                pending.push(*referent);
            }
            TypeKind::Tuple(elements) | TypeKind::ErrorUnion(elements) => {
                pending.extend(elements.iter().copied());
            }
            TypeKind::Callable { parameters, result } => {
                pending.extend(parameters.iter().copied());
                pending.push(*result);
            }
            TypeKind::AssociatedProjection(projection) => {
                pending.push(projection.base());
                pending.extend(
                    types
                        .substitution(projection.trait_ref().substitution())?
                        .iter()
                        .copied(),
                );
            }
            TypeKind::Never
            | TypeKind::Primitive(_)
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_) => {}
        }
    }
    Ok(false)
}

pub(super) fn match_impl(
    types: &CanonicalTypeStore,
    header: &TraitImplHeader,
    obligation: &TraitObligation,
) -> Result<Option<BTreeMap<GenericParameterId, TypeId>>, TypeStoreError> {
    if header.trait_ref().declaration() != obligation.trait_ref().declaration() {
        return Ok(None);
    }
    let parameters = header
        .generic_parameters()
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut replacements = BTreeMap::new();
    if !match_type(
        types,
        header.target(),
        obligation.subject(),
        &parameters,
        &mut replacements,
    )? {
        return Ok(None);
    }
    let templates = types.substitution(header.trait_ref().substitution())?;
    let actual = types.substitution(obligation.trait_ref().substitution())?;
    if templates.len() != actual.len() {
        return Ok(None);
    }
    for (&template, &actual) in templates.iter().zip(actual) {
        if !match_type(types, template, actual, &parameters, &mut replacements)? {
            return Ok(None);
        }
    }
    Ok(Some(replacements))
}

pub(super) fn match_type(
    types: &CanonicalTypeStore,
    template: TypeId,
    actual: TypeId,
    parameters: &BTreeSet<GenericParameterId>,
    replacements: &mut BTreeMap<GenericParameterId, TypeId>,
) -> Result<bool, TypeStoreError> {
    if let TypeKind::GenericParameter(parameter) = types.kind(template)?
        && parameters.contains(parameter)
    {
        return Ok(match replacements.get(parameter) {
            Some(existing) => *existing == actual,
            None => {
                replacements.insert(parameter.clone(), actual);
                true
            }
        });
    }
    let pairs = match (types.kind(template)?, types.kind(actual)?) {
        (TypeKind::Never, TypeKind::Never)
        | (TypeKind::Primitive(_), TypeKind::Primitive(_))
        | (TypeKind::GenericParameter(_), TypeKind::GenericParameter(_))
        | (TypeKind::SelfType(_), TypeKind::SelfType(_))
            if types.kind(template)? == types.kind(actual)? =>
        {
            Vec::new()
        }
        (
            TypeKind::Nominal {
                head: left,
                substitution: left_arguments,
            },
            TypeKind::Nominal {
                head: right,
                substitution: right_arguments,
            },
        ) if left == right => types
            .substitution(*left_arguments)?
            .iter()
            .copied()
            .zip(types.substitution(*right_arguments)?.iter().copied())
            .collect(),
        (
            TypeKind::Reference {
                mutability: left,
                referent: left_referent,
            },
            TypeKind::Reference {
                mutability: right,
                referent: right_referent,
            },
        ) if left == right => vec![(*left_referent, *right_referent)],
        (
            TypeKind::RawPointer {
                mutability: left,
                referent: left_referent,
            },
            TypeKind::RawPointer {
                mutability: right,
                referent: right_referent,
            },
        ) if left == right => vec![(*left_referent, *right_referent)],
        (TypeKind::Tuple(left), TypeKind::Tuple(right))
        | (TypeKind::ErrorUnion(left), TypeKind::ErrorUnion(right))
            if left.len() == right.len() =>
        {
            left.iter().copied().zip(right.iter().copied()).collect()
        }
        (
            TypeKind::Callable {
                parameters: left,
                result: left_result,
            },
            TypeKind::Callable {
                parameters: right,
                result: right_result,
            },
        ) if left.len() == right.len() => left
            .iter()
            .copied()
            .chain([*left_result])
            .zip(right.iter().copied().chain([*right_result]))
            .collect(),
        (TypeKind::AssociatedProjection(left), TypeKind::AssociatedProjection(right))
            if left.associated() == right.associated()
                && left.trait_ref().declaration() == right.trait_ref().declaration() =>
        {
            let mut pairs = vec![(left.base(), right.base())];
            pairs.extend(
                types
                    .substitution(left.trait_ref().substitution())?
                    .iter()
                    .copied()
                    .zip(
                        types
                            .substitution(right.trait_ref().substitution())?
                            .iter()
                            .copied(),
                    ),
            );
            pairs
        }
        _ => return Ok(false),
    };
    for (left, right) in pairs {
        if !match_type(types, left, right, parameters, replacements)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HeaderConflict {
    Duplicate,
    Overlap,
    Specialization,
}

pub(crate) fn classify_overlap(
    signatures: &impl TraitHeaderFacts,
    left: &TraitImplHeader,
    right: &TraitImplHeader,
) -> Result<Option<HeaderConflict>, TypeStoreError> {
    #[cfg(test)]
    super::coherence_index::tests::observe_comparison();

    if left.trait_ref().declaration() != right.trait_ref().declaration() {
        return Ok(None);
    }
    let left_pattern = HeaderPattern::new(signatures, left)?;
    let right_pattern = HeaderPattern::new(signatures, right)?;
    if left_pattern.alpha_shape == right_pattern.alpha_shape
        && left_pattern.alpha_bounds == right_pattern.alpha_bounds
    {
        return Ok(Some(HeaderConflict::Duplicate));
    }
    let left_subsumes = pattern_subsumes(&left_pattern.shape, &right_pattern.shape);
    let right_subsumes = pattern_subsumes(&right_pattern.shape, &left_pattern.shape);
    if left_subsumes != right_subsumes {
        return Ok(Some(HeaderConflict::Specialization));
    }
    if patterns_unify(&left_pattern.shape, &right_pattern.shape) {
        return Ok(Some(HeaderConflict::Overlap));
    }
    Ok(None)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct HeaderShape {
    trait_declaration: CanonicalTarget,
    trait_arguments: Box<[TypePattern]>,
    target: TypePattern,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AlphaBound {
    parameter: usize,
    trait_declaration: CanonicalTarget,
    arguments: Box<[AlphaPattern]>,
}

struct HeaderPattern {
    shape: HeaderShape,
    alpha_shape: AlphaHeaderShape,
    alpha_bounds: Box<[AlphaBound]>,
}

impl HeaderPattern {
    fn new(
        signatures: &impl TraitHeaderFacts,
        header: &TraitImplHeader,
    ) -> Result<Self, TypeStoreError> {
        let parameters = header
            .generic_parameters()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let positions = header
            .generic_parameters()
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, parameter)| (parameter, index))
            .collect::<BTreeMap<_, _>>();
        let trait_arguments = signatures
            .header_types()
            .substitution(header.trait_ref().substitution())?
            .iter()
            .copied()
            .map(|ty| type_pattern(signatures.header_types(), ty, &parameters))
            .collect::<Result<Vec<_>, _>>()?;
        let target = type_pattern(signatures.header_types(), header.target(), &parameters)?;
        let shape = HeaderShape {
            trait_declaration: header.trait_ref().declaration().clone(),
            trait_arguments: trait_arguments.into_boxed_slice(),
            target,
        };
        let alpha_shape = alpha_header_shape(&shape, &positions);
        let mut alpha_bounds = Vec::new();
        for (parameter_index, parameter) in header.generic_parameters().iter().enumerate() {
            let signature = match signatures.header_generic_parameter(parameter) {
                FactState::Known(signature) => signature,
                FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                    continue;
                }
            };
            for bound in signature.bounds() {
                let arguments = signatures
                    .header_types()
                    .substitution(bound.substitution())?
                    .iter()
                    .copied()
                    .map(|ty| type_pattern(signatures.header_types(), ty, &parameters))
                    .collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .map(|pattern| alpha_pattern(pattern, &positions))
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                alpha_bounds.push(AlphaBound {
                    parameter: parameter_index,
                    trait_declaration: bound.declaration().clone(),
                    arguments,
                });
            }
        }
        alpha_bounds.sort();
        Ok(Self {
            shape,
            alpha_shape,
            alpha_bounds: alpha_bounds.into_boxed_slice(),
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct AlphaHeaderShape {
    trait_declaration: CanonicalTarget,
    trait_arguments: Box<[AlphaPattern]>,
    target: AlphaPattern,
}

fn alpha_header_shape(
    shape: &HeaderShape,
    positions: &BTreeMap<GenericParameterId, usize>,
) -> AlphaHeaderShape {
    AlphaHeaderShape {
        trait_declaration: shape.trait_declaration.clone(),
        trait_arguments: shape
            .trait_arguments
            .iter()
            .map(|pattern| alpha_pattern(pattern, positions))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        target: alpha_pattern(&shape.target, positions),
    }
}

fn pattern_subsumes(general: &HeaderShape, specific: &HeaderShape) -> bool {
    if general.trait_declaration != specific.trait_declaration
        || general.trait_arguments.len() != specific.trait_arguments.len()
    {
        return false;
    }
    let mut bindings = BTreeMap::new();
    general
        .trait_arguments
        .iter()
        .zip(specific.trait_arguments.iter())
        .all(|(general, specific)| one_way_pattern_match(general, specific, &mut bindings))
        && one_way_pattern_match(&general.target, &specific.target, &mut bindings)
}

fn patterns_unify(left: &HeaderShape, right: &HeaderShape) -> bool {
    if left.trait_declaration != right.trait_declaration
        || left.trait_arguments.len() != right.trait_arguments.len()
    {
        return false;
    }
    let left = left
        .trait_arguments
        .iter()
        .chain([&left.target])
        .cloned()
        .collect::<Vec<_>>();
    let right = right
        .trait_arguments
        .iter()
        .chain([&right.target])
        .cloned()
        .collect::<Vec<_>>();
    unify_pattern_lists(&left, &right)
}

pub(super) fn classify_normalized(ty: TypeId, symbolic: bool) -> TraitNormalization {
    if symbolic {
        TraitNormalization::Symbolic(ty)
    } else {
        TraitNormalization::Normalized(ty)
    }
}
