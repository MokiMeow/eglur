use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use eglur_diagnostics::{
    DiagnosticDataValue, DiagnosticStore, DiagnosticStoreError, Poison, PoisonAccumulator,
    RelatedSpan, SourceObligationKey, SourceSpan,
};
use eglur_hir::{
    BodyId, EnumVariantPayload, ExprKind, FunctionBody, GenericParameter, HirFile, ItemKind,
    LiteralKind, Mutability, Name, OriginId, Parameter, PatternKind, PointerMutability, StmtKind,
    TypeRefId, TypeRefKind, is_residual_constructor_name,
};
use eglur_source::FileId;

use super::model::declaration_fact_state;
use super::*;
use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    TypecheckCapability,
};
use crate::diagnostic::{
    record_name_ambiguous, record_name_not_found, record_type_error,
    record_type_error_for_obligation, record_type_error_with_related, record_visibility_violation,
    source_obligation_key,
};
use crate::overloads::{OverloadPairConflict, classify_overload_pair};
use crate::traits::{
    CoherenceIndex, HeaderConflict, TraitHeaderFacts, classify_overlap, header_mentions_parameter,
    header_parameter_below_error_union,
};
use crate::type_namespace::{
    TypeNameLookup, lookup_external_module_type, lookup_prelude_type, lookup_source_module_type,
};
use crate::type_rendering::{DiagnosticTypeFacts, DiagnosticTypeNames, DiagnosticTypeRenderer};
use crate::types::is_reserved_type_name;
use crate::{
    AssociatedProjection, CallableSignatureId, CanonicalTarget, CanonicalTypeStore, Declaration,
    DeclarationCollection, DeclarationKind, DefId, ExternalDefId, ExternalNameEnvironment,
    FactState, FactTable, GenericParameterId, InhabitanceFacts, IntrinsicNominal, LanguageItem,
    LanguageItems, ModuleAddress, ModuleDeclarations, ModuleId, NameKey, Namespace, NominalHead,
    NominalInhabitanceProduct, NominalInhabitanceRule, RawPointerMutability, ReferenceMutability,
    ResolvedImports, SemanticFact, SemanticLocation, SemanticSource, TraitRef,
    TrustedInhabitanceEnvironment, TrustedLanguageItemEnvironment, TrustedNominalInhabitance,
    TypeId, TypeKind, TypeStoreError,
};

mod associated_bindings;
mod contexts;
mod declarations;
mod diagnostics;
mod enum_variants;
mod enums;
mod generic_names;
mod generic_redeclarations;
mod impl_headers;
mod source_types;
#[cfg(test)]
mod tests;
mod trusted_inherent;
mod trusted_resolution;

/// Boundary inputs selected by the driver for declaration-signature resolution.
pub struct SignatureResolutionInputs<'a> {
    external_names: Option<&'a ExternalNameEnvironment>,
    trusted: Option<TrustedDeclarationInputs<'a>>,
    deferred_entry_source: Option<FileId>,
}

impl<'a> SignatureResolutionInputs<'a> {
    #[must_use]
    pub const fn new(
        external_names: Option<&'a ExternalNameEnvironment>,
        trusted: Option<TrustedDeclarationInputs<'a>>,
    ) -> Self {
        Self {
            external_names,
            trusted,
            deferred_entry_source: None,
        }
    }

    #[must_use]
    pub const fn with_deferred_entry_source(mut self, source: FileId) -> Self {
        self.deferred_entry_source = Some(source);
        self
    }
}

/// Resolves declaration headers into one canonical type graph without checking bodies.
pub fn resolve_declaration_signatures(
    collection: &DeclarationCollection,
    imports: &ResolvedImports,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    inputs: SignatureResolutionInputs<'_>,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<ResolvedDeclarationSemantics>, SignatureResolutionError> {
    let mut recorder = DemandRecorder::new(support);
    let sources = sources_by_file(collection, sources)?;
    let capability =
        CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures);
    let mut supported = true;
    for module in collection.modules() {
        supported &= recorder.require(capability, SemanticLocation::source(module.file()));
    }
    if !supported {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }

    let mut resolver = SignatureResolver::new(
        collection,
        imports,
        sources,
        inputs.deferred_entry_source,
        inputs.external_names,
        inputs.trusted,
        diagnostics,
    )?;
    resolver.resolve_trusted()?;
    resolver.resolve_trusted_inherent_owners()?;
    resolver.validate_language_items()?;
    resolver.resolve_source_generics()?;
    resolver.prime_source_impl_headers()?;
    resolver.validate_source_impl_headers()?;
    resolver.resolve_source_declarations()?;
    resolver.validate_source_overload_families()?;
    resolver.resolve_body_type_occurrences()?;
    resolver.index_enum_variant_names()?;
    let trusted_inhabitance = resolver.resolve_trusted_inhabitance()?;
    let signatures = resolver.finish();
    let inhabitance = InhabitanceFacts::new(trusted_inhabitance)?;
    Ok(
        recorder.finish(PhaseProduct::Performed(ResolvedDeclarationSemantics {
            signatures,
            inhabitance,
        })),
    )
}

