use crate::fingerprint::identity::declaration_binder_positions;
use crate::{
    CanonicalFingerprintBuilder, CanonicalTarget, ConformanceComponentFact,
    ConformanceComponentFingerprint, ConformanceEnvironmentFact, ConformanceFingerprint,
    DeclarationCollection, DeclarationKind, DeclarationSignatures, FingerprintDomain,
    StableDeclarationKeys, TraitEnvironment,
};

use super::{SemanticFingerprintError, interface};

pub(super) fn compute(
    declarations: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    trait_environment: &TraitEnvironment,
    keys: &StableDeclarationKeys,
) -> Result<ConformanceEnvironmentFact, SemanticFingerprintError> {
    let mut components = Vec::new();
    for module in declarations.modules() {
        for declaration in module
            .declarations()
            .filter(|declaration| declaration.kind() == DeclarationKind::Impl)
        {
            if !interface::impl_is_externally_reachable(declaration.id(), declarations, signatures)?
            {
                continue;
            }

            let target = CanonicalTarget::Source(declaration.id());
            let Some((header, bindings)) =
                trait_environment.checked_implementation_contract(&target)
            else {
                return Err(SemanticFingerprintError::MissingConformanceImplementation(
                    declaration.id(),
                ));
            };
            let implementation = keys.source(declaration.id()).cloned().ok_or(
                SemanticFingerprintError::UnknownStableDeclaration(declaration.id()),
            )?;
            let binders = declaration_binder_positions(declarations, signatures, declaration.id())?;
            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Conformance);
            builder.write_u8(0);
            implementation.write_to(&mut builder);
            interface::write_impl_header(&mut builder, header, signatures, keys, &binders)?;

            let mut stable_bindings = bindings
                .values()
                .map(|binding| {
                    let key = keys
                        .target(binding.associated())
                        .ok_or_else(|| interface::target_error(binding.associated()))?;
                    Ok((key, binding))
                })
                .collect::<Result<Vec<_>, SemanticFingerprintError>>()?;
            stable_bindings.sort_by(|left, right| left.0.cmp(&right.0));
            builder.write_len(stable_bindings.len());
            for (_, binding) in stable_bindings {
                interface::write_associated_binding(
                    &mut builder,
                    binding,
                    signatures,
                    keys,
                    &binders,
                )?;
            }

            let mut method_targets = trait_environment
                .method_targets()
                .filter(|(candidate, _, _)| *candidate == &target)
                .map(|(_, trait_method, selected)| {
                    let trait_method = keys
                        .target(trait_method)
                        .ok_or_else(|| interface::target_error(trait_method))?;
                    let selected = keys
                        .target(selected)
                        .ok_or_else(|| interface::target_error(selected))?;
                    Ok((trait_method, selected))
                })
                .collect::<Result<Vec<_>, SemanticFingerprintError>>()?;
            method_targets.sort();
            builder.write_len(method_targets.len());
            for (trait_method, selected) in method_targets {
                trait_method.write_to(&mut builder);
                selected.write_to(&mut builder);
            }

            components.push(ConformanceComponentFact {
                implementation,
                fingerprint: ConformanceComponentFingerprint::new(builder.finish()),
            });
        }
    }
    components.sort_by(|left, right| left.implementation.cmp(&right.implementation));

    let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Conformance);
    builder.write_u8(1);
    builder.write_len(components.len());
    for component in &components {
        component.implementation.write_to(&mut builder);
        builder.write_digest(component.fingerprint.digest());
    }
    Ok(ConformanceEnvironmentFact {
        fingerprint: ConformanceFingerprint::new(builder.finish()),
        components: components.into_boxed_slice(),
    })
}
