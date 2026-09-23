use std::collections::BTreeMap;

use crate::traits::{CheckedMethodBound, checked_method_bound_mapping};
use crate::{
    CallableSignature, CanonicalTarget, DeclarationCollection, DeclarationSignature,
    DeclarationSignatures, DefId, FactState, GenericParameterId, TraitEnvironment,
    TraitEnvironmentError, TraitObligation, TraitRef,
};

use super::{MethodFrameBinding, MethodFrameRoute, MethodFrameSource, TraitEvidenceError};

pub(super) fn build_method_routes(
    collection: &DeclarationCollection,
    signatures: &mut DeclarationSignatures,
    environment: &mut TraitEnvironment,
) -> Result<BTreeMap<(CanonicalTarget, CanonicalTarget), MethodFrameRoute>, TraitEvidenceError> {
    let targets = environment
        .method_targets()
        .map(|(implementation, method, target)| {
            (implementation.clone(), method.clone(), target.clone())
        })
        .collect::<Vec<_>>();
    let mut routes = BTreeMap::new();
    for (implementation, method, target) in targets {
        let CanonicalTarget::Source(target_id) = target else {
            continue;
        };
        let Some(parent) = collection
            .declaration(target_id)
            .and_then(crate::Declaration::parent)
        else {
            return Err(TraitEvidenceError::InvalidCheckedProof("method parent"));
        };
        let trait_signature = callable(signatures, &method)?;
        let mut bindings = Vec::new();

        if target == method {
            if CanonicalTarget::Source(parent)
                != *signatures.trait_method_owner(&method).ok_or(
                    TraitEvidenceError::InvalidCheckedProof("trait method owner"),
                )?
            {
                return Err(TraitEvidenceError::InvalidCheckedProof(
                    "default method parent",
                ));
            }
            let trait_parameters = trait_parameters(signatures, &CanonicalTarget::Source(parent))?;
            let arguments = trait_parameters
                .iter()
                .cloned()
                .map(|parameter| signatures.types_mut().generic_parameter(parameter))
                .collect::<Result<Vec<_>, _>>()
                .map_err(TraitEnvironmentError::from)?;
            let substitution = signatures
                .types_mut()
                .intern_substitution(arguments)
                .map_err(TraitEnvironmentError::from)?;
            let self_type = signatures
                .types_mut()
                .self_type(CanonicalTarget::Source(parent))
                .map_err(TraitEnvironmentError::from)?;
            let obligation = TraitObligation::new(
                self_type,
                TraitRef::new(CanonicalTarget::Source(parent), substitution),
            );
            bind(
                environment,
                target_id,
                &obligation,
                MethodFrameSource::Root,
                &mut bindings,
            )?;
            parameter_bounds(
                signatures,
                environment,
                target_id,
                &trait_parameters,
                MethodFrameSource::TraitBound,
                None,
                &mut bindings,
            )?;
            method_bounds(
                signatures,
                environment,
                target_id,
                &trait_signature,
                &mut bindings,
            )?;
        } else {
            if CanonicalTarget::Source(parent) != implementation {
                return Err(TraitEvidenceError::InvalidCheckedProof(
                    "override method parent",
                ));
            }
            let (header, _) = environment
                .checked_implementation_contract(&implementation)
                .ok_or_else(|| {
                    TraitEvidenceError::UnknownCheckedImplementation(implementation.clone())
                })?;
            let header = header.clone();
            let current_impl = TraitObligation::new(header.target(), header.trait_ref().clone());
            parameter_bounds(
                signatures,
                environment,
                target_id,
                header.generic_parameters(),
                MethodFrameSource::ImplementationBound,
                Some(&current_impl),
                &mut bindings,
            )?;
            let actual = callable(signatures, &CanonicalTarget::Source(target_id))?;
            let mapped = checked_method_bound_mapping(
                environment,
                signatures,
                &implementation,
                &header,
                &trait_signature,
                &actual,
            )?
            .ok_or(TraitEvidenceError::InvalidCheckedProof(
                "override method bound mapping",
            ))?;
            for CheckedMethodBound {
                parameter,
                bound_index,
                obligation,
            } in mapped
            {
                bind(
                    environment,
                    target_id,
                    &obligation,
                    MethodFrameSource::MethodBound {
                        parameter,
                        bound_index,
                    },
                    &mut bindings,
                )?;
            }
        }

        bindings.sort();
        bindings.dedup();
        routes.insert(
            (implementation, method),
            MethodFrameRoute {
                target: target_id,
                bindings: bindings.into_boxed_slice(),
            },
        );
    }
    Ok(routes)
}

