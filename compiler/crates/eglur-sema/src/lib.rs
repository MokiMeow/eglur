//! Canonical declarations, semantic facts, and bounded name resolution.

#![forbid(unsafe_code)]

mod analysis_work;
mod body_types;
mod capability;
mod declaration;
mod declaration_graph;
mod diagnostic;
mod effects;
mod environment;
mod fact;
mod fingerprint;
mod flow;
mod identity;
mod imports;
mod inhabitance;
mod integer;
mod language_items;
mod overloads;
mod owned_poison;
mod ownership;
mod program;
mod record_reservation;
mod resolve;
mod signatures;
mod source;
mod trait_evidence;
mod traits;
mod type_namespace;
mod type_patterns;
mod type_rendering;
mod types;

pub use analysis_work::{
    AnalysisCallInputFailure, AnalysisCallInputRegistry, AnalysisCallInputs,
    AnalysisCallRegistryFailure, AnalysisRootBalance, AnalysisRootClaimError, AnalysisRootIdentity,
    AnalysisRootScope, AnalysisRootStopReason, AnalysisRootTicket, AnalysisSourceInputFailure,
    AnalysisSourceInputRegistry, AnalysisSourceInputs, AnalysisWork, AnalysisWorkAccounting,
    AnalysisWorkCategory, AnalysisWorkFailure, AnalysisWorkIncomplete, AnalysisWorkOverflow,
    AnalysisWorkPricing, AnalysisWorkUnits,
};
pub use body_types::*;
pub use capability::*;
pub use declaration::*;
pub use effects::*;
pub use environment::*;
pub use fact::*;
pub use fingerprint::*;
pub use flow::*;
pub use identity::*;
pub use imports::*;
pub use inhabitance::*;
pub use integer::*;
pub use language_items::*;
pub use ownership::*;
pub use program::*;
pub use resolve::*;
pub use signatures::*;
pub use source::*;
pub use trait_evidence::*;
pub use traits::*;
pub use types::*;
