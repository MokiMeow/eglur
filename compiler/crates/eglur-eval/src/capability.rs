/// Independently selectable operations owned by checked-program execution.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ExecutionCapability {
    CoreValues,
    Calls,
    Aggregates,
    ReferencesAndMutation,
    Branching,
    FailureFlow,
    CheckedIntegerArithmetic,
    TrustedScalarHooks,
    TestEffects,
}

impl ExecutionCapability {
    /// Complete closed execution-capability vocabulary for this evaluator revision.
    pub const ALL: [Self; 9] = [
        Self::CoreValues,
        Self::Calls,
        Self::Aggregates,
        Self::ReferencesAndMutation,
        Self::Branching,
        Self::FailureFlow,
        Self::CheckedIntegerArithmetic,
        Self::TrustedScalarHooks,
        Self::TestEffects,
    ];

    #[must_use]
    pub const fn stable_key(self) -> &'static str {
        match self {
            Self::CoreValues => "execute.core_values",
            Self::Calls => "execute.calls",
            Self::Aggregates => "execute.aggregates",
            Self::ReferencesAndMutation => "execute.references_and_mutation",
            Self::Branching => "execute.branching",
            Self::FailureFlow => "execute.failure_flow",
            Self::CheckedIntegerArithmetic => "execute.checked_integer_arithmetic",
            Self::TrustedScalarHooks => "execute.trusted_scalar_hooks",
            Self::TestEffects => "execute.test_effects",
        }
    }

    #[must_use]
    pub const fn prerequisites(self) -> &'static [Self] {
        const CORE: &[ExecutionCapability] = &[ExecutionCapability::CoreValues];
        const CALLS: &[ExecutionCapability] =
            &[ExecutionCapability::CoreValues, ExecutionCapability::Calls];
        match self {
            Self::CoreValues => &[],
            Self::Calls
            | Self::Aggregates
            | Self::ReferencesAndMutation
            | Self::Branching
            | Self::FailureFlow
            | Self::CheckedIntegerArithmetic => CORE,
            Self::TrustedScalarHooks | Self::TestEffects => CALLS,
        }
    }

    #[must_use]
    pub const fn is_implemented(self) -> bool {
        true
    }
}

/// Pure execution-support query implemented by a driver-owned profile.
pub trait ExecutionSupport {
    fn supports_execution(&self, capability: ExecutionCapability) -> bool;
}

/// Runtime operation selected by trusted canonical declaration identity.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RuntimeHook {
    OptSome,
    OptNone,
    ResOk,
    ResErr,
    ToStr,
    WrappingAdd,
    WrappingSub,
    WrappingMul,
    TestEmit,
    TestAbort,
}

impl RuntimeHook {
    #[must_use]
    pub const fn capability(self) -> ExecutionCapability {
        match self {
            Self::OptSome | Self::OptNone | Self::ResOk | Self::ResErr => {
                ExecutionCapability::FailureFlow
            }
            Self::ToStr | Self::WrappingAdd | Self::WrappingSub | Self::WrappingMul => {
                ExecutionCapability::TrustedScalarHooks
            }
            Self::TestEmit | Self::TestAbort => ExecutionCapability::TestEffects,
        }
    }
}

/// Canonical trusted-hook lookup for the catalog used to check the program.
///
/// Implementations are trusted boundary inputs: mappings must be stable for one execution and
/// must agree with the external declaration identities and contracts supplied during checking.
/// They must never rediscover a hook from source spelling.
pub trait RuntimeCatalog {
    fn runtime_hook(&self, declaration: &eglur_sema::ExternalDefId) -> Option<RuntimeHook>;
}
