use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::trusted_method_contracts::validate_trusted_method_contracts;

use crate::{
    ExternalDefId, GenericParameterId, IntrinsicNominal, NameKey, RawPointerMutability,
    ReferenceMutability, TrustedInhabitanceEnvironment, TrustedLanguageItemEnvironment,
    TypeStoreError,
};

/// Portable pre-canonical type expression accepted at the trusted sema boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedTypeExpr {
    Never,
    Primitive(crate::PrimitiveType),
    Intrinsic {
        head: IntrinsicNominal,
        arguments: Box<[TrustedTypeExpr]>,
    },
    Nominal {
        head: ExternalDefId,
        arguments: Box<[TrustedTypeExpr]>,
    },
    GenericParameter(u32),
    SelfType(ExternalDefId),
    Reference {
        mutability: ReferenceMutability,
        referent: Box<TrustedTypeExpr>,
    },
    RawPointer {
        mutability: RawPointerMutability,
        referent: Box<TrustedTypeExpr>,
    },
    Tuple(Box<[TrustedTypeExpr]>),
    Callable {
        parameters: Box<[TrustedTypeExpr]>,
        result: Box<TrustedTypeExpr>,
    },
    ErrorUnion(Box<[TrustedTypeExpr]>),
    AssociatedProjection {
        base: Box<TrustedTypeExpr>,
        trait_declaration: ExternalDefId,
        trait_arguments: Box<[TrustedTypeExpr]>,
        associated: ExternalDefId,
    },
}

/// Portable nominal trait application before canonical interning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedTraitExpr {
    pub declaration: ExternalDefId,
    pub arguments: Box<[TrustedTypeExpr]>,
}

/// Portable generic parameter accepted from a trusted adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedGenericParameter {
    pub name: Box<str>,
    pub bounds: Box<[TrustedTraitExpr]>,
}

/// Portable callable parameter accepted from a trusted adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedCallableParameter {
    pub name: Option<Box<str>>,
    pub ty: TrustedTypeExpr,
}

/// Portable closed applicability requirement accepted from a trusted adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedCallableRequirement {
    FixedWidthInteger(TrustedTypeExpr),
}

/// Portable associated-type declaration accepted from a trusted adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedAssociatedType {
    pub id: ExternalDefId,
    pub name: Box<str>,
}

/// Portable associated-type binding accepted from a trusted adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedAssociatedBinding {
    pub declaration: ExternalDefId,
    pub associated: ExternalDefId,
    pub value: TrustedTypeExpr,
}

/// Explicit membership of one hidden callable in a trusted trait.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedTraitMethod {
    pub declaration: ExternalDefId,
    pub name: NameKey,
    pub has_default: bool,
}

/// Exact hidden callable selected for one method of a trusted implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedImplMethod {
    pub trait_method: ExternalDefId,
    pub callable: ExternalDefId,
}

/// Functional adapter that dispatches one argument through a trusted trait method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedTraitAdapter {
    pub method: ExternalDefId,
    pub argument: u32,
}

/// Portable payload contract for one trusted intrinsic constructor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedConstructorPayload {
    Unit,
    Positional(Box<[TrustedTypeExpr]>),
}

/// Bounded declaration families accepted from a trusted catalog adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedSignatureKind {
    Nominal {
        generic_parameters: Box<[TrustedGenericParameter]>,
    },
    Trait {
        closed: bool,
        generic_parameters: Box<[TrustedGenericParameter]>,
        associated_types: Box<[TrustedAssociatedType]>,
    },
    Callable {
        bang: bool,
        generic_parameters: Box<[TrustedGenericParameter]>,
        parameters: Box<[TrustedCallableParameter]>,
        result: TrustedTypeExpr,
        requirements: Box<[TrustedCallableRequirement]>,
    },
    Constructor {
        generic_parameters: Box<[TrustedGenericParameter]>,
        payload: TrustedConstructorPayload,
        result: TrustedTypeExpr,
    },
    Impl {
        negative: bool,
        generic_parameters: Box<[TrustedGenericParameter]>,
        trait_ref: TrustedTraitExpr,
        target: TrustedTypeExpr,
        associated_bindings: Box<[TrustedAssociatedBinding]>,
    },
}

