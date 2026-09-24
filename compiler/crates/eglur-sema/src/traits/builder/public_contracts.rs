use std::collections::{BTreeSet, VecDeque};

use eglur_hir::{EnumVariantPayload, GenericParameter, HirFile, ItemKind, OriginId, TypeRefId};

use super::*;
use crate::ResolvedType;
use crate::diagnostic::record_public_contract_visibility_violation;
use crate::signatures::TypeOccurrenceId;
use crate::signatures::public_contracts::{first_unnameable_trait, first_unnameable_type};

enum ContractCheck {
    Nameable,
    Unnameable,
    Poisoned(Poison),
    Unperformed,
}

pub(super) fn validate_public_type_contracts(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    signatures: &mut DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    environment: &mut TraitEnvironment,
) -> Result<bool, TraitEnvironmentError> {
    let source_map = sources
        .iter()
        .map(|source| (source.file(), source))
        .collect::<BTreeMap<_, _>>();
    let dependencies = signatures.application_dependencies(collection);
    let mut positions_by_file = BTreeMap::new();

    'declarations: for declaration in collection
        .modules()
        .flat_map(|module| module.declarations())
    {
        let owner = declaration.id();
        if !collection.has_externally_reachable_type_contract(owner) {
            continue;
        }
        let target = CanonicalTarget::Source(owner);
        match signatures.declaration(&target) {
            FactState::Known(_) => {}
            FactState::Poisoned(_) | FactState::NotApplicable => continue,
            FactState::Unperformed => return Ok(false),
        }

        match signatures.type_parameters(&target) {
            FactState::Known(parameters) => {
                for parameter in parameters.iter() {
                    match signatures.generic_parameter(parameter) {
                        FactState::Known(_) => {}
                        FactState::Poisoned(_) => continue 'declarations,
                        FactState::Unperformed => return Ok(false),
                        FactState::NotApplicable => {
                            return Err(TraitEnvironmentError::UnavailableGenericParameter(
                                parameter.clone(),
                            ));
                        }
                    }
                }
            }
            FactState::Poisoned(_) => continue,
            FactState::Unperformed => return Ok(false),
            FactState::NotApplicable => {}
        }
        let module = collection
            .module(owner.module())
            .ok_or(TraitEnvironmentError::UnknownModule(owner.module()))?;
        let source = source_map
            .get(&module.file())
            .copied()
            .ok_or(TraitEnvironmentError::MissingSource(module.file()))?;
        let hir = source
            .hir()
            .ok_or(TraitEnvironmentError::MissingHir(module.file()))?;
        let positions = positions_by_file
            .entry(source.file())
            .or_insert_with(|| contract_positions(hir))
            .get(&declaration.origin())
            .ok_or(TraitEnvironmentError::WrongDeclarationItem(owner))?;

        for &type_ref in positions {
            let (origin, resolved) =
                match signatures.occurrence(TypeOccurrenceId::new(source.file(), type_ref)) {
                    FactState::Known(occurrence) => {
                        (occurrence.origin(), occurrence.resolved().clone())
                    }
                    FactState::Poisoned(_) => continue 'declarations,
                    FactState::Unperformed | FactState::NotApplicable => return Ok(false),
                };
            let check = match resolved {
                ResolvedType::Type(ty) => {
                    check_type(collection, signatures, environment, owner, ty)?
                }
                ResolvedType::Trait(trait_ref) => {
                    check_trait(collection, signatures, environment, owner, &trait_ref)?
                }
            };
            let poison = match check {
                ContractCheck::Nameable => continue,
                ContractCheck::Unnameable => Some(record_public_contract_visibility_violation(
                    diagnostics,
                    source.origin_span(origin).cloned().ok_or(
                        TraitEnvironmentError::MissingOriginSpan {
                            file: source.file(),
                            origin,
                        },
                    )?,
                )?),
                ContractCheck::Poisoned(poison) => Some(poison),
                ContractCheck::Unperformed => return Ok(false),
            };
            if let Some(poison) = poison {
                poison_dependents(signatures, &dependencies, target.clone(), poison);
                if !applications::synchronize_implementations(signatures, environment) {
                    environment.clear_query_caches();
                }
            }
            break;
        }
    }
    Ok(true)
}

