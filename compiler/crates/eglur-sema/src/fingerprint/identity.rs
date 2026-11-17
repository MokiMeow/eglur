use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{
    CallableSignature, CanonicalFingerprintBuilder, CanonicalTarget, CanonicalTypeStore,
    Declaration, DeclarationCollection, DeclarationKind, DeclarationSignature,
    DeclarationSignatures, DefId, ExternalDefId, FactState, FingerprintDigest, FingerprintDomain,
    GenericParameterId, IntrinsicNominal, ModuleAddress, NameKey, NominalHead,
    RawPointerMutability, ReferenceMutability, TraitRef, TypeId, TypeKind, TypeStoreError,
};

mod binders;
pub(crate) use binders::{BinderContext, DeclarationBinderContexts};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum StableDeclarationSegment {
    Named {
        kind: DeclarationKind,
        name: NameKey,
        overload: Option<FingerprintDigest>,
    },
    Impl(FingerprintDigest),
    GenericParameter(u32),
}

/// Portable source-declaration identity reconstructed without graph-local IDs.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StableDeclarationKey {
    module: ModuleAddress,
    segments: Box<[StableDeclarationSegment]>,
}

impl StableDeclarationKey {
    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    pub(crate) fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        builder.write_str(self.module.as_str());
        builder.write_len(self.segments.len());
        for segment in &self.segments {
            match segment {
                StableDeclarationSegment::Named {
                    kind,
                    name,
                    overload,
                } => {
                    builder.write_u8(0);
                    write_declaration_kind(builder, *kind);
                    write_name(builder, name);
                    builder.write_bool(overload.is_some());
                    if let Some(discriminator) = overload {
                        builder.write_digest(*discriminator);
                    }
                }
                StableDeclarationSegment::Impl(discriminator) => {
                    builder.write_u8(1);
                    builder.write_digest(*discriminator);
                }
                StableDeclarationSegment::GenericParameter(index) => {
                    builder.write_u8(2);
                    builder.write_u32(*index);
                }
            }
        }
    }
}

/// Stable target identity for source and trusted declarations.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StableTargetKey {
    Source(StableDeclarationKey),
    External(ExternalDefId),
}

impl StableTargetKey {
    pub(crate) fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::Source(key) => {
                builder.write_u8(0);
                key.write_to(builder);
            }
            Self::External(key) => {
                builder.write_u8(1);
                builder.write_str(key.as_str());
            }
        }
    }
}

/// Current-session map from graph-local declaration IDs to portable keys.
pub struct StableDeclarationKeys {
    keys: BTreeMap<DefId, StableDeclarationKey>,
}

impl StableDeclarationKeys {
    pub fn derive(
        collection: &DeclarationCollection,
        signatures: &DeclarationSignatures,
    ) -> Result<Self, StableDeclarationError> {
        KeyDeriver::new(collection, signatures).derive()
    }

    #[must_use]
    pub fn source(&self, declaration: DefId) -> Option<&StableDeclarationKey> {
        self.keys.get(&declaration)
    }

    #[must_use]
    pub fn target(&self, target: &CanonicalTarget) -> Option<StableTargetKey> {
        match target {
            CanonicalTarget::Source(declaration) => self
                .source(*declaration)
                .cloned()
                .map(StableTargetKey::Source),
            CanonicalTarget::External(declaration) => {
                Some(StableTargetKey::External(declaration.clone()))
            }
        }
    }

    pub(crate) fn stable_type(
        &self,
        signatures: &DeclarationSignatures,
        ty: TypeId,
        binders: &BinderContext,
    ) -> Result<StableType, StableDeclarationError> {
        StableType::from_type(signatures.types(), self, ty, binders)
    }

