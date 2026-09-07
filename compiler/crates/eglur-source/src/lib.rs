//! Production boundary for source identity, bytes, spans, and coordinates.

#![forbid(unsafe_code)]

mod file;
mod line_index;
mod span;

pub use file::{SourceFile, SourceValidationError, ValidatedSource};
pub use line_index::{CoordinateError, LineIndex, SourceCoordinate};
pub use span::{ByteOffset, ByteRange, FileId, FileSpan};