/// One portable declaration contract before trusted-environment validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedDeclarationSignature {
    pub(super) id: ExternalDefId,
    pub(super) kind: TrustedSignatureKind,
    pub(super) trait_methods: Box<[TrustedTraitMethod]>,
    pub(super) impl_methods: Box<[TrustedImplMethod]>,
    pub(super) trait_adapter: Option<TrustedTraitAdapter>,
}

impl TrustedDeclarationSignature {
    #[must_use]
    pub fn new(id: ExternalDefId, kind: TrustedSignatureKind) -> Self {
        Self {
            id,
            kind,
            trait_methods: Box::new([]),
            impl_methods: Box::new([]),
            trait_adapter: None,
        }
    }

    #[must_use]
    pub fn with_trait_methods(
        mut self,
        methods: impl IntoIterator<Item = TrustedTraitMethod>,
    ) -> Self {
        self.trait_methods = methods.into_iter().collect::<Vec<_>>().into_boxed_slice();
        self
    }

    #[must_use]
    pub fn with_impl_methods(
        mut self,
        methods: impl IntoIterator<Item = TrustedImplMethod>,
    ) -> Self {
        self.impl_methods = methods.into_iter().collect::<Vec<_>>().into_boxed_slice();
        self
    }

    #[must_use]
    pub fn with_trait_adapter(mut self, adapter: TrustedTraitAdapter) -> Self {
        self.trait_adapter = Some(adapter);
        self
    }

    #[must_use]
    pub const fn id(&self) -> &ExternalDefId {
        &self.id
    }

    #[must_use]
    pub const fn kind(&self) -> &TrustedSignatureKind {
        &self.kind
    }

    #[must_use]
    pub fn trait_methods(&self) -> &[TrustedTraitMethod] {
        &self.trait_methods
    }

    #[must_use]
    pub fn impl_methods(&self) -> &[TrustedImplMethod] {
        &self.impl_methods
    }

    #[must_use]
    pub const fn trait_adapter(&self) -> Option<&TrustedTraitAdapter> {
        self.trait_adapter.as_ref()
    }
}

/// Driver-independent, deterministically ordered trusted declaration input.
pub struct TrustedSignatureEnvironment {
    pub(super) declarations: Box<[TrustedDeclarationSignature]>,
    pub(super) by_id: BTreeMap<ExternalDefId, usize>,
    hidden_callables: BTreeSet<ExternalDefId>,
}

impl TrustedSignatureEnvironment {
    pub fn new(
        declarations: impl IntoIterator<Item = TrustedDeclarationSignature>,
    ) -> Result<Self, TrustedSignatureError> {
        let mut declarations = declarations.into_iter().collect::<Vec<_>>();
        for declaration in &mut declarations {
            normalize_trusted_members(declaration);
        }
        declarations.sort_by(|left, right| left.id.cmp(&right.id));
        let mut by_id = BTreeMap::new();
        let mut all_ids = BTreeSet::new();
        for (index, declaration) in declarations.iter().enumerate() {
            if by_id.insert(declaration.id.clone(), index).is_some()
                || !all_ids.insert(declaration.id.clone())
            {
                return Err(TrustedSignatureError::DuplicateDeclaration(
                    declaration.id.clone(),
                ));
            }
        }
        for declaration in &declarations {
            validate_trusted_names(declaration)?;
            if let TrustedSignatureKind::Trait {
                associated_types, ..
            } = declaration.kind()
            {
                for associated in associated_types {
                    if !all_ids.insert(associated.id.clone()) {
                        return Err(TrustedSignatureError::DuplicateDeclaration(
                            associated.id.clone(),
                        ));
                    }
                }
            }
            if let TrustedSignatureKind::Impl {
                associated_bindings,
                ..
            } = declaration.kind()
            {
                for binding in associated_bindings {
                    if !all_ids.insert(binding.declaration.clone()) {
                        return Err(TrustedSignatureError::DuplicateDeclaration(
                            binding.declaration.clone(),
                        ));
                    }
                }
            }
        }
        let (method_owners, hidden_callables) =
            validate_trusted_method_relations(&declarations, &by_id)?;
        validate_trusted_method_contracts(&declarations, &by_id, &method_owners)?;
        Ok(Self {
            declarations: declarations.into_boxed_slice(),
            by_id,
            hidden_callables,
        })
    }

