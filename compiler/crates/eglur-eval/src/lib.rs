//! Bounded reference execution for successfully checked Eglur programs.

#![forbid(unsafe_code)]

mod capability;
mod evaluator;
mod value;

pub use capability::*;
pub use evaluator::*;
pub use value::*;
