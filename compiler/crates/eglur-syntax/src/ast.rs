use crate::{SyntaxKind, SyntaxNode};

/// Typed syntax-node projection with no semantic facts.
pub trait AstNode: Clone + Sized {
    /// Reports whether this wrapper owns a syntax kind.
    fn can_cast(kind: SyntaxKind) -> bool;

    /// Casts a syntax node when its kind belongs to this wrapper.
    fn cast(node: SyntaxNode) -> Option<Self>;

    /// Returns the underlying Eglur-owned syntax node.
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($name:ident, $($kind:pat_param)|+) => {
        #[derive(Clone, Debug)]
        pub struct $name(SyntaxNode);

        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                matches!(kind, $($kind)|+)
            }

            fn cast(node: SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then_some(Self(node))
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.0
            }
        }
    };
}

ast_node!(SourceFile, SyntaxKind::Root);
ast_node!(Module, SyntaxKind::ModuleDecl);
ast_node!(InvalidSyntax, SyntaxKind::ErrorNode);

/// A top-level node outside the source-file facade's known grammar.
#[derive(Clone, Debug)]
pub struct UnexpectedSyntax(SyntaxNode);

impl UnexpectedSyntax {
    /// Returns the underlying Eglur-owned syntax node.
    #[must_use]
    pub const fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}
ast_node!(
    Item,
    SyntaxKind::ImportDecl
        | SyntaxKind::FnDecl
        | SyntaxKind::ExternDecl
        | SyntaxKind::TraitDecl
        | SyntaxKind::ImplDecl
        | SyntaxKind::StructDecl
        | SyntaxKind::EnumDecl
        | SyntaxKind::TypeDecl
        | SyntaxKind::ConstDecl
);
ast_node!(
    Expr,
    SyntaxKind::BlockExpr
        | SyntaxKind::BinaryExpr
        | SyntaxKind::UnaryExpr
        | SyntaxKind::RefExpr
        | SyntaxKind::OwnExpr
        | SyntaxKind::MoveExpr
        | SyntaxKind::DropOwnExpr
        | SyntaxKind::CallExpr
        | SyntaxKind::MemberExpr
        | SyntaxKind::TupleProjectionExpr
        | SyntaxKind::TryExpr
        | SyntaxKind::DTypeBracketCallExpr
        | SyntaxKind::ReduceByCandidateExpr
        | SyntaxKind::LiteralExpr
        | SyntaxKind::NameExpr
        | SyntaxKind::HoleExpr
        | SyntaxKind::UnitExpr
        | SyntaxKind::GroupExpr
        | SyntaxKind::UnsafeExpr
        | SyntaxKind::TupleExpr
        | SyntaxKind::ArrayExpr
        | SyntaxKind::MapExpr
        | SyntaxKind::StructLiteralExpr
        | SyntaxKind::VariantConstructorExpr
        | SyntaxKind::LambdaExpr
        | SyntaxKind::IfExpr
        | SyntaxKind::MatchExpr
        | SyntaxKind::ResultExpr
        | SyntaxKind::CheckedExpr
        | SyntaxKind::TensorSchemaExpr
);
ast_node!(
    Pattern,
    SyntaxKind::Pattern
        | SyntaxKind::OrPattern
        | SyntaxKind::AliasPattern
        | SyntaxKind::BindingPattern
        | SyntaxKind::WildcardPattern
        | SyntaxKind::GroupPattern
        | SyntaxKind::LiteralPattern
        | SyntaxKind::UnitPattern
        | SyntaxKind::VariantPattern
        | SyntaxKind::TuplePattern
        | SyntaxKind::StructPattern
);
ast_node!(
    Type,
    SyntaxKind::TypeExpr
        | SyntaxKind::RefType
        | SyntaxKind::PointerType
        | SyntaxKind::PathType
        | SyntaxKind::DynType
        | SyntaxKind::TupleType
        | SyntaxKind::FnType
);

impl SourceFile {
    /// Iterates module, item, and recovery regions in source order.
    pub fn elements(&self) -> impl Iterator<Item = SourceElement> + '_ {
        self.syntax().children().map(SourceElement::cast)
    }

    /// Iterates parsed top-level items in source order.
    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.syntax().children().filter_map(Item::cast)
    }
}

/// One semantically relevant top-level syntax region.
#[derive(Clone, Debug)]
pub enum SourceElement {
    /// The optional module declaration.
    Module(Module),
    /// One parsed top-level item.
    Item(Item),
    /// One parser recovery region.
    Invalid(InvalidSyntax),
    /// A node not classified by the current typed source-file facade.
    Unexpected(UnexpectedSyntax),
}

impl SourceElement {
    fn cast(node: SyntaxNode) -> Self {
        if Module::can_cast(node.kind()) {
            return Self::Module(Module::cast(node).expect("module kind was checked"));
        }
        if Item::can_cast(node.kind()) {
            return Self::Item(Item::cast(node).expect("item kind was checked"));
        }
        if InvalidSyntax::can_cast(node.kind()) {
            return Self::Invalid(
                InvalidSyntax::cast(node).expect("invalid-syntax kind was checked"),
            );
        }
        Self::Unexpected(UnexpectedSyntax(node))
    }

    /// Returns the underlying Eglur-owned syntax node.
    #[must_use]
    pub fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Module(module) => module.syntax(),
            Self::Item(item) => item.syntax(),
            Self::Invalid(invalid) => invalid.syntax(),
            Self::Unexpected(unexpected) => unexpected.syntax(),
        }
    }
}
