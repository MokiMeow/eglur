use crate::{BodyId, ExprId, Mutability, Name, OriginId, Path, PatternId, TypeRefId, Visibility};
use crate::{ItemId, Literal};

/// Optional module declaration, separate from graph-local module identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleDecl {
    path: Path,
    origin: OriginId,
}

impl ModuleDecl {
    pub(crate) const fn new(path: Path, origin: OriginId) -> Self {
        Self { path, origin }
    }

    #[must_use]
    pub const fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// One declaration occurrence in the file-local item arena.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    origin: OriginId,
    attributes: Box<[Attribute]>,
    visibility: Visibility,
    kind: ItemKind,
}

impl Item {
    pub(crate) fn new(
        origin: OriginId,
        attributes: Box<[Attribute]>,
        visibility: Visibility,
        kind: ItemKind,
    ) -> Self {
        Self {
            origin,
            attributes,
            visibility,
            kind,
        }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub fn attributes(&self) -> &[Attribute] {
        &self.attributes
    }

    #[must_use]
    pub const fn visibility(&self) -> Visibility {
        self.visibility
    }

    #[must_use]
    pub const fn kind(&self) -> &ItemKind {
        &self.kind
    }
}

/// Complete source declaration taxonomy before semantic capability checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemKind {
    Import(ImportDecl),
    Function(FunctionDecl),
    Extern(ExternDecl),
    Trait(TraitDecl),
    Impl(ImplDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    TypeAlias(TypeAliasDecl),
    Const(ConstDecl),
    AssociatedType(AssociatedTypeDecl),
    AssociatedTypeBinding(AssociatedTypeBinding),
    StorageField(StorageFieldDecl),
    Property(PropertyDecl),
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDecl {
    pub source: ImportSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportSource {
    Single {
        path: Path,
        alias: Option<Name>,
    },
    Group {
        prefix: Path,
        entries: Box<[ImportEntry]>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEntry {
    pub name: Name,
    pub alias: Option<Name>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionDecl {
    pub name: Name,
    pub overload: bool,
    pub generic_parameters: Box<[GenericParameter]>,
    pub parameters: Box<[Parameter]>,
    pub return_type: Option<TypeRefId>,
    pub body: BodyId,
    pub body_form: FunctionBody,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FunctionBody {
    Absent,
    Expression(ExprId),
    Block(ExprId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternDecl {
    pub abi: Literal,
    pub name: Name,
    pub parameters: Box<[Parameter]>,
    pub return_type: Option<TypeRefId>,
    pub body: BodyId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parameter {
    pub pattern: PatternId,
    pub ty: Option<TypeRefId>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenericParameter {
    pub name: Name,
    pub bounds: Box<[TypeRefId]>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitDecl {
    pub name: Name,
    pub closed: bool,
    pub generic_parameters: Box<[GenericParameter]>,
    pub items: Box<[ItemId]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImplDecl {
    pub generic_parameters: Box<[GenericParameter]>,
    pub negative: bool,
    pub trait_ref: TypeRefId,
    pub target: TypeRefId,
    pub items: Box<[ItemId]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructDecl {
    pub name: Name,
    pub generic_parameters: Box<[GenericParameter]>,
    pub base: Option<TypeRefId>,
    pub items: Box<[ItemId]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumDecl {
    pub name: Name,
    pub generic_parameters: Box<[GenericParameter]>,
    pub variants: Box<[EnumVariant]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumVariant {
    pub name: Name,
    pub attributes: Box<[Attribute]>,
    pub visibility: Visibility,
    pub payload: EnumVariantPayload,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnumVariantPayload {
    Absent,
    Tuple(Box<[TypeRefId]>),
    Named(Box<[EnumVariantField]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnumVariantField {
    pub name: Name,
    pub ty: TypeRefId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeAliasDecl {
    pub name: Name,
    pub generic_parameters: Box<[GenericParameter]>,
    pub value: TypeRefId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstDecl {
    pub name: Name,
    pub ty: TypeRefId,
    pub body: BodyId,
    pub value: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociatedTypeDecl {
    pub name: Name,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociatedTypeBinding {
    pub name: Name,
    pub value: TypeRefId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageFieldSyntax {
    Field,
    Var,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageFieldDecl {
    pub syntax: StorageFieldSyntax,
    pub mutability: Mutability,
    pub name: Name,
    pub ty: TypeRefId,
    pub initializer: Option<BodyExpression>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PropertyDecl {
    pub mutability: Mutability,
    pub name: Name,
    pub ty: TypeRefId,
    pub accessors: Box<[ItemId]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BodyExpression {
    pub body: BodyId,
    pub expression: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attribute {
    pub meta: AttributeMeta,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeMeta {
    pub path: Path,
    pub arguments: Box<[AttributeArgument]>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttributeArgument {
    pub name: Option<Name>,
    pub value: AttributeValue,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributeValue {
    Literal(Literal),
    Meta(Box<AttributeMeta>),
    Invalid(OriginId),
}
