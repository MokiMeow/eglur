use std::alloc::{Layout, LayoutError};
use std::fmt;
use std::sync::Arc;

use crate::record_reservation::RecordReservation;

use super::{AnalysisWork, AnalysisWorkCategory, AnalysisWorkOverflow, AnalysisWorkPricing};

/// Attempt-local location of a balance, unrelated to semantic or source identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisRootIdentity {
    consumer: usize,
    requirement: usize,
}

impl AnalysisRootIdentity {
    #[must_use]
    pub const fn consumer(self) -> usize {
        self.consumer
    }

    #[must_use]
    pub const fn requirement(self) -> usize {
        self.requirement
    }
}

/// An owned handle to an existing balance; cloning it does not renew allowance.
#[derive(Clone, Debug)]
pub struct AnalysisRootTicket {
    attempt: Arc<()>,
    root: AnalysisRootIdentity,
}

impl AnalysisRootTicket {
    #[must_use]
    pub const fn identity(&self) -> AnalysisRootIdentity {
        self.root
    }
}

impl PartialEq for AnalysisRootTicket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.attempt, &other.attempt) && self.root == other.root
    }
}

impl Eq for AnalysisRootTicket {}

/// A read-only balance snapshot, including after an incomplete attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisRootBalance {
    allowance_ticks: u64,
    remaining_ticks: u64,
}

impl AnalysisRootBalance {
    #[must_use]
    pub const fn allowance_ticks(self) -> u64 {
        self.allowance_ticks
    }

    #[must_use]
    pub const fn remaining_ticks(self) -> u64 {
        self.remaining_ticks
    }

    #[must_use]
    pub const fn charged_ticks(self) -> u64 {
        self.allowance_ticks - self.remaining_ticks
    }
}

/// A tool-resource stop, not negative evidence about the source program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisRootStopReason {
    Exhausted {
        required_ticks: u64,
        remaining_ticks: u64,
    },
    Unrepresentable(AnalysisWorkOverflow),
}

/// The first root interruption retained by the accounting controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisWorkIncomplete {
    root: AnalysisRootIdentity,
    reason: AnalysisRootStopReason,
}

impl AnalysisWorkIncomplete {
    #[must_use]
    pub const fn root(self) -> AnalysisRootIdentity {
        self.root
    }

    #[must_use]
    pub const fn reason(self) -> AnalysisRootStopReason {
        self.reason
    }
}

/// Invalid accounting custody, distinct from source errors and tool exhaustion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisRootClaimError {
    ForeignAttempt,
    UnknownConsumer,
    UnknownRoot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisWorkFailure {
    Claim(AnalysisRootClaimError),
    Incomplete(AnalysisWorkIncomplete),
}

impl fmt::Display for AnalysisWorkFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claim(error) => write!(formatter, "invalid analysis root claim: {error:?}"),
            Self::Incomplete(incomplete) => write!(
                formatter,
                "analysis incomplete at consumer {}, requirement {}: {:?}",
                incomplete.root.consumer, incomplete.root.requirement, incomplete.reason,
            ),
        }
    }
}

impl std::error::Error for AnalysisWorkFailure {}

struct ConsumerRoots {
    roots: Vec<AnalysisRootBalance>,
}

