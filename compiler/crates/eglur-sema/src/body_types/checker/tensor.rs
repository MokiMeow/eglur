use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_tensor_constructor(
        &mut self,
        context: BodyContext<'_, '_>,
        expression: ExprId,
        origin: OriginId,
        call: &eglur_hir::DTypeCallExpr,
        declaration: CanonicalTarget,
        constructor: crate::TensorSourceConstructor,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckTensorConstructors),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let dtype = match self.type_occurrence(source, call.dtype)? {
            Outcome::Known(ty) => ty,
            other => return Ok(other.map(|ty| self.value_expression(ty))),
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
                .map(Outcome::Poisoned);
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
            return Ok(Outcome::Unperformed);
        };
        let result = self
            .signatures
            .types_mut()
            .nominal(NominalHead::Declaration(tensor), [dtype])?;
        let nat = self.primitive(PrimitiveType::Nat)?;
        let mut continuation = NormalContinuation::Present;
        for argument in &call.arguments {
            if argument.name.is_some() {
                return Ok(self.unsupported(
                    TypecheckCapability::AnalyzeTensorSource,
                    source,
                    argument.origin,
                ));
            }
            let value = match self.with_control_path(continuation, |checker| {
                checker.type_expression(
                    module,
                    source,
                    hir,
                    body,
                    argument.value,
                    TypeRequest::definitive(Some(nat)),
                )
            })? {
                Outcome::Known(value) => value,
                other => return Ok(other),
            };
            if !self.signatures.types().is_assignable(value.ty, nat)? {
                let slot = ApplicationSlot {
                    name: "dims".into(),
                    template: nat,
                    value: argument.value,
                    origin: argument.origin,
                    related: None,
                    evaluation_order: 0,
                    receiver_adjustment: false,
                };
                return self
                    .call_argument_mismatch(source, &slot, nat, value.ty)
                    .map(Outcome::Poisoned);
            }
            continuation = continuation.strict(value.normal_continuation);
        }
        let callee_ty = self
            .signatures
            .types_mut()
            .callable(std::iter::repeat_n(nat, call.arguments.len()), result)?;
        self.record_expression(
            ExpressionKey::new(module, call.callee),
            Outcome::Known(self.value_expression(callee_ty)),
        )?;
        let arguments = call
            .arguments
            .iter()
            .map(|argument| argument.value)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.invocations.insert(
            ExpressionKey::new(module, expression),
            SemanticFact::Known(InvocationFact {
                target: InvocationTarget::TensorConstructor {
                    declaration,
                    constructor,
                    dtype,
                },
                evaluation_order: arguments.clone(),
                arguments,
            }),
        )?;
        Ok(Outcome::Known(
            self.value_expression_with_continuation(result, continuation),
        ))
    }
}
