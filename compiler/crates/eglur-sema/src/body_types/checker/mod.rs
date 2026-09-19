use std::collections::{BTreeMap, BTreeSet};

use eglur_diagnostics::{DiagnosticDataValue, DiagnosticStore, Poison, RelatedSpan, SourceSpan};
use eglur_hir::{
    BinaryOperator, Body, BodyId, CallArgument, CallExpr, ConstructorPayload, ExprId, ExprKind,
    FunctionBody, HirFile, ItemKind, Literal, LiteralKind, Mutability, Name, OriginId,
    PatternElement, PatternId, PatternKind, ReduceByCandidateExpr, StmtId, StmtKind,
    TensorSchemaFamily, TypeRefId, UnaryOperator,
};
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    TypecheckCapability,
};
use crate::diagnostic::{record_type_error, record_type_error_with_related};
use crate::overloads::exact_parameter_match;
use crate::type_rendering::DiagnosticTypeRenderer;
use crate::{
    CallableInstanceId, CallableParameter, CallableRequirement, CallableSignature,
    CallableSignatureId, CanonicalTarget, CheckedNumericOperation, CheckedNumericOperator,
    ConstructorPayloadSignature, ControlRegion, ControlRegionKey, ControlTransfer,
    ControlTransferKind, ControlTransferSource, DeclarationCollection, DeclarationKind,
    DeclarationSignature, DeclarationSignatures, DefId, DuplicateFact, EnumVariantPayloadSignature,
    ExpressionKey, ExternalNameEnvironment, ExternalSemanticOwner, FactState, FactTable,
    GenericParameterId, IfBranch, Inhabitance, InhabitanceFacts, IntegerEvaluation, IntegerValue,
    IntrinsicNominal, LanguageItem, LocalBindingId, ModuleId, NameKey, NameLookupMaterialization,
    NameResolution, Namespace, NominalHead, NormalContinuation, NumericFaultCause, PrimitiveType,
    RawPointerMutability, ReferenceMutability, ResolutionKey, ResolvedDeclarationSemantics,
    ResolvedImports, ResolvedName, SemanticFact, SemanticLocation, SemanticSource, StatementKey,
    SubstId, TraitEnvironment, TraitNormalization, TraitObligation, TraitReceiverCandidate,
    TraitRef, TypeId, TypeKind, TypeOccurrenceId, materialize_name_lookup,
};

use super::{BodyTypes, BodyTypingError, *};

mod application;
mod calls;
mod constants;
mod construction;
mod control;
mod dispatch;
mod expression;
mod failure;
mod invocation;
mod numeric;
mod orchestration;
mod ownership;
mod patterns;
mod statements;
mod support;
mod tensor;
mod traversal;

