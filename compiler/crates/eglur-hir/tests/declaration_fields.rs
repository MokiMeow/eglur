use eglur_hir::{
    BodyId, ExprId, ExprKind, GenericParameter, HirFile, Item, ItemId, Literal, LiteralKind, Name,
    OriginId, Parameter, Path, PatternKind, SourceOrigin, TypeRefId, TypeRefKind, Visibility,
    lower,
};
use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, Parse, SyntaxElement, SyntaxKind, parse};

#[path = "declaration_fields/attributes.rs"]
mod attributes;
#[path = "declaration_fields/imports.rs"]
mod imports;
#[path = "declaration_fields/members.rs"]
mod members;
#[path = "declaration_fields/recovery.rs"]
mod recovery;
#[path = "declaration_fields/signatures.rs"]
mod signatures;

struct LoweredDeclarations {
    parsed: Parse,
    hir: HirFile,
}

impl LoweredDeclarations {
    fn parse(text: &str) -> Self {
        let source = SourceFile::new(FileId::new(29), text.as_bytes().to_vec());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8 source should reach the parser");
        };
        assert_eq!(parsed.syntax().text(), text);
        let hir = lower(&parsed);
        Self { parsed, hir }
    }

    fn valid(text: &str) -> Self {
        let source = Self::parse(text);
        assert!(
            source.parsed.is_success(),
            "{text}: {:?}",
            source.parsed.diagnostics()
        );
        assert!(source.hir.invalid_occurrences().is_empty());
        source
    }

    fn item(&self, id: ItemId, kind: SyntaxKind, spelling: &str, visibility: Visibility) -> &Item {
        let item = self.hir.item(id);
        self.written(item.origin(), kind, spelling);
        assert_eq!(item.visibility(), visibility);
        item
    }

    fn name(&self, name: &Name, spelling: &str) {
        let bare = spelling.strip_suffix('!');
        assert_eq!(name.text(), Some(bare.unwrap_or(spelling)));
        assert_eq!(name.is_bang(), bare.is_some());
        let kind = if bare.is_some() {
            SyntaxKind::BangIdentifier
        } else {
            SyntaxKind::Identifier
        };
        self.written(name.origin(), kind, spelling);
    }

    fn path(&self, path: &Path, spelling: &str) {
        let names: Vec<_> = spelling.split("::").collect();
        assert_eq!(path.segments().len(), names.len());
        for (name, expected) in path.segments().iter().zip(&names) {
            self.name(name, expected);
        }
        assert_eq!(path.text().as_deref(), Some(spelling.trim_end_matches('!')),);
        self.raw_written(path.origin(), spelling);
    }

    fn ty(&self, id: TypeRefId, spelling: &str) {
        let ty = self.hir.type_ref(id);
        self.written(ty.origin(), SyntaxKind::PathType, spelling);
        let TypeRefKind::Path { path, arguments } = ty.kind() else {
            panic!("expected path type {spelling}");
        };
        assert!(arguments.is_empty());
        self.path(path, spelling);
    }

    fn generic(&self, parameter: &GenericParameter, name: &str, bounds: &[&str]) {
        self.name(&parameter.name, name);
        assert_eq!(parameter.bounds.len(), bounds.len());
        for (&id, &spelling) in parameter.bounds.iter().zip(bounds) {
            self.ty(id, spelling);
        }
        let spelling = if bounds.is_empty() {
            name.to_owned()
        } else {
            format!("{name}: {}", bounds.join(" + "))
        };
        self.written(parameter.origin, SyntaxKind::GenericParameter, &spelling);
    }

    fn parameter(&self, body: BodyId, parameter: &Parameter, name: &str, ty: &str) {
        self.written(
            parameter.origin,
            SyntaxKind::Parameter,
            &format!("{name}: {ty}"),
        );
        self.ty(parameter.ty.expect("written parameter type"), ty);
        assert_eq!(parameter.pattern.body(), body);
        let pattern = self.hir.body(body).pattern(parameter.pattern).unwrap();
        let PatternKind::Binding(binding) = pattern.kind() else {
            panic!("expected parameter binding {name}");
        };
        self.name(binding, name);
        self.written(pattern.origin(), SyntaxKind::BindingPattern, name);
    }

    fn literal(&self, literal: &Literal, kind: LiteralKind, spelling: &str) {
        assert_eq!(literal.kind(), kind);
        assert_eq!(literal.spelling(), spelling);
        let syntax = match kind {
            LiteralKind::Integer => SyntaxKind::Integer,
            LiteralKind::Float => SyntaxKind::Float,
            LiteralKind::String => SyntaxKind::String,
            LiteralKind::Bool if spelling == "true" => SyntaxKind::TrueKw,
            LiteralKind::Bool => SyntaxKind::FalseKw,
            _ => panic!("literal family outside these declaration fixtures"),
        };
        self.written(literal.origin(), syntax, spelling);
    }

    fn integer(&self, body: BodyId, id: ExprId, spelling: &str) {
        assert_eq!(id.body(), body);
        let expression = self.hir.body(body).expression(id).unwrap();
        let ExprKind::Literal(literal) = expression.kind() else {
            panic!("expected initializer literal {spelling}");
        };
        self.written(expression.origin(), SyntaxKind::LiteralExpr, spelling);
        self.literal(literal, LiteralKind::Integer, spelling);
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
        assert_eq!(spans.len(), 1, "unique {kind:?} occurrence: {spelling}");
        assert_eq!(spans[0].file(), self.hir.file());
        assert_eq!(self.hir.origin(origin), SourceOrigin::Written(spans[0]));
    }

    fn raw_written(&self, origin: OriginId, spelling: &str) {
        let text = self.parsed.syntax().text();
        let occurrences: Vec<_> = text.match_indices(spelling).collect();
        assert_eq!(occurrences.len(), 1, "unique source text: {spelling}");
        let SourceOrigin::Written(span) = self.hir.origin(origin) else {
            panic!("declaration fields must retain written origins");
        };
        assert_eq!(span.file(), self.hir.file());
        assert_eq!(span.range().start().as_usize(), occurrences[0].0);
        assert_eq!(
            span.range().end().as_usize(),
            occurrences[0].0 + spelling.len()
        );
    }
}
