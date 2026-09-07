use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_sema::{
    CanonicalFingerprintBuilder, ConformanceEnvironmentFact, ConformanceFingerprint,
    EffectFingerprint, FingerprintDigest, FingerprintDomain, InterfaceFingerprint, ModuleAddress,
    SemanticFingerprints, SemanticSource,
};
use eglur_source::FileId;

use crate::catalog::TrustedCatalog;
use crate::schedule::{ImportDependency, ImportSchedule};
use crate::session::LoweredSource;

mod catalog;

macro_rules! fingerprint_type {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(FingerprintDigest);

        impl $name {
            #[must_use]
            pub const fn digest(self) -> FingerprintDigest {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

fingerprint_type!(SourceFingerprint);
fingerprint_type!(CatalogFingerprint);
fingerprint_type!(AnalysisInputKey);

/// Exact source input associated with its portable logical module identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFingerprintFact {
    module: ModuleAddress,
    fingerprint: SourceFingerprint,
}

impl SourceFingerprintFact {
    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    #[must_use]
    pub const fn fingerprint(&self) -> SourceFingerprint {
        self.fingerprint
    }
}

/// One direct import represented by its reusable semantic outputs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDependencyFingerprint {
    module: ModuleAddress,
    interface: InterfaceFingerprint,
    effect: EffectFingerprint,
}

impl ImportDependencyFingerprint {
    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    #[must_use]
    pub const fn interface(&self) -> InterfaceFingerprint {
        self.interface
    }

    #[must_use]
    pub const fn effect(&self) -> EffectFingerprint {
        self.effect
    }
}

/// Closed semantic dependencies that may invalidate one import SCC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisDependencyFingerprint {
    Import(ImportDependencyFingerprint),
    /// Package-global source impl contracts; trusted impls are catalog inputs.
    GlobalConformance(ConformanceFingerprint),
}

/// Canonical input key and its inspectable inputs for one import SCC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisUnitFingerprint {
    members: Box<[ModuleAddress]>,
    dependencies: Box<[AnalysisDependencyFingerprint]>,
    key: AnalysisInputKey,
}

impl AnalysisUnitFingerprint {
    #[must_use]
    pub fn members(&self) -> &[ModuleAddress] {
        &self.members
    }

    #[must_use]
    pub fn dependencies(&self) -> &[AnalysisDependencyFingerprint] {
        &self.dependencies
    }

    #[must_use]
    pub const fn key(&self) -> AnalysisInputKey {
        self.key
    }
}

/// Complete current-session fingerprint facts, with no persistent cache state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisFingerprints {
    catalog: CatalogFingerprint,
    conformance: ConformanceEnvironmentFact,
    sources: Box<[SourceFingerprintFact]>,
    units: Box<[AnalysisUnitFingerprint]>,
}

impl AnalysisFingerprints {
    #[must_use]
    pub const fn catalog(&self) -> CatalogFingerprint {
        self.catalog
    }

    #[must_use]
    pub const fn conformance(&self) -> &ConformanceEnvironmentFact {
        &self.conformance
    }

    #[must_use]
    pub fn sources(&self) -> &[SourceFingerprintFact] {
        &self.sources
    }

    #[must_use]
    pub fn units(&self) -> &[AnalysisUnitFingerprint] {
        &self.units
    }

    #[must_use]
    pub fn source(&self, module: &ModuleAddress) -> Option<&SourceFingerprintFact> {
        self.sources
            .binary_search_by(|candidate| candidate.module.cmp(module))
            .ok()
            .map(|index| &self.sources[index])
    }

    #[must_use]
    pub fn unit_containing(&self, module: &ModuleAddress) -> Option<&AnalysisUnitFingerprint> {
        self.units
            .iter()
            .find(|unit| unit.members.binary_search(module).is_ok())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisFingerprintError {
    SourceProjectionLength { lowered: usize, semantic: usize },
    SourceProjectionMismatch { lowered: FileId, semantic: FileId },
    DuplicateSource(FileId),
    DuplicateModule(ModuleAddress),
    UnknownSource(FileId),
    MissingSemanticModule(ModuleAddress),
}

impl fmt::Display for AnalysisFingerprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceProjectionLength { lowered, semantic } => write!(
                formatter,
                "fingerprint source projection length differs: lowered={lowered}, semantic={semantic}"
            ),
            Self::SourceProjectionMismatch { lowered, semantic } => write!(
                formatter,
                "fingerprint source projection mismatches {lowered:?} and {semantic:?}"
            ),
            Self::DuplicateSource(file) => {
                write!(formatter, "fingerprint inputs repeat source {file:?}")
            }
            Self::DuplicateModule(module) => {
                write!(formatter, "fingerprint inputs repeat module {module}")
            }
            Self::UnknownSource(file) => {
                write!(
                    formatter,
                    "fingerprint schedule references unknown source {file:?}"
                )
            }
            Self::MissingSemanticModule(module) => {
                write!(
                    formatter,
                    "fingerprint inputs lack semantic outputs for {module}"
                )
            }
        }
    }
}

