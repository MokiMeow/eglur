use super::*;

struct Frame {
    module: ModuleId,
    body: eglur_hir::BodyId,
    locals: BTreeMap<LocalBindingId, usize>,
    witnesses: Witnesses,
}

pub(super) enum Flow {
    Value(RuntimeValue),
    Yield(RuntimeValue),
    Return(RuntimeValue),
    ShortCircuit(RuntimeValue),
    NumericFault(RuntimeDiagnostic),
    RuntimeAbort(RuntimeDiagnostic),
    Limit(ToolLimit),
    InvalidCheckedProgram,
}

pub(super) struct Evaluator<'a> {
    view: ProgramView<'a>,
    catalog: &'a dyn RuntimeCatalog,
    limits: EvaluationLimits,
    steps: u64,
    call_depth: u32,
    store: Store,
    pub(super) events: Vec<RuntimeEvent>,
}

impl<'a> Evaluator<'a> {
    pub(super) fn new(
        view: ProgramView<'a>,
        catalog: &'a dyn RuntimeCatalog,
        limits: EvaluationLimits,
    ) -> Self {
        Self {
            view,
            catalog,
            limits,
            steps: 0,
            call_depth: 0,
            store: Store::default(),
            events: Vec::new(),
        }
    }

    fn tick(&mut self) -> Result<(), ToolLimit> {
        if self.steps >= self.limits.max_steps {
            return Err(ToolLimit::Steps {
                limit: self.limits.max_steps,
            });
        }
        self.steps += 1;
        Ok(())
    }

    pub(super) fn call_source(
        &mut self,
        declaration: DefId,
        arguments: Vec<RuntimeValue>,
        witnesses: Witnesses,
    ) -> Flow {
        if self.call_depth >= self.limits.max_call_depth {
            return Flow::Limit(ToolLimit::CallDepth {
                limit: self.limits.max_call_depth,
            });
        }
        if let Err(limit) = self.tick() {
            return Flow::Limit(limit);
        }
        self.call_depth += 1;
        let result = self.call_source_inner(declaration, arguments, witnesses);
        self.call_depth -= 1;
        match result {
            Flow::Return(value) => Flow::Value(value),
            other => other,
        }
    }

    pub(super) fn call_source_inner(
        &mut self,
        declaration: DefId,
        arguments: Vec<RuntimeValue>,
        witnesses: Witnesses,
    ) -> Flow {
        let Some((_, signature, form)) = self.view.callable(declaration) else {
            return invalid_flow();
        };
        let Some(body_id) = signature.body() else {
            return invalid_flow();
        };
        if signature.parameters().len() != arguments.len() {
            return invalid_flow();
        }
        let mut frame = Frame {
            module: declaration.module(),
            body: body_id,
            locals: BTreeMap::new(),
            witnesses,
        };
        for (parameter, value) in signature.parameters().iter().zip(arguments) {
            let Some(pattern) = parameter.source_pattern() else {
                return invalid_flow();
            };
            if !self.bind_pattern(&mut frame, pattern, value) {
                return invalid_flow();
            }
        }
        match form {
            FunctionBody::Expression(expression) | FunctionBody::Block(expression) => {
                self.eval_expr(&mut frame, expression)
            }
            FunctionBody::Absent => invalid_flow(),
        }
    }

