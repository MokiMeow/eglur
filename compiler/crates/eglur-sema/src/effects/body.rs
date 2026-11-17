use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_diagnostics::{DiagnosticStoreError, Poison};
use eglur_hir::{
    Body, ConstructorPayload, ExprId, ExprKind, FunctionBody, ItemKind, OriginId, PatternId,
    ReduceBySingleValue, StmtId, StmtKind,
};
use eglur_source::FileId;

use crate::resolve::PatternBindingQueryError;
use crate::{
    BodyTypes, CallableSignature, CallableSignatureId, CanonicalTarget, CheckedDispatch,
    DeclarationCollection, DeclarationSignature, DeclarationSignatures, DefId, ExpressionKey,
    FactState, InvocationTarget, LocalBindingId, ModuleId, NameResolution, ReduceByCommitment,
    ReferenceMutability, SemanticSource, TraitEvidence, TypeKind, TypeStoreError,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum MutableRoot {
    Local(LocalBindingId),
    Boundary(LocalBindingId),
    Nonlocal,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct MutablePath {
    roots: Box<[MutableRoot]>,
}

impl MutablePath {
    fn new(roots: impl IntoIterator<Item = MutableRoot>) -> Self {
        Self {
            roots: roots
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        }
    }

    pub(super) fn roots(&self) -> &[MutableRoot] {
        &self.roots
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum EffectDispatch {
    Target(CanonicalTarget),
    BoundRequirement { method: CanonicalTarget },
    Constructor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EffectCall {
    origin: OriginId,
    dispatch: EffectDispatch,
    bang: bool,
    mutable_paths: Box<[MutablePath]>,
}

impl EffectCall {
    pub(super) const fn origin(&self) -> OriginId {
        self.origin
    }

    pub(super) const fn dispatch(&self) -> &EffectDispatch {
        &self.dispatch
    }

    pub(super) const fn is_bang(&self) -> bool {
        self.bang
    }

    pub(super) fn mutable_paths(&self) -> &[MutablePath] {
        &self.mutable_paths
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EffectMutation {
    origin: OriginId,
    path: MutablePath,
}

impl EffectMutation {
    pub(super) const fn origin(&self) -> OriginId {
        self.origin
    }

    pub(super) const fn path(&self) -> &MutablePath {
        &self.path
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum EffectOperation {
    Call(EffectCall),
    Mutation(EffectMutation),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EffectBody {
    owner: DefId,
    signature: CallableSignatureId,
    operations: Box<[EffectOperation]>,
}

#[derive(Debug)]
pub(super) enum EffectBodyFact {
    Known(EffectBody),
    Poisoned { owner: DefId, poison: Poison },
    Unperformed { owner: DefId },
}

impl EffectBodyFact {
    pub(super) const fn owner(&self) -> DefId {
        match self {
            Self::Known(body) => body.owner(),
            Self::Poisoned { owner, .. } | Self::Unperformed { owner } => *owner,
        }
    }

    pub(super) const fn known(&self) -> Option<&EffectBody> {
        match self {
            Self::Known(body) => Some(body),
            Self::Poisoned { .. } | Self::Unperformed { .. } => None,
        }
    }
}

impl EffectBody {
    pub(super) const fn owner(&self) -> DefId {
        self.owner
    }

    pub(super) const fn signature(&self) -> CallableSignatureId {
        self.signature
    }

    pub(super) fn operations(&self) -> &[EffectOperation] {
        &self.operations
    }
}

#[derive(Clone, Default)]
struct FlowState {
    aliases: BTreeMap<LocalBindingId, BTreeSet<MutableRoot>>,
    escaped: BTreeSet<LocalBindingId>,
}

impl FlowState {
    fn join(states: impl IntoIterator<Item = Self>) -> Self {
        let mut joined = Self::default();
        for state in states {
            for (binding, roots) in state.aliases {
                joined.aliases.entry(binding).or_default().extend(roots);
            }
            joined.escaped.extend(state.escaped);
        }
        joined
    }

    fn mark_escaping(&mut self, roots: impl IntoIterator<Item = MutableRoot>) {
        self.escaped.extend(roots.into_iter().filter_map(|root| {
            if let MutableRoot::Local(binding) = root {
                Some(binding)
            } else {
                None
            }
        }));
    }

    fn account_for_escape(&self, root: MutableRoot) -> MutableRoot {
        match root {
            MutableRoot::Local(binding) if self.escaped.contains(&binding) => MutableRoot::Nonlocal,
            other => other,
        }
    }
}

pub(super) fn collect_effect_bodies(
    collection: &DeclarationCollection,
    names: &NameResolution,
    sources: &[SemanticSource<'_>],
    signatures: &DeclarationSignatures,
    body_types: &BodyTypes,
    trait_evidence: &TraitEvidence,
) -> Result<Box<[EffectBodyFact]>, EffectBodyError> {
    let mut source_map = BTreeMap::new();
    for source in sources {
        if source_map.insert(source.file(), source).is_some() {
            return Err(EffectBodyError::DuplicateSource(source.file()));
        }
    }
    let mut bodies = Vec::new();
    for declaration in collection
        .modules()
        .flat_map(|module| module.declarations())
    {
        let FactState::Known(DeclarationSignature::Callable(signature_id)) =
            signatures.source_declaration(declaration.id())
        else {
            continue;
        };
        let signature = signatures
            .callable(*signature_id)
            .ok_or(EffectBodyError::UnknownCallableSignature(*signature_id))?;
        if signature.body().is_none()
            || !matches!(body_types.body(declaration.id()), FactState::Known(_))
        {
            continue;
        }
        let module = collection
            .module(declaration.id().module())
            .ok_or(EffectBodyError::UnknownModule(declaration.id().module()))?;
        let source = source_map
            .get(&module.file())
            .copied()
            .ok_or(EffectBodyError::MissingSource(module.file()))?;
        let hir = source
            .hir()
            .ok_or(EffectBodyError::MissingHir(module.file()))?;
        let item_id = declaration
            .item()
            .ok_or(EffectBodyError::MissingDeclarationItem(declaration.id()))?;
        let ItemKind::Function(function) = hir.item(item_id).kind() else {
            return Err(EffectBodyError::WrongDeclarationKind(declaration.id()));
        };
        let root = match function.body_form {
            FunctionBody::Absent => continue,
            FunctionBody::Expression(root) | FunctionBody::Block(root) => root,
        };
        let body = hir.body(function.body);
        let boundary_parameters =
            boundary_parameter_bindings(module.id(), body, signature, signatures, names)?;
        let mut collector = BodyCollector {
            module: module.id(),
            body,
            names,
            signatures,
            body_types,
            trait_evidence,
            boundary_parameters,
            operations: Vec::new(),
            blocked: None,
        };
        let mut state = FlowState::default();
        collector.visit_expression(root, &mut state)?;
        bodies.push(match collector.blocked {
            Some(EffectBodyBlock::Poisoned(poison)) => EffectBodyFact::Poisoned {
                owner: declaration.id(),
                poison,
            },
            Some(EffectBodyBlock::Unperformed) => EffectBodyFact::Unperformed {
                owner: declaration.id(),
            },
            None => EffectBodyFact::Known(EffectBody {
                owner: declaration.id(),
                signature: *signature_id,
                operations: collector.operations.into_boxed_slice(),
            }),
        });
    }
    bodies.sort_by_key(EffectBodyFact::owner);
    Ok(bodies.into_boxed_slice())
}

fn boundary_parameter_bindings(
    module: ModuleId,
    body: &Body,
    signature: &CallableSignature,
    signatures: &DeclarationSignatures,
    names: &NameResolution,
) -> Result<BTreeSet<LocalBindingId>, EffectBodyError> {
    let mut bindings = BTreeSet::new();
    for parameter in signature.parameters() {
        if !matches!(
            signatures.types().kind(parameter.ty())?,
            TypeKind::Reference {
                mutability: ReferenceMutability::Mutable,
                ..
            }
        ) {
            continue;
        }
        let Some(pattern_id) = parameter.source_pattern() else {
            continue;
        };
        bindings.extend(names.pattern_bindings(module, body, pattern_id)?);
    }
    Ok(bindings)
}

struct BodyCollector<'analysis> {
    module: ModuleId,
    body: &'analysis Body,
    names: &'analysis NameResolution,
    signatures: &'analysis DeclarationSignatures,
    body_types: &'analysis BodyTypes,
    trait_evidence: &'analysis TraitEvidence,
    boundary_parameters: BTreeSet<LocalBindingId>,
    operations: Vec<EffectOperation>,
    blocked: Option<EffectBodyBlock>,
}

enum EffectBodyBlock {
    Poisoned(Poison),
    Unperformed,
}

impl BodyCollector<'_> {
    fn visit_expression(
        &mut self,
        expression_id: ExprId,
        state: &mut FlowState,
    ) -> Result<(), EffectBodyError> {
        let expression = self
            .body
            .expression(expression_id)
            .ok_or(EffectBodyError::WrongBodyExpression(expression_id))?;
        match expression.kind() {
            ExprKind::Block { statements } => {
                for &statement in statements.iter() {
                    self.visit_statement(statement, state)?;
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.visit_expression(*left, state)?;
                self.visit_expression(*right, state)?;
            }
            ExprKind::Unary { operand, .. }
            | ExprKind::Reference { value: operand, .. }
            | ExprKind::Try(operand)
            | ExprKind::Group(operand)
            | ExprKind::Unsafe(operand)
            | ExprKind::Result(operand)
            | ExprKind::Checked(operand) => self.visit_expression(*operand, state)?,
            ExprKind::Call(call) => {
                self.visit_expression(call.callee, state)?;
                for argument in call.arguments.iter() {
                    self.visit_expression(argument.value, state)?;
                }
                self.record_call(expression_id, expression.origin(), state)?;
            }
            ExprKind::CatalogCall(call) => {
                for argument in call.arguments.iter() {
                    self.visit_expression(argument.value, state)?;
                }
                self.record_call(expression_id, expression.origin(), state)?;
            }
            ExprKind::DTypeCall(call) => {
                self.visit_expression(call.callee, state)?;
                for argument in call.arguments.iter() {
                    self.visit_expression(argument.value, state)?;
                }
                self.record_call(expression_id, expression.origin(), state)?;
            }
            ExprKind::Member { receiver, .. } | ExprKind::TupleProjection { receiver, .. } => {
                self.visit_expression(*receiver, state)?;
            }
            ExprKind::ReduceByCandidate(candidate) => {
                let key = ExpressionKey::new(self.module, expression_id);
                match self.body_types.reduce_by_commitment(key) {
                    FactState::Known(ReduceByCommitment::Ordinary) => {
                        let ordinary = candidate
                            .ordinary
                            .as_ref()
                            .expect("ordinary commitment retains an ordinary candidate");
                        self.visit_expression(ordinary.call.callee, state)?;
                        for argument in ordinary.call.arguments.iter() {
                            self.visit_expression(argument.value, state)?;
                        }
                        self.record_call(expression_id, expression.origin(), state)?;
                    }
                    FactState::Known(ReduceByCommitment::TensorSingle) => {
                        self.visit_expression(candidate.receiver, state)?;
                        let single = candidate
                            .single_reducer
                            .as_ref()
                            .expect("single commitment retains a reducer");
                        let value = match single.value {
                            ReduceBySingleValue::Expression(value)
                            | ReduceBySingleValue::PhasePair { reducer: value, .. } => value,
                        };
                        self.visit_expression(value, state)?;
                    }
                    FactState::Known(ReduceByCommitment::TensorPhases) => {
                        self.visit_expression(candidate.receiver, state)?;
                        let phases = candidate
                            .ordered_phases
                            .as_ref()
                            .expect("phase commitment retains ordered phases");
                        for phase in phases.phases.iter() {
                            self.visit_expression(phase.reducer, state)?;
                        }
                    }
                    FactState::Poisoned(poison) => self.block_on_poison(poison)?,
                    FactState::Unperformed | FactState::NotApplicable => {
                        self.block_on_unperformed();
                    }
                }
            }
            ExprKind::Tuple(values) | ExprKind::Array(values) => {
                for &value in values.iter() {
                    self.visit_expression(value, state)?;
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries.iter() {
                    self.visit_expression(entry.key, state)?;
                    self.visit_expression(entry.value, state)?;
                }
            }
            ExprKind::StructLiteral { fields, .. } => {
                for field in fields.iter() {
                    self.visit_expression(field.value, state)?;
                }
            }
            ExprKind::VariantConstructor { payload, .. } => match payload {
                ConstructorPayload::Absent => {}
                ConstructorPayload::Tuple(values) => {
                    for &value in values.iter() {
                        self.visit_expression(value, state)?;
                    }
                }
                ConstructorPayload::Named(fields) => {
                    for field in fields.iter() {
                        self.visit_expression(field.value, state)?;
                    }
                }
            },
            ExprKind::If(value) => {
                self.visit_expression(value.condition, state)?;
                let entry = state.clone();
                let mut branches = Vec::new();
                let mut then_state = entry.clone();
                self.visit_expression(value.then_branch, &mut then_state)?;
                branches.push(then_state);
                for branch in [value.else_branch, value.unknown_branch]
                    .into_iter()
                    .flatten()
                {
                    let mut branch_state = entry.clone();
                    self.visit_expression(branch, &mut branch_state)?;
                    branches.push(branch_state);
                }
                if value.else_branch.is_none() {
                    branches.push(entry);
                }
                *state = FlowState::join(branches);
            }
            ExprKind::Match(value) => {
                self.visit_expression(value.scrutinee, state)?;
                let scrutinee_roots = self.expression_roots(value.scrutinee, state)?;
                let entry = state.clone();
                let mut branches = Vec::new();
                for arm in value.arms.iter() {
                    let mut branch_state = entry.clone();
                    self.bind_aliases(arm.pattern, scrutinee_roots.clone(), &mut branch_state)?;
                    if let Some(guard) = &arm.guard {
                        self.visit_expression(guard.condition, &mut branch_state)?;
                    }
                    self.visit_expression(arm.value, &mut branch_state)?;
                    branches.push(branch_state);
                }
                if !branches.is_empty() {
                    *state = FlowState::join(branches);
                }
            }
            ExprKind::Own(_)
            | ExprKind::Move(_)
            | ExprKind::DropOwn { .. }
            | ExprKind::Literal(_)
            | ExprKind::Name(_)
            | ExprKind::Hole
            | ExprKind::Unit
            | ExprKind::Lambda(_)
            | ExprKind::TensorSchema(_)
            | ExprKind::Invalid => {}
        }
        Ok(())
    }

    fn visit_statement(
        &mut self,
        statement_id: StmtId,
        state: &mut FlowState,
    ) -> Result<(), EffectBodyError> {
        let statement = self
            .body
            .statement(statement_id)
            .ok_or(EffectBodyError::WrongBodyStatement(statement_id))?;
        match statement.kind() {
            StmtKind::Let {
                pattern,
                initializer,
                ..
            } => {
                let roots = if let Some(initializer) = initializer {
                    self.visit_expression(*initializer, state)?;
                    self.expression_roots(*initializer, state)?
                } else {
                    BTreeSet::new()
                };
                self.bind_aliases(*pattern, roots, state)?;
            }
            StmtKind::Assign { target, value } => {
                self.visit_expression(*target, state)?;
                self.visit_expression(*value, state)?;
                let target_expression = self
                    .body
                    .expression(*target)
                    .ok_or(EffectBodyError::WrongBodyExpression(*target))?;
                if !matches!(target_expression.kind(), ExprKind::Hole) {
                    let target_roots = self.assignment_roots(*target, state)?;
                    self.operations
                        .push(EffectOperation::Mutation(EffectMutation {
                            origin: statement.origin(),
                            path: MutablePath::new(target_roots.iter().cloned()),
                        }));
                    if target_roots.iter().any(|root| {
                        matches!(
                            root,
                            MutableRoot::Boundary(_) | MutableRoot::Nonlocal | MutableRoot::Unknown
                        )
                    }) {
                        state.mark_escaping(self.expression_roots(*value, state)?);
                    }
                    self.update_reference_alias(*target, *value, state)?;
                }
            }
            StmtKind::While { condition, body } => {
                self.visit_expression(*condition, state)?;
                let entry = state.clone();
                let mut loop_state = entry.clone();
                self.visit_expression(*body, &mut loop_state)?;
                *state = FlowState::join([entry, loop_state]);
            }
            StmtKind::For { iterable, body, .. } => {
                self.visit_expression(*iterable, state)?;
                let entry = state.clone();
                let mut loop_state = entry.clone();
                self.visit_expression(*body, &mut loop_state)?;
                *state = FlowState::join([entry, loop_state]);
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.visit_expression(*value, state)?;
                    state.mark_escaping(self.expression_roots(*value, state)?);
                }
            }
            StmtKind::Expression(value) => self.visit_expression(*value, state)?,
            StmtKind::Yield(value) => {
                self.visit_expression(*value, state)?;
                state.mark_escaping(self.expression_roots(*value, state)?);
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Invalid => {}
        }
        Ok(())
    }

    fn record_call(
        &mut self,
        expression: ExprId,
        origin: OriginId,
        state: &FlowState,
    ) -> Result<(), EffectBodyError> {
        let key = ExpressionKey::new(self.module, expression);
        let invocation = match self.body_types.invocation(key) {
            FactState::Known(invocation) => invocation,
            FactState::Poisoned(poison) => {
                self.block_on_poison(poison)?;
                return Ok(());
            }
            FactState::Unperformed | FactState::NotApplicable => {
                self.block_on_unperformed();
                return Ok(());
            }
        };
        let checked = match self.trait_evidence.invocation(key) {
            FactState::Known(checked) => checked,
            FactState::Poisoned(poison) => {
                self.block_on_poison(poison)?;
                return Ok(());
            }
            FactState::Unperformed | FactState::NotApplicable => {
                self.block_on_unperformed();
                return Ok(());
            }
        };
        let (dispatch, bang) = match invocation.target() {
            InvocationTarget::TensorConstructor {
                declaration,
                constructor,
                ..
            } => (
                EffectDispatch::Target(declaration.clone()),
                constructor.is_bang(),
            ),
            InvocationTarget::Constructor { .. } => (EffectDispatch::Constructor, false),
            InvocationTarget::Callable(_) => match checked.dispatch() {
                CheckedDispatch::Direct(target) => {
                    if let Some(adapter) = self.signatures.trait_adapter(target) {
                        (
                            EffectDispatch::BoundRequirement {
                                method: adapter.method().clone(),
                            },
                            self.target_is_bang(adapter.method())?,
                        )
                    } else {
                        (
                            EffectDispatch::Target(target.clone()),
                            self.target_is_bang(target)?,
                        )
                    }
                }
                CheckedDispatch::Bound { method, .. } => (
                    EffectDispatch::BoundRequirement {
                        method: method.clone(),
                    },
                    self.target_is_bang(method)?,
                ),
                CheckedDispatch::ImplTemplate { callable, .. } => (
                    EffectDispatch::Target(callable.clone()),
                    self.target_is_bang(callable)?,
                ),
            },
        };
        let mutable_paths = match invocation.target() {
            InvocationTarget::TensorConstructor { .. } => Vec::new(),
            InvocationTarget::Constructor { .. } => Vec::new(),
            InvocationTarget::Callable(instance) => {
                let instance = self
                    .body_types
                    .callable_instance(*instance)
                    .ok_or(EffectBodyError::UnknownCallableInstance(*instance))?;
                let signature = self.signatures.callable(instance.signature()).ok_or(
                    EffectBodyError::UnknownCallableSignature(instance.signature()),
                )?;
                if signature.parameters().len() != invocation.arguments().len() {
                    return Err(EffectBodyError::InvocationArityMismatch(key));
                }
                signature
                    .parameters()
                    .iter()
                    .zip(invocation.arguments())
                    .filter_map(|(parameter, &argument)| {
                        matches!(
                            self.signatures.types().kind(parameter.ty()),
                            Ok(TypeKind::Reference {
                                mutability: ReferenceMutability::Mutable,
                                ..
                            })
                        )
                        .then_some(argument)
                    })
                    .map(|argument| self.expression_roots(argument, state).map(MutablePath::new))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        self.operations.push(EffectOperation::Call(EffectCall {
            origin,
            dispatch,
            bang,
            mutable_paths: mutable_paths.into_boxed_slice(),
        }));
        Ok(())
    }

    fn block_on_poison(&mut self, poison: &Poison) -> Result<(), EffectBodyError> {
        self.blocked = Some(match self.blocked.take() {
            Some(EffectBodyBlock::Poisoned(existing)) => {
                EffectBodyBlock::Poisoned(existing.merge(poison)?)
            }
            Some(EffectBodyBlock::Unperformed) | None => EffectBodyBlock::Poisoned(poison.clone()),
        });
        Ok(())
    }

    fn block_on_unperformed(&mut self) {
        if self.blocked.is_none() {
            self.blocked = Some(EffectBodyBlock::Unperformed);
        }
    }

    fn target_is_bang(&self, target: &CanonicalTarget) -> Result<bool, EffectBodyError> {
        let FactState::Known(DeclarationSignature::Callable(signature)) =
            self.signatures.declaration(target)
        else {
            return Err(EffectBodyError::NonCallableDispatch(target.clone()));
        };
        self.signatures
            .callable(*signature)
            .map(CallableSignature::is_bang)
            .ok_or(EffectBodyError::UnknownCallableSignature(*signature))
    }

    fn bind_aliases(
        &self,
        pattern: PatternId,
        initializer_roots: BTreeSet<MutableRoot>,
        state: &mut FlowState,
    ) -> Result<(), EffectBodyError> {
        let bindings = self
            .names
            .pattern_bindings(self.module, self.body, pattern)?;
        for binding in bindings {
            if self.binding_is_reference(binding)? {
                let roots = if initializer_roots.is_empty() {
                    BTreeSet::from([MutableRoot::Unknown])
                } else {
                    initializer_roots.clone()
                };
                state.aliases.insert(binding, roots);
            }
        }
        Ok(())
    }

    fn update_reference_alias(
        &self,
        target: ExprId,
        value: ExprId,
        state: &mut FlowState,
    ) -> Result<(), EffectBodyError> {
        let key = ExpressionKey::new(self.module, target);
        let FactState::Known(typed) = self.body_types.expression(key) else {
            return Ok(());
        };
        let Some(crate::TypedPlaceKind::Local(binding)) =
            typed.place().map(crate::TypedPlace::kind)
        else {
            return Ok(());
        };
        if self.binding_is_reference(*binding)? {
            let roots = self.expression_roots(value, state)?;
            state.aliases.insert(
                *binding,
                if roots.is_empty() {
                    BTreeSet::from([MutableRoot::Unknown])
                } else {
                    roots
                },
            );
        }
        Ok(())
    }

    fn binding_is_reference(&self, binding: LocalBindingId) -> Result<bool, EffectBodyError> {
        let FactState::Known(local) = self.body_types.local(binding) else {
            return Err(EffectBodyError::MissingLocalType(binding));
        };
        Ok(matches!(
            self.signatures.types().kind(local.ty())?,
            TypeKind::Reference { .. }
        ))
    }

    fn assignment_roots(
        &self,
        target: ExprId,
        state: &FlowState,
    ) -> Result<BTreeSet<MutableRoot>, EffectBodyError> {
        let key = ExpressionKey::new(self.module, target);
        if let FactState::Known(typed) = self.body_types.expression(key)
            && let Some(crate::TypedPlaceKind::Local(binding)) =
                typed.place().map(crate::TypedPlace::kind)
        {
            return Ok(BTreeSet::from([
                state.account_for_escape(MutableRoot::Local(*binding))
            ]));
        }
        self.expression_roots(target, state)
    }

    fn expression_roots(
        &self,
        expression: ExprId,
        state: &FlowState,
    ) -> Result<BTreeSet<MutableRoot>, EffectBodyError> {
        let key = ExpressionKey::new(self.module, expression);
        if let FactState::Known(typed) = self.body_types.expression(key)
            && let Some(place) = typed.place()
        {
            return self.place_roots(place.kind(), state);
        }
        let expression = self
            .body
            .expression(expression)
            .ok_or(EffectBodyError::WrongBodyExpression(expression))?;
        let mut roots = BTreeSet::new();
        match expression.kind() {
            ExprKind::Reference { value, .. }
            | ExprKind::Try(value)
            | ExprKind::Group(value)
            | ExprKind::Unsafe(value)
            | ExprKind::Result(value)
            | ExprKind::Checked(value) => roots.extend(self.expression_roots(*value, state)?),
            ExprKind::Member { receiver, .. } | ExprKind::TupleProjection { receiver, .. } => {
                roots.extend(self.expression_roots(*receiver, state)?);
            }
            ExprKind::Tuple(values) | ExprKind::Array(values) => {
                for &value in values.iter() {
                    roots.extend(self.expression_roots(value, state)?);
                }
            }
            ExprKind::Map(entries) => {
                for entry in entries.iter() {
                    roots.extend(self.expression_roots(entry.key, state)?);
                    roots.extend(self.expression_roots(entry.value, state)?);
                }
            }
            ExprKind::StructLiteral { fields, .. } => {
                for field in fields.iter() {
                    roots.extend(self.expression_roots(field.value, state)?);
                }
            }
            ExprKind::VariantConstructor { payload, .. } => match payload {
                ConstructorPayload::Absent => {}
                ConstructorPayload::Tuple(values) => {
                    for &value in values.iter() {
                        roots.extend(self.expression_roots(value, state)?);
                    }
                }
                ConstructorPayload::Named(fields) => {
                    for field in fields.iter() {
                        roots.extend(self.expression_roots(field.value, state)?);
                    }
                }
            },
            ExprKind::If(value) => {
                roots.extend(self.expression_roots(value.then_branch, state)?);
                for branch in [value.else_branch, value.unknown_branch]
                    .into_iter()
                    .flatten()
                {
                    roots.extend(self.expression_roots(branch, state)?);
                }
            }
            ExprKind::Match(value) => {
                for arm in value.arms.iter() {
                    roots.extend(self.expression_roots(arm.value, state)?);
                }
            }
            _ => {
                if let FactState::Known(typed) = self.body_types.expression(key)
                    && matches!(
                        self.signatures.types().kind(typed.ty()),
                        Ok(TypeKind::Reference { .. })
                    )
                {
                    roots.insert(MutableRoot::Unknown);
                }
            }
        }
        Ok(roots
            .into_iter()
            .map(|root| state.account_for_escape(root))
            .collect())
    }

    fn place_roots(
        &self,
        place: &crate::TypedPlaceKind,
        state: &FlowState,
    ) -> Result<BTreeSet<MutableRoot>, EffectBodyError> {
        let roots = match place {
            crate::TypedPlaceKind::Local(binding) => {
                if self.boundary_parameters.contains(binding) {
                    BTreeSet::from([MutableRoot::Boundary(*binding)])
                } else if let Some(aliases) = state.aliases.get(binding) {
                    aliases.clone()
                } else {
                    BTreeSet::from([MutableRoot::Local(*binding)])
                }
            }
            crate::TypedPlaceKind::StoredField { receiver, .. }
            | crate::TypedPlaceKind::TupleElement { receiver, .. } => {
                self.expression_roots(*receiver, state)?
            }
            crate::TypedPlaceKind::Dereference { reference } => {
                self.expression_roots(*reference, state)?
            }
        };
        Ok(roots
            .into_iter()
            .map(|root| state.account_for_escape(root))
            .collect())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum EffectBodyError {
    DuplicateSource(FileId),
    UnknownModule(ModuleId),
    MissingSource(FileId),
    MissingHir(FileId),
    MissingDeclarationItem(DefId),
    WrongDeclarationKind(DefId),
    WrongBodyExpression(ExprId),
    WrongBodyStatement(StmtId),
    PatternBinding(PatternBindingQueryError),
    MissingLocalType(LocalBindingId),
    UnknownCallableInstance(crate::CallableInstanceId),
    UnknownCallableSignature(CallableSignatureId),
    InvocationArityMismatch(ExpressionKey),
    NonCallableDispatch(CanonicalTarget),
    DiagnosticStore(DiagnosticStoreError),
    TypeStore(TypeStoreError),
}

impl fmt::Display for EffectBodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "effect body construction invariant failed: {self:?}"
        )
    }
}

impl std::error::Error for EffectBodyError {}

impl From<TypeStoreError> for EffectBodyError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

impl From<PatternBindingQueryError> for EffectBodyError {
    fn from(error: PatternBindingQueryError) -> Self {
        Self::PatternBinding(error)
    }
}

impl From<DiagnosticStoreError> for EffectBodyError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}