/// Checks source bodies once against canonical declaration signatures.
#[allow(
    clippy::too_many_arguments,
    reason = "the phase boundary keeps independently owned semantic products explicit"
)]
pub fn typecheck_bodies(
    collection: &DeclarationCollection,
    imports: &ResolvedImports,
    names: &NameResolution,
    external_names: Option<&ExternalNameEnvironment>,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    declarations: &mut ResolvedDeclarationSemantics,
    trait_environment: &mut TraitEnvironment,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<BodyTypes>, BodyTypingError> {
    let mut recorder = DemandRecorder::new(support);
    let capability = CompileCapability::Typecheck(TypecheckCapability::CheckBodies);
    let mut supported = true;
    for module in collection.modules() {
        supported &= recorder.require(capability, SemanticLocation::source(module.file()));
    }
    if !supported {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }

    let mut source_map = BTreeMap::new();
    for source in sources {
        if source_map.insert(source.file(), source).is_some() {
            return Err(BodyTypingError::DuplicateSource(source.file()));
        }
    }
    let (signatures, inhabitance) = declarations.parts_mut();
    let mut checker = BodyChecker {
        collection,
        imports,
        names,
        external_names,
        sources: source_map,
        signatures,
        inhabitance,
        trait_environment,
        diagnostics,
        recorder,
        expressions: FactTable::new(),
        const_eligibility: BTreeMap::new(),
        preparing_numeric_tree: false,
        evaluate_constant_declarations: support.supports(CompileCapability::Typecheck(
            TypecheckCapability::EvaluateSelectedIntegerConstDeclarations,
        )),
        locals: FactTable::new(),
        members: FactTable::new(),
        invocations: FactTable::new(),
        reduce_by_commitments: FactTable::new(),
        regions: FactTable::new(),
        checked_numeric: FactTable::new(),
        ownership_operations: FactTable::new(),
        bodies: FactTable::new(),
        transfers: Vec::new(),
        callable_instances: Vec::new(),
        callable_instance_ids: BTreeMap::new(),
        obligations: Vec::new(),
        method_dispatches: Vec::new(),
        region_stack: Vec::new(),
        block_region_overrides: Vec::new(),
        result_contexts: Vec::new(),
        question_floors: vec![0],
        checked_contexts: Vec::new(),
        control_path_barriers: Vec::new(),
        current_return: None,
        current_owner: None,
    };
    checker.check_all()?;
    Ok(checker.finish())
}

struct BodyChecker<'input, 'hir, 'support> {
    collection: &'input DeclarationCollection,
    imports: &'input ResolvedImports,
    names: &'input NameResolution,
    external_names: Option<&'input ExternalNameEnvironment>,
    sources: BTreeMap<FileId, &'input SemanticSource<'hir>>,
    signatures: &'input mut DeclarationSignatures,
    inhabitance: &'input mut InhabitanceFacts,
    trait_environment: &'input mut TraitEnvironment,
    diagnostics: &'input mut DiagnosticStore,
    recorder: DemandRecorder<'support>,
    expressions: FactTable<ExpressionKey, TypedExpression>,
    const_eligibility: BTreeMap<ExpressionKey, bool>,
    preparing_numeric_tree: bool,
    evaluate_constant_declarations: bool,
    locals: FactTable<LocalBindingId, LocalTypeFact>,
    members: FactTable<ExpressionKey, MemberAccess>,
    invocations: FactTable<ExpressionKey, InvocationFact>,
    reduce_by_commitments: FactTable<ExpressionKey, ReduceByCommitment>,
    regions: FactTable<ControlRegionKey, ControlRegion>,
    checked_numeric: FactTable<ExpressionKey, CheckedNumericOperation>,
    ownership_operations: FactTable<ExpressionKey, TypedOwnershipOperation>,
    bodies: FactTable<DefId, BodyTypeSummary>,
    transfers: Vec<ControlTransfer>,
    callable_instances: Vec<CallableInstance>,
    callable_instance_ids: BTreeMap<(CallableSignatureId, SubstId), CallableInstanceId>,
    obligations: Vec<PendingTraitObligation>,
    method_dispatches: Vec<PendingMethodDispatch>,
    region_stack: Vec<ControlRegionKey>,
    block_region_overrides: Vec<ExprId>,
    result_contexts: Vec<ResultContext>,
    question_floors: Vec<usize>,
    checked_contexts: Vec<CheckedContext>,
    control_path_barriers: Vec<usize>,
    current_return: Option<TypeId>,
    current_owner: Option<DefId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureFamily {
    Res,
    Opt,
}

#[derive(Clone, Copy)]
struct FailureCarrier {
    ty: TypeId,
    family: FailureFamily,
    payload: TypeId,
    error: Option<TypeId>,
}

struct ResultContext {
    region: ControlRegionKey,
    expected: Option<FailureCarrier>,
    family: Option<FailureFamily>,
}

struct CheckedContext {
    region: ControlRegionKey,
}

#[derive(Clone)]
enum Outcome<T> {
    Known(T),
    Poisoned(Poison),
    Deferred,
    Unperformed,
    NotApplicable,
}

impl<T> Outcome<T> {
    fn map<U>(self, transform: impl FnOnce(T) -> U) -> Outcome<U> {
        match self {
            Self::Known(value) => Outcome::Known(transform(value)),
            Self::Poisoned(poison) => Outcome::Poisoned(poison),
            Self::Deferred => Outcome::Deferred,
            Self::Unperformed => Outcome::Unperformed,
            Self::NotApplicable => Outcome::NotApplicable,
        }
    }

    fn map_unit(self) -> Outcome<()> {
        self.map(|_| ())
    }
}

#[derive(Clone, Copy)]
struct BlockStep {
    normal_continuation: NormalContinuation,
    yield_type: Option<TypeId>,
}

impl BlockStep {
    const fn continuing(normal_continuation: NormalContinuation) -> Self {
        Self {
            normal_continuation,
            yield_type: None,
        }
    }

    fn yielding(value: TypedExpression) -> Self {
        Self {
            normal_continuation: NormalContinuation::Absent,
            yield_type: value.normal_continuation.is_present().then_some(value.ty),
        }
    }

    const fn exit() -> Self {
        Self {
            normal_continuation: NormalContinuation::Absent,
            yield_type: None,
        }
    }
}

#[derive(Clone, Copy)]
struct TypeRequest {
    expected: Option<TypeId>,
    allow_deferred: bool,
    default_integer: bool,
    value_required: bool,
}

#[derive(Clone, Copy)]
struct BodyContext<'input, 'hir> {
    module: ModuleId,
    source: &'input SemanticSource<'hir>,
    hir: &'hir HirFile,
    body: &'hir Body,
}

#[derive(Clone, Copy)]
struct LetBindingRequest {
    origin: OriginId,
    syntax: eglur_hir::LetSyntax,
    mutability: Mutability,
    pattern: PatternId,
    annotation: Option<TypeRefId>,
    initializer: Option<ExprId>,
}

#[derive(Clone, Copy)]
struct CheckedBinaryRequest {
    expression: ExprId,
    origin: OriginId,
    operator: BinaryOperator,
    left: ExprId,
    right: ExprId,
    ty: TypeRequest,
}

#[derive(Clone, Copy)]
struct ReturnStatementRequest {
    statement: StmtId,
    origin: OriginId,
    value: Option<ExprId>,
}

#[derive(Clone, Copy)]
struct IntrinsicCallRequest<'a> {
    expression: ExprId,
    origin: OriginId,
    call: &'a CallExpr,
    target: &'a CanonicalTarget,
    constructor: &'a crate::ConstructorSignature,
    ty: TypeRequest,
}