    fn eval_expr(&mut self, frame: &mut Frame, expression: ExprId) -> Flow {
        if let Err(limit) = self.tick() {
            return Flow::Limit(limit);
        }
        let Some(body) = self.view.body(frame.module, expression.body()) else {
            return invalid_flow();
        };
        if let FactState::Known(typed) = self
            .view
            .program
            .resolution()
            .body_types()
            .expression(ExpressionKey::new(frame.module, expression))
            && let Some(value) = typed.integer_constant()
        {
            return Flow::Value(RuntimeValue::Value(Value::Integer(value)));
        }
        let Some(expression_node) = body.expression(expression) else {
            return invalid_flow();
        };
        let kind = expression_node.kind().clone();
        match kind {
            ExprKind::Block { statements } => {
                for statement in statements {
                    match self.eval_stmt(frame, statement) {
                        Flow::Value(_) => {}
                        Flow::Yield(value) => return Flow::Value(value),
                        other => return other,
                    }
                }
                Flow::Value(RuntimeValue::Value(Value::Unit))
            }
            ExprKind::Literal(literal) => self.eval_literal(frame.module, expression, &literal),
            ExprKind::Unit => Flow::Value(RuntimeValue::Value(Value::Unit)),
            ExprKind::Name(_) if self.view.invocation(frame.module, expression).is_some() => {
                self.eval_invocation(frame, expression)
            }
            ExprKind::Name(_) => self.eval_name(frame, expression),
            ExprKind::Group(inner) | ExprKind::Unsafe(inner) => self.eval_expr(frame, inner),
            ExprKind::Tuple(values) => {
                let mut evaluated = Vec::with_capacity(values.len());
                for value in values {
                    evaluated.push(flow_value!(self.eval_expr(frame, value)));
                }
                Flow::Value(RuntimeValue::tuple(evaluated))
            }
            ExprKind::Reference { value, .. } => match self.resolve_place(frame, value) {
                Ok(Some(place)) => Flow::Value(RuntimeValue::Reference(place)),
                Ok(None) => invalid_flow(),
                Err(flow) => flow,
            },
            ExprKind::Member { receiver, .. } => self.eval_member(frame, expression, receiver),
            ExprKind::TupleProjection { receiver, index } => {
                let Ok(index) = index.parse::<usize>() else {
                    return invalid_flow();
                };
                match self.resolve_place(frame, expression) {
                    Ok(Some(place)) => self
                        .store
                        .read(&place)
                        .map_or_else(invalid_flow, Flow::Value),
                    Ok(None) => flow_value!(self.eval_expr(frame, receiver))
                        .tuple_element(index)
                        .map(Flow::Value)
                        .unwrap_or_else(invalid_flow),
                    Err(flow) => flow,
                }
            }
            ExprKind::Call(_)
            | ExprKind::CatalogCall(_)
            | ExprKind::StructLiteral { .. }
            | ExprKind::VariantConstructor { .. } => self.eval_invocation(frame, expression),
            ExprKind::If(branches) => {
                let RuntimeValue::Value(Value::Bool(condition)) =
                    flow_value!(self.eval_expr(frame, branches.condition))
                else {
                    return invalid_flow();
                };
                let branch = if condition {
                    Some(branches.then_branch)
                } else {
                    branches.else_branch
                };
                branch.map_or_else(
                    || Flow::Value(RuntimeValue::Value(Value::Unit)),
                    |branch| self.eval_expr(frame, branch),
                )
            }
            ExprKind::Match(matched) => {
                let scrutinee = flow_value!(self.eval_expr(frame, matched.scrutinee));
                for arm in matched.arms {
                    let mut bindings = Vec::new();
                    if !self.match_pattern(frame, arm.pattern, &scrutinee, &mut bindings) {
                        continue;
                    }
                    for (binding, value) in bindings {
                        self.bind_local(frame, binding, value);
                    }
                    if let Some(guard) = arm.guard {
                        let RuntimeValue::Value(Value::Bool(matches)) =
                            flow_value!(self.eval_expr(frame, guard.condition))
                        else {
                            return invalid_flow();
                        };
                        if !matches {
                            continue;
                        }
                    }
                    return self.eval_expr(frame, arm.value);
                }
                invalid_flow()
            }
            ExprKind::Try(value) => {
                let RuntimeValue::Value(value) = flow_value!(self.eval_expr(frame, value)) else {
                    return invalid_flow();
                };
                let items = self.view.program.resolution().signatures().language_items();
                let Some(declaration) = value.variant_declaration() else {
                    return invalid_flow();
                };
                if Some(declaration) == items.declaration(LanguageItem::ResOk)
                    || Some(declaration) == items.declaration(LanguageItem::OptSome)
                {
                    let Some(payload) = value.variant_payload().and_then(|values| values.first())
                    else {
                        return invalid_flow();
                    };
                    Flow::Value(RuntimeValue::Value(payload.clone()))
                } else if Some(declaration) == items.declaration(LanguageItem::ResErr)
                    || Some(declaration) == items.declaration(LanguageItem::OptNone)
                {
                    Flow::ShortCircuit(RuntimeValue::Value(value))
                } else {
                    invalid_flow()
                }
            }
            ExprKind::Result(inner) => match self.eval_expr(frame, inner) {
                Flow::ShortCircuit(value) => Flow::Value(value),
                other => other,
            },
            ExprKind::Checked(inner) => match self.eval_expr(frame, inner) {
                Flow::NumericFault(diagnostic) => {
                    self.failure_variant(LanguageItem::ResErr, Value::NumericError(diagnostic))
                }
                Flow::Value(RuntimeValue::Value(value)) => {
                    self.failure_variant(LanguageItem::ResOk, value)
                }
                other => other,
            },
            ExprKind::Binary {
                operator,
                left,
                right,
            } => self.eval_binary(frame, expression, operator, left, right),
            ExprKind::Unary { operator, operand } => {
                self.eval_unary(frame, expression, operator, operand)
            }
            ExprKind::Own(_)
            | ExprKind::Move(_)
            | ExprKind::DropOwn { .. }
            | ExprKind::DTypeCall(_)
            | ExprKind::ReduceByCandidate(_)
            | ExprKind::Hole
            | ExprKind::Array(_)
            | ExprKind::Map(_)
            | ExprKind::Lambda(_)
            | ExprKind::TensorSchema(_)
            | ExprKind::Invalid => invalid_flow(),
        }
    }

