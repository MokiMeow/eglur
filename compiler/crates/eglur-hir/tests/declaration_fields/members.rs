use eglur_hir::{
    EnumVariantPayload, FunctionBody, ItemKind, Mutability, StorageFieldSyntax, Visibility,
};
use eglur_syntax::SyntaxKind;

use super::LoweredDeclarations;

#[test]
fn trait_members_keep_associated_types_signatures_fields_and_properties_in_order() {
    let members = [
        "type Associated",
        "fn required(arg: RequiredArg) -> RequiredResult",
        "field trait_field: TraitFieldType",
        "prop trait_prop: TraitPropType {}",
    ];
    let text = format!(
        "pub closed trait Protocol[TraitParam: TraitBound] {{\n{}\n}}",
        members.join("\n")
    );
    let source = LoweredDeclarations::valid(&format!("{text}\n"));
    assert_eq!(source.hir.roots().len(), 1);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::TraitDecl,
        &text,
        Visibility::Public,
    );
    let ItemKind::Trait(declaration) = item.kind() else {
        panic!("expected trait");
    };
    source.name(&declaration.name, "Protocol");
    assert!(declaration.closed);
    assert_eq!(declaration.generic_parameters.len(), 1);
    source.generic(
        &declaration.generic_parameters[0],
        "TraitParam",
        &["TraitBound"],
    );
    assert_eq!(declaration.items.len(), members.len());

    let item = source.item(
        declaration.items[0],
        SyntaxKind::AssocTypeDecl,
        members[0],
        Visibility::Private,
    );
    let ItemKind::AssociatedType(associated) = item.kind() else {
        panic!("first member is an associated declaration");
    };
    source.name(&associated.name, "Associated");

    let item = source.item(
        declaration.items[1],
        SyntaxKind::FnDecl,
        members[1],
        Visibility::Private,
    );
    let ItemKind::Function(method) = item.kind() else {
        panic!("second member is a required method");
    };
    source.name(&method.name, "required");
    assert!(!method.overload);
    assert!(method.generic_parameters.is_empty());
    assert_eq!(method.parameters.len(), 1);
    source.parameter(method.body, &method.parameters[0], "arg", "RequiredArg");
    source.ty(method.return_type.unwrap(), "RequiredResult");
    assert_eq!(method.body_form, FunctionBody::Absent);
    assert_eq!(source.hir.body(method.body).expressions().len(), 0);

    let item = source.item(
        declaration.items[2],
        SyntaxKind::StorageFieldDecl,
        members[2],
        Visibility::Private,
    );
    let ItemKind::StorageField(field) = item.kind() else {
        panic!("third member is a field");
    };
    source.name(&field.name, "trait_field");
    source.ty(field.ty, "TraitFieldType");
    assert_eq!(field.syntax, StorageFieldSyntax::Field);
    assert_eq!(field.mutability, Mutability::Immutable);
    assert!(field.initializer.is_none());

    let item = source.item(
        declaration.items[3],
        SyntaxKind::PropertyDecl,
        members[3],
        Visibility::Private,
    );
    let ItemKind::Property(property) = item.kind() else {
        panic!("fourth member is a property");
    };
    source.name(&property.name, "trait_prop");
    source.ty(property.ty, "TraitPropType");
    assert_eq!(property.mutability, Mutability::Immutable);
    assert!(property.accessors.is_empty());
}

