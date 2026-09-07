use eglur_source::FileSpan;

/// Front-end phase that owns one syntax diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxPhase {
    Lex,
    Parse,
}

/// Stable cause kind for source-level lexer and parser diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxDiagnosticKind {
    LexInvalidChar,
    LexInvalidEscape,
    LexInvalidIdentifierChar,
    LexInvalidNumber,
    LexNestedBlockComment,
    LexUnterminatedBlockComment,
    LexUnterminatedString,
    ParseControlHeadBraceRequiresGrouping,
    ParseExpectedExpression,
    ParseExpectedPattern,
    ParseInvalidRangeSyntax,
    ParseInvalidRestPosition,
    ParseResourceLimitExceeded,
    ParseUnclosedDelimiter,
    ParseUnexpectedToken,
}

impl SyntaxDiagnosticKind {
    /// Returns the stable public diagnostic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::LexInvalidChar => "LEX_INVALID_CHAR",
            Self::LexInvalidEscape => "LEX_INVALID_ESCAPE",
            Self::LexInvalidIdentifierChar => "LEX_INVALID_IDENTIFIER_CHAR",
            Self::LexInvalidNumber => "LEX_INVALID_NUMBER",
            Self::LexNestedBlockComment => "LEX_NESTED_BLOCK_COMMENT",
            Self::LexUnterminatedBlockComment => "LEX_UNTERMINATED_BLOCK_COMMENT",
            Self::LexUnterminatedString => "LEX_UNTERMINATED_STRING",
            Self::ParseControlHeadBraceRequiresGrouping => {
                "PARSE_CONTROL_HEAD_BRACE_REQUIRES_GROUPING"
            }
            Self::ParseExpectedExpression => "PARSE_EXPECTED_EXPRESSION",
            Self::ParseExpectedPattern => "PARSE_EXPECTED_PATTERN",
            Self::ParseInvalidRangeSyntax => "PARSE_INVALID_RANGE_SYNTAX",
            Self::ParseInvalidRestPosition => "PARSE_INVALID_REST_POSITION",
            Self::ParseResourceLimitExceeded => "PARSE_RESOURCE_LIMIT_EXCEEDED",
            Self::ParseUnclosedDelimiter => "PARSE_UNCLOSED_DELIMITER",
            Self::ParseUnexpectedToken => "PARSE_UNEXPECTED_TOKEN",
        }
    }

    /// Returns the phase that owns this cause kind.
    #[must_use]
    pub const fn phase(self) -> SyntaxPhase {
        match self {
            Self::LexInvalidChar
            | Self::LexInvalidEscape
            | Self::LexInvalidIdentifierChar
            | Self::LexInvalidNumber
            | Self::LexNestedBlockComment
            | Self::LexUnterminatedBlockComment
            | Self::LexUnterminatedString => SyntaxPhase::Lex,
            Self::ParseControlHeadBraceRequiresGrouping
            | Self::ParseExpectedExpression
            | Self::ParseExpectedPattern
            | Self::ParseInvalidRangeSyntax
            | Self::ParseInvalidRestPosition
            | Self::ParseResourceLimitExceeded
            | Self::ParseUnclosedDelimiter
            | Self::ParseUnexpectedToken => SyntaxPhase::Parse,
        }
    }
}

/// Parser-owned resource whose deterministic limit stopped parsing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseResource {
    RecursiveDepth,
    DelimiterDepth,
    EventCount,
}

impl ParseResource {
    /// Returns the stable diagnostic-data spelling for this resource.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RecursiveDepth => "recursive_depth",
            Self::DelimiterDepth => "delimiter_depth",
            Self::EventCount => "event_count",
        }
    }
}

/// Structured data for `PARSE_RESOURCE_LIMIT_EXCEEDED`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseResourceLimit {
    resource: ParseResource,
    limit: i64,
    observed_at_least: i64,
}

impl ParseResourceLimit {
    pub(crate) const fn new(resource: ParseResource, limit: i64, observed_at_least: i64) -> Self {
        Self {
            resource,
            limit,
            observed_at_least,
        }
    }

    /// Returns the parser resource that exhausted its budget.
    #[must_use]
    pub const fn resource(self) -> ParseResource {
        self.resource
    }

    /// Returns the deterministic limit active for this parse.
    #[must_use]
    pub const fn limit(self) -> i64 {
        self.limit
    }

    /// Returns the smallest observed value known to exceed the limit.
    #[must_use]
    pub const fn observed_at_least(self) -> i64 {
        self.observed_at_least
    }
}

/// Stable syntax cause bound to one exact source range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SyntaxDiagnostic {
    kind: SyntaxDiagnosticKind,
    span: FileSpan,
    recovery_span: FileSpan,
    parse_resource_limit: Option<ParseResourceLimit>,
}

impl SyntaxDiagnostic {
    /// Binds a stable cause kind to its primary source span.
    pub(crate) const fn new(kind: SyntaxDiagnosticKind, span: FileSpan) -> Self {
        Self {
            kind,
            span,
            recovery_span: span,
            parse_resource_limit: None,
        }
    }

    pub(crate) const fn parse_resource_limit(
        span: FileSpan,
        resource: ParseResource,
        limit: i64,
        observed_at_least: i64,
    ) -> Self {
        Self {
            kind: SyntaxDiagnosticKind::ParseResourceLimitExceeded,
            span,
            recovery_span: span,
            parse_resource_limit: Some(ParseResourceLimit::new(resource, limit, observed_at_least)),
        }
    }

    pub(crate) const fn set_recovery_span(&mut self, recovery_span: FileSpan) {
        self.recovery_span = recovery_span;
    }

    /// Returns the stable cause kind.
    #[must_use]
    pub const fn kind(self) -> SyntaxDiagnosticKind {
        self.kind
    }

    /// Returns the owning front-end phase.
    #[must_use]
    pub const fn phase(self) -> SyntaxPhase {
        self.kind.phase()
    }

    /// Returns the exact primary source span.
    #[must_use]
    pub const fn span(self) -> FileSpan {
        self.span
    }

    /// Returns the parser region whose recovery was authorized by this cause.
    #[must_use]
    pub const fn recovery_span(self) -> FileSpan {
        self.recovery_span
    }

    /// Returns structured resource data when parsing stopped at a resource limit.
    #[must_use]
    pub const fn resource_limit(self) -> Option<ParseResourceLimit> {
        self.parse_resource_limit
    }
}