    fn eval_stmt(&mut self, frame: &mut Frame, statement: StmtId) -> Flow {
        if let Err(limit) = self.tick() {
            return Flow::Limit(limit);
        }
        let Some(body) = self.view.body(frame.module, statement.body()) else {
            return invalid_flow();
        };
        let Some(statement_node) = body.statement(statement) else {
            return invalid_flow();
        };
        match statement_node.kind().clone() {
            StmtKind::Let {
                pattern,
                initializer,
                ..
            } => {
                let value = if let Some(initializer) = initializer {
                    flow_value!(self.eval_expr(frame, initializer))
                } else {
                    RuntimeValue::Value(Value::Unit)
                };
                if self.bind_pattern(frame, pattern, value) {
                    Flow::Value(RuntimeValue::Value(Value::Unit))
                } else {
                    invalid_flow()
                }
            }
            StmtKind::Assign { target, value } => {
                let Some(body) = self.view.body(frame.module, target.body()) else {
                    return invalid_flow();
                };
                if matches!(
                    body.expression(target).map(eglur_hir::Expr::kind),
                    Some(ExprKind::Hole)
                ) {
                    let _ = flow_value!(self.eval_expr(frame, value));
                    return Flow::Value(RuntimeValue::Value(Value::Unit));
                }
                let place = match self.resolve_place(frame, target) {
                    Ok(Some(place)) => place,
                    Ok(None) => return invalid_flow(),
                    Err(flow) => return flow,
                };
                let value = flow_value!(self.eval_expr(frame, value));
                self.store
                    .write(&place, value)
                    .map(|()| Flow::Value(RuntimeValue::Value(Value::Unit)))
                    .unwrap_or_else(invalid_flow)
            }
            StmtKind::Return(value) => {
                let value = match value {
                    Some(value) => match self.eval_expr(frame, value) {
                        Flow::Value(value) => value,
                        other => return other,
                    },
                    None => RuntimeValue::Value(Value::Unit),
                };
                Flow::Return(value)
            }
            StmtKind::Expression(value) => self.eval_expr(frame, value),
            StmtKind::Yield(value) => {
                let value = flow_value!(self.eval_expr(frame, value));
                Flow::Yield(value)
            }
            StmtKind::While { .. }
            | StmtKind::For { .. }
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Invalid => invalid_flow(),
        }
    }

    fn eval_name(&self, frame: &Frame, expression: ExprId) -> Flow {
        let key = ResolutionKey::Expression {
            module: frame.module,
            expression,
        };
        match self.view.program.resolution().names().fact(key) {
            FactState::Known(ResolvedName::Local(binding)) => {
                let Some(slot) = frame.locals.get(binding) else {
                    return invalid_flow();
                };
                self.store
                    .slots
                    .get(*slot)
                    .cloned()
                    .map(Flow::Value)
                    .unwrap_or_else(invalid_flow)
            }
            _ => invalid_flow(),
        }
    }

    fn eval_literal(&self, module: ModuleId, expression: ExprId, literal: &Literal) -> Flow {
        let value = match literal.kind() {
            LiteralKind::Bool => Value::Bool(literal.spelling() == "true"),
            LiteralKind::String => match decode_string_literal(literal.spelling()) {
                Some(value) => Value::Str(value.into_boxed_str()),
                None => return invalid_flow(),
            },
            LiteralKind::Integer => {
                let FactState::Known(typed) = self
                    .view
                    .program
                    .resolution()
                    .body_types()
                    .expression(ExpressionKey::new(module, expression))
                else {
                    return invalid_flow();
                };
                let Some(value) = typed.integer_constant() else {
                    return invalid_flow();
                };
                Value::Integer(value)
            }
            LiteralKind::Float
            | LiteralKind::FormatString
            | LiteralKind::AxisName
            | LiteralKind::AxisIndex
            | LiteralKind::Invalid => return invalid_flow(),
        };
        Flow::Value(RuntimeValue::Value(value))
    }

