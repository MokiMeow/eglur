use std::fmt;

use super::identity::DeclarationBinderContexts;

use crate::{
    BodyFingerprint, BodyTypes, ConformanceComponentFingerprint, ConformanceFingerprint,
    DeclarationCollection, DeclarationSignatures, EffectFacts, EffectFingerprint,
    GenericParameterId, InterfaceFingerprint, ModuleAddress, NameResolution, PhaseProduct,
    ResolvedImports, SemanticSource, StableDeclarationError, StableDeclarationKey,
    StableDeclarationKeys, StableTargetKey, TraitEnvironment, TraitEvidence, TypeStoreError,
};

mod body;
mod conformance;
mod effect;
mod interface;

use body::BodyProjection;

/// One externally reachable source impl contract in the package trait environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceComponentFact {
    implementation: StableDeclarationKey,
    fingerprint: ConformanceComponentFingerprint,
}

impl ConformanceComponentFact {
    #[must_use]
    pub const fn implementation(&self) -> &StableDeclarationKey {
        &self.implementation
    }

    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        self.implementation.module()
    }

    #[must_use]
    pub const fn fingerprint(&self) -> ConformanceComponentFingerprint {
        self.fingerprint
    }
}

/// Inspectable source-conformance input to package-global trait lookup.
///
/// Trusted external conformance remains part of the catalog fingerprint rather
/// than being duplicated here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConformanceEnvironmentFact {
    fingerprint: ConformanceFingerprint,
    components: Box<[ConformanceComponentFact]>,
}

impl ConformanceEnvironmentFact {
    #[must_use]
    pub const fn fingerprint(&self) -> ConformanceFingerprint {
        self.fingerprint
    }

    #[must_use]
    pub fn components(&self) -> &[ConformanceComponentFact] {
        &self.components
    }
}

/// Canonical semantic outputs for one source module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleSemanticFingerprints {
    module: ModuleAddress,
    interface: InterfaceFingerprint,
    body: BodyFingerprint,
    effect: EffectFingerprint,
}

impl ModuleSemanticFingerprints {
    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    #[must_use]
    pub const fn interface(&self) -> InterfaceFingerprint {
        self.interface
    }

    #[must_use]
    pub const fn body(&self) -> BodyFingerprint {
        self.body
    }

    #[must_use]
    pub const fn effect(&self) -> EffectFingerprint {
        self.effect
    }
}

/// Complete semantic output fingerprints, ordered by canonical module address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticFingerprints {
    modules: Box<[ModuleSemanticFingerprints]>,
    conformance: ConformanceEnvironmentFact,
}

