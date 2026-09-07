use std::fmt;
use std::sync::Arc;

use eglur_diagnostics::SourceLabel;

const SOURCE_EXTENSION: &str = ".eglur";

/// One unvalidated virtual file supplied at the compiler boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInput {
    path: Box<str>,
    source_label: Box<str>,
    bytes: Arc<[u8]>,
}

impl FileInput {
    /// Creates one exact path-to-bytes mapping entry whose diagnostic label is its path.
    #[must_use]
    pub fn new(path: impl Into<Box<str>>, bytes: impl Into<Arc<[u8]>>) -> Self {
        let path = path.into();
        Self {
            source_label: path.clone(),
            path,
            bytes: bytes.into(),
        }
    }

    /// Creates one mapping with a diagnostic source identity distinct from its path.
    #[must_use]
    pub fn with_source_label(
        path: impl Into<Box<str>>,
        source_label: impl Into<Box<str>>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Self {
        Self {
            path: path.into(),
            source_label: source_label.into(),
            bytes: bytes.into(),
        }
    }

    /// Returns the path exactly as supplied by the invoker.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the diagnostic source identity exactly as supplied by the invoker.
    #[must_use]
    pub fn source_label(&self) -> &str {
        &self.source_label
    }

    /// Returns the exact mapped bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Canonical validated in-memory file-set invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSetRequest {
    files: Box<[CanonicalFileInput]>,
    entry: LogicalPath,
    root: LogicalRoot,
}

impl FileSetRequest {
    /// Validates and canonicalizes a virtual file-set request.
    pub fn new(
        files: impl IntoIterator<Item = FileInput>,
        entry: impl Into<Box<str>>,
        logical_root: Option<&str>,
    ) -> Result<Self, FileSetError> {
        let mut files = files.into_iter().collect::<Vec<_>>();
        if files.is_empty() {
            return Err(FileSetError::EmptyFileSet);
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));

        for file in &files {
            if !is_canonical_relative_posix_path(&file.path) {
                return Err(FileSetError::InvalidPath {
                    path: file.path.clone(),
                });
            }
        }
        if let Some(pair) = files.windows(2).find(|pair| pair[0].path == pair[1].path) {
            return Err(FileSetError::DuplicatePath {
                path: pair[0].path.clone(),
            });
        }