    #[must_use]
    pub fn declaration(&self, id: &ExternalDefId) -> Option<&TrustedDeclarationSignature> {
        self.by_id.get(id).map(|&index| &self.declarations[index])
    }

    pub fn declarations(&self) -> impl ExactSizeIterator<Item = &TrustedDeclarationSignature> {
        self.declarations.iter()
    }

    #[must_use]
    pub fn is_hidden_callable(&self, declaration: &ExternalDefId) -> bool {
        self.hidden_callables.contains(declaration)
    }
}

fn normalize_trusted_members(declaration: &mut TrustedDeclarationSignature) {
    declaration.trait_methods.sort_by(|left, right| {
        left.declaration
            .cmp(&right.declaration)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.has_default.cmp(&right.has_default))
    });
    declaration.impl_methods.sort_by(|left, right| {
        left.trait_method
            .cmp(&right.trait_method)
            .then_with(|| left.callable.cmp(&right.callable))
    });
    match &mut declaration.kind {
        TrustedSignatureKind::Trait {
            associated_types, ..
        } => associated_types.sort_by(|left, right| {
            left.id
                .cmp(&right.id)
                .then_with(|| left.name.cmp(&right.name))
        }),
        TrustedSignatureKind::Impl {
            associated_bindings,
            ..
        } => associated_bindings.sort_by(|left, right| {
            left.declaration
                .cmp(&right.declaration)
                .then_with(|| left.associated.cmp(&right.associated))
        }),
        TrustedSignatureKind::Nominal { .. }
        | TrustedSignatureKind::Callable { .. }
        | TrustedSignatureKind::Constructor { .. } => {}
    }
}

fn validate_trusted_names(
    declaration: &TrustedDeclarationSignature,
) -> Result<(), TrustedSignatureError> {
    if !declaration.trait_methods.is_empty()
        && !matches!(declaration.kind, TrustedSignatureKind::Trait { .. })
    {
        return Err(TrustedSignatureError::UnexpectedTraitMethods(
            declaration.id.clone(),
        ));
    }
    if !declaration.impl_methods.is_empty()
        && !matches!(declaration.kind, TrustedSignatureKind::Impl { .. })
    {
        return Err(TrustedSignatureError::UnexpectedImplMethods(
            declaration.id.clone(),
        ));
    }
    if declaration.trait_adapter.is_some()
        && !matches!(declaration.kind, TrustedSignatureKind::Callable { .. })
    {
        return Err(TrustedSignatureError::UnexpectedTraitAdapter(
            declaration.id.clone(),
        ));
    }
    let parameters = match &declaration.kind {
        TrustedSignatureKind::Nominal { generic_parameters }
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
    };
    let mut names = BTreeSet::new();
    for parameter in parameters {
        if parameter.name.is_empty() || !names.insert(parameter.name.as_ref()) {
            return Err(TrustedSignatureError::InvalidGenericParameter {
                declaration: declaration.id.clone(),
                name: parameter.name.clone(),
            });
        }
    }
    if let TrustedSignatureKind::Trait {
        associated_types, ..
    } = &declaration.kind
    {
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for associated in associated_types {
            if associated.name.is_empty()
                || !ids.insert(associated.id.clone())
                || !names.insert(associated.name.as_ref())
            {
                return Err(TrustedSignatureError::InvalidAssociatedType {
                    declaration: declaration.id.clone(),
                    associated: associated.id.clone(),
                });
            }
        }
    }
    Ok(())
}

