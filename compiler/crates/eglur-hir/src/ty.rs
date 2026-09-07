use crate::{Mutability, OriginId, Path, TypeRefId};

/// One file-local source type occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeRef {
    origin: OriginId,
    kind: TypeRefKind,
}

impl TypeRef {
    pub(crate) const fn new(origin: OriginId, kind: TypeRefKind) -> Self {
        Self { origin, kind }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn kind(&self) -> &TypeRefKind {
        &self.kind
    }
}

/// Structural source type before resolution or normalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeRefKind {
    Reference {
        mutability: Mutability,
        referent: TypeRefId,
    },
    Pointer {
        mutability: PointerMutability,
        referent: TypeRefId,
    },
    Path {
        path: Path,
        arguments: Box<[TypeRefId]>,
    },
    Dynamic {
        trait_ref: TypeRefId,
    },
    Tuple(Box<[TypeRefId]>),
    Function {
        parameters: Box<[TypeRefId]>,
        result: TypeRefId,
    },
    Group(TypeRefId),
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PointerMutability {
    Const,
    Mutable,
}
