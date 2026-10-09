use eglur_sema::{
    CanonicalFingerprintBuilder, EffectCategory, EffectSummary, ExternalNameCoverage,
    ExternalNameEnvironment, ExternalSemanticOwner, FingerprintDomain, InherentMemberForm,
    IntrinsicNominal, LanguageItem, Namespace, PrimitiveType, RawPointerMutability,
    ReferenceMutability, StateScope, TensorSourceConstructor, TrustedCallableRequirement,
    TrustedConstructorPayload, TrustedDeclarationSignature, TrustedEnumPayload,
    TrustedGenericParameter, TrustedInhabitanceProduct, TrustedInherentOwner,
    TrustedInherentOwnerHead, TrustedNominalInhabitance, TrustedSignatureKind, TrustedTraitExpr,
    TrustedTypeExpr,
};

use super::CatalogFingerprint;
use crate::catalog::TrustedCatalog;

pub(super) fn fingerprint(catalog: Option<&TrustedCatalog>) -> CatalogFingerprint {
    let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Catalog);
    builder.write_bool(catalog.is_some());
    let Some(catalog) = catalog else {
        return CatalogFingerprint(builder.finish());
    };

    write_names(&mut builder, catalog);
    write_signatures(&mut builder, catalog);
    write_inhabitance(&mut builder, catalog);
    write_effects(&mut builder, catalog);
    write_language_items(&mut builder, catalog);
    CatalogFingerprint(builder.finish())
}

fn write_names(builder: &mut CanonicalFingerprintBuilder, catalog: &TrustedCatalog) {
    write_name_environment(builder, catalog.names());
}

fn write_name_environment(
    builder: &mut CanonicalFingerprintBuilder,
    names: &ExternalNameEnvironment,
) {
    builder.write_u8(match names.coverage() {
        ExternalNameCoverage::Complete => 0,
        ExternalNameCoverage::PartialStandardLibrary => 1,
    });

    builder.write_len(names.modules().len());
    for module in names.modules() {
        builder.write_str(module.as_str());
    }

    builder.write_len(names.declarations().len());
    for declaration in names.declarations() {
        builder.write_str(declaration.id().as_str());
        builder.write_str(declaration.module().as_str());
        write_name(builder, declaration.name());
        builder.write_declaration_kind(declaration.kind());
        write_semantic_owner(builder, declaration.semantic_owner());
    }

    builder.write_len(names.prelude_bindings().len());
    for (namespace, name, declarations) in names.prelude_bindings() {
        write_namespace(builder, namespace);
        write_name(builder, name);
        builder.write_len(declarations.len());
        for declaration in declarations {
            builder.write_str(declaration.as_str());
        }
    }
}

fn write_semantic_owner(builder: &mut CanonicalFingerprintBuilder, owner: ExternalSemanticOwner) {
    match owner {
        ExternalSemanticOwner::General => builder.write_u8(0),
        ExternalSemanticOwner::TensorSource(constructor) => {
            builder.write_u8(1);
            builder.write_u8(match constructor {
                TensorSourceConstructor::Zeros => 0,
                TensorSourceConstructor::Ones => 1,
                TensorSourceConstructor::Nan => 2,
                TensorSourceConstructor::Rand => 3,
            });
        }
    }
}

fn write_signatures(builder: &mut CanonicalFingerprintBuilder, catalog: &TrustedCatalog) {
    write_signature_declarations(builder, catalog.signatures().declarations());
    write_inherent_owners(builder, catalog.signatures().inherent_owners());
}