        let mut source_labels = std::collections::BTreeSet::new();
        let files = files
            .into_iter()
            .map(|file| {
                let source_label = if is_source_path(&file.path) {
                    let Some(label) = SourceLabel::new(file.source_label) else {
                        return Err(FileSetError::InvalidSourceLabel { path: file.path });
                    };
                    if !source_labels.insert(label.clone()) {
                        return Err(FileSetError::DuplicateSourceLabel {
                            label: label.as_str().into(),
                        });
                    }
                    Some(label)
                } else {
                    None
                };
                Ok(CanonicalFileInput {
                    path: LogicalPath(file.path),
                    source_label,
                    bytes: file.bytes,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let entry = entry.into();
        if !is_canonical_relative_posix_path(&entry) {
            return Err(FileSetError::InvalidEntryPath { entry });
        }
        let Some(entry_file) = files
            .iter()
            .find(|file| file.path.as_str() == entry.as_ref())
        else {
            return Err(FileSetError::EntryMissing { entry });
        };
        if !is_source_path(entry_file.path.as_str()) {
            return Err(FileSetError::EntryNotSource { entry });
        }

        let source_count = files
            .iter()
            .filter(|file| file.source_label.is_some())
            .count();
        let root = match (source_count, logical_root) {
            (1, None) => LogicalRoot::Current,
            (1, Some(root)) => {
                return Err(FileSetError::ExplicitRootForSingleSource { root: root.into() });
            }
            (_, None) => return Err(FileSetError::MissingRootForMultipleSources),
            (_, Some(".")) => LogicalRoot::Current,
            (_, Some(root)) if is_canonical_relative_posix_path(root) => {
                LogicalRoot::Path(LogicalPath(root.into()))
            }
            (_, Some(root)) => return Err(FileSetError::InvalidRoot { root: root.into() }),
        };

        for file in files.iter().filter(|file| file.source_label.is_some()) {
            if root.relative_source_path(&file.path).is_none() {
                return Err(FileSetError::SourceOutsideRoot {
                    source: file.path.as_str().into(),
                    root: root.as_str().into(),
                });
            }
        }

        Ok(Self {
            files: files.into_boxed_slice(),
            entry: LogicalPath(entry),
            root,
        })
    }

    /// Creates the baseline implicit-root request for one source.
    pub fn single(
        path: impl Into<Box<str>>,
        bytes: impl Into<Arc<[u8]>>,
    ) -> Result<Self, FileSetError> {
        let path = path.into();
        Self::new([FileInput::new(path.clone(), bytes)], path, None)
    }

    /// Returns the designated entry path.
    #[must_use]
    pub fn entry(&self) -> &str {
        self.entry.as_str()
    }

    /// Returns the established logical root.
    #[must_use]
    pub fn logical_root(&self) -> &str {
        self.root.as_str()
    }

    pub(crate) fn files(&self) -> &[CanonicalFileInput] {
        &self.files
    }

    pub(crate) const fn root(&self) -> &LogicalRoot {
        &self.root
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct LogicalPath(Box<str>);

impl LogicalPath {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CanonicalFileInput {
    pub(crate) path: LogicalPath,
    pub(crate) source_label: Option<SourceLabel>,
    pub(crate) bytes: Arc<[u8]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LogicalRoot {
    Current,
    Path(LogicalPath),
}

impl LogicalRoot {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Self::Current => ".",
            Self::Path(path) => path.as_str(),
        }
    }

    pub(crate) fn relative_source_path<'path>(
        &self,
        path: &'path LogicalPath,
    ) -> Option<&'path str> {
        match self {
            Self::Current => Some(path.as_str()),
            Self::Path(root) => path
                .as_str()
                .strip_prefix(root.as_str())
                .and_then(|suffix| suffix.strip_prefix('/')),
        }
    }
}

/// Invalid file-set invocation configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileSetError {
    EmptyFileSet,
    InvalidPath { path: Box<str> },
    DuplicatePath { path: Box<str> },
    InvalidSourceLabel { path: Box<str> },
    DuplicateSourceLabel { label: Box<str> },
    InvalidEntryPath { entry: Box<str> },
    EntryMissing { entry: Box<str> },
    EntryNotSource { entry: Box<str> },
    ExplicitRootForSingleSource { root: Box<str> },
    MissingRootForMultipleSources,
    InvalidRoot { root: Box<str> },
    SourceOutsideRoot { source: Box<str>, root: Box<str> },
}

impl fmt::Display for FileSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFileSet => formatter.write_str("file set must contain at least one mapping"),
            Self::InvalidPath { path } => {
                write!(
                    formatter,
                    "mapped path `{path}` is not canonical relative POSIX syntax"
                )
            }
            Self::DuplicatePath { path } => write!(formatter, "mapped path `{path}` is duplicated"),
            Self::InvalidSourceLabel { path } => {
                write!(formatter, "source `{path}` has an empty diagnostic label")
            }
            Self::DuplicateSourceLabel { label } => {
                write!(formatter, "diagnostic source label `{label}` is duplicated")
            }
            Self::InvalidEntryPath { entry } => write!(
                formatter,
                "entry path `{entry}` is not canonical relative POSIX syntax"
            ),
            Self::EntryMissing { entry } => {
                write!(
                    formatter,
                    "entry path `{entry}` is not present in the file set"
                )
            }
            Self::EntryNotSource { entry } => {
                write!(formatter, "entry path `{entry}` is not an .eglur source")
            }
            Self::ExplicitRootForSingleSource { root } => write!(
                formatter,
                "single-source file set must not provide logical root `{root}`"
            ),
            Self::MissingRootForMultipleSources => {
                formatter.write_str("multi-source file set requires an explicit logical root")
            }
            Self::InvalidRoot { root } => write!(
                formatter,
                "logical root `{root}` is not canonical relative POSIX syntax"
            ),
            Self::SourceOutsideRoot { source, root } => {
                write!(
                    formatter,
                    "source `{source}` is outside logical root `{root}`"
                )
            }
        }
    }
}

impl std::error::Error for FileSetError {}

pub(crate) fn is_source_path(path: &str) -> bool {
    path.ends_with(SOURCE_EXTENSION)
}

fn is_canonical_relative_posix_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    let drive_qualified = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if value.is_empty()
        || value.starts_with('/')
        || drive_qualified
        || value.contains('\\')
        || value.contains('\0')
    {
        return false;
    }
    value
        .split('/')
        .all(|component| !matches!(component, "" | "." | ".."))
}

