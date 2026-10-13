use super::*;

/// Original input custody, not semantic type equality or diagnostic blame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExpectationCustody {
    /// A local inference borrows the current consumer; it cannot admit a root.
    Caller,
    Written(TypeOccurrenceId),
    CallArgument {
        call: ExpressionKey,
        index: usize,
    },
    Selection(ExpressionKey),
}

impl ExpectationCustody {
    pub(super) fn call_argument(call: ExpressionKey, syntax: &CallExpr, index: usize) -> Self {
        if crate::analysis_work::is_original_call(syntax) {
            Self::CallArgument { call, index }
        } else {
            Self::Caller
        }
    }
}

/// Retargeting a child or normalized type preserves its original whole demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ExpectedType {
    ty: TypeId,
    custody: ExpectationCustody,
}

impl ExpectedType {
    pub(super) const fn new(ty: TypeId, custody: ExpectationCustody) -> Self {
        Self { ty, custody }
    }

    pub(super) const fn caller(ty: TypeId) -> Self {
        Self::new(ty, ExpectationCustody::Caller)
    }

    pub(super) const fn written(ty: TypeId, file: FileId, type_ref: TypeRefId) -> Self {
        Self::new(
            ty,
            ExpectationCustody::Written(TypeOccurrenceId::new(file, type_ref)),
        )
    }

    pub(super) const fn ty(self) -> TypeId {
        self.ty
    }

    pub(super) const fn custody(self) -> ExpectationCustody {
        self.custody
    }

    pub(super) const fn with_type(self, ty: TypeId) -> Self {
        Self::new(ty, self.custody)
    }
}

impl TypeRequest {
    pub(super) fn expected_type(self) -> Option<TypeId> {
        self.expected.map(ExpectedType::ty)
    }

    pub(super) fn custody(self) -> ExpectationCustody {
        self.expected
            .map_or(ExpectationCustody::Caller, ExpectedType::custody)
    }

    pub(super) fn retarget(self, expected: Option<TypeId>) -> Option<ExpectedType> {
        expected.map(|ty| ExpectedType::new(ty, self.custody()))
    }

    pub(super) fn local(expected: Option<TypeId>) -> Self {
        Self::definitive(expected.map(ExpectedType::caller))
    }
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn normalize_expected_at(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        expected: ExpectedType,
    ) -> Result<Outcome<ExpectedType>, BodyTypingError> {
        #[cfg(test)]
        tests::observe(tests::Event::Normalize(expected));

        Ok(self
            .normalize_type_at(source, origin, expected.ty())?
            .map(|ty| expected.with_type(ty)))
    }
}

#[cfg(test)]
pub(super) fn observe_request(key: ExpressionKey, request: TypeRequest) {
    tests::observe(tests::Event::Request { key, request });
}

#[cfg(test)]
pub(super) fn observe_candidate(expected: ExpectedType) {
    tests::observe(tests::Event::Candidate(expected));
}

#[cfg(test)]
mod tests;
