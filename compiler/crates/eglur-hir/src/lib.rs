//! Source-semantic HIR and the sole production syntax-lowering boundary.

#![forbid(unsafe_code)]

mod body;
mod ids;
mod item;
mod lower;
mod name;
mod origin;
mod ty;

pub use body::*;
pub use ids::*;
pub use item::*;
pub use lower::lower;
pub use name::*;
pub use origin::*;
pub use ty::*;

use eglur_source::{FileId, FileSpan};

/// One fully initialized source-semantic file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HirFile {
    file: FileId,
    module: Option<ModuleDecl>,
    roots: Box<[ItemId]>,
    items: Box<[Item]>,
    bodies: Box<[Body]>,
    types: Box<[TypeRef]>,
    origins: Box<[SourceOrigin]>,
    syntax_causes: Box<[FileSpan]>,
    invalid: Box<[InvalidOccurrence]>,
}

impl HirFile {
    /// Returns the source identity represented by this HIR file.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    /// Returns the optional source module declaration.
    #[must_use]
    pub const fn module(&self) -> Option<&ModuleDecl> {
        self.module.as_ref()
    }

    /// Returns root declarations in source order.
    #[must_use]
    pub fn roots(&self) -> &[ItemId] {
        &self.roots
    }

    /// Returns one file-local declaration occurrence.
    #[must_use]
    pub fn item(&self, id: ItemId) -> &Item {
        &self.items[id.index()]
    }

    /// Iterates all declaration occurrences in deterministic allocation order.
    pub fn items(&self) -> impl ExactSizeIterator<Item = (ItemId, &Item)> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| (ItemId::from_index(index), item))
    }

    /// Returns one semantic body.
    #[must_use]
    pub fn body(&self, id: BodyId) -> &Body {
        &self.bodies[id.index()]
    }

    /// Iterates all bodies in deterministic allocation order.
    pub fn bodies(&self) -> impl ExactSizeIterator<Item = (BodyId, &Body)> {
        self.bodies
            .iter()
            .enumerate()
            .map(|(index, body)| (BodyId::from_index(index), body))
    }

    /// Returns one file-local source type occurrence.
    #[must_use]
    pub fn type_ref(&self, id: TypeRefId) -> &TypeRef {
        &self.types[id.index()]
    }

    /// Iterates all source type occurrences in deterministic allocation order.
    pub fn type_refs(&self) -> impl ExactSizeIterator<Item = (TypeRefId, &TypeRef)> {
        self.types
            .iter()
            .enumerate()
            .map(|(index, ty)| (TypeRefId::from_index(index), ty))
    }

    /// Resolves one source-origin identity.
    #[must_use]
    pub fn origin(&self, id: OriginId) -> SourceOrigin {
        self.origins[id.index()]
    }

    /// Iterates source origins in deterministic allocation order.
    pub fn origins(&self) -> impl ExactSizeIterator<Item = (OriginId, SourceOrigin)> + '_ {
        self.origins
            .iter()
            .copied()
            .enumerate()
            .map(|(index, origin)| (OriginId::from_index(index), origin))
    }

    /// Resolves one parser recovery cause to its exact diagnostic span.
    #[must_use]
    pub fn syntax_cause(&self, id: SyntaxCauseId) -> FileSpan {
        self.syntax_causes[id.index()]
    }

    /// Iterates parser recovery causes in syntax-diagnostic order.
    pub fn syntax_causes(&self) -> impl ExactSizeIterator<Item = (SyntaxCauseId, FileSpan)> + '_ {
        self.syntax_causes
            .iter()
            .copied()
            .enumerate()
            .map(|(index, span)| (SyntaxCauseId::from_index(index), span))
    }

    /// Returns all structural syntax-invalid occurrences.
    #[must_use]
    pub fn invalid_occurrences(&self) -> &[InvalidOccurrence] {
        &self.invalid
    }
}