/// Owns phase-stable balances over a fixed set of input consumers.
///
/// Consumer headers are input-bounded setup; root records are lazy and paid at
/// admission. The input adapter must admit each original demand once and reuse
/// its ticket on replay. Descendants receive only a claimed scope.
///
/// Maintenance is prepaid analytically over each consumer's append-only storage,
/// not as a per-root physical latency bound. Allocator overhead and OOM recovery
/// are outside this accounting contract. No implicit pricing or allowance exists.
pub struct AnalysisWorkAccounting {
    pricing: AnalysisWorkPricing,
    attempt: Arc<()>,
    consumers: Vec<ConsumerRoots>,
    incomplete: Option<AnalysisWorkIncomplete>,
    #[cfg(test)]
    observer: Option<fn(ChargeEvent)>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChargeKind {
    Admission,
    Work,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ChargeEvent {
    pub root: AnalysisRootIdentity,
    pub kind: ChargeKind,
    pub work: AnalysisWork,
    pub ticks: u64,
}

impl AnalysisWorkAccounting {
    /// Prepares empty consumer headers, rejecting an unrepresentable layout.
    pub fn new(pricing: AnalysisWorkPricing, consumer_count: usize) -> Result<Self, LayoutError> {
        Layout::array::<ConsumerRoots>(consumer_count)?;
        let mut consumers = Vec::with_capacity(consumer_count);
        consumers.resize_with(consumer_count, || ConsumerRoots { roots: Vec::new() });

        Ok(Self {
            pricing,
            attempt: Arc::new(()),
            consumers,
            incomplete: None,
            #[cfg(test)]
            observer: None,
        })
    }

    /// Admits one original demand only after paying its record-maintenance cost.
    pub fn admit(
        &mut self,
        consumer: usize,
        allowance_ticks: u64,
    ) -> Result<AnalysisRootTicket, AnalysisWorkFailure> {
        self.admit_with_work(
            consumer,
            allowance_ticks,
            Ok(AnalysisWork {
                steps: 0,
                inspected_bytes: 0,
                transferred_bytes: 0,
                reserved_bytes: 0,
            }),
        )
    }

    pub(super) fn admit_with_work(
        &mut self,
        consumer: usize,
        allowance_ticks: u64,
        additional_work: Result<AnalysisWork, AnalysisWorkOverflow>,
    ) -> Result<AnalysisRootTicket, AnalysisWorkFailure> {
        let roots = self
            .consumers
            .get(consumer)
            .ok_or(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::UnknownConsumer,
            ))?;
        let root = AnalysisRootIdentity {
            consumer,
            requirement: roots.roots.len(),
        };
        self.ensure_active()?;

        let reservation = RecordReservation::try_for_append(&roots.roots);
        let planned = additional_work
            .and_then(|work| {
                reservation
                    .as_ref()
                    .map(|_| work)
                    .ok_or(AnalysisWorkOverflow::Quantity(
                        AnalysisWorkCategory::ReservedBytes,
                    ))
            })
            .and_then(|additional| Self::admission_work()?.checked_add(additional));
        let price = planned.and_then(|work| self.pricing.price(work));
        #[cfg(test)]
        if let (Some(observer), Ok(work), Ok(cost)) = (self.observer, planned, price) {
            observer(ChargeEvent {
                root,
                kind: ChargeKind::Admission,
                work,
                ticks: cost.ticks(),
            });
        }
        let remaining_ticks = match price {
            Ok(cost) if cost.ticks() <= allowance_ticks => allowance_ticks - cost.ticks(),
            Ok(cost) => {
                return Err(self.stop(
                    root,
                    AnalysisRootStopReason::Exhausted {
                        required_ticks: cost.ticks(),
                        remaining_ticks: allowance_ticks,
                    },
                ));
            }
            Err(overflow) => {
                return Err(self.stop(root, AnalysisRootStopReason::Unrepresentable(overflow)));
            }
        };

        let roots = &mut self.consumers[consumer].roots;
        reservation
            .expect("admission checked the reservation")
            .reserve(roots);
        roots.push(AnalysisRootBalance {
            allowance_ticks,
            remaining_ticks,
        });

        Ok(AnalysisRootTicket {
            attempt: Arc::clone(&self.attempt),
            root,
        })
    }

    /// Reclaims the existing balance for a later phase, paying a custody step.
    pub fn claim(
        &mut self,
        ticket: &AnalysisRootTicket,
    ) -> Result<AnalysisRootScope<'_>, AnalysisWorkFailure> {
        self.validate_ticket(ticket)
            .map_err(AnalysisWorkFailure::Claim)?;
        self.precharge(ticket.root, Self::bookkeeping_work())?;

        Ok(AnalysisRootScope {
            accounting: self,
            root: ticket.root,
        })
    }

    /// Observes custody without authorizing analysis work or clearing a stop.
    pub fn balance(
        &self,
        ticket: &AnalysisRootTicket,
    ) -> Result<AnalysisRootBalance, AnalysisRootClaimError> {
        self.validate_ticket(ticket)?;
        Ok(self.consumers[ticket.root.consumer].roots[ticket.root.requirement])
    }

    #[must_use]
    pub const fn incomplete(&self) -> Option<AnalysisWorkIncomplete> {
        self.incomplete
    }

    pub(super) fn bind_input_consumer(
        &self,
        consumer: usize,
    ) -> Result<Arc<()>, AnalysisWorkFailure> {
        self.validate_input_binding(consumer, &self.attempt)?;
        Ok(Arc::clone(&self.attempt))
    }