#[derive(Clone, Copy)]
struct MethodCallRequest<'a> {
    expression: ExprId,
    origin: OriginId,
    call: &'a CallExpr,
    receiver: ExprId,
    member: &'a Name,
    ty: TypeRequest,
}

#[derive(Clone, Copy)]
struct ApplicationRequest<'a> {
    origin: OriginId,
    generic_parameters: &'a [GenericParameterId],
    slots: &'a [ApplicationSlot],
    result_template: TypeId,
    explicit_arguments: &'a [TypeRefId],
    explicit_parameters: Option<&'a [GenericParameterId]>,
    preset_replacements: Option<&'a BTreeMap<GenericParameterId, TypeId>>,
    allowed_ownership_parameter: Option<&'a GenericParameterId>,
    ty: TypeRequest,
    defer_underconstrained: bool,
}

#[derive(Clone, Copy)]
struct TupleProjectionRequest<'a> {
    index: &'a str,
    origin: OriginId,
}

#[derive(Clone, Copy)]
struct StructLiteralRequest<'a> {
    expression: ExprId,
    origin: OriginId,
    head_origin: OriginId,
    fields: &'a [eglur_hir::FieldInitializer],
    ty: TypeRequest,
}

#[derive(Clone, Copy)]
struct VariantConstructorRequest<'a> {
    expression: ExprId,
    origin: OriginId,
    head_origin: OriginId,
    qualified: bool,
    payload: &'a ConstructorPayload,
    ty: TypeRequest,
}

struct ConstructorTarget {
    declaration: CanonicalTarget,
    signature: DeclarationSignature,
    kind: ResolvedTargetKind,
}

struct PatternConstructorContract {
    declaration: CanonicalTarget,
    generic_parameters: Vec<GenericParameterId>,
    result: TypeId,
    shape: ConstructorShape,
}

struct PatternCheck {
    coverage: PatternCoverage,
    irrefutable: bool,
    bindings: BTreeMap<LocalBindingId, TypeId>,
}

impl PatternCheck {
    fn irrefutable() -> Self {
        Self {
            coverage: PatternCoverage {
                total: true,
                variants: BTreeSet::new(),
            },
            irrefutable: true,
            bindings: BTreeMap::new(),
        }
    }

    fn partial() -> Self {
        Self {
            coverage: PatternCoverage {
                total: false,
                variants: BTreeSet::new(),
            },
            irrefutable: false,
            bindings: BTreeMap::new(),
        }
    }

    fn binding(binding: LocalBindingId, ty: TypeId) -> Self {
        Self::irrefutable().with_binding(binding, ty)
    }

    fn with_binding(mut self, binding: LocalBindingId, ty: TypeId) -> Self {
        self.bindings.entry(binding).or_insert(ty);
        self
    }

    fn merge(&mut self, other: Self) {
        self.irrefutable &= other.irrefutable;
        self.coverage.total = self.irrefutable;
        self.coverage.variants.clear();
        for (binding, ty) in other.bindings {
            self.bindings.entry(binding).or_insert(ty);
        }
    }

    fn variant(declaration: CanonicalTarget, payload: Self) -> Self {
        Self {
            coverage: PatternCoverage {
                total: false,
                variants: if payload.irrefutable {
                    BTreeSet::from([declaration])
                } else {
                    BTreeSet::new()
                },
            },
            irrefutable: false,
            bindings: payload.bindings,
        }
    }
}

