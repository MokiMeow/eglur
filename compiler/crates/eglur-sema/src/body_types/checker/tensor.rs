use super::invocation::{InvocationContinuation, InvocationPlan};
use super::*;

pub(super) struct TensorConstructorFrame {
    expression: ExprId,
    callee: ExprId,
    arguments: Box<[eglur_hir::CallArgument]>,
    declaration: CanonicalTarget,
    constructor: crate::TensorSourceConstructor,
    dtype: TypeId,
    result: TypeId,
    nat: TypeId,
    next: usize,
    normal_continuation: NormalContinuation,
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn prepare_dtype_call(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: eglur_hir::DTypeCallExpr,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext { module, source, .. } = context;
        let (callee_id, callee) = context
            .body
            .grouping_chain(call.callee)
            .last()
            .ok_or(BodyTypingError::WrongBodyExpression(call.callee))?;
        if !matches!(callee.kind(), ExprKind::Name(_)) {
            return self
                .dtype_bracket_forbidden(source, origin)
                .map(InvocationPlan::Complete);
        }
        let key = ResolutionKey::Expression {
            module,
            expression: callee_id,
        };
        match self.names.fact(key) {
            FactState::Known(ResolvedName::Declarations(targets)) if targets.len() == 1 => {
                if let Some(ExternalSemanticOwner::TensorSource(constructor)) =
                    self.external_semantic_owner(&targets[0])
                {
                    let target = targets[0].clone();
                    self.prepare_tensor_constructor(
                        context,
                        expression,
                        origin,
                        call,
                        target,
                        constructor,
                    )
                } else {
                    self.dtype_bracket_forbidden(source, origin)
                        .map(InvocationPlan::Complete)
                }
            }
            FactState::Known(ResolvedName::Local(_))
            | FactState::Known(ResolvedName::Declarations(_))
            | FactState::Known(ResolvedName::TypeMemberOwner(_))
            | FactState::NotApplicable => self
                .dtype_bracket_forbidden(source, origin)
                .map(InvocationPlan::Complete),
            FactState::Poisoned(poison) => {
                Ok(InvocationPlan::Complete(Outcome::Poisoned(poison.clone())))
            }
            FactState::Unperformed => Ok(InvocationPlan::Complete(Outcome::Unperformed)),
        }
    }

    fn dtype_bracket_forbidden(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        self.type_error(
            source,
            origin,
            "TYPE_ARG_BRACKET_SYNTAX_FORBIDDEN",
            "type argument bracket syntax is forbidden for this callee",
            BTreeMap::new(),
        )
        .map(Outcome::Poisoned)
    }

    fn prepare_tensor_constructor(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: eglur_hir::DTypeCallExpr,
        declaration: CanonicalTarget,
        constructor: crate::TensorSourceConstructor,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let BodyContext { source, hir, .. } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckTensorConstructors),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        }
        let dtype = match self.type_occurrence(source, call.dtype)? {
            Outcome::Known(ty) => ty,
            other => {
                return Ok(InvocationPlan::Complete(
                    other.map(|ty| self.value_expression(ty)),
                ));
            }
        };
        if !matches!(self.signatures.types().kind(dtype)?, TypeKind::Primitive(primitive) if constructor.accepts_dtype(*primitive))
        {
            let actual = self.format_type(dtype)?;
            return self
                .type_error(
                    source,
                    hir.type_ref(call.dtype).origin(),
                    "CALL_ARG_TYPE_MISMATCH",
                    "call argument type mismatch for constructor dtype",
                    BTreeMap::from([
                        ("reason".into(), DiagnosticDataValue::from("type_family")),
                        ("actual_type".into(), DiagnosticDataValue::from(actual)),
                        (
                            "expected_family".into(),
                            DiagnosticDataValue::from("constructor_dtype"),
                        ),
                    ]),
                )
                .map(Outcome::Poisoned)
                .map(InvocationPlan::Complete);
        }
        let Some(tensor) = self
            .signatures
            .language_items()
            .declaration(LanguageItem::Tensor)
            .cloned()
        else {
            self.recorder.require(
                CompileCapability::Resolve(
                    crate::ResolveCapability::ResolveCompleteStandardLibraryNames,
                ),
                SemanticLocation::origin(source.file(), origin),
            );
            return Ok(InvocationPlan::Complete(Outcome::Unperformed));
        };
        let result = self
            .signatures
            .types_mut()
            .nominal(NominalHead::Declaration(tensor), [dtype])?;
        let nat = self.primitive(PrimitiveType::Nat)?;
        self.advance_tensor_constructor(
            context,
            Box::new(TensorConstructorFrame {
                expression,
                callee: call.callee,
                arguments: call.arguments,
                declaration,
                constructor,
                dtype,
                result,
                nat,
                next: 0,
                normal_continuation: NormalContinuation::Present,
            }),
        )
    }

    fn advance_tensor_constructor(
        &mut self,
        context: BodyContext<'_, '_>,
        frame: Box<TensorConstructorFrame>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        if let Some(argument) = frame.arguments.get(frame.next) {
            if argument.name.is_some() {
                return Ok(InvocationPlan::Complete(self.unsupported(
                    TypecheckCapability::AnalyzeTensorSource,
                    context.source,
                    argument.origin,
                )));
            }
            return Ok(InvocationPlan::Expression {
                expression: argument.value,
                request: TypeRequest::definitive(Some(frame.nat)),
                path: frame.normal_continuation,
                continuation: InvocationContinuation::TensorConstructor(frame),
            });
        }

        let callee_ty = self.signatures.types_mut().callable(
            std::iter::repeat_n(frame.nat, frame.arguments.len()),
            frame.result,
        )?;
        self.record_callee_type(context, frame.callee, callee_ty)?;
        self.invocations.insert(
            ExpressionKey::new(context.module, frame.expression),
            SemanticFact::Known(InvocationFact::new(
                InvocationTarget::TensorConstructor {
                    declaration: frame.declaration,
                    constructor: frame.constructor,
                    dtype: frame.dtype,
                },
                frame.arguments.iter().enumerate().map(|(order, argument)| {
                    (
                        order,
                        InvocationArgument::new(argument.value, ArgumentPassing::Value),
                    )
                }),
            )),
        )?;
        Ok(InvocationPlan::Complete(Outcome::Known(
            self.value_expression_with_continuation(frame.result, frame.normal_continuation),
        )))
    }

    pub(super) fn resume_tensor_constructor(
        &mut self,
        context: BodyContext<'_, '_>,
        mut frame: Box<TensorConstructorFrame>,
        outcome: Outcome<TypedExpression>,
    ) -> Result<InvocationPlan, BodyTypingError> {
        let value = match outcome {
            Outcome::Known(value) => value,
            other => return Ok(InvocationPlan::Complete(other)),
        };
        let argument = &frame.arguments[frame.next];
        if !self.signatures.types().is_assignable(value.ty, frame.nat)? {
            let slot = ApplicationSlot {
                name: "dims".into(),
                template: frame.nat,
                value: argument.value,
                origin: argument.origin,
                related: None,
                evaluation_order: 0,
                passing: ArgumentPassing::Value,
            };
            return self
                .call_argument_mismatch(context.source, &slot, frame.nat, value.ty)
                .map(Outcome::Poisoned)
                .map(InvocationPlan::Complete);
        }
        frame.normal_continuation = frame.normal_continuation.strict(value.normal_continuation);
        frame.next += 1;
        self.advance_tensor_constructor(context, frame)
    }
}
