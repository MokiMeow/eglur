use std::alloc::Layout;
use std::fmt;

use crate::{ExpressionKey, GenericParameterId, GenericParameterSignature, MethodDispatchSite};

use super::{
    AnalysisRootTicket, AnalysisWork, AnalysisWorkAccounting, AnalysisWorkCategory,
    AnalysisWorkFailure, AnalysisWorkOverflow,
};

mod bounds;
mod dispatch;
mod registry;

pub use bounds::AnalysisBoundInputs;
pub use dispatch::AnalysisDispatchInputs;
pub use registry::{AnalysisProofInputRegistry, AnalysisProofRegistryFailure};

/// Invalid original proof custody, distinct from negative semantic evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisProofInputFailure {
    UnknownDeclaredBound { site: ExpressionKey, index: usize },
    UnadmittedBound { site: ExpressionKey, index: usize },
    Work(AnalysisWorkFailure),
}

impl From<AnalysisWorkFailure> for AnalysisProofInputFailure {
    fn from(failure: AnalysisWorkFailure) -> Self {
        Self::Work(failure)
    }
}

impl fmt::Display for AnalysisProofInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownDeclaredBound { site, index } => {
                write!(formatter, "unknown declared bound {index} at {site:?}")
            }
            Self::UnadmittedBound { site, index } => {
                write!(
                    formatter,
                    "declared bound {index} was not admitted at {site:?}"
                )
            }
            Self::Work(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for AnalysisProofInputFailure {}

const fn ticket_work() -> AnalysisWork {
    AnalysisWork {
        steps: 1,
        inspected_bytes: size_of::<Option<AnalysisRootTicket>>() as u64,
        transferred_bytes: size_of::<AnalysisRootTicket>() as u64,
        reserved_bytes: 0,
    }
}

const fn no_work() -> AnalysisWork {
    AnalysisWork {
        steps: 0,
        inspected_bytes: 0,
        transferred_bytes: 0,
        reserved_bytes: 0,
    }
}

fn charge_ticket_retrieval(
    accounting: &mut AnalysisWorkAccounting,
    ticket: &AnalysisRootTicket,
    additional: AnalysisWork,
) -> Result<(), AnalysisWorkFailure> {
    let mut scope = accounting.claim(ticket)?;
    match ticket_work().checked_add(additional) {
        Ok(work) => scope.precharge(work),
        Err(overflow) => Err(scope.stop_unrepresentable(overflow)),
    }
}

#[cfg(test)]
mod tests;
