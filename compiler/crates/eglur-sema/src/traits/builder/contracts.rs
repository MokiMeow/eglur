use super::*;

#[derive(Clone, Debug)]
enum CheckedCallable {
    Known(CallableSignature),
    Poisoned(Poison),
}

#[derive(Clone, Debug)]
enum ContractMatch {
    Matches,
    Mismatch,
    Poisoned(Poison),
    InvalidArgument(crate::ImplArgumentViolation),
}

pub(super) fn validate_impl_methods(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    signatures: &mut DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    environment: &mut TraitEnvironment,
) -> Result<(), TraitEnvironmentError> {
    let mut source_map = BTreeMap::new();
    for source in sources {
        if source_map.insert(source.file(), source).is_some() {
            return Err(TraitEnvironmentError::DuplicateSource(source.file()));
        }
    }
    let implementations = environment.implementation_validation_inputs();

    for input in implementations {
        let implementation = input.implementation;
        let header = input.header;
        let existing_poison = input.existing_poison;
        if existing_poison.is_some() {
            continue;
        }
        let trait_target = header.trait_ref().declaration().clone();
        let trait_parameters = match signatures.declaration(&trait_target) {
            FactState::Known(DeclarationSignature::Trait {
                generic_parameters, ..
            }) => generic_parameters.to_vec(),
            FactState::Poisoned(poison) => {
                environment.poison_implementation(&implementation, poison.clone());
                continue;
            }
            FactState::Unperformed => {
                return Err(TraitEnvironmentError::UnavailableMethodSignature(
                    trait_target,
                ));
            }
            FactState::Known(_) | FactState::NotApplicable => {
                return Err(TraitEnvironmentError::UnexpectedMethodSignature(
                    trait_target,
                ));
            }
        };
        let trait_methods = signatures.trait_methods(&trait_target).to_vec();
        let mut targets = BTreeMap::new();
        let mut impl_poison = None;

        if let CanonicalTarget::External(_) = implementation {
            validate_trusted_impl_methods(
                environment,
                signatures,
                &implementation,
                &header,
                &trait_target,
                &trait_parameters,
                &trait_methods,
                &mut targets,
            )?;
            environment.extend_method_targets(targets);
            continue;
        }
        let CanonicalTarget::Source(implementation_id) = implementation else {
            unreachable!("canonical targets are source or trusted")
        };
        let impl_methods = child_methods(collection, implementation_id);

        for trait_method in trait_methods {
            let trait_method_target = trait_method.declaration().clone();
            let trait_callable = match checked_callable(signatures, &trait_method_target)? {
                CheckedCallable::Known(callable) => callable,
                CheckedCallable::Poisoned(poison) => {
                    merge_optional_poison(&mut impl_poison, &poison)?;
                    continue;
                }
            };
            let same_base = impl_methods
                .iter()
                .copied()
                .filter(|method| {
                    collection
                        .declaration(*method)
                        .and_then(crate::Declaration::name)
                        .is_some_and(|name| name.text() == trait_method.name().text())
                })
                .collect::<Vec<_>>();
            if same_base.is_empty() {
                if trait_method.has_default() {
                    targets.insert(
                        (implementation.clone(), trait_method_target.clone()),
                        trait_method_target,
                    );
                } else {
                    let poison = record_missing_impl_method(
                        collection,
                        &source_map,
                        signatures,
                        diagnostics,
                        implementation_id,
                        &header,
                        &trait_method,
                    )?;
                    merge_optional_poison(&mut impl_poison, &poison)?;
                }
                continue;
            }

            let exact = same_base
                .iter()
                .copied()
                .filter(|method| {
                    collection
                        .declaration(*method)
                        .and_then(crate::Declaration::name)
                        .is_some_and(|name| name.is_bang() == trait_method.name().is_bang())
                })
                .collect::<Vec<_>>();
            let candidates = if exact.is_empty() { &same_base } else { &exact };
            let mut matching = Vec::new();
            let mut known = Vec::new();
            let mut candidate_poison = None;
            for &candidate in candidates {
                let candidate_target = CanonicalTarget::Source(candidate);
                let callable = match checked_callable(signatures, &candidate_target)? {
                    CheckedCallable::Known(callable) => callable,
                    CheckedCallable::Poisoned(poison) => {
                        merge_optional_poison(&mut candidate_poison, &poison)?;
                        continue;
                    }
                };
                match method_contract_matches(
                    environment,
                    signatures,
                    &implementation,
                    &header,
                    &trait_target,
                    &trait_parameters,
                    &trait_callable,
                    &callable,
                )? {
                    ContractMatch::Matches => matching.push(candidate),
                    ContractMatch::Mismatch => {}
                    ContractMatch::InvalidArgument(violation) => {
                        let (code, message) =
                            crate::diagnostic::impl_argument_diagnostic(violation);
                        let poison = crate::diagnostic::record_type_error(
                            diagnostics,
                            code,
                            message,
                            source_declaration_span(collection, &source_map, candidate)?,
                            BTreeMap::new(),
                        )?;
                        merge_optional_poison(&mut candidate_poison, &poison)?;
                    }
                    ContractMatch::Poisoned(poison) => {
                        merge_optional_poison(&mut candidate_poison, &poison)?;
                    }
                }
                known.push((candidate, callable));
            }
            if let Some(poison) = candidate_poison {
                merge_optional_poison(&mut impl_poison, &poison)?;
                continue;
            }
            match matching.as_slice() {
                [method] if exact.is_empty() => {
                    let callable = known
                        .iter()
                        .find(|(candidate, _)| candidate == method)
                        .map(|(_, callable)| callable)
                        .expect("matching callable was retained");
                    let poison = record_trait_effect_mismatch(
                        collection,
                        &source_map,
                        signatures,
                        diagnostics,
                        implementation_id,
                        &header,
                        &trait_method,
                        &trait_callable,
                        *method,
                        callable,
                    )?;
                    merge_optional_poison(&mut impl_poison, &poison)?;
                }
                [method] => {
                    targets.insert(
                        (implementation.clone(), trait_method_target),
                        CanonicalTarget::Source(*method),
                    );
                }
                [] => {
                    let Some((method, callable)) = known.first() else {
                        return Err(TraitEnvironmentError::UnavailableMethodSignature(
                            CanonicalTarget::Source(same_base[0]),
                        ));
                    };
                    let poison = record_trait_impl_signature_mismatch(
                        collection,
                        &source_map,
                        signatures,
                        diagnostics,
                        implementation_id,
                        &header,
                        &trait_method,
                        &trait_callable,
                        *method,
                        callable,
                    )?;
                    merge_optional_poison(&mut impl_poison, &poison)?;
                }
                _ => {
                    return Err(TraitEnvironmentError::AmbiguousCheckedMethodContract {
                        implementation,
                        method: trait_method_target,
                    });
                }
            }
        }

        if let Some(poison) = impl_poison {
            environment.poison_implementation(&implementation, poison);
        } else {
            environment.extend_method_targets(targets);
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_trusted_impl_methods(
    environment: &mut TraitEnvironment,
    signatures: &mut DeclarationSignatures,
    implementation: &CanonicalTarget,
    header: &TraitImplHeader,
    trait_target: &CanonicalTarget,
    trait_parameters: &[GenericParameterId],
    trait_methods: &[TraitMethodSignature],
    targets: &mut BTreeMap<(CanonicalTarget, CanonicalTarget), CanonicalTarget>,
) -> Result<(), TraitEnvironmentError> {
    let declared = signatures.declared_impl_methods(implementation).to_vec();
    for trait_method in trait_methods {
        let expected = match checked_callable(signatures, trait_method.declaration())? {
            CheckedCallable::Known(callable) => callable,
            CheckedCallable::Poisoned(_) => {
                return Err(TraitEnvironmentError::InvalidTrustedMethodContract {
                    implementation: implementation.clone(),
                    method: trait_method.declaration().clone(),
                    reason: "poisoned trait method",
                });
            }
        };
        let selected = declared
            .iter()
            .find(|method| method.trait_method() == trait_method.declaration())
            .map(DeclaredImplMethod::callable)
            .or_else(|| {
                trait_method
                    .has_default()
                    .then_some(trait_method.declaration())
            })
            .ok_or_else(|| TraitEnvironmentError::InvalidTrustedMethodContract {
                implementation: implementation.clone(),
                method: trait_method.declaration().clone(),
                reason: "missing required method",
            })?
            .clone();
        let actual = match checked_callable(signatures, &selected)? {
            CheckedCallable::Known(callable) => callable,
            CheckedCallable::Poisoned(_) => {
                return Err(TraitEnvironmentError::InvalidTrustedMethodContract {
                    implementation: implementation.clone(),
                    method: trait_method.declaration().clone(),
                    reason: "poisoned impl method",
                });
            }
        };
        if expected.is_bang() != actual.is_bang() {
            return Err(TraitEnvironmentError::InvalidTrustedMethodContract {
                implementation: implementation.clone(),
                method: trait_method.declaration().clone(),
                reason: "effect marker mismatch",
            });
        }
        if !matches!(
            method_contract_matches(
                environment,
                signatures,
                implementation,
                header,
                trait_target,
                trait_parameters,
                &expected,
                &actual,
            )?,
            ContractMatch::Matches
        ) {
            return Err(TraitEnvironmentError::InvalidTrustedMethodContract {
                implementation: implementation.clone(),
                method: trait_method.declaration().clone(),
                reason: "signature mismatch",
            });
        }
        targets.insert(
            (implementation.clone(), trait_method.declaration().clone()),
            selected,
        );
    }
    Ok(())
}

fn child_methods(collection: &DeclarationCollection, owner: DefId) -> Vec<DefId> {
    collection
        .module(owner.module())
        .into_iter()
        .flat_map(crate::ModuleDeclarations::declarations)
        .filter(|declaration| {
            declaration.parent() == Some(owner) && declaration.kind() == DeclarationKind::Function
        })
        .map(crate::Declaration::id)
        .collect()
}

fn declaration_name(
    collection: &DeclarationCollection,
    declaration: DefId,
) -> Result<&NameKey, TraitEnvironmentError> {
    collection
        .declaration(declaration)
        .ok_or(TraitEnvironmentError::UnknownSourceDeclaration(declaration))?
        .name()
        .ok_or(TraitEnvironmentError::MissingMethodName(declaration))
}

fn checked_callable(
    signatures: &DeclarationSignatures,
    target: &CanonicalTarget,
) -> Result<CheckedCallable, TraitEnvironmentError> {
    match signatures.declaration(target) {
        FactState::Known(DeclarationSignature::Callable(callable)) => signatures
            .callable(*callable)
            .cloned()
            .map(CheckedCallable::Known)
            .ok_or(TraitEnvironmentError::UnknownCallableSignature(*callable)),
        FactState::Poisoned(poison) => Ok(CheckedCallable::Poisoned(poison.clone())),
        FactState::Unperformed => Err(TraitEnvironmentError::UnavailableMethodSignature(
            target.clone(),
        )),
        FactState::Known(_) | FactState::NotApplicable => Err(
            TraitEnvironmentError::UnexpectedMethodSignature(target.clone()),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn method_contract_matches(
    environment: &mut TraitEnvironment,
    signatures: &mut DeclarationSignatures,
    implementation: &CanonicalTarget,
    header: &TraitImplHeader,
    trait_target: &CanonicalTarget,
    trait_parameters: &[GenericParameterId],
    expected: &CallableSignature,
    actual: &CallableSignature,
) -> Result<ContractMatch, TraitEnvironmentError> {
    if expected.generic_parameters().len() != actual.generic_parameters().len()
        || expected.parameters().len() != actual.parameters().len()
    {
        return Ok(ContractMatch::Mismatch);
    }
    let trait_arguments = signatures
        .types()
        .substitution(header.trait_ref().substitution())?
        .to_vec();
    if trait_parameters.len() != trait_arguments.len() {
        return Err(TraitEnvironmentError::TraitMethodGenericArityMismatch {
            implementation: implementation.clone(),
            parameters: trait_parameters.len(),
            arguments: trait_arguments.len(),
        });
    }
    let mut replacements = trait_parameters
        .iter()
        .cloned()
        .zip(trait_arguments)
        .collect::<BTreeMap<_, _>>();
    for (expected, actual) in expected
        .generic_parameters()
        .iter()
        .zip(actual.generic_parameters())
    {
        let actual = signatures.types_mut().generic_parameter(actual.clone())?;
        replacements.insert(expected.clone(), actual);
    }

    for (expected_parameter, actual_parameter) in
        expected.parameters().iter().zip(actual.parameters())
    {
        let expected_type = specialize_contract_type(
            environment,
            signatures,
            implementation,
            expected_parameter.ty(),
            &replacements,
            trait_target,
            header.target(),
        )?;
        let actual_type = normalize_contract_type(
            environment,
            signatures,
            implementation,
            actual_parameter.ty(),
        )?;
        match (expected_type, actual_type) {
            (ContractType::Known(expected), ContractType::Known(actual)) if expected == actual => {}
            (ContractType::Poisoned(poison), _) | (_, ContractType::Poisoned(poison)) => {
                return Ok(ContractMatch::Poisoned(poison));
            }
            (ContractType::InvalidArgument(violation), _)
            | (_, ContractType::InvalidArgument(violation)) => {
                return Ok(ContractMatch::InvalidArgument(violation));
            }
            _ => return Ok(ContractMatch::Mismatch),
        }
    }
    let expected_result = specialize_contract_type(
        environment,
        signatures,
        implementation,
        expected.result(),
        &replacements,
        trait_target,
        header.target(),
    )?;
    let actual_result =
        normalize_contract_type(environment, signatures, implementation, actual.result())?;
    match (expected_result, actual_result) {
        (ContractType::Known(expected), ContractType::Known(actual)) if expected == actual => {}
        (ContractType::Poisoned(poison), _) | (_, ContractType::Poisoned(poison)) => {
            return Ok(ContractMatch::Poisoned(poison));
        }
        (ContractType::InvalidArgument(violation), _)
        | (_, ContractType::InvalidArgument(violation)) => {
            return Ok(ContractMatch::InvalidArgument(violation));
        }
        _ => return Ok(ContractMatch::Mismatch),
    }

    for (expected_parameter, actual_parameter) in expected
        .generic_parameters()
        .iter()
        .zip(actual.generic_parameters())
    {
        let expected_bounds = match checked_generic_bounds(signatures, expected_parameter)? {
            CheckedBounds::Known(bounds) => bounds,
            CheckedBounds::Poisoned(poison) => return Ok(ContractMatch::Poisoned(poison)),
        };
        let actual_bounds = match checked_generic_bounds(signatures, actual_parameter)? {
            CheckedBounds::Known(bounds) => bounds,
            CheckedBounds::Poisoned(poison) => return Ok(ContractMatch::Poisoned(poison)),
        };
        let expected_bounds = normalize_contract_bounds(
            environment,
            signatures,
            implementation,
            &expected_bounds,
            &replacements,
            Some((trait_target, header.target())),
        )?;
        let actual_bounds = normalize_contract_bounds(
            environment,
            signatures,
            implementation,
            &actual_bounds,
            &BTreeMap::new(),
            None,
        )?;
        match (expected_bounds, actual_bounds) {
            (ContractBounds::Known(expected), ContractBounds::Known(actual))
                if expected == actual => {}
            (ContractBounds::Poisoned(poison), _) | (_, ContractBounds::Poisoned(poison)) => {
                return Ok(ContractMatch::Poisoned(poison));
            }
            (ContractBounds::InvalidArgument(violation), _)
            | (_, ContractBounds::InvalidArgument(violation)) => {
                return Ok(ContractMatch::InvalidArgument(violation));
            }
            _ => return Ok(ContractMatch::Mismatch),
        }
    }
    Ok(ContractMatch::Matches)
}

pub(crate) struct CheckedMethodBound {
    pub(crate) parameter: GenericParameterId,
    pub(crate) bound_index: usize,
    pub(crate) obligation: TraitObligation,
}

pub(crate) fn checked_method_bound_mapping(
    environment: &mut TraitEnvironment,
    signatures: &mut DeclarationSignatures,
    implementation: &CanonicalTarget,
    header: &TraitImplHeader,
    trait_method: &CallableSignature,
    override_method: &CallableSignature,
) -> Result<Option<Vec<CheckedMethodBound>>, TraitEnvironmentError> {
    let trait_target = header.trait_ref().declaration();
    let trait_parameters = match signatures.declaration(trait_target) {
        FactState::Known(DeclarationSignature::Trait {
            generic_parameters, ..
        }) => generic_parameters.to_vec(),
        _ => return Ok(None),
    };
    let trait_arguments = signatures
        .types()
        .substitution(header.trait_ref().substitution())?
        .to_vec();
    if trait_parameters.len() != trait_arguments.len()
        || trait_method.generic_parameters().len() != override_method.generic_parameters().len()
    {
        return Ok(None);
    }
    let mut replacements = trait_parameters
        .into_iter()
        .zip(trait_arguments)
        .collect::<BTreeMap<_, _>>();
    for (expected, actual) in trait_method
        .generic_parameters()
        .iter()
        .zip(override_method.generic_parameters())
    {
        replacements.insert(
            expected.clone(),
            signatures.types_mut().generic_parameter(actual.clone())?,
        );
    }

    let mut mapping = Vec::new();
    for (expected, actual) in trait_method
        .generic_parameters()
        .iter()
        .zip(override_method.generic_parameters())
    {
        let expected_bounds = match signatures.generic_parameter(expected) {
            FactState::Known(signature) => signature.declared_bounds().to_vec(),
            _ => return Ok(None),
        };
        let actual_bounds = match signatures.generic_parameter(actual) {
            FactState::Known(signature) => signature.declared_bounds().to_vec(),
            _ => return Ok(None),
        };
        let mut actual_normalized = Vec::new();
        for bound in actual_bounds {
            let ContractBounds::Known(normalized) = normalize_contract_bounds(
                environment,
                signatures,
                implementation,
                std::slice::from_ref(&bound),
                &BTreeMap::new(),
                None,
            )?
            else {
                return Ok(None);
            };
            let [normalized] = normalized.as_slice() else {
                return Ok(None);
            };
            actual_normalized.push((normalized.clone(), bound));
        }
        for (bound_index, bound) in expected_bounds.iter().enumerate() {
            let ContractBounds::Known(normalized) = normalize_contract_bounds(
                environment,
                signatures,
                implementation,
                std::slice::from_ref(bound),
                &replacements,
                Some((trait_target, header.target())),
            )?
            else {
                return Ok(None);
            };
            let [normalized] = normalized.as_slice() else {
                return Ok(None);
            };
            let Some((_, actual_bound)) = actual_normalized
                .iter()
                .find(|(candidate, _)| candidate == normalized)
            else {
                return Ok(None);
            };
            let subject = signatures.types_mut().generic_parameter(actual.clone())?;
            mapping.push(CheckedMethodBound {
                parameter: expected.clone(),
                bound_index,
                obligation: TraitObligation::new(subject, actual_bound.clone()),
            });
        }
    }
    Ok(Some(mapping))
}

#[derive(Clone, Debug)]
enum ContractType {
    Known(TypeId),
    Unresolved,
    Poisoned(Poison),
    InvalidArgument(crate::ImplArgumentViolation),
}

#[derive(Clone, Debug)]
enum ContractBounds {
    Known(Vec<TraitRef>),
    Unresolved,
    Poisoned(Poison),
    InvalidArgument(crate::ImplArgumentViolation),
}

#[derive(Clone, Debug)]
enum CheckedBounds {
    Known(Vec<TraitRef>),
    Poisoned(Poison),
}

fn specialize_contract_type(
    environment: &mut TraitEnvironment,
    signatures: &mut DeclarationSignatures,
    implementation: &CanonicalTarget,
    ty: TypeId,
    replacements: &BTreeMap<GenericParameterId, TypeId>,
    self_owner: &CanonicalTarget,
    self_type: TypeId,
) -> Result<ContractType, TraitEnvironmentError> {
    let ty = signatures.types_mut().substitute(ty, replacements)?;
    let ty = signatures
        .types_mut()
        .replace_self_type(ty, self_owner, self_type)?;
    normalize_contract_type(environment, signatures, implementation, ty)
}

fn normalize_contract_type(
    environment: &mut TraitEnvironment,
    signatures: &mut DeclarationSignatures,
    implementation: &CanonicalTarget,
    ty: TypeId,
) -> Result<ContractType, TraitEnvironmentError> {
    let normalized = match implementation {
        CanonicalTarget::Source(owner) => {
            environment.normalize_for_owner(*owner, signatures, ty)?
        }
        CanonicalTarget::External(_) => {
            environment.normalize_for_implementation(implementation, signatures, ty)?
        }
    };
    Ok(match normalized {
        TraitNormalization::Normalized(ty) | TraitNormalization::Symbolic(ty) => {
            ContractType::Known(ty)
        }
        TraitNormalization::Poisoned(poison) => ContractType::Poisoned(poison),
        TraitNormalization::InvalidArgument(violation) => ContractType::InvalidArgument(violation),
        TraitNormalization::Unsatisfied(_)
        | TraitNormalization::Ambiguous(_)
        | TraitNormalization::Cyclic(_) => ContractType::Unresolved,
    })
}

fn checked_generic_bounds(
    signatures: &DeclarationSignatures,
    parameter: &GenericParameterId,
) -> Result<CheckedBounds, TraitEnvironmentError> {
    match signatures.generic_parameter(parameter) {
        FactState::Known(signature) => Ok(CheckedBounds::Known(signature.bounds().to_vec())),
        FactState::Poisoned(poison) => Ok(CheckedBounds::Poisoned(poison.clone())),
        FactState::Unperformed | FactState::NotApplicable => Err(
            TraitEnvironmentError::UnavailableGenericParameter(parameter.clone()),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn normalize_contract_bounds(
    environment: &mut TraitEnvironment,
    signatures: &mut DeclarationSignatures,
    implementation: &CanonicalTarget,
    bounds: &[TraitRef],
    replacements: &BTreeMap<GenericParameterId, TypeId>,
    self_replacement: Option<(&CanonicalTarget, TypeId)>,
) -> Result<ContractBounds, TraitEnvironmentError> {
    let mut normalized = Vec::with_capacity(bounds.len());
    for bound in bounds {
        let arguments = signatures
            .types()
            .substitution(bound.substitution())?
            .to_vec();
        let mut normalized_arguments = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let mut argument = signatures.types_mut().substitute(argument, replacements)?;
            if let Some((owner, replacement)) = self_replacement {
                argument =
                    signatures
                        .types_mut()
                        .replace_self_type(argument, owner, replacement)?;
            }
            match normalize_contract_type(environment, signatures, implementation, argument)? {
                ContractType::Known(argument) => normalized_arguments.push(argument),
                ContractType::Unresolved => return Ok(ContractBounds::Unresolved),
                ContractType::InvalidArgument(violation) => {
                    return Ok(ContractBounds::InvalidArgument(violation));
                }
                ContractType::Poisoned(poison) => {
                    return Ok(ContractBounds::Poisoned(poison));
                }
            }
        }
        let substitution = signatures
            .types_mut()
            .intern_substitution(normalized_arguments)?;
        normalized.push(TraitRef::new(bound.declaration().clone(), substitution));
    }
    normalized.sort();
    normalized.dedup();
    Ok(ContractBounds::Known(normalized))
}

pub(super) fn source_declaration_span(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    declaration: DefId,
) -> Result<SourceSpan, TraitEnvironmentError> {
    let declaration = collection
        .declaration(declaration)
        .ok_or(TraitEnvironmentError::UnknownSourceDeclaration(declaration))?;
    let module = collection.module(declaration.id().module()).ok_or(
        TraitEnvironmentError::UnknownModule(declaration.id().module()),
    )?;
    let source = sources
        .get(&module.file())
        .copied()
        .ok_or(TraitEnvironmentError::MissingSource(module.file()))?;
    source.origin_span(declaration.origin()).cloned().ok_or(
        TraitEnvironmentError::MissingOriginSpan {
            file: source.file(),
            origin: declaration.origin(),
        },
    )
}

fn trait_method_related_spans(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    trait_method: &TraitMethodSignature,
) -> Result<Vec<RelatedSpan>, TraitEnvironmentError> {
    let CanonicalTarget::Source(trait_method) = trait_method.declaration() else {
        return Ok(Vec::new());
    };
    Ok(vec![
        RelatedSpan::new(
            source_declaration_span(collection, sources, *trait_method)?,
            "trait method declared here",
        )
        .expect("static related-span label is non-empty"),
    ])
}

fn render_trait_name(
    collection: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    implementation: DefId,
    header: &TraitImplHeader,
) -> Result<String, TraitEnvironmentError> {
    Ok(DiagnosticTypeRenderer::new(
        collection,
        signatures.diagnostic_type_names(),
        signatures,
        implementation,
    )
    .render_trait_ref(header.trait_ref())?)
}

fn render_callable(
    collection: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    owner: DefId,
    callable: &CallableSignature,
) -> Result<String, TraitEnvironmentError> {
    Ok(DiagnosticTypeRenderer::new(
        collection,
        signatures.diagnostic_type_names(),
        signatures,
        owner,
    )
    .render_callable_signature(callable)?)
}

fn render_trait_callable(
    collection: &DeclarationCollection,
    signatures: &DeclarationSignatures,
    implementation: DefId,
    header: &TraitImplHeader,
    method: &TraitMethodSignature,
    callable: &CallableSignature,
) -> Result<String, TraitEnvironmentError> {
    let owner = match method.declaration() {
        CanonicalTarget::Source(owner) => *owner,
        CanonicalTarget::External(_) => implementation,
    };
    Ok(DiagnosticTypeRenderer::new(
        collection,
        signatures.diagnostic_type_names(),
        signatures,
        owner,
    )
    .with_self_binding(header.trait_ref().declaration().clone(), header.target())
    .render_callable_signature(callable)?)
}

fn record_missing_impl_method(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    implementation: DefId,
    header: &TraitImplHeader,
    trait_method: &TraitMethodSignature,
) -> Result<Poison, TraitEnvironmentError> {
    let trait_name = render_trait_name(collection, signatures, implementation, header)?;
    let method = trait_method.name().spelling();
    Ok(record_type_error_with_related(
        diagnostics,
        "TRAIT_IMPL_MISSING_METHOD",
        format!("trait `{trait_name}` method `{method}` is missing from impl"),
        source_declaration_span(collection, sources, implementation)?,
        trait_method_related_spans(collection, sources, trait_method)?,
        BTreeMap::from([
            (
                Box::<str>::from("trait"),
                DiagnosticDataValue::from(trait_name),
            ),
            (
                Box::<str>::from("method"),
                DiagnosticDataValue::from(method),
            ),
        ]),
    )?)
}

#[allow(clippy::too_many_arguments)]
fn record_trait_effect_mismatch(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    implementation: DefId,
    header: &TraitImplHeader,
    trait_method: &TraitMethodSignature,
    expected: &CallableSignature,
    impl_method: DefId,
    actual: &CallableSignature,
) -> Result<Poison, TraitEnvironmentError> {
    let trait_name = render_trait_name(collection, signatures, implementation, header)?;
    let method = trait_method.name().spelling();
    let actual_name = declaration_name(collection, impl_method)?.spelling();
    let expected_signature = render_trait_callable(
        collection,
        signatures,
        implementation,
        header,
        trait_method,
        expected,
    )?;
    let actual_signature = render_callable(collection, signatures, impl_method, actual)?;
    Ok(record_type_error_with_related(
        diagnostics,
        "TRAIT_EFFECT_MISMATCH",
        format!("trait method `{method}` and impl method `{actual_name}` must match effect marker"),
        source_declaration_span(collection, sources, impl_method)?,
        trait_method_related_spans(collection, sources, trait_method)?,
        method_mismatch_data(trait_name, method, expected_signature, actual_signature),
    )?)
}

#[allow(clippy::too_many_arguments)]
fn record_trait_impl_signature_mismatch(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    implementation: DefId,
    header: &TraitImplHeader,
    trait_method: &TraitMethodSignature,
    expected: &CallableSignature,
    impl_method: DefId,
    actual: &CallableSignature,
) -> Result<Poison, TraitEnvironmentError> {
    let trait_name = render_trait_name(collection, signatures, implementation, header)?;
    let method = trait_method.name().spelling();
    let expected_signature = render_trait_callable(
        collection,
        signatures,
        implementation,
        header,
        trait_method,
        expected,
    )?;
    let actual_signature = render_callable(collection, signatures, impl_method, actual)?;
    Ok(record_type_error_with_related(
        diagnostics,
        "TRAIT_IMPL_SIGNATURE_MISMATCH",
        format!("trait impl method `{method}` signature mismatch for `{trait_name}`"),
        source_declaration_span(collection, sources, impl_method)?,
        trait_method_related_spans(collection, sources, trait_method)?,
        method_mismatch_data(trait_name, method, expected_signature, actual_signature),
    )?)
}

fn method_mismatch_data(
    trait_name: String,
    method: String,
    expected_signature: String,
    actual_signature: String,
) -> BTreeMap<Box<str>, DiagnosticDataValue> {
    BTreeMap::from([
        (
            Box::<str>::from("trait"),
            DiagnosticDataValue::from(trait_name),
        ),
        (
            Box::<str>::from("method"),
            DiagnosticDataValue::from(method),
        ),
        (
            Box::<str>::from("expected_signature"),
            DiagnosticDataValue::from(expected_signature),
        ),
        (
            Box::<str>::from("actual_signature"),
            DiagnosticDataValue::from(actual_signature),
        ),
    ])
}

fn merge_optional_poison(
    target: &mut Option<Poison>,
    poison: &Poison,
) -> Result<(), DiagnosticStoreError> {
    *target = Some(match target.take() {
        Some(existing) => existing.merge(poison)?,
        None => poison.clone(),
    });
    Ok(())
}