#[test]
fn impl_trait_target_polarity_and_associated_binding_remain_distinct() {
    let binding = "type Output = BoundResult";
    let method_text = "fn perform!(task: TaskInput) -> TaskResult = 311";
    let positive = format!(
        "impl[ImplParam: ImplBound] ImplementedTrait for ConcreteTarget {{\n{binding}\n{method_text}\n}}"
    );
    let negative = "impl !NegativeTrait for NegativeTarget {}";
    let source = LoweredDeclarations::valid(&format!("{positive}\n{negative}\n"));
    assert_eq!(source.hir.roots().len(), 2);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::ImplDecl,
        &positive,
        Visibility::Private,
    );
    let ItemKind::Impl(declaration) = item.kind() else {
        panic!("expected impl");
    };
    assert!(!declaration.negative);
    assert_eq!(declaration.generic_parameters.len(), 1);
    source.generic(
        &declaration.generic_parameters[0],
        "ImplParam",
        &["ImplBound"],
    );
    source.ty(declaration.trait_ref, "ImplementedTrait");
    source.ty(declaration.target, "ConcreteTarget");
    assert_eq!(declaration.items.len(), 2);

    let item = source.item(
        declaration.items[0],
        SyntaxKind::AssocTypeBinding,
        binding,
        Visibility::Private,
    );
    let ItemKind::AssociatedTypeBinding(associated) = item.kind() else {
        panic!("expected binding, not declaration");
    };
    source.name(&associated.name, "Output");
    source.ty(associated.value, "BoundResult");
    let item = source.item(
        declaration.items[1],
        SyntaxKind::FnDecl,
        method_text,
        Visibility::Private,
    );
    let ItemKind::Function(method) = item.kind() else {
        panic!("expected impl method");
    };
    source.name(&method.name, "perform!");
    assert!(!method.overload);
    assert!(method.generic_parameters.is_empty());
    assert_eq!(method.parameters.len(), 1);
    source.parameter(method.body, &method.parameters[0], "task", "TaskInput");
    source.ty(method.return_type.unwrap(), "TaskResult");
    let FunctionBody::Expression(value) = method.body_form else {
        panic!("expected method body");
    };
    source.integer(method.body, value, "311");

    let item = source.item(
        source.hir.roots()[1],
        SyntaxKind::ImplDecl,
        negative,
        Visibility::Private,
    );
    let ItemKind::Impl(declaration) = item.kind() else {
        panic!("expected negative impl");
    };
    assert!(declaration.negative);
    assert!(declaration.generic_parameters.is_empty());
    assert!(declaration.items.is_empty());
    source.ty(declaration.trait_ref, "NegativeTrait");
    source.ty(declaration.target, "NegativeTarget");
}

