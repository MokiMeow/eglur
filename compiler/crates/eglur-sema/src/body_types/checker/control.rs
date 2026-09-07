use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn enter_control_region(
        &mut self,
        key: ControlRegionKey,
    ) -> Result<(), BodyTypingError> {
        let parent = self.region_stack.last().cloned();
        let region = ControlRegion::new(key.clone(), parent);
        match self.regions.state(&key) {
            FactState::Unperformed => self
                .regions
                .insert(key.clone(), SemanticFact::Known(region))?,
            FactState::Known(existing) if existing == &region => {}
            FactState::Known(_) | FactState::Poisoned(_) | FactState::NotApplicable => {
                return Err(DuplicateFact.into());
            }
        }
        self.region_stack.push(key);
        Ok(())
    }

    pub(super) fn leave_control_region(
        &mut self,
        expected: &ControlRegionKey,
    ) -> Result<(), BodyTypingError> {
        let actual = self
            .region_stack
            .pop()
            .ok_or(BodyTypingError::MissingControlRegion)?;
        if &actual != expected {
            return Err(BodyTypingError::ControlRegionStackMismatch {
                expected: expected.clone(),
                actual,
            });
        }
        Ok(())
    }

    pub(super) fn current_control_region(&self) -> Result<&ControlRegionKey, BodyTypingError> {
        self.region_stack
            .last()
            .ok_or(BodyTypingError::MissingControlRegion)
    }

    pub(super) fn push_control_path_barrier(
        &mut self,
        continuation: NormalContinuation,
    ) -> Option<usize> {
        if continuation.is_present() {
            return None;
        }
        let floor = self.region_stack.len();
        self.control_path_barriers.push(floor);
        Some(floor)
    }

    pub(super) fn pop_control_path_barrier(
        &mut self,
        expected: Option<usize>,
    ) -> Result<(), BodyTypingError> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let actual = self
            .control_path_barriers
            .pop()
            .ok_or(BodyTypingError::MissingControlPathBarrier)?;
        if actual != expected {
            return Err(BodyTypingError::ControlPathBarrierMismatch { expected, actual });
        }
        Ok(())
    }

    pub(super) fn with_control_path<T>(
        &mut self,
        continuation: NormalContinuation,
        analyze: impl FnOnce(&mut Self) -> Result<T, BodyTypingError>,
    ) -> Result<T, BodyTypingError> {
        let barrier = self.push_control_path_barrier(continuation);
        let outcome = analyze(self);
        self.pop_control_path_barrier(barrier)?;
        outcome
    }

    pub(super) fn innermost_yield_target(&self) -> Result<ControlRegionKey, BodyTypingError> {
        self.current_control_region().cloned()
    }

    pub(super) fn callable_target(&self) -> Result<ControlRegionKey, BodyTypingError> {
        self.region_stack
            .iter()
            .rev()
            .find(|region| matches!(region, ControlRegionKey::Callable(_)))
            .cloned()
            .ok_or(BodyTypingError::MissingCallableRegion)
    }

    pub(super) fn record_return_transfer(
        &mut self,
        module: ModuleId,
        statement: StmtId,
    ) -> Result<(), BodyTypingError> {
        let target = self.callable_target()?;
        self.record_control_transfer(
            ControlTransferSource::Statement(StatementKey::new(module, statement)),
            ControlTransferKind::Return,
            target,
        )
        .map(|_| ())
    }

    pub(super) fn record_control_transfer(
        &mut self,
        source: ControlTransferSource,
        kind: ControlTransferKind,
        target: ControlRegionKey,
    ) -> Result<bool, BodyTypingError> {
        let target_index = self
            .region_stack
            .iter()
            .position(|region| region == &target)
            .ok_or_else(|| BodyTypingError::InactiveControlTarget(target.clone()))?;
        if self
            .control_path_barriers
            .iter()
            .any(|&floor| target_index < floor)
        {
            return Ok(false);
        }
        let exited_regions = self.region_stack[target_index..]
            .to_vec()
            .into_boxed_slice();
        self.transfers
            .push(ControlTransfer::new(source, kind, target, exited_regions));
        Ok(true)
    }

    pub(super) fn type_empty_match(
        &mut self,
        context: BodyContext<'_, '_>,
        origin: OriginId,
        scrutinee: ExprId,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::AnalyzeInhabitance),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let scrutinee = self.type_expression(
            module,
            source,
            hir,
            body,
            scrutinee,
            TypeRequest::definitive(None),
        )?;
        let Outcome::Known(scrutinee) = scrutinee else {
            return Ok(scrutinee);
        };
        match self.inhabitance.classify(self.signatures, scrutinee.ty)? {
            Inhabitance::Uninhabited => Ok(Outcome::Known(
                self.value_expression(self.signatures.types().never()),
            )),
            Inhabitance::Unknown | Inhabitance::Inhabited => self
                .type_error(
                    source,
                    origin,
                    "MATCH_NON_EXHAUSTIVE",
                    "match is non-exhaustive for a possibly inhabited scrutinee",
                    BTreeMap::new(),
                )
                .map(Outcome::Poisoned),
        }
    }

    pub(super) fn type_if(
        &mut self,
        context: BodyContext<'_, '_>,
        expression_id: ExprId,
        origin: OriginId,
        expression: &eglur_hir::IfExpr,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(Outcome::Unperformed);
        }

        let condition = self.type_expression(
            module,
            source,
            hir,
            body,
            expression.condition,
            TypeRequest::definitive(None),
        )?;
        let mut poison = None;
        let mut unperformed = false;
        let mut deferred = false;
        let condition_continuation = match condition {
            Outcome::Known(condition) => {
                let bool_ty = self.primitive(PrimitiveType::Bool)?;
                if condition.ty != bool_ty {
                    let found = self.type_error(
                        source,
                        self.expression_origin(source, expression.condition)?,
                        "IF_CONDITION_NOT_BOOL",
                        "if condition must have type Bool or Tri",
                        BTreeMap::new(),
                    )?;
                    self.merge_poison(&mut poison, &found)?;
                } else if expression.unknown_branch.is_some() {
                    let found = self.type_error(
                        source,
                        origin,
                        "IF_BOOL_UNKNOWN_BRANCH_FOR_BOOL",
                        "if Bool condition has a forbidden unknown branch",
                        BTreeMap::new(),
                    )?;
                    self.merge_poison(&mut poison, &found)?;
                }
                condition.normal_continuation
            }
            Outcome::Poisoned(found) => {
                self.merge_poison(&mut poison, &found)?;
                NormalContinuation::Absent
            }
            Outcome::Unperformed | Outcome::NotApplicable => {
                unperformed = true;
                NormalContinuation::Absent
            }
            Outcome::Deferred => {
                deferred = true;
                NormalContinuation::Absent
            }
        };

        if expression.else_branch.is_none() && request.value_required {
            let found = self.type_error(
                source,
                origin,
                "IF_MISSING_ELSE_VALUE",
                "if without else cannot be used as a value",
                BTreeMap::new(),
            )?;
            self.merge_poison(&mut poison, &found)?;
        }

        let mut common = None;
        let mut any_normal_branch = expression.else_branch.is_none();
        let owner = ExpressionKey::new(module, expression_id);
        let branches = [
            (IfBranch::Then, Some(expression.then_branch)),
            (IfBranch::Else, expression.else_branch),
            (IfBranch::Unknown, expression.unknown_branch),
        ];
        for (branch_slot, branch_id) in branches
            .into_iter()
            .filter_map(|(slot, branch)| branch.map(|branch| (slot, branch)))
        {
            let branch = self.with_control_path(condition_continuation, |checker| {
                checker.type_expression_in_region(
                    ControlRegionKey::IfBranch {
                        owner,
                        branch: branch_slot,
                    },
                    context,
                    branch_id,
                    TypeRequest::definitive(common.or(request.expected)),
                )
            })?;
            match branch {
                Outcome::Known(branch) => {
                    if !branch.normal_continuation.is_present() {
                        continue;
                    }
                    any_normal_branch = true;
                    if expression.else_branch.is_none() {
                        continue;
                    }
                    if let Some(left) = common {
                        if let Some(joined) = self.branch_common_type(left, branch.ty)? {
                            common = Some(joined);
                        } else {
                            let found = self.type_error(
                                source,
                                origin,
                                "IF_BRANCH_TYPE_MISMATCH",
                                "if branch result types mismatch",
                                BTreeMap::from([
                                    (
                                        Box::<str>::from("left_type"),
                                        DiagnosticDataValue::from(self.format_type(left)?),
                                    ),
                                    (
                                        Box::<str>::from("right_type"),
                                        DiagnosticDataValue::from(self.format_type(branch.ty)?),
                                    ),
                                ]),
                            )?;
                            self.merge_poison(&mut poison, &found)?;
                        }
                    } else {
                        common = Some(branch.ty);
                    }
                }
                Outcome::Poisoned(found) => self.merge_poison(&mut poison, &found)?,
                Outcome::Unperformed | Outcome::NotApplicable => unperformed = true,
                Outcome::Deferred => deferred = true,
            }
        }

        if let Some(poison) = poison {
            return Ok(Outcome::Poisoned(poison));
        }
        if unperformed {
            return Ok(Outcome::Unperformed);
        }
        if deferred {
            return Ok(Outcome::Deferred);
        }
        let ty = if expression.else_branch.is_none() {
            self.primitive(PrimitiveType::Unit)?
        } else if let Some(common) = common {
            common
        } else {
            self.signatures.types().never()
        };
        let branch_continuation = if any_normal_branch {
            NormalContinuation::Present
        } else {
            NormalContinuation::Absent
        };
        Ok(Outcome::Known(self.value_expression_with_continuation(
            ty,
            condition_continuation.strict(branch_continuation),
        )))
    }

    pub(super) fn type_match(
        &mut self,
        context: BodyContext<'_, '_>,
        expression_id: ExprId,
        origin: OriginId,
        expression: &eglur_hir::MatchExpr,
        request: TypeRequest,
    ) -> Result<Outcome<TypedExpression>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            hir,
            body,
        } = context;
        if !self.recorder.require(
            CompileCapability::Typecheck(TypecheckCapability::CheckBranching),
            SemanticLocation::origin(source.file(), origin),
        ) {
            return Ok(Outcome::Unperformed);
        }
        let scrutinee = self.type_expression(
            module,
            source,
            hir,
            body,
            expression.scrutinee,
            TypeRequest::definitive(None),
        )?;
        let scrutinee_type = match &scrutinee {
            Outcome::Known(value) => Some(value.ty),
            _ => None,
        };

        let mut poison = None;
        let mut unperformed = false;
        let mut deferred = false;
        let scrutinee_continuation = match scrutinee {
            Outcome::Known(value) => value.normal_continuation,
            Outcome::Poisoned(found) => {
                self.merge_poison(&mut poison, &found)?;
                NormalContinuation::Absent
            }
            Outcome::Unperformed | Outcome::NotApplicable => {
                unperformed = true;
                NormalContinuation::Absent
            }
            Outcome::Deferred => {
                deferred = true;
                NormalContinuation::Absent
            }
        };

        let mut common = None;
        let mut any_normal_arm = false;
        let mut coverage = Vec::with_capacity(expression.arms.len());
        let owner = ExpressionKey::new(module, expression_id);
        for (arm_index, arm) in expression.arms.iter().enumerate() {
            let arm_barrier = self.push_control_path_barrier(scrutinee_continuation);
            let region = ControlRegionKey::MatchArm {
                owner,
                arm: arm_index,
            };
            self.enter_control_region(region.clone())?;
            let mut guard_continuation = NormalContinuation::Present;
            let mut guard_false_continuation = None;
            let pattern = if let Some(expected) = scrutinee_type {
                self.type_match_pattern(context, arm.pattern, expected)?
            } else {
                Outcome::Unperformed
            };
            match pattern {
                Outcome::Known(pattern) => {
                    if let Some((name, duplicate_origin)) =
                        self.first_pattern_duplicate(body, arm.pattern)?
                    {
                        let found = self.type_error(
                            source,
                            duplicate_origin,
                            "PATTERN_DUPLICATE_BINDING",
                            format!("pattern binding `{}` is duplicated", name.spelling()),
                            BTreeMap::from([(
                                Box::<str>::from("name"),
                                DiagnosticDataValue::from(name.spelling()),
                            )]),
                        )?;
                        self.bind_pattern_poison(module, body, arm.pattern, found.clone())?;
                        self.merge_poison(&mut poison, &found)?;
                    } else {
                        for (binding, ty) in pattern.bindings {
                            self.bind_match_local(binding, ty)?;
                        }
                        coverage.push((arm, pattern.coverage));
                    }
                }
                Outcome::Poisoned(found) => {
                    self.bind_pattern_poison(module, body, arm.pattern, found.clone())?;
                    self.merge_poison(&mut poison, &found)?;
                }
                Outcome::Unperformed | Outcome::NotApplicable => unperformed = true,
                Outcome::Deferred => deferred = true,
            }

            if let Some(guard) = &arm.guard {
                let bool_ty = self.primitive(PrimitiveType::Bool)?;
                match self.type_expression(
                    module,
                    source,
                    hir,
                    body,
                    guard.condition,
                    TypeRequest::definitive(Some(bool_ty)),
                )? {
                    Outcome::Known(value) if value.ty == bool_ty => {
                        guard_continuation = value.normal_continuation;
                        guard_false_continuation = Some(value.normal_continuation);
                    }
                    Outcome::Known(value) => {
                        guard_continuation = value.normal_continuation;
                        guard_false_continuation = Some(value.normal_continuation);
                        let found = self.type_error(
                            source,
                            self.expression_origin(source, guard.condition)?,
                            "MATCH_GUARD_TYPE_MISMATCH",
                            "match guard must have type Bool",
                            BTreeMap::from([
                                (
                                    Box::<str>::from("expected_type"),
                                    DiagnosticDataValue::from("Bool"),
                                ),
                                (
                                    Box::<str>::from("actual_type"),
                                    DiagnosticDataValue::from(self.format_type(value.ty)?),
                                ),
                            ]),
                        )?;
                        self.merge_poison(&mut poison, &found)?;
                    }
                    Outcome::Poisoned(found) => {
                        guard_continuation = NormalContinuation::Absent;
                        self.merge_poison(&mut poison, &found)?;
                    }
                    Outcome::Unperformed | Outcome::NotApplicable => {
                        guard_continuation = NormalContinuation::Absent;
                        unperformed = true;
                    }
                    Outcome::Deferred => {
                        guard_continuation = NormalContinuation::Absent;
                        deferred = true;
                    }
                }
            }

            if guard_false_continuation.is_some_and(NormalContinuation::is_present) {
                self.record_control_transfer(
                    ControlTransferSource::Expression(ExpressionKey::new(
                        module,
                        arm.guard
                            .as_ref()
                            .expect("guard continuation requires a guard")
                            .condition,
                    )),
                    ControlTransferKind::MatchGuardFalse,
                    region.clone(),
                )?;
            }

            let arm_value = self.with_control_path(guard_continuation, |checker| {
                let value = checker.type_expression(
                    module,
                    source,
                    hir,
                    body,
                    arm.value,
                    TypeRequest::definitive(common.or(request.expected)),
                )?;
                if matches!(
                    &value,
                    Outcome::Known(value) if value.normal_continuation.is_present()
                ) {
                    checker.record_control_transfer(
                        ControlTransferSource::RegionCompletion(region.clone()),
                        ControlTransferKind::NormalCompletion,
                        region.clone(),
                    )?;
                }
                Ok(value)
            })?;
            match arm_value {
                Outcome::Known(value)
                    if !guard_continuation.is_present()
                        || !value.normal_continuation.is_present() => {}
                Outcome::Known(value) => {
                    any_normal_arm = true;
                    if let Some(left) = common {
                        if let Some(joined) = self.branch_common_type(left, value.ty)? {
                            common = Some(joined);
                        } else {
                            let found = self.type_error(
                                source,
                                self.expression_origin(source, arm.value)?,
                                "MATCH_ARM_TYPE_MISMATCH",
                                "match arm result types mismatch",
                                BTreeMap::from([
                                    (
                                        Box::<str>::from("left_type"),
                                        DiagnosticDataValue::from(self.format_type(left)?),
                                    ),
                                    (
                                        Box::<str>::from("right_type"),
                                        DiagnosticDataValue::from(self.format_type(value.ty)?),
                                    ),
                                ]),
                            )?;
                            self.merge_poison(&mut poison, &found)?;
                        }
                    } else {
                        common = Some(value.ty);
                    }
                }
                Outcome::Poisoned(found) => self.merge_poison(&mut poison, &found)?,
                Outcome::Unperformed | Outcome::NotApplicable => unperformed = true,
                Outcome::Deferred => deferred = true,
            }
            self.leave_control_region(&region)?;
            self.pop_control_path_barrier(arm_barrier)?;
        }

        if poison.is_none()
            && !unperformed
            && !deferred
            && let Some(expected) = scrutinee_type
        {
            match self.check_match_coverage(source, expected, &coverage)? {
                Outcome::Known(()) => {}
                Outcome::Poisoned(found) => self.merge_poison(&mut poison, &found)?,
                Outcome::Unperformed | Outcome::NotApplicable => unperformed = true,
                Outcome::Deferred => deferred = true,
            }
        }
        if let Some(poison) = poison {
            return Ok(Outcome::Poisoned(poison));
        }
        if unperformed {
            return Ok(Outcome::Unperformed);
        }
        if deferred {
            return Ok(Outcome::Deferred);
        }
        let ty = common.unwrap_or_else(|| self.signatures.types().never());
        let arm_continuation = if any_normal_arm {
            NormalContinuation::Present
        } else {
            NormalContinuation::Absent
        };
        Ok(Outcome::Known(self.value_expression_with_continuation(
            ty,
            scrutinee_continuation.strict(arm_continuation),
        )))
    }

    pub(super) fn branch_common_type(
        &self,
        left: TypeId,
        right: TypeId,
    ) -> Result<Option<TypeId>, BodyTypingError> {
        if left == right {
            return Ok(Some(left));
        }
        if matches!(
            self.signatures.types().kind(left)?,
            TypeKind::RawPointer { .. }
        ) || matches!(
            self.signatures.types().kind(right)?,
            TypeKind::RawPointer { .. }
        ) {
            return Ok(None);
        }
        if self.signatures.types().is_assignable(right, left)? {
            return Ok(Some(left));
        }
        if self.signatures.types().is_assignable(left, right)? {
            return Ok(Some(right));
        }
        Ok(None)
    }
}
