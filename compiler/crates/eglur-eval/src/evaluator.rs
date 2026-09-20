use std::collections::{BTreeMap, BTreeSet};

use eglur_hir::{
    BinaryOperator, Body, ExprId, ExprKind, FunctionBody, ItemKind, Literal, LiteralKind,
    PatternElement, PatternId, PatternKind, PatternPayload, StmtId, StmtKind, UnaryOperator,
};
use eglur_sema::{
    ArgumentPassing, CallableSignature, CanonicalTarget, CheckedDispatch, CheckedEntryResult,
    CheckedNumericOperator, CheckedProgram, DeclarationSignature, DefId, EvidenceEnvironment,
    EvidenceSource, ExpressionKey, ExternalDefId, FactState, IntegerEvaluation, InvocationArgument,
    InvocationFact, InvocationTarget, LanguageItem, LocalBindingId, MemberKind, ModuleId,
    NumericFaultCause, PhaseProduct, PrimitiveType, ResolutionKey, ResolvedName, SubstId,
    TraitEvidence, TypeId, TypeKind, TypedPlaceKind, WitnessSlotId,
};

use crate::{
    ExecutionCapability, ExecutionSupport, RuntimeCatalog, RuntimeDiagnostic, RuntimeEvent,
    RuntimeHook, Value,
};

/// Deterministic out-of-band evaluator limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvaluationLimits {
    max_steps: u64,
    max_call_depth: u32,
}

impl EvaluationLimits {
    #[must_use]
    pub const fn new(max_steps: u64, max_call_depth: u32) -> Self {
        Self {
            max_steps,
            max_call_depth,
        }
    }

    #[must_use]
    pub const fn max_steps(self) -> u64 {
        self.max_steps
    }

    #[must_use]
    pub const fn max_call_depth(self) -> u32 {
        self.max_call_depth
    }
}

