use std::collections::BTreeMap;
use std::fmt;

use crate::{
    CanonicalTarget, DeclarationSignature, DeclarationSignatures, ExternalDefId, FactState,
    GenericParameterId, IntrinsicNominal, NominalHead, TrustedTypeExpr, TypeId, TypeKind,
    TypeStoreError,
};

mod fixed_point;

use fixed_point::{NodeFormula, NodeId};

/// Conservative proof state for whether a well-formed type has a value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Inhabitance {
    Uninhabited,
    Unknown,
    Inhabited,
}

/// One product alternative in a trusted nominal sum rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedInhabitanceProduct {
    members: Box<[TrustedTypeExpr]>,
}

impl TrustedInhabitanceProduct {
    #[must_use]
    pub fn new(members: impl IntoIterator<Item = TrustedTypeExpr>) -> Self {
        Self {
            members: members.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        }
    }

    #[must_use]
    pub fn members(&self) -> &[TrustedTypeExpr] {
        &self.members
    }
}

/// Portable inhabitance rule for one trusted nominal declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedNominalInhabitance {
    Unknown,
    Inhabited,
    Product(Box<[TrustedTypeExpr]>),
    Sum(Box<[TrustedInhabitanceProduct]>),
}

/// One trusted nominal inhabitance fact before canonical type binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedInhabitanceFact {
    declaration: ExternalDefId,
    rule: TrustedNominalInhabitance,
}

impl TrustedInhabitanceFact {
    #[must_use]
    pub const fn new(declaration: ExternalDefId, rule: TrustedNominalInhabitance) -> Self {
        Self { declaration, rule }
    }

    #[must_use]
    pub const fn declaration(&self) -> &ExternalDefId {
        &self.declaration
    }

    #[must_use]
    pub const fn rule(&self) -> &TrustedNominalInhabitance {
        &self.rule
    }
}

/// Deterministically ordered trusted inhabitance facts keyed by external identity.
pub struct TrustedInhabitanceEnvironment {
    facts: Box<[TrustedInhabitanceFact]>,
    by_id: BTreeMap<ExternalDefId, usize>,
}

impl TrustedInhabitanceEnvironment {
    pub fn new(
        facts: impl IntoIterator<Item = TrustedInhabitanceFact>,
    ) -> Result<Self, TrustedInhabitanceError> {
        let mut facts = facts.into_iter().collect::<Vec<_>>();
        facts.sort_by(|left, right| left.declaration.cmp(&right.declaration));
        let mut by_id = BTreeMap::new();
        for (index, fact) in facts.iter().enumerate() {
            if by_id.insert(fact.declaration.clone(), index).is_some() {
                return Err(TrustedInhabitanceError::DuplicateDeclaration(
                    fact.declaration.clone(),
                ));
            }
        }
        Ok(Self {
            facts: facts.into_boxed_slice(),
            by_id,
        })
    }

    #[must_use]
    pub fn rule(&self, declaration: &ExternalDefId) -> Option<&TrustedNominalInhabitance> {
        self.by_id
            .get(declaration)
            .map(|&index| self.facts[index].rule())
    }

    pub fn facts(&self) -> impl ExactSizeIterator<Item = &TrustedInhabitanceFact> {
        self.facts.iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedInhabitanceError {
    DuplicateDeclaration(ExternalDefId),
}

impl fmt::Display for TrustedInhabitanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateDeclaration(declaration) => write!(
                formatter,
                "duplicate trusted inhabitance fact {}",
                declaration.as_str()
            ),
        }
    }
}

impl std::error::Error for TrustedInhabitanceError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NominalInhabitanceProduct {
    members: Box<[TypeId]>,
}

