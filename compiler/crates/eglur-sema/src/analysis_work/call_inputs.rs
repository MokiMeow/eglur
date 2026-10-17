use std::alloc::Layout;
use std::fmt;

use eglur_hir::{Body, CallExpr, CallSyntax, ExprKind};

use crate::ExpressionKey;

use super::{
    AnalysisRootTicket, AnalysisWork, AnalysisWorkAccounting, AnalysisWorkCategory,
    AnalysisWorkFailure, AnalysisWorkOverflow,
};

mod registry;
mod set;
pub use registry::{AnalysisCallInputRegistry, AnalysisCallRegistryFailure};
pub use set::AnalysisBodyInputs;

pub(crate) fn is_original_call(call: &CallExpr) -> bool {
    !call.partial_application
        && matches!(
            call.syntax,
            CallSyntax::Direct | CallSyntax::ExplicitTypeArguments | CallSyntax::TrailingArgument
        )
}

fn original_call(body: &Body, site: ExpressionKey) -> Result<&CallExpr, AnalysisCallInputFailure> {
    if site.expression().body() == body.id()
        && (site.expression().local() as usize) < body.expressions().len()
        && let Some(ExprKind::Call(call)) =
            body.expression(site.expression()).map(|value| value.kind())
        && is_original_call(call)
    {
        return Ok(call);
    }
    Err(AnalysisCallInputFailure::NotOriginalCall(site))
}

/// Invalid source-call custody or an interrupted accounting operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisCallInputFailure {
    NotOriginalCall(ExpressionKey),
    UnknownArgument { call: ExpressionKey, index: usize },
    Work(AnalysisWorkFailure),
}

impl From<AnalysisWorkFailure> for AnalysisCallInputFailure {
    fn from(failure: AnalysisWorkFailure) -> Self {
        Self::Work(failure)
    }
}

impl fmt::Display for AnalysisCallInputFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotOriginalCall(call) => {
                write!(formatter, "not an original source call: {call:?}")
            }
            Self::UnknownArgument { call, index } => {
                write!(formatter, "unknown written argument {index} at {call:?}")
            }
            Self::Work(failure) => failure.fmt(formatter),
        }
    }
}

impl std::error::Error for AnalysisCallInputFailure {}

/// Attempt-local custody for one original call and its written argument slots.
///
/// The source adapter constructs this holder once, before candidate preparation,
/// and retains it across passes. This is not a call-site registry or an active
/// compiler budget. Header preparation is paid by selection; argument balances
/// are admitted lazily. Callee-derived results share selection, while contextual
/// expectations keep their existing tickets. No implicit allowance is supplied.
pub struct AnalysisCallInputs {
    site: ExpressionKey,
    selection: AnalysisRootTicket,
    contextual_result: Option<AnalysisRootTicket>,
    arguments: Vec<Option<AnalysisRootTicket>>,
}

impl AnalysisCallInputs {
    /// Admits an original HIR call before allocating its written-slot headers.
    ///
    /// Module-to-body association and once-only admission belong to the source
    /// adapter. Postfix-desugared calls and partial applications need their own
    /// input adapter; their generated arguments are not treated as written slots.
    pub fn admit(
        accounting: &mut AnalysisWorkAccounting,
        consumer: usize,
        site: ExpressionKey,
        body: &Body,
        selection_allowance_ticks: u64,
        contextual_result: Option<&AnalysisRootTicket>,
    ) -> Result<Self, AnalysisCallInputFailure> {
        Self::admit_with_work(
            accounting,
            consumer,
            site,
            body,
            selection_allowance_ticks,
            contextual_result,
            Ok(AnalysisWork {
                steps: 0,
                inspected_bytes: 0,
                transferred_bytes: 0,
                reserved_bytes: 0,
            }),
        )
    }

