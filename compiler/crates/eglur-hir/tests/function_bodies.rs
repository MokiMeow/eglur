use eglur_hir::{ExprKind, FunctionBody, HirFile, ItemKind, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::ast::AstNode;
use eglur_syntax::{FrontendOutcome, Parse, SyntaxKind, parse};

fn lower_text(text: &str) -> (Parse, HirFile) {
    let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parse) = parse(&source) else {
        panic!("test source should reach the parser");
    };
    assert!(parse.is_success(), "{text}: {:?}", parse.diagnostics());

    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty(), "{text}");
    (parse, hir)
}

#[test]
fn declaration_equals_distinguishes_expression_blocks_in_every_function_owner() {
    let owners = [
        ("fn value() -> i64 ", "\n"),
        ("pub fn value() -> i64 ", "\n"),
        ("#[note(value = 1)]\nfn value() -> i64 ", "\n"),
        ("struct S { fn value() -> i64 ", "\n}\n"),
        ("trait T { fn value() -> i64 ", "\n}\n"),
        ("impl T for S { fn value() -> i64 ", "\n}\n"),
        (
            "struct S { prop item: i64 { fn get(self: &Self) -> i64 ",
            "\n} }\n",
        ),
        (
            "trait T { prop item: i64 { fn get(self: &Self) -> i64 ",
            "\n} }\n",
        ),
    ];
    let block = "{ let value = 7\n yield value }";
    for (prefix, suffix) in owners {
        for (separator, expression_body) in [
            ("", false),
            ("/* = */ ", false),
            ("= ", true),
            ("=\n/* body */ ", true),
        ] {
            let text = format!("{prefix}{separator}{block}{suffix}");
            let (parse, hir) = lower_text(&text);
            let mut nodes = vec![parse.root().syntax().clone()];
            let mut written_blocks = Vec::new();
            while let Some(node) = nodes.pop() {
                if node.kind() == SyntaxKind::BlockExpr {
                    written_blocks.push(node.span());
                }
                nodes.extend(node.children());
            }
            assert_eq!(written_blocks.len(), 1);

            let functions = hir
                .items()
                .filter_map(|(_, item)| match item.kind() {
                    ItemKind::Function(function) => Some(function),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(functions.len(), 1);
            let function = functions[0];
            let root = match (expression_body, function.body_form) {
                (true, FunctionBody::Expression(root)) | (false, FunctionBody::Block(root)) => root,
                _ => panic!("wrong body form for {text}: {:?}", function.body_form),
            };

            assert_eq!(root.body(), function.body);
            let root = hir.body(function.body).expression(root).unwrap();
            assert!(matches!(root.kind(), ExprKind::Block { .. }));
            assert_eq!(hir.origin(root.origin()).source(), written_blocks[0]);
        }
    }
}

#[test]
fn trait_method_absence_and_nonblock_expression_bodies_remain_distinct() {
    let (_, hir) = lower_text(concat!(
        "trait T {\n",
        "fn required() -> i64\n",
        "fn literal() -> i64 = 7\n",
        "fn grouped() -> i64 = ({ yield 7 })\n",
        "fn conditional() -> i64 = if true { yield 7 } else { yield 0 }\n",
        "}\n",
    ));
    let mut checked = 0;
    for (_, item) in hir.items() {
        let ItemKind::Function(function) = item.kind() else {
            continue;
        };
        checked += 1;
        if function.name.text() == Some("required") {
            assert_eq!(function.body_form, FunctionBody::Absent);
            assert_eq!(hir.body(function.body).expressions().count(), 0);
            continue;
        }
        let FunctionBody::Expression(root) = function.body_form else {
            panic!("method with = should retain its expression-body form");
        };
        let expression = hir.body(function.body).expression(root).unwrap();
        match function.name.text().unwrap() {
            "literal" => assert!(matches!(expression.kind(), ExprKind::Literal(_))),
            "grouped" => assert!(matches!(expression.kind(), ExprKind::Group(_))),
            "conditional" => assert!(matches!(expression.kind(), ExprKind::If(_))),
            name => panic!("unexpected method {name}"),
        }
    }
    assert_eq!(checked, 4);
}