#[derive(Clone)]
struct TraitInfo {
    arity: usize,
    closed: bool,
    associated: BTreeMap<Box<str>, Box<[CanonicalTarget]>>,
}

#[derive(Clone, Copy)]
enum SourceCallable<'hir> {
    Function(&'hir eglur_hir::FunctionDecl),
    Extern(&'hir eglur_hir::ExternDecl),
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct BodyTypeOwner {
    declaration: DefId,
    allow_self: bool,
}

fn body_owners(
    module: &ModuleDeclarations,
    hir: &HirFile,
) -> Result<BTreeMap<BodyId, BodyTypeOwner>, SignatureResolutionError> {
    let mut owners = BTreeMap::new();
    for declaration in module.declarations() {
        let Some(item) = declaration.item().map(|item| hir.item(item).kind()) else {
            continue;
        };
        let (body, allow_self) = match item {
            ItemKind::Function(function) => (Some(function.body), true),
            ItemKind::Extern(function) => (Some(function.body), false),
            ItemKind::Const(value) => (Some(value.body), false),
            ItemKind::StorageField(field) => {
                (field.initializer.map(|initializer| initializer.body), false)
            }
            _ => (None, false),
        };
        if let Some(body) = body {
            let _ = insert_body_owner(
                &mut owners,
                body,
                BodyTypeOwner {
                    declaration: declaration.id(),
                    allow_self,
                },
            )?;
        }
    }

    let mut pending = owners.keys().copied().collect::<Vec<_>>();
    let mut cursor = 0;
    while let Some(&body_id) = pending.get(cursor) {
        cursor += 1;
        let owner = owners[&body_id];
        for (_, expression) in hir.body(body_id).expressions() {
            if let ExprKind::Lambda(lambda) = expression.kind()
                && insert_body_owner(&mut owners, lambda.body, owner)?
            {
                pending.push(lambda.body);
            }
        }
    }
    Ok(owners)
}

fn insert_body_owner(
    owners: &mut BTreeMap<BodyId, BodyTypeOwner>,
    body: BodyId,
    owner: BodyTypeOwner,
) -> Result<bool, SignatureResolutionError> {
    match owners.entry(body) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(owner);
            Ok(true)
        }
        std::collections::btree_map::Entry::Occupied(entry) if *entry.get() == owner => Ok(false),
        std::collections::btree_map::Entry::Occupied(_) => Err(
            SignatureResolutionError::WrongDeclarationItem(owner.declaration),
        ),
    }
}

impl<'hir> SourceCallable<'hir> {
    fn name(self) -> &'hir Name {
        match self {
            Self::Function(declaration) => &declaration.name,
            Self::Extern(declaration) => &declaration.name,
        }
    }

    fn parameters(self) -> &'hir [Parameter] {
        match self {
            Self::Function(declaration) => &declaration.parameters,
            Self::Extern(declaration) => &declaration.parameters,
        }
    }

    fn return_type(self) -> Option<TypeRefId> {
        match self {
            Self::Function(declaration) => declaration.return_type,
            Self::Extern(declaration) => declaration.return_type,
        }
    }

    fn body(self) -> BodyId {
        match self {
            Self::Function(declaration) => declaration.body,
            Self::Extern(declaration) => declaration.body,
        }
    }

    fn type_positions(self) -> (TypePosition, TypePosition) {
        match self {
            Self::Function(_) => (
                TypePosition::CallableBoundary,
                TypePosition::CallableBoundary,
            ),
            Self::Extern(_) => (TypePosition::ExternParameter, TypePosition::ExternReturn),
        }
    }
}

