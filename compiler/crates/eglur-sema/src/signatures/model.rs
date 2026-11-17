use std::collections::{BTreeMap, HashMap, HashSet};

use eglur_diagnostics::Poison;
use eglur_hir::{BodyId, OriginId, PatternId, TypeRefId};
use eglur_source::FileId;

use super::{
    ConcreteOwnerHead, GenericParameterRecord, GenericParameterRecords, GenericParameterSignature,
    InherentMethodInventory,
};
use crate::type_rendering::{DiagnosticTypeFacts, DiagnosticTypeNames};
use crate::{
    CallableSignatureId, CanonicalTarget, CanonicalTypeStore, DefId, FactState, FactTable,
    GenericParameterId, InhabitanceFacts, LanguageItems, NameKey, SemanticFact, TraitRef, TypeId,
};

/// File-qualified source type occurrence identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TypeOccurrenceId {
    pub(super) file: FileId,
    pub(super) type_ref: TypeRefId,
}

impl TypeOccurrenceId {
    #[must_use]
    pub const fn new(file: FileId, type_ref: TypeRefId) -> Self {
        Self { file, type_ref }
    }

    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    #[must_use]
    pub const fn type_ref(self) -> TypeRefId {
        self.type_ref
    }
}

/// Canonical result for one written source type, retaining its diagnostic provenance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTypeOccurrence {
    pub(super) resolved: ResolvedType,
    pub(super) origin: OriginId,
    pub(super) written_target: Option<CanonicalTarget>,
}

impl ResolvedTypeOccurrence {
    #[must_use]
    pub const fn resolved(&self) -> &ResolvedType {
        &self.resolved
    }

    #[must_use]
    pub const fn canonical_type(&self) -> Option<TypeId> {
        match self.resolved {
            ResolvedType::Type(ty) => Some(ty),
            ResolvedType::Trait(_) => None,
        }
    }

    #[must_use]
    pub const fn trait_ref(&self) -> Option<&TraitRef> {
        match &self.resolved {
            ResolvedType::Type(_) => None,
            ResolvedType::Trait(trait_ref) => Some(trait_ref),
        }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn written_target(&self) -> Option<&CanonicalTarget> {
        self.written_target.as_ref()
    }
}

/// Semantic category resolved from a source type-reference occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedType {
    Type(TypeId),
    Trait(TraitRef),
}

/// One callable parameter after declaration-signature resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableParameter {
    pub(super) name: Option<Box<str>>,
    pub(super) ty: TypeId,
    pub(super) source_pattern: Option<PatternId>,
}

impl CallableParameter {
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[must_use]
    pub const fn ty(&self) -> TypeId {
        self.ty
    }

    #[must_use]
    pub const fn source_pattern(&self) -> Option<PatternId> {
        self.source_pattern
    }
}

/// Closed applicability requirements attached to one trusted callable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallableRequirement {
    FixedWidthInteger(TypeId),
}

/// One overload/signature identity, distinct from its declaration and body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableSignature {
    pub(super) id: CallableSignatureId,
    pub(super) declaration: CanonicalTarget,
    pub(super) bang: bool,
    pub(super) generic_parameters: Box<[GenericParameterId]>,
    pub(super) parameters: Box<[CallableParameter]>,
    pub(super) result: TypeId,
    pub(super) requirements: Box<[CallableRequirement]>,
    pub(super) body: Option<BodyId>,
}

impl CallableSignature {
    #[must_use]
    pub const fn id(&self) -> CallableSignatureId {
        self.id
    }

    #[must_use]
    pub const fn declaration(&self) -> &CanonicalTarget {
        &self.declaration
    }

    #[must_use]
    pub const fn is_bang(&self) -> bool {
        self.bang
    }

    #[must_use]
    pub fn generic_parameters(&self) -> &[GenericParameterId] {
        &self.generic_parameters
    }

