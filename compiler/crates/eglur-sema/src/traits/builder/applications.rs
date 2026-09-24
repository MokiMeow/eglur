use std::collections::{BTreeSet, VecDeque};

use super::super::environment::ApplicationCheck;
use super::*;
use crate::signatures::{TypeApplication, TypeApplicationSite};

pub(super) fn validate_type_applications(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    signatures: &mut DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    environment: &mut TraitEnvironment,
) -> Result<bool, TraitEnvironmentError> {
    let applications = signatures.type_applications().cloned().collect::<Vec<_>>();
    let dependencies = signatures.application_dependencies(collection);
    let mut source_map = BTreeMap::new();
    for source in sources {
        if source_map.insert(source.file(), source).is_some() {
            return Err(TraitEnvironmentError::DuplicateSource(source.file()));
        }
    }

    let initial = environment
        .implementation_validation_inputs()
        .into_iter()
        .filter_map(|input| {
            input
                .existing_poison
                .map(|poison| (input.implementation, poison))
        });
    propagate_poison(signatures, &dependencies, initial);
    synchronize_implementations(signatures, environment);

    let mut unperformed_owners = BTreeSet::new();
    loop {
        for application in &applications {
            if let TypeApplicationSite::Source { occurrence, .. } = application.site()
                && matches!(signatures.occurrence(*occurrence), FactState::Poisoned(_))
            {
                continue;
            }
            match signatures.declaration(&application.site().owner()) {
                FactState::Poisoned(_) => continue,
                FactState::Unperformed => {
                    unperformed_owners.insert(application.site().owner());
                    continue;
                }
                FactState::Known(_) | FactState::NotApplicable => {}
            }
            match environment.check_type_application(signatures, application)? {
                ApplicationCheck::Checked => {}
                ApplicationCheck::Unperformed => {
                    unperformed_owners.insert(application.site().owner());
                }
                ApplicationCheck::Failed(failure) => {
                    let poison = application_failure(
                        collection,
                        &source_map,
                        signatures,
                        diagnostics,
                        application,
                        failure,
                    )?;
                    propagate_poison(
                        signatures,
                        &dependencies,
                        [(application.site().owner(), poison)],
                    );
                }
            }
        }
        // A rejected impl can have supplied earlier proofs. Recheck after its cache is invalidated.
        if !synchronize_implementations(signatures, environment) {
            return Ok(unperformed_owners
                .iter()
                .all(|owner| matches!(signatures.declaration(owner), FactState::Poisoned(_))));
        }
    }
}

fn propagate_poison(
    signatures: &mut DeclarationSignatures,
    dependencies: &BTreeMap<CanonicalTarget, BTreeSet<CanonicalTarget>>,
    initial: impl IntoIterator<Item = (CanonicalTarget, Poison)>,
) {
    let mut queue = initial.into_iter().collect::<VecDeque<_>>();
    while let Some((target, poison)) = queue.pop_front() {
        if signatures.poison_type_application_owner(target.clone(), poison.clone()) {
            queue.extend(
                dependencies
                    .get(&target)
                    .into_iter()
                    .flatten()
                    .cloned()
                    .map(|dependent| (dependent, poison.clone())),
            );
        }
    }
}

pub(super) fn synchronize_implementations(
    signatures: &DeclarationSignatures,
    environment: &mut TraitEnvironment,
) -> bool {
    let mut changed = false;
    for input in environment.implementation_validation_inputs() {
        if input.existing_poison.is_none()
            && let FactState::Poisoned(poison) = signatures.declaration(&input.implementation)
        {
            environment.poison_implementation(&input.implementation, poison.clone());
            changed = true;
        }
    }
    changed
}

fn application_failure(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    signatures: &DeclarationSignatures,
    diagnostics: &mut DiagnosticStore,
    application: &TypeApplication,
    failure: TraitNormalization,
) -> Result<Poison, TraitEnvironmentError> {
    let TypeApplicationSite::Source { owner, occurrence } = application.site() else {
        return Err(TraitEnvironmentError::InvalidTrustedTypeApplication {
            owner: application.site().owner(),
            target: application.target().clone(),
        });
    };
    if let TraitNormalization::Poisoned(poison) = failure {
        return Ok(poison);
    }
    let source = sources
        .get(&occurrence.file())
        .ok_or(TraitEnvironmentError::MissingSource(occurrence.file()))?;
    let hir = source
        .hir()
        .ok_or(TraitEnvironmentError::MissingHir(occurrence.file()))?;
    let origin = hir.type_ref(occurrence.type_ref()).origin();
    let span =
        source
            .origin_span(origin)
            .cloned()
            .ok_or(TraitEnvironmentError::MissingOriginSpan {
                file: occurrence.file(),
                origin,
            })?;
    let (code, message, data) = match failure {
        TraitNormalization::Unsatisfied(obligation) | TraitNormalization::Ambiguous(obligation) => {
            let renderer = DiagnosticTypeRenderer::new(
                collection,
                signatures.diagnostic_type_names(),
                signatures,
                *owner,
            );
            (
                "TRAIT_BOUND_NOT_SATISFIED",
                "trait bound is not satisfied",
                BTreeMap::from([
                    (
                        Box::<str>::from("trait"),
                        DiagnosticDataValue::from(
                            renderer.render_trait_ref(obligation.trait_ref())?,
                        ),
                    ),
                    (
                        Box::<str>::from("type"),
                        DiagnosticDataValue::from(renderer.render_type(obligation.subject())?),
                    ),
                ]),
            )
        }
        TraitNormalization::InvalidArgument(violation) => {
            let (code, message) = crate::diagnostic::impl_argument_diagnostic(violation);
            (code, message, BTreeMap::new())
        }
        TraitNormalization::Cyclic(_) => (
            "ASSOCIATED_TYPE_CYCLE",
            "associated type normalization contains a cycle",
            BTreeMap::new(),
        ),
        TraitNormalization::Poisoned(_)
        | TraitNormalization::Normalized(_)
        | TraitNormalization::Symbolic(_) => {
            unreachable!("successful normalization is not an application failure")
        }
    };
    Ok(crate::diagnostic::record_type_error(
        diagnostics,
        code,
        message,
        span,
        data,
    )?)
}
