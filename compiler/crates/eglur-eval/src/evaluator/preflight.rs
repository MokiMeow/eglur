use super::*;
use eglur_sema::{CanonicalTypeStore, TypeStoreError};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PlannedCall {
    declaration: DefId,
    witnesses: Box<[(WitnessSlotId, WitnessId)]>,
}

pub(super) struct Planner<'a> {
    view: ProgramView<'a>,
    support: &'a dyn ExecutionSupport,
    catalog: &'a dyn RuntimeCatalog,
    limits: EvaluationLimits,
    call_depth: u32,
    limit: Option<ToolLimit>,
    planned: BTreeSet<PlannedCall>,
    witness_store: WitnessStore,
}

impl<'a> Planner<'a> {
    pub(super) fn new(
        view: ProgramView<'a>,
        support: &'a dyn ExecutionSupport,
        catalog: &'a dyn RuntimeCatalog,
        limits: EvaluationLimits,
    ) -> Self {
        Self {
            view,
            support,
            catalog,
            limits,
            call_depth: 0,
            limit: None,
            planned: BTreeSet::new(),
            witness_store: WitnessStore::default(),
        }
    }

    pub(super) const fn limit(&self) -> Option<ToolLimit> {
        self.limit
    }

    fn require(&self, capability: ExecutionCapability) -> Result<(), SupportReason> {
        self.support
            .supports_execution(capability)
            .then_some(())
            .ok_or(SupportReason::MissingCapability(capability))
    }

