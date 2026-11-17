//! Canonical semantic diagnostics, root retention, poison, and presentation.

#![forbid(unsafe_code)]

mod model;
mod render;
mod root;
mod snapshot;
mod span;
mod store;

pub use model::{
    Diagnostic, DiagnosticCode, DiagnosticDataValue, DiagnosticPhase, RelatedOrder, Severity,
};
pub use render::render_plain;
pub use root::{BoundaryRootKey, DiagnosticRoot, DiagnosticRootError};
pub use snapshot::{AcceptedDiagnostics, DiagnosticSnapshot, ErrorLimit, RejectedDiagnostics};
pub use span::{
    PrimarySpan, RelatedSpan, SourceLabel, SourceProjectionError, SourceSpan, SourceTextRange,
};
pub use store::{
    DiagnosticCheckpoint, DiagnosticFailure, DiagnosticStore, DiagnosticStoreError, Poison,
};