struct SignatureResolver<'input, 'hir> {
    type_applications: super::applications::TypeApplicationFacts,
    collection: &'input DeclarationCollection,
    imports: &'input ResolvedImports,
    sources: BTreeMap<FileId, &'input SemanticSource<'hir>>,
    deferred_entry_source: Option<FileId>,
    external_names: Option<&'input ExternalNameEnvironment>,
    trusted: Option<&'input TrustedSignatureEnvironment>,
    trusted_inhabitance: Option<&'input TrustedInhabitanceEnvironment>,
    trusted_language_items: Option<&'input TrustedLanguageItemEnvironment>,
    diagnostics: &'input mut DiagnosticStore,
    types: CanonicalTypeStore,
    diagnostic_type_names: DiagnosticTypeNames,
    declarations: FactTable<CanonicalTarget, DeclarationSignature>,
    enum_variant_names: HashMap<CanonicalTarget, SemanticFact<HashSet<NameKey>>>,
    source_enum_variants: enum_variants::SourceEnumVariantIndex<'hir>,
    overload_family_poisons: BTreeMap<DefId, Poison>,
    generic_parameters: FactTable<GenericParameterId, GenericParameterSignature>,
    occurrences: FactTable<TypeOccurrenceId, ResolvedTypeOccurrence>,
    callables: Vec<CallableSignature>,
    trait_methods: BTreeMap<CanonicalTarget, Box<[TraitMethodSignature]>>,
    trait_method_owners: BTreeMap<CanonicalTarget, CanonicalTarget>,
    declared_impl_methods: BTreeMap<CanonicalTarget, Box<[DeclaredImplMethod]>>,
    trait_adapters: BTreeMap<CanonicalTarget, TraitAdapterSignature>,
    trusted_inherent_owners: BTreeMap<ConcreteOwnerHead, InherentMethodInventory>,
    prelude_traits: BTreeSet<PreludeTraitSignature>,
    generic_types: BTreeMap<GenericParameterId, TypeId>,
    generic_bounds: BTreeMap<GenericParameterId, Box<[TraitRef]>>,
    source_generic_redeclarations: BTreeSet<DefId>,
    source_generic_names: std::rc::Rc<generic_names::SourceGenericNameIndex>,
    frozen_source_generic_scopes: Option<BTreeMap<DefId, Resolution<()>>>,
    static_self_bindings: BTreeMap<DefId, SelfBinding>,
    source_generic_bound_seeds: BTreeMap<(DefId, TypeRefId), Resolution<TraitRef>>,
    source_generic_states: BTreeMap<DefId, SourceGenericState>,
    source_generic_stack: Vec<DefId>,
    trait_info: BTreeMap<CanonicalTarget, TraitInfo>,
    alias_arities: BTreeMap<DefId, usize>,
    alias_states: BTreeMap<DefId, AliasState>,
    impl_headers: BTreeMap<DefId, TraitImplHeader>,
    impl_header_poison: BTreeMap<DefId, Poison>,
    retained_impl_headers: FactTable<CanonicalTarget, TraitImplHeader>,
    rejected_impl_headers: BTreeMap<CanonicalTarget, TraitImplHeader>,
    impl_later_roots: BTreeMap<DefId, Poison>,
}

impl TraitHeaderFacts for SignatureResolver<'_, '_> {
    fn header_types(&self) -> &CanonicalTypeStore {
        &self.types
    }

    fn header_generic_parameter(
        &self,
        parameter: &GenericParameterId,
    ) -> FactState<'_, GenericParameterSignature> {
        self.generic_parameters.state(parameter)
    }
}

impl DiagnosticTypeFacts for SignatureResolver<'_, '_> {
    fn types(&self) -> &CanonicalTypeStore {
        &self.types
    }

    fn generic_parameter_name(&self, parameter: &GenericParameterId) -> Option<&str> {
        match self.generic_parameters.state(parameter) {
            FactState::Known(parameter) => Some(parameter.name()),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => None,
        }
    }

    fn generic_parameter_bounds(&self, parameter: &GenericParameterId) -> Option<&[TraitRef]> {
        match self.generic_parameters.state(parameter) {
            FactState::Known(parameter) => Some(parameter.bounds()),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => None,
        }
    }
}