impl NominalInhabitanceProduct {
    pub(crate) fn new(members: impl IntoIterator<Item = TypeId>) -> Self {
        Self {
            members: members.into_iter().collect::<Vec<_>>().into_boxed_slice(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NominalInhabitanceRule {
    Unknown,
    Inhabited,
    Product(Box<[TypeId]>),
    Sum(Box<[NominalInhabitanceProduct]>),
}

/// Memoized root facts plus the canonical trusted nominal rules used to derive them.
pub struct InhabitanceFacts {
    trusted: BTreeMap<ExternalDefId, NominalInhabitanceRule>,
    facts: BTreeMap<TypeId, Inhabitance>,
}

impl InhabitanceFacts {
    pub(crate) fn new(
        trusted: impl IntoIterator<Item = (ExternalDefId, NominalInhabitanceRule)>,
    ) -> Result<Self, InhabitanceError> {
        let mut normalized = BTreeMap::new();
        for (declaration, rule) in trusted {
            if normalized.insert(declaration.clone(), rule).is_some() {
                return Err(InhabitanceError::DuplicateTrustedRule(declaration));
            }
        }
        Ok(Self {
            trusted: normalized,
            facts: BTreeMap::new(),
        })
    }

    pub fn classify(
        &mut self,
        signatures: &mut DeclarationSignatures,
        root: TypeId,
    ) -> Result<Inhabitance, InhabitanceError> {
        if let Some(&fact) = self.facts.get(&root) {
            return Ok(fact);
        }
        let fact = InhabitanceGraph::new(signatures, &self.trusted).classify(root)?;
        self.facts.insert(root, fact);
        Ok(fact)
    }

    #[must_use]
    pub fn fact(&self, ty: TypeId) -> Option<Inhabitance> {
        self.facts.get(&ty).copied()
    }

    pub fn facts(&self) -> impl ExactSizeIterator<Item = (TypeId, Inhabitance)> + '_ {
        self.facts.iter().map(|(&ty, &fact)| (ty, fact))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InhabitanceError {
    TypeStore(TypeStoreError),
    DuplicateTrustedRule(ExternalDefId),
    NominalArity {
        declaration: CanonicalTarget,
        parameters: usize,
        arguments: usize,
    },
    UnexpectedSignature {
        declaration: CanonicalTarget,
        expected: &'static str,
    },
}

impl fmt::Display for InhabitanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeStore(error) => error.fmt(formatter),
            Self::DuplicateTrustedRule(declaration) => write!(
                formatter,
                "duplicate canonical inhabitance rule {}",
                declaration.as_str()
            ),
            Self::NominalArity {
                declaration,
                parameters,
                arguments,
            } => write!(
                formatter,
                "inhabitance rule for {declaration:?} has {parameters} parameters but {arguments} arguments"
            ),
            Self::UnexpectedSignature {
                declaration,
                expected,
            } => write!(
                formatter,
                "inhabitance analysis expected {expected} signature for {declaration:?}"
            ),
        }
    }
}

impl std::error::Error for InhabitanceError {}

impl From<TypeStoreError> for InhabitanceError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

struct ActiveNominal {
    arguments: Box<[TypeId]>,
    node: NodeId,
}

struct InhabitanceGraph<'facts> {
    signatures: &'facts mut DeclarationSignatures,
    trusted: &'facts BTreeMap<ExternalDefId, NominalInhabitanceRule>,
    nodes: Vec<NodeFormula>,
    by_type: BTreeMap<TypeId, NodeId>,
    active: BTreeMap<NominalHead, ActiveNominal>,
    uninhabited: NodeId,
    unknown: NodeId,
    inhabited: NodeId,
}

impl<'facts> InhabitanceGraph<'facts> {
    fn new(
        signatures: &'facts mut DeclarationSignatures,
        trusted: &'facts BTreeMap<ExternalDefId, NominalInhabitanceRule>,
    ) -> Self {
        let nodes = vec![
            NodeFormula::Constant(Inhabitance::Uninhabited),
            NodeFormula::Constant(Inhabitance::Unknown),
            NodeFormula::Constant(Inhabitance::Inhabited),
        ];
        Self {
            signatures,
            trusted,
            nodes,
            by_type: BTreeMap::new(),
            active: BTreeMap::new(),
            uninhabited: 0,
            unknown: 1,
            inhabited: 2,
        }
    }