impl std::error::Error for AnalysisFingerprintError {}

pub(crate) fn compute_analysis_fingerprints(
    sources: &[LoweredSource],
    semantic_sources: &[SemanticSource<'_>],
    schedule: &ImportSchedule,
    dependencies: &[ImportDependency],
    catalog: Option<&TrustedCatalog>,
    semantics: &SemanticFingerprints,
) -> Result<AnalysisFingerprints, AnalysisFingerprintError> {
    if sources.len() != semantic_sources.len() {
        return Err(AnalysisFingerprintError::SourceProjectionLength {
            lowered: sources.len(),
            semantic: semantic_sources.len(),
        });
    }

    let mut facts_by_file = BTreeMap::new();
    let mut modules = BTreeSet::new();
    for (source, semantic) in sources.iter().zip(semantic_sources) {
        if source.file() != semantic.file() {
            return Err(AnalysisFingerprintError::SourceProjectionMismatch {
                lowered: source.file(),
                semantic: semantic.file(),
            });
        }
        if !modules.insert(semantic.module().clone()) {
            return Err(AnalysisFingerprintError::DuplicateModule(
                semantic.module().clone(),
            ));
        }
        let fact = SourceFingerprintFact {
            module: semantic.module().clone(),
            fingerprint: source_fingerprint(semantic.module(), source.source_bytes()),
        };
        if facts_by_file.insert(source.file(), fact).is_some() {
            return Err(AnalysisFingerprintError::DuplicateSource(source.file()));
        }
    }

    let catalog = catalog::fingerprint(catalog);
    let mut units = Vec::with_capacity(schedule.units().len());
    for unit in schedule.units() {
        let member_files = unit.members().iter().copied().collect::<BTreeSet<_>>();
        let mut members = unit
            .members()
            .iter()
            .map(|file| {
                facts_by_file
                    .get(file)
                    .ok_or(AnalysisFingerprintError::UnknownSource(*file))
            })
            .collect::<Result<Vec<_>, _>>()?;
        members.sort_by(|left, right| left.module.cmp(&right.module));

        let dependency_files = dependencies
            .iter()
            .filter(|dependency| {
                member_files.contains(&dependency.importer())
                    && !member_files.contains(&dependency.imported())
            })
            .map(|dependency| dependency.imported())
            .collect::<BTreeSet<_>>();
        let mut import_dependencies = dependency_files
            .into_iter()
            .map(|file| {
                let source = facts_by_file
                    .get(&file)
                    .ok_or(AnalysisFingerprintError::UnknownSource(file))?;
                let outputs = semantics.module(source.module()).ok_or_else(|| {
                    AnalysisFingerprintError::MissingSemanticModule(source.module().clone())
                })?;
                Ok(ImportDependencyFingerprint {
                    module: source.module().clone(),
                    interface: outputs.interface(),
                    effect: outputs.effect(),
                })
            })
            .collect::<Result<Vec<_>, AnalysisFingerprintError>>()?;
        import_dependencies.sort_by(|left, right| left.module.cmp(&right.module));
        let mut analysis_dependencies = import_dependencies
            .into_iter()
            .map(AnalysisDependencyFingerprint::Import)
            .collect::<Vec<_>>();
        analysis_dependencies.push(AnalysisDependencyFingerprint::GlobalConformance(
            semantics.conformance().fingerprint(),
        ));

        let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::AnalysisInput);
        builder.write_len(members.len());
        for member in &members {
            builder.write_str(member.module().as_str());
            builder.write_digest(member.fingerprint().digest());
        }
        builder.write_len(analysis_dependencies.len());
        for dependency in &analysis_dependencies {
            match dependency {
                AnalysisDependencyFingerprint::Import(dependency) => {
                    builder.write_u8(0);
                    builder.write_str(dependency.module().as_str());
                    builder.write_digest(dependency.interface().digest());
                    builder.write_digest(dependency.effect().digest());
                }
                AnalysisDependencyFingerprint::GlobalConformance(fingerprint) => {
                    builder.write_u8(1);
                    builder.write_digest(fingerprint.digest());
                }
            }
        }
        builder.write_digest(catalog.digest());

        units.push(AnalysisUnitFingerprint {
            members: members
                .into_iter()
                .map(|member| member.module().clone())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            dependencies: analysis_dependencies.into_boxed_slice(),
            key: AnalysisInputKey(builder.finish()),
        });
    }

    let mut sources = facts_by_file.into_values().collect::<Vec<_>>();
    sources.sort_by(|left, right| left.module.cmp(&right.module));
    Ok(AnalysisFingerprints {
        catalog,
        conformance: semantics.conformance().clone(),
        sources: sources.into_boxed_slice(),
        units: units.into_boxed_slice(),
    })
}

fn source_fingerprint(module: &ModuleAddress, bytes: &[u8]) -> SourceFingerprint {
    let mut builder = CanonicalFingerprintBuilder::new(FingerprintDomain::Source);
    builder.write_str(module.as_str());
    builder.write_bytes(bytes);
    SourceFingerprint(builder.finish())
}