fn item_generic_parameters(kind: &ItemKind) -> Option<&[GenericParameter]> {
    match kind {
        ItemKind::Function(value) => Some(&value.generic_parameters),
        ItemKind::Trait(value) => Some(&value.generic_parameters),
        ItemKind::Impl(value) => Some(&value.generic_parameters),
        ItemKind::Struct(value) => Some(&value.generic_parameters),
        ItemKind::Enum(value) => Some(&value.generic_parameters),
        ItemKind::TypeAlias(value) => Some(&value.generic_parameters),
        _ => None,
    }
}

fn type_ref_contains_generic_projection(
    hir: &HirFile,
    root: TypeRefId,
    generics: &generic_names::SourceGenericNames,
) -> bool {
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        match hir.type_ref(current).kind() {
            TypeRefKind::Reference { referent, .. }
            | TypeRefKind::Pointer { referent, .. }
            | TypeRefKind::Group(referent)
            | TypeRefKind::Dynamic {
                trait_ref: referent,
            } => pending.push(*referent),
            TypeRefKind::Path { path, arguments } => {
                if path.segments().len() > 1
                    && path.segments()[0]
                        .text()
                        .is_some_and(|base| generics.contains_key(base))
                {
                    return true;
                }
                pending.extend(arguments.iter().copied());
            }
            TypeRefKind::Tuple(elements) => pending.extend(elements.iter().copied()),
            TypeRefKind::Function { parameters, result } => {
                pending.extend(parameters.iter().copied());
                pending.push(*result);
            }
            TypeRefKind::Invalid => {}
        }
    }
    false
}

fn item_declaration_name(kind: &ItemKind) -> Option<&Name> {
    match kind {
        ItemKind::Function(value) => Some(&value.name),
        ItemKind::Extern(value) => Some(&value.name),
        ItemKind::Trait(value) => Some(&value.name),
        ItemKind::Struct(value) => Some(&value.name),
        ItemKind::Enum(value) => Some(&value.name),
        ItemKind::TypeAlias(value) => Some(&value.name),
        ItemKind::Const(value) => Some(&value.name),
        ItemKind::AssociatedType(value) => Some(&value.name),
        ItemKind::AssociatedTypeBinding(value) => Some(&value.name),
        ItemKind::StorageField(value) => Some(&value.name),
        ItemKind::Property(value) => Some(&value.name),
        ItemKind::Import(_) | ItemKind::Impl(_) | ItemKind::Invalid => None,
    }
}