    pub(crate) fn stable_trait_ref(
        &self,
        signatures: &DeclarationSignatures,
        trait_ref: &TraitRef,
        binders: &BinderContext,
    ) -> Result<StableTraitRef, StableDeclarationError> {
        let declaration =
            self.target(trait_ref.declaration())
                .ok_or_else(|| match trait_ref.declaration() {
                    CanonicalTarget::Source(declaration) => {
                        StableDeclarationError::UnknownDeclaration(*declaration)
                    }
                    CanonicalTarget::External(_) => unreachable!("external targets are stable"),
                })?;
        let arguments = signatures
            .types()
            .substitution(trait_ref.substitution())?
            .iter()
            .map(|&argument| self.stable_type(signatures, argument, binders))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StableTraitRef {
            declaration,
            arguments: arguments.into_boxed_slice(),
        })
    }
}

struct KeyDeriver<'a> {
    collection: &'a DeclarationCollection,
    signatures: &'a DeclarationSignatures,
    keys: BTreeMap<DefId, StableDeclarationKey>,
    active: BTreeSet<DefId>,
    generic_parameter_indices: BTreeMap<DefId, BTreeMap<DefId, usize>>,
    binder_contexts: DeclarationBinderContexts<'a>,
}

impl<'a> KeyDeriver<'a> {
    fn new(collection: &'a DeclarationCollection, signatures: &'a DeclarationSignatures) -> Self {
        Self {
            collection,
            signatures,
            keys: BTreeMap::new(),
            active: BTreeSet::new(),
            generic_parameter_indices: BTreeMap::new(),
            binder_contexts: DeclarationBinderContexts::new(collection, signatures),
        }
    }

    fn derive(mut self) -> Result<StableDeclarationKeys, StableDeclarationError> {
        for module in self.collection.modules() {
            for declaration in module.declarations() {
                self.derive_one(declaration.id())?;
            }
        }
        Ok(StableDeclarationKeys { keys: self.keys })
    }

    fn derive_one(
        &mut self,
        declaration_id: DefId,
    ) -> Result<StableDeclarationKey, StableDeclarationError> {
        if let Some(key) = self.keys.get(&declaration_id) {
            return Ok(key.clone());
        }
        if !self.active.insert(declaration_id) {
            return Err(StableDeclarationError::CyclicDeclarationPath(
                declaration_id,
            ));
        }
        let declaration = self
            .collection
            .declaration(declaration_id)
            .ok_or(StableDeclarationError::UnknownDeclaration(declaration_id))?;
        let module = self
            .collection
            .module(declaration_id.module())
            .ok_or(StableDeclarationError::UnknownDeclaration(declaration_id))?;
        let mut segments = if let Some(parent) = declaration.parent() {
            let parent_key = self.derive_one(parent)?;
            if parent_key.module != *module.address() {
                return Err(StableDeclarationError::CrossModuleParent {
                    declaration: declaration_id,
                    parent,
                });
            }
            parent_key.segments.into_vec()
        } else {
            Vec::new()
        };
        segments.push(self.segment(declaration)?);
        let key = StableDeclarationKey {
            module: module.address().clone(),
            segments: segments.into_boxed_slice(),
        };
        self.active.remove(&declaration_id);
        self.keys.insert(declaration_id, key.clone());
        Ok(key)
    }

    fn segment(
        &mut self,
        declaration: &Declaration,
    ) -> Result<StableDeclarationSegment, StableDeclarationError> {
        if declaration.kind() == DeclarationKind::Impl {
            return Ok(StableDeclarationSegment::Impl(
                self.impl_discriminator(declaration.id())?,
            ));
        }
        if declaration.kind() == DeclarationKind::GenericParameter {
            let parent =
                declaration
                    .parent()
                    .ok_or(StableDeclarationError::OrphanGenericParameter(
                        declaration.id(),
                    ))?;
            return Ok(StableDeclarationSegment::GenericParameter(
                self.generic_parameter_index(parent, declaration.id())?,
            ));
        }
        let name = declaration
            .name()
            .cloned()
            .ok_or(StableDeclarationError::UnnamedDeclaration(declaration.id()))?;
        let overload = self
            .is_overload_family_member(declaration)
            .then(|| self.overload_discriminator(declaration.id()))
            .transpose()?;
        Ok(StableDeclarationSegment::Named {
            kind: declaration.kind(),
            name,
            overload,
        })
    }

