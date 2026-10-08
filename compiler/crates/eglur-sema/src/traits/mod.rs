mod builder;
mod coherence_index;
mod environment;
mod error;
mod model;
mod overlap;

pub use builder::build_trait_environment;
pub(crate) use builder::{CheckedMethodBound, checked_method_bound_mapping};
pub(crate) use coherence_index::CoherenceIndex;
pub use environment::TraitEnvironment;
pub use error::TraitEnvironmentError;
pub(crate) use model::TraitReceiverCandidate;
pub use model::{
    ImplArgumentViolation, TraitImplEvidence, TraitNormalization, TraitObligation, TraitProof,
};
pub(crate) use overlap::{
    HeaderConflict, TraitHeaderFacts, classify_overlap, header_mentions_parameter,
    header_parameter_below_error_union,
};
