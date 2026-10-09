use std::collections::BTreeMap;

use eglur_hir::Visibility;

use super::TrustedTypeExpr;
use crate::{
    CallableSignatureId, CanonicalTarget, ExternalDefId, IntrinsicNominal, NameKey, NominalHead,
    PrimitiveType, TypeId,
};

/// Ordinary method-owner identity at the portable catalog boundary.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TrustedInherentOwnerHead {
    Primitive(PrimitiveType),
    Intrinsic(IntrinsicNominal),
    Nominal(ExternalDefId),
}

impl TrustedInherentOwnerHead {
    pub(super) fn canonical(&self) -> ConcreteOwnerHead {
        match self {
            Self::Primitive(ty) => ConcreteOwnerHead::Primitive(*ty),
            Self::Intrinsic(head) => ConcreteOwnerHead::Nominal(NominalHead::Intrinsic(*head)),
            Self::Nominal(id) => ConcreteOwnerHead::Nominal(NominalHead::Declaration(
                CanonicalTarget::External(id.clone()),
            )),
        }
    }
}

/// Call syntax and receiver contract, independently of the owner's representation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InherentMemberForm {
    Associated,
    ValueReceiver,
    SharedReceiver,
    MutableReceiver,
}

/// Explicit membership of an existing callable in an ordinary owner family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedInherentMember {
    pub callable: ExternalDefId,
    pub name: NameKey,
    pub owner_pattern: TrustedTypeExpr,
    /// Owner binders occupy this prefix of the callable's generic parameters.
    pub owner_parameter_count: usize,
    pub form: InherentMemberForm,
    pub visibility: Visibility,
    pub overload: bool,
}

/// Complete ordinary member inventory for one head; an empty inventory is meaningful.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedInherentOwner {
    pub head: TrustedInherentOwnerHead,
    pub members: Box<[TrustedInherentMember]>,
}

/// Canonical ordinary owner head, separate from its applied type pattern.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConcreteOwnerHead {
    Primitive(PrimitiveType),
    Nominal(NominalHead),
}

/// An admitted ordinary member linked to the existing canonical callable table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InherentMemberSignature {
    callable: CallableSignatureId,
    owner_pattern: TypeId,
    owner_parameter_count: usize,
    form: InherentMemberForm,
    visibility: Visibility,
}

impl InherentMemberSignature {
    pub(super) const fn new(
        callable: CallableSignatureId,
        owner_pattern: TypeId,
        owner_parameter_count: usize,
        form: InherentMemberForm,
        visibility: Visibility,
    ) -> Self {
        Self {
            callable,
            owner_pattern,
            owner_parameter_count,
            form,
            visibility,
        }
    }

    #[must_use]
    pub const fn callable(&self) -> CallableSignatureId {
        self.callable
    }

    #[must_use]
    pub const fn owner_pattern(&self) -> TypeId {
        self.owner_pattern
    }

    #[must_use]
    pub const fn owner_parameter_count(&self) -> usize {
        self.owner_parameter_count
    }

    #[must_use]
    pub const fn form(&self) -> InherentMemberForm {
        self.form
    }

    #[must_use]
    pub const fn visibility(&self) -> Visibility {
        self.visibility
    }
}

/// Complete overload family, including private members and applied owner patterns.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InherentFamilySignature {
    members: Box<[InherentMemberSignature]>,
}

impl InherentFamilySignature {
    pub(super) fn new(members: Vec<InherentMemberSignature>) -> Self {
        Self {
            members: members.into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn members(&self) -> &[InherentMemberSignature] {
        &self.members
    }

    #[must_use]
    pub fn is_public(&self) -> bool {
        self.members
            .iter()
            .all(|member| member.visibility == Visibility::Public)
    }
}

/// Complete ordinary membership for one head, indexed by explicit member name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InherentMethodInventory {
    families: BTreeMap<NameKey, InherentFamilySignature>,
}

impl InherentMethodInventory {
    pub(super) const fn new(families: BTreeMap<NameKey, InherentFamilySignature>) -> Self {
        Self { families }
    }

    #[must_use]
    pub fn family(&self, name: &NameKey) -> Option<&InherentFamilySignature> {
        self.families.get(name)
    }

    pub fn families(&self) -> impl ExactSizeIterator<Item = (&NameKey, &InherentFamilySignature)> {
        self.families.iter()
    }
}
