mod builder;
mod environment;
mod error;
mod model;
mod overlap;

pub use builder::build_trait_environment;
pub use environment::TraitEnvironment;
pub use error::TraitEnvironmentError;
pub(crate) use model::TraitReceiverCandidate;
pub use model::{TraitImplEvidence, TraitNormalization, TraitObligation, TraitProof};
pub(crate) use overlap::{
    HeaderConflict, TraitHeaderFacts, classify_overlap, header_mentions_parameter,
    header_parameter_below_error_union,
};