    fn classify(mut self, root: TypeId) -> Result<Inhabitance, InhabitanceError> {
        let root = self.build_type(root)?;
        let values = fixed_point::least_fixed_point(&self.nodes);
        Ok(values[root])
    }

    fn build_type(&mut self, ty: TypeId) -> Result<NodeId, InhabitanceError> {
        if let Some(&node) = self.by_type.get(&ty) {
            return Ok(node);
        }
        let kind = self.signatures.types().kind(ty)?.clone();
        let node = match kind {
            TypeKind::Never => self.uninhabited,
            TypeKind::Primitive(_) | TypeKind::Callable { .. } => self.inhabited,
            TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_)
            | TypeKind::RawPointer { .. }
            | TypeKind::AssociatedProjection(_) => self.unknown,
            TypeKind::Reference { referent, .. } => self.product_node([referent])?,
            TypeKind::Tuple(elements) => self.product_node(elements.iter().copied())?,
            TypeKind::ErrorUnion(members) => self.sum_node(members.iter().copied())?,
            TypeKind::Nominal { head, substitution } => {
                let arguments = self
                    .signatures
                    .types()
                    .substitution(substitution)?
                    .to_vec()
                    .into_boxed_slice();
                if let Some(active) = self.active.get(&head) {
                    if active.arguments == arguments {
                        return Ok(active.node);
                    }
                    return Ok(self.unknown);
                }
                let node = self.add_node(NodeFormula::Constant(Inhabitance::Uninhabited));
                self.by_type.insert(ty, node);
                self.active.insert(
                    head.clone(),
                    ActiveNominal {
                        arguments: arguments.clone(),
                        node,
                    },
                );
                let formula = self.nominal_formula(&head, &arguments)?;
                let removed = self.active.remove(&head);
                debug_assert!(removed.is_some());
                self.nodes[node] = formula;
                return Ok(node);
            }
        };
        self.by_type.insert(ty, node);
        Ok(node)
    }

    fn nominal_formula(
        &mut self,
        head: &NominalHead,
        arguments: &[TypeId],
    ) -> Result<NodeFormula, InhabitanceError> {
        match head {
            NominalHead::Declaration(CanonicalTarget::Primitive(primitive)) => Err(
                InhabitanceError::TypeStore(TypeStoreError::PrimitiveNominalHead(*primitive)),
            ),
            NominalHead::Declaration(CanonicalTarget::Intrinsic(intrinsic)) => Err(
                InhabitanceError::TypeStore(TypeStoreError::IntrinsicDeclarationHead(*intrinsic)),
            ),
            NominalHead::Intrinsic(IntrinsicNominal::Opt) => {
                Ok(NodeFormula::Constant(Inhabitance::Inhabited))
            }
            NominalHead::Intrinsic(IntrinsicNominal::Res) => {
                self.sum_formula(arguments.iter().copied())
            }
            NominalHead::Intrinsic(IntrinsicNominal::Box | IntrinsicNominal::Own) => {
                self.product_formula(arguments.iter().copied())
            }
            NominalHead::Declaration(CanonicalTarget::Source(declaration)) => {
                self.declared_nominal_formula(CanonicalTarget::Source(*declaration), arguments)
            }
            NominalHead::Declaration(CanonicalTarget::External(declaration)) => {
                let target = CanonicalTarget::External(declaration.clone());
                if matches!(
                    self.signatures.declaration(&target),
                    FactState::Known(DeclarationSignature::Enum { .. })
                ) {
                    self.declared_nominal_formula(target, arguments)
                } else {
                    self.trusted_nominal_formula(declaration, arguments)
                }
            }
        }
    }

