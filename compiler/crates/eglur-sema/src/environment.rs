use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{DeclarationKind, ModuleAddress, NameKey, Namespace};

/// Whether external lookup represents the complete standard-library name graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalNameCoverage {
    Complete,
    PartialStandardLibrary,
}

/// Dedicated tensor constructor semantics owned outside general callable typing.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TensorSourceConstructor {
    Zeros,
    Ones,
    Nan,
    Rand,
}

impl TensorSourceConstructor {
    #[must_use]
    pub const fn is_bang(self) -> bool {
        matches!(self, Self::Rand)
    }

    #[must_use]
    pub const fn accepts_dtype(self, dtype: crate::PrimitiveType) -> bool {
        use crate::PrimitiveType::*;
        match self {
            Self::Nan | Self::Rand => matches!(dtype, F32),
            Self::Zeros | Self::Ones => matches!(
                dtype,
                Bool | I8
                    | I16
                    | I32
                    | I64
                    | U8
                    | U16
                    | U32
                    | U64
                    | F16
                    | Bf16
                    | F32
                    | F64
                    | Complex64
                    | Complex128
            ),
        }
    }
}

/// Semantic layer responsible for one trusted external declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExternalSemanticOwner {
    General,
    TensorSource(TensorSourceConstructor),
}

/// Portable trusted declaration identity supplied by an external catalog.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExternalDefId(Box<str>);

