mod analysis;
mod body;
mod diagnostics;
mod model;

pub use analysis::{OwnershipAnalysisError, analyze_ownership};
pub use model::{OwnershipBodyStatus, OwnershipBodySummary, OwnershipFacts, OwnershipRootFact};

pub(crate) use body::analyze_body;
pub(crate) use diagnostics::emit_ownership_diagnostics;
pub(crate) use model::{OwnershipAvailability, OwnershipFailure, OwnershipFailureKind};