fn write_inherent_owners<'a>(
    builder: &mut CanonicalFingerprintBuilder,
    owners: impl ExactSizeIterator<Item = &'a TrustedInherentOwner>,
) {
    builder.write_len(owners.len());
    for owner in owners {
        match &owner.head {
            TrustedInherentOwnerHead::Primitive(ty) => {
                builder.write_u8(0);
                builder.write_str(ty.spelling());
            }
            TrustedInherentOwnerHead::Intrinsic(head) => {
                builder.write_u8(1);
                builder.write_str(head.spelling());
            }
            TrustedInherentOwnerHead::Nominal(head) => {
                builder.write_u8(2);
                builder.write_str(head.as_str());
            }
        }
        builder.write_len(owner.members.len());
        for member in &owner.members {
            builder.write_str(member.callable.as_str());
            write_name(builder, &member.name);
            CatalogType::from(&member.owner_pattern).write_to(builder);
            builder.write_len(member.owner_parameter_count);
            builder.write_u8(match member.form {
                InherentMemberForm::Associated => 0,
                InherentMemberForm::ValueReceiver => 1,
                InherentMemberForm::SharedReceiver => 2,
                InherentMemberForm::MutableReceiver => 3,
            });
            builder.write_bool(member.visibility == eglur_hir::Visibility::Public);
            builder.write_bool(member.overload);
        }
    }
}

fn write_signature_declarations<'a>(
    builder: &mut CanonicalFingerprintBuilder,
    declarations: impl ExactSizeIterator<Item = &'a TrustedDeclarationSignature>,
) {
    builder.write_len(declarations.len());
    for declaration in declarations {
        builder.write_str(declaration.id().as_str());
        match declaration.kind() {
            TrustedSignatureKind::Nominal { generic_parameters } => {
                builder.write_u8(0);
                write_generics(builder, generic_parameters);
            }
            TrustedSignatureKind::Enum {
                generic_parameters,
                variants,
            } => {
                builder.write_u8(5);
                write_generics(builder, generic_parameters);
                builder.write_len(variants.len());
                for variant in variants {
                    builder.write_str(variant.id.as_str());
                    write_name(builder, &variant.name);
                    match &variant.payload {
                        TrustedEnumPayload::Absent => builder.write_u8(0),
                        TrustedEnumPayload::Tuple(types) => {
                            builder.write_u8(1);
                            write_ordered_types(builder, types);
                        }
                        TrustedEnumPayload::Named(fields) => {
                            builder.write_u8(2);
                            builder.write_len(fields.len());
                            for field in fields {
                                builder.write_str(field.id.as_str());
                                write_name(builder, &field.name);
                                CatalogType::from(&field.ty).write_to(builder);
                            }
                        }
                    }
                }
            }
            TrustedSignatureKind::Trait {
                closed,
                generic_parameters,
                associated_types,
            } => {
                builder.write_u8(1);
                builder.write_bool(*closed);
                write_generics(builder, generic_parameters);
                let mut associated_types = associated_types.iter().collect::<Vec<_>>();
                associated_types.sort_by(|left, right| {
                    left.id
                        .cmp(&right.id)
                        .then_with(|| left.name.cmp(&right.name))
                });
                builder.write_len(associated_types.len());
                for associated in associated_types {
                    builder.write_str(associated.id.as_str());
                    builder.write_str(&associated.name);
                }
            }
            TrustedSignatureKind::Callable {
                bang,
                generic_parameters,
                parameters,
                result,
                requirements,
            } => {
                builder.write_u8(2);
                builder.write_bool(*bang);
                write_generics(builder, generic_parameters);
                builder.write_len(parameters.len());
                for parameter in parameters {
                    builder.write_bool(parameter.name.is_some());
                    if let Some(name) = &parameter.name {
                        builder.write_str(name);
                    }
                    CatalogType::from(&parameter.ty).write_to(builder);
                }
                CatalogType::from(result).write_to(builder);
                let mut requirements = requirements
                    .iter()
                    .map(CatalogRequirement::from)
                    .collect::<Vec<_>>();
                requirements.sort();
                requirements.dedup();
                builder.write_len(requirements.len());
                for requirement in requirements {
                    requirement.write_to(builder);
                }
            }
            TrustedSignatureKind::Constructor {
                generic_parameters,
                payload,
                result,
            } => {
                builder.write_u8(3);
                write_generics(builder, generic_parameters);
                match payload {
                    TrustedConstructorPayload::Unit => builder.write_u8(0),
                    TrustedConstructorPayload::Positional(types) => {
                        builder.write_u8(1);
                        write_ordered_types(builder, types);
                    }
                }
                CatalogType::from(result).write_to(builder);
            }
            TrustedSignatureKind::Impl {
                negative,
                generic_parameters,
                trait_ref,
                target,
                associated_bindings,
            } => {
                builder.write_u8(4);
                builder.write_bool(*negative);
                write_generics(builder, generic_parameters);
                CatalogTrait::from(trait_ref).write_to(builder);
                CatalogType::from(target).write_to(builder);
                let mut bindings = associated_bindings.iter().collect::<Vec<_>>();
                bindings.sort_by(|left, right| {
                    left.declaration
                        .cmp(&right.declaration)
                        .then_with(|| left.associated.cmp(&right.associated))
                });
                builder.write_len(bindings.len());
                for binding in bindings {
                    builder.write_str(binding.declaration.as_str());
                    builder.write_str(binding.associated.as_str());
                    CatalogType::from(&binding.value).write_to(builder);
                }
            }
        }
        builder.write_len(declaration.trait_methods().len());
        for method in declaration.trait_methods() {
            builder.write_str(method.declaration.as_str());
            write_name(builder, &method.name);
            builder.write_bool(method.has_default);
        }
        builder.write_len(declaration.impl_methods().len());
        for method in declaration.impl_methods() {
            builder.write_str(method.trait_method.as_str());
            builder.write_str(method.callable.as_str());
        }
        builder.write_bool(declaration.trait_adapter().is_some());
        if let Some(adapter) = declaration.trait_adapter() {
            builder.write_str(adapter.method.as_str());
            builder.write_u32(adapter.argument);
        }
    }
}

