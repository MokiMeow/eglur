use eglur_hir::{
    Expr, ExprId, ExprKind, FunctionBody, HirFile, ItemKind, LiteralKind, Name, OriginId,
    PatternId, PatternKind, SourceOrigin, Stmt, StmtId, StmtKind, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, parse};

#[path = "body_fields/aggregates.rs"]
mod aggregates;
#[path = "body_fields/control.rs"]
mod control;
#[path = "body_fields/statements.rs"]
mod statements;

struct LoweredBody {
    parsed: Parse,
    hir: HirFile,
    root: ExprId,
}

impl LoweredBody {
    fn new(text: &str, function_name: &str) -> Self {
        let source = SourceFile::new(FileId::new(17), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8 source should reach the parser");
        };
        assert!(parsed.is_success(), "{text}: {:?}", parsed.diagnostics());
        assert_eq!(parsed.syntax().text(), text);
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty(), "{text}");

        let function = hir
            .roots()
            .iter()
            .find_map(|&id| match hir.item(id).kind() {
                ItemKind::Function(function) if function.name.text() == Some(function_name) => {
                    Some(function)
                }
                _ => None,
            })
            .expect("source should contain the selected function");
        let (FunctionBody::Block(root) | FunctionBody::Expression(root)) = function.body_form
        else {
            panic!("selected function should have a body");
        };
        assert_eq!(root.body(), function.body);
        Self { parsed, hir, root }
    }

    fn expression(&self, id: ExprId) -> &Expr {
        assert_eq!(id.body(), self.root.body());
        self.hir.body(self.root.body()).expression(id).unwrap()
    }

    fn statement(&self, id: StmtId) -> &Stmt {
        assert_eq!(id.body(), self.root.body());
        self.hir.body(self.root.body()).statement(id).unwrap()
    }

    fn block(&self, id: ExprId) -> &[StmtId] {
        let ExprKind::Block { statements } = self.expression(id).kind() else {
            panic!("expected a block");
        };
        for &statement in statements {
            self.statement(statement);
        }
        statements
    }

    fn yielded(&self, block: ExprId) -> ExprId {
        let [statement] = self.block(block) else {
            panic!("expected exactly one yield statement");
        };
        let StmtKind::Yield(value) = self.statement(*statement).kind() else {
            panic!("expected a yield statement");
        };
        self.expression(*value);
        *value
    }

    fn integer(&self, id: ExprId, spelling: &str) {
        let expression = self.expression(id);
        let ExprKind::Literal(literal) = expression.kind() else {
            panic!("expected integer {spelling}: {:?}", expression.kind());
        };
        assert_eq!(literal.kind(), LiteralKind::Integer);
        assert_eq!(literal.spelling(), spelling);
        self.written(expression.origin(), SyntaxKind::LiteralExpr, spelling);
        self.written(literal.origin(), SyntaxKind::Integer, spelling);
    }

    fn name(&self, id: ExprId, spelling: &str) {
        let expression = self.expression(id);
        let ExprKind::Name(path) = expression.kind() else {
            panic!("expected name {spelling}: {:?}", expression.kind());
        };
        assert_eq!(path.segments().len(), 1);
        self.identifier(&path.segments()[0], spelling);
        self.written(expression.origin(), SyntaxKind::NameExpr, spelling);
    }

    fn identifier(&self, name: &Name, spelling: &str) {
        assert_eq!(name.text(), Some(spelling));
        assert!(!name.is_bang());
        self.written(name.origin(), SyntaxKind::Identifier, spelling);
    }

    fn binding(&self, id: PatternId, spelling: &str) {
        assert_eq!(id.body(), self.root.body());
        let pattern = self.hir.body(self.root.body()).pattern(id).unwrap();
        let PatternKind::Binding(name) = pattern.kind() else {
            panic!("expected binding {spelling}");
        };
        self.identifier(name, spelling);
        self.written(pattern.origin(), SyntaxKind::BindingPattern, spelling);
    }

    fn written(&self, origin: OriginId, kind: SyntaxKind, spelling: &str) {
        let mut spans = Vec::new();
        let mut pending = vec![self.parsed.syntax()];
        while let Some(node) = pending.pop() {
            if node.kind() == kind && node.text().trim() == spelling {
                spans.push(node.span());
            }
            for element in node.children_with_tokens() {
                match element {
                    SyntaxElement::Node(child) => pending.push(child),
                    SyntaxElement::Token(token)
                        if token.kind() == kind && token.text().trim() == spelling =>
                    {
                        spans.push(token.span());
                    }
                    SyntaxElement::Token(_) => {}
                }
            }
        }
        assert_eq!(
            spans.len(),
            1,
            "unique {kind:?} source occurrence: {spelling}"
        );
        assert_eq!(self.hir.origin(origin), SourceOrigin::Written(spans[0]));
        assert_eq!(spans[0].file(), self.hir.file());
    }
}
