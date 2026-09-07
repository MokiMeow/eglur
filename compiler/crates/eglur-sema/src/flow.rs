use eglur_hir::{BinaryOperator, ExprId, StmtId};

use crate::{DefId, ModuleId, TypeId};

/// Module-qualified expression identity used by body-owned semantic facts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ExpressionKey {
    module: ModuleId,
    expression: ExprId,
}

impl ExpressionKey {
    #[must_use]
    pub const fn new(module: ModuleId, expression: ExprId) -> Self {
        Self { module, expression }
    }

    #[must_use]
    pub const fn module(self) -> ModuleId {
        self.module
    }

    #[must_use]
    pub const fn expression(self) -> ExprId {
        self.expression
    }
}

/// Module-qualified statement identity used by control-transfer facts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StatementKey {
    module: ModuleId,
    statement: StmtId,
}

impl StatementKey {
    #[must_use]
    pub const fn new(module: ModuleId, statement: StmtId) -> Self {
        Self { module, statement }
    }

    #[must_use]
    pub const fn module(self) -> ModuleId {
        self.module
    }

    #[must_use]
    pub const fn statement(self) -> StmtId {
        self.statement
    }
}

/// Stable source slot of an `if` branch region.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum IfBranch {
    Then,
    Else,
    Unknown,
}

/// Structural identity of one finite lexical control/obligation region.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlRegionKey {
    Callable(DefId),
    Block(ExpressionKey),
    IfBranch {
        owner: ExpressionKey,
        branch: IfBranch,
    },
    MatchArm {
        owner: ExpressionKey,
        arm: usize,
    },
    Result(ExpressionKey),
    Checked(ExpressionKey),
}

/// Parent-linked lexical region fact produced during body typing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlRegion {
    key: ControlRegionKey,
    parent: Option<ControlRegionKey>,
}

impl ControlRegion {
    pub(crate) const fn new(key: ControlRegionKey, parent: Option<ControlRegionKey>) -> Self {
        Self { key, parent }
    }

    #[must_use]
    pub const fn key(&self) -> &ControlRegionKey {
        &self.key
    }

    #[must_use]
    pub const fn parent(&self) -> Option<&ControlRegionKey> {
        self.parent.as_ref()
    }
}

/// Source occurrence that creates one typed control edge.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlTransferSource {
    Expression(ExpressionKey),
    Statement(StatementKey),
    RegionCompletion(ControlRegionKey),
}

/// Recoverable scalar fault captured by an enclosing `checked` region.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NumericFaultCause {
    Overflow,
    DivisionByZero,
    RemainderByZero,
}

/// Semantic reason for a typed control edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ControlTransferKind {
    NormalCompletion,
    MatchGuardFalse,
    Yield,
    Return,
    QuestionShortCircuit,
    CheckedFault(NumericFaultCause),
}

/// Target-aware control edge with its structurally fixed exited-region set.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ControlTransfer {
    source: ControlTransferSource,
    kind: ControlTransferKind,
    target: ControlRegionKey,
    exited_regions: Box<[ControlRegionKey]>,
}

impl ControlTransfer {
    pub(crate) fn new(
        source: ControlTransferSource,
        kind: ControlTransferKind,
        target: ControlRegionKey,
        exited_regions: Box<[ControlRegionKey]>,
    ) -> Self {
        Self {
            source,
            kind,
            target,
            exited_regions,
        }
    }

    #[must_use]
    pub const fn source(&self) -> &ControlTransferSource {
        &self.source
    }

    #[must_use]
    pub const fn kind(&self) -> ControlTransferKind {
        self.kind
    }

    #[must_use]
    pub const fn target(&self) -> &ControlRegionKey {
        &self.target
    }

    /// Returns exited regions from the target boundary toward the innermost region.
    #[must_use]
    pub fn exited_regions(&self) -> &[ControlRegionKey] {
        &self.exited_regions
    }
}

/// Selected scalar operation whose runtime faults are captured by `checked`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CheckedNumericOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Negate,
}

impl CheckedNumericOperator {
    #[must_use]
    pub const fn from_binary(operator: BinaryOperator) -> Option<Self> {
        match operator {
            BinaryOperator::Add => Some(Self::Add),
            BinaryOperator::Subtract => Some(Self::Subtract),
            BinaryOperator::Multiply => Some(Self::Multiply),
            BinaryOperator::Divide => Some(Self::Divide),
            BinaryOperator::Remainder => Some(Self::Remainder),
            BinaryOperator::LogicalOr
            | BinaryOperator::LogicalAnd
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
            | BinaryOperator::Range
            | BinaryOperator::RangeInclusive
            | BinaryOperator::FractionalDivide => None,
        }
    }

    #[must_use]
    pub(crate) const fn faults(self, signed: bool) -> &'static [NumericFaultCause] {
        match self {
            Self::Add | Self::Subtract | Self::Multiply | Self::Negate => {
                &[NumericFaultCause::Overflow]
            }
            Self::Divide if signed => &[
                NumericFaultCause::DivisionByZero,
                NumericFaultCause::Overflow,
            ],
            Self::Divide => &[NumericFaultCause::DivisionByZero],
            Self::Remainder => &[NumericFaultCause::RemainderByZero],
        }
    }
}

/// Evaluator-facing checked numeric operation; promotion and fault classes are fixed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckedNumericOperation {
    operator: CheckedNumericOperator,
    result: TypeId,
    faults: Box<[NumericFaultCause]>,
}

impl CheckedNumericOperation {
    pub(crate) fn new(
        operator: CheckedNumericOperator,
        result: TypeId,
        faults: Box<[NumericFaultCause]>,
    ) -> Self {
        Self {
            operator,
            result,
            faults,
        }
    }

    #[must_use]
    pub const fn operator(&self) -> CheckedNumericOperator {
        self.operator
    }

    #[must_use]
    pub const fn result(&self) -> TypeId {
        self.result
    }

    #[must_use]
    pub fn faults(&self) -> &[NumericFaultCause] {
        &self.faults
    }
}

/// Whether evaluation can reach an expression's ordinary result edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NormalContinuation {
    Absent,
    Present,
}

impl NormalContinuation {
    /// Combines inputs evaluated by one strict expression.
    #[must_use]
    pub const fn strict(self, other: Self) -> Self {
        if matches!(self, Self::Present) && matches!(other, Self::Present) {
            Self::Present
        } else {
            Self::Absent
        }
    }

    #[must_use]
    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}
