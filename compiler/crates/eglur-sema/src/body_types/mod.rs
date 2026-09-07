mod checker;
mod error;
mod facts;

pub use checker::typecheck_bodies;
pub use error::BodyTypingError;
pub use facts::*;