    fn eval_member(&mut self, frame: &mut Frame, expression: ExprId, receiver: ExprId) -> Flow {
        let key = ExpressionKey::new(frame.module, expression);
        let FactState::Known(member) = self.view.program.resolution().body_types().member(key)
        else {
            return invalid_flow();
        };
        if !matches!(member.kind(), MemberKind::StoredField { .. }) {
            return invalid_flow();
        }
        match self.resolve_place(frame, expression) {
            Ok(Some(place)) => {
                return self
                    .store
                    .read(&place)
                    .map_or_else(invalid_flow, Flow::Value);
            }
            Ok(None) => {}
            Err(flow) => return flow,
        }
        let RuntimeValue::Value(Value::Struct { fields, .. }) =
            flow_value!(self.eval_expr(frame, receiver))
        else {
            return invalid_flow();
        };
        fields
            .iter()
            .find(|(field, _)| field == member.declaration())
            .map(|(_, value)| Flow::Value(RuntimeValue::Value(value.clone())))
            .unwrap_or_else(invalid_flow)
    }

    fn eval_invocation(&mut self, frame: &mut Frame, expression: ExprId) -> Flow {
        let Some((invocation, checked)) = self.view.invocation(frame.module, expression) else {
            return invalid_flow();
        };
        let mut evaluated = BTreeMap::new();
        for argument in invocation.evaluation_order() {
            let value = flow_value!(self.eval_argument(frame, *argument));
            if evaluated.insert(argument.expression(), value).is_some() {
                return invalid_flow();
            }
        }
        let mut arguments = Vec::with_capacity(invocation.arguments().len());
        for argument in invocation.arguments() {
            let Some(value) = evaluated.remove(&argument.expression()) else {
                return invalid_flow();
            };
            arguments.push(value);
        }
        if !evaluated.is_empty() {
            return invalid_flow();
        }
        let Some(evidence) = self.view.trait_evidence() else {
            return invalid_flow();
        };
        let Some(environment) = evidence.environment(checked.evidence_environment()) else {
            return invalid_flow();
        };
        let Some(child) = child_witnesses(environment, &frame.witnesses) else {
            return invalid_flow();
        };
        match invocation.target() {
            InvocationTarget::TensorConstructor { .. } => invalid_flow(),
            InvocationTarget::Constructor { declaration, .. } => {
                self.eval_constructor(declaration, checked.dispatch(), arguments)
            }
            InvocationTarget::Callable(_) => {
                let Some(direct_target) =
                    self.dispatch_target(checked.dispatch(), &frame.witnesses)
                else {
                    return invalid_flow();
                };
                let target = match self.view.trait_adapter_dispatch(
                    &direct_target,
                    frame.module,
                    invocation.arguments(),
                    environment,
                    &frame.witnesses,
                ) {
                    Ok(Some(adapter)) => {
                        if let Some(index) = adapter.borrow_argument {
                            let Some(argument) = arguments.get_mut(index) else {
                                return invalid_flow();
                            };
                            let value =
                                std::mem::replace(argument, RuntimeValue::Value(Value::Unit));
                            *argument = self.store.borrow_temporary(value);
                        }
                        adapter.target
                    }
                    Ok(None) => direct_target,
                    Err(_) => return invalid_flow(),
                };
                match target {
                    CanonicalTarget::Source(declaration) => {
                        self.call_source(declaration, arguments, child)
                    }
                    CanonicalTarget::External(declaration) => self.eval_hook(
                        &declaration,
                        CanonicalTarget::External(declaration.clone()),
                        arguments,
                    ),
                }
            }
        }
    }

    fn eval_argument(&mut self, frame: &mut Frame, argument: InvocationArgument) -> Flow {
        match argument.passing() {
            ArgumentPassing::Value => self.eval_expr(frame, argument.expression()),
            ArgumentPassing::BorrowPlace(_) => {
                match self.resolve_place(frame, argument.expression()) {
                    Ok(Some(place)) => Flow::Value(RuntimeValue::Reference(place)),
                    Ok(None) => invalid_flow(),
                    Err(flow) => flow,
                }
            }
            ArgumentPassing::BorrowTemporary(_) => {
                let value = flow_value!(self.eval_expr(frame, argument.expression()));
                Flow::Value(self.store.borrow_temporary(value))
            }
        }
    }

