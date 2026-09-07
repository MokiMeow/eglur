use std::fmt;

use crate::{ByteOffset, FileId, ValidatedSource};

/// One-based logical source position measured in Unicode scalar values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceCoordinate {
    line: u32,
    column: u32,
}

impl SourceCoordinate {
    /// Creates a one-based coordinate.
    #[must_use]
    pub const fn new(line: u32, column: u32) -> Option<Self> {
        if line == 0 || column == 0 {
            None
        } else {
            Some(Self { line, column })
        }
    }

    /// Returns the one-based logical line.
    #[must_use]
    pub const fn line(self) -> u32 {
        self.line
    }

    /// Returns the one-based Unicode-scalar column.
    #[must_use]
    pub const fn column(self) -> u32 {
        self.column
    }
}

/// Failure to project one physical byte offset into logical coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateError {
    /// The offset exceeds the physical source length.
    OutOfBounds {
        offset: ByteOffset,
        source_len: ByteOffset,
    },
    /// The offset lies inside a multi-byte UTF-8 scalar.
    NotCharBoundary(ByteOffset),
}

impl fmt::Display for CoordinateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { offset, source_len } => write!(
                formatter,
                "byte offset {} exceeds source length {}",
                offset.get(),
                source_len.get(),
            ),
            Self::NotCharBoundary(offset) => {
                write!(
                    formatter,
                    "byte offset {} splits a UTF-8 scalar",
                    offset.get()
                )
            }
        }
    }
}

impl std::error::Error for CoordinateError {}

/// Logical line starts derived from exact UTF-8 source bytes.
#[derive(Clone, Debug)]
pub struct LineIndex<'source> {
    source: ValidatedSource<'source>,
    logical_start: ByteOffset,
    line_starts: Box<[ByteOffset]>,
}

impl<'source> LineIndex<'source> {
    /// Indexes LF, CRLF, and lone CR endings at their physical byte offsets.
    #[must_use]
    pub fn new(source: ValidatedSource<'source>) -> Self {
        let bytes = source.bytes();
        let logical_start = source.logical_start();
        let mut line_starts = vec![logical_start];
        let mut index = logical_start.as_usize();

        while index < bytes.len() {
            match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                    index += 2;
                    line_starts.push(ByteOffset::new(
                        u32::try_from(index).expect("validated source length is bounded by u32"),
                    ));
                }
                b'\r' => {
                    index += 1;
                    line_starts.push(ByteOffset::new(
                        u32::try_from(index).expect("validated source length is bounded by u32"),
                    ));
                }
                b'\n' => {
                    index += 1;
                    line_starts.push(ByteOffset::new(
                        u32::try_from(index).expect("validated source length is bounded by u32"),
                    ));
                }
                _ => index += 1,
            }
        }

        Self {
            source,
            logical_start,
            line_starts: line_starts.into_boxed_slice(),
        }
    }

    /// Returns the source identity whose coordinates this index projects.
    #[must_use]
    pub const fn source_id(&self) -> FileId {
        self.source.id()
    }

    /// Returns the number of logical lines, including the final empty line.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        u32::try_from(self.line_starts.len())
            .expect("line count cannot exceed the bounded source byte length")
    }

    /// Projects an exact physical byte offset into a one-based logical position.
    pub fn coordinate(&self, offset: ByteOffset) -> Result<SourceCoordinate, CoordinateError> {
        let source_len = self.source.len();
        if offset > source_len {
            return Err(CoordinateError::OutOfBounds { offset, source_len });
        }
        let text = self.source.text();
        let mut physical_index = offset.as_usize();
        if !text.is_char_boundary(physical_index) {
            return Err(CoordinateError::NotCharBoundary(offset));
        }
        if physical_index < self.logical_start.as_usize() {
            physical_index = self.logical_start.as_usize();
        }
        if physical_index > 0
            && text.as_bytes()[physical_index - 1] == b'\r'
            && text.as_bytes().get(physical_index) == Some(&b'\n')
        {
            physical_index -= 1;
        }

        let line_index = self
            .line_starts
            .partition_point(|start| start.as_usize() <= physical_index)
            .saturating_sub(1);
        let line_start = self.line_starts[line_index].as_usize();
        let column = text[line_start..physical_index].chars().count() + 1;
        Ok(SourceCoordinate {
            line: u32::try_from(line_index + 1).expect("line count is bounded by source length"),
            column: u32::try_from(column).expect("column is bounded by source length"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{CoordinateError, LineIndex, SourceCoordinate};
    use crate::{ByteOffset, FileId, SourceFile};

    fn source(text: &str) -> SourceFile {
        SourceFile::new(FileId::new(1), text.as_bytes().to_vec())
    }

    #[test]
    fn bom_and_crlf_have_one_logical_coordinate_projection() {
        let source = source("\u{feff}a\r\nb\n");
        let validated = source.validate().unwrap();
        let lines = LineIndex::new(validated);

        assert_eq!(lines.line_count(), 3);
        assert_eq!(
            lines.coordinate(ByteOffset::new(0)).unwrap(),
            SourceCoordinate::new(1, 1).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(3)).unwrap(),
            SourceCoordinate::new(1, 1).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(4)).unwrap(),
            SourceCoordinate::new(1, 2).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(5)).unwrap(),
            SourceCoordinate::new(1, 2).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(6)).unwrap(),
            SourceCoordinate::new(2, 1).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(8)).unwrap(),
            SourceCoordinate::new(3, 1).unwrap()
        );
    }

    #[test]
    fn lone_cr_starts_one_new_logical_line() {
        let source = source("a\rb\r\nc\n");
        let validated = source.validate().unwrap();
        let lines = LineIndex::new(validated);

        assert_eq!(lines.line_count(), 4);
        assert_eq!(
            lines.coordinate(ByteOffset::new(2)).unwrap(),
            SourceCoordinate::new(2, 1).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(5)).unwrap(),
            SourceCoordinate::new(3, 1).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(7)).unwrap(),
            SourceCoordinate::new(4, 1).unwrap()
        );
    }

    #[test]
    fn columns_count_unicode_scalars_instead_of_bytes() {
        let source = source("aéz");
        let validated = source.validate().unwrap();
        let lines = LineIndex::new(validated);

        assert_eq!(
            lines.coordinate(ByteOffset::new(3)).unwrap(),
            SourceCoordinate::new(1, 3).unwrap()
        );
        assert_eq!(
            lines.coordinate(ByteOffset::new(2)),
            Err(CoordinateError::NotCharBoundary(ByteOffset::new(2)))
        );
    }

    #[test]
    fn coordinate_rejects_offsets_beyond_the_source() {
        let source = source("x");
        let validated = source.validate().unwrap();
        let lines = LineIndex::new(validated);

        assert_eq!(
            lines.coordinate(ByteOffset::new(2)),
            Err(CoordinateError::OutOfBounds {
                offset: ByteOffset::new(2),
                source_len: ByteOffset::new(1),
            })
        );
    }
}
