use eglur_hir::{ItemKind, TypeRefKind, lower};
use eglur_source::{FileId, FileSpan, SourceFile};
use eglur_syntax::ast::AstNode;
use eglur_syntax::{FrontendOutcome, SyntaxKind, parse};

fn written(text: &str, span: FileSpan) -> &str {
    &text[span.range().start().as_usize()..span.range().end().as_usize()]
}

#[test]
fn type_paths_exclude_arguments_but_type_occurrences_retain_their_complete_spans() {
    let cases: &[(&str, &[&str])] = &[
        ("i64", &["i64"]),
        ("pkg::Value", &["pkg::Value"]),
        ("pkg::Box[Opt[i64]]", &["pkg::Box", "Opt", "i64"]),
        (
            "pkg /* prefix */ :: /* member */ Box [ i64 ]",
            &["pkg /* prefix */ :: /* member */ Box", "i64"],
        ),
        ("(pkg::Box[i64])", &["pkg::Box", "i64"]),
        ("&mut pkg::Box[i64]", &["pkg::Box", "i64"]),
        ("*const pkg::Box[i64]", &["pkg::Box", "i64"]),
        ("(pkg::Box[i64], Bool)", &["pkg::Box", "i64", "Bool"]),
        ("Fn(pkg::Box[i64]) -> Unit", &["pkg::Box", "i64", "Unit"]),
        ("dyn Show[i64]", &["Show", "i64"]),
        ("T::Item", &["T::Item"]),
        ("Self::Item", &["Self::Item"]),
    ];
    for (spelling, expected_paths) in cases {
        let text = format!("type Alias = {spelling}\n");
        let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parse) = parse(&source) else {
            panic!("source should reach the parser");
        };
        assert!(parse.is_success(), "{text}: {:?}", parse.diagnostics());
        let hir = lower(&parse);
        assert!(hir.invalid_occurrences().is_empty());

        let mut syntax_spans = Vec::new();
        let mut nodes = vec![parse.root().syntax().clone()];
        while let Some(node) = nodes.pop() {
            if node.kind() == SyntaxKind::PathType {
                syntax_spans.push(node.span());
            }
            nodes.extend(node.children());
        }

        let mut type_spans = Vec::new();
        let mut paths = Vec::new();
        for (_, ty) in hir.type_refs() {
            let TypeRefKind::Path { path, .. } = ty.kind() else {
                continue;
            };
            type_spans.push(hir.origin(ty.origin()).source());
            let span = hir.origin(path.origin()).source();
            let first = hir.origin(path.segments()[0].origin()).source();
            let last = hir.origin(path.last().unwrap().origin()).source();
            assert_eq!(span.file(), source.id());
            assert_eq!(span.range().start(), first.range().start(), "{text}");
            assert_eq!(span.range().end(), last.range().end(), "{text}");
            paths.push(written(&text, span));
            for name in path.segments() {
                assert_eq!(
                    written(&text, hir.origin(name.origin()).source()),
                    name.text().unwrap()
                );
            }
        }

        syntax_spans.sort_by_key(|span| (span.range().start(), span.range().end()));
        type_spans.sort_by_key(|span| (span.range().start(), span.range().end()));
        assert_eq!(type_spans, syntax_spans, "{text}");
        paths.sort_unstable();
        let mut expected = expected_paths.to_vec();
        expected.sort_unstable();
        assert_eq!(paths, expected, "{text}");
    }
}

#[test]
fn invalid_type_recovery_preserves_causes_and_following_type_paths() {
    for invalid in ["Box[]", "Box[i64, &]", "Box[i64 i64]"] {
        let text = format!("type Broken = {invalid}\ntype After = pkg::Box[i64]\n");
        let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parse) = parse(&source) else {
            panic!("source should reach the parser");
        };
        assert!(!parse.is_success());
        let hir = lower(&parse);
        assert!(!hir.invalid_occurrences().is_empty());
        assert_eq!(
            hir.syntax_causes()
                .map(|(_, span)| span)
                .collect::<Vec<_>>(),
            parse
                .diagnostics()
                .iter()
                .map(|d| d.span())
                .collect::<Vec<_>>()
        );

        let alias = hir
            .items()
            .find_map(|(_, item)| match item.kind() {
                ItemKind::TypeAlias(alias) if alias.name.text() == Some("After") => Some(alias),
                _ => None,
            })
            .expect("the following declaration survives recovery");
        let ty = hir.type_ref(alias.value);
        let TypeRefKind::Path { path, arguments } = ty.kind() else {
            panic!("following alias must retain its path type");
        };
        assert_eq!(arguments.len(), 1);
        assert_eq!(
            written(&text, hir.origin(path.origin()).source()),
            "pkg::Box"
        );
        assert_eq!(
            written(&text, hir.origin(ty.origin()).source()),
            " pkg::Box[i64]"
        );
    }
}