#[derive(Clone)]
struct PatternCoverage {
    total: bool,
    variants: BTreeSet<CanonicalTarget>,
}

#[derive(Clone)]
struct ApplicationSlot {
    name: Box<str>,
    template: TypeId,
    value: ExprId,
    origin: OriginId,
    related: Option<SourceSpan>,
    evaluation_order: usize,
    receiver_adjustment: bool,
}

fn invocation_fact(target: InvocationTarget, slots: &[ApplicationSlot]) -> InvocationFact {
    let arguments = slots
        .iter()
        .map(|slot| slot.value)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut evaluation_order = slots
        .iter()
        .map(|slot| (slot.evaluation_order, slot.value))
        .collect::<Vec<_>>();
    evaluation_order.sort_by_key(|(order, _)| *order);
    debug_assert!(
        evaluation_order
            .windows(2)
            .all(|pair| pair[0].0 != pair[1].0),
        "checked invocation slots must have distinct evaluation order"
    );
    InvocationFact {
        target,
        arguments,
        evaluation_order: evaluation_order
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CallBindingFailure {
    InvalidName { argument_index: usize },
    UnknownNamed { argument_index: usize },
    PositionalAfterNamed { argument_index: usize },
    TooMany { argument_index: usize },
    Duplicate { argument_index: usize, slot: usize },
    Missing { slot: usize },
}

struct ResolvedApplication {
    substitution: SubstId,
    replacements: BTreeMap<GenericParameterId, TypeId>,
    specialized_slots: Box<[TypeId]>,
    result: TypeId,
    normal_continuation: NormalContinuation,
}

#[derive(Clone)]
struct MethodCandidate {
    callable: CallableSignature,
    explicit_parameters: Box<[GenericParameterId]>,
    preset_replacements: BTreeMap<GenericParameterId, TypeId>,
    dispatch: MethodDispatchTemplate,
}

#[derive(Clone)]
enum MethodDispatchTemplate {
    Direct,
    Trait {
        subject: TypeId,
        trait_ref: TraitRef,
        method: CanonicalTarget,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConstraintOutcome {
    NoEvidence,
    Evidence,
    Conflict,
}

enum ConstructorShape {
    Absent,
    Tuple(Vec<TypeId>),
    Named(Vec<CanonicalTarget>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResolvedTargetKind {
    Function,
    VariantConstructor,
    Trait,
    Impl,
    Struct,
    NominalType,
    Enum,
    Field,
    TypeAlias,
    Const,
    AssociatedType,
    AssociatedTypeBinding,
    Property,
    GenericParameter,
}

impl ResolvedTargetKind {
    const fn is_type_namespace_target(self) -> bool {
        match self {
            Self::Trait | Self::Struct | Self::NominalType | Self::Enum | Self::TypeAlias => true,
            Self::Function
            | Self::VariantConstructor
            | Self::Impl
            | Self::Field
            | Self::Const
            | Self::AssociatedType
            | Self::AssociatedTypeBinding
            | Self::Property
            | Self::GenericParameter => false,
        }
    }

    const fn from_declaration(kind: DeclarationKind) -> Self {
        match kind {
            DeclarationKind::Function | DeclarationKind::ExternFunction => Self::Function,
            DeclarationKind::IntrinsicConstructor | DeclarationKind::EnumVariant => {
                Self::VariantConstructor
            }
            DeclarationKind::Trait => Self::Trait,
            DeclarationKind::Impl => Self::Impl,
            DeclarationKind::Struct => Self::Struct,
            DeclarationKind::OpaqueType => Self::NominalType,
            DeclarationKind::Enum => Self::Enum,
            DeclarationKind::EnumVariantField | DeclarationKind::StorageField => Self::Field,
            DeclarationKind::TypeAlias => Self::TypeAlias,
            DeclarationKind::Const => Self::Const,
            DeclarationKind::AssociatedType => Self::AssociatedType,
            DeclarationKind::AssociatedTypeBinding => Self::AssociatedTypeBinding,
            DeclarationKind::Property => Self::Property,
            DeclarationKind::GenericParameter => Self::GenericParameter,
        }
    }

    const fn from_signature(signature: &DeclarationSignature) -> Self {
        match signature {
            DeclarationSignature::Callable(_) => Self::Function,
            DeclarationSignature::Constructor(_) | DeclarationSignature::EnumVariant { .. } => {
                Self::VariantConstructor
            }
            DeclarationSignature::Trait { .. } => Self::Trait,
            DeclarationSignature::Impl { .. } => Self::Impl,
            DeclarationSignature::Struct { .. } => Self::Struct,
            DeclarationSignature::Nominal { .. } => Self::NominalType,
            DeclarationSignature::Enum { .. } => Self::Enum,
            DeclarationSignature::Field { .. } => Self::Field,
            DeclarationSignature::TypeAlias { .. } => Self::TypeAlias,
            DeclarationSignature::Const { .. } => Self::Const,
            DeclarationSignature::AssociatedType { .. } => Self::AssociatedType,
            DeclarationSignature::AssociatedTypeBinding(_) => Self::AssociatedTypeBinding,
            DeclarationSignature::Property { .. } => Self::Property,
            DeclarationSignature::GenericParameter(_) => Self::GenericParameter,
        }
    }

    const fn spelling(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::VariantConstructor => "variant_constructor",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Struct => "struct",
            Self::NominalType => "nominal_type",
            Self::Enum => "enum",
            Self::Field => "field",
            Self::TypeAlias => "type_alias",
            Self::Const => "const",
            Self::AssociatedType => "associated_type",
            Self::AssociatedTypeBinding => "associated_type_binding",
            Self::Property => "property",
            Self::GenericParameter => "generic_parameter",
        }
    }
}

impl ConstructorShape {
    const fn form(&self) -> ConstructorPayloadForm {
        match self {
            Self::Absent => ConstructorPayloadForm::Unit,
            Self::Tuple(_) => ConstructorPayloadForm::Positional,
            Self::Named(_) => ConstructorPayloadForm::Named,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConstructorPayloadForm {
    Unit,
    Positional,
    Named,
}

impl ConstructorPayloadForm {
    const fn from_payload(payload: &ConstructorPayload) -> Self {
        match payload {
            ConstructorPayload::Absent => Self::Unit,
            ConstructorPayload::Tuple(_) => Self::Positional,
            ConstructorPayload::Named(_) => Self::Named,
        }
    }

    const fn from_pattern(payload: &eglur_hir::PatternPayload) -> Self {
        match payload {
            eglur_hir::PatternPayload::Absent => Self::Unit,
            eglur_hir::PatternPayload::Positional(_) => Self::Positional,
            eglur_hir::PatternPayload::Named(_) => Self::Named,
        }
    }

    const fn spelling(self) -> &'static str {
        match self {
            Self::Unit => "unit",
            Self::Positional => "positional",
            Self::Named => "named",
        }
    }
}

impl TypeRequest {
    const fn definitive(expected: Option<TypeId>) -> Self {
        Self {
            expected,
            allow_deferred: false,
            default_integer: true,
            value_required: true,
        }
    }

    const fn discarded() -> Self {
        Self {
            expected: None,
            allow_deferred: false,
            default_integer: true,
            value_required: false,
        }
    }

    const fn probe() -> Self {
        Self {
            expected: None,
            allow_deferred: true,
            default_integer: false,
            value_required: true,
        }
    }
}

#[derive(Clone, Copy)]
enum PatternBindingMode {
    Parameter,
    Let(Mutability),
}

impl PatternBindingMode {
    const fn mutability(self) -> Mutability {
        match self {
            Self::Parameter => Mutability::Immutable,
            Self::Let(mutability) => mutability,
        }
    }
}

fn fixed_width_integer_max(primitive: PrimitiveType) -> Option<&'static str> {
    match primitive {
        PrimitiveType::I8 => Some("127"),
        PrimitiveType::I16 => Some("32767"),
        PrimitiveType::I32 => Some("2147483647"),
        PrimitiveType::I64 => Some("9223372036854775807"),
        PrimitiveType::U8 => Some("255"),
        PrimitiveType::U16 => Some("65535"),
        PrimitiveType::U32 => Some("4294967295"),
        PrimitiveType::U64 => Some("18446744073709551615"),
        _ => None,
    }
}

fn decimal_literal_fits(spelling: &str, maximum: &str) -> bool {
    let mut digits = Vec::with_capacity(spelling.len());
    let mut leading_zero = true;
    for byte in spelling.bytes() {
        if byte == b'_' {
            continue;
        }
        if !byte.is_ascii_digit() {
            return false;
        }
        if leading_zero && byte == b'0' {
            continue;
        }
        leading_zero = false;
        digits.push(byte);
    }
    digits.len() < maximum.len()
        || (digits.len() == maximum.len() && digits.as_slice() <= maximum.as_bytes())
}

fn decimal_literal_is_zero(spelling: &str) -> bool {
    let mut saw_digit = false;
    for byte in spelling.bytes() {
        if byte == b'_' {
            continue;
        }
        if !byte.is_ascii_digit() || byte != b'0' {
            return false;
        }
        saw_digit = true;
    }
    saw_digit
}
