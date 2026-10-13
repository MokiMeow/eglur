use std::fmt;
use std::num::NonZeroU64;

mod accounting;

pub use accounting::{
    AnalysisRootBalance, AnalysisRootClaimError, AnalysisRootIdentity, AnalysisRootScope,
    AnalysisRootStopReason, AnalysisRootTicket, AnalysisWorkAccounting, AnalysisWorkFailure,
    AnalysisWorkIncomplete,
};

/// Unpriced quantities for a planned analysis operation.
///
/// Supplying complete counts remains the operation owner's responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisWork {
    pub steps: u64,
    pub inspected_bytes: u64,
    pub transferred_bytes: u64,
    pub reserved_bytes: u64,
}

impl AnalysisWork {
    /// Combines quantities without dropping a category or wrapping its count.
    pub fn checked_add(self, other: Self) -> Result<Self, AnalysisWorkOverflow> {
        let add = |left: u64, right: u64, category| {
            left.checked_add(right)
                .ok_or(AnalysisWorkOverflow::Quantity(category))
        };

        Ok(Self {
            steps: add(self.steps, other.steps, AnalysisWorkCategory::Steps)?,
            inspected_bytes: add(
                self.inspected_bytes,
                other.inspected_bytes,
                AnalysisWorkCategory::InspectedBytes,
            )?,
            transferred_bytes: add(
                self.transferred_bytes,
                other.transferred_bytes,
                AnalysisWorkCategory::TransferredBytes,
            )?,
            reserved_bytes: add(
                self.reserved_bytes,
                other.reserved_bytes,
                AnalysisWorkCategory::ReservedBytes,
            )?,
        })
    }
}

/// One independently counted dimension of analysis work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisWorkCategory {
    Steps,
    InspectedBytes,
    TransferredBytes,
    ReservedBytes,
}

/// An estimate that cannot be represented, not a source-program arithmetic fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnalysisWorkOverflow {
    Quantity(AnalysisWorkCategory),
    Price,
}

impl fmt::Display for AnalysisWorkOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quantity(category) => {
                write!(formatter, "analysis work quantity overflow: {category:?}")
            }
            Self::Price => formatter.write_str("analysis work price overflow"),
        }
    }
}

impl std::error::Error for AnalysisWorkOverflow {}

/// A cost in integer ticks of the pricing profile that produced it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisWorkUnits(u64);

impl AnalysisWorkUnits {
    #[must_use]
    pub const fn ticks(self) -> u64 {
        self.0
    }
}

/// Positive linear prices; inspection and transfer share a byte coefficient.
///
/// Ticks retain fractional byte-to-step prices without rounding each request.
/// Profile construction belongs to accounting, not callers supplying quantities.
/// There is no implicit production profile:
///
/// ```compile_fail
/// use eglur_sema::AnalysisWorkPricing;
/// let profile: AnalysisWorkPricing = Default::default();
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnalysisWorkPricing {
    ticks_per_step: NonZeroU64,
    ticks_per_processed_byte: NonZeroU64,
    ticks_per_reserved_byte: NonZeroU64,
}

impl AnalysisWorkPricing {
    /// Computes the precharge for the declared quantities.
    pub fn price(self, work: AnalysisWork) -> Result<AnalysisWorkUnits, AnalysisWorkOverflow> {
        let quantities = [
            (work.steps, self.ticks_per_step),
            (work.inspected_bytes, self.ticks_per_processed_byte),
            (work.transferred_bytes, self.ticks_per_processed_byte),
            (work.reserved_bytes, self.ticks_per_reserved_byte),
        ];
        let mut total = 0_u64;

        for (quantity, coefficient) in quantities {
            let priced = quantity
                .checked_mul(coefficient.get())
                .ok_or(AnalysisWorkOverflow::Price)?;
            total = total
                .checked_add(priced)
                .ok_or(AnalysisWorkOverflow::Price)?;
        }

        Ok(AnalysisWorkUnits(total))
    }
}

#[cfg(test)]
mod tests;
