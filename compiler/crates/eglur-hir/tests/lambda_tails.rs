use eglur_hir::{ExprKind, HirFile, ItemKind, LambdaExpr, StmtKind, lower};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxKind, SyntaxNode, parse};

fn parse_text(text: &str) -> Parse {
    let source = SourceFile::new(FileId::new(0), text.as_bytes().to_vec());
    let FrontendOutcome::Parsed(parse) = parse(&source) else {
        panic!("test source should reach the parser");
    };
    assert_eq!(parse.syntax().text(), text);
    parse
}

fn lower_text(text: &str) -> (Parse, HirFile) {
    let parse = parse_text(text);
    assert!(parse.is_success(), "{text}: {:?}", parse.diagnostics());

    let hir = lower(&parse);
    assert!(hir.invalid_occurrences().is_empty(), "{text}");
    (parse, hir)
}

fn nodes_of_kind(parse: &Parse, kind: SyntaxKind) -> Vec<SyntaxNode> {
    let mut pending = vec![parse.syntax()];
    let mut found = Vec::new();
    while let Some(node) = pending.pop() {
        pending.extend(node.children().collect::<Vec<_>>().into_iter().rev());
        if node.kind() == kind {
            found.push(node);
        }
    }
    found
}

fn single_lambda(hir: &HirFile) -> &LambdaExpr {
    let mut lambdas = hir
        .bodies()
        .flat_map(|(_, body)| body.expressions())
        .filter_map(|(_, expression)| match expression.kind() {
            ExprKind::Lambda(lambda) => Some(lambda),
            _ => None,
        });
    let lambda = lambdas.next().expect("source should contain a lambda");
    assert!(lambdas.next().is_none());
    lambda
}

#[test]
fn closure_tails_survive_trailing_newlines_and_comments() {
    let openings = [
        "let callback = { |value: i64|\nlet copy = value\n",
        "consume { |value: i64|\nlet copy = value\n",
        "consume { ||\nlet copy = 1\n",
        "consume { | |\nlet copy = 1\n",
        "spawn! {\nlet copy = 1\n",
    ];
    let endings = [
        "",
        " ",
        "\n",
        "\n\n",
        " // result\n",
        " /* result */ ",
        " /* result\ncomment */\n",
        "\r\n",
        "\r\n\r\n// result\r\n",
    ];

    for opening in openings {
        for ending in endings {
            let text = format!("fn main!() -> Unit {{\n{opening}copy{ending}}}\nyield ()\n}}\n");
            let (parse, hir) = lower_text(&text);
            let bodies = nodes_of_kind(&parse, SyntaxKind::LambdaBody);
            assert_eq!(bodies.len(), 1);
            let children = bodies[0].children().collect::<Vec<_>>();
            assert_eq!(
                children.iter().map(SyntaxNode::kind).collect::<Vec<_>>(),
                [SyntaxKind::LetStmt, SyntaxKind::NameExpr],
                "{text}"
            );

            let lambda = single_lambda(&hir);
            assert_eq!(lambda.statements.len(), 1);
            let statement = lambda.statements[0];
            assert_eq!(statement.body(), lambda.body);
            let statement = hir.body(lambda.body).statement(statement).unwrap();
            assert!(matches!(statement.kind(), StmtKind::Let { .. }));
            assert_eq!(hir.origin(statement.origin()).source(), children[0].span());

            let tail = lambda.tail.expect("final expression should remain a tail");
            assert_eq!(tail.body(), lambda.body);
            let tail = hir.body(lambda.body).expression(tail).unwrap();
            let ExprKind::Name(path) = tail.kind() else {
                panic!("lambda tail should retain the copy binding");
            };
            assert_eq!(path.last().unwrap().text(), Some("copy"));
            assert_eq!(hir.origin(tail.origin()).source(), children[1].span());
        }
    }
}

#[test]
fn only_the_last_expression_becomes_a_lambda_tail() {
    let (parse, hir) =
        lower_text("fn main() -> Unit {\nlet callback = { ||\n1\n-2\n3\n}\nyield ()\n}\n");
    let bodies = nodes_of_kind(&parse, SyntaxKind::LambdaBody);
    assert_eq!(
        bodies[0]
            .children()
            .map(|node| node.kind())
            .collect::<Vec<_>>(),
        [
            SyntaxKind::ExprStmt,
            SyntaxKind::ExprStmt,
            SyntaxKind::LiteralExpr
        ]
    );

    let lambda = single_lambda(&hir);
    assert_eq!(lambda.statements.len(), 2);
    let body = hir.body(lambda.body);
    for (statement, expected) in lambda.statements.iter().zip(["1", "-2"]) {
        let StmtKind::Expression(value) = body.statement(*statement).unwrap().kind() else {
            panic!("non-final expression should remain a statement");
        };
        let ExprKind::Literal(literal) = body.expression(*value).unwrap().kind() else {
            panic!("expression order should retain each integer");
        };
        assert_eq!(literal.spelling(), expected);
    }
    let ExprKind::Literal(literal) = body.expression(lambda.tail.unwrap()).unwrap().kind() else {
        panic!("last integer should be the lambda tail");
    };
    assert_eq!(literal.spelling(), "3");
}

