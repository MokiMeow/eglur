use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use crate::{PrimarySpan, RelatedSpan};

/// Structurally valid stable diagnostic code spelling.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(Box<str>);

impl DiagnosticCode {
    /// Creates an uppercase-snake-case diagnostic code.
    #[must_use]
    pub fn new(code: impl Into<Box<str>>) -> Option<Self> {
        let code = code.into();
        is_diagnostic_code(&code).then_some(Self(code))
    }

    /// Returns the stable code spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Actual compiler phase that detected one diagnostic.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct DiagnosticPhase(PhaseKind);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PhaseKind {
    Lex,
    Parse,
    Resolve,
    Type,
    Effect,
    Trait,
    Runtime,
    Stdlib,
    Axes,
    Concurrency,
    Extension(Box<str>),
}

impl DiagnosticPhase {
    /// Creates a baseline phase or a non-empty implementation extension.
    #[must_use]
    pub fn new(phase: impl Into<Box<str>>) -> Option<Self> {
        let phase = phase.into();
        let kind = match phase.as_ref() {
            "lex" => PhaseKind::Lex,
            "parse" => PhaseKind::Parse,
            "resolve" => PhaseKind::Resolve,
            "type" => PhaseKind::Type,
            "effect" => PhaseKind::Effect,
            "trait" => PhaseKind::Trait,
            "runtime" => PhaseKind::Runtime,
            "stdlib" => PhaseKind::Stdlib,
            "axes" => PhaseKind::Axes,
            "concurrency" => PhaseKind::Concurrency,
            "" => return None,
            _ => PhaseKind::Extension(phase),
        };
        Some(Self(kind))
    }

    /// Returns the exact phase spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match &self.0 {
            PhaseKind::Lex => "lex",
            PhaseKind::Parse => "parse",
            PhaseKind::Resolve => "resolve",
            PhaseKind::Type => "type",
            PhaseKind::Effect => "effect",
            PhaseKind::Trait => "trait",
            PhaseKind::Runtime => "runtime",
            PhaseKind::Stdlib => "stdlib",
            PhaseKind::Axes => "axes",
            PhaseKind::Concurrency => "concurrency",
            PhaseKind::Extension(extension) => extension,
        }
    }
}

impl fmt::Display for DiagnosticPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

/// Stable diagnostic severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Note,
}

impl Severity {
    fn rank(self) -> u8 {
        match self {
            Self::Error => 0,
            Self::Warning => 1,
            Self::Note => 2,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let spelling = match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
        };
        spelling.fmt(formatter)
    }
}

/// Closed scalar value family for structured diagnostic data.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiagnosticDataValue {
    String(Box<str>),
    Integer(i64),
    Bool(bool),
}

impl fmt::Display for DiagnosticDataValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(value) => value.fmt(formatter),
            Self::Integer(value) => value.fmt(formatter),
            Self::Bool(value) => value.fmt(formatter),
        }
    }
}

impl From<&str> for DiagnosticDataValue {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}

impl From<String> for DiagnosticDataValue {
    fn from(value: String) -> Self {
        Self::String(value.into_boxed_str())
    }
}

impl From<i64> for DiagnosticDataValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<bool> for DiagnosticDataValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// Ordering authority for related spans attached to one diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelatedOrder {
    /// Apply the normative source, byte-range, and label order.
    Source,
    /// Preserve a semantic sequence explicitly prescribed by this code's rules.
    CodeDefined,
}

/// Canonical closed diagnostic record retained by the compiler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    code: DiagnosticCode,
    severity: Severity,
    phase: DiagnosticPhase,
    message: Box<str>,
    primary_span: PrimarySpan,
    help: Option<Box<str>>,
    related: Box<[RelatedSpan]>,
    data: BTreeMap<Box<str>, DiagnosticDataValue>,
}