fn write_generics(
    builder: &mut CanonicalFingerprintBuilder,
    parameters: &[TrustedGenericParameter],
) {
    builder.write_len(parameters.len());
    for parameter in parameters {
        let mut bounds = parameter
            .bounds
            .iter()
            .map(CatalogTrait::from)
            .collect::<Vec<_>>();
        bounds.sort();
        bounds.dedup();
        builder.write_len(bounds.len());
        for bound in bounds {
            bound.write_to(builder);
        }
    }
}

fn write_inhabitance(builder: &mut CanonicalFingerprintBuilder, catalog: &TrustedCatalog) {
    let facts = catalog.inhabitance();
    builder.write_len(facts.facts().len());
    for fact in facts.facts() {
        builder.write_str(fact.declaration().as_str());
        match fact.rule() {
            TrustedNominalInhabitance::Unknown => builder.write_u8(0),
            TrustedNominalInhabitance::Inhabited => builder.write_u8(1),
            TrustedNominalInhabitance::Product(types) => {
                builder.write_u8(2);
                write_product(builder, types);
            }
            TrustedNominalInhabitance::Sum(products) => {
                builder.write_u8(3);
                let mut products = products.iter().map(canonical_product).collect::<Vec<_>>();
                products.sort();
                products.dedup();
                builder.write_len(products.len());
                for product in products {
                    write_catalog_types(builder, &product);
                }
            }
        }
    }
}

fn write_effects(builder: &mut CanonicalFingerprintBuilder, catalog: &TrustedCatalog) {
    let effects = catalog.effects();
    builder.write_len(effects.summaries().len());
    for effect in effects.summaries() {
        builder.write_str(effect.declaration().as_str());
        match effect.summary() {
            EffectSummary::Unknown => builder.write_u8(0),
            EffectSummary::Known(summary) => {
                builder.write_u8(1);
                builder.write_len(summary.categories().len());
                for category in summary.categories() {
                    builder.write_u8(match category {
                        EffectCategory::Io => 0,
                        EffectCategory::State => 1,
                        EffectCategory::Nondet => 2,
                    });
                }
                builder.write_u8(match summary.state_scope() {
                    StateScope::None => 0,
                    StateScope::Localizable => 1,
                    StateScope::Nonlocal => 2,
                });
            }
        }
    }
}