    fn is_overload_family_member(&self, declaration: &Declaration) -> bool {
        declaration.kind() == DeclarationKind::Function
            && self
                .collection
                .module(declaration.id().module())
                .is_some_and(|module| {
                    module
                        .direct_candidates(
                            declaration.parent(),
                            declaration.name(),
                            DeclarationKind::Function,
                        )
                        .len()
                        > 1
                })
    }

    fn overload_discriminator(
        &mut self,
        declaration: DefId,
    ) -> Result<FingerprintDigest, StableDeclarationError> {
        let signature = self.callable_signature(declaration)?;
        let bang = signature.is_bang();
        let binders = self.binder_contexts.context(declaration)?;
        let parameter_types = signature
            .parameters()
            .iter()
            .map(|parameter| parameter.ty())
            .collect::<Vec<_>>();
        let mut builder =
            CanonicalFingerprintBuilder::new(FingerprintDomain::DeclarationDiscriminator);
        builder.write_u8(0);
        builder.write_bool(bang);
        builder.write_len(parameter_types.len());
        for parameter in parameter_types {
            let ty = self.stable_type(parameter, &binders)?;
            ty.write_to(&mut builder);
        }
        Ok(builder.finish())
    }

    fn impl_discriminator(
        &mut self,
        declaration: DefId,
    ) -> Result<FingerprintDigest, StableDeclarationError> {
        let header = match self
            .signatures
            .impl_header(&CanonicalTarget::Source(declaration))
        {
            FactState::Known(header) => header,
            FactState::Unperformed | FactState::NotApplicable | FactState::Poisoned(_) => {
                return Err(StableDeclarationError::IncompleteSignature(declaration));
            }
        };
        let binders = self.binder_contexts.context(declaration)?;
        let trait_ref = self.stable_trait_ref(header.trait_ref(), &binders)?;
        let target = self.stable_type(header.target(), &binders)?;
        let mut builder =
            CanonicalFingerprintBuilder::new(FingerprintDomain::DeclarationDiscriminator);
        builder.write_u8(1);
        trait_ref.write_to(&mut builder);
        target.write_to(&mut builder);
        Ok(builder.finish())
    }

    fn generic_parameter_index(
        &mut self,
        parent: DefId,
        parameter: DefId,
    ) -> Result<u32, StableDeclarationError> {
        #[cfg(test)]
        tests::generic_ordinals::observe_query();

        if let std::collections::btree_map::Entry::Vacant(entry) =
            self.generic_parameter_indices.entry(parent)
        {
            let parameters = direct_generic_parameters(self.signatures, parent)?;
            #[cfg(test)]
            tests::generic_ordinals::observe_owner();
            entry.insert(index_source_generic_parameters(parameters));
        }
        self.generic_parameter_indices[&parent]
            .get(&parameter)
            .copied()
            .and_then(|index| u32::try_from(index).ok())
            .ok_or(StableDeclarationError::UnknownGenericParameter {
                owner: parent,
                parameter,
            })
    }

    fn callable_signature(
        &self,
        declaration: DefId,
    ) -> Result<&'a CallableSignature, StableDeclarationError> {
        let FactState::Known(DeclarationSignature::Callable(signature)) =
            self.signatures.source_declaration(declaration)
        else {
            return Err(StableDeclarationError::IncompleteSignature(declaration));
        };
        self.signatures
            .callable(*signature)
            .ok_or(StableDeclarationError::IncompleteSignature(declaration))
    }

    fn stable_type(
        &mut self,
        ty: TypeId,
        binders: &BinderContext,
    ) -> Result<StableType, StableDeclarationError> {
        StableType::from_type_with(ty, binders, self.signatures.types(), &mut |target| {
            self.stable_target(target)
        })
    }

    fn stable_trait_ref(
        &mut self,
        trait_ref: &TraitRef,
        binders: &BinderContext,
    ) -> Result<StableTraitRef, StableDeclarationError> {
        let declaration = self.stable_target(trait_ref.declaration())?;
        let arguments = self
            .signatures
            .types()
            .substitution(trait_ref.substitution())?
            .iter()
            .map(|&argument| self.stable_type(argument, binders))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StableTraitRef {
            declaration,
            arguments: arguments.into_boxed_slice(),
        })
    }

    fn stable_target(
        &mut self,
        target: &CanonicalTarget,
    ) -> Result<StableTargetKey, StableDeclarationError> {
        match target {
            CanonicalTarget::Source(declaration) => {
                self.derive_one(*declaration).map(StableTargetKey::Source)
            }
            CanonicalTarget::External(declaration) => {
                Ok(StableTargetKey::External(declaration.clone()))
            }
        }
    }
}