fn validate_trusted_method_relations(
    declarations: &[TrustedDeclarationSignature],
    by_id: &BTreeMap<ExternalDefId, usize>,
) -> Result<
    (
        BTreeMap<ExternalDefId, ExternalDefId>,
        BTreeSet<ExternalDefId>,
    ),
    TrustedSignatureError,
> {
    let mut method_owners = BTreeMap::new();
    let mut hidden_callables = BTreeSet::new();
    for declaration in declarations {
        for method in &declaration.trait_methods {
            let Some(&index) = by_id.get(&method.declaration) else {
                return Err(TrustedSignatureError::UnknownMethodCallable {
                    declaration: declaration.id.clone(),
                    callable: method.declaration.clone(),
                });
            };
            let TrustedSignatureKind::Callable { bang, .. } = declarations[index].kind() else {
                return Err(TrustedSignatureError::MethodTargetNotCallable {
                    declaration: declaration.id.clone(),
                    callable: method.declaration.clone(),
                });
            };
            if *bang != method.name.is_bang() {
                return Err(TrustedSignatureError::MethodBangMismatch {
                    declaration: declaration.id.clone(),
                    method: method.declaration.clone(),
                });
            }
            if method_owners
                .insert(method.declaration.clone(), declaration.id.clone())
                .is_some()
            {
                return Err(TrustedSignatureError::DuplicateTraitMethod(
                    method.declaration.clone(),
                ));
            }
            hidden_callables.insert(method.declaration.clone());
        }
    }
    for declaration in declarations {
        let implemented_trait = match declaration.kind() {
            TrustedSignatureKind::Impl { trait_ref, .. } => Some(&trait_ref.declaration),
            _ => None,
        };
        let mut seen_methods = BTreeSet::new();
        for method in &declaration.impl_methods {
            let Some(owner) = method_owners.get(&method.trait_method) else {
                return Err(TrustedSignatureError::UnknownTraitMethod {
                    declaration: declaration.id.clone(),
                    method: method.trait_method.clone(),
                });
            };
            if implemented_trait != Some(owner) || !seen_methods.insert(method.trait_method.clone())
            {
                return Err(TrustedSignatureError::InvalidImplMethod {
                    declaration: declaration.id.clone(),
                    method: method.trait_method.clone(),
                });
            }
            let Some(&index) = by_id.get(&method.callable) else {
                return Err(TrustedSignatureError::UnknownMethodCallable {
                    declaration: declaration.id.clone(),
                    callable: method.callable.clone(),
                });
            };
            if !matches!(
                declarations[index].kind(),
                TrustedSignatureKind::Callable { .. }
            ) {
                return Err(TrustedSignatureError::MethodTargetNotCallable {
                    declaration: declaration.id.clone(),
                    callable: method.callable.clone(),
                });
            }
            hidden_callables.insert(method.callable.clone());
        }
        if let Some(adapter) = &declaration.trait_adapter {
            if !method_owners.contains_key(&adapter.method) {
                return Err(TrustedSignatureError::UnknownTraitMethod {
                    declaration: declaration.id.clone(),
                    method: adapter.method.clone(),
                });
            }
            let TrustedSignatureKind::Callable { parameters, .. } = declaration.kind() else {
                unreachable!("trait adapter kind was validated")
            };
            if usize::try_from(adapter.argument).map_or(true, |index| index >= parameters.len()) {
                return Err(TrustedSignatureError::AdapterArgumentOutOfRange {
                    declaration: declaration.id.clone(),
                    argument: adapter.argument,
                });
            }
        }
    }
    Ok((method_owners, hidden_callables))
}

