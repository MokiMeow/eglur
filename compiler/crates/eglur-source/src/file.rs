use std::fmt;
use std::sync::Arc;

use crate::{ByteOffset, ByteRange, FileId, FileSpan};

const MAX_SOURCE_LEN: usize = u32::MAX as usize;
const UTF8_BOM: &str = "\u{feff}";

/// Immutable arbitrary source bytes with an externally assigned identity.
#[derive(Clone, Debug)]
pub struct SourceFile {
    id: FileId,
    bytes: Arc<[u8]>,
}

impl SourceFile {
    /// Creates a source file without validating or normalizing its bytes.
    #[must_use]
    pub fn new(id: FileId, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            id,
            bytes: bytes.into(),
        }
    }

    /// Returns the driver-owned source identity.
    #[must_use]
    pub const fn id(&self) -> FileId {
        self.id
    }

    /// Returns the exact immutable input bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the physical byte length.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Reports whether the physical source is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Validates the source-size and UTF-8 preconditions without changing bytes.
    pub fn validate(&self) -> Result<ValidatedSource<'_>, SourceValidationError> {
        checked_source_len(self.id, self.bytes.len())?;
        match std::str::from_utf8(&self.bytes) {
            Ok(text) => Ok(ValidatedSource { source: self, text }),
            Err(error) => {
                let start = u32::try_from(error.valid_up_to())
                    .expect("source size was checked before UTF-8 validation");
                let error_len = error.error_len().unwrap_or_else(|| {
                    self.bytes
                        .len()
                        .checked_sub(error.valid_up_to())
                        .expect("valid UTF-8 prefix cannot exceed the source")
                });
                let end = start
                    .checked_add(
                        u32::try_from(error_len)
                            .expect("UTF-8 error length is bounded by source length"),
                    )
                    .expect("UTF-8 error range is bounded by source length");
                let range = ByteRange::new(ByteOffset::new(start), ByteOffset::new(end))
                    .expect("UTF-8 error range is ordered");
                Err(SourceValidationError::InvalidUtf8(FileSpan::new(
                    self.id, range,
                )))
            }
        }
    }
}

/// UTF-8 view proven to retain the exact bytes of one `SourceFile`.
#[derive(Clone, Copy, Debug)]
pub struct ValidatedSource<'source> {
    source: &'source SourceFile,
    text: &'source str,
}

impl<'source> ValidatedSource<'source> {
    /// Returns the source identity.
    #[must_use]
    pub const fn id(self) -> FileId {
        self.source.id
    }

    /// Returns the original source container.
    #[must_use]
    pub const fn source(self) -> &'source SourceFile {
        self.source
    }

    /// Returns the physical text, including a leading BOM and exact line endings.
    #[must_use]
    pub const fn text(self) -> &'source str {
        self.text
    }

    /// Returns the exact physical bytes.
    #[must_use]
    pub const fn bytes(self) -> &'source [u8] {
        self.text.as_bytes()
    }

    /// Returns the checked physical byte length.
    #[must_use]
    pub fn len(self) -> ByteOffset {
        ByteOffset::new(
            u32::try_from(self.text.len()).expect("validated source length is bounded by u32"),
        )
    }

    /// Reports whether the physical source is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.text.is_empty()
    }

    /// Reports whether the exact source begins with the UTF-8 BOM scalar.
    #[must_use]
    pub fn has_leading_bom(self) -> bool {
        self.text.starts_with(UTF8_BOM)
    }

    /// Returns the first logical source byte after an ignored leading BOM.
    #[must_use]
    pub fn logical_start(self) -> ByteOffset {
        ByteOffset::new(if self.has_leading_bom() { 3 } else { 0 })
    }
}

/// Controlled source-boundary rejection before tokenization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceValidationError {
    /// The source cannot be represented by the syntax tree's `u32` ranges.
    TooLarge {
        /// Source identity supplied by the driver.
        file: FileId,
        /// Exact physical byte length.
        actual_len: usize,
        /// Largest supported physical byte length.
        max_len: usize,
    },
    /// The exact invalid byte sequence starts at this span.
    InvalidUtf8(FileSpan),
}

impl fmt::Display for SourceValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge {
                file,
                actual_len,
                max_len,
            } => write!(
                formatter,
                "source {file} has {actual_len} bytes; maximum supported length is {max_len}",
            ),
            Self::InvalidUtf8(span) => write!(
                formatter,
                "source {} contains invalid UTF-8 at byte {}",
                span.file(),
                span.range().start().get(),
            ),
        }
    }
}

impl std::error::Error for SourceValidationError {}

fn checked_source_len(file: FileId, actual_len: usize) -> Result<(), SourceValidationError> {
    if actual_len <= MAX_SOURCE_LEN {
        Ok(())
    } else {
        Err(SourceValidationError::TooLarge {
            file,
            actual_len,
            max_len: MAX_SOURCE_LEN,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_SOURCE_LEN, SourceFile, SourceValidationError, checked_source_len};
    use crate::{ByteOffset, FileId};

    #[test]
    fn source_preserves_arbitrary_bytes_and_external_identity() {
        let source = SourceFile::new(FileId::new(17), vec![0xff, 0x00]);

        assert_eq!(source.id(), FileId::new(17));
        assert_eq!(source.bytes(), &[0xff, 0x00]);
        assert_eq!(source.len(), 2);
        assert!(!source.is_empty());
    }

    #[test]
    fn invalid_utf8_reports_the_exact_invalid_sequence() {
        let source = SourceFile::new(FileId::new(2), b"ok\xfftail".to_vec());
        let error = source
            .validate()
            .expect_err("source must reject invalid UTF-8");

        let SourceValidationError::InvalidUtf8(span) = error else {
            panic!("expected invalid UTF-8 rejection");
        };
        assert_eq!(span.file(), FileId::new(2));
        assert_eq!(span.range().start(), ByteOffset::new(2));
        assert_eq!(span.range().end(), ByteOffset::new(3));
    }

    #[test]
    fn validation_retains_bom_and_crlf_bytes() {
        let bytes = "\u{feff}a\r\nb\n".as_bytes().to_vec();
        let source = SourceFile::new(FileId::new(4), bytes.clone());
        let validated = source.validate().expect("source should be valid UTF-8");

        assert_eq!(validated.bytes(), bytes);
        assert_eq!(validated.text(), "\u{feff}a\r\nb\n");
        assert!(validated.has_leading_bom());
        assert_eq!(validated.logical_start(), ByteOffset::new(3));
        assert_eq!(validated.source().id(), source.id());
    }

    #[test]
    fn checked_length_rejects_an_unrepresentable_source_without_allocating_it() {
        let file = FileId::new(9);
        assert!(checked_source_len(file, MAX_SOURCE_LEN).is_ok());
        assert_eq!(
            checked_source_len(file, MAX_SOURCE_LEN + 1),
            Err(SourceValidationError::TooLarge {
                file,
                actual_len: MAX_SOURCE_LEN + 1,
                max_len: MAX_SOURCE_LEN,
            })
        );
    }
}
