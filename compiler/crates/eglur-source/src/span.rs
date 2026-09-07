use std::fmt;

/// Stable identity assigned by the compiler driver to one source file.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FileId(u32);

impl FileId {
    /// Creates a file identity from its driver-owned integer.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the driver-owned integer identity.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for FileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Checked byte offset accepted by the lossless syntax tree.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteOffset(u32);

impl ByteOffset {
    /// Creates an offset from an already bounded byte index.
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the zero-based byte index.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Converts the offset to a platform index without loss.
    #[must_use]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

/// Half-open checked byte range in one source file.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ByteRange {
    start: ByteOffset,
    end: ByteOffset,
}

impl ByteRange {
    /// Creates a range when `start <= end`.
    #[must_use]
    pub const fn new(start: ByteOffset, end: ByteOffset) -> Option<Self> {
        if start.get() <= end.get() {
            Some(Self { start, end })
        } else {
            None
        }
    }

    /// Creates one zero-width range at an insertion point.
    #[must_use]
    pub const fn empty(offset: ByteOffset) -> Self {
        Self {
            start: offset,
            end: offset,
        }
    }

    /// Returns the inclusive start offset.
    #[must_use]
    pub const fn start(self) -> ByteOffset {
        self.start
    }

    /// Returns the exclusive end offset.
    #[must_use]
    pub const fn end(self) -> ByteOffset {
        self.end
    }

    /// Returns the byte length.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.end.get() - self.start.get()
    }

    /// Reports whether the range is zero-width.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start.get() == self.end.get()
    }
}

/// One source-bound byte range without a filesystem path dependency.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FileSpan {
    file: FileId,
    range: ByteRange,
}

impl FileSpan {
    /// Binds a checked byte range to one source identity.
    #[must_use]
    pub const fn new(file: FileId, range: ByteRange) -> Self {
        Self { file, range }
    }

    /// Returns the source identity.
    #[must_use]
    pub const fn file(self) -> FileId {
        self.file
    }

    /// Returns the half-open byte range.
    #[must_use]
    pub const fn range(self) -> ByteRange {
        self.range
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteOffset, ByteRange, FileId, FileSpan};

    #[test]
    fn ranges_reject_reversed_offsets_and_preserve_identity() {
        let start = ByteOffset::new(2);
        let end = ByteOffset::new(7);
        let range = ByteRange::new(start, end).expect("ordered range should be valid");
        let span = FileSpan::new(FileId::new(3), range);

        assert_eq!(range.start(), start);
        assert_eq!(range.end(), end);
        assert_eq!(range.len(), 5);
        assert!(!range.is_empty());
        assert_eq!(span.file().get(), 3);
        assert_eq!(span.range(), range);
        assert!(ByteRange::new(end, start).is_none());
    }

    #[test]
    fn empty_ranges_are_unambiguous_insertion_points() {
        let offset = ByteOffset::new(11);
        let range = ByteRange::empty(offset);

        assert_eq!(range.start(), offset);
        assert_eq!(range.end(), offset);
        assert!(range.is_empty());
    }
}