fn index_source_generic_parameters(parameters: &[GenericParameterId]) -> BTreeMap<DefId, usize> {
    let mut indices = BTreeMap::new();
    for (index, parameter) in parameters.iter().enumerate() {
        #[cfg(test)]
        tests::generic_ordinals::observe_parameter();

        if let GenericParameterId::Source(parameter) = parameter {
            indices.entry(*parameter).or_insert(index);
        }
    }
    indices
}

pub(crate) fn direct_generic_parameters(
    signatures: &DeclarationSignatures,
    declaration: DefId,
) -> Result<&[GenericParameterId], StableDeclarationError> {
    if let FactState::Known(header) = signatures.impl_header(&CanonicalTarget::Source(declaration))
    {
        return Ok(header.generic_parameters());
    }
    match signatures.source_declaration(declaration) {
        FactState::Known(DeclarationSignature::Callable(id)) => signatures
            .callable(*id)
            .map(CallableSignature::generic_parameters)
            .ok_or(StableDeclarationError::IncompleteSignature(declaration)),
        FactState::Known(DeclarationSignature::Constructor(signature)) => {
            Ok(signature.generic_parameters())
        }
        FactState::Known(DeclarationSignature::Trait {
            generic_parameters, ..
        })
        | FactState::Known(DeclarationSignature::Struct {
            generic_parameters, ..
        })
        | FactState::Known(DeclarationSignature::Nominal { generic_parameters })
        | FactState::Known(DeclarationSignature::Enum {
            generic_parameters, ..
        })
        | FactState::Known(DeclarationSignature::TypeAlias {
            generic_parameters, ..
        }) => Ok(generic_parameters),
        FactState::Known(
            DeclarationSignature::Impl { .. }
            | DeclarationSignature::EnumVariant { .. }
            | DeclarationSignature::Field { .. }
            | DeclarationSignature::Const { .. }
            | DeclarationSignature::AssociatedType { .. }
            | DeclarationSignature::AssociatedTypeBinding(_)
            | DeclarationSignature::Property { .. }
            | DeclarationSignature::GenericParameter(_),
        ) => Ok(&[]),
        FactState::Unperformed | FactState::Poisoned(_) | FactState::NotApplicable => {
            Err(StableDeclarationError::IncompleteSignature(declaration))
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum StableType {
    Never,
    Primitive(crate::PrimitiveType),
    Nominal {
        head: StableNominalHead,
        arguments: Box<[StableType]>,
    },
    GenericParameter(u32),
    SelfType(StableTargetKey),
    Reference {
        mutability: ReferenceMutability,
        referent: Box<StableType>,
    },
    RawPointer {
        mutability: RawPointerMutability,
        referent: Box<StableType>,
    },
    Tuple(Box<[StableType]>),
    Callable {
        parameters: Box<[StableType]>,
        result: Box<StableType>,
    },
    ErrorUnion(Box<[StableType]>),
    AssociatedProjection {
        base: Box<StableType>,
        trait_ref: StableTraitRef,
        associated: StableTargetKey,
    },
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum StableNominalHead {
    Intrinsic(IntrinsicNominal),
    Declaration(StableTargetKey),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct StableTraitRef {
    declaration: StableTargetKey,
    arguments: Box<[StableType]>,
}

impl StableType {
    fn from_type(
        types: &CanonicalTypeStore,
        keys: &StableDeclarationKeys,
        ty: TypeId,
        binders: &BinderContext,
    ) -> Result<Self, StableDeclarationError> {
        Self::from_type_with(ty, binders, types, &mut |target| {
            keys.target(target).ok_or_else(|| match target {
                CanonicalTarget::Source(declaration) => {
                    StableDeclarationError::UnknownDeclaration(*declaration)
                }
                CanonicalTarget::External(_) => unreachable!("external targets are stable"),
            })
        })
    }

    fn from_type_with(
        ty: TypeId,
        binders: &BinderContext,
        types: &CanonicalTypeStore,
        stable_target: &mut impl FnMut(
            &CanonicalTarget,
        ) -> Result<StableTargetKey, StableDeclarationError>,
    ) -> Result<Self, StableDeclarationError> {
        enum Step<'a> {
            Enter(TypeId),
            Nominal {
                head: StableNominalHead,
                start: usize,
            },
            Reference(ReferenceMutability),
            RawPointer(RawPointerMutability),
            Tuple(usize),
            Callable(usize),
            ErrorUnion(usize),
            ProjectionBase(&'a crate::AssociatedProjection),
            ProjectionArguments {
                base: Box<StableType>,
                declaration: StableTargetKey,
                associated: &'a CanonicalTarget,
                start: usize,
            },
        }

        let mut pending = vec![Step::Enter(ty)];
        let mut values = Vec::new();
        while let Some(step) = pending.pop() {
            match step {
                Step::Enter(ty) => match types.kind(ty)? {
                    TypeKind::Never => values.push(Self::Never),
                    TypeKind::Primitive(primitive) => values.push(Self::Primitive(*primitive)),
                    TypeKind::GenericParameter(parameter) => {
                        values.push(Self::GenericParameter(
                            binders.ordinal(parameter).ok_or_else(|| {
                                StableDeclarationError::UnboundGenericParameter(parameter.clone())
                            })?,
                        ));
                    }
                    TypeKind::SelfType(target) => {
                        values.push(Self::SelfType(stable_target(target)?))
                    }
                    TypeKind::Nominal { head, substitution } => {
                        let head = match head {
                            NominalHead::Intrinsic(head) => StableNominalHead::Intrinsic(*head),
                            NominalHead::Declaration(target) => {
                                StableNominalHead::Declaration(stable_target(target)?)
                            }
                        };
                        let arguments = types.substitution(*substitution)?;
                        pending.push(Step::Nominal {
                            head,
                            start: values.len(),
                        });
                        pending.extend(arguments.iter().rev().copied().map(Step::Enter));
                    }
                    TypeKind::Reference {
                        mutability,
                        referent,
                    } => {
                        pending.push(Step::Reference(*mutability));
                        pending.push(Step::Enter(*referent));
                    }
                    TypeKind::RawPointer {
                        mutability,
                        referent,
                    } => {
                        pending.push(Step::RawPointer(*mutability));
                        pending.push(Step::Enter(*referent));
                    }
                    TypeKind::Tuple(elements) => {
                        pending.push(Step::Tuple(values.len()));
                        pending.extend(elements.iter().rev().copied().map(Step::Enter));
                    }
                    TypeKind::Callable { parameters, result } => {
                        pending.push(Step::Callable(values.len()));
                        pending.push(Step::Enter(*result));
                        pending.extend(parameters.iter().rev().copied().map(Step::Enter));
                    }
                    TypeKind::ErrorUnion(members) => {
                        pending.push(Step::ErrorUnion(values.len()));
                        pending.extend(members.iter().rev().copied().map(Step::Enter));
                    }
                    TypeKind::AssociatedProjection(projection) => {
                        pending.push(Step::ProjectionBase(projection));
                        pending.push(Step::Enter(projection.base()));
                    }
                },
                Step::Nominal { head, start } => {
                    let arguments = values.split_off(start).into_boxed_slice();
                    values.push(Self::Nominal { head, arguments });
                }
                Step::Reference(mutability) => {
                    let referent = Box::new(values.pop().expect("reference follows its referent"));
                    values.push(Self::Reference {
                        mutability,
                        referent,
                    });
                }
                Step::RawPointer(mutability) => {
                    let referent = Box::new(values.pop().expect("pointer follows its referent"));
                    values.push(Self::RawPointer {
                        mutability,
                        referent,
                    });
                }
                Step::Tuple(start) => {
                    let elements = values.split_off(start).into_boxed_slice();
                    values.push(Self::Tuple(elements));
                }
                Step::Callable(start) => {
                    let result = Box::new(values.pop().expect("callable follows its result"));
                    let parameters = values.split_off(start).into_boxed_slice();
                    values.push(Self::Callable { parameters, result });
                }
                Step::ErrorUnion(start) => {
                    let mut members = values.split_off(start);
                    members.sort();
                    members.dedup();
                    values.push(Self::ErrorUnion(members.into_boxed_slice()));
                }
                Step::ProjectionBase(projection) => {
                    let base = Box::new(values.pop().expect("projection follows its base"));
                    let declaration = stable_target(projection.trait_ref().declaration())?;
                    let arguments = types.substitution(projection.trait_ref().substitution())?;
                    pending.push(Step::ProjectionArguments {
                        base,
                        declaration,
                        associated: projection.associated(),
                        start: values.len(),
                    });
                    pending.extend(arguments.iter().rev().copied().map(Step::Enter));
                }
                Step::ProjectionArguments {
                    base,
                    declaration,
                    associated,
                    start,
                } => {
                    let arguments = values.split_off(start).into_boxed_slice();
                    values.push(Self::AssociatedProjection {
                        base,
                        trait_ref: StableTraitRef {
                            declaration,
                            arguments,
                        },
                        associated: stable_target(associated)?,
                    });
                }
            }
        }
        Ok(values.pop().expect("type conversion produces its root"))
    }

    pub(crate) fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::Never => builder.write_u8(0),
            Self::Primitive(primitive) => {
                builder.write_u8(1);
                builder.write_str(primitive.spelling());
            }
            Self::Nominal { head, arguments } => {
                builder.write_u8(2);
                head.write_to(builder);
                write_sequence(builder, arguments, Self::write_to);
            }
            Self::GenericParameter(index) => {
                builder.write_u8(3);
                builder.write_u32(*index);
            }
            Self::SelfType(target) => {
                builder.write_u8(4);
                target.write_to(builder);
            }
            Self::Reference {
                mutability,
                referent,
            } => {
                builder.write_u8(5);
                builder.write_u8(match mutability {
                    ReferenceMutability::Shared => 0,
                    ReferenceMutability::Mutable => 1,
                });
                referent.write_to(builder);
            }
            Self::RawPointer {
                mutability,
                referent,
            } => {
                builder.write_u8(6);
                builder.write_u8(match mutability {
                    RawPointerMutability::Const => 0,
                    RawPointerMutability::Mutable => 1,
                });
                referent.write_to(builder);
            }
            Self::Tuple(elements) => {
                builder.write_u8(7);
                write_sequence(builder, elements, Self::write_to);
            }
            Self::Callable { parameters, result } => {
                builder.write_u8(8);
                write_sequence(builder, parameters, Self::write_to);
                result.write_to(builder);
            }
            Self::ErrorUnion(members) => {
                builder.write_u8(9);
                write_sequence(builder, members, Self::write_to);
            }
            Self::AssociatedProjection {
                base,
                trait_ref,
                associated,
            } => {
                builder.write_u8(10);
                base.write_to(builder);
                trait_ref.write_to(builder);
                associated.write_to(builder);
            }
        }
    }
}

impl StableNominalHead {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::Intrinsic(head) => {
                builder.write_u8(0);
                builder.write_str(head.spelling());
            }
            Self::Declaration(target) => {
                builder.write_u8(1);
                target.write_to(builder);
            }
        }
    }
}

impl StableTraitRef {
    pub(crate) fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        self.declaration.write_to(builder);
        write_sequence(builder, &self.arguments, StableType::write_to);
    }
}

