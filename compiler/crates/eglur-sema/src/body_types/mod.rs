mod checker;
mod error;
mod execution;
mod facts;
mod locals;

use locals::LocalTypeFacts;

pub use checker::{
    BodyAnalysisInput, BodyAnalysisWork, typecheck_bodies, typecheck_bodies_with_work,
};
pub use error::BodyTypingError;
pub use execution::{
    BodyExecution, BodyProofInputs, BodyTypingPrefix, BoundProofInput, CompletedBodyExecution,
    DispatchProofInput, InterruptedBody, MeteredBodyProofInputs,
};
pub use facts::*;
