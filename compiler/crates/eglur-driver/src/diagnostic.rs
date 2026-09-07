use std::collections::BTreeMap;

use eglur_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticDataValue, DiagnosticFailure, DiagnosticPhase,
    DiagnosticRoot, DiagnosticStore, Poison, PrimarySpan, Severity, SourceLabel, SourceSpan,
};
use eglur_source::{FileSpan, LineIndex, SourceFile};
use eglur_syntax::{SyntaxDiagnostic, SyntaxDiagnosticKind, SyntaxPhase};

use crate::session::SessionError;

pub(crate) fn record_syntax(
    store: &mut DiagnosticStore,
    label: &SourceLabel,
    lines: &LineIndex<'_>,
    diagnostic: SyntaxDiagnostic,
) -> Result<Poison, SessionError> {
    let primary = SourceSpan::projected(label.clone(), diagnostic.span(), lines)?;
    let mut normalized = Diagnostic::new(
        code(diagnostic.kind().code()),
        Severity::Error,
        phase(match diagnostic.phase() {
            SyntaxPhase::Lex => "lex",
            SyntaxPhase::Parse => "parse",
        }),
        syntax_message(diagnostic.kind()),
        PrimarySpan::Source(primary),
    );
    if let Some(limit) = diagnostic.resource_limit() {
        let mut data = BTreeMap::new();
        data.insert("resource".into(), limit.resource().as_str().into());
        data.insert("limit".into(), limit.limit().into());
        data.insert("observed_at_least".into(), limit.observed_at_least().into());
        normalized = normalized.with_data(data);
    }
    record_root(store, normalized)
}

pub(crate) fn record_invalid_utf8(
    store: &mut DiagnosticStore,
    label: &SourceLabel,
    span: FileSpan,
) -> Result<Poison, SessionError> {
    record_root(
        store,
        Diagnostic::new(
            code("LEX_INVALID_UTF8"),
            Severity::Error,
            phase("lex"),
            "source UTF-8 invalid",
            PrimarySpan::Source(SourceSpan::byte_only(label.clone(), span)),
        ),
    )
}

pub(crate) fn record_module_root_reserved(
    store: &mut DiagnosticStore,
    label: &SourceLabel,
    source: &SourceFile,
) -> Result<Poison, SessionError> {
    let mut data = BTreeMap::new();
    data.insert("name".into(), DiagnosticDataValue::from("std"));
    record_root(
        store,
        Diagnostic::new(
            code("MODULE_ROOT_RESERVED"),
            Severity::Error,
            phase("resolve"),
            "module root std is reserved",
            PrimarySpan::Source(SourceSpan::source_start(label.clone(), source)),
        )
        .with_data(data),
    )
}

pub(crate) fn record_module_component_invalid(
    store: &mut DiagnosticStore,
    label: &SourceLabel,
    source: &SourceFile,
    name: &str,
    index: usize,
) -> Result<Poison, SessionError> {
    let mut data = BTreeMap::new();
    data.insert("name".into(), DiagnosticDataValue::from(name));
    data.insert(
        "index".into(),
        DiagnosticDataValue::from(
            i64::try_from(index).expect("allocated path component index must fit i64"),
        ),
    );
    record_root(
        store,
        Diagnostic::new(
            code("MODULE_PATH_COMPONENT_INVALID"),
            Severity::Error,
            phase("resolve"),
            "module path component is invalid",
            PrimarySpan::Source(SourceSpan::source_start(label.clone(), source)),
        )
        .with_data(data),
    )
}

pub(crate) fn record_module_path_mismatch(
    store: &mut DiagnosticStore,
    label: &SourceLabel,
    lines: &LineIndex<'_>,
    span: FileSpan,
) -> Result<Poison, SessionError> {
    record_root(
        store,
        Diagnostic::new(
            code("MODULE_PATH_MISMATCH"),
            Severity::Error,
            phase("resolve"),
            "module path mismatch",
            PrimarySpan::Source(SourceSpan::projected(label.clone(), span, lines)?),
        ),
    )
}

fn record_root(
    store: &mut DiagnosticStore,
    diagnostic: Diagnostic,
) -> Result<Poison, SessionError> {
    let root = DiagnosticRoot::source(diagnostic)?;
    Ok(store.record_failure(DiagnosticFailure::Root(root))?)
}