fn write_sequence<T>(
    builder: &mut CanonicalFingerprintBuilder,
    values: &[T],
    write: impl Fn(&T, &mut CanonicalFingerprintBuilder),
) {
    builder.write_len(values.len());
    for value in values {
        write(value, builder);
    }
}

pub(crate) fn write_name(builder: &mut CanonicalFingerprintBuilder, name: &NameKey) {
    builder.write_str(name.text());
    builder.write_bool(name.is_bang());
}

pub(crate) fn write_declaration_kind(
    builder: &mut CanonicalFingerprintBuilder,
    kind: DeclarationKind,
) {
    builder.write_u8(match kind {
        DeclarationKind::Function => 0,
        DeclarationKind::ExternFunction => 1,
        DeclarationKind::IntrinsicConstructor => 2,
        DeclarationKind::Trait => 3,
        DeclarationKind::Impl => 4,
        DeclarationKind::Struct => 5,
        DeclarationKind::Enum => 6,
        DeclarationKind::OpaqueType => 7,
        DeclarationKind::EnumVariant => 8,
        DeclarationKind::EnumVariantField => 9,
        DeclarationKind::TypeAlias => 10,
        DeclarationKind::Const => 11,
        DeclarationKind::AssociatedType => 12,
        DeclarationKind::AssociatedTypeBinding => 13,
        DeclarationKind::StorageField => 14,
        DeclarationKind::Property => 15,
        DeclarationKind::GenericParameter => 16,
    });
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StableDeclarationError {
    UnknownDeclaration(DefId),
    CyclicDeclarationPath(DefId),
    CrossModuleParent { declaration: DefId, parent: DefId },
    UnnamedDeclaration(DefId),
    OrphanGenericParameter(DefId),
    UnknownGenericParameter { owner: DefId, parameter: DefId },
    GenericParameterOverflow(usize),
    IncompleteSignature(DefId),
    UnboundGenericParameter(GenericParameterId),
    TypeStore(TypeStoreError),
}

impl fmt::Display for StableDeclarationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDeclaration(declaration) => {
                write!(formatter, "unknown source declaration {declaration:?}")
            }
            Self::CyclicDeclarationPath(declaration) => write!(
                formatter,
                "source declaration {declaration:?} has a cyclic owner path"
            ),
            Self::CrossModuleParent {
                declaration,
                parent,
            } => write!(
                formatter,
                "source declaration {declaration:?} has cross-module parent {parent:?}"
            ),
            Self::UnnamedDeclaration(declaration) => write!(
                formatter,
                "source declaration {declaration:?} has no stable path component"
            ),
            Self::OrphanGenericParameter(declaration) => write!(
                formatter,
                "generic parameter {declaration:?} has no declaring owner"
            ),
            Self::UnknownGenericParameter { owner, parameter } => write!(
                formatter,
                "declaration {owner:?} does not own generic parameter {parameter:?}"
            ),
            Self::GenericParameterOverflow(count) => write!(
                formatter,
                "generic parameter count {count} exceeds the stable key representation"
            ),
            Self::IncompleteSignature(declaration) => write!(
                formatter,
                "source declaration {declaration:?} has no complete signature for stable identity"
            ),
            Self::UnboundGenericParameter(parameter) => write!(
                formatter,
                "canonical type references generic parameter {parameter:?} outside its binder"
            ),
            Self::TypeStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StableDeclarationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TypeStore(error) => Some(error),
            Self::UnknownDeclaration(_)
            | Self::CyclicDeclarationPath(_)
            | Self::CrossModuleParent { .. }
            | Self::UnnamedDeclaration(_)
            | Self::OrphanGenericParameter(_)
            | Self::UnknownGenericParameter { .. }
            | Self::GenericParameterOverflow(_)
            | Self::IncompleteSignature(_)
            | Self::UnboundGenericParameter(_) => None,
        }
    }
}

impl From<TypeStoreError> for StableDeclarationError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}
