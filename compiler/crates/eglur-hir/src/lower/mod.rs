mod body;
mod item;
mod pattern;
mod tensor;
mod ty;

use eglur_source::{ByteRange, FileId, FileSpan};
use eglur_syntax::ast::AstNode;
use eglur_syntax::{Parse, SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::{
    AxisExpr, AxisExprId, Body, BodyId, DesugaringKind, Expr, ExprId, HirFile, InvalidCategory,
    InvalidOccurrence, Item, ItemId, ModuleDecl, Name, OriginId, Path, Pattern, PatternId,
    SourceOrigin, Stmt, StmtId, SyntaxCauseId, TypeRef, TypeRefId,
};

/// Lowers one production parse into a complete source-semantic HIR file.
#[must_use]
pub fn lower(parse: &Parse) -> HirFile {
    let syntax_causes = parse
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.span())
        .collect();
    let syntax_recovery_spans = parse
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.recovery_span())
        .collect();
    let mut builder = Builder::new(parse.file(), syntax_causes, syntax_recovery_spans);
    for index in 0..parse.diagnostics().len() {
        builder.record_reported_syntax(SyntaxCauseId::from_index(index));
    }
    builder.lower_root(parse.root());
    builder.finish()
}

struct Builder {
    file: FileId,
    module: Option<ModuleDecl>,
    roots: Vec<ItemId>,
    items: Vec<Item>,
    bodies: Vec<BodyBuilder>,
    types: Vec<TypeRef>,
    origins: Vec<SourceOrigin>,
    syntax_causes: Vec<FileSpan>,
    syntax_recovery_spans: Vec<FileSpan>,
    invalid: Vec<InvalidOccurrence>,
}

impl Builder {
    fn new(
        file: FileId,
        syntax_causes: Vec<FileSpan>,
        syntax_recovery_spans: Vec<FileSpan>,
    ) -> Self {
        Self {
            file,
            module: None,
            roots: Vec::new(),
            items: Vec::new(),
            bodies: Vec::new(),
            types: Vec::new(),
            origins: Vec::new(),
            syntax_causes,
            syntax_recovery_spans,
            invalid: Vec::new(),
        }
    }

    fn finish(mut self) -> HirFile {
        for occurrence in &mut self.invalid {
            if occurrence.category() == InvalidCategory::ReportedSyntax
                || !occurrence.syntax_causes().is_empty()
            {
                continue;
            }
            let invalid_span = self.origins[occurrence.origin().index()].source();
            let causes = self
                .syntax_causes
                .iter()
                .zip(&self.syntax_recovery_spans)
                .enumerate()
                .filter(|&(_, (&primary, &recovery))| {
                    spans_overlap(invalid_span, primary)
                        || recovery_contains(invalid_span, recovery)
                })
                .map(|(index, _)| SyntaxCauseId::from_index(index))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            occurrence.set_syntax_causes(causes);
        }
        let bodies = self
            .bodies
            .into_iter()
            .map(BodyBuilder::finish)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        HirFile {
            file: self.file,
            module: self.module,
            roots: self.roots.into_boxed_slice(),
            items: self.items.into_boxed_slice(),
            bodies,
            types: self.types.into_boxed_slice(),
            origins: self.origins.into_boxed_slice(),
            syntax_causes: self.syntax_causes.into_boxed_slice(),
            invalid: self.invalid.into_boxed_slice(),
        }
    }

    fn written(&mut self, span: FileSpan) -> OriginId {
        self.alloc_origin(SourceOrigin::Written(span))
    }

    fn derived(&mut self, span: FileSpan, kind: DesugaringKind) -> OriginId {
        self.alloc_origin(SourceOrigin::Derived { source: span, kind })
    }

    fn alloc_origin(&mut self, origin: SourceOrigin) -> OriginId {
        let id = OriginId::from_index(self.origins.len());
        self.origins.push(origin);
        id
    }