    #[must_use]
    pub fn parameters(&self) -> &[CallableParameter] {
        &self.parameters
    }

    #[must_use]
    pub const fn result(&self) -> TypeId {
        self.result
    }

    #[must_use]
    pub fn requirements(&self) -> &[CallableRequirement] {
        &self.requirements
    }

    #[must_use]
    pub const fn body(&self) -> Option<BodyId> {
        self.body
    }

    pub(crate) fn with_specialized_contract(
        &self,
        generic_parameters: impl IntoIterator<Item = GenericParameterId>,
        parameter_types: impl IntoIterator<Item = TypeId>,
        result: TypeId,
    ) -> Option<Self> {
        let parameter_types = parameter_types.into_iter().collect::<Vec<_>>();
        if parameter_types.len() != self.parameters.len() {
            return None;
        }
        let parameters = self
            .parameters
            .iter()
            .zip(parameter_types)
            .map(|(parameter, ty)| CallableParameter {
                name: parameter.name.clone(),
                ty,
                source_pattern: parameter.source_pattern,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Some(Self {
            id: self.id,
            declaration: self.declaration.clone(),
            bang: self.bang,
            generic_parameters: generic_parameters
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            parameters,
            result,
            requirements: self.requirements.clone(),
            body: self.body,
        })
    }
}

/// Canonical payload contract for one intrinsic constructor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstructorPayloadSignature {
    Unit,
    Positional(Box<[TypeId]>),
}

/// Canonical payload and result contract for one intrinsic constructor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConstructorSignature {
    pub(super) declaration: CanonicalTarget,
    pub(super) generic_parameters: Box<[GenericParameterId]>,
    pub(super) payload: ConstructorPayloadSignature,
    pub(super) result: TypeId,
}

impl ConstructorSignature {
    #[must_use]
    pub const fn declaration(&self) -> &CanonicalTarget {
        &self.declaration
    }

    #[must_use]
    pub fn generic_parameters(&self) -> &[GenericParameterId] {
        &self.generic_parameters
    }

    #[must_use]
    pub const fn payload(&self) -> &ConstructorPayloadSignature {
        &self.payload
    }

    #[must_use]
    pub const fn result(&self) -> TypeId {
        self.result
    }
}

/// Resolved value assigned to one trait-associated type by an impl.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssociatedBindingSignature {
    pub(super) declaration: CanonicalTarget,
    pub(super) associated: CanonicalTarget,
    pub(super) value: TypeId,
}

/// Canonical impl head retained independently from later impl-body validity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitImplHeader {
    pub(super) declaration: CanonicalTarget,
    pub(super) generic_parameters: Box<[GenericParameterId]>,
    pub(super) trait_ref: TraitRef,
    pub(super) target: TypeId,
}

/// Canonical membership and default status of one callable trait method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TraitMethodSignature {
    declaration: CanonicalTarget,
    name: NameKey,
    has_default: bool,
}

impl TraitMethodSignature {
    pub(super) const fn new(
        declaration: CanonicalTarget,
        name: NameKey,
        has_default: bool,
    ) -> Self {
        Self {
            declaration,
            name,
            has_default,
        }
    }

    #[must_use]
    pub(crate) const fn declaration(&self) -> &CanonicalTarget {
        &self.declaration
    }

    #[must_use]
    pub(crate) const fn name(&self) -> &NameKey {
        &self.name
    }

    #[must_use]
    pub(crate) const fn has_default(&self) -> bool {
        self.has_default
    }
}

/// Exact callable declared for one trusted implementation method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeclaredImplMethod {
    trait_method: CanonicalTarget,
    callable: CanonicalTarget,
}

impl DeclaredImplMethod {
    pub(super) const fn new(trait_method: CanonicalTarget, callable: CanonicalTarget) -> Self {
        Self {
            trait_method,
            callable,
        }
    }

    #[must_use]
    pub(crate) const fn trait_method(&self) -> &CanonicalTarget {
        &self.trait_method
    }

