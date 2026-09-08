use super::*;

use crate::declaration_graph::{DeclarationGraphError, dependency_first_sccs};

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn has_nonconstant_call(&self, module: ModuleId, body: &Body) -> bool {
        body.expressions().any(|(_, expression)| {
            let callee = match expression.kind() {
                ExprKind::Call(call) => call.callee,
                ExprKind::DTypeCall(call) => call.callee,
                ExprKind::CatalogCall(call) => return call.member.is_bang(),
                _ => return false,
            };
            let bang = body.expression(callee).is_some_and(|expression| match expression.kind() {
                ExprKind::Name(path) => path.last().is_some_and(eglur_hir::Name::is_bang),
                ExprKind::Member { member, .. } => member.is_bang(),
                _ => false,
            });
            bang || matches!(self.names.fact(ResolutionKey::Expression { module, expression: callee }),
                FactState::Known(ResolvedName::Declarations(targets)) if targets.iter().any(|target| {
                    matches!(target, CanonicalTarget::Source(id) if self.collection.declaration(*id)
                        .is_some_and(|declaration| matches!(declaration.kind(), DeclarationKind::Function | DeclarationKind::ExternFunction)))
                }))
        })
    }

    pub(super) fn check_constant_declarations(
        &mut self,
        declarations: &[DefId],
    ) -> Result<(), BodyTypingError> {
        let positions = declarations
            .iter()
            .enumerate()
            .map(|(position, &declaration)| (declaration, position))
            .collect::<BTreeMap<_, _>>();
        let mut dependencies = BTreeMap::new();
        for &declaration in declarations {
            let (module, source, hir, item) = self.source_item(declaration)?;
            self.require_constant_declarations(source, item.origin());
            let ItemKind::Const(value) = item.kind() else {
                return Err(BodyTypingError::WrongDeclarationKind(declaration));
            };
            let mut edges = BTreeMap::new();
            for (expression, value) in hir.body(value.body).expressions() {
                let FactState::Known(ResolvedName::Declarations(targets)) = self
                    .names
                    .fact(ResolutionKey::Expression { module, expression })
                else {
                    continue;
                };
                if let [CanonicalTarget::Source(target)] = targets.as_ref()
                    && positions.contains_key(target)
                {
                    edges.entry(*target).or_insert(value.origin());
                }
            }
            dependencies.insert(declaration, edges);
        }
        let components = dependency_first_sccs(
            declarations.iter().copied(),
            dependencies.iter().flat_map(|(&declaration, edges)| {
                edges
                    .keys()
                    .map(move |&dependency| (declaration, dependency))
            }),
        )
        .map_err(|DeclarationGraphError::UnknownNode(node)| {
            BodyTypingError::UnknownDeclaration(node)
        })?;

        // Diagnose cycles before any same-module forward edge can claim the cause.
        for component in &components {
            let cyclic =
                component.len() > 1 || dependencies[&component[0]].contains_key(&component[0]);
            if cyclic {
                for &declaration in component {
                    let (_, _, _, item) = self.source_item(declaration)?;
                    self.reject_constant(
                        declaration,
                        item.origin(),
                        "CONST_CYCLE",
                        "const dependency cycle",
                    )?;
                }
            }
        }
        for component in &components {
            for &declaration in component {
                if matches!(self.bodies.state(&declaration), FactState::Poisoned(_)) {
                    continue;
                }
                let mut inherited: Option<Poison> = None;
                for dependency in dependencies[&declaration].keys() {
                    if let FactState::Poisoned(poison) = self.bodies.state(dependency) {
                        inherited = Some(match inherited {
                            Some(previous) => previous.merge(poison)?,
                            None => poison.clone(),
                        });
                    }
                }
                if let Some(poison) = inherited {
                    self.bodies
                        .insert(declaration, SemanticFact::Poisoned(poison))?;
                    continue;
                }
                let forward = dependencies[&declaration].iter().find(|(dependency, _)| {
                    dependency.module() == declaration.module()
                        && positions[dependency] > positions[&declaration]
                });
                if let Some((_, &origin)) = forward {
                    self.reject_constant(
                        declaration,
                        origin,
                        "CONST_FORWARD_REFERENCE",
                        "const forward reference",
                    )?;
                    continue;
                }
                self.check_declaration(declaration)?;
            }
        }
        Ok(())
    }

    pub(super) fn require_constant_declarations(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> bool {
        self.recorder.require(
            CompileCapability::Typecheck(
                TypecheckCapability::EvaluateSelectedIntegerConstDeclarations,
            ),
            SemanticLocation::origin(source.file(), origin),
        )
    }

    pub(super) fn reject_constant(
        &mut self,
        declaration: DefId,
        origin: OriginId,
        code: &'static str,
        message: &str,
    ) -> Result<(), BodyTypingError> {
        let (_, source, _, _) = self.source_item(declaration)?;
        let previous = self.current_owner.replace(declaration);
        let poison = self.type_error(source, origin, code, message, BTreeMap::new());
        self.current_owner = previous;
        self.bodies
            .insert(declaration, SemanticFact::Poisoned(poison?))?;
        Ok(())
    }

    pub(super) fn type_constant_reference(
        &mut self,
        source: &SemanticSource<'_>,
        expression: ExprId,
        target: &CanonicalTarget,
        ty: TypeId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let origin = self.expression_origin(source, expression)?;
        if !self.require_constant_declarations(source, origin) {
            return Ok(Outcome::Unperformed);
        }
        let ty = match self.normalize_type_at(source, origin, ty)? {
            Outcome::Known(ty) => ty,
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred | Outcome::Unperformed | Outcome::NotApplicable => {
                return Ok(Outcome::Unperformed);
            }
        };
        let CanonicalTarget::Source(declaration) = target else {
            return Ok(self.unsupported(
                TypecheckCapability::EvaluateScalarConstExpressions,
                source,
                origin,
            ));
        };
        match self.bodies.state(declaration) {
            FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
            FactState::Unperformed | FactState::NotApplicable => return Ok(Outcome::Unperformed),
            FactState::Known(_) => {}
        }
        let (module, _, _, item) = self.source_item(*declaration)?;
        let ItemKind::Const(value) = item.kind() else {
            return Err(BodyTypingError::WrongDeclarationKind(*declaration));
        };
        let key = ExpressionKey::new(module, value.value);
        let FactState::Known(initializer) = self.expressions.state(&key) else {
            return Err(BodyTypingError::InvalidIntegerConstant);
        };
        let Some(value) = initializer.integer_constant else {
            return Ok(self.unsupported(
                TypecheckCapability::EvaluateScalarConstExpressions,
                source,
                origin,
            ));
        };
        let mut typed = self.value_expression(ty);
        typed.integer_constant = Some(value);
        Ok(Outcome::Known(typed))
    }
}
