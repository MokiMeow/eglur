use std::collections::BTreeMap;

use crate::fingerprint::identity::{BinderContext, DeclarationBinderContexts, write_name};
use crate::{
    AssociatedBindingSignature, CallableRequirement, CanonicalFingerprintBuilder, CanonicalTarget,
    ConstructorPayloadSignature, Declaration, DeclarationCollection, DeclarationKind,
    DeclarationSignature, DeclarationSignatures, FactState, FingerprintDomain, GenericParameterId,
    InterfaceFingerprint, ModuleAddress, Namespace, ResolvedImports, StableDeclarationKeys,
    StableTargetKey, TraitImplHeader, TraitRef, TypeId,
};

use super::{BodyProjection, SemanticFingerprintError};

pub(super) fn compute(
    declarations: &DeclarationCollection,
    imports: &ResolvedImports,
    signatures: &DeclarationSignatures,
    bodies: &BodyProjection,
    keys: &StableDeclarationKeys,
    binder_contexts: &mut DeclarationBinderContexts<'_>,
) -> Result<BTreeMap<ModuleAddress, InterfaceFingerprint>, SemanticFingerprintError> {
    let mut writer = InterfaceWriter {
        declarations,
        imports,
        signatures,
        bodies,
        keys,
        binder_contexts,
    };
    let mut reachable = BTreeMap::new();
    declarations
        .modules()
        .map(|module| {
            let mut entries = Vec::new();
            for declaration in module.declarations() {
                if interface_reachable(declaration.id(), declarations, signatures, &mut reachable)?
                {
                    let key = keys.source(declaration.id()).ok_or(
                        SemanticFingerprintError::UnknownStableDeclaration(declaration.id()),
                    )?;
                    entries.push((key, declaration));
                }
            }
            entries.sort_by(|left, right| left.0.cmp(right.0));

            let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Interface);
            builder.write_str(module.address().as_str());
            builder.write_len(entries.len());
            for (_, declaration) in entries {
                writer.write_declaration_contract(&mut builder, declaration)?;
            }
            writer.write_reexports(&mut builder, module)?;
            Ok((
                module.address().clone(),
                InterfaceFingerprint::new(builder.finish()),
            ))
        })
        .collect()
}

struct StableReexport {
    name: crate::NameKey,
    namespace: Namespace,
    target: StableTargetKey,
    source: Option<crate::DefId>,
}

pub(super) fn interface_reachable(
    declaration: crate::DefId,
    declarations: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    memo: &mut BTreeMap<crate::DefId, bool>,
) -> Result<bool, SemanticFingerprintError> {
    if let Some(&reachable) = memo.get(&declaration) {
        return Ok(reachable);
    }
    let declaration_fact = declarations
        .declaration(declaration)
        .ok_or(SemanticFingerprintError::UnknownDeclaration(declaration))?;
    let reachable = if let Some(parent) = declaration_fact.parent() {
        interface_reachable(parent, declarations, signatures, memo)?
            && child_is_externally_reachable(declaration_fact, parent, declarations)?
    } else if declaration_fact.kind() == DeclarationKind::Impl {
        impl_is_externally_reachable(declaration, declarations, signatures)?
    } else {
        declaration_fact.visibility().is_public()
    };
    memo.insert(declaration, reachable);
    Ok(reachable)
}

pub(super) fn child_is_externally_reachable(
    declaration: &Declaration,
    parent: crate::DefId,
    declarations: &DeclarationCollection,
) -> Result<bool, SemanticFingerprintError> {
    let parent = declarations
        .declaration(parent)
        .ok_or(SemanticFingerprintError::UnknownDeclaration(parent))?;
    Ok(match parent.kind() {
        DeclarationKind::Trait
        | DeclarationKind::Impl
        | DeclarationKind::Enum
        | DeclarationKind::EnumVariant
        | DeclarationKind::Property => true,
        DeclarationKind::Struct => {
            declaration.kind() == DeclarationKind::GenericParameter
                || declaration.visibility().is_public()
        }
        DeclarationKind::Function
        | DeclarationKind::ExternFunction
        | DeclarationKind::IntrinsicConstructor
        | DeclarationKind::OpaqueType
        | DeclarationKind::TypeAlias => declaration.kind() == DeclarationKind::GenericParameter,
        DeclarationKind::Const
        | DeclarationKind::PrimitiveType
        | DeclarationKind::AssociatedType
        | DeclarationKind::AssociatedTypeBinding
        | DeclarationKind::EnumVariantField
        | DeclarationKind::StorageField
        | DeclarationKind::GenericParameter => declaration.visibility().is_public(),
    })
}