    #[must_use]
    pub(crate) const fn callable(&self) -> &CanonicalTarget {
        &self.callable
    }
}

/// Canonical functional adapter relation for one checked callable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraitAdapterSignature {
    method: CanonicalTarget,
    argument: usize,
}

impl TraitAdapterSignature {
    pub(super) const fn new(method: CanonicalTarget, argument: usize) -> Self {
        Self { method, argument }
    }

    #[must_use]
    pub const fn method(&self) -> &CanonicalTarget {
        &self.method
    }

    #[must_use]
    pub const fn argument(&self) -> usize {
        self.argument
    }
}

/// One trusted prelude trait binding retained with its shadowable type name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PreludeTraitSignature {
    name: NameKey,
    declaration: CanonicalTarget,
}

impl PreludeTraitSignature {
    pub(super) const fn new(name: NameKey, declaration: CanonicalTarget) -> Self {
        Self { name, declaration }
    }

    #[must_use]
    pub(crate) const fn name(&self) -> &NameKey {
        &self.name
    }

    #[must_use]
    pub(crate) const fn declaration(&self) -> &CanonicalTarget {
        &self.declaration
    }
}

impl TraitImplHeader {
    #[must_use]
    pub const fn declaration(&self) -> &CanonicalTarget {
        &self.declaration
    }

    #[must_use]
    pub fn generic_parameters(&self) -> &[GenericParameterId] {
        &self.generic_parameters
    }

    #[must_use]
    pub const fn trait_ref(&self) -> &TraitRef {
        &self.trait_ref
    }

    #[must_use]
    pub const fn target(&self) -> TypeId {
        self.target
    }
}

impl AssociatedBindingSignature {
    #[must_use]
    pub const fn declaration(&self) -> &CanonicalTarget {
        &self.declaration
    }

    #[must_use]
    pub const fn associated(&self) -> &CanonicalTarget {
        &self.associated
    }

    #[must_use]
    pub const fn value(&self) -> TypeId {
        self.value
    }
}

/// Canonical signature fact attached to a source or trusted declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeclarationSignature {
    Callable(CallableSignatureId),
    Constructor(ConstructorSignature),
    Trait {
        closed: bool,
        generic_parameters: Box<[GenericParameterId]>,
        associated_types: Box<[CanonicalTarget]>,
    },
    Impl {
        associated_bindings: Box<[AssociatedBindingSignature]>,
    },
    Struct {
        generic_parameters: Box<[GenericParameterId]>,
        storage_fields: Box<[CanonicalTarget]>,
        properties: Box<[CanonicalTarget]>,
    },
    Nominal {
        generic_parameters: Box<[GenericParameterId]>,
    },
    Enum {
        generic_parameters: Box<[GenericParameterId]>,
        variants: Box<[CanonicalTarget]>,
    },
    EnumVariant {
        owner: CanonicalTarget,
        name: NameKey,
        payload: EnumVariantPayloadSignature,
    },
    Field {
        name: NameKey,
        ty: TypeId,
    },
    TypeAlias {
        generic_parameters: Box<[GenericParameterId]>,
        value: TypeId,
    },
    Const {
        ty: TypeId,
    },
    AssociatedType {
        owner_trait: CanonicalTarget,
    },
    AssociatedTypeBinding(AssociatedBindingSignature),
    Property {
        ty: TypeId,
    },
    GenericParameter(GenericParameterSignature),
}

/// Canonical payload shape for one source or trusted enum variant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnumVariantPayloadSignature {
    Absent,
    Tuple(Box<[TypeId]>),
    Named(Box<[CanonicalTarget]>),
}

