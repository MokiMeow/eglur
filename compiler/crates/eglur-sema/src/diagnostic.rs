use std::collections::BTreeMap;

use eglur_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticDataValue, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticRootError, DiagnosticStore, DiagnosticStoreError, Poison,
    PrimarySpan, RelatedOrder, RelatedSpan, Severity, SourceSpan,
};

pub(crate) fn record_name_not_found(
    store: &mut DiagnosticStore,
    span: SourceSpan,
    name: &str,
) -> Result<Poison, SemanticDiagnosticError> {
    let mut data = BTreeMap::new();
    data.insert("name".into(), DiagnosticDataValue::from(name));
    record_root(
        store,
        Diagnostic::new(
            code("NAME_NOT_FOUND"),
            Severity::Error,
            resolve_phase(),
            format!("name `{name}` not found"),
            PrimarySpan::Source(span),
        )
        .with_data(data),
    )
}

pub(crate) fn record_name_ambiguous(
    store: &mut DiagnosticStore,
    span: SourceSpan,
    name: &str,
    candidates: impl IntoIterator<Item = String>,
) -> Result<Poison, SemanticDiagnosticError> {
    let candidates = candidates.into_iter().collect::<Vec<_>>().join(", ");
    let mut data = BTreeMap::new();
    data.insert("name".into(), DiagnosticDataValue::from(name));
    data.insert("candidates".into(), DiagnosticDataValue::from(candidates));
    record_root(
        store,
        Diagnostic::new(
            code("NAME_AMBIGUOUS"),
            Severity::Error,
            resolve_phase(),
            format!("ambiguous name `{name}`"),
            PrimarySpan::Source(span),
        )
        .with_data(data),
    )
}

pub(crate) fn record_local_redeclaration(
    store: &mut DiagnosticStore,
    span: SourceSpan,
    first: SourceSpan,
    name: &str,
) -> Result<Poison, SemanticDiagnosticError> {
    let mut data = BTreeMap::new();
    data.insert("name".into(), DiagnosticDataValue::from(name));
    let related = RelatedSpan::new(first, "first binding declared here")
        .expect("static related-span label is non-empty");
    record_root(
        store,
        Diagnostic::new(
            code("LOCAL_BINDING_REDECLARATION"),
            Severity::Error,
            resolve_phase(),
            format!("local binding `{name}` redeclared"),
            PrimarySpan::Source(span),
        )
        .with_related([related], RelatedOrder::CodeDefined)
        .with_data(data),
    )
}

pub(crate) fn record_import_error(
    store: &mut DiagnosticStore,
    code_spelling: &'static str,
    message: impl Into<String>,
    span: SourceSpan,
) -> Result<Poison, SemanticDiagnosticError> {
    record_root(
        store,
        Diagnostic::new(
            code(code_spelling),
            Severity::Error,
            resolve_phase(),
            message.into(),
            PrimarySpan::Source(span),
        ),
    )
}

pub(crate) fn record_visibility_violation(
    store: &mut DiagnosticStore,
    span: SourceSpan,
) -> Result<Poison, SemanticDiagnosticError> {
    record_import_error(
        store,
        "VISIBILITY_VIOLATION",
        "visibility violation: declaration is private",
        span,
    )
}

pub(crate) const fn impl_argument_diagnostic(
    violation: crate::ImplArgumentViolation,
) -> (&'static str, &'static str) {
    match violation {
        crate::ImplArgumentViolation::Ownership(_) => (
            "OWNERSHIP_TYPE_POSITION_FORBIDDEN",
            "ownership type position is forbidden for an inferred impl argument",
        ),
        crate::ImplArgumentViolation::Reference(_) => (
            "REFERENCE_TYPE_POSITION_FORBIDDEN",
            "reference type position is forbidden for an inferred impl argument",
        ),
    }
}

pub(crate) fn record_type_error(
    store: &mut DiagnosticStore,
    code_spelling: &'static str,
    message: impl Into<String>,
    span: SourceSpan,
    data: BTreeMap<Box<str>, DiagnosticDataValue>,
) -> Result<Poison, SemanticDiagnosticError> {
    record_root(
        store,
        Diagnostic::new(
            code(code_spelling),
            Severity::Error,
            type_phase(),
            message.into(),
            PrimarySpan::Source(span),
        )
        .with_data(data),
    )
}

pub(crate) fn record_type_error_with_related(
    store: &mut DiagnosticStore,
    code_spelling: &'static str,
    message: impl Into<String>,
    span: SourceSpan,
    related: impl IntoIterator<Item = RelatedSpan>,
    data: BTreeMap<Box<str>, DiagnosticDataValue>,
) -> Result<Poison, SemanticDiagnosticError> {
    record_root(
        store,
        Diagnostic::new(
            code(code_spelling),
            Severity::Error,
            type_phase(),
            message.into(),
            PrimarySpan::Source(span),
        )
        .with_related(related, RelatedOrder::CodeDefined)
        .with_data(data),
    )
}

pub(crate) fn record_effect_violation(
    store: &mut DiagnosticStore,
    message: impl Into<String>,
    span: SourceSpan,
    help: impl Into<Box<str>>,
    related: impl IntoIterator<Item = RelatedSpan>,
    caller: &str,
    callee: &str,
) -> Result<Poison, SemanticDiagnosticError> {
    let data = BTreeMap::from([
        (
            Box::<str>::from("caller"),
            DiagnosticDataValue::from(caller),
        ),
        (
            Box::<str>::from("callee"),
            DiagnosticDataValue::from(callee),
        ),
    ]);
    record_root(
        store,
        Diagnostic::new(
            code("EFFECT_VIOLATION"),
            Severity::Error,
            effect_phase(),
            message.into(),
            PrimarySpan::Source(span),
        )
        .with_help(help)
        .with_related(related, RelatedOrder::CodeDefined)
        .with_data(data),
    )
}

fn record_root(
    store: &mut DiagnosticStore,
    diagnostic: Diagnostic,
) -> Result<Poison, SemanticDiagnosticError> {
    let root = DiagnosticRoot::source(diagnostic)?;
    Ok(store.record_failure(DiagnosticFailure::Root(root))?)
}

fn code(spelling: &'static str) -> DiagnosticCode {
    DiagnosticCode::new(spelling).expect("semantic diagnostic code is statically valid")
}

fn resolve_phase() -> DiagnosticPhase {
    DiagnosticPhase::new("resolve").expect("resolve is a baseline diagnostic phase")
}

fn type_phase() -> DiagnosticPhase {
    DiagnosticPhase::new("type").expect("type is a baseline diagnostic phase")
}

fn effect_phase() -> DiagnosticPhase {
    DiagnosticPhase::new("effect").expect("effect is a baseline diagnostic phase")
}

#[derive(Debug)]
pub(crate) enum SemanticDiagnosticError {
    Root(DiagnosticRootError),
    Store(DiagnosticStoreError),
}

impl From<DiagnosticRootError> for SemanticDiagnosticError {
    fn from(error: DiagnosticRootError) -> Self {
        Self::Root(error)
    }
}

impl From<DiagnosticStoreError> for SemanticDiagnosticError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::Store(error)
    }
}
