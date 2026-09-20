use eglur_hir::{ImportSource, ItemKind, Visibility};
use eglur_syntax::SyntaxKind;

use super::LoweredDeclarations;

#[test]
fn module_and_single_imports_keep_paths_aliases_and_bang_names_separate() {
    let declarations = [
        "pub import read_ns::read_leaf as local_read",
        "import write_ns::save! as store!",
        "import plain_ns::plain_leaf",
    ];
    let source = LoweredDeclarations::valid(&format!(
        "module module_root::module_leaf\n{}\n",
        declarations.join("\n"),
    ));
    let module = source.hir.module().expect("written module declaration");
    source.written(
        module.origin(),
        SyntaxKind::ModuleDecl,
        "module module_root::module_leaf",
    );
    source.path(module.path(), "module_root::module_leaf");
    assert_eq!(source.hir.roots().len(), declarations.len());

    for (index, path, alias, visibility) in [
        (
            0,
            "read_ns::read_leaf",
            Some("local_read"),
            Visibility::Public,
        ),
        (1, "write_ns::save!", Some("store!"), Visibility::Private),
        (2, "plain_ns::plain_leaf", None, Visibility::Private),
    ] {
        let item = source.item(
            source.hir.roots()[index],
            SyntaxKind::ImportDecl,
            declarations[index],
            visibility,
        );
        assert!(item.attributes().is_empty());
        let ItemKind::Import(import) = item.kind() else {
            panic!("expected import");
        };
        let ImportSource::Single {
            path: actual,
            alias: actual_alias,
        } = &import.source
        else {
            panic!("single import must not become a group");
        };
        source.path(actual, path);
        match (actual_alias, alias) {
            (Some(actual), Some(expected)) => source.name(actual, expected),
            (None, None) => {}
            _ => panic!("import alias presence changed"),
        }
    }
}

#[test]
fn grouped_imports_preserve_entry_order_alias_pairing_and_empty_groups() {
    let spelling = "pub import group_ns::api::{Zulu as LocalZ, bump! as raise!, Alpha,}";
    let source = LoweredDeclarations::valid(&format!("{spelling}\nimport empty_ns::{{}}\n"));
    assert!(source.hir.module().is_none());
    assert_eq!(source.hir.roots().len(), 2);
    let item = source.item(
        source.hir.roots()[0],
        SyntaxKind::ImportDecl,
        spelling,
        Visibility::Public,
    );
    let ItemKind::Import(import) = item.kind() else {
        panic!("expected grouped import");
    };
    let ImportSource::Group { prefix, entries } = &import.source else {
        panic!("expected group");
    };
    source.path(prefix, "group_ns::api");
    assert_eq!(entries.len(), 3);
    for (entry, name, alias, spelling) in [
        (&entries[0], "Zulu", Some("LocalZ"), "Zulu as LocalZ"),
        (&entries[1], "bump!", Some("raise!"), "bump! as raise!"),
        (&entries[2], "Alpha", None, "Alpha"),
    ] {
        source.name(&entry.name, name);
        source.written(entry.origin, SyntaxKind::ImportEntry, spelling);
        match (&entry.alias, alias) {
            (Some(actual), Some(expected)) => source.name(actual, expected),
            (None, None) => {}
            _ => panic!("grouped alias pairing changed"),
        }
    }

    let item = source.item(
        source.hir.roots()[1],
        SyntaxKind::ImportDecl,
        "import empty_ns::{}",
        Visibility::Private,
    );
    let ItemKind::Import(import) = item.kind() else {
        panic!("expected empty import");
    };
    let ImportSource::Group { prefix, entries } = &import.source else {
        panic!("empty group is not a single import");
    };
    source.path(prefix, "empty_ns");
    assert!(entries.is_empty());
}

#[test]
fn relative_import_heads_remain_part_of_the_written_path() {
    for path in ["self::sibling::Thing", "super::parent::Other"] {
        let spelling = format!("import {path}");
        let source = LoweredDeclarations::valid(&format!("{spelling}\n"));
        assert_eq!(source.hir.roots().len(), 1);
        let item = source.item(
            source.hir.roots()[0],
            SyntaxKind::ImportDecl,
            &spelling,
            Visibility::Private,
        );
        let ItemKind::Import(import) = item.kind() else {
            panic!("expected relative import");
        };
        let ImportSource::Single {
            path: actual,
            alias,
        } = &import.source
        else {
            panic!("expected single path");
        };
        source.path(actual, path);
        assert!(alias.is_none());
    }
}