fn contract_positions(hir: &HirFile) -> BTreeMap<OriginId, Vec<TypeRefId>> {
    let mut by_origin = BTreeMap::new();
    for (_, item) in hir.items() {
        let mut positions = Vec::new();
        match item.kind() {
            ItemKind::Function(value) => {
                append_bounds(&mut positions, &value.generic_parameters);
                positions.extend(value.parameters.iter().filter_map(|parameter| parameter.ty));
                positions.extend(value.return_type);
            }
            ItemKind::Extern(value) => {
                positions.extend(value.parameters.iter().filter_map(|parameter| parameter.ty));
                positions.extend(value.return_type);
            }
            ItemKind::Trait(value) => append_bounds(&mut positions, &value.generic_parameters),
            ItemKind::Struct(value) => append_bounds(&mut positions, &value.generic_parameters),
            ItemKind::Enum(value) => {
                append_bounds(&mut positions, &value.generic_parameters);
                for variant in &value.variants {
                    let variant_positions = match &variant.payload {
                        EnumVariantPayload::Absent => Vec::new(),
                        EnumVariantPayload::Tuple(types) => types.to_vec(),
                        EnumVariantPayload::Named(fields) => {
                            for field in fields.iter() {
                                by_origin.insert(field.origin, vec![field.ty]);
                            }
                            Vec::new()
                        }
                    };
                    by_origin.insert(variant.origin, variant_positions);
                }
            }
            ItemKind::TypeAlias(value) => {
                append_bounds(&mut positions, &value.generic_parameters);
                positions.push(value.value);
            }
            ItemKind::Const(value) => positions.push(value.ty),
            ItemKind::StorageField(value) => positions.push(value.ty),
            ItemKind::Property(value) => positions.push(value.ty),
            _ => {}
        }
        by_origin.insert(item.origin(), positions);
    }
    by_origin
}

fn append_bounds(positions: &mut Vec<TypeRefId>, parameters: &[GenericParameter]) {
    for parameter in parameters {
        positions.extend(parameter.bounds.iter().copied());
    }
}

fn check_type(
    collection: &DeclarationCollection,
    signatures: &mut DeclarationSignatures,
    environment: &mut TraitEnvironment,
    owner: DefId,
    root: TypeId,
) -> Result<ContractCheck, TraitEnvironmentError> {
    match environment.normalize_for_owner(owner, signatures, root)? {
        TraitNormalization::Normalized(ty) | TraitNormalization::Symbolic(ty) => Ok(
            if first_unnameable_type(collection, signatures.types(), ty)?.is_some() {
                ContractCheck::Unnameable
            } else {
                ContractCheck::Nameable
            },
        ),
        TraitNormalization::Poisoned(poison) => Ok(ContractCheck::Poisoned(poison)),
        TraitNormalization::InvalidArgument(_)
        | TraitNormalization::Unsatisfied(_)
        | TraitNormalization::Ambiguous(_)
        | TraitNormalization::Cyclic(_) => Ok(ContractCheck::Unperformed),
    }
}

fn check_trait(
    collection: &DeclarationCollection,
    signatures: &mut DeclarationSignatures,
    environment: &mut TraitEnvironment,
    owner: DefId,
    root: &TraitRef,
) -> Result<ContractCheck, TraitEnvironmentError> {
    let arguments = signatures
        .types()
        .substitution(root.substitution())?
        .to_vec();
    let mut normalized = Vec::with_capacity(arguments.len());
    for argument in arguments {
        match environment.normalize_for_owner(owner, signatures, argument)? {
            TraitNormalization::Normalized(ty) | TraitNormalization::Symbolic(ty) => {
                normalized.push(ty);
            }
            TraitNormalization::Poisoned(poison) => return Ok(ContractCheck::Poisoned(poison)),
            TraitNormalization::InvalidArgument(_)
            | TraitNormalization::Unsatisfied(_)
            | TraitNormalization::Ambiguous(_)
            | TraitNormalization::Cyclic(_) => return Ok(ContractCheck::Unperformed),
        }
    }
    let substitution = signatures.types_mut().intern_substitution(normalized)?;
    let trait_ref = TraitRef::new(root.declaration().clone(), substitution);
    Ok(
        if first_unnameable_trait(collection, signatures.types(), trait_ref)?.is_some() {
            ContractCheck::Unnameable
        } else {
            ContractCheck::Nameable
        },
    )
}

fn poison_dependents(
    signatures: &mut DeclarationSignatures,
    dependencies: &BTreeMap<CanonicalTarget, BTreeSet<CanonicalTarget>>,
    owner: CanonicalTarget,
    poison: Poison,
) {
    let mut pending = VecDeque::from([owner]);
    while let Some(declaration) = pending.pop_front() {
        if signatures.poison_public_contract(declaration.clone(), poison.clone()) {
            pending.extend(
                dependencies
                    .get(&declaration)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
    }
}