fn write_language_items(builder: &mut CanonicalFingerprintBuilder, catalog: &TrustedCatalog) {
    let items = catalog.language_items();
    builder.write_len(items.items().len());
    for (item, declaration) in items.items() {
        builder.write_u8(match item {
            LanguageItem::NumericError => 0,
            LanguageItem::Tensor => 5,
            LanguageItem::OptSome => 1,
            LanguageItem::OptNone => 2,
            LanguageItem::ResOk => 3,
            LanguageItem::ResErr => 4,
        });
        builder.write_str(declaration.as_str());
    }
}

fn write_ordered_types(builder: &mut CanonicalFingerprintBuilder, types: &[TrustedTypeExpr]) {
    let types = types.iter().map(CatalogType::from).collect::<Vec<_>>();
    write_catalog_types(builder, &types);
}

fn write_product(builder: &mut CanonicalFingerprintBuilder, types: &[TrustedTypeExpr]) {
    let mut types = types.iter().map(CatalogType::from).collect::<Vec<_>>();
    types.sort();
    types.dedup();
    write_catalog_types(builder, &types);
}

fn canonical_product(product: &TrustedInhabitanceProduct) -> Box<[CatalogType]> {
    let mut types = product
        .members()
        .iter()
        .map(CatalogType::from)
        .collect::<Vec<_>>();
    types.sort();
    types.dedup();
    types.into_boxed_slice()
}

