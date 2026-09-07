//! Canonical declarations, semantic facts, and bounded name resolution.

#![forbid(unsafe_code)]

mod body_types;
mod capability;
mod declaration;
mod diagnostic;
mod effects;
mod environment;
mod fact;
mod fingerprint;
mod flow;
mod identity;
mod imports;
mod inhabitance;
mod language_items;
mod overloads;
mod ownership;
mod program;
mod resolve;
mod signatures;
mod source;
mod trait_evidence;
mod traits;
mod type_namespace;
mod type_patterns;
mod type_rendering;
mod types;

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
pub use language_items::*;
pub use ownership::*;
pub use program::*;
pub use resolve::*;
pub use signatures::*;
pub use source::*;
pub use trait_evidence::*;
pub use traits::*;
pub use types::*;
