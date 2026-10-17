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
    pub(super) fn selection(original: Option<ExpressionKey>) -> Self {
        original.map_or(Self::Caller, Self::Selection)
    }

    pub(super) fn call_argument(original: Option<ExpressionKey>, index: usize) -> Self {
        original.map_or(Self::Caller, |call| Self::CallArgument { call, index })
    }
}

/// Retargeting a child or normalized type preserves its original whole demand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExpectedType {
    ty: TypeId,
    custody: ExpectationCustody,
    work: Option<crate::AnalysisRootTicket>,
}

impl ExpectedType {
    pub(super) const fn new(ty: TypeId, custody: ExpectationCustody) -> Self {
        Self {
            ty,
            custody,
            work: None,
        }
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

    pub(super) const fn ty(&self) -> TypeId {
        self.ty
    }

    pub(super) const fn custody(&self) -> ExpectationCustody {
        self.custody
    }

    pub(super) fn with_type(self, ty: TypeId) -> Self {
        Self { ty, ..self }
    }

    pub(super) fn with_work(mut self, work: Option<crate::AnalysisRootTicket>) -> Self {
        self.work = work;
        self
    }

    pub(super) fn work(&self) -> Option<&crate::AnalysisRootTicket> {
        self.work.as_ref()
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        TypeId,
        ExpectationCustody,
        Option<crate::AnalysisRootTicket>,
    ) {
        (self.ty, self.custody, self.work)
    }
}

impl TypeRequest {
    pub(super) fn expected(&self) -> Option<ExpectedType> {
        self.expected
            .map(|ty| ExpectedType::new(ty, self.custody).with_work(self.work.clone()))
    }

    pub(super) fn expected_type(&self) -> Option<TypeId> {
        self.expected
    }

    pub(super) fn custody(&self) -> ExpectationCustody {
        self.custody
    }

    /// Changes the expected type without discarding custody or inference flags.
    pub(super) fn retarget(&self, expected: Option<TypeId>) -> Self {
        Self {
            expected,
            ..self.clone()
        }
    }

    pub(super) fn with_custody(self, custody: ExpectationCustody) -> Self {
        let work = if custody == self.custody {
            self.work
        } else {
            None
        };
        Self {
            custody,
            work,
            ..self
        }
    }

    /// Starts definitive checking while retaining an existing whole demand.
    pub(super) fn definitive_child(&self, expected: Option<TypeId>) -> Self {
        Self::local(expected)
            .with_custody(self.custody)
            .with_work(self.work.clone())
    }

    pub(super) fn with_work(mut self, work: Option<crate::AnalysisRootTicket>) -> Self {
        self.work = work;
        self
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
    ) -> Result<Outcome<ExpectedType>, BodyFlow> {
        self.ensure_analysis_active()?;
        let work = self.expectation_work(source.file(), expected.custody(), expected.work())?;
        let expected = expected.with_work(work);
        #[cfg(test)]
        tests::observe(tests::Event::Normalize(expected.clone()));

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
pub(super) mod tests;
