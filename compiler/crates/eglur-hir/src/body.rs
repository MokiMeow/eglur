use crate::{
    AxisExprId, BodyId, DesugaringKind, ExprId, Literal, Mutability, Name, OriginId, Path,
    PatternId, StmtId, TypeRefId,
};

/// One lexical semantic body with body-qualified occurrence arenas.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Body {
    id: BodyId,
    expressions: Box<[Expr]>,
    statements: Box<[Stmt]>,
    patterns: Box<[Pattern]>,
    axes: Box<[AxisExpr]>,
}

impl Body {
    pub(crate) fn new(
        id: BodyId,
        expressions: Box<[Expr]>,
        statements: Box<[Stmt]>,
        patterns: Box<[Pattern]>,
        axes: Box<[AxisExpr]>,
    ) -> Self {
        Self {
            id,
            expressions,
            statements,
            patterns,
            axes,
        }
    }

    #[must_use]
    pub const fn id(&self) -> BodyId {
        self.id
    }

    #[must_use]
    pub fn expression(&self, id: ExprId) -> Option<&Expr> {
        (id.body() == self.id).then(|| &self.expressions[id.index()])
    }

    pub fn expressions(&self) -> impl ExactSizeIterator<Item = (ExprId, &Expr)> {
        self.expressions
            .iter()
            .enumerate()
            .map(|(index, expr)| (ExprId::from_index(self.id, index), expr))
    }

    #[must_use]
    pub fn statement(&self, id: StmtId) -> Option<&Stmt> {
        (id.body() == self.id).then(|| &self.statements[id.index()])
    }

    pub fn statements(&self) -> impl ExactSizeIterator<Item = (StmtId, &Stmt)> {
        self.statements
            .iter()
            .enumerate()
            .map(|(index, stmt)| (StmtId::from_index(self.id, index), stmt))
    }

