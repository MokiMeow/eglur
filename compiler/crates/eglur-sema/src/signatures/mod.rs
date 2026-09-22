mod applications;
mod error;
mod generics;
mod model;
mod resolver;
mod trusted;
mod trusted_method_contracts;

pub use error::SignatureResolutionError;
pub use generics::GenericParameterSignature;
pub use model::*;
pub use resolver::{SignatureResolutionInputs, resolve_declaration_signatures};
pub use trusted::*;

pub(crate) use applications::{TypeApplication, TypeApplicationSite};
