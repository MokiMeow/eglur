use std::cmp::Ordering;
use std::fmt;

use eglur_hir::SourceOrigin;
use eglur_source::{
    ByteOffset, ByteRange, CoordinateError, FileId, FileSpan, LineIndex, SourceCoordinate,
    SourceFile,
};

/// Canonical source-unit identity used by diagnostic interchange and ordering.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceLabel(Box<str>);

impl SourceLabel {
    /// Creates a non-empty diagnostic source identity.
    #[must_use]
    pub fn new(text: impl Into<Box<str>>) -> Option<Self> {
        let text = text.into();
        (!text.is_empty()).then_some(Self(text))
    }

    /// Returns the exact source identity spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Logical coordinate projection of one canonical physical byte range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceTextRange {
    start: SourceCoordinate,
    end: SourceCoordinate,
}

impl SourceTextRange {
    fn new(start: SourceCoordinate, end: SourceCoordinate) -> Self {
        Self { start, end }
    }

    /// Returns the inclusive logical start coordinate.
    #[must_use]
    pub const fn start(self) -> SourceCoordinate {
        self.start
    }

    /// Returns the exclusive logical end coordinate.
    #[must_use]
    pub const fn end(self) -> SourceCoordinate {
        self.end
    }

    fn cmp_full(self, other: Self) -> Ordering {
        coordinate_key(self.start)
            .cmp(&coordinate_key(other.start))
            .then_with(|| coordinate_key(self.end).cmp(&coordinate_key(other.end)))
    }
}

/// Failure to project a physical span with the supplied source line index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceProjectionError {
    /// The line index belongs to a different source file.
    FileMismatch {
        span_file: FileId,
        index_file: FileId,
    },
    /// A physical endpoint exceeds the indexed source.
    OutOfBounds(CoordinateError),
}

impl fmt::Display for SourceProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileMismatch {
                span_file,
                index_file,
            } => write!(
                formatter,
                "span belongs to source {span_file}, but line index belongs to source {index_file}",
            ),
            Self::OutOfBounds(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SourceProjectionError {}

/// Canonical physical diagnostic span with an optional derived text projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSpan {
    source: SourceLabel,
    file_span: FileSpan,
    text: Option<SourceTextRange>,
}

impl SourceSpan {
    /// Creates a byte-only span when no valid text projection is available.
    #[must_use]
    pub const fn byte_only(source: SourceLabel, file_span: FileSpan) -> Self {
        Self {
            source,
            file_span,
            text: None,
        }
    }

    /// Creates the canonical zero-width source-start blame location.
    ///
    /// This remains representable even when the complete source exceeds the
    /// syntax tree's size limit. Valid UTF-8 receives the required line-1,
    /// column-1 projection; undecodable bytes remain byte-only.
    #[must_use]
    pub fn source_start(source: SourceLabel, file: &SourceFile) -> Self {
        let file_span = FileSpan::new(file.id(), ByteRange::empty(ByteOffset::new(0)));
        let text = std::str::from_utf8(file.bytes()).is_ok().then(|| {
            let start =
                SourceCoordinate::new(1, 1).expect("one-based source start coordinate is valid");
            SourceTextRange::new(start, start)
        });
        Self {
            source,
            file_span,
            text,
        }
    }

    /// Derives text coordinates from the exact indexed source when possible.
    ///
    /// A scalar-splitting endpoint produces a valid byte-only span, as required
    /// by the diagnostic contract. Cross-file and out-of-bounds projections are
    /// rejected as producer errors.
    pub fn projected(
        source: SourceLabel,
        file_span: FileSpan,
        lines: &LineIndex<'_>,
    ) -> Result<Self, SourceProjectionError> {
        if file_span.file() != lines.source_id() {
            return Err(SourceProjectionError::FileMismatch {
                span_file: file_span.file(),
                index_file: lines.source_id(),
            });
        }

        let start = project_endpoint(lines, file_span.range().start())?;
        let end = project_endpoint(lines, file_span.range().end())?;
        let text = match (start, end) {
            (Some(start), Some(end)) => Some(SourceTextRange::new(start, end)),
            _ => None,
        };

        Ok(Self {
            source,
            file_span,
            text,
        })
    }