    pub(super) fn plan_callable(
        &mut self,
        declaration: DefId,
        witnesses: &Witnesses,
    ) -> Result<(), SupportReason> {
        self.require(ExecutionCapability::Calls)?;
        let key = PlannedCall {
            declaration,
            witnesses: witnesses
                .iter()
                .map(|(slot, value)| (*slot, *value))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };
        if !self.planned.insert(key) {
            return Ok(());
        }
        if self.call_depth >= self.limits.max_call_depth() {
            self.limit = Some(ToolLimit::CallDepth {
                limit: self.limits.max_call_depth(),
            });
            return Ok(());
        }
        self.call_depth += 1;
        let result = (|| {
            let (_, signature, form) =
                self.view
                    .callable(declaration)
                    .ok_or(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    ))?;
            for parameter in signature.parameters() {
                if let Some(pattern) = parameter.source_pattern() {
                    self.plan_pattern(
                        declaration.module(),
                        signature.body().ok_or(SupportReason::Feature(
                            UnsupportedFeature::InvalidCheckedProgram,
                        ))?,
                        pattern,
                    )?;
                }
            }
            match form {
                FunctionBody::Expression(expression) | FunctionBody::Block(expression) => {
                    self.plan_expr(declaration.module(), expression, witnesses)
                }
                FunctionBody::Absent => Err(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                )),
            }
        })();
        self.call_depth -= 1;
        result
    }

    fn plan_expr(
        &mut self,
        module: ModuleId,
        expression: ExprId,
        witnesses: &Witnesses,
    ) -> Result<(), SupportReason> {
        self.require(ExecutionCapability::CoreValues)?;
        if let FactState::Known(typed) = self
            .view
            .program
            .resolution()
            .body_types()
            .expression(ExpressionKey::new(module, expression))
            && typed.integer_constant().is_some()
        {
            return Ok(());
        }
        let body = self
            .view
            .body(module, expression.body())
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?;
        let kind = body
            .expression(expression)
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?
            .kind()
            .clone();
        match kind {
            ExprKind::Block { statements } => {
                for statement in statements {
                    self.plan_stmt(module, statement, witnesses)?;
                }
                Ok(())
            }
            ExprKind::Binary { left, right, .. } => {
                self.plan_checked_numeric(module, expression)?;
                self.plan_expr(module, left, witnesses)?;
                self.plan_expr(module, right, witnesses)
            }
            ExprKind::Unary { operator, operand } => {
                match operator {
                    UnaryOperator::Dereference => {
                        self.require(ExecutionCapability::ReferencesAndMutation)?;
                    }
                    UnaryOperator::Not => {}
                    UnaryOperator::Negate => {
                        self.plan_checked_numeric(module, expression)?;
                    }
                }
                self.plan_expr(module, operand, witnesses)
            }
            ExprKind::Reference { value, .. } => {
                self.require(ExecutionCapability::ReferencesAndMutation)?;
                self.plan_expr(module, value, witnesses)
            }
            ExprKind::Call(_) | ExprKind::CatalogCall(_) => {
                self.plan_invocation(module, expression, witnesses)
            }
            ExprKind::Member { receiver, .. } => {
                let key = ExpressionKey::new(module, expression);
                let FactState::Known(member) =
                    self.view.program.resolution().body_types().member(key)
                else {
                    return Err(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    ));
                };
                if matches!(member.kind(), MemberKind::Property) {
                    return Err(SupportReason::Feature(UnsupportedFeature::PropertyRead));
                }
                self.require(ExecutionCapability::Aggregates)?;
                self.plan_expr(module, receiver, witnesses)
            }
            ExprKind::TupleProjection { receiver, .. } => {
                self.require(ExecutionCapability::Aggregates)?;
                self.plan_expr(module, receiver, witnesses)
            }
            ExprKind::Try(value) => {
                self.require(ExecutionCapability::FailureFlow)?;
                self.plan_expr(module, value, witnesses)
            }
            ExprKind::Literal(literal) => match literal.kind() {
                LiteralKind::Float | LiteralKind::AxisIndex | LiteralKind::AxisName => Err(
                    SupportReason::Feature(UnsupportedFeature::FloatOrComplexValue),
                ),
                LiteralKind::FormatString => {
                    Err(SupportReason::Feature(UnsupportedFeature::FormatString))
                }
                LiteralKind::Integer | LiteralKind::String | LiteralKind::Bool => Ok(()),
                LiteralKind::Invalid => Err(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                )),
            },
            ExprKind::Name(_) if self.view.invocation(module, expression).is_some() => {
                self.plan_invocation(module, expression, witnesses)
            }
            ExprKind::Name(_) => {
                let key = ResolutionKey::Expression { module, expression };
                match self.view.program.resolution().names().fact(key) {
                    FactState::Known(ResolvedName::Local(_)) => Ok(()),
                    FactState::Known(ResolvedName::Declarations(_)) => {
                        let key = ExpressionKey::new(module, expression);
                        if matches!(
                            self.view
                                .program
                                .resolution()
                                .body_types()
                                .callable_value(key),
                            FactState::Known(_)
                        ) {
                            return self.plan_callable_value(key, witnesses);
                        }
                        let FactState::Known(typed) =
                            self.view.program.resolution().body_types().expression(key)
                        else {
                            return Err(SupportReason::Feature(
                                UnsupportedFeature::InvalidCheckedProgram,
                            ));
                        };
                        let feature = match self
                            .view
                            .program
                            .resolution()
                            .signatures()
                            .types()
                            .kind(typed.ty())
                        {
                            Ok(TypeKind::Callable { .. }) => UnsupportedFeature::FunctionValue,
                            Ok(_) => UnsupportedFeature::NonlocalValue,
                            Err(_) => UnsupportedFeature::InvalidCheckedProgram,
                        };
                        Err(SupportReason::Feature(feature))
                    }
                    FactState::Known(ResolvedName::TypeMemberOwner(_))
                    | FactState::Poisoned(_)
                    | FactState::Unperformed
                    | FactState::NotApplicable => Err(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    )),
                }
            }
            ExprKind::Unit => Ok(()),
            ExprKind::Group(value) => self.plan_expr(module, value, witnesses),
            ExprKind::Tuple(values) => {
                self.require(ExecutionCapability::Aggregates)?;
                let key = ExpressionKey::new(module, expression);
                let FactState::Known(typed) =
                    self.view.program.resolution().body_types().expression(key)
                else {
                    return Err(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    ));
                };
                let supported = supports_local_tuple_type(
                    self.view.program.resolution().signatures().types(),
                    typed.ty(),
                )
                .map_err(|_| SupportReason::Feature(UnsupportedFeature::InvalidCheckedProgram))?;
                if !supported {
                    return Err(SupportReason::Feature(
                        UnsupportedFeature::ReferenceAggregate,
                    ));
                }
                for value in values {
                    self.plan_expr(module, value, witnesses)?;
                }
                Ok(())
            }
            ExprKind::StructLiteral { .. } | ExprKind::VariantConstructor { .. } => {
                self.require(ExecutionCapability::Aggregates)?;
                self.plan_invocation(module, expression, witnesses)
            }
            ExprKind::If(branches) => {
                self.require(ExecutionCapability::Branching)?;
                self.plan_condition(module, branches.condition, witnesses)?;
                self.plan_expr(module, branches.then_branch, witnesses)?;
                if let Some(branch) = branches.else_branch {
                    self.plan_expr(module, branch, witnesses)?;
                }
                if let Some(branch) = branches.unknown_branch {
                    self.plan_expr(module, branch, witnesses)?;
                }
                Ok(())
            }
            ExprKind::Match(matched) => {
                self.require(ExecutionCapability::Branching)?;
                self.plan_expr(module, matched.scrutinee, witnesses)?;
                for arm in matched.arms {
                    self.plan_pattern(module, expression.body(), arm.pattern)?;
                    if let Some(guard) = arm.guard {
                        self.plan_condition(module, guard.condition, witnesses)?;
                    }
                    self.plan_expr(module, arm.value, witnesses)?;
                }
                Ok(())
            }
            ExprKind::Result(inner) => {
                self.require(ExecutionCapability::FailureFlow)?;
                self.plan_expr(module, inner, witnesses)
            }
            ExprKind::Checked(inner) => {
                self.require(ExecutionCapability::CheckedIntegerArithmetic)?;
                self.plan_expr(module, inner, witnesses)
            }
            ExprKind::Own(_) | ExprKind::Move(_) | ExprKind::DropOwn { .. } => Err(
                SupportReason::Feature(UnsupportedFeature::OwnershipOperation),
            ),
            ExprKind::Lambda(_) => Err(SupportReason::Feature(UnsupportedFeature::Closure)),
            ExprKind::Array(_) | ExprKind::Map(_) => {
                Err(SupportReason::Feature(UnsupportedFeature::Container))
            }
            ExprKind::Unsafe(_) => Err(SupportReason::Feature(UnsupportedFeature::UnsafeOrPointer)),
            ExprKind::TensorSchema(_) | ExprKind::ReduceByCandidate(_) => {
                Err(SupportReason::Feature(UnsupportedFeature::Tensor))
            }
            ExprKind::DTypeCall(_) => Err(SupportReason::Feature(UnsupportedFeature::Tensor)),
            ExprKind::Hole | ExprKind::Invalid => Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            )),
        }
    }

    fn plan_checked_numeric(
        &self,
        module: ModuleId,
        expression: ExprId,
    ) -> Result<(), SupportReason> {
        let key = ExpressionKey::new(module, expression);
        let operation = match self
            .view
            .program
            .resolution()
            .body_types()
            .checked_numeric(key)
        {
            FactState::Known(operation) => operation,
            FactState::Unperformed => {
                return Err(SupportReason::Feature(
                    UnsupportedFeature::OrdinaryArithmetic,
                ));
            }
            FactState::Poisoned(_) | FactState::NotApplicable => {
                return Err(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ));
            }
        };
        let Some(result) = self.view.primitive_type(operation.result()) else {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        };
        if !result.is_fixed_width_integer() {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        }
        self.require(ExecutionCapability::CheckedIntegerArithmetic)
    }

    fn plan_condition(
        &mut self,
        module: ModuleId,
        condition: ExprId,
        witnesses: &Witnesses,
    ) -> Result<(), SupportReason> {
        let key = ExpressionKey::new(module, condition);
        let FactState::Known(typed) = self.view.program.resolution().body_types().expression(key)
        else {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        };
        match self
            .view
            .program
            .resolution()
            .signatures()
            .types()
            .kind(typed.ty())
        {
            Ok(TypeKind::Primitive(PrimitiveType::Bool) | TypeKind::Never) => {
                self.plan_expr(module, condition, witnesses)
            }
            Ok(_) => Err(SupportReason::Feature(
                UnsupportedFeature::TriStateCondition,
            )),
            Err(_) => Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            )),
        }
    }

    fn plan_stmt(
        &mut self,
        module: ModuleId,
        statement: StmtId,
        witnesses: &Witnesses,
    ) -> Result<(), SupportReason> {
        let body = self
            .view
            .body(module, statement.body())
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?;
        let kind = body
            .statement(statement)
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?
            .kind()
            .clone();
        match kind {
            StmtKind::Let {
                pattern,
                initializer,
                ..
            } => {
                self.plan_pattern(module, statement.body(), pattern)?;
                if let Some(initializer) = initializer {
                    self.plan_expr(module, initializer, witnesses)?;
                }
                Ok(())
            }
            StmtKind::Assign { target, value } => {
                let body = self
                    .view
                    .body(module, target.body())
                    .ok_or(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    ))?;
                if !matches!(
                    body.expression(target).map(eglur_hir::Expr::kind),
                    Some(ExprKind::Hole)
                ) {
                    self.require(ExecutionCapability::ReferencesAndMutation)?;
                    self.plan_expr(module, target, witnesses)?;
                }
                self.plan_expr(module, value, witnesses)
            }
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    self.plan_expr(module, value, witnesses)?;
                }
                Ok(())
            }
            StmtKind::Expression(value) | StmtKind::Yield(value) => {
                self.plan_expr(module, value, witnesses)
            }
            StmtKind::While { .. }
            | StmtKind::For { .. }
            | StmtKind::Break
            | StmtKind::Continue => Err(SupportReason::Feature(UnsupportedFeature::Loop)),
            StmtKind::Invalid => Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            )),
        }
    }

    fn plan_pattern(
        &self,
        module: ModuleId,
        body_id: eglur_hir::BodyId,
        pattern: PatternId,
    ) -> Result<(), SupportReason> {
        let body = self
            .view
            .body(module, body_id)
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?;
        let pattern = body.pattern(pattern).ok_or(SupportReason::Feature(
            UnsupportedFeature::InvalidCheckedProgram,
        ))?;
        match pattern.kind() {
            PatternKind::Binding(_)
            | PatternKind::Wildcard
            | PatternKind::Literal(_)
            | PatternKind::Unit => Ok(()),
            PatternKind::Alias { pattern, .. } | PatternKind::Group(pattern) => {
                self.plan_pattern(module, body_id, *pattern)
            }
            PatternKind::Or(patterns) => {
                for pattern in patterns {
                    self.plan_pattern(module, body_id, *pattern)?;
                }
                Ok(())
            }
            PatternKind::Tuple(elements) => {
                for element in elements {
                    let PatternElement::Pattern(pattern) = element else {
                        return Err(SupportReason::Feature(UnsupportedFeature::AdvancedPattern));
                    };
                    self.plan_pattern(module, body_id, *pattern)?;
                }
                Ok(())
            }
            PatternKind::Variant { payload, .. } => match payload {
                PatternPayload::Absent => Ok(()),
                PatternPayload::Positional(elements) => {
                    for element in elements {
                        let PatternElement::Pattern(pattern) = element else {
                            return Err(SupportReason::Feature(
                                UnsupportedFeature::AdvancedPattern,
                            ));
                        };
                        self.plan_pattern(module, body_id, *pattern)?;
                    }
                    Ok(())
                }
                PatternPayload::Named(_) => {
                    Err(SupportReason::Feature(UnsupportedFeature::AdvancedPattern))
                }
            },
            PatternKind::Struct { .. } => {
                Err(SupportReason::Feature(UnsupportedFeature::AdvancedPattern))
            }
            PatternKind::Invalid => Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            )),
        }
    }

    fn plan_invocation(
        &mut self,
        module: ModuleId,
        expression: ExprId,
        witnesses: &Witnesses,
    ) -> Result<(), SupportReason> {
        self.require(ExecutionCapability::Calls)?;
        let (invocation, checked) =
            self.view
                .invocation(module, expression)
                .ok_or(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ))?;
        if let CheckedInvocation::Indirect { callee, ty } = checked {
            if !matches!(invocation.target(), InvocationTarget::CallableValue { callee: found, ty: found_ty } if found == callee && found_ty == ty)
            {
                return Err(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ));
            }
            self.plan_expr(module, *callee, witnesses)?;
            for argument in invocation.evaluation_order() {
                if argument.passing() != ArgumentPassing::Value {
                    self.require(ExecutionCapability::ReferencesAndMutation)?;
                }
                self.plan_expr(module, argument.expression(), witnesses)?;
            }
            return Ok(());
        }
        let CheckedInvocation::Static {
            dispatch,
            evidence_environment,
        } = checked
        else {
            unreachable!()
        };
        for argument in invocation.evaluation_order() {
            if argument.passing() != ArgumentPassing::Value {
                self.require(ExecutionCapability::ReferencesAndMutation)?;
            }
            self.plan_expr(module, argument.expression(), witnesses)?;
        }
        let evidence = self.view.trait_evidence().ok_or(SupportReason::Feature(
            UnsupportedFeature::InvalidCheckedProgram,
        ))?;
        let environment =
            evidence
                .environment(*evidence_environment)
                .ok_or(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ))?;
        let child = self
            .witness_store
            .child_witnesses(evidence, environment, witnesses)
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?;
        match invocation.target() {
            InvocationTarget::CallableValue { .. } => Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            )),
            InvocationTarget::TensorConstructor { .. } => {
                Err(SupportReason::Feature(UnsupportedFeature::Tensor))
            }
            InvocationTarget::Constructor { declaration, .. } => {
                self.plan_public_arguments(module, invocation.arguments())?;
                self.plan_constructor(declaration, dispatch)
            }
            InvocationTarget::Callable(_) => {
                let selected = self
                    .view
                    .selected_call(dispatch, witnesses, &mut self.witness_store)
                    .ok_or(SupportReason::Feature(
                        UnsupportedFeature::InvalidCheckedProgram,
                    ))?;
                let adapter = self
                    .view
                    .trait_adapter_dispatch(
                        &selected.target,
                        module,
                        invocation.arguments(),
                        environment,
                        witnesses,
                        &mut self.witness_store,
                    )
                    .map_err(|_| {
                        SupportReason::Feature(UnsupportedFeature::InvalidCheckedProgram)
                    })?;
                let selected = if let Some(adapter) = adapter {
                    if adapter.borrow_argument.is_some() {
                        self.require(ExecutionCapability::ReferencesAndMutation)?;
                    }
                    adapter.selected
                } else {
                    selected
                };
                match &selected.target {
                    CanonicalTarget::Source(declaration) => {
                        let frame = if selected.method.is_some() {
                            self.witness_store
                                .method_frame(evidence, &selected, environment, witnesses)
                                .ok_or(SupportReason::Feature(
                                    UnsupportedFeature::InvalidCheckedProgram,
                                ))?
                        } else {
                            child
                        };
                        self.plan_callable(*declaration, &frame)
                    }
                    CanonicalTarget::External(declaration) => {
                        let hook = self.plan_hook(declaration)?;
                        self.plan_hook_arguments(hook, declaration, module, invocation.arguments())
                    }
                }
            }
        }
    }

    fn plan_callable_value(
        &mut self,
        key: ExpressionKey,
        witnesses: &Witnesses,
    ) -> Result<(), SupportReason> {
        self.require(ExecutionCapability::Calls)?;
        let evidence = self.view.trait_evidence().ok_or(SupportReason::Feature(
            UnsupportedFeature::InvalidCheckedProgram,
        ))?;
        let FactState::Known(checked) = evidence.callable_value(key) else {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        };
        let environment =
            evidence
                .environment(checked.evidence_environment())
                .ok_or(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ))?;
        let selected = self
            .view
            .selected_call(checked.dispatch(), witnesses, &mut self.witness_store)
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?;
        match &selected.target {
            CanonicalTarget::Source(declaration) => {
                let frame = if selected.method.is_some() {
                    self.witness_store
                        .method_frame(evidence, &selected, environment, witnesses)
                        .ok_or(SupportReason::Feature(
                            UnsupportedFeature::InvalidCheckedProgram,
                        ))?
                } else {
                    self.witness_store
                        .child_witnesses(evidence, environment, witnesses)
                        .ok_or(SupportReason::Feature(
                            UnsupportedFeature::InvalidCheckedProgram,
                        ))?
                };
                self.plan_callable(*declaration, &frame)
            }
            CanonicalTarget::External(declaration) => {
                let signature = self.view.callable_signature(&selected.target).ok_or(
                    SupportReason::Feature(UnsupportedFeature::InvalidCheckedProgram),
                )?;
                let types = self.view.program.resolution().signatures().types();
                for parameter in signature.parameters() {
                    if has_direct_callable_payload(types, parameter.ty()).map_err(|_| {
                        SupportReason::Feature(UnsupportedFeature::InvalidCheckedProgram)
                    })? {
                        return Err(SupportReason::Feature(UnsupportedFeature::FunctionValue));
                    }
                }
                self.plan_hook(declaration).map(|_| ())
            }
        }
    }

    fn plan_constructor(
        &self,
        declaration: &CanonicalTarget,
        dispatch: &CheckedDispatch,
    ) -> Result<(), SupportReason> {
        if !matches!(dispatch, CheckedDispatch::Direct(found) if found == declaration) {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        }
        match declaration {
            CanonicalTarget::Source(_) => self.require(ExecutionCapability::Aggregates),
            CanonicalTarget::External(declaration) => self.plan_hook(declaration).map(|_| ()),
        }
    }

    fn plan_hook(&self, declaration: &ExternalDefId) -> Result<RuntimeHook, SupportReason> {
        let hook = self
            .catalog
            .runtime_hook(declaration)
            .ok_or_else(|| SupportReason::MissingRuntimeHook(declaration.clone()))?;
        self.require(hook.capability())?;
        Ok(hook)
    }

    fn plan_hook_arguments(
        &self,
        hook: RuntimeHook,
        declaration: &ExternalDefId,
        module: ModuleId,
        arguments: &[InvocationArgument],
    ) -> Result<(), SupportReason> {
        self.plan_public_arguments(module, arguments)?;
        if hook != RuntimeHook::ToStr {
            return Ok(());
        }
        if arguments.len() != 1 {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        }
        let signature = self
            .view
            .callable_signature(&CanonicalTarget::External(declaration.clone()))
            .ok_or(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ))?;
        let [parameter] = signature.parameters() else {
            return Err(SupportReason::Feature(
                UnsupportedFeature::InvalidCheckedProgram,
            ));
        };
        let types = self.view.program.resolution().signatures().types();
        let subject = match types.kind(parameter.ty()) {
            Ok(TypeKind::Reference { referent, .. }) => *referent,
            Ok(_) => parameter.ty(),
            Err(_) => {
                return Err(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ));
            }
        };
        if matches!(
            types.kind(subject),
            Ok(TypeKind::Primitive(
                PrimitiveType::Bool
                    | PrimitiveType::I8
                    | PrimitiveType::I16
                    | PrimitiveType::I32
                    | PrimitiveType::I64
                    | PrimitiveType::U8
                    | PrimitiveType::U16
                    | PrimitiveType::U32
                    | PrimitiveType::U64
                    | PrimitiveType::Nat
                    | PrimitiveType::PositiveNat
                    | PrimitiveType::Str
            ))
        ) {
            Ok(())
        } else {
            Err(SupportReason::Feature(UnsupportedFeature::HookArgumentType))
        }
    }

    fn plan_public_arguments(
        &self,
        module: ModuleId,
        arguments: &[InvocationArgument],
    ) -> Result<(), SupportReason> {
        let resolution = self.view.program.resolution();
        let types = resolution.signatures().types();
        for argument in arguments {
            let FactState::Known(typed) = resolution
                .body_types()
                .expression(ExpressionKey::new(module, argument.expression()))
            else {
                return Err(SupportReason::Feature(
                    UnsupportedFeature::InvalidCheckedProgram,
                ));
            };
            if has_direct_callable_payload(types, typed.ty())
                .map_err(|_| SupportReason::Feature(UnsupportedFeature::InvalidCheckedProgram))?
            {
                return Err(SupportReason::Feature(UnsupportedFeature::FunctionValue));
            }
        }
        Ok(())
    }
}