#[test]
fn statement_only_lambda_bodies_do_not_acquire_a_tail() {
    for (statements, kinds) in [
        ("", vec![]),
        ("let value = 1\n", vec![SyntaxKind::LetStmt]),
        ("_ = 1\n", vec![SyntaxKind::AssignStmt]),
        ("return 1\n", vec![SyntaxKind::ReturnStmt]),
        ("while false {}\n", vec![SyntaxKind::WhileStmt]),
        ("for value in values {}\n", vec![SyntaxKind::ForStmt]),
        ("break\n", vec![SyntaxKind::BreakStmt]),
        ("continue\n", vec![SyntaxKind::ContinueStmt]),
    ] {
        let text =
            format!("fn main() -> Unit {{\nlet callback = {{ ||\n{statements}}}\nyield ()\n}}\n");
        let (parse, hir) = lower_text(&text);
        let bodies = nodes_of_kind(&parse, SyntaxKind::LambdaBody);
        assert_eq!(
            bodies[0]
                .children()
                .map(|node| node.kind())
                .collect::<Vec<_>>(),
            kinds,
            "{text}"
        );
        let lambda = single_lambda(&hir);
        assert_eq!(lambda.statements.len(), kinds.len(), "{text}");
        assert_eq!(lambda.tail, None, "{text}");
    }
}

#[test]
fn ordinary_blocks_still_discard_their_final_expressions() {
    let (_, hir) = lower_text(concat!(
        "fn ordinary() -> Unit {\n1\n}\n",
        "fn main() -> Unit {\nlet callback = { ||\n{\n2\n}\n}\nyield ()\n}\n",
    ));
    let ItemKind::Function(ordinary) = hir.item(hir.roots()[0]).kind() else {
        panic!("first declaration should be a function");
    };
    let eglur_hir::FunctionBody::Block(ordinary_root) = ordinary.body_form else {
        panic!("ordinary function should retain block-body form");
    };
    let lambda = single_lambda(&hir);
    assert!(lambda.statements.is_empty());

    for (root, expected) in [(ordinary_root, "1"), (lambda.tail.unwrap(), "2")] {
        let body = hir.body(root.body());
        let ExprKind::Block { statements } = body.expression(root).unwrap().kind() else {
            panic!("expression should retain an ordinary block");
        };
        assert_eq!(statements.len(), 1);
        let StmtKind::Expression(value) = body.statement(statements[0]).unwrap().kind() else {
            panic!("ordinary block must not invent yield");
        };
        let ExprKind::Literal(literal) = body.expression(*value).unwrap().kind() else {
            panic!("ordinary block should retain its discarded integer");
        };
        assert_eq!(literal.spelling(), expected);
    }
}

#[test]
fn nested_lambda_tails_keep_their_own_body_identity() {
    let (_, hir) = lower_text(concat!(
        "fn main() -> Unit {\nlet callback = { |outer|\n",
        "let inner = { |item|\nlet copy = item\ncopy\n}\ninner\n",
        "}\nyield ()\n}\n",
    ));
    let lambdas = hir
        .bodies()
        .flat_map(|(_, body)| body.expressions())
        .filter_map(|(_, expression)| match expression.kind() {
            ExprKind::Lambda(lambda) => Some(lambda),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lambdas.len(), 2);
    assert_ne!(lambdas[0].body, lambdas[1].body);

    let mut tails = Vec::new();
    for lambda in lambdas {
        assert_eq!(lambda.statements.len(), 1);
        assert_eq!(lambda.statements[0].body(), lambda.body);
        let tail = lambda.tail.expect("each lambda should retain its own tail");
        assert_eq!(tail.body(), lambda.body);
        let ExprKind::Name(path) = hir.body(lambda.body).expression(tail).unwrap().kind() else {
            panic!("each tail should be a binding name");
        };
        tails.push(path.last().unwrap().text().unwrap());
    }
    tails.sort_unstable();
    assert_eq!(tails, ["copy", "inner"]);
}

#[test]
fn lambda_tail_continuations_remain_one_expression() {
    for (expression, kind) in [
        ("1 +\n2", SyntaxKind::BinaryExpr),
        ("(1\n+ 2)", SyntaxKind::GroupExpr),
        ("value\n.to_str()", SyntaxKind::CallExpr),
        ("if true { yield 1 } else { yield 2 }", SyntaxKind::IfExpr),
    ] {
        let text = format!(
            "fn main() -> Unit {{\nlet callback = {{ |value|\n{expression}\n}}\nyield ()\n}}\n"
        );
        let (parse, hir) = lower_text(&text);
        let bodies = nodes_of_kind(&parse, SyntaxKind::LambdaBody);
        let children = bodies[0].children().collect::<Vec<_>>();
        assert_eq!(children.len(), 1, "{text}");
        assert_eq!(children[0].kind(), kind, "{text}");

        let lambda = single_lambda(&hir);
        assert!(lambda.statements.is_empty(), "{text}");
        let tail = hir
            .body(lambda.body)
            .expression(lambda.tail.unwrap())
            .unwrap();
        assert_eq!(hir.origin(tail.origin()).source(), children[0].span());
    }
}

#[test]
fn unfinished_lambda_bodies_retain_lossless_failure_provenance() {
    for (text, has_following) in [
        ("fn broken() -> Unit = { || 1\n", false),
        (
            "fn broken() -> Unit = { || 1 +\n}\nfn following() -> Unit = ()\n",
            true,
        ),
    ] {
        let parse = parse_text(text);
        assert!(!parse.is_success(), "{text}");
        assert!(!parse.diagnostics().is_empty());
        let hir = lower(&parse);
        assert!(!hir.invalid_occurrences().is_empty(), "{text}");
        assert_eq!(
            hir.syntax_causes()
                .map(|(_, span)| span)
                .collect::<Vec<_>>(),
            parse
                .diagnostics()
                .iter()
                .map(|diagnostic| diagnostic.span())
                .collect::<Vec<_>>(),
            "{text}"
        );
        if has_following {
            assert!(hir.items().any(|(_, item)| {
                matches!(item.kind(), ItemKind::Function(function) if function.name.text() == Some("following"))
            }));
        }
    }
}
