use eglur_hir::{FunctionBody, ItemKind, LiteralKind, Visibility};
use eglur_syntax::SyntaxKind;

use super::LoweredDeclarations;

#[test]
fn function_signature_preserves_generic_bound_and_parameter_roles_in_order() {
    let spelling = "pub overload fn dispatch![GenericZ: cap::Second + First, GenericA](right: RightInput, left: LeftInput) -> CallResult = 101";
    let source = LoweredDeclarations::valid(&format!("{spelling}\n"));
    assert_eq!(source.hir.roots().len(), 1);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::FnDecl,
        spelling,
        Visibility::Public,
    );
    assert!(item.attributes().is_empty());
    let ItemKind::Function(function) = item.kind() else {
        panic!("expected function");
    };
    source.name(&function.name, "dispatch!");
    assert!(function.overload);
    assert_eq!(function.generic_parameters.len(), 2);
    source.generic(
        &function.generic_parameters[0],
        "GenericZ",
        &["cap::Second", "First"],
    );
    source.generic(&function.generic_parameters[1], "GenericA", &[]);
    assert_eq!(function.parameters.len(), 2);
    source.parameter(
        function.body,
        &function.parameters[0],
        "right",
        "RightInput",
    );
    source.parameter(function.body, &function.parameters[1], "left", "LeftInput");
    source.ty(function.return_type.unwrap(), "CallResult");
    let FunctionBody::Expression(value) = function.body_form else {
        panic!("expected expression body");
    };
    source.integer(function.body, value, "101");
}

#[test]
fn missing_return_and_empty_parameter_lists_are_not_synthesized() {
    let spelling = "fn plain() = 203";
    let source = LoweredDeclarations::valid(&format!("{spelling}\n"));
    assert_eq!(source.hir.roots().len(), 1);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::FnDecl,
        spelling,
        Visibility::Private,
    );
    let ItemKind::Function(function) = item.kind() else {
        panic!("expected function");
    };
    source.name(&function.name, "plain");
    assert!(!function.overload);
    assert!(function.generic_parameters.is_empty());
    assert!(function.parameters.is_empty());
    assert!(function.return_type.is_none());
    let FunctionBody::Expression(value) = function.body_form else {
        panic!("expected body");
    };
    source.integer(function.body, value, "203");
}

#[test]
fn extern_declarations_keep_abi_parameters_return_presence_and_body_identity() {
    let first = "extern \"C\" fn foreign!(out: OutArg, inbound: InArg) -> ForeignResult";
    let second = "pub extern \"system\" fn bare()";
    let source = LoweredDeclarations::valid(&format!("{first}\n{second}\n"));
    assert_eq!(source.hir.roots().len(), 2);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::ExternDecl,
        first,
        Visibility::Private,
    );
    let ItemKind::Extern(external) = item.kind() else {
        panic!("expected extern declaration");
    };
    source.literal(&external.abi, LiteralKind::String, "\"C\"");
    source.name(&external.name, "foreign!");
    assert_eq!(external.parameters.len(), 2);
    source.parameter(external.body, &external.parameters[0], "out", "OutArg");
    source.parameter(external.body, &external.parameters[1], "inbound", "InArg");
    source.ty(external.return_type.unwrap(), "ForeignResult");
    assert_eq!(source.hir.body(external.body).expressions().len(), 0);

    let item = source.item(
        source.hir.roots()[1],
        SyntaxKind::ExternDecl,
        second,
        Visibility::Public,
    );
    let ItemKind::Extern(empty) = item.kind() else {
        panic!("expected second extern declaration");
    };
    source.literal(&empty.abi, LiteralKind::String, "\"system\"");
    source.name(&empty.name, "bare");
    assert!(empty.parameters.is_empty());
    assert!(empty.return_type.is_none());
    assert_eq!(source.hir.body(empty.body).expressions().len(), 0);
    assert_ne!(external.body, empty.body);
    assert!(
        source
            .hir
            .body(empty.body)
            .pattern(external.parameters[0].pattern)
            .is_none()
    );
}

#[test]
fn alias_and_const_keep_their_own_types_generics_and_initializers() {
    let alias_text = "pub type Alias[AliasZ: AliasBound, AliasA] = AliasTarget";
    let const_text = "const Limit: LimitType = 307";
    let source = LoweredDeclarations::valid(&format!("{alias_text}\n{const_text}\n"));
    assert_eq!(source.hir.roots().len(), 2);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::TypeDecl,
        alias_text,
        Visibility::Public,
    );
    let ItemKind::TypeAlias(alias) = item.kind() else {
        panic!("expected alias");
    };
    source.name(&alias.name, "Alias");
    assert_eq!(alias.generic_parameters.len(), 2);
    source.generic(&alias.generic_parameters[0], "AliasZ", &["AliasBound"]);
    source.generic(&alias.generic_parameters[1], "AliasA", &[]);
    source.ty(alias.value, "AliasTarget");

    let item = source.item(
        source.hir.roots()[1],
        SyntaxKind::ConstDecl,
        const_text,
        Visibility::Private,
    );
    let ItemKind::Const(constant) = item.kind() else {
        panic!("expected constant");
    };
    source.name(&constant.name, "Limit");
    source.ty(constant.ty, "LimitType");
    source.integer(constant.body, constant.value, "307");
}