#[test]
fn struct_members_preserve_storage_initializers_and_nested_accessor_ownership() {
    let setter = "fn set_count!(incoming: SetterInput) -> SetterResult = 419";
    let getter = "fn get_count() -> GetterResult = 421";
    let property = format!("pub prop mut count: PropertyType {{\n{setter}\n{getter}\n}}");
    let members = [
        "field fixed: FixedType",
        "pub field mut stored: StoredType = 401",
        "var legacy: LegacyType = 409",
        &property,
        "prop quiet: QuietType {}",
        "pub fn utility() -> MethodResult = 431",
    ];
    let text = format!(
        "pub struct Record[StructParam: StructBound] : ParentType {{\n{}\n}}",
        members.join("\n")
    );
    let source = LoweredDeclarations::valid(&format!("{text}\n"));
    assert_eq!(source.hir.roots().len(), 1);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::StructDecl,
        &text,
        Visibility::Public,
    );
    let ItemKind::Struct(declaration) = item.kind() else {
        panic!("expected struct");
    };
    source.name(&declaration.name, "Record");
    source.ty(declaration.base.unwrap(), "ParentType");
    assert_eq!(declaration.generic_parameters.len(), 1);
    source.generic(
        &declaration.generic_parameters[0],
        "StructParam",
        &["StructBound"],
    );
    assert_eq!(declaration.items.len(), members.len());

    let mut initializers = Vec::new();
    for (index, visibility, syntax, mutability, name, ty, initializer) in [
        (
            0,
            Visibility::Private,
            StorageFieldSyntax::Field,
            Mutability::Immutable,
            "fixed",
            "FixedType",
            None,
        ),
        (
            1,
            Visibility::Public,
            StorageFieldSyntax::Field,
            Mutability::Mutable,
            "stored",
            "StoredType",
            Some("401"),
        ),
        (
            2,
            Visibility::Private,
            StorageFieldSyntax::Var,
            Mutability::Mutable,
            "legacy",
            "LegacyType",
            Some("409"),
        ),
    ] {
        let item = source.item(
            declaration.items[index],
            SyntaxKind::StorageFieldDecl,
            members[index],
            visibility,
        );
        let ItemKind::StorageField(field) = item.kind() else {
            panic!("expected storage field");
        };
        source.name(&field.name, name);
        source.ty(field.ty, ty);
        assert_eq!(field.syntax, syntax);
        assert_eq!(field.mutability, mutability);
        match (field.initializer, initializer) {
            (Some(actual), Some(expected)) => {
                source.integer(actual.body, actual.expression, expected);
                initializers.push(actual);
            }
            (None, None) => {}
            _ => panic!("field initializer presence changed"),
        }
    }
    assert_eq!(initializers.len(), 2);
    assert_ne!(initializers[0].body, initializers[1].body);
    assert!(
        source
            .hir
            .body(initializers[0].body)
            .expression(initializers[1].expression)
            .is_none()
    );

    let item = source.item(
        declaration.items[3],
        SyntaxKind::PropertyDecl,
        &property,
        Visibility::Public,
    );
    let ItemKind::Property(property) = item.kind() else {
        panic!("expected mutable property");
    };
    source.name(&property.name, "count");
    source.ty(property.ty, "PropertyType");
    assert_eq!(property.mutability, Mutability::Mutable);
    assert_eq!(property.accessors.len(), 2);
    let item = source.item(
        property.accessors[0],
        SyntaxKind::FnDecl,
        setter,
        Visibility::Private,
    );
    let ItemKind::Function(setter) = item.kind() else {
        panic!("first accessor is the setter");
    };
    source.name(&setter.name, "set_count!");
    assert_eq!(setter.parameters.len(), 1);
    source.parameter(
        setter.body,
        &setter.parameters[0],
        "incoming",
        "SetterInput",
    );
    source.ty(setter.return_type.unwrap(), "SetterResult");
    let FunctionBody::Expression(value) = setter.body_form else {
        panic!("expected setter body");
    };
    source.integer(setter.body, value, "419");

    let item = source.item(
        property.accessors[1],
        SyntaxKind::FnDecl,
        getter,
        Visibility::Private,
    );
    let ItemKind::Function(getter) = item.kind() else {
        panic!("second accessor is the getter");
    };
    source.name(&getter.name, "get_count");
    assert!(getter.parameters.is_empty());
    source.ty(getter.return_type.unwrap(), "GetterResult");
    let FunctionBody::Expression(value) = getter.body_form else {
        panic!("expected getter body");
    };
    source.integer(getter.body, value, "421");
    assert_ne!(setter.body, getter.body);
    assert!(
        source
            .hir
            .body(getter.body)
            .pattern(setter.parameters[0].pattern)
            .is_none()
    );

    let item = source.item(
        declaration.items[4],
        SyntaxKind::PropertyDecl,
        members[4],
        Visibility::Private,
    );
    let ItemKind::Property(quiet) = item.kind() else {
        panic!("expected empty property");
    };
    source.name(&quiet.name, "quiet");
    source.ty(quiet.ty, "QuietType");
    assert_eq!(quiet.mutability, Mutability::Immutable);
    assert!(quiet.accessors.is_empty());

    let item = source.item(
        declaration.items[5],
        SyntaxKind::FnDecl,
        members[5],
        Visibility::Public,
    );
    let ItemKind::Function(method) = item.kind() else {
        panic!("last member is an ordinary method");
    };
    source.name(&method.name, "utility");
    assert!(method.parameters.is_empty());
    source.ty(method.return_type.unwrap(), "MethodResult");
    let FunctionBody::Expression(value) = method.body_form else {
        panic!("expected method body");
    };
    source.integer(method.body, value, "431");
    assert_ne!(method.body, getter.body);
    assert_ne!(method.body, setter.body);
}