    fn record_invalid(&mut self, category: InvalidCategory, span: FileSpan) -> OriginId {
        let origin = self.written(span);
        self.invalid.push(InvalidOccurrence::new(category, origin));
        origin
    }

    fn record_reported_syntax(&mut self, cause: SyntaxCauseId) {
        let origin = self.written(self.syntax_causes[cause.index()]);
        self.invalid.push(InvalidOccurrence::with_syntax_causes(
            InvalidCategory::ReportedSyntax,
            origin,
            Box::new([cause]),
        ));
    }

    fn invalid_name(&mut self, span: FileSpan) -> Name {
        Name::invalid(self.record_invalid(InvalidCategory::Name, span))
    }

    fn lower_name_token(&mut self, token: Option<SyntaxToken>, fallback: FileSpan) -> Name {
        let Some(token) = token.filter(|token| !token.is_missing()) else {
            return self.invalid_name(fallback);
        };
        let bang = token.kind() == SyntaxKind::BangIdentifier;
        if !matches!(
            token.kind(),
            SyntaxKind::Identifier | SyntaxKind::BangIdentifier
        ) {
            return self.invalid_name(token.span());
        }
        let text = if bang {
            token.text().strip_suffix('!').unwrap_or(token.text())
        } else {
            token.text()
        };
        let origin = self.written(token.span());
        Name::valid(text.into(), bang, origin)
    }

    fn lower_path(&mut self, node: Option<SyntaxNode>, fallback: FileSpan) -> Path {
        let Some(node) = node else {
            let invalid = self.invalid_name(fallback);
            let origin = invalid.origin();
            return Path::new(vec![invalid].into_boxed_slice(), origin);
        };
        let name_tokens = direct_tokens(&node)
            .into_iter()
            .filter(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::Identifier | SyntaxKind::BangIdentifier
                )
            })
            .collect::<Vec<_>>();
        let Some((first, last)) = name_tokens.first().zip(name_tokens.last()) else {
            let invalid = self.invalid_name(node.span());
            let origin = invalid.origin();
            return Path::new(vec![invalid].into_boxed_slice(), origin);
        };
        let origin_span = if name_tokens.iter().any(SyntaxToken::is_missing) {
            node.span()
        } else {
            let first = first.span();
            let last = last.span();
            let range = ByteRange::new(first.range().start(), last.range().end())
                .expect("path tokens are source-ordered");
            FileSpan::new(first.file(), range)
        };
        let origin = self.written(origin_span);
        let segments = name_tokens
            .into_iter()
            .map(|token| self.lower_name_token(Some(token), node.span()))
            .collect::<Vec<_>>();
        Path::new(segments.into_boxed_slice(), origin)
    }

    fn alloc_item(&mut self, item: Item) -> ItemId {
        let id = ItemId::from_index(self.items.len());
        self.items.push(item);
        id
    }

    fn new_body(&mut self) -> BodyId {
        let id = BodyId::from_index(self.bodies.len());
        self.bodies.push(BodyBuilder::new(id));
        id
    }

    fn body_mut(&mut self, body: BodyId) -> &mut BodyBuilder {
        &mut self.bodies[body.index()]
    }

    fn alloc_expr(&mut self, body: BodyId, expr: Expr) -> ExprId {
        self.body_mut(body).alloc_expr(expr)
    }

    fn alloc_stmt(&mut self, body: BodyId, stmt: Stmt) -> StmtId {
        self.body_mut(body).alloc_stmt(stmt)
    }

    fn alloc_pattern(&mut self, body: BodyId, pattern: Pattern) -> PatternId {
        self.body_mut(body).alloc_pattern(pattern)
    }

    fn alloc_axis(&mut self, body: BodyId, axis: AxisExpr) -> AxisExprId {
        self.body_mut(body).alloc_axis(axis)
    }

    fn alloc_type(&mut self, ty: TypeRef) -> TypeRefId {
        let id = TypeRefId::from_index(self.types.len());
        self.types.push(ty);
        id
    }
}

