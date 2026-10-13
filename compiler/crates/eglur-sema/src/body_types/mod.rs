mod checker;
mod error;
mod execution;
mod facts;

pub use checker::{typecheck_bodies, typecheck_bodies_with_work};
pub use error::BodyTypingError;
pub use execution::{BodyExecution, BodyTypingPrefix, InterruptedBody};
pub use facts::*;