pub(super) fn impl_is_externally_reachable(
    declaration: crate::DefId,
    declarations: &DeclarationCollection,
    signatures: &DeclarationSignatures,
) -> Result<bool, SemanticFingerprintError> {
    let FactState::Known(header) = signatures.impl_header(&CanonicalTarget::Source(declaration))
    else {
        return Err(SemanticFingerprintError::IncompleteSignature(declaration));
    };
    Ok(
        crate::signatures::public_contracts::conformance_is_externally_reachable(
            declarations,
            signatures.types(),
            header,
        )?,
    )
}

pub(super) fn write_impl_header(
    builder: &mut CanonicalFingerprintBuilder,
    header: &TraitImplHeader,
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BinderContext,
) -> Result<(), SemanticFingerprintError> {
    write_generic_contract(
        builder,
        header.generic_parameters(),
        signatures,
        keys,
        binders,
    )?;
    keys.stable_trait_ref(signatures, header.trait_ref(), binders)?
        .write_to(builder);
    keys.stable_type(signatures, header.target(), binders)?
        .write_to(builder);
    Ok(())
}

pub(super) fn write_associated_binding(
    builder: &mut CanonicalFingerprintBuilder,
    binding: &AssociatedBindingSignature,
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BinderContext,
) -> Result<(), SemanticFingerprintError> {
    keys.target(binding.declaration())
        .ok_or_else(|| target_error(binding.declaration()))?
        .write_to(builder);
    keys.target(binding.associated())
        .ok_or_else(|| target_error(binding.associated()))?
        .write_to(builder);
    keys.stable_type(signatures, binding.value(), binders)?
        .write_to(builder);
    Ok(())
}

fn write_generic_contract(
    builder: &mut CanonicalFingerprintBuilder,
    parameters: &[GenericParameterId],
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BinderContext,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(parameters.len());
    for parameter in parameters {
        let FactState::Known(parameter) = signatures.generic_parameter(parameter) else {
            return Err(SemanticFingerprintError::IncompleteGenericParameter(
                parameter.clone(),
            ));
        };
        write_trait_refs(builder, parameter.bounds(), signatures, keys, binders)?;
    }
    Ok(())
}

fn write_trait_refs(
    builder: &mut CanonicalFingerprintBuilder,
    trait_refs: &[TraitRef],
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BinderContext,
) -> Result<(), SemanticFingerprintError> {
    let mut stable = trait_refs
        .iter()
        .map(|trait_ref| keys.stable_trait_ref(signatures, trait_ref, binders))
        .collect::<Result<Vec<_>, _>>()?;
    stable.sort();
    stable.dedup();
    builder.write_len(stable.len());
    for trait_ref in stable {
        trait_ref.write_to(builder);
    }
    Ok(())
}

fn write_types(
    builder: &mut CanonicalFingerprintBuilder,
    types: &[TypeId],
    signatures: &DeclarationSignatures,
    keys: &StableDeclarationKeys,
    binders: &BinderContext,
) -> Result<(), SemanticFingerprintError> {
    builder.write_len(types.len());
    for &ty in types {
        keys.stable_type(signatures, ty, binders)?.write_to(builder);
    }
    Ok(())
}

fn write_targets(
    builder: &mut CanonicalFingerprintBuilder,
    targets: &[CanonicalTarget],
    keys: &StableDeclarationKeys,
) -> Result<(), SemanticFingerprintError> {
    let mut stable = targets
        .iter()
        .map(|target| keys.target(target).ok_or_else(|| target_error(target)))
        .collect::<Result<Vec<_>, _>>()?;
    stable.sort();
    stable.dedup();
    builder.write_len(stable.len());
    for target in stable {
        target.write_to(builder);
    }
    Ok(())
}