/// Completed canonical signature graph for one semantic compilation graph.
pub struct DeclarationSignatures {
    pub(super) type_applications: super::applications::TypeApplicationFacts,
    pub(super) public_contract_poisons: BTreeMap<CanonicalTarget, Poison>,
    pub(super) types: CanonicalTypeStore,
    pub(super) diagnostic_type_names: DiagnosticTypeNames,
    pub(super) declarations: FactTable<CanonicalTarget, DeclarationSignature>,
    pub(super) enum_variant_names: HashMap<CanonicalTarget, SemanticFact<HashSet<NameKey>>>,
    pub(super) overload_family_poisons: BTreeMap<DefId, Poison>,
    pub(super) generic_parameters: GenericParameterRecords,
    pub(super) occurrences: FactTable<TypeOccurrenceId, ResolvedTypeOccurrence>,
    pub(super) callables: Box<[CallableSignature]>,
    pub(super) impl_headers: FactTable<CanonicalTarget, TraitImplHeader>,
    pub(super) rejected_impl_headers: BTreeMap<CanonicalTarget, TraitImplHeader>,
    pub(super) impl_later_roots: BTreeMap<DefId, Poison>,
    pub(super) trait_methods: BTreeMap<CanonicalTarget, Box<[TraitMethodSignature]>>,
    pub(super) trait_method_owners: BTreeMap<CanonicalTarget, CanonicalTarget>,
    pub(super) declared_impl_methods: BTreeMap<CanonicalTarget, Box<[DeclaredImplMethod]>>,
    pub(super) trait_adapters: BTreeMap<CanonicalTarget, TraitAdapterSignature>,
    pub(super) trusted_inherent_owners: BTreeMap<ConcreteOwnerHead, InherentMethodInventory>,
    pub(super) prelude_traits: Box<[PreludeTraitSignature]>,
    pub(super) language_items: LanguageItems,
}

impl DeclarationSignatures {
    /// None denotes unavailable membership; Some may be a complete empty inventory.
    #[must_use]
    pub fn trusted_inherent_owner(
        &self,
        head: &ConcreteOwnerHead,
    ) -> Option<&InherentMethodInventory> {
        self.trusted_inherent_owners.get(head)
    }

    #[must_use]
    pub const fn types(&self) -> &CanonicalTypeStore {
        &self.types
    }

    pub(crate) const fn types_mut(&mut self) -> &mut CanonicalTypeStore {
        &mut self.types
    }

    #[must_use]
    pub const fn language_items(&self) -> &LanguageItems {
        &self.language_items
    }

    pub(crate) const fn diagnostic_type_names(&self) -> &DiagnosticTypeNames {
        &self.diagnostic_type_names
    }

