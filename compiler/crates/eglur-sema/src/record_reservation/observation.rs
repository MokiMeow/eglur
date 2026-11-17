use std::cell::RefCell;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReservationObservation {
    pub(crate) len: usize,
    pub(crate) before_capacity: usize,
    pub(crate) requested_capacity: Option<usize>,
    pub(crate) after_capacity: usize,
    pub(crate) record_bytes: usize,
}

thread_local! {
    static OBSERVATIONS: RefCell<Option<Vec<ReservationObservation>>> = const { RefCell::new(None) };
}

pub(super) fn record<T>(
    len: usize,
    before_capacity: usize,
    requested_capacity: Option<usize>,
    after_capacity: usize,
) {
    OBSERVATIONS.with_borrow_mut(|observations| {
        if let Some(observations) = observations {
            observations.push(ReservationObservation {
                len,
                before_capacity,
                requested_capacity,
                after_capacity,
                record_bytes: size_of::<T>(),
            });
        }
    });
}

struct ObservationScope;

impl Drop for ObservationScope {
    fn drop(&mut self) {
        OBSERVATIONS.with_borrow_mut(|observations| *observations = None);
    }
}

pub(crate) fn observe<R>(operation: impl FnOnce() -> R) -> (R, Vec<ReservationObservation>) {
    OBSERVATIONS.with_borrow_mut(|observations| {
        assert!(observations.is_none(), "record observation already active");
        *observations = Some(Vec::new());
    });
    let scope = ObservationScope;
    let result = operation();
    let observations = OBSERVATIONS.with_borrow_mut(|observations| observations.take().unwrap());
    drop(scope);
    (result, observations)
}

pub(crate) fn assert_append(observation: &ReservationObservation, len: usize, capacity: usize) {
    assert_eq!(observation.len + 1, len);
    assert_eq!(observation.after_capacity, capacity);
    assert!(observation.after_capacity > observation.len);

    if observation.len == observation.before_capacity {
        let requested = if observation.before_capacity == 0 {
            1
        } else {
            observation.before_capacity * 2
        };
        assert_eq!(observation.requested_capacity, Some(requested));
        assert!(observation.after_capacity >= requested);
    } else {
        assert_eq!(observation.requested_capacity, None);
        assert_eq!(observation.after_capacity, observation.before_capacity);
    }
}

pub(crate) fn assert_growth_envelope(
    observations: &[ReservationObservation],
    published_header_bytes: usize,
) {
    let mut requested_bytes = 0;
    let mut possible_relocation_bytes = 0;
    for observation in observations {
        if let Some(requested) = observation.requested_capacity {
            requested_bytes += requested * observation.record_bytes;
            possible_relocation_bytes += observation.len * observation.record_bytes;
        }
    }
    assert!(requested_bytes <= 4 * published_header_bytes);
    assert!(possible_relocation_bytes <= 2 * published_header_bytes);
}