    #[must_use]
    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        (id.body() == self.id).then(|| &self.patterns[id.index()])
    }

    pub fn patterns(&self) -> impl ExactSizeIterator<Item = (PatternId, &Pattern)> {
        self.patterns
            .iter()
            .enumerate()
            .map(|(index, pattern)| (PatternId::from_index(self.id, index), pattern))
    }

    #[must_use]
    pub fn axis(&self, id: AxisExprId) -> Option<&AxisExpr> {
        (id.body() == self.id).then(|| &self.axes[id.index()])
    }

    pub fn axes(&self) -> impl ExactSizeIterator<Item = (AxisExprId, &AxisExpr)> {
        self.axes
            .iter()
            .enumerate()
            .map(|(index, axis)| (AxisExprId::from_index(self.id, index), axis))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Expr {
    origin: OriginId,
    kind: ExprKind,
}

impl Expr {
    pub(crate) const fn new(origin: OriginId, kind: ExprKind) -> Self {
        Self { origin, kind }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn kind(&self) -> &ExprKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExprKind {
    Block {
        statements: Box<[StmtId]>,
    },
    Binary {
        operator: BinaryOperator,
        left: ExprId,
        right: ExprId,
    },
    Unary {
        operator: UnaryOperator,
        operand: ExprId,
    },
    Reference {
        mutability: Mutability,
        value: ExprId,
    },
    Own(Name),
    Move(Name),
    DropOwn {
        explicit_move: bool,
        name: Name,
    },
    Call(CallExpr),
    CatalogCall(CatalogCallExpr),
    Member {
        receiver: ExprId,
        member: Name,
    },
    TupleProjection {
        receiver: ExprId,
        index: Box<str>,
    },
    Try(ExprId),
    DTypeCall(DTypeCallExpr),
    ReduceByCandidate(ReduceByCandidateExpr),
    Literal(Literal),
    Name(Path),
    Hole,
    Unit,
    Group(ExprId),
    Unsafe(ExprId),
    Tuple(Box<[ExprId]>),
    Array(Box<[ExprId]>),
    Map(Box<[MapEntry]>),
    StructLiteral {
        path: Path,
        fields: Box<[FieldInitializer]>,
    },
    VariantConstructor {
        path: Path,
        payload: ConstructorPayload,
    },
    Lambda(LambdaExpr),
    If(IfExpr),
    Match(MatchExpr),
    Result(ExprId),
    Checked(ExprId),
    TensorSchema(TensorSchemaExpr),
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BinaryOperator {
    LogicalOr,
    LogicalAnd,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Range,
    RangeInclusive,
    Add,
    Subtract,
    Multiply,
    Divide,
    FractionalDivide,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnaryOperator {
    Not,
    Negate,
    Dereference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallExpr {
    pub callee: ExprId,
    pub arguments: Box<[CallArgument]>,
    pub type_arguments: Box<[TypeRefId]>,
    pub syntax: CallSyntax,
    pub partial_application: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CallSyntax {
    Direct,
    ExplicitTypeArguments,
    /// A trailing block lowered as a synthetic final argument, before callable resolution.
    TrailingArgument,
    PostfixThen,
    PostfixApply,
}

impl CallSyntax {
    /// Returns the rewrite that produced the call occurrence itself.
    #[must_use]
    pub const fn call_desugaring(self) -> Option<DesugaringKind> {
        match self {
            Self::Direct | Self::ExplicitTypeArguments | Self::TrailingArgument => None,
            Self::PostfixThen => Some(DesugaringKind::PostfixThen),
            Self::PostfixApply => Some(DesugaringKind::PostfixApply),
        }
    }

    /// Returns the rewrite that produced a trailing argument occurrence.
    #[must_use]
    pub const fn argument_desugaring(self) -> Option<DesugaringKind> {
        match self {
            Self::TrailingArgument => Some(DesugaringKind::TrailingArgument),
            Self::Direct | Self::ExplicitTypeArguments | Self::PostfixThen | Self::PostfixApply => {
                None
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallArgument {
    pub name: Option<Name>,
    pub value: ExprId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogCallExpr {
    pub owner: Path,
    pub owner_arguments: Box<[TypeRefId]>,
    pub member: Name,
    pub arguments: Box<[CallArgument]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DTypeCallExpr {
    pub callee: ExprId,
    pub dtype: TypeRefId,
    pub arguments: Box<[CallArgument]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReduceByCandidateExpr {
    pub receiver: ExprId,
    pub arguments_origin: OriginId,
    pub ordinary: Option<ReduceByOrdinaryCandidate>,
    pub single_reducer: Option<ReduceBySingleCandidate>,
    pub ordered_phases: Option<ReduceByPhasesCandidate>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReduceByOrdinaryCandidate {
    pub call: CallExpr,
    pub candidate_names: Box<[ExprId]>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReduceBySingleCandidate {
    pub value: ReduceBySingleValue,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReduceBySingleValue {
    Expression(ExprId),
    PhasePair {
        axis: Name,
        reducer: ExprId,
        origin: OriginId,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReduceByPhasesCandidate {
    pub phases: Box<[ReduceByPhase]>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReduceByPhase {
    pub axes: Box<[Name]>,
    pub reducer: ExprId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapEntry {
    pub key: ExprId,
    pub value: ExprId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldInitializer {
    pub name: Name,
    pub value: ExprId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConstructorPayload {
    Absent,
    Tuple(Box<[ExprId]>),
    Named(Box<[FieldInitializer]>),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LambdaSyntax {
    /// A closure with an explicit parameter list.
    Explicit,
    /// A block without parameters, before placeholder or thunk classification.
    Implicit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LambdaExpr {
    pub syntax: LambdaSyntax,
    pub body: BodyId,
    pub parameters: Box<[crate::Parameter]>,
    pub statements: Box<[StmtId]>,
    pub tail: Option<ExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IfExpr {
    pub condition: ExprId,
    pub then_branch: ExprId,
    pub else_branch: Option<ExprId>,
    pub unknown_branch: Option<ExprId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchExpr {
    pub scrutinee: ExprId,
    pub arms: Box<[MatchArm]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchArm {
    pub pattern: PatternId,
    pub guard: Option<MatchGuard>,
    pub value: ExprId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchGuard {
    pub condition: ExprId,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stmt {
    origin: OriginId,
    kind: StmtKind,
}

impl Stmt {
    pub(crate) const fn new(origin: OriginId, kind: StmtKind) -> Self {
        Self { origin, kind }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn kind(&self) -> &StmtKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StmtKind {
    Let {
        syntax: LetSyntax,
        mutability: Mutability,
        pattern: PatternId,
        ty: Option<TypeRefId>,
        initializer: Option<ExprId>,
    },
    Assign {
        target: ExprId,
        value: ExprId,
    },
    While {
        condition: ExprId,
        body: ExprId,
    },
    For {
        binding: Name,
        iterable: ExprId,
        body: ExprId,
    },
    Break,
    Continue,
    Return(Option<ExprId>),
    Expression(ExprId),
    Yield(ExprId),
    Invalid,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LetSyntax {
    Let,
    Var,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pattern {
    origin: OriginId,
    kind: PatternKind,
}

impl Pattern {
    pub(crate) const fn new(origin: OriginId, kind: PatternKind) -> Self {
        Self { origin, kind }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn kind(&self) -> &PatternKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternKind {
    Or(Box<[PatternId]>),
    Alias {
        name: Name,
        pattern: PatternId,
    },
    Binding(Name),
    Wildcard,
    Group(PatternId),
    Literal(Literal),
    Unit,
    Variant {
        path: Path,
        payload: PatternPayload,
    },
    Tuple(Box<[PatternElement]>),
    Struct {
        path: Option<Path>,
        fields: Box<[FieldPattern]>,
    },
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternPayload {
    Absent,
    Positional(Box<[PatternElement]>),
    Named(Box<[FieldPattern]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternElement {
    Pattern(PatternId),
    Rest(OriginId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldPattern {
    Rest(OriginId),
    Field {
        name: Name,
        pattern: Option<PatternId>,
        origin: OriginId,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TensorSchemaFamily {
    View,
    Rearrange,
    Inflate,
    Reduce,
    Contract,
    Einop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TensorSchemaExpr {
    pub family: TensorSchemaFamily,
    pub operands: Box<[TransformAxisList]>,
    pub outputs: Box<[TransformAxisList]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransformAxisList {
    pub axes: Box<[AxisExprId]>,
    pub origin: OriginId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AxisExpr {
    origin: OriginId,
    kind: AxisExprKind,
}

impl AxisExpr {
    pub(crate) const fn new(origin: OriginId, kind: AxisExprKind) -> Self {
        Self { origin, kind }
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn kind(&self) -> &AxisExprKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AxisExprKind {
    Name(Name),
    Integer(Literal),
    Pack(Option<Name>),
    Group(AxisExprId),
    Add { left: AxisExprId, right: AxisExprId },
    Multiply { left: AxisExprId, right: AxisExprId },
    Invalid,
}
