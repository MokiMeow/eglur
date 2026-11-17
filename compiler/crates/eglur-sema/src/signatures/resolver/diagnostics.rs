use super::*;

impl<'input, 'hir> SignatureResolver<'input, 'hir> {
    pub(super) fn record_type_occurrence(
        &mut self,
        source: &SemanticSource<'hir>,
        type_ref: TypeRefId,
        origin: OriginId,
        written_target: Option<CanonicalTarget>,
        result: Resolution<TypeId>,
    ) -> Result<Resolution<TypeId>, SignatureResolutionError> {
        let fact = match &result {
            Resolution::Known(ty) => Some(SemanticFact::Known(ResolvedTypeOccurrence {
                resolved: ResolvedType::Type(*ty),
                origin,
                written_target,
            })),
            Resolution::Poisoned(poison) => Some(SemanticFact::Poisoned(poison.clone())),
            Resolution::Unperformed => None,
        };
        if let Some(fact) = fact {
            self.occurrences
                .insert(TypeOccurrenceId::new(source.file(), type_ref), fact)?;
        }
        Ok(result)
    }

    pub(super) fn record_trait_occurrence(
        &mut self,
        source: &SemanticSource<'hir>,
        type_ref: TypeRefId,
        origin: OriginId,
        written_target: Option<CanonicalTarget>,
        result: Resolution<TraitRef>,
    ) -> Result<Resolution<TraitRef>, SignatureResolutionError> {
        let fact = match &result {
            Resolution::Known(trait_ref) => Some(SemanticFact::Known(ResolvedTypeOccurrence {
                resolved: ResolvedType::Trait(trait_ref.clone()),
                origin,
                written_target,
            })),
            Resolution::Poisoned(poison) => Some(SemanticFact::Poisoned(poison.clone())),
            Resolution::Unperformed => None,
        };
        if let Some(fact) = fact {
            self.occurrences
                .insert(TypeOccurrenceId::new(source.file(), type_ref), fact)?;
        }
        Ok(result)
    }

    pub(super) fn invalid_resolution<T>(
        &self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
    ) -> Result<Resolution<T>, SignatureResolutionError> {
        source
            .invalid_poison(origin)
            .cloned()
            .map(Resolution::Poisoned)
            .ok_or(SignatureResolutionError::MissingInvalidPoison {
                file: source.file(),
                origin,
            })
    }

    pub(super) fn invalid_path_resolution<T>(
        &self,
        source: &SemanticSource<'hir>,
        path: &eglur_hir::Path,
    ) -> Result<Resolution<T>, SignatureResolutionError> {
        let mut poison = None;
        for segment in path.segments().iter().filter(|segment| !segment.is_valid()) {
            let found = source.invalid_poison(segment.origin()).ok_or(
                SignatureResolutionError::MissingInvalidPoison {
                    file: source.file(),
                    origin: segment.origin(),
                },
            )?;
            merge_poison(&mut poison, found)?;
        }
        poison
            .map(Resolution::Poisoned)
            .ok_or(SignatureResolutionError::MissingInvalidPoison {
                file: source.file(),
                origin: path.origin(),
            })
    }

    pub(super) fn source_error(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        code: &'static str,
        message: impl Into<String>,
        data: BTreeMap<Box<str>, DiagnosticDataValue>,
    ) -> Result<Poison, SignatureResolutionError> {
        Ok(record_type_error(
            self.diagnostics,
            code,
            message,
            self.required_span(source, origin)?,
            data,
        )?)
    }

    pub(super) fn source_error_for_obligation(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        key: SourceObligationKey,
        code: &'static str,
        message: impl Into<String>,
        data: BTreeMap<Box<str>, DiagnosticDataValue>,
    ) -> Result<Poison, SignatureResolutionError> {
        Ok(record_type_error_for_obligation(
            self.diagnostics,
            key,
            code,
            message,
            self.required_span(source, origin)?,
            data,
        )?)
    }

    pub(super) fn record_arity_error(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        expected: usize,
        actual: usize,
    ) -> Result<Poison, SignatureResolutionError> {
        self.source_error(
            source,
            origin,
            "TYPE_ARG_ARITY_MISMATCH",
            format!("type argument arity mismatch: expected {expected}, found {actual}"),
            BTreeMap::from([
                (
                    Box::<str>::from("expected_count"),
                    diagnostic_count(expected),
                ),
                (Box::<str>::from("actual_count"), diagnostic_count(actual)),
            ]),
        )
    }

    pub(super) fn projection_error(
        &mut self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
        reason: &str,
    ) -> Result<Poison, SignatureResolutionError> {
        self.source_error(
            source,
            origin,
            "ASSOCIATED_TYPE_PROJECTION_INVALID",
            format!("associated type projection is invalid: {reason}"),
            BTreeMap::from([(
                Box::<str>::from("reason"),
                DiagnosticDataValue::from(reason),
            )]),
        )
    }

    pub(super) fn required_span(
        &self,
        source: &SemanticSource<'hir>,
        origin: OriginId,
    ) -> Result<SourceSpan, SignatureResolutionError> {
        source
            .origin_span(origin)
            .cloned()
            .ok_or(SignatureResolutionError::MissingOriginSpan {
                file: source.file(),
                origin,
            })
    }

    pub(super) fn source_hir(
        &self,
        file: FileId,
    ) -> Result<&'hir HirFile, SignatureResolutionError> {
        self.sources
            .get(&file)
            .and_then(|source| source.hir())
            .ok_or(SignatureResolutionError::MissingHir(file))
    }
}