    fn declared_nominal_formula(
        &mut self,
        target: CanonicalTarget,
        arguments: &[TypeId],
    ) -> Result<NodeFormula, InhabitanceError> {
        let signature = match self.signatures.declaration(&target) {
            FactState::Known(signature) => signature.clone(),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => {
                return Ok(NodeFormula::Constant(Inhabitance::Unknown));
            }
        };
        match signature {
            DeclarationSignature::Struct {
                generic_parameters,
                storage_fields,
                ..
            } => {
                let replacements = self.replacements(&target, &generic_parameters, arguments)?;
                let mut fields = Vec::with_capacity(storage_fields.len());
                for field in storage_fields {
                    let Some(template) = self.field_type(&field)? else {
                        return Ok(NodeFormula::Constant(Inhabitance::Unknown));
                    };
                    fields.push(
                        self.signatures
                            .types_mut()
                            .substitute(template, &replacements)?,
                    );
                }
                self.product_formula(fields)
            }
            DeclarationSignature::Enum {
                generic_parameters,
                variants,
            } => {
                let replacements = self.replacements(&target, &generic_parameters, arguments)?;
                let mut alternatives = Vec::with_capacity(variants.len());
                for variant in variants {
                    let signature = match self.signatures.declaration(&variant) {
                        FactState::Known(signature) => signature.clone(),
                        FactState::Poisoned(_)
                        | FactState::Unperformed
                        | FactState::NotApplicable => {
                            return Ok(NodeFormula::Constant(Inhabitance::Unknown));
                        }
                    };
                    let DeclarationSignature::EnumVariant { payload, .. } = signature else {
                        return Err(InhabitanceError::UnexpectedSignature {
                            declaration: variant,
                            expected: "enum variant",
                        });
                    };
                    let members = match payload {
                        crate::EnumVariantPayloadSignature::Absent => Vec::new(),
                        crate::EnumVariantPayloadSignature::Tuple(types) => types.to_vec(),
                        crate::EnumVariantPayloadSignature::Named(fields) => {
                            let mut types = Vec::with_capacity(fields.len());
                            for field in fields {
                                let Some(ty) = self.field_type(&field)? else {
                                    return Ok(NodeFormula::Constant(Inhabitance::Unknown));
                                };
                                types.push(ty);
                            }
                            types
                        }
                    };
                    let members = members
                        .into_iter()
                        .map(|member| {
                            self.signatures
                                .types_mut()
                                .substitute(member, &replacements)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    alternatives.push(self.product_node(members)?);
                }
                Ok(NodeFormula::Sum(alternatives.into_boxed_slice()))
            }
            _ => Ok(NodeFormula::Constant(Inhabitance::Unknown)),
        }
    }

    fn trusted_nominal_formula(
        &mut self,
        declaration: &ExternalDefId,
        arguments: &[TypeId],
    ) -> Result<NodeFormula, InhabitanceError> {
        let Some(rule) = self.trusted.get(declaration).cloned() else {
            return Ok(NodeFormula::Constant(Inhabitance::Unknown));
        };
        let replacements = arguments
            .iter()
            .enumerate()
            .map(|(index, &argument)| {
                let index = u32::try_from(index).expect("trusted nominal arity fits u32");
                (
                    GenericParameterId::Trusted {
                        owner: declaration.clone(),
                        index,
                    },
                    argument,
                )
            })
            .collect::<BTreeMap<_, _>>();
        match rule {
            NominalInhabitanceRule::Unknown => Ok(NodeFormula::Constant(Inhabitance::Unknown)),
            NominalInhabitanceRule::Inhabited => Ok(NodeFormula::Constant(Inhabitance::Inhabited)),
            NominalInhabitanceRule::Product(members) => {
                let members = self.specialize_members(&members, &replacements)?;
                self.product_formula(members)
            }
            NominalInhabitanceRule::Sum(products) => {
                let mut alternatives = Vec::with_capacity(products.len());
                for product in products {
                    let members = self.specialize_members(&product.members, &replacements)?;
                    alternatives.push(self.product_node(members)?);
                }
                Ok(NodeFormula::Sum(alternatives.into_boxed_slice()))
            }
        }
    }

    fn specialize_members(
        &mut self,
        members: &[TypeId],
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<Vec<TypeId>, InhabitanceError> {
        members
            .iter()
            .map(|&member| {
                self.signatures
                    .types_mut()
                    .substitute(member, replacements)
                    .map_err(InhabitanceError::from)
            })
            .collect()
    }

    fn replacements(
        &self,
        declaration: &CanonicalTarget,
        parameters: &[GenericParameterId],
        arguments: &[TypeId],
    ) -> Result<BTreeMap<GenericParameterId, TypeId>, InhabitanceError> {
        if parameters.len() != arguments.len() {
            return Err(InhabitanceError::NominalArity {
                declaration: declaration.clone(),
                parameters: parameters.len(),
                arguments: arguments.len(),
            });
        }
        Ok(parameters
            .iter()
            .cloned()
            .zip(arguments.iter().copied())
            .collect())
    }

    fn field_type(&self, field: &CanonicalTarget) -> Result<Option<TypeId>, InhabitanceError> {
        match self.signatures.declaration(field) {
            FactState::Known(DeclarationSignature::Field { ty, .. }) => Ok(Some(*ty)),
            FactState::Known(_) => Err(InhabitanceError::UnexpectedSignature {
                declaration: field.clone(),
                expected: "storage field",
            }),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => Ok(None),
        }
    }

    fn product_node(
        &mut self,
        members: impl IntoIterator<Item = TypeId>,
    ) -> Result<NodeId, InhabitanceError> {
        let formula = self.product_formula(members)?;
        Ok(self.add_node(formula))
    }

    fn product_formula(
        &mut self,
        members: impl IntoIterator<Item = TypeId>,
    ) -> Result<NodeFormula, InhabitanceError> {
        let dependencies = members
            .into_iter()
            .map(|member| self.build_type(member))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NodeFormula::Product(dependencies.into_boxed_slice()))
    }

    fn sum_node(
        &mut self,
        alternatives: impl IntoIterator<Item = TypeId>,
    ) -> Result<NodeId, InhabitanceError> {
        let formula = self.sum_formula(alternatives)?;
        Ok(self.add_node(formula))
    }

    fn sum_formula(
        &mut self,
        alternatives: impl IntoIterator<Item = TypeId>,
    ) -> Result<NodeFormula, InhabitanceError> {
        let dependencies = alternatives
            .into_iter()
            .map(|alternative| self.build_type(alternative))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NodeFormula::Sum(dependencies.into_boxed_slice()))
    }

    fn add_node(&mut self, formula: NodeFormula) -> NodeId {
        let node = self.nodes.len();
        self.nodes.push(formula);
        node
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TrustedInhabitanceEnvironment, TrustedInhabitanceError, TrustedInhabitanceFact,
        TrustedNominalInhabitance,
    };
    use crate::ExternalDefId;

    #[test]
    fn trusted_environment_orders_facts_and_rejects_duplicates() {
        let first = ExternalDefId::new("std::test::First").unwrap();
        let second = ExternalDefId::new("std::test::Second").unwrap();
        let environment = TrustedInhabitanceEnvironment::new([
            TrustedInhabitanceFact::new(second.clone(), TrustedNominalInhabitance::Inhabited),
            TrustedInhabitanceFact::new(first.clone(), TrustedNominalInhabitance::Unknown),
        ])
        .unwrap();
        assert_eq!(
            environment
                .facts()
                .map(|fact| fact.declaration().as_str())
                .collect::<Vec<_>>(),
            [first.as_str(), second.as_str()]
        );
        assert!(matches!(
            TrustedInhabitanceEnvironment::new([
                TrustedInhabitanceFact::new(
                    first.clone(),
                    TrustedNominalInhabitance::Unknown,
                ),
                TrustedInhabitanceFact::new(
                    first.clone(),
                    TrustedNominalInhabitance::Inhabited,
                ),
            ]),
            Err(TrustedInhabitanceError::DuplicateDeclaration(found)) if found == first
        ));
    }
}