/// Malformed or semantically inconsistent trusted declaration input.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedSignatureError {
    DuplicateDeclaration(ExternalDefId),
    InvalidGenericParameter {
        declaration: ExternalDefId,
        name: Box<str>,
    },
    InvalidAssociatedType {
        declaration: ExternalDefId,
        associated: ExternalDefId,
    },
    UnexpectedTraitMethods(ExternalDefId),
    UnexpectedImplMethods(ExternalDefId),
    UnexpectedTraitAdapter(ExternalDefId),
    DuplicateTraitMethod(ExternalDefId),
    UnknownTraitMethod {
        declaration: ExternalDefId,
        method: ExternalDefId,
    },
    UnknownMethodCallable {
        declaration: ExternalDefId,
        callable: ExternalDefId,
    },
    MethodTargetNotCallable {
        declaration: ExternalDefId,
        callable: ExternalDefId,
    },
    MethodBangMismatch {
        declaration: ExternalDefId,
        method: ExternalDefId,
    },
    InvalidImplMethod {
        declaration: ExternalDefId,
        method: ExternalDefId,
    },
    AdapterArgumentOutOfRange {
        declaration: ExternalDefId,
        argument: u32,
    },
    MissingRequiredMethod {
        implementation: ExternalDefId,
        method: ExternalDefId,
    },
    MethodContractMismatch {
        implementation: ExternalDefId,
        method: ExternalDefId,
    },
    AdapterContractMismatch(ExternalDefId),
    MissingNameDeclaration(ExternalDefId),
    DeclarationKindMismatch(ExternalDefId),
    UnknownTypeHead(ExternalDefId),
    UnknownTrait(ExternalDefId),
    UnknownAssociatedType(ExternalDefId),
    NegativeImplUnsupported(ExternalDefId),
    CoherenceViolation(ExternalDefId),
    UnconstrainedImplParameter {
        declaration: ExternalDefId,
        parameter: GenericParameterId,
    },
    GenericParameterOutOfRange {
        declaration: ExternalDefId,
        index: u32,
    },
    Arity {
        declaration: ExternalDefId,
        expected: usize,
        actual: usize,
    },
    DuplicateBound {
        declaration: ExternalDefId,
        parameter: u32,
    },
    InvalidTypePosition {
        declaration: ExternalDefId,
        reason: &'static str,
    },
    TypeStore(TypeStoreError),
}