fn write_public_targets(
    builder: &mut CanonicalFingerprintBuilder,
    targets: &[CanonicalTarget],
    declarations: &DeclarationCollection,
    keys: &StableDeclarationKeys,
) -> Result<(), SemanticFingerprintError> {
    let public = targets
        .iter()
        .filter(|target| {
            target_declaration(target, declarations)
                .is_some_and(|declaration| declaration.visibility().is_public())
        })
        .cloned()
        .collect::<Vec<_>>();
    write_targets(builder, &public, keys)
}

fn target_declaration<'a>(
    target: &CanonicalTarget,
    declarations: &'a DeclarationCollection,
) -> Option<&'a Declaration> {
    let CanonicalTarget::Source(declaration) = target else {
        return None;
    };
    declarations.declaration(*declaration)
}

pub(super) fn target_error(target: &CanonicalTarget) -> SemanticFingerprintError {
    match target {
        CanonicalTarget::Source(declaration) => {
            SemanticFingerprintError::UnknownStableDeclaration(*declaration)
        }
        CanonicalTarget::External(_) | CanonicalTarget::Primitive(_) => {
            unreachable!("non-source targets are stable")
        }
    }
}

struct InterfaceWriter<'a, 'snapshot> {
    declarations: &'a DeclarationCollection,
    imports: &'a ResolvedImports,
    signatures: &'a DeclarationSignatures,
    bodies: &'a BodyProjection,
    keys: &'a StableDeclarationKeys,
    binder_contexts: &'a mut DeclarationBinderContexts<'snapshot>,
}

impl InterfaceWriter<'_, '_> {
    fn write_declaration_contract(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        declaration: &Declaration,
    ) -> Result<(), SemanticFingerprintError> {
        let keys = self.keys;
        let bodies = self.bodies;

        keys.source(declaration.id())
            .ok_or(SemanticFingerprintError::UnknownStableDeclaration(
                declaration.id(),
            ))?
            .write_to(builder);
        builder.write_declaration_kind(declaration.kind());
        builder.write_bool(declaration.visibility().is_public());
        self.write_declaration_signature(builder, declaration)?;
        if declaration.kind() == DeclarationKind::Const {
            let digest = bodies
                .declaration(declaration.id())
                .ok_or(SemanticFingerprintError::MissingBody(declaration.id()))?;
            builder.write_digest(digest);
        }
        Ok(())
    }

    fn write_reexports(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        module: &crate::ModuleDeclarations,
    ) -> Result<(), SemanticFingerprintError> {
        let declarations = self.declarations;
        let imports = self.imports;
        let keys = self.keys;

        let mut reexports = Vec::new();
        for import in module.imports() {
            let FactState::Known(bindings) = imports.fact(import.id()) else {
                return Err(SemanticFingerprintError::IncompleteImport(import.id()));
            };
            for binding in bindings.iter().filter(|binding| binding.is_public()) {
                for target in binding.targets() {
                    reexports.push(StableReexport {
                        name: binding.local_name().clone(),
                        namespace: binding.namespace(),
                        target: keys.target(target).ok_or_else(|| target_error(target))?,
                        source: match target {
                            CanonicalTarget::Source(declaration) => Some(*declaration),
                            CanonicalTarget::External(_) | CanonicalTarget::Primitive(_) => None,
                        },
                    });
                }
            }
        }
        reexports.sort_by(|left, right| {
            (&left.name, left.namespace, &left.target).cmp(&(
                &right.name,
                right.namespace,
                &right.target,
            ))
        });
        reexports.dedup_by(|left, right| {
            left.name == right.name
                && left.namespace == right.namespace
                && left.target == right.target
        });

        builder.write_len(reexports.len());
        for reexport in reexports {
            write_name(builder, &reexport.name);
            builder.write_u8(match reexport.namespace {
                Namespace::Value => 0,
                Namespace::Type => 1,
            });
            reexport.target.write_to(builder);
            builder.write_bool(reexport.source.is_some());
            if let Some(source) = reexport.source {
                let declaration = declarations
                    .declaration(source)
                    .ok_or(SemanticFingerprintError::UnknownDeclaration(source))?;
                self.write_declaration_family_contract(builder, declaration)?;
            }
        }
        Ok(())
    }

