use std::alloc::Layout;
use std::marker::PhantomData;

#[cfg(test)]
pub(crate) mod observation;

#[cfg(test)]
mod tests;

/// Checked capacity preparation; record identity and publication stay with the caller.
/// From empty append-only storage, requested backing and possible prefix relocation
/// total at most 4N and 2N slots. Allocator overhead and record payloads are excluded.
pub(crate) struct RecordReservation<T> {
    len: usize,
    capacity: usize,
    requested_capacity: Option<usize>,
    record: PhantomData<fn() -> T>,
}

impl<T> RecordReservation<T> {
    pub(crate) fn for_append(records: &Vec<T>) -> Self {
        Self::from_dimensions(records.len(), records.capacity())
    }

    pub(crate) fn try_for_append(records: &Vec<T>) -> Option<Self> {
        Self::try_from_dimensions(records.len(), records.capacity())
    }

    fn from_dimensions(len: usize, capacity: usize) -> Self {
        Self::try_from_dimensions(len, capacity).expect("unrepresentable record reservation")
    }

    fn try_from_dimensions(len: usize, capacity: usize) -> Option<Self> {
        assert!(len <= capacity, "record length exceeds capacity");
        len.checked_add(1)?;

        let requested_capacity = if len < capacity {
            None
        } else {
            let requested = if capacity == 0 {
                1
            } else {
                capacity.checked_mul(2)?
            };
            Layout::array::<T>(requested).ok()?;
            Some(requested)
        };

        Some(Self {
            len,
            capacity,
            requested_capacity,
            record: PhantomData,
        })
    }

    pub(crate) fn reserve(self, records: &mut Vec<T>) {
        assert_eq!(records.len(), self.len, "record reservation length changed");
        assert_eq!(
            records.capacity(),
            self.capacity,
            "record reservation capacity changed"
        );

        if let Some(requested) = self.requested_capacity {
            // Double the reported capacity, not an assumed allocator growth factor.
            records.reserve_exact(requested - self.len);
        }

        #[cfg(test)]
        observation::record::<T>(
            self.len,
            self.capacity,
            self.requested_capacity,
            records.capacity(),
        );
    }
}