    #[must_use]
    pub fn declaration(
        &self,
        declaration: &CanonicalTarget,
    ) -> FactState<'_, DeclarationSignature> {
        if let Some(poison) = self.application_poison(declaration) {
            return FactState::Poisoned(poison);
        }
        if let Some(poison) = self.public_contract_poisons.get(declaration) {
            return FactState::Poisoned(poison);
        }
        declaration_fact_state(
            &self.declarations,
            &self.overload_family_poisons,
            declaration,
        )
    }

    #[must_use]
    pub fn source_declaration(&self, declaration: DefId) -> FactState<'_, DeclarationSignature> {
        self.declaration(&CanonicalTarget::Source(declaration))
    }

    pub(crate) fn enum_variant_names(
        &self,
        declaration: &CanonicalTarget,
    ) -> FactState<'_, HashSet<NameKey>> {
        match self.enum_variant_names.get(declaration) {
            Some(SemanticFact::Known(names)) => FactState::Known(names),
            Some(SemanticFact::Poisoned(poison)) => FactState::Poisoned(poison),
            Some(SemanticFact::NotApplicable) => FactState::NotApplicable,
            None => FactState::Unperformed,
        }
    }

    #[must_use]
    pub fn generic_parameter(
        &self,
        parameter: &GenericParameterId,
    ) -> FactState<'_, GenericParameterSignature> {
        match self.generic_parameter_record(parameter) {
            FactState::Known(record) => FactState::Known(record.signature()),
            FactState::Poisoned(poison) => FactState::Poisoned(poison),
            FactState::NotApplicable => FactState::NotApplicable,
            FactState::Unperformed => FactState::Unperformed,
        }
    }

    /// Retains original definition identity under the same effective poison as semantic queries.
    #[must_use]
    pub fn generic_parameter_record(
        &self,
        parameter: &GenericParameterId,
    ) -> FactState<'_, GenericParameterRecord> {
        effective_generic_parameter_record(
            &self.generic_parameters,
            &self.type_applications,
            &self.public_contract_poisons,
            parameter,
        )
    }

    pub(crate) fn generic_parameter_record_with_types(
        &mut self,
        parameter: &GenericParameterId,
    ) -> (
        FactState<'_, GenericParameterRecord>,
        &mut CanonicalTypeStore,
    ) {
        let record = effective_generic_parameter_record(
            &self.generic_parameters,
            &self.type_applications,
            &self.public_contract_poisons,
            parameter,
        );
        (record, &mut self.types)
    }

    #[must_use]
    pub fn occurrence(
        &self,
        occurrence: TypeOccurrenceId,
    ) -> FactState<'_, ResolvedTypeOccurrence> {
        self.occurrences.state(&occurrence)
    }

    #[must_use]
    pub fn callable(&self, id: CallableSignatureId) -> Option<&CallableSignature> {
        self.callables.get(id.index())
    }

    #[must_use]
    pub fn impl_header(&self, declaration: &CanonicalTarget) -> FactState<'_, TraitImplHeader> {
        if let Some(poison) = self.application_poison(declaration) {
            return FactState::Poisoned(poison);
        }
        if let Some(poison) = self.public_contract_poisons.get(declaration) {
            return FactState::Poisoned(poison);
        }
        self.impl_headers.state(declaration)
    }

    pub(crate) fn poison_public_contract(
        &mut self,
        declaration: CanonicalTarget,
        poison: Poison,
    ) -> bool {
        match self.public_contract_poisons.entry(declaration) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(poison);
                true
            }
            std::collections::btree_map::Entry::Occupied(_) => false,
        }
    }

    pub(crate) fn impl_headers(
        &self,
    ) -> impl ExactSizeIterator<Item = (&CanonicalTarget, &SemanticFact<TraitImplHeader>)> {
        self.impl_headers.iter()
    }

    pub(crate) fn rejected_impl_headers(
        &self,
    ) -> impl ExactSizeIterator<Item = (&CanonicalTarget, &TraitImplHeader)> {
        self.rejected_impl_headers.iter()
    }

    pub(crate) fn impl_later_roots(&self, declaration: DefId) -> Option<&Poison> {
        self.impl_later_roots.get(&declaration)
    }

    #[must_use]
    pub(crate) fn trait_methods(&self, declaration: &CanonicalTarget) -> &[TraitMethodSignature] {
        self.trait_methods
            .get(declaration)
            .map_or(&[], AsRef::as_ref)
    }

    #[must_use]
    pub fn trait_method_owner(&self, method: &CanonicalTarget) -> Option<&CanonicalTarget> {
        self.trait_method_owners.get(method)
    }

    #[must_use]
    pub(crate) fn declared_impl_methods(
        &self,
        implementation: &CanonicalTarget,
    ) -> &[DeclaredImplMethod] {
        self.declared_impl_methods
            .get(implementation)
            .map_or(&[], AsRef::as_ref)
    }

    #[must_use]
    pub fn trait_adapter(&self, callable: &CanonicalTarget) -> Option<&TraitAdapterSignature> {
        self.trait_adapters.get(callable)
    }

    #[must_use]
    pub(crate) fn prelude_traits(&self) -> &[PreludeTraitSignature] {
        &self.prelude_traits
    }

    pub(crate) fn declarations(
        &self,
    ) -> impl ExactSizeIterator<Item = (&CanonicalTarget, &SemanticFact<DeclarationSignature>)>
    {
        self.declarations.iter()
    }

    #[must_use]
    pub fn declaration_fact_count(&self) -> usize {
        self.declarations.iter().len()
            + self
                .trait_methods
                .values()
                .map(|methods| methods.len())
                .sum::<usize>()
            + self
                .declared_impl_methods
                .values()
                .map(|methods| methods.len())
                .sum::<usize>()
            + self.trait_adapters.len()
    }

    #[must_use]
    pub fn occurrence_fact_count(&self) -> usize {
        self.occurrences.iter().len()
    }

    pub(crate) fn has_poisoned_facts(&self) -> bool {
        self.has_invalid_applications()
            || !self.public_contract_poisons.is_empty()
            || self.declarations.has_poisoned()
            || !self.overload_family_poisons.is_empty()
            || self.generic_parameters.has_poisoned()
            || self.occurrences.has_poisoned()
            || self.impl_headers.has_poisoned()
    }
}

