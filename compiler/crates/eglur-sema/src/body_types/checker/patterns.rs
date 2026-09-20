use super::*;

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn type_match_pattern(
        &mut self,
        context: BodyContext<'_, '_>,
        pattern_id: PatternId,
        expected: TypeId,
    ) -> Result<Outcome<PatternCheck>, BodyTypingError> {
        let BodyContext {
            module,
            source,
            body,
            ..
        } = context;
        let pattern = body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?;
        match pattern.kind() {
            PatternKind::Binding(name) => {
                let binding = self.local_binding(module, body.id(), name.origin())?;
                Ok(Outcome::Known(PatternCheck::binding(binding, expected)))
            }
            PatternKind::Wildcard => Ok(Outcome::Known(PatternCheck::irrefutable())),
            PatternKind::Alias { name, pattern } => {
                let binding = self.local_binding(module, body.id(), name.origin())?;
                self.type_match_pattern(context, *pattern, expected)
                    .map(|outcome| outcome.map(|pattern| pattern.with_binding(binding, expected)))
            }
            PatternKind::Group(pattern) => self.type_match_pattern(context, *pattern, expected),
            PatternKind::Literal(literal) => {
                let actual =
                    self.type_literal(source, literal, TypeRequest::definitive(Some(expected)))?;
                match actual {
                    Outcome::Known(actual)
                        if self.signatures.types().is_assignable(actual.ty, expected)? =>
                    {
                        Ok(Outcome::Known(PatternCheck::partial()))
                    }
                    Outcome::Known(actual) => self
                        .pattern_type_mismatch(
                            source,
                            pattern.origin(),
                            "literal",
                            expected,
                            Some(actual.ty),
                        )
                        .map(Outcome::Poisoned),
                    other => Ok(other.map(|_| PatternCheck::partial())),
                }
            }
            PatternKind::Unit => {
                let unit = self.primitive(PrimitiveType::Unit)?;
                if expected == unit {
                    Ok(Outcome::Known(PatternCheck::irrefutable()))
                } else {
                    self.pattern_type_mismatch(source, pattern.origin(), "unit", expected, None)
                        .map(Outcome::Poisoned)
                }
            }
            PatternKind::Tuple(elements) => {
                self.type_tuple_pattern(context, pattern_id, elements, expected)
            }
            PatternKind::Variant { payload, .. } => {
                self.type_variant_pattern(context, pattern_id, payload, expected)
            }
            PatternKind::Or(_) | PatternKind::Struct { .. } => Ok(self.unsupported(
                TypecheckCapability::CheckAdvancedPatterns,
                source,
                pattern.origin(),
            )),
            PatternKind::Invalid => Ok(Outcome::Poisoned(
                source.invalid_poison(pattern.origin()).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin: pattern.origin(),
                    },
                )?,
            )),
        }
    }

    pub(super) fn type_tuple_pattern(
        &mut self,
        context: BodyContext<'_, '_>,
        pattern_id: PatternId,
        elements: &[PatternElement],
        expected: TypeId,
    ) -> Result<Outcome<PatternCheck>, BodyTypingError> {
        let source = context.source;
        let origin = context
            .body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?
            .origin();
        let TypeKind::Tuple(types) = self.signatures.types().kind(expected)?.clone() else {
            return self
                .pattern_type_mismatch(source, origin, "tuple", expected, None)
                .map(Outcome::Poisoned);
        };
        let explicit = elements
            .iter()
            .filter(|element| matches!(element, PatternElement::Pattern(_)))
            .count();
        let has_rest = elements
            .iter()
            .any(|element| matches!(element, PatternElement::Rest(_)));
        if (!has_rest && explicit != types.len()) || (has_rest && explicit > types.len()) {
            return self
                .type_error(
                    source,
                    origin,
                    "PATTERN_ARITY_MISMATCH",
                    "tuple pattern arity mismatch",
                    BTreeMap::from([
                        (
                            Box::<str>::from("expected_count"),
                            self.diagnostic_count(types.len())?,
                        ),
                        (
                            Box::<str>::from("actual_count"),
                            self.diagnostic_count(explicit)?,
                        ),
                    ]),
                )
                .map(Outcome::Poisoned);
        }
        let mut check = PatternCheck::irrefutable();
        let mut type_index = 0;
        for element in elements {
            match element {
                PatternElement::Pattern(pattern) => {
                    let child = self.type_match_pattern(context, *pattern, types[type_index])?;
                    type_index += 1;
                    match child {
                        Outcome::Known(child) => check.merge(child),
                        other => return Ok(other),
                    }
                }
                PatternElement::Rest(_) => type_index = types.len(),
            }
        }
        Ok(Outcome::Known(check))
    }

    pub(super) fn type_variant_pattern(
        &mut self,
        context: BodyContext<'_, '_>,
        pattern_id: PatternId,
        payload: &eglur_hir::PatternPayload,
        expected: TypeId,
    ) -> Result<Outcome<PatternCheck>, BodyTypingError> {
        let source = context.source;
        let pattern = context
            .body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?;
        let target = match self.pattern_constructor_target(
            context.module,
            source,
            pattern_id,
            pattern.origin(),
        )? {
            Outcome::Known(target) => target,
            Outcome::Poisoned(poison) => return Ok(Outcome::Poisoned(poison)),
            Outcome::Deferred => return Ok(Outcome::Deferred),
            Outcome::Unperformed | Outcome::NotApplicable => return Ok(Outcome::Unperformed),
        };
        if matches!(payload, eglur_hir::PatternPayload::Named(_))
            && target.kind.is_type_namespace_target()
        {
            return Ok(self.unsupported(
                TypecheckCapability::CheckAdvancedPatterns,
                source,
                pattern.origin(),
            ));
        }
        if target.kind != ResolvedTargetKind::VariantConstructor {
            return self
                .pattern_type_mismatch(source, pattern.origin(), "variant", expected, None)
                .map(Outcome::Poisoned);
        }
        let contract = self.pattern_constructor_contract(&target)?;
        let parameter_set = contract
            .generic_parameters
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut replacements = BTreeMap::new();
        let inferred =
            self.infer_constraints(contract.result, expected, &parameter_set, &mut replacements)?;
        if inferred == ConstraintOutcome::Conflict
            || !contract
                .generic_parameters
                .iter()
                .all(|parameter| replacements.contains_key(parameter))
        {
            return self
                .pattern_type_mismatch(
                    source,
                    pattern.origin(),
                    "variant",
                    expected,
                    Some(contract.result),
                )
                .map(Outcome::Poisoned);
        }
        let result = self
            .signatures
            .types_mut()
            .substitute(contract.result, &replacements)?;
        if result != expected {
            return self
                .pattern_type_mismatch(source, pattern.origin(), "variant", expected, Some(result))
                .map(Outcome::Poisoned);
        }
        let actual_form = ConstructorPayloadForm::from_pattern(payload);
        if contract.shape.form() != actual_form {
            let owner = self.target_label(&contract.declaration);
            let variant = self
                .target_name(&contract.declaration)
                .unwrap_or(owner.as_str())
                .to_owned();
            return self
                .type_error(
                    source,
                    pattern.origin(),
                    "PATTERN_PAYLOAD_FORM_MISMATCH",
                    "pattern payload form mismatch",
                    BTreeMap::from([
                        (Box::<str>::from("owner"), DiagnosticDataValue::from(owner)),
                        (
                            Box::<str>::from("variant"),
                            DiagnosticDataValue::from(variant),
                        ),
                        (
                            Box::<str>::from("expected_form"),
                            DiagnosticDataValue::from(contract.shape.form().spelling()),
                        ),
                        (
                            Box::<str>::from("actual_form"),
                            DiagnosticDataValue::from(actual_form.spelling()),
                        ),
                    ]),
                )
                .map(Outcome::Poisoned);
        }
        let payload_check = match (&contract.shape, payload) {
            (ConstructorShape::Absent, eglur_hir::PatternPayload::Absent) => {
                PatternCheck::irrefutable()
            }
            (ConstructorShape::Tuple(types), eglur_hir::PatternPayload::Positional(elements)) => {
                let explicit = elements
                    .iter()
                    .filter(|element| matches!(element, PatternElement::Pattern(_)))
                    .count();
                let has_rest = elements
                    .iter()
                    .any(|element| matches!(element, PatternElement::Rest(_)));
                if (!has_rest && explicit != types.len()) || (has_rest && explicit > types.len()) {
                    return self
                        .type_error(
                            source,
                            pattern.origin(),
                            "PATTERN_ARITY_MISMATCH",
                            "variant pattern arity mismatch",
                            BTreeMap::from([
                                (
                                    Box::<str>::from("expected_count"),
                                    self.diagnostic_count(types.len())?,
                                ),
                                (
                                    Box::<str>::from("actual_count"),
                                    self.diagnostic_count(explicit)?,
                                ),
                            ]),
                        )
                        .map(Outcome::Poisoned);
                }
                let mut check = PatternCheck::irrefutable();
                let mut type_index = 0;
                for element in elements {
                    match element {
                        PatternElement::Pattern(pattern) => {
                            let template = types[type_index];
                            type_index += 1;
                            let child_type = self
                                .signatures
                                .types_mut()
                                .substitute(template, &replacements)?;
                            match self.type_match_pattern(context, *pattern, child_type)? {
                                Outcome::Known(child) => check.merge(child),
                                other => return Ok(other),
                            }
                        }
                        PatternElement::Rest(_) => type_index = types.len(),
                    }
                }
                check
            }
            (ConstructorShape::Named(_), eglur_hir::PatternPayload::Named(_)) => {
                return Ok(self.unsupported(
                    TypecheckCapability::CheckAdvancedPatterns,
                    source,
                    pattern.origin(),
                ));
            }
            _ => {
                return Err(BodyTypingError::InconsistentConstructorPayloadForm {
                    expected: contract.shape.form().spelling(),
                    actual: actual_form.spelling(),
                });
            }
        };
        Ok(Outcome::Known(PatternCheck::variant(
            contract.declaration,
            payload_check,
        )))
    }

    pub(super) fn bind_match_local(
        &mut self,
        binding: LocalBindingId,
        ty: TypeId,
    ) -> Result<(), BodyTypingError> {
        let region = self.current_control_region()?.clone();
        match self.locals.state(&binding) {
            FactState::Known(existing) if existing.ty == ty && existing.region == region => Ok(()),
            FactState::Unperformed => self
                .locals
                .insert(
                    binding,
                    SemanticFact::Known(LocalTypeFact {
                        ty,
                        mutability: Mutability::Immutable,
                        region,
                    }),
                )
                .map_err(Into::into),
            FactState::Known(_) | FactState::Poisoned(_) | FactState::NotApplicable => {
                Err(BodyTypingError::InconsistentPatternBinding(binding))
            }
        }
    }

    pub(super) fn pattern_type_mismatch(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        pattern_kind: &'static str,
        expected: TypeId,
        actual: Option<TypeId>,
    ) -> Result<Poison, BodyTypingError> {
        let reason = if actual.is_some() {
            "typed_pattern"
        } else {
            "structural_form"
        };
        let mut data = BTreeMap::from([
            (
                Box::<str>::from("reason"),
                DiagnosticDataValue::from(reason),
            ),
            (
                Box::<str>::from("pattern_kind"),
                DiagnosticDataValue::from(pattern_kind),
            ),
            (
                Box::<str>::from("expected_type"),
                DiagnosticDataValue::from(self.format_type(expected)?),
            ),
        ]);
        if let Some(actual) = actual {
            data.insert(
                Box::<str>::from("actual_type"),
                DiagnosticDataValue::from(self.format_type(actual)?),
            );
        }
        self.type_error(
            source,
            origin,
            "PATTERN_TYPE_MISMATCH",
            "pattern type mismatch",
            data,
        )
    }

    pub(super) fn check_match_coverage(
        &mut self,
        source: &SemanticSource<'_>,
        scrutinee: TypeId,
        arms: &[(&eglur_hir::MatchArm, PatternCoverage)],
    ) -> Result<Outcome<()>, BodyTypingError> {
        let mut total = false;
        let mut variants = BTreeSet::new();
        for (arm, coverage) in arms {
            let unreachable = total
                || (!coverage.variants.is_empty()
                    && coverage
                        .variants
                        .iter()
                        .all(|variant| variants.contains(variant)));
            if unreachable {
                return self
                    .type_error(
                        source,
                        self.pattern_origin(source, arm.pattern)?,
                        "MATCH_UNREACHABLE_ARM",
                        "match arm is unreachable",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned);
            }
            if arm.guard.is_none() {
                total |= coverage.total;
                variants.extend(coverage.variants.iter().cloned());
            }
        }
        if total {
            return Ok(Outcome::Known(()));
        }
        let Some(universe) = self.match_variant_universe(scrutinee)? else {
            let origin = arms
                .first()
                .map(|(arm, _)| self.pattern_origin(source, arm.pattern))
                .transpose()?
                .ok_or(BodyTypingError::MissingMatchCoverageOrigin)?;
            self.demand_unimplemented(TypecheckCapability::AnalyzePatternCoverage, source, origin);
            return Ok(Outcome::Unperformed);
        };
        if universe.iter().all(|variant| variants.contains(variant)) {
            return Ok(Outcome::Known(()));
        }
        let origin = arms
            .first()
            .map(|(arm, _)| self.pattern_origin(source, arm.pattern))
            .transpose()?
            .ok_or(BodyTypingError::MissingMatchCoverageOrigin)?;
        self.type_error(
            source,
            origin,
            "MATCH_NON_EXHAUSTIVE",
            "match is non-exhaustive",
            BTreeMap::new(),
        )
        .map(Outcome::Poisoned)
    }

    pub(super) fn match_variant_universe(
        &self,
        scrutinee: TypeId,
    ) -> Result<Option<BTreeSet<CanonicalTarget>>, BodyTypingError> {
        let TypeKind::Nominal { head, .. } = self.signatures.types().kind(scrutinee)? else {
            return Ok(None);
        };
        let intrinsic = |left: LanguageItem, right: LanguageItem| {
            let left = self.signatures.language_items().declaration(left)?.clone();
            let right = self.signatures.language_items().declaration(right)?.clone();
            Some(BTreeSet::from([left, right]))
        };
        let variants = match head {
            NominalHead::Intrinsic(IntrinsicNominal::Opt) => {
                intrinsic(LanguageItem::OptSome, LanguageItem::OptNone)
            }
            NominalHead::Intrinsic(IntrinsicNominal::Res) => {
                intrinsic(LanguageItem::ResOk, LanguageItem::ResErr)
            }
            NominalHead::Declaration(_) => None,
            NominalHead::Intrinsic(IntrinsicNominal::Own | IntrinsicNominal::Box) => None,
        };
        Ok(variants)
    }

    pub(super) fn pattern_origin(
        &self,
        source: &SemanticSource<'_>,
        pattern: PatternId,
    ) -> Result<OriginId, BodyTypingError> {
        source
            .hir()
            .and_then(|hir| hir.body(pattern.body()).pattern(pattern))
            .map(eglur_hir::Pattern::origin)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern))
    }

    pub(super) fn unsupported_let_pattern(
        body: &Body,
        pattern_id: PatternId,
    ) -> Result<Option<OriginId>, BodyTypingError> {
        let pattern = body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?;
        match pattern.kind() {
            PatternKind::Binding(_)
            | PatternKind::Wildcard
            | PatternKind::Unit
            | PatternKind::Invalid => Ok(None),
            PatternKind::Group(inner) => Self::unsupported_let_pattern(body, *inner),
            PatternKind::Tuple(elements) => {
                for element in elements {
                    let PatternElement::Pattern(inner) = element else {
                        return Ok(Some(pattern.origin()));
                    };
                    if let Some(origin) = Self::unsupported_let_pattern(body, *inner)? {
                        return Ok(Some(origin));
                    }
                }
                Ok(None)
            }
            _ => Ok(Some(pattern.origin())),
        }
    }

    pub(super) fn first_pattern_duplicate(
        &self,
        body: &Body,
        pattern: PatternId,
    ) -> Result<Option<(NameKey, OriginId)>, BodyTypingError> {
        let mut seen = BTreeMap::new();
        self.first_pattern_duplicate_with(body, pattern, &mut seen)
    }

    pub(super) fn first_pattern_duplicate_with(
        &self,
        body: &Body,
        pattern_id: PatternId,
        seen: &mut BTreeMap<NameKey, OriginId>,
    ) -> Result<Option<(NameKey, OriginId)>, BodyTypingError> {
        let pattern = body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?;
        match pattern.kind() {
            PatternKind::Binding(name) => Ok(Self::duplicate_pattern_name(name, seen)),
            PatternKind::Alias { name, pattern } => {
                if let Some(duplicate) = Self::duplicate_pattern_name(name, seen) {
                    return Ok(Some(duplicate));
                }
                self.first_pattern_duplicate_with(body, *pattern, seen)
            }
            PatternKind::Group(pattern) => self.first_pattern_duplicate_with(body, *pattern, seen),
            PatternKind::Tuple(elements) => {
                for element in elements {
                    if let PatternElement::Pattern(pattern) = element
                        && let Some(duplicate) =
                            self.first_pattern_duplicate_with(body, *pattern, seen)?
                    {
                        return Ok(Some(duplicate));
                    }
                }
                Ok(None)
            }
            PatternKind::Variant { payload, .. } => match payload {
                eglur_hir::PatternPayload::Positional(elements) => {
                    for element in elements {
                        if let PatternElement::Pattern(pattern) = element
                            && let Some(duplicate) =
                                self.first_pattern_duplicate_with(body, *pattern, seen)?
                        {
                            return Ok(Some(duplicate));
                        }
                    }
                    Ok(None)
                }
                eglur_hir::PatternPayload::Named(fields) => {
                    self.first_field_pattern_duplicate(body, fields, seen)
                }
                eglur_hir::PatternPayload::Absent => Ok(None),
            },
            PatternKind::Struct { fields, .. } => {
                self.first_field_pattern_duplicate(body, fields, seen)
            }
            PatternKind::Or(alternatives) => {
                let base = seen.clone();
                for alternative in alternatives {
                    let mut alternative_seen = base.clone();
                    if let Some(duplicate) = self.first_pattern_duplicate_with(
                        body,
                        *alternative,
                        &mut alternative_seen,
                    )? {
                        return Ok(Some(duplicate));
                    }
                }
                Ok(None)
            }
            PatternKind::Wildcard
            | PatternKind::Literal(_)
            | PatternKind::Unit
            | PatternKind::Invalid => Ok(None),
        }
    }

    pub(super) fn first_field_pattern_duplicate(
        &self,
        body: &Body,
        fields: &[eglur_hir::FieldPattern],
        seen: &mut BTreeMap<NameKey, OriginId>,
    ) -> Result<Option<(NameKey, OriginId)>, BodyTypingError> {
        for field in fields {
            match field {
                eglur_hir::FieldPattern::Field {
                    name,
                    pattern: None,
                    ..
                } => {
                    if let Some(duplicate) = Self::duplicate_pattern_name(name, seen) {
                        return Ok(Some(duplicate));
                    }
                }
                eglur_hir::FieldPattern::Field {
                    pattern: Some(pattern),
                    ..
                } => {
                    if let Some(duplicate) =
                        self.first_pattern_duplicate_with(body, *pattern, seen)?
                    {
                        return Ok(Some(duplicate));
                    }
                }
                eglur_hir::FieldPattern::Rest(_) => {}
            }
        }
        Ok(None)
    }

    pub(super) fn duplicate_pattern_name(
        name: &Name,
        seen: &mut BTreeMap<NameKey, OriginId>,
    ) -> Option<(NameKey, OriginId)> {
        let name_key = NameKey::from_hir(name)?;
        if seen.insert(name_key.clone(), name.origin()).is_some() {
            Some((name_key, name.origin()))
        } else {
            None
        }
    }

    pub(super) fn bind_pattern(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        body: &Body,
        pattern_id: PatternId,
        ty: TypeId,
        mode: PatternBindingMode,
    ) -> Result<Outcome<()>, BodyTypingError> {
        let mutability = mode.mutability();
        let pattern = body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?;
        let origin = pattern.origin();
        let kind = pattern.kind().clone();
        match kind {
            PatternKind::Binding(name) => {
                let binding = self.local_binding(module, body.id(), name.origin())?;
                let region = self.current_control_region()?.clone();
                self.locals.insert(
                    binding,
                    SemanticFact::Known(LocalTypeFact {
                        ty,
                        mutability,
                        region,
                    }),
                )?;
                Ok(Outcome::Known(()))
            }
            PatternKind::Wildcard => {
                if self.signatures.types().ownership_root(ty)?.is_some() {
                    self.type_error(
                        source,
                        origin,
                        "OWNERSHIP_DISCARD_FORBIDDEN",
                        "ownership value discard is forbidden",
                        BTreeMap::new(),
                    )
                    .map(Outcome::Poisoned)
                } else {
                    Ok(Outcome::Known(()))
                }
            }
            PatternKind::Group(inner) if matches!(mode, PatternBindingMode::Let(_)) => {
                self.bind_pattern(module, source, body, inner, ty, mode)
            }
            PatternKind::Unit if matches!(mode, PatternBindingMode::Let(_)) => {
                let unit = self.primitive(PrimitiveType::Unit)?;
                if ty == unit {
                    Ok(Outcome::Known(()))
                } else {
                    let data = BTreeMap::from([
                        (
                            Box::<str>::from("reason"),
                            DiagnosticDataValue::from("unit_required"),
                        ),
                        (
                            Box::<str>::from("pattern_kind"),
                            DiagnosticDataValue::from("unit"),
                        ),
                        (
                            Box::<str>::from("actual_type"),
                            DiagnosticDataValue::from(self.format_type(ty)?),
                        ),
                        (
                            Box::<str>::from("expected_type"),
                            DiagnosticDataValue::from("Unit"),
                        ),
                    ]);
                    self.type_error(
                        source,
                        origin,
                        "LET_PATTERN_TYPE_MISMATCH",
                        "let Unit pattern type mismatch",
                        data,
                    )
                    .map(Outcome::Poisoned)
                }
            }
            PatternKind::Tuple(elements) if matches!(mode, PatternBindingMode::Let(_)) => {
                if elements
                    .iter()
                    .any(|element| matches!(element, PatternElement::Rest(_)))
                {
                    return self
                        .type_error(
                            source,
                            origin,
                            "LET_PATTERN_UNSUPPORTED",
                            "let pattern is unsupported in v0.1",
                            BTreeMap::new(),
                        )
                        .map(Outcome::Poisoned);
                }
                let TypeKind::Tuple(types) = self.signatures.types().kind(ty)?.clone() else {
                    let data = BTreeMap::from([
                        (
                            Box::<str>::from("reason"),
                            DiagnosticDataValue::from("tuple_required"),
                        ),
                        (
                            Box::<str>::from("pattern_kind"),
                            DiagnosticDataValue::from("tuple"),
                        ),
                        (
                            Box::<str>::from("actual_type"),
                            DiagnosticDataValue::from(self.format_type(ty)?),
                        ),
                    ]);
                    return self
                        .type_error(
                            source,
                            origin,
                            "LET_PATTERN_TYPE_MISMATCH",
                            "let tuple pattern type mismatch: initializer is not a tuple",
                            data,
                        )
                        .map(Outcome::Poisoned);
                };
                if elements.len() != types.len() {
                    let expected_count = self.diagnostic_count(elements.len())?;
                    let actual_count = self.diagnostic_count(types.len())?;
                    let data = BTreeMap::from([
                        (
                            Box::<str>::from("reason"),
                            DiagnosticDataValue::from("tuple_arity"),
                        ),
                        (
                            Box::<str>::from("pattern_kind"),
                            DiagnosticDataValue::from("tuple"),
                        ),
                        (
                            Box::<str>::from("actual_type"),
                            DiagnosticDataValue::from(self.format_type(ty)?),
                        ),
                        (Box::<str>::from("expected_count"), expected_count),
                        (Box::<str>::from("actual_count"), actual_count),
                    ]);
                    return self
                        .type_error(
                            source,
                            origin,
                            "LET_PATTERN_TYPE_MISMATCH",
                            "let tuple pattern type mismatch: arity differs",
                            data,
                        )
                        .map(Outcome::Poisoned);
                }
                let mut outcome = Outcome::Known(());
                for (element, element_ty) in elements.iter().zip(types.iter().copied()) {
                    let PatternElement::Pattern(element) = element else {
                        unreachable!("rest elements were rejected before binding")
                    };
                    let bound =
                        self.bind_pattern(module, source, body, *element, element_ty, mode)?;
                    outcome = self.merge_unit(outcome, bound)?;
                }
                Ok(outcome)
            }
            PatternKind::Invalid => Ok(Outcome::Poisoned(
                source.invalid_poison(origin).cloned().ok_or(
                    BodyTypingError::MissingOriginSpan {
                        file: source.file(),
                        origin,
                    },
                )?,
            )),
            _ => {
                let (code, message) = match mode {
                    PatternBindingMode::Parameter => (
                        "PARAM_PATTERN_UNSUPPORTED",
                        "parameter pattern is unsupported in v0.1",
                    ),
                    PatternBindingMode::Let(_) => (
                        "LET_PATTERN_UNSUPPORTED",
                        "let pattern is unsupported in v0.1",
                    ),
                };
                self.type_error(source, origin, code, message, BTreeMap::new())
                    .map(Outcome::Poisoned)
            }
        }
    }

    pub(super) fn bind_pattern_poison(
        &mut self,
        module: ModuleId,
        body: &Body,
        pattern_id: PatternId,
        poison: Poison,
    ) -> Result<(), BodyTypingError> {
        let pattern = body
            .pattern(pattern_id)
            .ok_or(BodyTypingError::WrongBodyPattern(pattern_id))?;
        match pattern.kind() {
            PatternKind::Binding(name) => {
                let binding = self.local_binding(module, body.id(), name.origin())?;
                self.poison_pattern_local(binding, poison)?;
            }
            PatternKind::Alias { name, pattern } => {
                let binding = self.local_binding(module, body.id(), name.origin())?;
                self.poison_pattern_local(binding, poison.clone())?;
                self.bind_pattern_poison(module, body, *pattern, poison)?;
            }
            PatternKind::Or(alternatives) => {
                for alternative in alternatives {
                    self.bind_pattern_poison(module, body, *alternative, poison.clone())?;
                }
            }
            PatternKind::Tuple(alternatives) => {
                for alternative in alternatives {
                    if let PatternElement::Pattern(pattern) = alternative {
                        self.bind_pattern_poison(module, body, *pattern, poison.clone())?;
                    }
                }
            }
            PatternKind::Group(inner) => {
                self.bind_pattern_poison(module, body, *inner, poison)?;
            }
            PatternKind::Struct { fields, .. } => {
                for field in fields {
                    match field {
                        eglur_hir::FieldPattern::Field {
                            name,
                            pattern: None,
                            ..
                        } => {
                            let binding = self.local_binding(module, body.id(), name.origin())?;
                            self.poison_pattern_local(binding, poison.clone())?;
                        }
                        eglur_hir::FieldPattern::Field {
                            pattern: Some(pattern),
                            ..
                        } => {
                            self.bind_pattern_poison(module, body, *pattern, poison.clone())?;
                        }
                        eglur_hir::FieldPattern::Rest(_) => {}
                    }
                }
            }
            PatternKind::Variant { payload, .. } => match payload {
                eglur_hir::PatternPayload::Positional(elements) => {
                    for element in elements {
                        if let PatternElement::Pattern(pattern) = element {
                            self.bind_pattern_poison(module, body, *pattern, poison.clone())?;
                        }
                    }
                }
                eglur_hir::PatternPayload::Named(fields) => {
                    for field in fields {
                        match field {
                            eglur_hir::FieldPattern::Field {
                                name,
                                pattern: None,
                                ..
                            } => {
                                let binding =
                                    self.local_binding(module, body.id(), name.origin())?;
                                self.poison_pattern_local(binding, poison.clone())?;
                            }
                            eglur_hir::FieldPattern::Field {
                                pattern: Some(pattern),
                                ..
                            } => {
                                self.bind_pattern_poison(module, body, *pattern, poison.clone())?;
                            }
                            eglur_hir::FieldPattern::Rest(_) => {}
                        }
                    }
                }
                eglur_hir::PatternPayload::Absent => {}
            },
            PatternKind::Wildcard
            | PatternKind::Literal(_)
            | PatternKind::Unit
            | PatternKind::Invalid => {}
        }
        Ok(())
    }

    pub(super) fn poison_pattern_local(
        &mut self,
        binding: LocalBindingId,
        poison: Poison,
    ) -> Result<(), BodyTypingError> {
        match self.locals.state(&binding) {
            FactState::Unperformed => self
                .locals
                .insert(binding, SemanticFact::Poisoned(poison))
                .map_err(Into::into),
            FactState::Poisoned(_) => Ok(()),
            FactState::Known(_) | FactState::NotApplicable => {
                Err(BodyTypingError::InconsistentPatternBinding(binding))
            }
        }
    }
}