impl SemanticFingerprints {
    #[allow(
        clippy::too_many_arguments,
        reason = "the projection consumes independently owned completed semantic products"
    )]
    pub fn compute(
        declarations: &DeclarationCollection,
        imports: &ResolvedImports,
        names: &NameResolution,
        signatures: &DeclarationSignatures,
        sources: &[SemanticSource<'_>],
        body_types: &BodyTypes,
        trait_environment: &TraitEnvironment,
        trait_evidence: &PhaseProduct<TraitEvidence>,
        effects: &PhaseProduct<EffectFacts>,
    ) -> Result<Option<Self>, SemanticFingerprintError> {
        let (PhaseProduct::Performed(trait_evidence), PhaseProduct::Performed(effects)) =
            (trait_evidence, effects)
        else {
            return Ok(None);
        };
        if names.has_poisoned_facts()
            || signatures.has_poisoned_facts()
            || trait_environment.has_poisoned_implementations()
            || body_types.has_poisoned_facts()
            || !trait_evidence.is_complete_for(body_types)
            || effects.has_poisoned_facts()
        {
            return Ok(None);
        }
        let keys = match StableDeclarationKeys::derive(declarations, signatures) {
            Ok(keys) => keys,
            Err(StableDeclarationError::IncompleteSignature(_)) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut binder_contexts = DeclarationBinderContexts::new(declarations, signatures);
        let bodies = match BodyProjection::compute(
            declarations,
            names,
            signatures,
            sources,
            body_types,
            trait_environment,
            trait_evidence,
            &keys,
            &mut binder_contexts,
        ) {
            Ok(bodies) => bodies,
            Err(error) if error.is_incomplete() => return Ok(None),
            Err(error) => return Err(error),
        };
        let interfaces = match interface::compute(
            declarations,
            imports,
            signatures,
            &bodies,
            &keys,
            &mut binder_contexts,
        ) {
            Ok(interfaces) => interfaces,
            Err(error) if error.is_incomplete() => return Ok(None),
            Err(error) => return Err(error),
        };
        let effects = match effect::compute(declarations, imports, signatures, effects, &keys) {
            Ok(effects) => effects,
            Err(error) if error.is_incomplete() => return Ok(None),
            Err(error) => return Err(error),
        };
        let conformance = conformance::compute(
            declarations,
            signatures,
            trait_environment,
            &keys,
            &mut binder_contexts,
        )?;

        let mut modules =
            declarations
                .modules()
                .map(|module| {
                    let address = module.address();
                    Ok(ModuleSemanticFingerprints {
                        module: address.clone(),
                        interface: interfaces.get(address).copied().ok_or_else(|| {
                            SemanticFingerprintError::MissingModule(address.clone())
                        })?,
                        body: bodies.modules().get(address).copied().ok_or_else(|| {
                            SemanticFingerprintError::MissingModule(address.clone())
                        })?,
                        effect: effects.get(address).copied().ok_or_else(|| {
                            SemanticFingerprintError::MissingModule(address.clone())
                        })?,
                    })
                })
                .collect::<Result<Vec<_>, SemanticFingerprintError>>()?;
        modules.sort_by(|left, right| left.module.cmp(&right.module));
        Ok(Some(Self {
            modules: modules.into_boxed_slice(),
            conformance,
        }))
    }

    #[must_use]
    pub fn modules(&self) -> &[ModuleSemanticFingerprints] {
        &self.modules
    }

    #[must_use]
    pub fn module(&self, module: &ModuleAddress) -> Option<&ModuleSemanticFingerprints> {
        self.modules
            .binary_search_by(|candidate| candidate.module.cmp(module))
            .ok()
            .map(|index| &self.modules[index])
    }

    #[must_use]
    pub const fn conformance(&self) -> &ConformanceEnvironmentFact {
        &self.conformance
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticFingerprintError {
    StableDeclaration(StableDeclarationError),
    TypeStore(TypeStoreError),
    MissingModule(ModuleAddress),
    UnknownDeclaration(crate::DefId),
    UnknownStableDeclaration(crate::DefId),
    MissingDeclarationItem(crate::DefId),
    WrongDeclarationItem(crate::DefId),
    MissingSource(eglur_source::FileId),
    MissingBody(crate::DefId),
    MissingConformanceImplementation(crate::DefId),
    CyclicBodyNode,
    IncompleteSignature(crate::DefId),
    IncompleteGenericParameter(GenericParameterId),
    IncompleteImport(crate::ImportId),
    IncompleteBodyFact(&'static str),
    IncompleteEffect(StableTargetKey),
    IncompleteTraitEvidence,
    UnknownCallableInstance,
    UnknownEvidenceEnvironment,
    UnknownEvidenceProof,
    CyclicEvidenceProof,
    UnknownBoundWitness,
    OrphanGenericParameter(crate::DefId),
    UnknownGenericParameter(crate::DefId),
}

impl SemanticFingerprintError {
    fn is_incomplete(&self) -> bool {
        matches!(
            self,
            Self::IncompleteSignature(_)
                | Self::IncompleteGenericParameter(_)
                | Self::IncompleteImport(_)
                | Self::IncompleteBodyFact(_)
                | Self::IncompleteEffect(_)
                | Self::IncompleteTraitEvidence
        )
    }
}

impl fmt::Display for SemanticFingerprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StableDeclaration(error) => error.fmt(formatter),
            Self::TypeStore(error) => error.fmt(formatter),
            Self::MissingModule(module) => {
                write!(
                    formatter,
                    "module {module} has no complete semantic fingerprint"
                )
            }
            Self::UnknownDeclaration(declaration) => {
                write!(
                    formatter,
                    "unknown declaration {declaration:?} in semantic projection"
                )
            }
            Self::UnknownStableDeclaration(declaration) => write!(
                formatter,
                "declaration {declaration:?} has no stable semantic identity"
            ),
            Self::MissingDeclarationItem(declaration) => write!(
                formatter,
                "declaration {declaration:?} has no source item for body projection"
            ),
            Self::WrongDeclarationItem(declaration) => write!(
                formatter,
                "declaration {declaration:?} has an incompatible source item"
            ),
            Self::MissingSource(file) => {
                write!(
                    formatter,
                    "module source {file:?} is absent from semantic projection"
                )
            }
            Self::MissingBody(declaration) => {
                write!(
                    formatter,
                    "declaration {declaration:?} has no expected source body"
                )
            }
            Self::MissingConformanceImplementation(declaration) => write!(
                formatter,
                "source impl {declaration:?} is absent from the checked trait environment"
            ),
            Self::CyclicBodyNode => formatter.write_str("source HIR contains a cyclic body node"),
            Self::IncompleteSignature(declaration) => write!(
                formatter,
                "declaration {declaration:?} has no complete semantic signature"
            ),
            Self::IncompleteGenericParameter(parameter) => write!(
                formatter,
                "generic parameter {parameter:?} has no complete semantic contract"
            ),
            Self::IncompleteImport(import) => {
                write!(
                    formatter,
                    "import {import:?} has no complete re-export fact"
                )
            }
            Self::IncompleteBodyFact(fact) => {
                write!(formatter, "checked body has no complete {fact} fact")
            }
            Self::IncompleteEffect(target) => {
                write!(
                    formatter,
                    "callable {target:?} has no completed effect summary"
                )
            }
            Self::IncompleteTraitEvidence => {
                formatter.write_str("checked invocation has incomplete trait evidence")
            }
            Self::UnknownCallableInstance => {
                formatter.write_str("checked invocation references an unknown callable instance")
            }
            Self::UnknownEvidenceEnvironment => {
                formatter.write_str("checked invocation references an unknown evidence environment")
            }
            Self::UnknownEvidenceProof => {
                formatter.write_str("checked invocation references an unknown evidence proof")
            }
            Self::CyclicEvidenceProof => {
                formatter.write_str("checked invocation references a cyclic evidence proof")
            }
            Self::UnknownBoundWitness => {
                formatter.write_str("checked dispatch references an unknown bound witness")
            }
            Self::OrphanGenericParameter(declaration) => write!(
                formatter,
                "generic parameter {declaration:?} has no semantic owner"
            ),
            Self::UnknownGenericParameter(declaration) => write!(
                formatter,
                "generic parameter declaration {declaration:?} is absent from its owner contract"
            ),
        }
    }
}

impl std::error::Error for SemanticFingerprintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::StableDeclaration(error) => Some(error),
            Self::TypeStore(error) => Some(error),
            Self::MissingModule(_)
            | Self::UnknownDeclaration(_)
            | Self::UnknownStableDeclaration(_)
            | Self::MissingDeclarationItem(_)
            | Self::WrongDeclarationItem(_)
            | Self::MissingSource(_)
            | Self::MissingBody(_)
            | Self::MissingConformanceImplementation(_)
            | Self::CyclicBodyNode
            | Self::IncompleteSignature(_)
            | Self::IncompleteGenericParameter(_)
            | Self::IncompleteImport(_)
            | Self::IncompleteBodyFact(_)
            | Self::IncompleteEffect(_)
            | Self::IncompleteTraitEvidence
            | Self::UnknownCallableInstance
            | Self::UnknownEvidenceEnvironment
            | Self::UnknownEvidenceProof
            | Self::CyclicEvidenceProof
            | Self::UnknownBoundWitness
            | Self::OrphanGenericParameter(_)
            | Self::UnknownGenericParameter(_) => None,
        }
    }
}

impl From<StableDeclarationError> for SemanticFingerprintError {
    fn from(error: StableDeclarationError) -> Self {
        Self::StableDeclaration(error)
    }
}

impl From<TypeStoreError> for SemanticFingerprintError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}