    fn eval_constructor(
        &mut self,
        declaration: &CanonicalTarget,
        dispatch: &CheckedDispatch,
        arguments: Vec<RuntimeValue>,
    ) -> Flow {
        if !matches!(dispatch, CheckedDispatch::Direct(found) if found == declaration) {
            return invalid_flow();
        }
        let mut values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let Some(value) = argument.into_public() else {
                return invalid_flow();
            };
            values.push(value);
        }
        match declaration {
            CanonicalTarget::External(external) => self.eval_hook(
                external,
                declaration.clone(),
                values.into_iter().map(RuntimeValue::Value).collect(),
            ),
            CanonicalTarget::Source(_) => {
                let signatures = self.view.program.resolution().signatures();
                match signatures.declaration(declaration) {
                    FactState::Known(DeclarationSignature::Struct { storage_fields, .. }) => {
                        if storage_fields.len() != values.len() {
                            return invalid_flow();
                        }
                        Flow::Value(RuntimeValue::Value(Value::Struct {
                            declaration: declaration.clone(),
                            fields: storage_fields
                                .iter()
                                .cloned()
                                .zip(values)
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        }))
                    }
                    FactState::Known(DeclarationSignature::EnumVariant { .. }) => {
                        Flow::Value(RuntimeValue::Value(Value::Variant {
                            declaration: declaration.clone(),
                            payload: values.into_boxed_slice(),
                        }))
                    }
                    _ => invalid_flow(),
                }
            }
        }
    }

    fn dispatch_target(
        &self,
        dispatch: &CheckedDispatch,
        witnesses: &Witnesses,
    ) -> Option<CanonicalTarget> {
        match dispatch {
            CheckedDispatch::Direct(target) => Some(target.clone()),
            CheckedDispatch::ImplTemplate { callable, .. } => Some(callable.clone()),
            CheckedDispatch::Bound { witness, method } => {
                let evidence = witnesses.get(witness)?;
                self.view
                    .program
                    .resolution()
                    .trait_environment()
                    .checked_impl_method(&evidence.implementation, method)
                    .cloned()
            }
        }
    }

    fn eval_hook(
        &mut self,
        external: &ExternalDefId,
        declaration: CanonicalTarget,
        arguments: Vec<RuntimeValue>,
    ) -> Flow {
        if let Err(limit) = self.tick() {
            return Flow::Limit(limit);
        }
        let Some(hook) = self.catalog.runtime_hook(external) else {
            return invalid_flow();
        };
        let mut values = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let argument = match (hook, argument) {
                (RuntimeHook::ToStr, RuntimeValue::Reference(place)) => {
                    let Some(value) = self.store.read(&place) else {
                        return invalid_flow();
                    };
                    value
                }
                (_, value) => value,
            };
            let Some(value) = argument.into_public() else {
                return invalid_flow();
            };
            values.push(value);
        }
        match hook {
            RuntimeHook::OptSome | RuntimeHook::ResOk | RuntimeHook::ResErr => {
                if values.len() != 1 {
                    return invalid_flow();
                }
                Flow::Value(RuntimeValue::Value(Value::Variant {
                    declaration,
                    payload: values.into_boxed_slice(),
                }))
            }
            RuntimeHook::OptNone => {
                if !values.is_empty() {
                    return invalid_flow();
                }
                Flow::Value(RuntimeValue::Value(Value::Variant {
                    declaration,
                    payload: Box::new([]),
                }))
            }
            RuntimeHook::ToStr => match values.as_slice() {
                [value] => match display_value(value) {
                    Some(value) => {
                        Flow::Value(RuntimeValue::Value(Value::Str(value.into_boxed_str())))
                    }
                    None => invalid_flow(),
                },
                _ => invalid_flow(),
            },
            RuntimeHook::WrappingAdd | RuntimeHook::WrappingSub | RuntimeHook::WrappingMul => {
                match values.as_slice() {
                    [Value::Integer(left), Value::Integer(right)]
                        if left.primitive() == right.primitive() =>
                    {
                        match hook {
                            RuntimeHook::WrappingAdd => left.wrapping_add(*right),
                            RuntimeHook::WrappingSub => left.wrapping_sub(*right),
                            RuntimeHook::WrappingMul => left.wrapping_mul(*right),
                            _ => unreachable!(),
                        }
                        .map(|value| Flow::Value(RuntimeValue::Value(Value::Integer(value))))
                        .unwrap_or_else(invalid_flow)
                    }
                    _ => invalid_flow(),
                }
            }
            RuntimeHook::TestEmit => match values.as_slice() {
                [Value::Str(message)] => {
                    self.events.push(RuntimeEvent::Emitted(message.clone()));
                    Flow::Value(RuntimeValue::Value(Value::Unit))
                }
                _ => invalid_flow(),
            },
            RuntimeHook::TestAbort => match values.as_slice() {
                [Value::Str(code), Value::Str(message)] => {
                    Flow::RuntimeAbort(RuntimeDiagnostic::new(
                        code.clone(),
                        message.clone(),
                        [(
                            Box::<str>::from("source"),
                            Box::<str>::from("std::test::abort!"),
                        )],
                    ))
                }
                _ => invalid_flow(),
            },
        }
    }

    fn failure_variant(&self, item: LanguageItem, payload: Value) -> Flow {
        let Some(declaration) = self
            .view
            .program
            .resolution()
            .signatures()
            .language_items()
            .declaration(item)
            .cloned()
        else {
            return invalid_flow();
        };
        Flow::Value(RuntimeValue::Value(Value::Variant {
            declaration,
            payload: Box::new([payload]),
        }))
    }

    fn eval_binary(
        &mut self,
        frame: &mut Frame,
        expression: ExprId,
        _operator: BinaryOperator,
        left: ExprId,
        right: ExprId,
    ) -> Flow {
        let RuntimeValue::Value(Value::Integer(left)) = flow_value!(self.eval_expr(frame, left))
        else {
            return invalid_flow();
        };
        let RuntimeValue::Value(Value::Integer(right)) = flow_value!(self.eval_expr(frame, right))
        else {
            return invalid_flow();
        };
        let key = ExpressionKey::new(frame.module, expression);
        let FactState::Known(operation) = self
            .view
            .program
            .resolution()
            .body_types()
            .checked_numeric(key)
        else {
            return invalid_flow();
        };
        let Some(destination) = self.view.primitive_type(operation.result()) else {
            return invalid_flow();
        };
        match left.checked(operation.operator(), destination, Some(right)) {
            IntegerEvaluation::Value(value) => {
                Flow::Value(RuntimeValue::Value(Value::Integer(value)))
            }
            IntegerEvaluation::Fault(fault) => {
                Flow::NumericFault(numeric_fault(fault, operation.operator(), destination))
            }
            IntegerEvaluation::Invalid => invalid_flow(),
        }
    }

    fn eval_unary(
        &mut self,
        frame: &mut Frame,
        expression: ExprId,
        operator: UnaryOperator,
        operand: ExprId,
    ) -> Flow {
        if operator == UnaryOperator::Dereference {
            let RuntimeValue::Reference(place) = flow_value!(self.eval_expr(frame, operand)) else {
                return invalid_flow();
            };
            return self
                .store
                .read(&place)
                .map_or_else(invalid_flow, Flow::Value);
        }
        if operator == UnaryOperator::Not {
            let RuntimeValue::Value(Value::Bool(value)) =
                flow_value!(self.eval_expr(frame, operand))
            else {
                return invalid_flow();
            };
            return Flow::Value(RuntimeValue::Value(Value::Bool(!value)));
        }
        let RuntimeValue::Value(Value::Integer(value)) =
            flow_value!(self.eval_expr(frame, operand))
        else {
            return invalid_flow();
        };
        let key = ExpressionKey::new(frame.module, expression);
        let FactState::Known(operation) = self
            .view
            .program
            .resolution()
            .body_types()
            .checked_numeric(key)
        else {
            return invalid_flow();
        };
        let Some(destination) = self.view.primitive_type(operation.result()) else {
            return invalid_flow();
        };
        match value.checked(operation.operator(), destination, None) {
            IntegerEvaluation::Value(value) => {
                Flow::Value(RuntimeValue::Value(Value::Integer(value)))
            }
            IntegerEvaluation::Fault(fault) => {
                Flow::NumericFault(numeric_fault(fault, operation.operator(), destination))
            }
            IntegerEvaluation::Invalid => invalid_flow(),
        }
    }

    fn resolve_place(
        &mut self,
        frame: &mut Frame,
        expression: ExprId,
    ) -> Result<Option<Place>, Flow> {
        let key = ExpressionKey::new(frame.module, expression);
        let FactState::Known(typed) = self.view.program.resolution().body_types().expression(key)
        else {
            return Err(invalid_flow());
        };
        let Some(place) = typed.place() else {
            return Ok(None);
        };
        match place.kind() {
            TypedPlaceKind::Local(binding) => {
                let slot = *frame.locals.get(binding).ok_or_else(invalid_flow)?;
                self.store.slots.get(slot).ok_or_else(invalid_flow)?;
                Ok(Some(Place {
                    root: slot,
                    projections: Vec::new(),
                }))
            }
            TypedPlaceKind::StoredField { receiver, field } => {
                let Some(mut place) = self.resolve_projection_base(frame, *receiver)? else {
                    return Err(invalid_flow());
                };
                place.projections.push(Projection::Field(field.clone()));
                Ok(Some(place))
            }
            TypedPlaceKind::TupleElement { receiver, index } => {
                let Some(mut place) = self.resolve_place(frame, *receiver)? else {
                    return Err(invalid_flow());
                };
                place.projections.push(Projection::Tuple(*index));
                Ok(Some(place))
            }
            TypedPlaceKind::Dereference { reference } => match self.eval_expr(frame, *reference) {
                Flow::Value(RuntimeValue::Reference(place)) => Ok(Some(place)),
                Flow::Value(RuntimeValue::Value(_) | RuntimeValue::Tuple(_)) => Err(invalid_flow()),
                flow => Err(flow),
            },
        }
    }

    fn resolve_projection_base(
        &mut self,
        frame: &mut Frame,
        expression: ExprId,
    ) -> Result<Option<Place>, Flow> {
        let Some(place) = self.resolve_place(frame, expression)? else {
            return Ok(None);
        };
        match self.store.read(&place).ok_or_else(invalid_flow)? {
            RuntimeValue::Reference(referent) => Ok(Some(referent)),
            RuntimeValue::Value(_) | RuntimeValue::Tuple(_) => Ok(Some(place)),
        }
    }

    fn bind_pattern(&mut self, frame: &mut Frame, pattern: PatternId, value: RuntimeValue) -> bool {
        let Some(body) = self.view.body(frame.module, frame.body) else {
            return false;
        };
        let Some(pattern_node) = body.pattern(pattern) else {
            return false;
        };
        match pattern_node.kind() {
            PatternKind::Binding(name) => {
                let Some(binding) = self.view.program.resolution().names().local_binding(
                    frame.module,
                    frame.body,
                    name.origin(),
                ) else {
                    return false;
                };
                self.bind_local(frame, binding, value);
                true
            }
            PatternKind::Wildcard | PatternKind::Unit => true,
            PatternKind::Group(inner) => self.bind_pattern(frame, *inner, value),
            PatternKind::Tuple(elements) => {
                let Some(values) = value.into_tuple() else {
                    return false;
                };
                if elements.len() != values.len() {
                    return false;
                }
                for (element, value) in elements.iter().zip(values.into_vec()) {
                    let PatternElement::Pattern(element) = element else {
                        return false;
                    };
                    if !self.bind_pattern(frame, *element, value) {
                        return false;
                    }
                }
                true
            }
            PatternKind::Alias { name, pattern } => {
                let Some(binding) = self.view.program.resolution().names().local_binding(
                    frame.module,
                    frame.body,
                    name.origin(),
                ) else {
                    return false;
                };
                self.bind_local(frame, binding, value.clone());
                self.bind_pattern(frame, *pattern, value)
            }
            PatternKind::Or(_)
            | PatternKind::Literal(_)
            | PatternKind::Variant { .. }
            | PatternKind::Struct { .. }
            | PatternKind::Invalid => false,
        }
    }

    fn bind_local(&mut self, frame: &mut Frame, binding: LocalBindingId, value: RuntimeValue) {
        let slot = self.store.allocate(value);
        frame.locals.insert(binding, slot);
    }

    fn match_pattern(
        &mut self,
        frame: &Frame,
        pattern: PatternId,
        value: &RuntimeValue,
        bindings: &mut Vec<(LocalBindingId, RuntimeValue)>,
    ) -> bool {
        let Some(body) = self.view.body(frame.module, frame.body) else {
            return false;
        };
        let Some(pattern_node) = body.pattern(pattern) else {
            return false;
        };
        match pattern_node.kind() {
            PatternKind::Binding(name) => {
                let Some(binding) = self.view.program.resolution().names().local_binding(
                    frame.module,
                    frame.body,
                    name.origin(),
                ) else {
                    return false;
                };
                bindings.push((binding, value.clone()));
                true
            }
            PatternKind::Wildcard => true,
            PatternKind::Unit => matches!(value, RuntimeValue::Value(Value::Unit)),
            PatternKind::Literal(literal) => {
                matches!(value, RuntimeValue::Value(value) if literal_matches(literal, value))
            }
            PatternKind::Group(inner) => self.match_pattern(frame, *inner, value, bindings),
            PatternKind::Alias { name, pattern } => {
                let checkpoint = bindings.len();
                let Some(binding) = self.view.program.resolution().names().local_binding(
                    frame.module,
                    frame.body,
                    name.origin(),
                ) else {
                    return false;
                };
                bindings.push((binding, value.clone()));
                if self.match_pattern(frame, *pattern, value, bindings) {
                    true
                } else {
                    bindings.truncate(checkpoint);
                    false
                }
            }
            PatternKind::Or(alternatives) => {
                for alternative in alternatives {
                    let checkpoint = bindings.len();
                    if self.match_pattern(frame, *alternative, value, bindings) {
                        return true;
                    }
                    bindings.truncate(checkpoint);
                }
                false
            }
            PatternKind::Tuple(elements) => {
                let Some(value_count) = value.tuple_len() else {
                    return false;
                };
                if elements.len() != value_count {
                    return false;
                }
                let checkpoint = bindings.len();
                for (index, element) in elements.iter().enumerate() {
                    let PatternElement::Pattern(pattern) = element else {
                        bindings.truncate(checkpoint);
                        return false;
                    };
                    let Some(value) = value.tuple_element(index) else {
                        bindings.truncate(checkpoint);
                        return false;
                    };
                    if !self.match_pattern(frame, *pattern, &value, bindings) {
                        bindings.truncate(checkpoint);
                        return false;
                    }
                }
                true
            }
            PatternKind::Variant { payload, .. } => {
                let key = ResolutionKey::Pattern {
                    module: frame.module,
                    pattern,
                };
                let FactState::Known(ResolvedName::Declarations(targets)) =
                    self.view.program.resolution().names().fact(key)
                else {
                    return false;
                };
                let [target] = targets.as_ref() else {
                    return false;
                };
                let RuntimeValue::Value(Value::Variant {
                    declaration,
                    payload: values,
                }) = value
                else {
                    return false;
                };
                if declaration != target {
                    return false;
                }
                match payload {
                    PatternPayload::Absent => values.is_empty(),
                    PatternPayload::Positional(elements) => {
                        if elements.len() != values.len() {
                            return false;
                        }
                        let checkpoint = bindings.len();
                        for (element, value) in elements.iter().zip(values.iter()) {
                            let PatternElement::Pattern(pattern) = element else {
                                bindings.truncate(checkpoint);
                                return false;
                            };
                            if !self.match_pattern(
                                frame,
                                *pattern,
                                &RuntimeValue::Value(value.clone()),
                                bindings,
                            ) {
                                bindings.truncate(checkpoint);
                                return false;
                            }
                        }
                        true
                    }
                    PatternPayload::Named(_) => false,
                }
            }
            PatternKind::Struct { .. } | PatternKind::Invalid => false,
        }
    }
}