#[cfg(test)]
mod tests {
    use super::{FileInput, FileSetError, FileSetRequest};

    fn file(path: &str) -> FileInput {
        FileInput::new(path, b"".to_vec())
    }

    #[test]
    fn request_rejects_noncanonical_and_duplicate_paths_deterministically() {
        let invalid = FileSetRequest::new(
            [file("z.eglur"), file("a/../bad.eglur")],
            "z.eglur",
            Some("."),
        );

        for path in [
            "",
            "/absolute.eglur",
            "C:drive.eglur",
            "a//empty.eglur",
            "./dot.eglur",
            "a/./dot.eglur",
            "a/../parent.eglur",
            "a\\backslash.eglur",
            "nul\0byte.eglur",
        ] {
            assert!(matches!(
                FileSetRequest::new([file("main.eglur"), file(path)], "main.eglur", Some(".")),
                Err(FileSetError::InvalidPath { .. })
            ));
        }
        assert_eq!(
            invalid,
            Err(FileSetError::InvalidPath {
                path: "a/../bad.eglur".into()
            })
        );

        let duplicate = FileSetRequest::new(
            [file("main.eglur"), file("main.eglur")],
            "main.eglur",
            Some("."),
        );
        assert_eq!(
            duplicate,
            Err(FileSetError::DuplicatePath {
                path: "main.eglur".into()
            })
        );
    }

    #[test]
    fn request_validates_source_labels_without_constraining_support_mappings() {
        let labeled = FileInput::with_source_label("main.eglur", "code", b"".to_vec());
        assert_eq!(labeled.path(), "main.eglur");
        assert_eq!(labeled.source_label(), "code");

        assert_eq!(
            FileSetRequest::new(
                [FileInput::with_source_label("main.eglur", "", b"".to_vec())],
                "main.eglur",
                None
            ),
            Err(FileSetError::InvalidSourceLabel {
                path: "main.eglur".into()
            })
        );
        assert_eq!(
            FileSetRequest::new(
                [
                    FileInput::with_source_label("a.eglur", "code", b"".to_vec()),
                    FileInput::with_source_label("b.eglur", "code", b"".to_vec()),
                ],
                "a.eglur",
                Some(".")
            ),
            Err(FileSetError::DuplicateSourceLabel {
                label: "code".into()
            })
        );

        let request = FileSetRequest::new(
            [
                FileInput::with_source_label("asset.bin", "", b"".to_vec()),
                file("main.eglur"),
            ],
            "main.eglur",
            None,
        )
        .unwrap();
        assert_eq!(request.entry(), "main.eglur");
    }

    #[test]
    fn root_rules_count_only_sources_and_allow_support_files_outside_root() {
        let request = FileSetRequest::new(
            [
                file("assets/data.bin"),
                file("src/a.eglur"),
                file("src/b.eglur"),
            ],
            "src/a.eglur",
            Some("src"),
        )
        .unwrap();
        assert_eq!(request.logical_root(), "src");

        assert_eq!(
            FileSetRequest::single("main.eglur", b"".to_vec())
                .unwrap()
                .logical_root(),
            "."
        );
        assert!(matches!(
            FileSetRequest::new(
                [file("src/a.eglur"), file("other/b.eglur")],
                "src/a.eglur",
                Some("src")
            ),
            Err(FileSetError::SourceOutsideRoot { .. })
        ));
        assert!(matches!(
            FileSetRequest::new([file("main.eglur")], "main.eglur", Some(".")),
            Err(FileSetError::ExplicitRootForSingleSource { .. })
        ));
        assert!(matches!(
            FileSetRequest::new([file("a.eglur"), file("b.eglur")], "a.eglur", None),
            Err(FileSetError::MissingRootForMultipleSources)
        ));
        assert!(matches!(
            FileSetRequest::new(
                [file("a.eglur"), file("b.eglur")],
                "a.eglur",
                Some("../bad")
            ),
            Err(FileSetError::InvalidRoot { .. })
        ));
        assert!(matches!(
            FileSetRequest::new([file("main.eglur")], "missing.eglur", None),
            Err(FileSetError::EntryMissing { .. })
        ));
        assert!(matches!(
            FileSetRequest::new([file("main.eglur"), file("notes.txt")], "notes.txt", None),
            Err(FileSetError::EntryNotSource { .. })
        ));
    }
}