fn callable(
    signatures: &DeclarationSignatures,
    target: &CanonicalTarget,
) -> Result<CallableSignature, TraitEvidenceError> {
    let FactState::Known(DeclarationSignature::Callable(id)) = signatures.declaration(target)
    else {
        return Err(TraitEvidenceError::InvalidCheckedProof("method signature"));
    };
    signatures
        .callable(*id)
        .cloned()
        .ok_or(TraitEvidenceError::InvalidCheckedProof(
            "callable signature",
        ))
}

fn trait_parameters(
    signatures: &DeclarationSignatures,
    owner: &CanonicalTarget,
) -> Result<Vec<GenericParameterId>, TraitEvidenceError> {
    match signatures.declaration(owner) {
        FactState::Known(DeclarationSignature::Trait {
            generic_parameters, ..
        }) => Ok(generic_parameters.to_vec()),
        _ => Err(TraitEvidenceError::InvalidCheckedProof("trait signature")),
    }
}

fn parameter_bounds(
    signatures: &mut DeclarationSignatures,
    environment: &TraitEnvironment,
    target: DefId,
    parameters: &[GenericParameterId],
    source: impl Fn(usize) -> MethodFrameSource,
    current_impl: Option<&TraitObligation>,
    bindings: &mut Vec<MethodFrameBinding>,
) -> Result<(), TraitEvidenceError> {
    let mut ordinal = 0;
    for parameter in parameters {
        let FactState::Known(signature) = signatures.generic_parameter(parameter) else {
            return Err(TraitEvidenceError::InvalidCheckedProof("generic signature"));
        };
        let bounds = signature.declared_bounds().to_vec();
        let subject = signatures
            .types_mut()
            .generic_parameter(parameter.clone())
            .map_err(TraitEnvironmentError::from)?;
        for bound in bounds {
            let obligation = TraitObligation::new(subject, bound);
            if current_impl == Some(&obligation)
                && environment
                    .declared_witness_for_obligation(target, &obligation)
                    .is_none()
            {
                // The current impl assumption subsumes this declared bound without a slot.
                ordinal += 1;
                continue;
            }
            bind(environment, target, &obligation, source(ordinal), bindings)?;
            ordinal += 1;
        }
    }
    Ok(())
}

fn method_bounds(
    signatures: &mut DeclarationSignatures,
    environment: &TraitEnvironment,
    target: DefId,
    method: &CallableSignature,
    bindings: &mut Vec<MethodFrameBinding>,
) -> Result<(), TraitEvidenceError> {
    for parameter in method.generic_parameters() {
        let FactState::Known(signature) = signatures.generic_parameter(parameter) else {
            return Err(TraitEvidenceError::InvalidCheckedProof(
                "method generic signature",
            ));
        };
        let bounds = signature.declared_bounds().to_vec();
        let subject = signatures
            .types_mut()
            .generic_parameter(parameter.clone())
            .map_err(TraitEnvironmentError::from)?;
        for (bound_index, bound) in bounds.into_iter().enumerate() {
            bind(
                environment,
                target,
                &TraitObligation::new(subject, bound),
                MethodFrameSource::MethodBound {
                    parameter: parameter.clone(),
                    bound_index,
                },
                bindings,
            )?;
        }
    }
    Ok(())
}

fn bind(
    environment: &TraitEnvironment,
    owner: DefId,
    obligation: &TraitObligation,
    source: MethodFrameSource,
    bindings: &mut Vec<MethodFrameBinding>,
) -> Result<(), TraitEvidenceError> {
    let destination = environment
        .declared_witness_for_obligation(owner, obligation)
        .ok_or(TraitEvidenceError::InvalidCheckedProof(
            "method witness slot",
        ))?;
    bindings.push(MethodFrameBinding {
        destination,
        source,
    });
    Ok(())
}