impl ExternalDefId {
    #[must_use]
    pub fn new(key: impl Into<Box<str>>) -> Option<Self> {
        let key = key.into();
        (!key.is_empty()).then_some(Self(key))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One trusted external declaration used by name resolution and semantic routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalDeclaration {
    id: ExternalDefId,
    module: ModuleAddress,
    name: NameKey,
    kind: DeclarationKind,
    semantic_owner: ExternalSemanticOwner,
}

impl ExternalDeclaration {
    pub fn new(
        id: ExternalDefId,
        module: ModuleAddress,
        name: NameKey,
        kind: DeclarationKind,
    ) -> Result<Self, ExternalEnvironmentError> {
        if kind.namespace().is_none() {
            return Err(ExternalEnvironmentError::UnnameableDeclarationKind(kind));
        }
        Ok(Self {
            id,
            module,
            name,
            kind,
            semantic_owner: ExternalSemanticOwner::General,
        })
    }

    pub fn tensor_source(
        id: ExternalDefId,
        module: ModuleAddress,
        name: NameKey,
        constructor: TensorSourceConstructor,
    ) -> Result<Self, ExternalEnvironmentError> {
        let mut declaration = Self::new(id, module, name, DeclarationKind::Function)?;
        declaration.semantic_owner = ExternalSemanticOwner::TensorSource(constructor);
        Ok(declaration)
    }

    #[must_use]
    pub const fn id(&self) -> &ExternalDefId {
        &self.id
    }

    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    #[must_use]
    pub const fn name(&self) -> &NameKey {
        &self.name
    }

    #[must_use]
    pub const fn kind(&self) -> DeclarationKind {
        self.kind
    }

    #[must_use]
    pub const fn semantic_owner(&self) -> ExternalSemanticOwner {
        self.semantic_owner
    }
}

/// Complete trusted external module and prelude name environment.
pub struct ExternalNameEnvironment {
    declarations: Box<[ExternalDeclaration]>,
    by_id: BTreeMap<ExternalDefId, usize>,
    by_module: BTreeMap<(ModuleAddress, Namespace, NameKey), Box<[ExternalDefId]>>,
    modules: BTreeSet<ModuleAddress>,
    prelude: BTreeMap<(Namespace, NameKey), Box<[ExternalDefId]>>,
    coverage: ExternalNameCoverage,
}

impl ExternalNameEnvironment {
    pub fn new(
        module_addresses: impl IntoIterator<Item = ModuleAddress>,
        declarations: impl IntoIterator<Item = ExternalDeclaration>,
        prelude: impl IntoIterator<Item = ExternalDefId>,
    ) -> Result<Self, ExternalEnvironmentError> {
        Self::with_coverage(
            module_addresses,
            declarations,
            prelude,
            ExternalNameCoverage::Complete,
        )
    }

    /// Builds an environment that intentionally represents only part of `std`.
    pub fn partial_standard_library(
        module_addresses: impl IntoIterator<Item = ModuleAddress>,
        declarations: impl IntoIterator<Item = ExternalDeclaration>,
        prelude: impl IntoIterator<Item = ExternalDefId>,
    ) -> Result<Self, ExternalEnvironmentError> {
        Self::with_coverage(
            module_addresses,
            declarations,
            prelude,
            ExternalNameCoverage::PartialStandardLibrary,
        )
    }

    fn with_coverage(
        module_addresses: impl IntoIterator<Item = ModuleAddress>,
        declarations: impl IntoIterator<Item = ExternalDeclaration>,
        prelude: impl IntoIterator<Item = ExternalDefId>,
        coverage: ExternalNameCoverage,
    ) -> Result<Self, ExternalEnvironmentError> {
        let mut modules = BTreeSet::new();
        for module in module_addresses {
            if !modules.insert(module.clone()) {
                return Err(ExternalEnvironmentError::DuplicateModule(module));
            }
        }
        let mut declarations = declarations.into_iter().collect::<Vec<_>>();
        declarations.sort_by(|left, right| left.id.cmp(&right.id));
        let mut by_id = BTreeMap::new();
        let mut module_accumulator =
            BTreeMap::<(ModuleAddress, Namespace, NameKey), Vec<ExternalDefId>>::new();
        for (index, declaration) in declarations.iter().enumerate() {
            if by_id.insert(declaration.id.clone(), index).is_some() {
                return Err(ExternalEnvironmentError::DuplicateIdentity(
                    declaration.id.clone(),
                ));
            }
            let namespace = declaration
                .kind
                .namespace()
                .expect("constructor rejects unnameable external kinds");
            if !modules.contains(&declaration.module) {
                return Err(ExternalEnvironmentError::UnknownDeclarationModule {
                    declaration: declaration.id.clone(),
                    module: declaration.module.clone(),
                });
            }
            module_accumulator
                .entry((
                    declaration.module.clone(),
                    namespace,
                    declaration.name.clone(),
                ))
                .or_default()
                .push(declaration.id.clone());
        }

        let mut prelude_accumulator = BTreeMap::<(Namespace, NameKey), Vec<ExternalDefId>>::new();
        for id in prelude {
            let Some(&index) = by_id.get(&id) else {
                return Err(ExternalEnvironmentError::UnknownPreludeIdentity(id));
            };
            let declaration = &declarations[index];
            let namespace = declaration
                .kind
                .namespace()
                .expect("constructor rejects unnameable external kinds");
            prelude_accumulator
                .entry((namespace, declaration.name.clone()))
                .or_default()
                .push(id);
        }

        Ok(Self {
            declarations: declarations.into_boxed_slice(),
            by_id,
            by_module: freeze_multimap(module_accumulator),
            modules,
            prelude: freeze_multimap(prelude_accumulator),
            coverage,
        })
    }

    #[must_use]
    pub fn declaration(&self, id: &ExternalDefId) -> Option<&ExternalDeclaration> {
        self.by_id.get(id).map(|&index| &self.declarations[index])
    }

    pub fn declarations(&self) -> impl ExactSizeIterator<Item = &ExternalDeclaration> {
        self.declarations.iter()
    }

    /// Iterates every represented external module, including empty modules.
    pub fn modules(&self) -> impl ExactSizeIterator<Item = &ModuleAddress> {
        self.modules.iter()
    }

    /// Iterates normalized prelude bindings in namespace and name order.
    pub fn prelude_bindings(
        &self,
    ) -> impl ExactSizeIterator<Item = (Namespace, &NameKey, &[ExternalDefId])> {
        self.prelude
            .iter()
            .map(|((namespace, name), declarations)| (*namespace, name, declarations.as_ref()))
    }

    #[must_use]
    pub const fn coverage(&self) -> ExternalNameCoverage {
        self.coverage
    }

    #[must_use]
    pub const fn standard_library_is_complete(&self) -> bool {
        matches!(self.coverage, ExternalNameCoverage::Complete)
    }

    #[must_use]
    pub fn has_module(&self, module: &ModuleAddress) -> bool {
        self.modules.contains(module)
    }

    #[must_use]
    pub fn module_candidates(
        &self,
        module: &ModuleAddress,
        namespace: Namespace,
        name: &NameKey,
    ) -> &[ExternalDefId] {
        self.by_module
            .get(&(module.clone(), namespace, name.clone()))
            .map_or(&[], AsRef::as_ref)
    }

    #[must_use]
    pub fn has_module_name(&self, module: &ModuleAddress, name: &NameKey) -> bool {
        [Namespace::Value, Namespace::Type]
            .into_iter()
            .any(|namespace| !self.module_candidates(module, namespace, name).is_empty())
    }

    #[must_use]
    pub fn prelude_candidates(&self, namespace: Namespace, name: &NameKey) -> &[ExternalDefId] {
        self.prelude
            .get(&(namespace, name.clone()))
            .map_or(&[], AsRef::as_ref)
    }
}

fn freeze_multimap<K: Ord>(
    source: BTreeMap<K, Vec<ExternalDefId>>,
) -> BTreeMap<K, Box<[ExternalDefId]>> {
    source
        .into_iter()
        .map(|(key, mut values)| {
            values.sort();
            values.dedup();
            (key, values.into_boxed_slice())
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalEnvironmentError {
    UnnameableDeclarationKind(DeclarationKind),
    DuplicateModule(ModuleAddress),
    DuplicateIdentity(ExternalDefId),
    UnknownDeclarationModule {
        declaration: ExternalDefId,
        module: ModuleAddress,
    },
    UnknownPreludeIdentity(ExternalDefId),
}

impl fmt::Display for ExternalEnvironmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnnameableDeclarationKind(kind) => {
                write!(
                    formatter,
                    "external declaration kind {kind:?} has no lookup namespace"
                )
            }
            Self::DuplicateModule(module) => {
                write!(formatter, "duplicate external module {module}")
            }
            Self::DuplicateIdentity(id) => {
                write!(
                    formatter,
                    "duplicate external declaration identity {}",
                    id.as_str()
                )
            }
            Self::UnknownDeclarationModule {
                declaration,
                module,
            } => write!(
                formatter,
                "external declaration {} belongs to unlisted module {module}",
                declaration.as_str()
            ),
            Self::UnknownPreludeIdentity(id) => write!(
                formatter,
                "prelude references unknown external declaration {}",
                id.as_str()
            ),
        }
    }
}

impl std::error::Error for ExternalEnvironmentError {}
