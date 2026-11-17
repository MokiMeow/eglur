mod applications;
mod error;
mod execution;
mod generics;
mod inherent;
mod model;
pub(crate) mod public_contracts;
mod resolver;
mod trusted;
mod trusted_inherent;
mod trusted_method_contracts;
mod work;

pub use error::SignatureResolutionError;
pub use execution::{InterruptedSignatures, SignatureExecution, SignatureResolutionPrefix};
pub use generics::GenericParameterSignature;
pub use inherent::*;
pub use model::*;
pub use resolver::{
    SignatureResolutionInputs, resolve_declaration_signatures,
    resolve_declaration_signatures_with_work,
};
pub use trusted::*;
pub use work::SignatureAnalysisWork;

pub(crate) use applications::{TypeApplication, TypeApplicationRegion, TypeApplicationSite};
pub(crate) use generics::InstantiatedParameterBound;
