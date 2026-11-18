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

    pub(super) fn pop_block_region_override(
        &mut self,
        expected: ExprId,
    ) -> Result<(), BodyTypingError> {
        let actual = self
            .block_region_overrides
            .pop()
            .ok_or(BodyTypingError::MissingBlockRegionOverride)?;
        if actual != expected {
            return Err(BodyTypingError::BlockRegionOverrideMismatch { expected, actual });
        }
        Ok(())
    }

    pub(super) fn push_control_path_barrier(
        &mut self,
        continuation: NormalContinuation,
    ) -> Option<usize> {
        if continuation.is_present() {
            return None;
        }
        let floor = self.region_stack.len();
        // Lexical cleanup keeps active barrier floors nondecreasing.
        debug_assert!(
            self.control_path_barriers
                .last()
                .is_none_or(|&active| active <= floor)
        );
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
            .rposition(|region| region == &target)
            .ok_or_else(|| BodyTypingError::InactiveControlTarget(target.clone()))?;
        if self
            .control_path_barriers
            .last()
            .is_some_and(|&floor| target_index < floor)
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