impl Default for EvaluationLimits {
    fn default() -> Self {
        Self::new(1_000_000, 1_024)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolLimit {
    Steps { limit: u64 },
    CallDepth { limit: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedFeature {
    MissingEntry,
    OrdinaryArithmetic,
    FloatOrComplexValue,
    FormatString,
    FunctionValue,
    NonlocalValue,
    PropertyRead,
    TriStateCondition,
    HookArgumentType,
    ReferenceAggregate,
    OwnershipOperation,
    Loop,
    Closure,
    Container,
    UnsafeOrPointer,
    Tensor,
    AdvancedPattern,
    InvalidCheckedProgram,
}

impl UnsupportedFeature {
    /// Complete typed evaluator non-support vocabulary for this revision.
    pub const ALL: [Self; 18] = [
        Self::MissingEntry,
        Self::OrdinaryArithmetic,
        Self::FloatOrComplexValue,
        Self::FormatString,
        Self::FunctionValue,
        Self::NonlocalValue,
        Self::PropertyRead,
        Self::TriStateCondition,
        Self::HookArgumentType,
        Self::ReferenceAggregate,
        Self::OwnershipOperation,
        Self::Loop,
        Self::Closure,
        Self::Container,
        Self::UnsafeOrPointer,
        Self::Tensor,
        Self::AdvancedPattern,
        Self::InvalidCheckedProgram,
    ];

    #[must_use]
    pub const fn stable_key(self) -> &'static str {
        match self {
            Self::MissingEntry => "missing_entry",
            Self::OrdinaryArithmetic => "ordinary_arithmetic",
            Self::FloatOrComplexValue => "float_or_complex_value",
            Self::FormatString => "format_string",
            Self::FunctionValue => "function_value",
            Self::NonlocalValue => "nonlocal_value",
            Self::PropertyRead => "property_read",
            Self::TriStateCondition => "tri_state_condition",
            Self::HookArgumentType => "hook_argument_type",
            Self::ReferenceAggregate => "reference_aggregate",
            Self::OwnershipOperation => "ownership_operation",
            Self::Loop => "loop",
            Self::Closure => "closure",
            Self::Container => "container",
            Self::UnsafeOrPointer => "unsafe_or_pointer",
            Self::Tensor => "tensor",
            Self::AdvancedPattern => "advanced_pattern",
            Self::InvalidCheckedProgram => "invalid_checked_program",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SupportReason {
    MissingCapability(ExecutionCapability),
    MissingRuntimeHook(ExternalDefId),
    Feature(UnsupportedFeature),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedExecution {
    value: Value,
    process_status: u8,
    value_diagnostics: Box<[RuntimeDiagnostic]>,
    events: Box<[RuntimeEvent]>,
}

impl CompletedExecution {
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub const fn process_status(&self) -> u8 {
        self.process_status
    }

    #[must_use]
    pub fn value_diagnostics(&self) -> &[RuntimeDiagnostic] {
        &self.value_diagnostics
    }

    #[must_use]
    pub fn events(&self) -> &[RuntimeEvent] {
        &self.events
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionOutcome {
    Completed(CompletedExecution),
    RuntimeAbort(RuntimeDiagnostic),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionAttempt {
    Executed(ExecutionOutcome),
    Unsupported(SupportReason),
    ToolLimitExceeded(ToolLimit),
}

/// Preflights and executes one checked executable program.
///
/// `catalog` must be the validated runtime projection of the trusted catalog used to produce
/// `program`.
#[must_use]
pub fn execute(
    program: &CheckedProgram,
    support: &dyn ExecutionSupport,
    catalog: &dyn RuntimeCatalog,
    limits: EvaluationLimits,
) -> ExecutionAttempt {
    let Some(entry) = program.entry() else {
        return ExecutionAttempt::Unsupported(SupportReason::Feature(
            UnsupportedFeature::MissingEntry,
        ));
    };
    let view = ProgramView::new(program);
    let mut planner = Planner::new(view, support, catalog, limits);
    let plan = planner.plan_callable(entry.declaration(), &Witnesses::default());
    if let Some(limit) = planner.limit() {
        return ExecutionAttempt::ToolLimitExceeded(limit);
    }
    if let Err(reason) = plan {
        return ExecutionAttempt::Unsupported(reason);
    }

    let mut evaluator = Evaluator::new(view, catalog, limits);
    match evaluator.call_source(entry.declaration(), Vec::new(), Witnesses::default()) {
        Flow::Value(value) | Flow::Return(value) => {
            let Some(value) = value.into_public() else {
                return ExecutionAttempt::Unsupported(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ));
            };
            let process_status = match process_status(program, entry.result(), &value) {
                Some(status) => status,
                None => {
                    return ExecutionAttempt::Unsupported(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    ));
                }
            };
            let mut value_diagnostics = Vec::new();
            value.collect_value_diagnostics(&mut value_diagnostics);
            ExecutionAttempt::Executed(ExecutionOutcome::Completed(CompletedExecution {
                value,
                process_status,
                value_diagnostics: value_diagnostics.into_boxed_slice(),
                events: evaluator.events.into_boxed_slice(),
            }))
        }
        Flow::RuntimeAbort(diagnostic) => {
            ExecutionAttempt::Executed(ExecutionOutcome::RuntimeAbort(diagnostic))
        }
        Flow::Limit(limit) => ExecutionAttempt::ToolLimitExceeded(limit),
        Flow::InvalidCheckedProgram => ExecutionAttempt::Unsupported(SupportReason::Feature(
            UnsupportedFeature::InvalidCheckedProgram,
        )),
        Flow::Yield(_) | Flow::ShortCircuit(_) | Flow::NumericFault(_) => {
            ExecutionAttempt::Unsupported(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))
        }
    }
}

fn process_status(
    program: &CheckedProgram,
    result: CheckedEntryResult,
    value: &Value,
) -> Option<u8> {
    match result {
        CheckedEntryResult::Unit => matches!(value, Value::Unit).then_some(0),
        CheckedEntryResult::Never => None,
        CheckedEntryResult::Result { .. } => {
            let items = program.resolution().signatures().language_items();
            if value.variant_declaration() == items.declaration(LanguageItem::ResOk) {
                Some(0)
            } else if value.variant_declaration() == items.declaration(LanguageItem::ResErr) {
                Some(1)
            } else {
                None
            }
        }
    }
}

mod checked_program;
mod numeric;
mod preflight;
mod runtime;
mod store;

use checked_program::{ProgramView, WitnessValue, Witnesses, child_witnesses};
use numeric::numeric_fault;
use preflight::Planner;
use runtime::{Evaluator, Flow};
use store::{Place, Projection, RuntimeValue, Store};
