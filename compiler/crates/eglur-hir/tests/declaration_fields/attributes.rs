use eglur_hir::{
    AttributeArgument, AttributeMeta, AttributeValue, ItemKind, LiteralKind, Visibility,
};
use eglur_syntax::SyntaxKind;

use super::LoweredDeclarations;

fn assert_meta(source: &LoweredDeclarations, meta: &AttributeMeta, path: &str, spelling: &str) {
    source.path(&meta.path, path);
    source.written(meta.origin, SyntaxKind::AttributeMeta, spelling);
}

fn assert_argument(
    source: &LoweredDeclarations,
    argument: &AttributeArgument,
    name: Option<&str>,
    spelling: &str,
) {
    source.raw_written(argument.origin, spelling);
    match (&argument.name, name) {
        (Some(actual), Some(expected)) => source.name(actual, expected),
        (None, None) => {}
        _ => panic!("named and positional attribute arguments must remain distinct"),
    }
}

#[test]
fn stacked_attributes_preserve_nested_arguments_literal_spellings_and_sibling_boundaries() {
    let outer = r#"meta::outer(flag = true,version = 2_047,ratio = 1.25e2,label = "x\n",strategy = policy::fast(37,mode = false),plain::Marker,"tail",)"#;
    let decorated = format!("#[{outer}]\n#[empty()]\npub fn decorated() -> DecoratedResult = 509");
    let sibling = "fn undecorated() = 601";
    let source = LoweredDeclarations::valid(&format!("{decorated}\n{sibling}\n"));
    assert_eq!(source.hir.roots().len(), 2);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::FnDecl,
        &decorated,
        Visibility::Public,
    );
    let ItemKind::Function(function) = item.kind() else {
        panic!("attributes must remain attached to the function");
    };
    source.name(&function.name, "decorated");
    assert_eq!(item.attributes().len(), 2);
    let attribute = &item.attributes()[0];
    source.written(
        attribute.origin,
        SyntaxKind::Attribute,
        &format!("#[{outer}]"),
    );
    assert_meta(&source, &attribute.meta, "meta::outer", outer);
    let arguments = &attribute.meta.arguments;
    assert_eq!(arguments.len(), 7);

    for (index, name, spelling, kind, value) in [
        (0, "flag", "flag = true", LiteralKind::Bool, "true"),
        (
            1,
            "version",
            "version = 2_047",
            LiteralKind::Integer,
            "2_047",
        ),
        (2, "ratio", "ratio = 1.25e2", LiteralKind::Float, "1.25e2"),
        (
            3,
            "label",
            r#"label = "x\n""#,
            LiteralKind::String,
            r#""x\n""#,
        ),
    ] {
        let argument = &arguments[index];
        assert_argument(&source, argument, Some(name), spelling);
        let AttributeValue::Literal(literal) = &argument.value else {
            panic!("expected literal argument {name}");
        };
        source.literal(literal, kind, value);
    }

    let strategy = &arguments[4];
    assert_argument(
        &source,
        strategy,
        Some("strategy"),
        "strategy = policy::fast(37,mode = false)",
    );
    let AttributeValue::Meta(meta) = &strategy.value else {
        panic!("named metadata must remain nested");
    };
    assert_meta(
        &source,
        meta,
        "policy::fast",
        "policy::fast(37,mode = false)",
    );
    assert_eq!(meta.arguments.len(), 2);
    assert_argument(&source, &meta.arguments[0], None, "37");
    let AttributeValue::Literal(literal) = &meta.arguments[0].value else {
        panic!("expected positional integer");
    };
    source.literal(literal, LiteralKind::Integer, "37");
    assert_argument(&source, &meta.arguments[1], Some("mode"), "mode = false");
    let AttributeValue::Literal(literal) = &meta.arguments[1].value else {
        panic!("expected named boolean");
    };
    source.literal(literal, LiteralKind::Bool, "false");

    assert_argument(&source, &arguments[5], None, "plain::Marker");
    let AttributeValue::Meta(meta) = &arguments[5].value else {
        panic!("bare path is metadata, not a string");
    };
    assert_meta(&source, meta, "plain::Marker", "plain::Marker");
    assert!(meta.arguments.is_empty());
    assert_argument(&source, &arguments[6], None, "\"tail\"");
    let AttributeValue::Literal(literal) = &arguments[6].value else {
        panic!("expected trailing positional literal");
    };
    source.literal(literal, LiteralKind::String, "\"tail\"");

    let empty = &item.attributes()[1];
    source.written(empty.origin, SyntaxKind::Attribute, "#[empty()]");
    assert_meta(&source, &empty.meta, "empty", "empty()");
    assert!(empty.meta.arguments.is_empty());
    let item = source.item(
        source.hir.roots()[1],
        SyntaxKind::FnDecl,
        sibling,
        Visibility::Private,
    );
    assert!(item.attributes().is_empty());
    let ItemKind::Function(function) = item.kind() else {
        panic!("expected unannotated sibling");
    };
    source.name(&function.name, "undecorated");
}

#[test]
fn container_and_method_attributes_follow_their_own_membership_edges() {
    let method = "#[method_tag]\nfn marked() -> MarkedResult = 701";
    for (header, kind, outer_attribute) in [
        ("struct Container", SyntaxKind::StructDecl, true),
        ("trait Container", SyntaxKind::TraitDecl, false),
        ("impl Contract for Target", SyntaxKind::ImplDecl, true),
    ] {
        let prefix = if outer_attribute {
            "#[container_tag]\n"
        } else {
            ""
        };
        let spelling = format!("{prefix}{header} {{\n{method}\n}}");
        let source = LoweredDeclarations::valid(&format!("{spelling}\n"));
        assert_eq!(source.hir.roots().len(), 1);
        let item = source.item(source.hir.roots()[0], kind, &spelling, Visibility::Private);
        assert_eq!(item.attributes().len(), usize::from(outer_attribute));
        if outer_attribute {
            let attribute = &item.attributes()[0];
            source.written(attribute.origin, SyntaxKind::Attribute, "#[container_tag]");
            assert_meta(&source, &attribute.meta, "container_tag", "container_tag");
            assert!(attribute.meta.arguments.is_empty());
        }
        let members = match (kind, item.kind()) {
            (SyntaxKind::StructDecl, ItemKind::Struct(declaration)) => &declaration.items,
            (SyntaxKind::TraitDecl, ItemKind::Trait(declaration)) => &declaration.items,
            (SyntaxKind::ImplDecl, ItemKind::Impl(declaration)) => &declaration.items,
            _ => panic!("container kind changed"),
        };
        assert_eq!(members.len(), 1);
        let item = source.item(members[0], SyntaxKind::FnDecl, method, Visibility::Private);
        let ItemKind::Function(function) = item.kind() else {
            panic!("expected owned method");
        };
        source.name(&function.name, "marked");
        assert_eq!(item.attributes().len(), 1);
        source.written(
            item.attributes()[0].origin,
            SyntaxKind::Attribute,
            "#[method_tag]",
        );
        assert_meta(
            &source,
            &item.attributes()[0].meta,
            "method_tag",
            "method_tag",
        );
        assert!(item.attributes()[0].meta.arguments.is_empty());
    }
}