impl Diagnostic {
    /// Creates a diagnostic with all required baseline fields.
    #[must_use]
    pub fn new(
        code: DiagnosticCode,
        severity: Severity,
        phase: DiagnosticPhase,
        message: impl Into<Box<str>>,
        primary_span: PrimarySpan,
    ) -> Self {
        Self {
            code,
            severity,
            phase,
            message: message.into(),
            primary_span,
            help: None,
            related: Box::new([]),
            data: BTreeMap::new(),
        }
    }

    /// Attaches optional human guidance without changing diagnostic identity.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<Box<str>>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Attaches every related span under an explicit ordering authority.
    #[must_use]
    pub fn with_related(
        mut self,
        related: impl IntoIterator<Item = RelatedSpan>,
        order: RelatedOrder,
    ) -> Self {
        let mut related: Vec<_> = related.into_iter().collect();
        if order == RelatedOrder::Source {
            related.sort();
        }
        self.related = related.into_boxed_slice();
        self
    }

    /// Attaches canonical structured scalar data.
    #[must_use]
    pub fn with_data(mut self, data: BTreeMap<Box<str>, DiagnosticDataValue>) -> Self {
        self.data = data;
        self
    }

    /// Returns the stable cause code.
    #[must_use]
    pub const fn code(&self) -> &DiagnosticCode {
        &self.code
    }

    /// Returns the diagnostic severity.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.severity
    }

    /// Returns the phase that actually detected the failure.
    #[must_use]
    pub const fn phase(&self) -> &DiagnosticPhase {
        &self.phase
    }

    /// Returns the human-readable message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the canonical primary location.
    #[must_use]
    pub const fn primary_span(&self) -> &PrimarySpan {
        &self.primary_span
    }

    /// Returns optional correction guidance.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// Returns related spans in their already-canonical sequence.
    #[must_use]
    pub fn related(&self) -> &[RelatedSpan] {
        &self.related
    }

    /// Returns deterministic structured diagnostic data.
    #[must_use]
    pub const fn data(&self) -> &BTreeMap<Box<str>, DiagnosticDataValue> {
        &self.data
    }
}

impl Ord for Diagnostic {
    fn cmp(&self, other: &Self) -> Ordering {
        self.primary_span
            .cmp_position(&other.primary_span)
            .then_with(|| self.severity.rank().cmp(&other.severity.rank()))
            .then_with(|| self.code.cmp(&other.code))
            .then_with(|| self.phase.as_str().cmp(other.phase.as_str()))
            .then_with(|| self.message.cmp(&other.message))
            .then_with(|| self.primary_span.cmp_full(&other.primary_span))
            .then_with(|| self.help.cmp(&other.help))
            .then_with(|| self.related.cmp(&other.related))
            .then_with(|| self.data.cmp(&other.data))
    }
}

impl PartialOrd for Diagnostic {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn is_diagnostic_code(code: &str) -> bool {
    let mut bytes = code.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }

    let mut previous_underscore = false;
    for byte in bytes {
        match byte {
            b'A'..=b'Z' | b'0'..=b'9' => previous_underscore = false,
            b'_' if !previous_underscore => previous_underscore = true,
            _ => return false,
        }
    }
    !previous_underscore
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticCode, DiagnosticPhase};

    #[test]
    fn diagnostic_codes_enforce_uppercase_snake_case() {
        for valid in ["NAME_NOT_FOUND", "TYC_034A", "E2E"] {
            assert!(DiagnosticCode::new(valid).is_some(), "rejected {valid}");
        }
        for invalid in ["", "name_not_found", "_NAME", "NAME__FOUND", "NAME_"] {
            assert!(DiagnosticCode::new(invalid).is_none(), "accepted {invalid}");
        }
    }

    #[test]
    fn phases_retain_baseline_and_extension_spellings() {
        let baseline = DiagnosticPhase::new("type").unwrap();
        let extension = DiagnosticPhase::new("borrowck").unwrap();

        assert_eq!(baseline.as_str(), "type");
        assert_eq!(extension.as_str(), "borrowck");
        assert!(DiagnosticPhase::new("").is_none());
    }
}