impl fmt::Display for TrustedSignatureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateDeclaration(id) => {
                write!(formatter, "duplicate trusted signature {}", id.as_str())
            }
            Self::InvalidGenericParameter { declaration, name } => write!(
                formatter,
                "trusted signature {} has invalid or duplicate generic parameter `{name}`",
                declaration.as_str()
            ),
            Self::InvalidAssociatedType {
                declaration,
                associated,
            } => write!(
                formatter,
                "trusted trait {} has invalid or duplicate associated type {}",
                declaration.as_str(),
                associated.as_str()
            ),
            Self::UnexpectedTraitMethods(declaration) => write!(
                formatter,
                "non-trait trusted declaration {} carries trait methods",
                declaration.as_str()
            ),
            Self::UnexpectedImplMethods(declaration) => write!(
                formatter,
                "non-impl trusted declaration {} carries impl methods",
                declaration.as_str()
            ),
            Self::UnexpectedTraitAdapter(declaration) => write!(
                formatter,
                "non-callable trusted declaration {} carries a trait adapter",
                declaration.as_str()
            ),
            Self::DuplicateTraitMethod(method) => {
                write!(
                    formatter,
                    "trusted trait method {} has multiple owners",
                    method.as_str()
                )
            }
            Self::UnknownTraitMethod {
                declaration,
                method,
            } => write!(
                formatter,
                "trusted declaration {} references unknown trait method {}",
                declaration.as_str(),
                method.as_str()
            ),
            Self::UnknownMethodCallable {
                declaration,
                callable,
            } => write!(
                formatter,
                "trusted declaration {} references unknown method callable {}",
                declaration.as_str(),
                callable.as_str()
            ),
            Self::MethodTargetNotCallable {
                declaration,
                callable,
            } => write!(
                formatter,
                "trusted declaration {} method target {} is not callable",
                declaration.as_str(),
                callable.as_str()
            ),
            Self::MethodBangMismatch {
                declaration,
                method,
            } => write!(
                formatter,
                "trusted trait {} method {} disagrees with its callable effect marker",
                declaration.as_str(),
                method.as_str()
            ),
            Self::InvalidImplMethod {
                declaration,
                method,
            } => write!(
                formatter,
                "trusted impl {} has duplicate or foreign method {}",
                declaration.as_str(),
                method.as_str()
            ),
            Self::AdapterArgumentOutOfRange {
                declaration,
                argument,
            } => write!(
                formatter,
                "trusted trait adapter {} references missing argument {argument}",
                declaration.as_str()
            ),
            Self::MissingRequiredMethod {
                implementation,
                method,
            } => write!(
                formatter,
                "trusted impl {} is missing required method {}",
                implementation.as_str(),
                method.as_str()
            ),
            Self::MethodContractMismatch {
                implementation,
                method,
            } => write!(
                formatter,
                "trusted impl {} method {} does not match its trait contract",
                implementation.as_str(),
                method.as_str()
            ),
            Self::AdapterContractMismatch(declaration) => write!(
                formatter,
                "trusted trait adapter {} does not match its method contract",
                declaration.as_str()
            ),
            Self::MissingNameDeclaration(id) => write!(
                formatter,
                "trusted signature {} has no external name declaration",
                id.as_str()
            ),
            Self::DeclarationKindMismatch(id) => write!(
                formatter,
                "trusted signature {} disagrees with its external declaration kind",
                id.as_str()
            ),
            Self::UnknownTypeHead(id) => {
                write!(
                    formatter,
                    "unknown trusted nominal type head {}",
                    id.as_str()
                )
            }
            Self::UnknownTrait(id) => {
                write!(formatter, "unknown trusted trait {}", id.as_str())
            }
            Self::UnknownAssociatedType(id) => {
                write!(formatter, "unknown trusted associated type {}", id.as_str())
            }
            Self::NegativeImplUnsupported(id) => write!(
                formatter,
                "trusted declaration {} uses a negative impl unsupported in v0.1",
                id.as_str()
            ),
            Self::CoherenceViolation(id) => write!(
                formatter,
                "trusted declaration {} violates trait coherence",
                id.as_str()
            ),
            Self::UnconstrainedImplParameter {
                declaration,
                parameter,
            } => write!(
                formatter,
                "trusted impl {} has unconstrained parameter {parameter:?}",
                declaration.as_str()
            ),
            Self::GenericParameterOutOfRange { declaration, index } => write!(
                formatter,
                "trusted signature {} references generic parameter {index} outside its declaration",
                declaration.as_str()
            ),
            Self::Arity {
                declaration,
                expected,
                actual,
            } => write!(
                formatter,
                "trusted declaration {} expects {expected} type arguments, found {actual}",
                declaration.as_str()
            ),
            Self::DuplicateBound {
                declaration,
                parameter,
            } => write!(
                formatter,
                "trusted declaration {} repeats a bound for generic parameter {parameter}",
                declaration.as_str()
            ),
            Self::InvalidTypePosition {
                declaration,
                reason,
            } => write!(
                formatter,
                "trusted declaration {} has invalid type position: {reason}",
                declaration.as_str()
            ),
            Self::TypeStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for TrustedSignatureError {}

impl From<TypeStoreError> for TrustedSignatureError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

/// Trusted declaration inputs that are normalized against one canonical type graph.
#[derive(Clone, Copy)]
pub struct TrustedDeclarationInputs<'a> {
    pub(super) signatures: &'a TrustedSignatureEnvironment,
    pub(super) inhabitance: Option<&'a TrustedInhabitanceEnvironment>,
    pub(super) language_items: Option<&'a TrustedLanguageItemEnvironment>,
}

impl<'a> TrustedDeclarationInputs<'a> {
    #[must_use]
    pub const fn new(
        signatures: &'a TrustedSignatureEnvironment,
        inhabitance: Option<&'a TrustedInhabitanceEnvironment>,
    ) -> Self {
        Self {
            signatures,
            inhabitance,
            language_items: None,
        }
    }

    #[must_use]
    pub const fn with_language_items(
        mut self,
        language_items: &'a TrustedLanguageItemEnvironment,
    ) -> Self {
        self.language_items = Some(language_items);
        self
    }
}