macro_rules! flow_value {
    ($flow:expr) => {
        match $flow {
            Flow::Value(value) => value,
            other => return other,
        }
    };
}
use flow_value;

fn invalid_flow() -> Flow {
    Flow::InvalidCheckedProgram
}

fn decode_string_literal(spelling: &str) -> Option<String> {
    let body = spelling.strip_prefix('"')?.strip_suffix('"')?;
    let mut decoded = String::with_capacity(body.len());
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match characters.next()? {
            '\\' => decoded.push('\\'),
            '"' => decoded.push('"'),
            'n' => decoded.push('\n'),
            'r' => decoded.push('\r'),
            't' => decoded.push('\t'),
            'u' => {
                if characters.next()? != '{' {
                    return None;
                }
                let mut digits = String::new();
                loop {
                    let digit = characters.next()?;
                    if digit == '}' {
                        break;
                    }
                    digits.push(digit);
                }
                if digits.is_empty() || digits.len() > 6 {
                    return None;
                }
                decoded.push(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?);
            }
            _ => return None,
        }
    }
    Some(decoded)
}

fn parse_integer(spelling: &str) -> Option<i128> {
    spelling.replace('_', "").parse().ok()
}

fn literal_matches(literal: &Literal, value: &Value) -> bool {
    match (literal.kind(), value) {
        (LiteralKind::Bool, Value::Bool(value)) => (literal.spelling() == "true") == *value,
        (LiteralKind::String, Value::Str(value)) => {
            decode_string_literal(literal.spelling()).as_deref() == Some(value)
        }
        (LiteralKind::Integer, Value::Integer(value)) => {
            parse_integer(literal.spelling()) == Some(value.value())
        }
        _ => false,
    }
}

fn display_value(value: &Value) -> Option<String> {
    match value {
        Value::Unit => Some("()".into()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Integer(value) => Some(value.value().to_string()),
        Value::Str(value) => Some(value.to_string()),
        Value::Tuple(_) | Value::Struct { .. } | Value::Variant { .. } | Value::NumericError(_) => {
            None
        }
    }
}