pub(super) fn declaration_fact_state<'a>(
    declarations: &'a FactTable<CanonicalTarget, DeclarationSignature>,
    overload_family_poisons: &'a BTreeMap<DefId, Poison>,
    declaration: &CanonicalTarget,
) -> FactState<'a, DeclarationSignature> {
    if let CanonicalTarget::Source(declaration) = declaration
        && let Some(poison) = overload_family_poisons.get(declaration)
    {
        return FactState::Poisoned(poison);
    }
    declarations.state(declaration)
}

impl DiagnosticTypeFacts for DeclarationSignatures {
    fn types(&self) -> &CanonicalTypeStore {
        &self.types
    }

    fn generic_parameter_name(&self, parameter: &GenericParameterId) -> Option<&str> {
        // Use-site poison invalidates bounds, not the retained declaration's name.
        match self.generic_parameters.state(parameter) {
            FactState::Known(parameter) => Some(parameter.name()),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => None,
        }
    }

    fn generic_parameter_bounds(&self, parameter: &GenericParameterId) -> Option<&[TraitRef]> {
        match self.generic_parameter(parameter) {
            FactState::Known(parameter) => Some(parameter.bounds()),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => None,
        }
    }
}

/// Declaration-header products that share one canonical type graph.
pub struct ResolvedDeclarationSemantics {
    pub(super) signatures: DeclarationSignatures,
    pub(super) inhabitance: InhabitanceFacts,
}

impl ResolvedDeclarationSemantics {
    #[must_use]
    pub const fn signatures(&self) -> &DeclarationSignatures {
        &self.signatures
    }

    #[must_use]
    pub const fn inhabitance(&self) -> &InhabitanceFacts {
        &self.inhabitance
    }

    pub fn signatures_mut(&mut self) -> &mut DeclarationSignatures {
        &mut self.signatures
    }

    pub(crate) fn parts_mut(&mut self) -> (&mut DeclarationSignatures, &mut InhabitanceFacts) {
        (&mut self.signatures, &mut self.inhabitance)
    }

    #[must_use]
    pub fn into_parts(self) -> (DeclarationSignatures, InhabitanceFacts) {
        (self.signatures, self.inhabitance)
    }
}

fn effective_generic_parameter_record<'a>(
    records: &'a GenericParameterRecords,
    applications: &'a super::applications::TypeApplicationFacts,
    public_contract_poisons: &'a BTreeMap<CanonicalTarget, Poison>,
    parameter: &GenericParameterId,
) -> FactState<'a, GenericParameterRecord> {
    if let GenericParameterId::Source(parameter) = parameter {
        let target = CanonicalTarget::Source(*parameter);
        if let Some(poison) = applications
            .poison(&target)
            .or_else(|| public_contract_poisons.get(&target))
        {
            return FactState::Poisoned(poison);
        }
    }
    records.record(parameter)
}