    fn write_declaration_family_contract(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        declaration: &Declaration,
    ) -> Result<(), SemanticFingerprintError> {
        let declarations = self.declarations;
        let keys = self.keys;

        self.write_declaration_contract(builder, declaration)?;
        let module = declarations.module(declaration.id().module()).ok_or(
            SemanticFingerprintError::UnknownDeclaration(declaration.id()),
        )?;
        let mut children = module
            .children(declaration.id())
            .map(|child| {
                let reachable =
                    child_is_externally_reachable(child, declaration.id(), declarations)?;
                let key = keys.source(child.id()).ok_or(
                    SemanticFingerprintError::UnknownStableDeclaration(child.id()),
                )?;
                Ok((reachable, key, child))
            })
            .collect::<Result<Vec<_>, SemanticFingerprintError>>()?;
        children.retain(|(reachable, _, _)| *reachable);
        children.sort_by(|left, right| left.1.cmp(right.1));
        builder.write_len(children.len());
        for (_, _, child) in children {
            self.write_declaration_family_contract(builder, child)?;
        }
        Ok(())
    }

    fn write_declaration_signature(
        &mut self,
        builder: &mut CanonicalFingerprintBuilder,
        declaration: &Declaration,
    ) -> Result<(), SemanticFingerprintError> {
        let declarations = self.declarations;
        let signatures = self.signatures;
        let keys = self.keys;

        let FactState::Known(signature) = signatures.source_declaration(declaration.id()) else {
            return Err(SemanticFingerprintError::IncompleteSignature(
                declaration.id(),
            ));
        };
        let binders = self.binder_contexts.context(declaration.id())?;
        match signature {
            DeclarationSignature::Callable(id) => {
                builder.write_u8(0);
                let signature = signatures.callable(*id).ok_or(
                    SemanticFingerprintError::IncompleteSignature(declaration.id()),
                )?;
                write_generic_contract(
                    builder,
                    signature.generic_parameters(),
                    signatures,
                    keys,
                    &binders,
                )?;
                builder.write_bool(signature.is_bang());
                builder.write_len(signature.parameters().len());
                for parameter in signature.parameters() {
                    builder.write_bool(parameter.name().is_some());
                    if let Some(name) = parameter.name() {
                        builder.write_str(name);
                    }
                    keys.stable_type(signatures, parameter.ty(), &binders)?
                        .write_to(builder);
                }
                keys.stable_type(signatures, signature.result(), &binders)?
                    .write_to(builder);
                builder.write_len(signature.requirements().len());
                for requirement in signature.requirements() {
                    match requirement {
                        CallableRequirement::FixedWidthInteger(ty) => {
                            builder.write_u8(0);
                            keys.stable_type(signatures, *ty, &binders)?
                                .write_to(builder);
                        }
                    }
                }
            }
            DeclarationSignature::Constructor(signature) => {
                builder.write_u8(1);
                write_generic_contract(
                    builder,
                    signature.generic_parameters(),
                    signatures,
                    keys,
                    &binders,
                )?;
                match signature.payload() {
                    ConstructorPayloadSignature::Unit => builder.write_u8(0),
                    ConstructorPayloadSignature::Positional(types) => {
                        builder.write_u8(1);
                        write_types(builder, types, signatures, keys, &binders)?;
                    }
                }
                keys.stable_type(signatures, signature.result(), &binders)?
                    .write_to(builder);
            }
            DeclarationSignature::Trait {
                closed,
                generic_parameters,
                associated_types,
            } => {
                builder.write_u8(2);
                write_generic_contract(builder, generic_parameters, signatures, keys, &binders)?;
                builder.write_bool(*closed);
                write_targets(builder, associated_types, keys)?;
            }
            DeclarationSignature::Impl {
                associated_bindings,
            } => {
                builder.write_u8(3);
                let FactState::Known(header) =
                    signatures.impl_header(&CanonicalTarget::Source(declaration.id()))
                else {
                    return Err(SemanticFingerprintError::IncompleteSignature(
                        declaration.id(),
                    ));
                };
                write_impl_header(builder, header, signatures, keys, &binders)?;
                let mut bindings = associated_bindings
                    .iter()
                    .map(|binding| {
                        keys.target(binding.declaration())
                            .ok_or_else(|| target_error(binding.declaration()))
                            .map(|key| (key, binding))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                bindings.sort_by(|left, right| left.0.cmp(&right.0));
                builder.write_len(bindings.len());
                for (_, binding) in bindings {
                    write_associated_binding(builder, binding, signatures, keys, &binders)?;
                }
            }
            DeclarationSignature::Struct {
                generic_parameters,
                storage_fields,
                properties,
            } => {
                builder.write_u8(4);
                write_generic_contract(builder, generic_parameters, signatures, keys, &binders)?;
                builder.write_bool(storage_fields.iter().all(|field| {
                    target_declaration(field, declarations)
                        .is_some_and(|field| field.visibility().is_public())
                }));
                write_public_targets(builder, storage_fields, declarations, keys)?;
                write_public_targets(builder, properties, declarations, keys)?;
            }
            DeclarationSignature::Nominal { generic_parameters } => {
                builder.write_u8(5);
                write_generic_contract(builder, generic_parameters, signatures, keys, &binders)?;
            }
            DeclarationSignature::Enum {
                generic_parameters,
                variants,
            } => {
                builder.write_u8(6);
                write_generic_contract(builder, generic_parameters, signatures, keys, &binders)?;
                write_targets(builder, variants, keys)?;
            }
            DeclarationSignature::EnumVariant {
                owner,
                name,
                payload,
            } => {
                builder.write_u8(7);
                keys.target(owner)
                    .ok_or_else(|| target_error(owner))?
                    .write_to(builder);
                write_name(builder, name);
                match payload {
                    crate::EnumVariantPayloadSignature::Absent => builder.write_u8(0),
                    crate::EnumVariantPayloadSignature::Tuple(types) => {
                        builder.write_u8(1);
                        write_types(builder, types, signatures, keys, &binders)?;
                    }
                    crate::EnumVariantPayloadSignature::Named(fields) => {
                        builder.write_u8(2);
                        write_targets(builder, fields, keys)?;
                    }
                }
            }
            DeclarationSignature::Field { name, ty } => {
                builder.write_u8(8);
                write_name(builder, name);
                keys.stable_type(signatures, *ty, &binders)?
                    .write_to(builder);
            }
            DeclarationSignature::TypeAlias {
                generic_parameters,
                value,
            } => {
                builder.write_u8(9);
                write_generic_contract(builder, generic_parameters, signatures, keys, &binders)?;
                keys.stable_type(signatures, *value, &binders)?
                    .write_to(builder);
            }
            DeclarationSignature::Const { ty } => {
                builder.write_u8(10);
                keys.stable_type(signatures, *ty, &binders)?
                    .write_to(builder);
            }
            DeclarationSignature::AssociatedType { owner_trait } => {
                builder.write_u8(11);
                keys.target(owner_trait)
                    .ok_or_else(|| target_error(owner_trait))?
                    .write_to(builder);
            }
            DeclarationSignature::AssociatedTypeBinding(binding) => {
                builder.write_u8(12);
                write_associated_binding(builder, binding, signatures, keys, &binders)?;
            }
            DeclarationSignature::Property { ty } => {
                builder.write_u8(13);
                keys.stable_type(signatures, *ty, &binders)?
                    .write_to(builder);
            }
            DeclarationSignature::GenericParameter(parameter) => {
                builder.write_u8(14);
                let index = binders.ordinal(parameter.id()).ok_or(
                    SemanticFingerprintError::UnknownGenericParameter(declaration.id()),
                )?;
                builder.write_u32(index);
                write_trait_refs(builder, parameter.bounds(), signatures, keys, &binders)?;
            }
        }
        Ok(())
    }
}