    /// Projects one HIR origin back to its exact user-written source range.
    pub fn from_origin(
        source: SourceLabel,
        origin: SourceOrigin,
        lines: &LineIndex<'_>,
    ) -> Result<Self, SourceProjectionError> {
        Self::projected(source, origin.source(), lines)
    }

    /// Returns the canonical diagnostic source identity.
    #[must_use]
    pub const fn source(&self) -> &SourceLabel {
        &self.source
    }

    /// Returns the exact internal file identity and physical byte range.
    #[must_use]
    pub const fn file_span(&self) -> FileSpan {
        self.file_span
    }

    /// Returns the derived logical projection when both endpoints are valid.
    #[must_use]
    pub const fn text(&self) -> Option<SourceTextRange> {
        self.text
    }

    pub(crate) fn cmp_position(&self, other: &Self) -> Ordering {
        self.source
            .cmp(&other.source)
            .then_with(|| {
                self.file_span
                    .range()
                    .start()
                    .cmp(&other.file_span.range().start())
            })
            .then_with(|| {
                self.file_span
                    .range()
                    .end()
                    .cmp(&other.file_span.range().end())
            })
    }

    fn cmp_full(&self, other: &Self) -> Ordering {
        self.cmp_position(other)
            .then_with(|| self.file_span.file().cmp(&other.file_span.file()))
            .then_with(|| cmp_optional_text(self.text, other.text))
    }
}

/// Primary diagnostic location or an explicitly source-unavailable boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrimarySpan {
    Source(SourceSpan),
    Unavailable,
}

impl PrimarySpan {
    /// Returns the source-bound primary span when one exists.
    #[must_use]
    pub const fn as_source(&self) -> Option<&SourceSpan> {
        match self {
            Self::Source(span) => Some(span),
            Self::Unavailable => None,
        }
    }

    pub(crate) fn cmp_position(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Source(left), Self::Source(right)) => left.cmp_position(right),
            (Self::Source(_), Self::Unavailable) => Ordering::Less,
            (Self::Unavailable, Self::Source(_)) => Ordering::Greater,
            (Self::Unavailable, Self::Unavailable) => Ordering::Equal,
        }
    }

    pub(crate) fn cmp_full(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Source(left), Self::Source(right)) => left.cmp_full(right),
            (Self::Source(_), Self::Unavailable) => Ordering::Less,
            (Self::Unavailable, Self::Source(_)) => Ordering::Greater,
            (Self::Unavailable, Self::Unavailable) => Ordering::Equal,
        }
    }
}

/// Labelled secondary source context for one retained diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelatedSpan {
    span: SourceSpan,
    label: Box<str>,
}

impl RelatedSpan {
    /// Creates a related span with a non-empty label.
    #[must_use]
    pub fn new(span: SourceSpan, label: impl Into<Box<str>>) -> Option<Self> {
        let label = label.into();
        (!label.is_empty()).then_some(Self { span, label })
    }

    /// Returns the related source location.
    #[must_use]
    pub const fn span(&self) -> &SourceSpan {
        &self.span
    }

    /// Returns the explanatory label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

impl Ord for RelatedSpan {
    fn cmp(&self, other: &Self) -> Ordering {
        self.span
            .cmp_position(&other.span)
            .then_with(|| self.label.cmp(&other.label))
            .then_with(|| self.span.cmp_full(&other.span))
    }
}

impl PartialOrd for RelatedSpan {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn project_endpoint(
    lines: &LineIndex<'_>,
    offset: eglur_source::ByteOffset,
) -> Result<Option<SourceCoordinate>, SourceProjectionError> {
    match lines.coordinate(offset) {
        Ok(coordinate) => Ok(Some(coordinate)),
        Err(CoordinateError::NotCharBoundary(_)) => Ok(None),
        Err(error @ CoordinateError::OutOfBounds { .. }) => {
            Err(SourceProjectionError::OutOfBounds(error))
        }
    }
}

fn coordinate_key(coordinate: SourceCoordinate) -> (u32, u32) {
    (coordinate.line(), coordinate.column())
}

fn cmp_optional_text(left: Option<SourceTextRange>, right: Option<SourceTextRange>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp_full(right),
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