fn method_receiver_kind(
    parameter: Option<&CallableParameter>,
    self_binding: Option<&SelfBinding>,
    types: &CanonicalTypeStore,
) -> Result<Option<MethodReceiverKind>, TypeStoreError> {
    let (Some(parameter), Some(self_binding)) = (parameter, self_binding) else {
        return Ok(None);
    };
    if parameter.name() != Some("self") {
        return Ok(None);
    }
    if parameter.ty == self_binding.ty {
        return Ok(Some(MethodReceiverKind::Owned));
    }
    let TypeKind::Reference {
        mutability,
        referent,
    } = types.kind(parameter.ty)?
    else {
        return Ok(None);
    };
    if *referent != self_binding.ty {
        return Ok(None);
    }
    Ok(Some(match mutability {
        ReferenceMutability::Shared => MethodReceiverKind::Shared,
        ReferenceMutability::Mutable => MethodReceiverKind::Mutable,
    }))
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum MethodReceiverKind {
    Owned,
    Shared,
    Mutable,
}

#[derive(Clone)]
struct SourceTypeContext {
    generics: generic_names::SourceGenericNames,
    self_binding: Option<SelfBinding>,
    structure_validation: TypeStructureValidation,
    application_region: super::applications::TypeApplicationRegion,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TypeStructureValidation {
    Immediate,
    AfterBindingCycles,
}

#[derive(Clone)]
struct SelfBinding {
    ty: TypeId,
    trait_ref: Option<TraitRef>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TypePosition {
    Ordinary,
    ConstructorDType,
    CallableBoundary,
    LocalBoundary,
    ConditionalOwnership,
    BorrowedOwnership,
    ExternParameter,
    ExternReturn,
    ExternNested,
    ErrorChannel,
}

impl TypePosition {
    const fn is_extern(self) -> bool {
        matches!(
            self,
            Self::ExternParameter | Self::ExternReturn | Self::ExternNested
        )
    }

    const fn nested(self) -> Self {
        if self.is_extern() {
            Self::ExternNested
        } else {
            Self::Ordinary
        }
    }

    const fn reference_referent(self) -> Self {
        if self.is_extern() {
            Self::ExternNested
        } else if matches!(self, Self::CallableBoundary | Self::LocalBoundary) {
            Self::BorrowedOwnership
        } else {
            Self::Ordinary
        }
    }

    const fn intrinsic_argument(self, head: IntrinsicNominal, index: usize) -> Self {
        if self.is_extern() {
            Self::ExternNested
        } else if matches!(head, IntrinsicNominal::Res) && index == 1 {
            Self::ErrorChannel
        } else if matches!(self, Self::CallableBoundary | Self::LocalBoundary)
            && index == 0
            && matches!(head, IntrinsicNominal::Opt | IntrinsicNominal::Res)
        {
            Self::ConditionalOwnership
        } else {
            Self::Ordinary
        }
    }
}

#[derive(Clone)]
enum Resolution<T> {
    Known(T),
    Poisoned(Poison),
    Unperformed,
}

impl<T> Resolution<T> {
    fn map<U>(self, transform: impl FnOnce(T) -> U) -> Resolution<U> {
        match self {
            Self::Known(value) => Resolution::Known(transform(value)),
            Self::Poisoned(poison) => Resolution::Poisoned(poison),
            Self::Unperformed => Resolution::Unperformed,
        }
    }

    fn into_fact(self) -> Option<SemanticFact<T>> {
        match self {
            Self::Known(value) => Some(SemanticFact::Known(value)),
            Self::Poisoned(poison) => Some(SemanticFact::Poisoned(poison)),
            Self::Unperformed => None,
        }
    }
}

impl<T> Resolution<Result<T, TypeStoreError>> {
    fn transpose(self) -> Result<Resolution<T>, TypeStoreError> {
        match self {
            Resolution::Known(value) => value.map(Resolution::Known),
            Resolution::Poisoned(poison) => Ok(Resolution::Poisoned(poison)),
            Resolution::Unperformed => Ok(Resolution::Unperformed),
        }
    }
}

fn reference_mutability(mutability: Mutability) -> ReferenceMutability {
    match mutability {
        Mutability::Immutable => ReferenceMutability::Shared,
        Mutability::Mutable => ReferenceMutability::Mutable,
    }
}

fn raw_pointer_mutability(mutability: PointerMutability) -> RawPointerMutability {
    match mutability {
        PointerMutability::Const => RawPointerMutability::Const,
        PointerMutability::Mutable => RawPointerMutability::Mutable,
    }
}

fn is_abi_numeric(primitive: crate::PrimitiveType) -> bool {
    matches!(
        primitive,
        crate::PrimitiveType::I8
            | crate::PrimitiveType::I16
            | crate::PrimitiveType::I32
            | crate::PrimitiveType::I64
            | crate::PrimitiveType::U8
            | crate::PrimitiveType::U16
            | crate::PrimitiveType::U32
            | crate::PrimitiveType::U64
            | crate::PrimitiveType::F32
            | crate::PrimitiveType::F64
    )
}

fn decode_string_literal(spelling: &str) -> Option<String> {
    let body = spelling.strip_prefix('"')?.strip_suffix('"')?;
    let mut decoded = String::with_capacity(body.len());
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => decoded.push('\\'),
            '"' => decoded.push('"'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'u' => {
                if characters.next()? != '{' {
                    return None;
                }
                let mut digits = String::new();
                loop {
                    let digit = characters.next()?;
                    if digit == '}' {
                        break;
                    }
                    digits.push(digit);
                }
                if digits.is_empty() || digits.len() > 6 {
                    return None;
                }
                let scalar = u32::from_str_radix(&digits, 16).ok()?;
                decoded.push(char::from_u32(scalar)?);
            }
            _ => return None,
        }
    }
    Some(decoded)
}

fn generic_target(parameter: &GenericParameterId) -> CanonicalTarget {
    match parameter {
        GenericParameterId::Source(id) => CanonicalTarget::Source(*id),
        GenericParameterId::Trusted { owner, .. } => CanonicalTarget::External(owner.clone()),
    }
}

fn introduces_reserved_name(kind: DeclarationKind, name: &str) -> bool {
    (matches!(
        kind,
        DeclarationKind::Struct
            | DeclarationKind::Enum
            | DeclarationKind::Trait
            | DeclarationKind::TypeAlias
            | DeclarationKind::GenericParameter
    ) && is_reserved_type_name(name))
        || (matches!(kind, DeclarationKind::Struct | DeclarationKind::EnumVariant)
            && is_residual_constructor_name(name))
}

fn target_label(target: &CanonicalTarget) -> String {
    match target {
        CanonicalTarget::Source(id) => format!("source:{:?}:{}", id.module(), id.local()),
        CanonicalTarget::External(id) => id.as_str().to_string(),
    }
}

fn diagnostic_count(count: usize) -> DiagnosticDataValue {
    DiagnosticDataValue::from(
        i64::try_from(count).expect("u32-bounded semantic collection count fits i64"),
    )
}

#[derive(Clone)]
struct AliasTemplate {
    generic_parameters: Box<[GenericParameterId]>,
    value: TypeId,
}

fn merge_poison(slot: &mut Option<Poison>, poison: &Poison) -> Result<(), DiagnosticStoreError> {
    *slot = Some(match slot {
        Some(existing) => existing.merge(poison)?,
        None => poison.clone(),
    });
    Ok(())
}

fn merge_resolution_poison<T>(
    resolution: Resolution<T>,
    poison: Option<Poison>,
) -> Result<Resolution<T>, DiagnosticStoreError> {
    let Some(poison) = poison else {
        return Ok(resolution);
    };
    Ok(match resolution {
        Resolution::Poisoned(existing) => Resolution::Poisoned(existing.merge(&poison)?),
        Resolution::Known(_) | Resolution::Unperformed => Resolution::Poisoned(poison),
    })
}

fn trusted_generic_parameters(kind: &TrustedSignatureKind) -> &[TrustedGenericParameter] {
    match kind {
        TrustedSignatureKind::Nominal { generic_parameters }
        | TrustedSignatureKind::Enum {
            generic_parameters, ..
        }
        | TrustedSignatureKind::Trait {
            generic_parameters, ..
        }
        | TrustedSignatureKind::Callable {
            generic_parameters, ..
        }
        | TrustedSignatureKind::Constructor {
            generic_parameters, ..
        }
        | TrustedSignatureKind::Impl {
            generic_parameters, ..
        } => generic_parameters,
    }
}

fn result_owner_count(kind: &TrustedSignatureKind) -> usize {
    trusted_generic_parameters(kind).len()
}

fn freeze_associated(
    values: impl IntoIterator<Item = (Box<str>, CanonicalTarget)>,
) -> BTreeMap<Box<str>, Box<[CanonicalTarget]>> {
    let mut accumulated = BTreeMap::<Box<str>, Vec<CanonicalTarget>>::new();
    for (name, target) in values {
        accumulated.entry(name).or_default().push(target);
    }
    accumulated
        .into_iter()
        .map(|(name, mut targets)| {
            targets.sort();
            targets.dedup();
            (name, targets.into_boxed_slice())
        })
        .collect()
}

fn sources_by_file<'input, 'hir>(
    collection: &DeclarationCollection,
    sources: &'input [SemanticSource<'hir>],
) -> Result<BTreeMap<FileId, &'input SemanticSource<'hir>>, SignatureResolutionError> {
    let mut indexed = BTreeMap::new();
    for source in sources {
        if indexed.insert(source.file(), source).is_some() {
            return Err(SignatureResolutionError::DuplicateSource(source.file()));
        }
    }
    for module in collection.modules() {
        let source = indexed
            .get(&module.file())
            .ok_or(SignatureResolutionError::MissingSource(module.file()))?;
        if source.module() != module.address() {
            return Err(SignatureResolutionError::SourceModuleMismatch(
                module.file(),
            ));
        }
    }
    Ok(indexed)
}

#[derive(Clone)]
enum SourceGenericState {
    Resolving,
    Resolved,
    Poisoned(Poison),
    Unperformed,
}

#[derive(Clone)]
enum AliasState {
    Resolving,
    Resolved {
        generic_parameters: Box<[GenericParameterId]>,
        value: TypeId,
    },
    Poisoned(Poison),
    Unperformed,
}