fn spans_overlap(left: FileSpan, right: FileSpan) -> bool {
    if left.file() != right.file() {
        return false;
    }
    let left = left.range();
    let right = right.range();
    if left.is_empty() {
        return if right.is_empty() {
            left.start() == right.start()
        } else {
            right.start() <= left.start() && left.start() < right.end()
        };
    }
    if right.is_empty() {
        return left.start() <= right.start() && right.start() < left.end();
    }
    left.start() < right.end() && right.start() < left.end()
}

fn recovery_contains(invalid: FileSpan, recovery: FileSpan) -> bool {
    if invalid.file() != recovery.file() {
        return false;
    }
    let invalid = invalid.range();
    let recovery = recovery.range();
    if recovery.is_empty() {
        return invalid.is_empty() && invalid.start() == recovery.start();
    }
    if invalid.is_empty() {
        return recovery.start() <= invalid.start() && invalid.start() < recovery.end();
    }
    recovery.start() <= invalid.start() && invalid.end() <= recovery.end()
}

struct BodyBuilder {
    id: BodyId,
    expressions: Vec<Expr>,
    statements: Vec<Stmt>,
    patterns: Vec<Pattern>,
    axes: Vec<AxisExpr>,
}

impl BodyBuilder {
    fn new(id: BodyId) -> Self {
        Self {
            id,
            expressions: Vec::new(),
            statements: Vec::new(),
            patterns: Vec::new(),
            axes: Vec::new(),
        }
    }

    fn finish(self) -> Body {
        Body::new(
            self.id,
            self.expressions.into_boxed_slice(),
            self.statements.into_boxed_slice(),
            self.patterns.into_boxed_slice(),
            self.axes.into_boxed_slice(),
        )
    }

    fn alloc_expr(&mut self, expr: Expr) -> ExprId {
        let id = ExprId::from_index(self.id, self.expressions.len());
        self.expressions.push(expr);
        id
    }

    fn alloc_stmt(&mut self, stmt: Stmt) -> StmtId {
        let id = StmtId::from_index(self.id, self.statements.len());
        self.statements.push(stmt);
        id
    }

    fn alloc_pattern(&mut self, pattern: Pattern) -> PatternId {
        let id = PatternId::from_index(self.id, self.patterns.len());
        self.patterns.push(pattern);
        id
    }

    fn alloc_axis(&mut self, axis: AxisExpr) -> AxisExprId {
        let id = AxisExprId::from_index(self.id, self.axes.len());
        self.axes.push(axis);
        id
    }
}

fn direct_nodes(node: &SyntaxNode) -> Vec<SyntaxNode> {
    node.children().collect()
}

fn direct_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|child| child.kind() == kind)
}

fn direct_children(node: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    node.children()
        .filter(|child| child.kind() == kind)
        .collect()
}

fn direct_tokens(node: &SyntaxNode) -> Vec<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|element| match element {
            SyntaxElement::Token(token) if !is_trivia(token.kind()) => Some(token),
            SyntaxElement::Node(_) | SyntaxElement::Token(_) => None,
        })
        .collect()
}

fn direct_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    direct_tokens(node)
        .into_iter()
        .find(|token| token.kind() == kind)
}

fn first_name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    direct_tokens(node).into_iter().find(|token| {
        matches!(
            token.kind(),
            SyntaxKind::Identifier | SyntaxKind::BangIdentifier
        )
    })
}

fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Bom
            | SyntaxKind::Whitespace
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment
            | SyntaxKind::Newline
    )
}

fn is_expression_kind(kind: SyntaxKind) -> bool {
    eglur_syntax::ast::Expr::can_cast(kind)
}

fn is_pattern_kind(kind: SyntaxKind) -> bool {
    eglur_syntax::ast::Pattern::can_cast(kind)
}

fn is_type_kind(kind: SyntaxKind) -> bool {
    eglur_syntax::ast::Type::can_cast(kind)
}
