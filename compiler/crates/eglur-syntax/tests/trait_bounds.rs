use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxDiagnosticKind, SyntaxKind, SyntaxNode, parse};

const DECLARATIONS: [&str; 10] = [
    "fn sample[PARAMS]() -> Unit = ()",
    "struct Container[PARAMS] {}",
    "enum Choice[PARAMS] {}",
    "trait Contract[PARAMS] {}",
    "impl[PARAMS] Contract for Container[T] {}",
    "type Alias[PARAMS] = T",
    "trait Host { fn method[PARAMS]() -> Unit\n}",
    "struct Host { fn method[PARAMS]() -> Unit = ()\n}",
    "impl Contract for Host { fn method[PARAMS]() -> Unit = ()\n}",
    "struct Host { prop count: i64 { fn get_count[PARAMS](self: &Self) -> i64 = 0\n} }",
];

fn parse_text(text: &str) -> Parse {
    let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("test source should reach the parser");
    };
    assert_eq!(parsed.syntax().text(), text);
    parsed
}

fn generic_parameters(parsed: &Parse) -> Vec<SyntaxNode> {
    let mut pending = vec![parsed.syntax()];
    while let Some(node) = pending.pop() {
        if node.kind() == SyntaxKind::GenericParameterList {
            return node.children().collect();
        }
        pending.extend(node.children());
    }
    panic!("declaration should retain its generic parameter list");
}

#[test]
fn inline_bounds_reject_non_path_heads_in_every_declaration_owner() {
    for declaration in DECLARATIONS {
        for bound in [
            "&Show",
            "&mut Show",
            "*const Show",
            "*mut Show",
            "(Show)",
            "(Show, Other)",
            "Fn(i64) -> Unit",
            "dyn Show",
        ] {
            for parameters in [
                format!("T: {bound}"),
                format!("T: Show + {bound}, U: Other"),
            ] {
                let rejected = declaration.replace("PARAMS", &parameters);
                let text = format!("{rejected}\nfn following() -> Unit = ()\n");
                let parsed = parse_text(&text);
                assert!(!parsed.is_success(), "non-path bound was accepted: {text}");
                assert_eq!(parsed.diagnostics().len(), 1, "{text}");
                let diagnostic = parsed.diagnostics()[0];
                assert_eq!(
                    diagnostic.kind(),
                    SyntaxDiagnosticKind::ParseUnexpectedToken,
                    "{text}"
                );

                let span = diagnostic.span().range();
                let bound_start = text.find(&format!(": {bound}")).map_or_else(
                    || text.find(&format!("+ {bound}")).unwrap() + 2,
                    |at| at + 2,
                );
                assert_eq!(span.start().as_usize(), bound_start, "{text}");
                let head = match bound {
                    "Fn(i64) -> Unit" => "Fn",
                    "dyn Show" => "dyn",
                    _ => &bound[..1],
                };
                assert_eq!(&text[span.start().as_usize()..span.end().as_usize()], head);

                let following = parsed.syntax().children().last().unwrap();
                assert_eq!(following.kind(), SyntaxKind::FnDecl, "{text}");
                assert_eq!(following.text().trim(), "fn following() -> Unit = ()");
            }
        }
    }
}

#[test]
fn nominal_bounds_keep_type_wrappers_order_and_nested_type_arguments() {
    for declaration in DECLARATIONS {
        for first in [
            "Show",
            "pkg::Show",
            "pkg::Show[A, B]",
            "Fn",
            "Fn[A]",
            "pkg::Fn[A]",
            "Show[Fn(A, B) -> C, &A, *const B, dyn Other, (A, B)]",
            "Show[\nA,\nB,\n]",
        ] {
            let parameters = format!("T: {first} + pkg::Other[T], U: Final");
            let text = format!("{}\n", declaration.replace("PARAMS", &parameters));
            let parsed = parse_text(&text);
            assert!(parsed.is_success(), "{text}: {:?}", parsed.diagnostics());

            let parameters = generic_parameters(&parsed);
            assert_eq!(parameters.len(), 2, "{text}");
            for (parameter, expected) in parameters
                .iter()
                .zip([vec![first, "pkg::Other[T]"], vec!["Final"]])
            {
                let bounds = parameter
                    .children()
                    .find(|node| node.kind() == SyntaxKind::TraitBoundList)
                    .unwrap();
                let types = bounds.children().collect::<Vec<_>>();
                assert_eq!(types.len(), expected.len(), "{text}");
                for (ty, expected) in types.iter().zip(expected) {
                    assert_eq!(ty.kind(), SyntaxKind::TypeExpr);
                    assert_eq!(ty.text().trim(), expected);
                    let path = ty.children().collect::<Vec<_>>();
                    assert_eq!(path.len(), 1);
                    assert_eq!(path[0].kind(), SyntaxKind::PathType);
                    assert_eq!(path[0].text().trim(), expected);
                }
            }
        }
    }
}

#[test]
fn ordinary_types_and_impl_trait_diagnostic_surface_are_unchanged() {
    for ty in [
        "&Show",
        "&mut Show",
        "*const Show",
        "*mut Show",
        "(Show)",
        "(Show, Other)",
        "Fn(i64) -> Unit",
        "dyn Show",
    ] {
        for text in [
            format!("type Alias = {ty}\n"),
            format!("fn sample(value: {ty}) -> {ty} = value\n"),
            format!("extern \"C\" fn sample(value: {ty}) -> {ty}\n"),
            format!("impl {ty} for Host {{}}\n"),
            format!("fn sample[T: Show[{ty}]]() -> Unit = ()\n"),
        ] {
            let parsed = parse_text(&text);
            assert!(parsed.is_success(), "{text}: {:?}", parsed.diagnostics());
        }
    }
}

#[test]
fn missing_bounds_still_recover_at_the_following_top_level_declaration() {
    for parameters in ["T:", "T: Show +", "T: , U: Other", "T: Show +, U: Other"] {
        let text = format!("fn sample[{parameters}]() -> Unit = ()\nfn following() -> Unit = ()\n");
        let parsed = parse_text(&text);
        assert_eq!(parsed.diagnostics().len(), 1, "{text}");
        assert_eq!(
            parsed.diagnostics()[0].kind(),
            SyntaxDiagnosticKind::ParseUnexpectedToken
        );
        let following = parsed.syntax().children().last().unwrap();
        assert_eq!(following.kind(), SyntaxKind::FnDecl);
        assert_eq!(following.text().trim(), "fn following() -> Unit = ()");
    }
}
