use eglur_hir::{
    BodyId, CallArgument, CallExpr, CallSyntax, ExprId, ExprKind, FunctionBody, HirFile, ItemKind,
    LambdaExpr, LambdaSyntax, LiteralKind, Name, OriginId, Path, SourceOrigin, TypeRefId,
    TypeRefKind, lower,
};
use eglur_source::{ByteRange, FileId, FileSpan, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, SyntaxNode, parse};

#[path = "callable_fields/calls.rs"]
mod calls;
#[path = "callable_fields/lambdas.rs"]
mod lambdas;

struct LoweredCallable {
    parsed: Parse,
    hir: HirFile,
    root: ExprId,
}

impl LoweredCallable {
    fn new(expression: &str) -> Self {
        let text = format!("fn inspect() -> Unit = {expression}\n");
        let source = SourceFile::new(FileId::new(41), text.as_bytes().to_vec());
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

    fn node(&self, kind: SyntaxKind, spelling: &str) -> SyntaxNode {
        let mut pending = vec![self.parsed.syntax()];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            pending.extend(node.children());
            if node.kind() == kind && node.text().trim() == spelling {
                found.push(node);
            }
        }
        assert_eq!(found.len(), 1, "unique {kind:?}: {spelling}");
        found.pop().unwrap()
    }

    fn token_span(&self, kind: SyntaxKind, spelling: &str) -> FileSpan {
        let mut pending = vec![self.parsed.syntax()];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            for element in node.children_with_tokens() {
                match element {
                    SyntaxElement::Node(child) => pending.push(child),
                    SyntaxElement::Token(token)
                        if token.kind() == kind && token.text() == spelling =>
                    {
                        found.push(token.span());
                    }
                    SyntaxElement::Token(_) => {}
                }
            }
        }
        assert_eq!(found.len(), 1, "unique {kind:?}: {spelling}");
        found[0]
    }

    fn written(&self, origin: OriginId, syntax: SyntaxKind, spelling: &str) {
        let span = self.node(syntax, spelling).span();
        assert_eq!(span.file(), self.hir.file());
        assert_eq!(self.hir.origin(origin), SourceOrigin::Written(span));
    }

    fn kind(&self, owner: BodyId, id: ExprId, syntax: SyntaxKind, spelling: &str) -> &ExprKind {
        assert_eq!(id.body(), owner);
        let expression = self.hir.body(owner).expression(id).unwrap();
        self.written(expression.origin(), syntax, spelling);
        expression.kind()
    }

    fn identifier(&self, name: &Name, text: &str, bang: bool) -> FileSpan {
        assert_eq!(name.text(), Some(text));
        assert_eq!(name.is_bang(), bang);
        let span = if bang {
            self.token_span(SyntaxKind::BangIdentifier, &format!("{text}!"))
        } else {
            self.token_span(SyntaxKind::Identifier, text)
        };
        assert_eq!(self.hir.origin(name.origin()), SourceOrigin::Written(span));
        span
    }

    fn path(&self, path: &Path, names: &[&str], bang: bool) {
        assert_eq!(path.segments().len(), names.len());
        assert_eq!(path.text().as_deref(), Some(names.join("::").as_str()));
        let spans: Vec<_> = path
            .segments()
            .iter()
            .zip(names)
            .enumerate()
            .map(|(index, (name, text))| {
                self.identifier(name, text, bang && index + 1 == names.len())
            })
            .collect();
        let range = ByteRange::new(
            spans.first().unwrap().range().start(),
            spans.last().unwrap().range().end(),
        )
        .unwrap();
        assert_eq!(
            self.hir.origin(path.origin()),
            SourceOrigin::Written(FileSpan::new(self.hir.file(), range))
        );
    }

    fn name(&self, owner: BodyId, id: ExprId, text: &str, bang: bool) {
        let spelling = if bang {
            format!("{text}!")
        } else {
            text.into()
        };
        let ExprKind::Name(path) = self.kind(owner, id, SyntaxKind::NameExpr, &spelling) else {
            panic!("expected name {spelling}");
        };
        self.path(path, &[text], bang);
    }

    fn integer(&self, owner: BodyId, id: ExprId, spelling: &str) {
        let ExprKind::Literal(literal) = self.kind(owner, id, SyntaxKind::LiteralExpr, spelling)
        else {
            panic!("expected integer {spelling}");
        };
        assert_eq!(literal.kind(), LiteralKind::Integer);
        assert_eq!(literal.spelling(), spelling);
        assert_eq!(
            self.hir.origin(literal.origin()),
            SourceOrigin::Written(self.token_span(SyntaxKind::Integer, spelling))
        );
    }

    fn call(&self, owner: BodyId, id: ExprId, spelling: &str, syntax: CallSyntax) -> &CallExpr {
        let ExprKind::Call(call) = self.kind(owner, id, SyntaxKind::CallExpr, spelling) else {
            panic!("expected call {spelling}");
        };
        assert_eq!(call.syntax, syntax);
        assert!(!call.partial_application);
        call
    }

    fn argument(
        &self,
        owner: BodyId,
        argument: &CallArgument,
        spelling: &str,
        syntax: SyntaxKind,
        name: Option<&str>,
    ) -> ExprId {
        self.written(argument.origin, syntax, spelling);
        match (&argument.name, name) {
            (Some(actual), Some(expected)) => {
                self.identifier(actual, expected, false);
            }
            (None, None) => {}
            _ => panic!("argument name does not match {name:?}"),
        }
        assert_eq!(argument.value.body(), owner);
        assert!(self.hir.body(owner).expression(argument.value).is_some());
        argument.value
    }

    fn type_kind(&self, id: TypeRefId, syntax: SyntaxKind, spelling: &str) -> &TypeRefKind {
        let ty = self.hir.type_ref(id);
        self.written(ty.origin(), syntax, spelling);
        ty.kind()
    }

    fn type_path(&self, id: TypeRefId, spelling: &str, names: &[&str]) -> &[TypeRefId] {
        let TypeRefKind::Path { path, arguments } =
            self.type_kind(id, SyntaxKind::PathType, spelling)
        else {
            panic!("expected path type {spelling}");
        };
        self.path(path, names, false);
        arguments
    }

    fn type_leaf(&self, id: TypeRefId, spelling: &str) {
        assert!(self.type_path(id, spelling, &[spelling]).is_empty());
    }

    fn lambda(
        &self,
        owner: BodyId,
        id: ExprId,
        spelling: &str,
        syntax: LambdaSyntax,
    ) -> &LambdaExpr {
        let ExprKind::Lambda(lambda) = self.kind(owner, id, SyntaxKind::LambdaExpr, spelling)
        else {
            panic!("expected lambda {spelling}");
        };
        assert_eq!(lambda.syntax, syntax);
        assert_ne!(lambda.body, owner);
        let outer = self.hir.body(owner);
        let inner = self.hir.body(lambda.body);
        assert!(inner.expression(id).is_none());
        for parameter in &lambda.parameters {
            assert_eq!(parameter.pattern.body(), lambda.body);
            assert!(inner.pattern(parameter.pattern).is_some());
            assert!(outer.pattern(parameter.pattern).is_none());
        }
        for &statement in &lambda.statements {
            assert_eq!(statement.body(), lambda.body);
            assert!(inner.statement(statement).is_some());
            assert!(outer.statement(statement).is_none());
        }
        if let Some(tail) = lambda.tail {
            assert_eq!(tail.body(), lambda.body);
            assert!(inner.expression(tail).is_some());
            assert!(outer.expression(tail).is_none());
        }
        lambda
    }
}