fn code(spelling: &str) -> DiagnosticCode {
    DiagnosticCode::new(spelling).expect("driver diagnostic codes are statically valid")
}

fn phase(spelling: &str) -> DiagnosticPhase {
    DiagnosticPhase::new(spelling).expect("driver diagnostic phases are statically valid")
}

fn syntax_message(kind: SyntaxDiagnosticKind) -> &'static str {
    match kind {
        SyntaxDiagnosticKind::LexInvalidChar => "lex invalid char",
        SyntaxDiagnosticKind::LexInvalidEscape => "escape invalid",
        SyntaxDiagnosticKind::LexInvalidIdentifierChar => "identifier ASCII invalid",
        SyntaxDiagnosticKind::LexInvalidNumber => "number invalid",
        SyntaxDiagnosticKind::LexNestedBlockComment => "comment nested",
        SyntaxDiagnosticKind::LexUnterminatedBlockComment => "comment unterminated",
        SyntaxDiagnosticKind::LexUnterminatedString => "string unterminated",
        SyntaxDiagnosticKind::ParseControlHeadBraceRequiresGrouping => {
            "parse control head brace requires grouping"
        }
        SyntaxDiagnosticKind::ParseExpectedExpression => "parse expected expression",
        SyntaxDiagnosticKind::ParseExpectedPattern => "parse expected pattern",
        SyntaxDiagnosticKind::ParseInvalidRangeSyntax => "parse range syntax invalid",
        SyntaxDiagnosticKind::ParseInvalidRestPosition => "parse rest position invalid",
        SyntaxDiagnosticKind::ParseResourceLimitExceeded => "parse resource limit exceeded",
        SyntaxDiagnosticKind::ParseUnclosedDelimiter => "parse unclosed delimiter",
        SyntaxDiagnosticKind::ParseUnexpectedToken => "parse unexpected token",
    }
}

#[cfg(test)]
mod tests {
    use eglur_syntax::SyntaxDiagnosticKind;

    use super::syntax_message;

    #[test]
    fn syntax_message_producer_satisfies_every_required_term_contract() {
        let cases: &[(SyntaxDiagnosticKind, &[&str])] = &[
            (
                SyntaxDiagnosticKind::LexInvalidChar,
                &["lex", "invalid", "char"],
            ),
            (
                SyntaxDiagnosticKind::LexInvalidEscape,
                &["escape", "invalid"],
            ),
            (
                SyntaxDiagnosticKind::LexInvalidIdentifierChar,
                &["identifier", "ASCII", "invalid"],
            ),
            (
                SyntaxDiagnosticKind::LexInvalidNumber,
                &["number", "invalid"],
            ),
            (
                SyntaxDiagnosticKind::LexNestedBlockComment,
                &["comment", "nested"],
            ),
            (
                SyntaxDiagnosticKind::LexUnterminatedBlockComment,
                &["comment", "unterminated"],
            ),
            (
                SyntaxDiagnosticKind::LexUnterminatedString,
                &["string", "unterminated"],
            ),
            (
                SyntaxDiagnosticKind::ParseControlHeadBraceRequiresGrouping,
                &["parse", "control head", "brace", "grouping"],
            ),
            (
                SyntaxDiagnosticKind::ParseExpectedExpression,
                &["parse", "expected", "expression"],
            ),
            (
                SyntaxDiagnosticKind::ParseExpectedPattern,
                &["parse", "expected", "pattern"],
            ),
            (
                SyntaxDiagnosticKind::ParseInvalidRangeSyntax,
                &["parse", "range", "syntax"],
            ),
            (
                SyntaxDiagnosticKind::ParseInvalidRestPosition,
                &["parse", "rest", "position"],
            ),
            (
                SyntaxDiagnosticKind::ParseResourceLimitExceeded,
                &["parse", "resource", "limit", "exceeded"],
            ),
            (
                SyntaxDiagnosticKind::ParseUnclosedDelimiter,
                &["parse", "unclosed", "delimiter"],
            ),
            (
                SyntaxDiagnosticKind::ParseUnexpectedToken,
                &["parse", "unexpected", "token"],
            ),
        ];

        for &(kind, required) in cases {
            let message = syntax_message(kind);
            for term in required {
                assert!(message.contains(term), "{kind:?} message omitted `{term}`");
            }
        }
    }
}