fn write_catalog_types(builder: &mut CanonicalFingerprintBuilder, types: &[CatalogType]) {
    builder.write_len(types.len());
    for ty in types {
        ty.write_to(builder);
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CatalogTrait {
    declaration: eglur_sema::ExternalDefId,
    arguments: Box<[CatalogType]>,
}

impl From<&TrustedTraitExpr> for CatalogTrait {
    fn from(trait_ref: &TrustedTraitExpr) -> Self {
        Self {
            declaration: trait_ref.declaration.clone(),
            arguments: trait_ref
                .arguments
                .iter()
                .map(CatalogType::from)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }
}

impl CatalogTrait {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        builder.write_str(self.declaration.as_str());
        write_catalog_types(builder, &self.arguments);
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CatalogRequirement {
    FixedWidthInteger(CatalogType),
}

impl From<&TrustedCallableRequirement> for CatalogRequirement {
    fn from(requirement: &TrustedCallableRequirement) -> Self {
        match requirement {
            TrustedCallableRequirement::FixedWidthInteger(ty) => {
                Self::FixedWidthInteger(CatalogType::from(ty))
            }
        }
    }
}

impl CatalogRequirement {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::FixedWidthInteger(ty) => {
                builder.write_u8(0);
                ty.write_to(builder);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CatalogType {
    Never,
    Primitive(PrimitiveType),
    Intrinsic(IntrinsicNominal, Box<[CatalogType]>),
    Nominal(eglur_sema::ExternalDefId, Box<[CatalogType]>),
    GenericParameter(u32),
    SelfType(eglur_sema::ExternalDefId),
    Reference(ReferenceMutability, Box<CatalogType>),
    RawPointer(RawPointerMutability, Box<CatalogType>),
    Tuple(Box<[CatalogType]>),
    Callable {
        parameters: Box<[CatalogType]>,
        result: Box<CatalogType>,
    },
    ErrorUnion(Box<[CatalogType]>),
    AssociatedProjection {
        base: Box<CatalogType>,
        trait_ref: CatalogTrait,
        associated: eglur_sema::ExternalDefId,
    },
}

impl From<&TrustedTypeExpr> for CatalogType {
    fn from(ty: &TrustedTypeExpr) -> Self {
        match ty {
            TrustedTypeExpr::Never => Self::Never,
            TrustedTypeExpr::Primitive(primitive) => Self::Primitive(*primitive),
            TrustedTypeExpr::Intrinsic { head, arguments } => Self::Intrinsic(
                *head,
                arguments
                    .iter()
                    .map(Self::from)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            TrustedTypeExpr::Nominal { head, arguments } => Self::Nominal(
                head.clone(),
                arguments
                    .iter()
                    .map(Self::from)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            TrustedTypeExpr::GenericParameter(index) => Self::GenericParameter(*index),
            TrustedTypeExpr::SelfType(declaration) => Self::SelfType(declaration.clone()),
            TrustedTypeExpr::Reference {
                mutability,
                referent,
            } => Self::Reference(*mutability, Box::new(Self::from(referent.as_ref()))),
            TrustedTypeExpr::RawPointer {
                mutability,
                referent,
            } => Self::RawPointer(*mutability, Box::new(Self::from(referent.as_ref()))),
            TrustedTypeExpr::Tuple(elements) => Self::Tuple(
                elements
                    .iter()
                    .map(Self::from)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            TrustedTypeExpr::Callable { parameters, result } => Self::Callable {
                parameters: parameters
                    .iter()
                    .map(Self::from)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
                result: Box::new(Self::from(result.as_ref())),
            },
            TrustedTypeExpr::ErrorUnion(elements) => {
                let mut elements = elements.iter().map(Self::from).collect::<Vec<_>>();
                elements.sort();
                elements.dedup();
                Self::ErrorUnion(elements.into_boxed_slice())
            }
            TrustedTypeExpr::AssociatedProjection {
                base,
                trait_declaration,
                trait_arguments,
                associated,
            } => Self::AssociatedProjection {
                base: Box::new(Self::from(base.as_ref())),
                trait_ref: CatalogTrait {
                    declaration: trait_declaration.clone(),
                    arguments: trait_arguments
                        .iter()
                        .map(Self::from)
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                },
                associated: associated.clone(),
            },
        }
    }
}

impl CatalogType {
    fn write_to(&self, builder: &mut CanonicalFingerprintBuilder) {
        match self {
            Self::Never => builder.write_u8(0),
            Self::Primitive(primitive) => {
                builder.write_u8(1);
                builder.write_str(primitive.spelling());
            }
            Self::Intrinsic(head, arguments) => {
                builder.write_u8(2);
                builder.write_str(head.spelling());
                write_catalog_types(builder, arguments);
            }
            Self::Nominal(head, arguments) => {
                builder.write_u8(3);
                builder.write_str(head.as_str());
                write_catalog_types(builder, arguments);
            }
            Self::GenericParameter(index) => {
                builder.write_u8(4);
                builder.write_u32(*index);
            }
            Self::SelfType(declaration) => {
                builder.write_u8(5);
                builder.write_str(declaration.as_str());
            }
            Self::Reference(mutability, referent) => {
                builder.write_u8(6);
                builder.write_u8(match mutability {
                    ReferenceMutability::Shared => 0,
                    ReferenceMutability::Mutable => 1,
                });
                referent.write_to(builder);
            }
            Self::RawPointer(mutability, referent) => {
                builder.write_u8(7);
                builder.write_u8(match mutability {
                    RawPointerMutability::Const => 0,
                    RawPointerMutability::Mutable => 1,
                });
                referent.write_to(builder);
            }
            Self::Tuple(elements) => {
                builder.write_u8(8);
                write_catalog_types(builder, elements);
            }
            Self::Callable { parameters, result } => {
                builder.write_u8(9);
                write_catalog_types(builder, parameters);
                result.write_to(builder);
            }
            Self::ErrorUnion(elements) => {
                builder.write_u8(10);
                write_catalog_types(builder, elements);
            }
            Self::AssociatedProjection {
                base,
                trait_ref,
                associated,
            } => {
                builder.write_u8(11);
                base.write_to(builder);
                trait_ref.write_to(builder);
                builder.write_str(associated.as_str());
            }
        }
    }
}

fn write_name(builder: &mut CanonicalFingerprintBuilder, name: &eglur_sema::NameKey) {
    builder.write_str(name.text());
    builder.write_bool(name.is_bang());
}

fn write_namespace(builder: &mut CanonicalFingerprintBuilder, namespace: Namespace) {
    builder.write_u8(match namespace {
        Namespace::Value => 0,
        Namespace::Type => 1,
    });
}

#[cfg(test)]
mod tests {
    use eglur_hir::Visibility;
    use eglur_sema::{
        DeclarationKind, ExternalDeclaration, ExternalDefId, ExternalNameEnvironment,
        InherentMemberForm, ModuleAddress, NameKey, TensorSourceConstructor,
        TrustedCallableParameter, TrustedEnumField, TrustedEnumPayload, TrustedEnumVariant,
        TrustedImplMethod, TrustedInherentMember, TrustedInherentOwner, TrustedInherentOwnerHead,
        TrustedSignatureEnvironment, TrustedTraitAdapter, TrustedTraitMethod,
    };

    use super::{
        CanonicalFingerprintBuilder, CatalogFingerprint, FingerprintDomain, PrimitiveType,
        TrustedDeclarationSignature, TrustedSignatureKind, TrustedTraitExpr, TrustedTypeExpr,
        write_inherent_owners, write_name_environment, write_signature_declarations,
    };

    fn id(value: &str) -> ExternalDefId {
        ExternalDefId::new(value).expect("test catalog identities are valid")
    }

    fn relation_fingerprint(declarations: &[TrustedDeclarationSignature]) -> CatalogFingerprint {
        let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Catalog);
        write_signature_declarations(&mut builder, declarations.iter());
        CatalogFingerprint(builder.finish())
    }

    fn inherent_fingerprint(owners: Vec<TrustedInherentOwner>) -> CatalogFingerprint {
        let signatures = TrustedSignatureEnvironment::with_inherent_owners(
            ["test::a", "test::b"].map(|id_value| {
                TrustedDeclarationSignature::new(
                    id(id_value),
                    TrustedSignatureKind::Callable {
                        bang: false,
                        generic_parameters: Box::new([]),
                        parameters: Box::new([]),
                        result: TrustedTypeExpr::Primitive(PrimitiveType::Unit),
                        requirements: Box::new([]),
                    },
                )
            }),
            owners,
        )
        .unwrap();
        let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Catalog);
        write_signature_declarations(&mut builder, signatures.declarations());
        write_inherent_owners(&mut builder, signatures.inherent_owners());
        CatalogFingerprint(builder.finish())
    }

    #[test]
    fn ordinary_membership_fingerprint_observes_coverage_identity_and_contract_fields() {
        let owner = TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Primitive(PrimitiveType::Str),
            members: Box::new([TrustedInherentMember {
                callable: id("test::a"),
                name: NameKey::new("empty", false).unwrap(),
                owner_pattern: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                owner_parameter_count: 0,
                form: InherentMemberForm::Associated,
                visibility: Visibility::Public,
                overload: false,
            }]),
        };
        let baseline = inherent_fingerprint(vec![owner.clone()]);
        let mut empty = owner.clone();
        empty.members = Box::new([]);
        assert_ne!(
            inherent_fingerprint(vec![]),
            inherent_fingerprint(vec![empty.clone()])
        );
        assert_ne!(baseline, inherent_fingerprint(vec![empty]));
        for field in 0..4 {
            let mut changed = owner.clone();
            match field {
                0 => changed.members[0].callable = id("test::b"),
                1 => changed.members[0].name = NameKey::new("different", false).unwrap(),
                2 => changed.members[0].visibility = Visibility::Private,
                3 => changed.members[0].overload = true,
                _ => unreachable!(),
            }
            assert_ne!(baseline, inherent_fingerprint(vec![changed]));
        }
        let mut changed_head = owner;
        changed_head.head = TrustedInherentOwnerHead::Primitive(PrimitiveType::I64);
        changed_head.members[0].owner_pattern = TrustedTypeExpr::Primitive(PrimitiveType::I64);
        assert_ne!(baseline, inherent_fingerprint(vec![changed_head]));
    }

    #[test]
    fn ordinary_membership_fingerprint_is_independent_of_input_order() {
        let mut members = ["test::a", "test::b"].map(|value| TrustedInherentMember {
            callable: id(value),
            name: NameKey::new("empty", false).unwrap(),
            owner_pattern: TrustedTypeExpr::Primitive(PrimitiveType::Str),
            owner_parameter_count: 0,
            form: InherentMemberForm::Associated,
            visibility: Visibility::Public,
            overload: true,
        });
        let inventory = |members: Box<[TrustedInherentMember]>| TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Primitive(PrimitiveType::Str),
            members,
        };
        let empty = TrustedInherentOwner {
            head: TrustedInherentOwnerHead::Primitive(PrimitiveType::I64),
            members: Box::new([]),
        };
        let first = inherent_fingerprint(vec![inventory(members.clone().into()), empty.clone()]);
        members.reverse();
        assert_eq!(
            first,
            inherent_fingerprint(vec![empty, inventory(members.into())])
        );
    }

    fn name_fingerprint(declaration: ExternalDeclaration) -> CatalogFingerprint {
        let module = declaration.module().clone();
        let names =
            ExternalNameEnvironment::partial_standard_library([module], [declaration], []).unwrap();
        let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Catalog);
        write_name_environment(&mut builder, &names);
        CatalogFingerprint(builder.finish())
    }

    fn declarations(
        has_default: bool,
        impl_callable: ExternalDefId,
        adapter_method: ExternalDefId,
    ) -> Box<[TrustedDeclarationSignature]> {
        let trait_id = id("test::Display");
        let method_id = id("test::Display.to_str");
        let implementation_id = id("test::Display.Bool");
        let adapter_id = id("test::to_str");
        Box::new([
            TrustedDeclarationSignature::new(
                trait_id.clone(),
                TrustedSignatureKind::Trait {
                    closed: false,
                    generic_parameters: Box::new([]),
                    associated_types: Box::new([]),
                },
            )
            .with_trait_methods([TrustedTraitMethod {
                declaration: method_id.clone(),
                name: NameKey::new("to_str", false).unwrap(),
                has_default,
            }]),
            TrustedDeclarationSignature::new(
                implementation_id,
                TrustedSignatureKind::Impl {
                    negative: false,
                    generic_parameters: Box::new([]),
                    trait_ref: TrustedTraitExpr {
                        declaration: trait_id,
                        arguments: Box::new([]),
                    },
                    target: TrustedTypeExpr::Primitive(PrimitiveType::Bool),
                    associated_bindings: Box::new([]),
                },
            )
            .with_impl_methods([TrustedImplMethod {
                trait_method: method_id,
                callable: impl_callable,
            }]),
            TrustedDeclarationSignature::new(
                adapter_id,
                TrustedSignatureKind::Callable {
                    bang: false,
                    generic_parameters: Box::new([]),
                    parameters: Box::<[TrustedCallableParameter]>::default(),
                    result: TrustedTypeExpr::Primitive(PrimitiveType::Str),
                    requirements: Box::new([]),
                },
            )
            .with_trait_adapter(TrustedTraitAdapter {
                method: adapter_method,
                argument: 0,
            }),
        ])
    }

    #[test]
    fn fingerprint_observes_every_trusted_method_relation() {
        let baseline = declarations(
            false,
            id("test::Display.Bool.to_str"),
            id("test::Display.to_str"),
        );
        let baseline = relation_fingerprint(&baseline);

        assert_ne!(
            baseline,
            relation_fingerprint(&declarations(
                true,
                id("test::Display.Bool.to_str"),
                id("test::Display.to_str"),
            ))
        );
        assert_ne!(
            baseline,
            relation_fingerprint(&declarations(
                false,
                id("test::Display.Bool.render"),
                id("test::Display.to_str"),
            ))
        );
        assert_ne!(
            baseline,
            relation_fingerprint(&declarations(
                false,
                id("test::Display.Bool.to_str"),
                id("test::Display.render"),
            ))
        );
    }

    #[test]
    fn fingerprint_observes_enum_members_and_each_payload_contract() {
        fn fingerprint(variants: Vec<TrustedEnumVariant>) -> CatalogFingerprint {
            let declarations =
                TrustedSignatureEnvironment::new([TrustedDeclarationSignature::new(
                    id("test::Choice"),
                    TrustedSignatureKind::Enum {
                        generic_parameters: Box::new([]),
                        variants: variants.into_boxed_slice(),
                    },
                )])
                .unwrap();
            relation_fingerprint(&declarations.declarations().cloned().collect::<Vec<_>>())
        }

        fn fields(variants: &mut [TrustedEnumVariant]) -> &mut [TrustedEnumField] {
            let TrustedEnumPayload::Named(fields) = &mut variants[1].payload else {
                panic!("second variant has named fields");
            };
            fields
        }

        let baseline = vec![
            TrustedEnumVariant {
                id: id("test::stop"),
                name: NameKey::new("Stop", false).unwrap(),
                payload: TrustedEnumPayload::Absent,
            },
            TrustedEnumVariant {
                id: id("test::record"),
                name: NameKey::new("Record", false).unwrap(),
                payload: TrustedEnumPayload::Named(Box::new([
                    TrustedEnumField {
                        id: id("test::left"),
                        name: NameKey::new("left", false).unwrap(),
                        ty: TrustedTypeExpr::Primitive(PrimitiveType::I64),
                    },
                    TrustedEnumField {
                        id: id("test::right"),
                        name: NameKey::new("right", false).unwrap(),
                        ty: TrustedTypeExpr::Primitive(PrimitiveType::Bool),
                    },
                ])),
            },
        ];
        let expected = fingerprint(baseline.clone());
        type Mutation = fn(&mut [TrustedEnumVariant]);
        let mutations: [(&str, Mutation); 11] = [
            ("variant identity", |variants| {
                variants[0].id = id("test::other")
            }),
            ("variant name", |variants| {
                variants[0].name = NameKey::new("End", false).unwrap()
            }),
            ("variant order", |variants| variants.swap(0, 1)),
            ("empty positional payload", |variants| {
                variants[0].payload = TrustedEnumPayload::Tuple(Box::new([]))
            }),
            ("positional payload", |variants| {
                variants[0].payload = TrustedEnumPayload::Tuple(Box::new([TrustedTypeExpr::Never]))
            }),
            ("empty named payload", |variants| {
                variants[0].payload = TrustedEnumPayload::Named(Box::new([]))
            }),
            ("field identity", |variants| {
                fields(variants)[0].id = id("test::other")
            }),
            ("field name", |variants| {
                fields(variants)[0].name = NameKey::new("other", false).unwrap()
            }),
            ("field type", |variants| {
                fields(variants)[0].ty = TrustedTypeExpr::Never
            }),
            ("field order", |variants| fields(variants).swap(0, 1)),
            ("payload form", |variants| {
                variants[1].payload = TrustedEnumPayload::Tuple(Box::new([
                    TrustedTypeExpr::Primitive(PrimitiveType::I64),
                    TrustedTypeExpr::Primitive(PrimitiveType::Bool),
                ]))
            }),
        ];
        for (label, mutate) in mutations {
            let mut changed = baseline.clone();
            mutate(&mut changed);
            assert_ne!(expected, fingerprint(changed), "{label}");
        }
    }

    #[test]
    fn fingerprint_observes_external_semantic_ownership_and_tensor_constructor_kind() {
        let module =
            ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("tensor")]).unwrap();
        let declaration_id = id("std::tensor::source");
        let name = NameKey::new("source", false).unwrap();
        let general = ExternalDeclaration::new(
            declaration_id.clone(),
            module.clone(),
            name.clone(),
            DeclarationKind::Function,
        )
        .unwrap();
        let zeros = ExternalDeclaration::tensor_source(
            declaration_id.clone(),
            module.clone(),
            name.clone(),
            TensorSourceConstructor::Zeros,
        )
        .unwrap();
        let ones = ExternalDeclaration::tensor_source(
            declaration_id,
            module,
            name,
            TensorSourceConstructor::Ones,
        )
        .unwrap();

        assert_ne!(name_fingerprint(general), name_fingerprint(zeros.clone()));
        assert_ne!(name_fingerprint(zeros), name_fingerprint(ones));
    }
}
