use eglur_hir::{
    BinaryOperator, ExprId, ExprKind, FunctionBody, HirFile, ItemKind, LiteralKind, Name, OriginId,
    SourceOrigin, StmtKind, UnaryOperator, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, parse};

#[path = "expression_fields/leaves_and_wrappers.rs"]
mod leaves_and_wrappers;
#[path = "expression_fields/operators.rs"]
mod operators;
#[path = "expression_fields/postfix.rs"]
mod postfix;

struct LoweredExpression {
    parsed: Parse,
    hir: HirFile,
    root: ExprId,
}

impl LoweredExpression {
    fn new(expression: &str) -> Self {
        let text = format!("fn inspect() -> Unit = {expression}\n");
        let source = SourceFile::new(FileId::new(37), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8 source should reach the parser");
        };
        assert!(
            parsed.is_success(),
            "{expression}: {:?}",
            parsed.diagnostics()
        );
        assert_eq!(parsed.syntax().text(), text);
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty());

        let [item] = hir.roots() else {
            panic!("expected one declaration");
        };
        let ItemKind::Function(function) = hir.item(*item).kind() else {
            panic!("expected a function");
        };
        assert_eq!(function.name.text(), Some("inspect"));
        let FunctionBody::Expression(root) = function.body_form else {
            panic!("expected an expression body");
        };
        assert_eq!(root.body(), function.body);
        Self { parsed, hir, root }
    }

    fn kind(&self, id: ExprId, syntax: SyntaxKind, spelling: &str) -> &ExprKind {
        assert_eq!(id.body(), self.root.body());
        let expression = self.hir.body(self.root.body()).expression(id).unwrap();
        self.written(expression.origin(), syntax, spelling);
        expression.kind()
    }

    fn written(&self, origin: OriginId, kind: SyntaxKind, spelling: &str) {
        let mut pending = vec![self.parsed.syntax()];
        let mut spans = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == kind && node.text().trim() == spelling {
                spans.push(node.span());
            }
            for element in node.children_with_tokens() {
                match element {
                    SyntaxElement::Node(child) => pending.push(child),
                    SyntaxElement::Token(token)
                        if token.kind() == kind && token.text() == spelling =>
                    {
                        spans.push(token.span());
                    }
                    SyntaxElement::Token(_) => {}
                }
            }
        }
        assert_eq!(spans.len(), 1, "unique {kind:?}: {spelling}");
        assert_eq!(self.hir.origin(origin), SourceOrigin::Written(spans[0]));
        assert_eq!(spans[0].file(), self.hir.file());
    }

    fn identifier(&self, name: &Name, text: &str, bang: bool) {
        assert_eq!(name.text(), Some(text));
        assert_eq!(name.is_bang(), bang);
        if bang {
            self.written(
                name.origin(),
                SyntaxKind::BangIdentifier,
                &format!("{text}!"),
            );
        } else {
            self.written(name.origin(), SyntaxKind::Identifier, text);
        }
    }

    fn name(&self, id: ExprId, text: &str, bang: bool) {
        let spelling = if bang {
            format!("{text}!")
        } else {
            text.into()
        };
        let ExprKind::Name(path) = self.kind(id, SyntaxKind::NameExpr, &spelling) else {
            panic!("expected name {spelling}");
        };
        let [name] = path.segments() else {
            panic!("expected an unqualified name");
        };
        self.identifier(name, text, bang);
        assert_eq!(
            self.hir.origin(path.origin()),
            self.hir.origin(name.origin())
        );
    }

    fn literal(&self, id: ExprId, spelling: &str, kind: LiteralKind, token: SyntaxKind) {
        let ExprKind::Literal(literal) = self.kind(id, SyntaxKind::LiteralExpr, spelling) else {
            panic!("expected literal {spelling}");
        };
        assert_eq!(literal.kind(), kind);
        assert_eq!(literal.spelling(), spelling);
        self.written(literal.origin(), token, spelling);
    }

    fn binary(&self, id: ExprId, spelling: &str, expected: BinaryOperator) -> (ExprId, ExprId) {
        let ExprKind::Binary {
            operator,
            left,
            right,
        } = self.kind(id, SyntaxKind::BinaryExpr, spelling)
        else {
            panic!("expected binary expression {spelling}");
        };
        assert_eq!(*operator, expected);
        (*left, *right)
    }

    fn unary(&self, id: ExprId, spelling: &str, expected: UnaryOperator) -> ExprId {
        let ExprKind::Unary { operator, operand } = self.kind(id, SyntaxKind::UnaryExpr, spelling)
        else {
            panic!("expected unary expression {spelling}");
        };
        assert_eq!(*operator, expected);
        *operand
    }

    fn group(&self, id: ExprId, spelling: &str) -> ExprId {
        let ExprKind::Group(inner) = self.kind(id, SyntaxKind::GroupExpr, spelling) else {
            panic!("expected group {spelling}");
        };
        *inner
    }

    fn yielded(&self, block: ExprId, spelling: &str, statement: &str) -> ExprId {
        let ExprKind::Block { statements } = self.kind(block, SyntaxKind::BlockExpr, spelling)
        else {
            panic!("expected block {spelling}");
        };
        let [id] = statements.as_ref() else {
            panic!("expected exactly one yield statement");
        };
        assert_eq!(id.body(), self.root.body());
        let yielded = self.hir.body(self.root.body()).statement(*id).unwrap();
        self.written(yielded.origin(), SyntaxKind::YieldStmt, statement);
        let StmtKind::Yield(value) = yielded.kind() else {
            panic!("expected yield {statement}");
        };
        *value
    }
}
