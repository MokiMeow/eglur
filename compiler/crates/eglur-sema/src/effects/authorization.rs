use std::collections::BTreeMap;
use std::fmt;

use eglur_diagnostics::{DiagnosticStore, RelatedSpan, SourceSpan};
use eglur_source::FileId;

use crate::diagnostic::record_effect_violation;
use crate::{
    CanonicalTarget, DeclarationCollection, DeclarationSignatures, DefId, FactState, NameKey,
    SemanticSource,
};

use super::EffectSummary;
use super::body::{EffectBodyFact, EffectCall, EffectDispatch, EffectOperation};
use super::inference::{EffectFacts, PathScope, classify_paths, transfer_call};

pub(super) fn authorize_effects(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    signatures: &DeclarationSignatures,
    body_facts: &[EffectBodyFact],
    facts: &EffectFacts,
    diagnostics: &mut DiagnosticStore,
) -> Result<(), EffectAuthorizationError> {
    let source_map = sources
        .iter()
        .map(|source| (source.file(), source))
        .collect::<BTreeMap<_, _>>();
    if source_map.len() != sources.len() {
        return Err(EffectAuthorizationError::DuplicateSource);
    }

    for body in body_facts.iter().filter_map(EffectBodyFact::known) {
        let signature = signatures.callable(body.signature()).ok_or(
            EffectAuthorizationError::UnknownCallableSignature(body.signature()),
        )?;
        if signature.is_bang() || !matches!(facts.source_summary(body.owner()), FactState::Known(_))
        {
            continue;
        }
        let caller = source_callable_name(collection, body.owner())?;
        for operation in body.operations() {
            match operation {
                EffectOperation::Mutation(mutation)
                    if classify_paths(std::slice::from_ref(mutation.path()))
                        != PathScope::Local =>
                {
                    record_violation(
                        collection,
                        &source_map,
                        body.owner(),
                        mutation.origin(),
                        &caller,
                        "assignment target",
                        None,
                        diagnostics,
                    )?;
                }
                EffectOperation::Mutation(_) => {}
                EffectOperation::Call(call) => {
                    let Some(summary) = call_summary(facts, call) else {
                        continue;
                    };
                    let violates = if call.is_bang() {
                        !local_bang_exception(call, &summary)
                    } else {
                        !matches!(
                            transfer_call(&summary, call.mutable_paths()),
                            EffectSummary::Known(summary) if summary.is_pure()
                        )
                    };
                    if violates {
                        let callee = call_target_name(collection, call)?;
                        let related = call_related_span(collection, &source_map, call)?;
                        record_violation(
                            collection,
                            &source_map,
                            body.owner(),
                            call.origin(),
                            &caller,
                            &callee,
                            related,
                            diagnostics,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn call_summary(facts: &EffectFacts, call: &EffectCall) -> Option<EffectSummary> {
    match call.dispatch() {
        EffectDispatch::Constructor | EffectDispatch::CallableValue => Some(EffectSummary::pure()),
        EffectDispatch::BoundRequirement { .. } if call.is_bang() => Some(EffectSummary::Unknown),
        EffectDispatch::BoundRequirement { .. } => Some(EffectSummary::pure()),
        EffectDispatch::Target(target) => match facts.summary(target) {
            FactState::Known(summary) => Some(summary.clone()),
            FactState::Poisoned(_) | FactState::Unperformed | FactState::NotApplicable => None,
        },
    }
}

fn local_bang_exception(call: &EffectCall, summary: &EffectSummary) -> bool {
    matches!(summary, EffectSummary::Known(known) if known.is_localizable_state_only())
        && !call.mutable_paths().is_empty()
        && classify_paths(call.mutable_paths()) == PathScope::Local
}

#[allow(
    clippy::too_many_arguments,
    reason = "diagnostic provenance is explicit"
)]
fn record_violation(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    owner: DefId,
    origin: eglur_hir::OriginId,
    caller: &str,
    callee: &str,
    related: Option<RelatedSpan>,
    diagnostics: &mut DiagnosticStore,
) -> Result<(), EffectAuthorizationError> {
    let span = source_span(collection, sources, owner, origin)?;
    let message = if callee == "assignment target" {
        format!("non-bang function `{caller}` cannot mutate non-local assignment target")
    } else {
        format!("non-bang function `{caller}` cannot call `{callee}`")
    };
    let help = format!("mark caller as `{caller}!` or remove the effectful operation");
    record_effect_violation(diagnostics, message, span, help, related, caller, callee)?;
    Ok(())
}

fn source_callable_name(
    collection: &DeclarationCollection,
    declaration: DefId,
) -> Result<String, EffectAuthorizationError> {
    collection
        .declaration(declaration)
        .ok_or(EffectAuthorizationError::UnknownSourceDeclaration(
            declaration,
        ))?
        .name()
        .map(NameKey::spelling)
        .ok_or(EffectAuthorizationError::MissingDeclarationName(
            declaration,
        ))
}

fn call_target_name(
    collection: &DeclarationCollection,
    call: &EffectCall,
) -> Result<String, EffectAuthorizationError> {
    let target = match call.dispatch() {
        EffectDispatch::Target(target) => target,
        EffectDispatch::BoundRequirement { method } => method,
        EffectDispatch::Constructor => return Ok("constructor".to_owned()),
        EffectDispatch::CallableValue => return Ok("callable value".to_owned()),
    };
    match target {
        CanonicalTarget::Primitive(primitive) => Ok(primitive.spelling().to_owned()),
        CanonicalTarget::Source(declaration) => source_callable_name(collection, *declaration),
        CanonicalTarget::External(declaration) => Ok(declaration
            .as_str()
            .rsplit("::")
            .next()
            .unwrap_or(declaration.as_str())
            .to_owned()),
    }
}

fn call_related_span(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    call: &EffectCall,
) -> Result<Option<RelatedSpan>, EffectAuthorizationError> {
    let declaration = match call.dispatch() {
        EffectDispatch::Target(CanonicalTarget::Source(declaration))
        | EffectDispatch::BoundRequirement {
            method: CanonicalTarget::Source(declaration),
        } => *declaration,
        EffectDispatch::Target(CanonicalTarget::External(_))
        | EffectDispatch::Target(CanonicalTarget::Primitive(_))
        | EffectDispatch::BoundRequirement {
            method: CanonicalTarget::Primitive(_),
        }
        | EffectDispatch::BoundRequirement {
            method: CanonicalTarget::External(_),
        }
        | EffectDispatch::Constructor
        | EffectDispatch::CallableValue => return Ok(None),
    };
    let source_declaration = collection.declaration(declaration).ok_or(
        EffectAuthorizationError::UnknownSourceDeclaration(declaration),
    )?;
    let span = source_span(
        collection,
        sources,
        declaration,
        source_declaration.origin(),
    )?;
    Ok(RelatedSpan::new(span, "effectful callee declared here"))
}

fn source_span(
    collection: &DeclarationCollection,
    sources: &BTreeMap<FileId, &SemanticSource<'_>>,
    owner: DefId,
    origin: eglur_hir::OriginId,
) -> Result<SourceSpan, EffectAuthorizationError> {
    let module = collection
        .module(owner.module())
        .ok_or(EffectAuthorizationError::UnknownModule(owner.module()))?;
    sources
        .get(&module.file())
        .copied()
        .ok_or(EffectAuthorizationError::MissingSource(module.file()))?
        .origin_span(origin)
        .cloned()
        .ok_or(EffectAuthorizationError::MissingOriginSpan {
            file: module.file(),
            origin,
        })
}

#[derive(Debug)]
pub(super) enum EffectAuthorizationError {
    DuplicateSource,
    UnknownCallableSignature(crate::CallableSignatureId),
    UnknownSourceDeclaration(DefId),
    MissingDeclarationName(DefId),
    UnknownModule(crate::ModuleId),
    MissingSource(FileId),
    MissingOriginSpan {
        file: FileId,
        origin: eglur_hir::OriginId,
    },
    Diagnostic(Box<str>),
}

impl fmt::Display for EffectAuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("effect authorization invariant failed: ")?;
        match self {
            Self::DuplicateSource => formatter.write_str("source input is duplicated"),
            Self::UnknownCallableSignature(signature) => {
                write!(formatter, "unknown callable signature {signature:?}")
            }
            Self::UnknownSourceDeclaration(declaration) => {
                write!(formatter, "unknown source declaration {declaration:?}")
            }
            Self::MissingDeclarationName(declaration) => {
                write!(formatter, "callable {declaration:?} has no source name")
            }
            Self::UnknownModule(module) => write!(formatter, "unknown module {module:?}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file}"),
            Self::MissingOriginSpan { file, origin } => {
                write!(formatter, "source {file} has no span for origin {origin:?}")
            }
            Self::Diagnostic(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for EffectAuthorizationError {}

impl From<crate::diagnostic::SemanticDiagnosticError> for EffectAuthorizationError {
    fn from(error: crate::diagnostic::SemanticDiagnosticError) -> Self {
        Self::Diagnostic(format!("{error:?}").into_boxed_str())
    }
}