    fn admit_with_work(
        accounting: &mut AnalysisWorkAccounting,
        consumer: usize,
        site: ExpressionKey,
        body: &Body,
        selection_allowance_ticks: u64,
        contextual_result: Option<&AnalysisRootTicket>,
        additional_work: Result<AnalysisWork, AnalysisWorkOverflow>,
    ) -> Result<Self, AnalysisCallInputFailure> {
        let call = original_call(body, site)?;
        if let Some(ticket) = contextual_result {
            accounting.balance(ticket).map_err(|error| {
                AnalysisCallInputFailure::Work(AnalysisWorkFailure::Claim(error))
            })?;
        }

        let selection = accounting.admit_with_work(
            consumer,
            selection_allowance_ticks,
            additional_work
                .and_then(|work| Self::header_work(call.arguments.len())?.checked_add(work)),
        )?;
        let mut arguments = Vec::with_capacity(call.arguments.len());
        arguments.resize_with(call.arguments.len(), || {
            #[cfg(test)]
            tests::record_header_write();

            None
        });

        Ok(Self {
            site,
            selection,
            contextual_result: contextual_result.cloned(),
            arguments,
        })
    }

    #[must_use]
    pub const fn site(&self) -> ExpressionKey {
        self.site
    }

    #[must_use]
    pub fn written_argument_count(&self) -> usize {
        self.arguments.len()
    }

    #[must_use]
    pub const fn selection_ticket(&self) -> &AnalysisRootTicket {
        &self.selection
    }

    /// Derived callee output cannot obtain another allowance.
    #[must_use]
    pub const fn callee_result_ticket(&self) -> &AnalysisRootTicket {
        &self.selection
    }

    /// An upstream expectation, when present, is not a new call-result demand.
    #[must_use]
    pub const fn contextual_result_ticket(&self) -> Option<&AnalysisRootTicket> {
        self.contextual_result.as_ref()
    }

    /// Reuses a written argument's balance; the allowance is used only initially.
    pub fn argument_ticket(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        index: usize,
        allowance_ticks: u64,
    ) -> Result<&AnalysisRootTicket, AnalysisCallInputFailure> {
        self.argument_ticket_with_work(accounting, index, allowance_ticks, None)
    }

    fn argument_ticket_with_work(
        &mut self,
        accounting: &mut AnalysisWorkAccounting,
        index: usize,
        allowance_ticks: u64,
        additional_work: Option<AnalysisWork>,
    ) -> Result<&AnalysisRootTicket, AnalysisCallInputFailure> {
        let slot =
            self.arguments
                .get_mut(index)
                .ok_or(AnalysisCallInputFailure::UnknownArgument {
                    call: self.site,
                    index,
                })?;
        accounting
            .balance(&self.selection)
            .map_err(|error| AnalysisCallInputFailure::Work(AnalysisWorkFailure::Claim(error)))?;

        if let Some(ticket) = slot {
            let mut scope = accounting.claim(ticket)?;
            if let Some(work) = additional_work {
                scope.precharge(work)?;
            }
        } else {
            let ticket = accounting.admit_with_work(
                self.selection.identity().consumer(),
                allowance_ticks,
                additional_work.map_or(Ok(Self::slot_install_work()), |work| {
                    Self::slot_install_work().checked_add(work)
                }),
            )?;
            *slot = Some(ticket);

            #[cfg(test)]
            tests::record_slot_install();
        }
        Ok(slot
            .as_ref()
            .expect("successful slot admission retains a ticket"))
    }

    fn header_work(count: usize) -> Result<AnalysisWork, AnalysisWorkOverflow> {
        Layout::array::<Option<AnalysisRootTicket>>(count)
            .map_err(|_| AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::ReservedBytes))?;
        let count = u64::try_from(count)
            .map_err(|_| AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::ReservedBytes))?;
        let bytes = count
            .checked_mul(size_of::<Option<AnalysisRootTicket>>() as u64)
            .ok_or(AnalysisWorkOverflow::Quantity(
                AnalysisWorkCategory::ReservedBytes,
            ))?;
        let transferred_bytes =
            bytes
                .checked_add(size_of::<Self>() as u64)
                .ok_or(AnalysisWorkOverflow::Quantity(
                    AnalysisWorkCategory::TransferredBytes,
                ))?;

        Ok(AnalysisWork {
            steps: 1,
            inspected_bytes: 0,
            transferred_bytes,
            reserved_bytes: bytes,
        })
    }

    const fn slot_install_work() -> AnalysisWork {
        AnalysisWork {
            steps: 1,
            inspected_bytes: 0,
            transferred_bytes: size_of::<Option<AnalysisRootTicket>>() as u64,
            reserved_bytes: 0,
        }
    }
}

#[cfg(test)]
mod tests;