#[test]
fn enum_payloads_preserve_absence_empty_tuple_and_ordered_field_pairing() {
    let variants = [
        "Missing",
        "Empty()",
        "Single(OnlyPayload)",
        "Tuple(TupleRight, TupleLeft)",
        "#[variant_tag]\nNamed{right: NamedRight, left: NamedLeft}",
    ];
    let text = format!(
        "#[enum_tag]\npub enum Choice[EnumParam: EnumBound] {{\n{}\n}}",
        variants.join("\n")
    );
    let source = LoweredDeclarations::valid(&format!("{text}\n"));
    assert_eq!(source.hir.roots().len(), 1);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::EnumDecl,
        &text,
        Visibility::Public,
    );
    assert_eq!(item.attributes().len(), 1);
    source.written(
        item.attributes()[0].origin,
        SyntaxKind::Attribute,
        "#[enum_tag]",
    );
    source.path(&item.attributes()[0].meta.path, "enum_tag");
    assert!(item.attributes()[0].meta.arguments.is_empty());
    let ItemKind::Enum(declaration) = item.kind() else {
        panic!("expected enum");
    };
    source.name(&declaration.name, "Choice");
    assert_eq!(declaration.generic_parameters.len(), 1);
    source.generic(
        &declaration.generic_parameters[0],
        "EnumParam",
        &["EnumBound"],
    );
    assert_eq!(declaration.variants.len(), variants.len());
    for (index, name) in ["Missing", "Empty", "Single", "Tuple", "Named"]
        .into_iter()
        .enumerate()
    {
        let variant = &declaration.variants[index];
        source.name(&variant.name, name);
        source.written(variant.origin, SyntaxKind::EnumVariant, variants[index]);
        assert_eq!(variant.visibility, Visibility::Private);
        assert_eq!(variant.attributes.len(), usize::from(index == 4));
    }
    assert!(matches!(
        declaration.variants[0].payload,
        EnumVariantPayload::Absent
    ));
    let EnumVariantPayload::Tuple(empty) = &declaration.variants[1].payload else {
        panic!("empty positional payload must remain present");
    };
    assert!(empty.is_empty());
    let EnumVariantPayload::Tuple(single) = &declaration.variants[2].payload else {
        panic!("expected singleton payload");
    };
    assert_eq!(single.len(), 1);
    source.ty(single[0], "OnlyPayload");
    let EnumVariantPayload::Tuple(tuple) = &declaration.variants[3].payload else {
        panic!("expected ordered tuple payload");
    };
    assert_eq!(tuple.len(), 2);
    source.ty(tuple[0], "TupleRight");
    source.ty(tuple[1], "TupleLeft");

    let named = &declaration.variants[4];
    source.written(
        named.attributes[0].origin,
        SyntaxKind::Attribute,
        "#[variant_tag]",
    );
    source.path(&named.attributes[0].meta.path, "variant_tag");
    assert!(named.attributes[0].meta.arguments.is_empty());
    let EnumVariantPayload::Named(fields) = &named.payload else {
        panic!("expected named payload");
    };
    assert_eq!(fields.len(), 2);
    for (field, name, ty) in [
        (&fields[0], "right", "NamedRight"),
        (&fields[1], "left", "NamedLeft"),
    ] {
        source.name(&field.name, name);
        source.ty(field.ty, ty);
        source.written(
            field.origin,
            SyntaxKind::EnumVariantField,
            &format!("{name}: {ty}"),
        );
    }
}

#[test]
fn empty_containers_do_not_invent_generics_members_or_a_base() {
    for (text, syntax) in [
        ("trait EmptyTrait {}", SyntaxKind::TraitDecl),
        ("struct EmptyStruct {}", SyntaxKind::StructDecl),
        ("enum EmptyEnum {}", SyntaxKind::EnumDecl),
    ] {
        let source = LoweredDeclarations::valid(&format!("{text}\n"));
        assert_eq!(source.hir.roots().len(), 1);
        let item = source.item(source.hir.roots()[0], syntax, text, Visibility::Private);
        assert!(item.attributes().is_empty());
        match (syntax, item.kind()) {
            (SyntaxKind::TraitDecl, ItemKind::Trait(declaration)) => {
                source.name(&declaration.name, "EmptyTrait");
                assert!(!declaration.closed);
                assert!(declaration.generic_parameters.is_empty());
                assert!(declaration.items.is_empty());
            }
            (SyntaxKind::StructDecl, ItemKind::Struct(declaration)) => {
                source.name(&declaration.name, "EmptyStruct");
                assert!(declaration.generic_parameters.is_empty());
                assert!(declaration.base.is_none());
                assert!(declaration.items.is_empty());
            }
            (SyntaxKind::EnumDecl, ItemKind::Enum(declaration)) => {
                source.name(&declaration.name, "EmptyEnum");
                assert!(declaration.generic_parameters.is_empty());
                assert!(declaration.variants.is_empty());
            }
            _ => panic!("empty container kind changed"),
        }
    }
}