    pub(super) fn validate_input_binding(
        &self,
        consumer: usize,
        attempt: &Arc<()>,
    ) -> Result<(), AnalysisWorkFailure> {
        if !Arc::ptr_eq(&self.attempt, attempt) {
            return Err(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::ForeignAttempt,
            ));
        }
        if consumer >= self.consumers.len() {
            return Err(AnalysisWorkFailure::Claim(
                AnalysisRootClaimError::UnknownConsumer,
            ));
        }
        self.ensure_active()
    }

    fn validate_ticket(&self, ticket: &AnalysisRootTicket) -> Result<(), AnalysisRootClaimError> {
        if !Arc::ptr_eq(&self.attempt, &ticket.attempt) {
            return Err(AnalysisRootClaimError::ForeignAttempt);
        }

        let consumer = self
            .consumers
            .get(ticket.root.consumer)
            .ok_or(AnalysisRootClaimError::UnknownConsumer)?;
        consumer
            .roots
            .get(ticket.root.requirement)
            .ok_or(AnalysisRootClaimError::UnknownRoot)?;
        Ok(())
    }

    fn ensure_active(&self) -> Result<(), AnalysisWorkFailure> {
        match self.incomplete {
            Some(incomplete) => Err(AnalysisWorkFailure::Incomplete(incomplete)),
            None => Ok(()),
        }
    }

    fn precharge(
        &mut self,
        root: AnalysisRootIdentity,
        work: AnalysisWork,
    ) -> Result<(), AnalysisWorkFailure> {
        self.ensure_active()?;
        let cost = match self.pricing.price(work) {
            Ok(cost) => cost.ticks(),
            Err(overflow) => {
                return Err(self.stop(root, AnalysisRootStopReason::Unrepresentable(overflow)));
            }
        };
        #[cfg(test)]
        if let Some(observer) = self.observer {
            observer(ChargeEvent {
                root,
                kind: ChargeKind::Work,
                work,
                ticks: cost,
            });
        }
        let remaining_ticks = self.consumers[root.consumer].roots[root.requirement].remaining_ticks;
        if cost > remaining_ticks {
            return Err(self.stop(
                root,
                AnalysisRootStopReason::Exhausted {
                    required_ticks: cost,
                    remaining_ticks,
                },
            ));
        }

        self.consumers[root.consumer].roots[root.requirement].remaining_ticks -= cost;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn observe_charges(&mut self, observer: fn(ChargeEvent)) {
        self.observer = Some(observer);
    }

    fn stop(
        &mut self,
        root: AnalysisRootIdentity,
        reason: AnalysisRootStopReason,
    ) -> AnalysisWorkFailure {
        let incomplete = *self
            .incomplete
            .get_or_insert(AnalysisWorkIncomplete { root, reason });
        AnalysisWorkFailure::Incomplete(incomplete)
    }

    const fn bookkeeping_work() -> AnalysisWork {
        AnalysisWork {
            steps: 1,
            inspected_bytes: 0,
            transferred_bytes: 0,
            reserved_bytes: 0,
        }
    }

    fn admission_work() -> Result<AnalysisWork, AnalysisWorkOverflow> {
        let header_bytes = u64::try_from(size_of::<AnalysisRootBalance>())
            .map_err(|_| AnalysisWorkOverflow::Quantity(AnalysisWorkCategory::ReservedBytes))?;
        let multiply = |factor, category| {
            header_bytes
                .checked_mul(factor)
                .ok_or(AnalysisWorkOverflow::Quantity(category))
        };

        // Requested backing <= 4N; relocation <= 2N plus N publication writes.
        Ok(AnalysisWork {
            steps: 1,
            inspected_bytes: 0,
            transferred_bytes: multiply(3, AnalysisWorkCategory::TransferredBytes)?,
            reserved_bytes: multiply(4, AnalysisWorkCategory::ReservedBytes)?,
        })
    }
}

/// Work authority for one inherited root, without root-admission authority.
///
/// ```compile_fail
/// use eglur_sema::AnalysisRootScope;
/// fn descendant(mut scope: AnalysisRootScope<'_>) {
///     scope.admit(0, 1000);
/// }
/// ```
pub struct AnalysisRootScope<'a> {
    accounting: &'a mut AnalysisWorkAccounting,
    root: AnalysisRootIdentity,
}

impl AnalysisRootScope<'_> {
    /// Pays before the caller performs work. Each call adds a bookkeeping step.
    /// Splitting an operation adds custody work, not price rounding.
    pub fn precharge(&mut self, work: AnalysisWork) -> Result<(), AnalysisWorkFailure> {
        self.accounting.ensure_active()?;
        match work.checked_add(AnalysisWorkAccounting::bookkeeping_work()) {
            Ok(work) => self.accounting.precharge(self.root, work),
            Err(overflow) => Err(self
                .accounting
                .stop(self.root, AnalysisRootStopReason::Unrepresentable(overflow))),
        }
    }

    /// Stops when a consumer cannot represent its planned work quantities.
    pub fn stop_unrepresentable(&mut self, overflow: AnalysisWorkOverflow) -> AnalysisWorkFailure {
        self.accounting
            .stop(self.root, AnalysisRootStopReason::Unrepresentable(overflow))
    }
}

#[cfg(test)]
mod tests;
