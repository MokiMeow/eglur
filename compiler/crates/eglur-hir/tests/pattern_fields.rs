use eglur_hir::{
    ExprKind, FieldPattern, FunctionBody, HirFile, ItemKind, LiteralKind, Name, OriginId, Path,
    PatternElement, PatternId, PatternKind, PatternPayload, SourceOrigin, lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::ast::{AstNode, Pattern as SyntaxPattern};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, SyntaxNode, parse};

#[path = "pattern_fields/composition.rs"]
mod composition;
#[path = "pattern_fields/payloads.rs"]
mod payloads;

struct LoweredPattern {
    parsed: Parse,
    hir: HirFile,
    root: PatternId,
}

impl LoweredPattern {
    fn new(pattern: &str) -> Self {
        let text =
            format!("fn inspect(input: Input) -> Unit = match input {{ {pattern} => () }}\n");
        let source = SourceFile::new(FileId::new(31), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8 source should reach the parser");
        };
        assert!(parsed.is_success(), "{pattern}: {:?}", parsed.diagnostics());
        assert_eq!(parsed.syntax().text(), text);
        let hir = lower(&parsed);
        assert!(hir.invalid_occurrences().is_empty());

        let [root] = hir.roots() else {
            panic!("expected one declaration");
        };
        let ItemKind::Function(function) = hir.item(*root).kind() else {
            panic!("expected a function");
        };
        assert_eq!(function.name.text(), Some("inspect"));
        let FunctionBody::Expression(expression) = function.body_form else {
            panic!("expected an expression body");
        };
        let body = hir.body(function.body);
        let ExprKind::Match(matched) = body.expression(expression).unwrap().kind() else {
            panic!("expected a match expression");
        };
        let [arm] = matched.arms.as_ref() else {
            panic!("expected one match arm");
        };
        assert_eq!(arm.pattern.body(), function.body);
        let root = arm.pattern;
        Self { parsed, hir, root }
    }

    fn node(&self, kind: SyntaxKind, spelling: &str) -> SyntaxNode {
        let mut pending = vec![self.parsed.syntax()];
        let mut matches = Vec::new();
        while let Some(node) = pending.pop() {
            if node.kind() == kind && node.text().trim() == spelling {
                matches.push(node.clone());
            }
            pending.extend(node.children());
        }
        assert_eq!(matches.len(), 1, "unique {kind:?}: {spelling}");
        matches.pop().unwrap()
    }

    fn kind(&self, id: PatternId, syntax: SyntaxKind, spelling: &str) -> &PatternKind {
        assert_eq!(id.body(), self.root.body());
        let pattern = self.hir.body(self.root.body()).pattern(id).unwrap();
        let node = self.node(syntax, spelling);
        assert_eq!(
            self.hir.origin(pattern.origin()),
            SourceOrigin::Written(node.span())
        );
        assert_eq!(node.span().file(), self.hir.file());
        let typed = SyntaxPattern::cast(node.clone()).expect("pattern syntax facade");
        assert_eq!(typed.syntax().kind(), node.kind());
        assert_eq!(typed.syntax().span(), node.span());
        assert_eq!(typed.syntax().text(), node.text());
        pattern.kind()
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

    fn name(&self, name: &Name, spelling: &str) {
        assert_eq!(name.text(), Some(spelling));
        assert!(!name.is_bang());
        self.written(name.origin(), SyntaxKind::Identifier, spelling);
    }

    fn path(&self, path: &Path, spelling: &str) {
        assert_eq!(path.text().as_deref(), Some(spelling));
        let text = self.parsed.syntax().text();
        let positions: Vec<_> = text
            .match_indices(spelling)
            .map(|(start, _)| start)
            .collect();
        let [start] = positions.as_slice() else {
            panic!("expected one written path {spelling}");
        };
        let SourceOrigin::Written(span) = self.hir.origin(path.origin()) else {
            panic!("path origin should be written");
        };
        assert_eq!(span.file(), self.hir.file());
        assert_eq!(span.range().start().as_usize(), *start);
        assert_eq!(span.range().end().as_usize(), start + spelling.len());

        let expected: Vec<_> = spelling.split("::").collect();
        assert_eq!(path.segments().len(), expected.len());
        for (name, spelling) in path.segments().iter().zip(expected) {
            self.name(name, spelling);
        }
    }

    fn binding(&self, id: PatternId, spelling: &str) {
        let PatternKind::Binding(name) = self.kind(id, SyntaxKind::BindingPattern, spelling) else {
            panic!("expected binding {spelling}");
        };
        self.name(name, spelling);
    }

    fn alias(&self, id: PatternId, spelling: &str, binding: &str) -> PatternId {
        let PatternKind::Alias { name, pattern } =
            self.kind(id, SyntaxKind::AliasPattern, spelling)
        else {
            panic!("expected alias {spelling}");
        };
        self.name(name, binding);
        *pattern
    }

    fn group(&self, id: PatternId, spelling: &str) -> PatternId {
        let PatternKind::Group(inner) = self.kind(id, SyntaxKind::GroupPattern, spelling) else {
            panic!("expected group {spelling}");
        };
        *inner
    }

    fn literal(&self, id: PatternId, spelling: &str, kind: LiteralKind, token: SyntaxKind) {
        let PatternKind::Literal(literal) = self.kind(id, SyntaxKind::LiteralPattern, spelling)
        else {
            panic!("expected literal {spelling}");
        };
        assert_eq!(literal.kind(), kind);
        assert_eq!(literal.spelling(), spelling);
        self.written(literal.origin(), token, spelling);
    }

    fn tuple(&self, id: PatternId, spelling: &str) -> &[PatternElement] {
        let PatternKind::Tuple(elements) = self.kind(id, SyntaxKind::TuplePattern, spelling) else {
            panic!("expected tuple {spelling}");
        };
        elements
    }

    fn variant(
        &self,
        id: PatternId,
        syntax: SyntaxKind,
        spelling: &str,
        owner: &str,
    ) -> &PatternPayload {
        let PatternKind::Variant { path, payload } = self.kind(id, syntax, spelling) else {
            panic!("expected variant {spelling}");
        };
        self.path(path, owner);
        payload
    }

    fn field(&self, field: &FieldPattern, name: &str, spelling: &str) -> Option<PatternId> {
        let FieldPattern::Field {
            name: actual,
            pattern,
            origin,
        } = field
        else {
            panic!("expected named field {spelling}");
        };
        self.name(actual, name);
        self.written(*origin, SyntaxKind::FieldPattern, spelling);
        *pattern
    }
}

fn child(element: &PatternElement) -> PatternId {
    let PatternElement::Pattern(id) = element else {
        panic!("expected a positional pattern child");
    };
    *id
}