fn has_direct_callable_payload(
    types: &CanonicalTypeStore,
    root: TypeId,
) -> Result<bool, TypeStoreError> {
    let mut pending = vec![root];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        match types.kind(current)? {
            TypeKind::Callable { .. } => return Ok(true),
            TypeKind::Tuple(elements) | TypeKind::ErrorUnion(elements) => {
                pending.extend(elements.iter().copied());
            }
            TypeKind::Reference { referent, .. } | TypeKind::RawPointer { referent, .. } => {
                pending.push(*referent);
            }
            // Nominal type arguments are not necessarily stored; constructors check their fields.
            TypeKind::Never
            | TypeKind::Primitive(_)
            | TypeKind::Nominal { .. }
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_)
            | TypeKind::AssociatedProjection(_) => {}
        }
    }
    Ok(false)
}

fn supports_local_tuple_type(
    types: &CanonicalTypeStore,
    ty: TypeId,
) -> Result<bool, TypeStoreError> {
    let mut pending = vec![ty];
    let mut visited = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !visited.insert(current) {
            continue;
        }
        match types.kind(current)? {
            TypeKind::Reference { .. } => {}
            TypeKind::Tuple(elements) => pending.extend(elements.iter().copied()),
            TypeKind::Never
            | TypeKind::Primitive(_)
            | TypeKind::Nominal { .. }
            | TypeKind::GenericParameter(_)
            | TypeKind::SelfType(_)
            | TypeKind::RawPointer { .. }
            | TypeKind::Callable { .. }
            | TypeKind::ErrorUnion(_)
            | TypeKind::AssociatedProjection(_) => {
                if types.contains_reference(current)? {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use eglur_sema::{
        CanonicalTypeStore, IntrinsicNominal, NominalHead, PrimitiveType, ReferenceMutability,
    };

    use super::{has_direct_callable_payload, supports_local_tuple_type};

    #[test]
    fn reference_tuple_shape_is_closed_over_references_and_tuples_only() {
        let mut types = CanonicalTypeStore::new().unwrap();
        let integer = types.primitive(PrimitiveType::I64).unwrap();
        let reference = types
            .reference(ReferenceMutability::Mutable, integer)
            .unwrap();
        let nested = types.tuple([reference]).unwrap();
        let supported = types.tuple([integer, reference, nested]).unwrap();
        assert!(supports_local_tuple_type(&types, supported).unwrap());

        let callable = types.callable([reference], integer).unwrap();
        let unsupported = types.tuple([reference, callable]).unwrap();
        assert!(!supports_local_tuple_type(&types, unsupported).unwrap());
    }

    #[test]
    fn direct_callable_payload_check_does_not_assume_nominal_type_arguments_are_stored() {
        let mut types = CanonicalTypeStore::new().unwrap();
        let string = types.primitive(PrimitiveType::Str).unwrap();
        let callable = types.callable([string], string).unwrap();
        let tuple = types.tuple([string, callable]).unwrap();
        let optional = types
            .nominal(NominalHead::Intrinsic(IntrinsicNominal::Opt), [callable])
            .unwrap();

        assert!(has_direct_callable_payload(&types, callable).unwrap());
        assert!(has_direct_callable_payload(&types, tuple).unwrap());
        assert!(!has_direct_callable_payload(&types, optional).unwrap());
    }
}
