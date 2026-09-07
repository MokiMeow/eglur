use std::collections::BTreeSet;
use std::fmt;

use eglur_eval::{ExecutionCapability, ExecutionSupport};
use eglur_sema::{CompileCapability, CompileSupport, ResolveCapability, TypecheckCapability};

/// Version of the compiler support-profile boundary.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SupportSchemaVersion {
    V1,
}

impl SupportSchemaVersion {
    #[must_use]
    pub const fn stable_key(self) -> &'static str {
        match self {
            Self::V1 => "eglur.support.v1",
        }
    }
}

/// Stable implementation-owned name of one closed support profile.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SupportProfileId(Box<str>);

impl SupportProfileId {
    #[must_use]
    pub fn new(value: impl Into<Box<str>>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()
            && value.trim() == value.as_ref()
            && !value.chars().any(char::is_control))
        .then_some(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Named, prerequisite-closed set of operations implemented by this compiler slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupportProfile {
    schema: SupportSchemaVersion,
    id: SupportProfileId,
    compile: BTreeSet<CompileCapability>,
    execute: BTreeSet<ExecutionCapability>,
}

impl SupportProfile {
    /// Returns the exact compile and execution profile for the first semantic slice.
    #[must_use]
    pub fn semantic_core_v1() -> Self {
        const COMPILE: [CompileCapability; 20] = [
            CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
            CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
            CompileCapability::Resolve(ResolveCapability::ResolveLexicalValues),
            CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
            CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues),
            CompileCapability::Resolve(ResolveCapability::ResolvePatternScopes),
            CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
            CompileCapability::Typecheck(TypecheckCapability::ResolveDeclarationSignatures),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEnvironment),
            CompileCapability::Typecheck(TypecheckCapability::CheckBodies),
            CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
            CompileCapability::Typecheck(TypecheckCapability::ResolveMethods),
            CompileCapability::Typecheck(TypecheckCapability::ResolveOverloads),
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            CompileCapability::Typecheck(TypecheckCapability::CheckFailureFlow),
            CompileCapability::Typecheck(
                TypecheckCapability::CheckSelectedCheckedIntegerArithmetic,
            ),
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            CompileCapability::Typecheck(TypecheckCapability::CheckOwnershipFlow),
            CompileCapability::Typecheck(TypecheckCapability::ResolveTraitEvidence),
            CompileCapability::Typecheck(TypecheckCapability::ComputeEffectSummaries),
        ];

        Self::new(
            SupportSchemaVersion::V1,
            SupportProfileId::new("semantic-core-v1")
                .expect("the built-in support profile ID is valid"),
            COMPILE,
            ExecutionCapability::ALL,
        )
        .expect("the built-in semantic-core profile is prerequisite-closed")
    }

    pub fn new(
        schema: SupportSchemaVersion,
        id: SupportProfileId,
        compile: impl IntoIterator<Item = CompileCapability>,
        execute: impl IntoIterator<Item = ExecutionCapability>,
    ) -> Result<Self, SupportProfileError> {
        let compile = compile.into_iter().collect::<BTreeSet<_>>();
        for &capability in &compile {
            if !capability.is_implemented() {
                return Err(SupportProfileError::UnimplementedCapability(capability));
            }
            for &prerequisite in capability.prerequisites() {
                if !compile.contains(&prerequisite) {
                    return Err(SupportProfileError::MissingPrerequisite {
                        capability,
                        prerequisite,
                    });
                }
            }
        }
        let execute = execute.into_iter().collect::<BTreeSet<_>>();
        for &capability in &execute {
            if !capability.is_implemented() {
                return Err(SupportProfileError::UnimplementedExecutionCapability(
                    capability,
                ));
            }
            for &prerequisite in capability.prerequisites() {
                if !execute.contains(&prerequisite) {
                    return Err(SupportProfileError::MissingExecutionPrerequisite {
                        capability,
                        prerequisite,
                    });
                }
            }
        }
        Ok(Self {
            schema,
            id,
            compile,
            execute,
        })
    }

    pub fn compile_only(
        schema: SupportSchemaVersion,
        id: SupportProfileId,
        compile: impl IntoIterator<Item = CompileCapability>,
    ) -> Result<Self, SupportProfileError> {
        Self::new(schema, id, compile, [])
    }

    #[must_use]
    pub const fn schema(&self) -> SupportSchemaVersion {
        self.schema
    }

    #[must_use]
    pub const fn id(&self) -> &SupportProfileId {
        &self.id
    }

    pub fn compile_capabilities(&self) -> impl ExactSizeIterator<Item = CompileCapability> + '_ {
        self.compile.iter().copied()
    }

    pub fn execution_capabilities(
        &self,
    ) -> impl ExactSizeIterator<Item = ExecutionCapability> + '_ {
        self.execute.iter().copied()
    }

    #[must_use]
    pub fn supports(&self, capability: CompileCapability) -> bool {
        self.compile.contains(&capability)
    }
}

impl ExecutionSupport for SupportProfile {
    fn supports_execution(&self, capability: ExecutionCapability) -> bool {
        self.execute.contains(&capability)
    }
}

impl CompileSupport for SupportProfile {
    fn supports(&self, capability: CompileCapability) -> bool {
        self.supports(capability)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupportProfileError {
    UnimplementedCapability(CompileCapability),
    MissingPrerequisite {
        capability: CompileCapability,
        prerequisite: CompileCapability,
    },
    UnimplementedExecutionCapability(ExecutionCapability),
    MissingExecutionPrerequisite {
        capability: ExecutionCapability,
        prerequisite: ExecutionCapability,
    },
}

impl fmt::Display for SupportProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnimplementedCapability(capability) => write!(
                formatter,
                "support profile includes unimplemented capability {}",
                capability.stable_key()
            ),
            Self::MissingPrerequisite {
                capability,
                prerequisite,
            } => write!(
                formatter,
                "support capability {} requires {}",
                capability.stable_key(),
                prerequisite.stable_key()
            ),
            Self::UnimplementedExecutionCapability(capability) => write!(
                formatter,
                "support profile includes unimplemented execution capability {}",
                capability.stable_key()
            ),
            Self::MissingExecutionPrerequisite {
                capability,
                prerequisite,
            } => write!(
                formatter,
                "execution capability {} requires {}",
                capability.stable_key(),
                prerequisite.stable_key()
            ),
        }
    }
}

impl std::error::Error for SupportProfileError {}
