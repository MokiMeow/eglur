use std::panic::{AssertUnwindSafe, catch_unwind};

use super::observation::{assert_append, assert_growth_envelope, observe};
use super::*;

#[test]
fn every_append_uses_prepared_capacity_and_preserves_the_prefix() {
    let mut records = Vec::new();
    let mut reservations = Vec::new();
    for value in 0..4097 {
        let (_, observed) = observe(|| {
            RecordReservation::for_append(&records).reserve(&mut records);
            records.push(value);
        });
        assert_eq!(observed.len(), 1);
        assert_append(&observed[0], records.len(), records.capacity());
        reservations.extend(observed);
        assert_growth_envelope(&reservations, records.len() * size_of::<usize>());
    }
    assert!(records.iter().copied().eq(0..records.len()));
}

#[test]
fn plans_use_reported_overcapacity_without_allocating() {
    for capacity in [1, 3, 17, 1024, 1_000_000] {
        let spare = RecordReservation::<u8>::from_dimensions(capacity - 1, capacity);
        assert_eq!(spare.requested_capacity, None);

        let full = RecordReservation::<u8>::from_dimensions(capacity, capacity);
        assert_eq!(full.requested_capacity, Some(2 * capacity));
    }
}

#[test]
fn checked_length_doubling_and_layout_limits_fail_without_allocating() {
    assert!(catch_unwind(|| RecordReservation::<u8>::from_dimensions(2, 1)).is_err());
    assert!(
        catch_unwind(|| RecordReservation::<()>::from_dimensions(usize::MAX, usize::MAX)).is_err()
    );
    let capacity = usize::MAX / 2 + 1;
    assert!(catch_unwind(|| RecordReservation::<u8>::from_dimensions(capacity, capacity)).is_err());
    let capacity = isize::MAX as usize / size_of::<u64>();
    assert!(
        catch_unwind(|| RecordReservation::<u64>::from_dimensions(capacity, capacity)).is_err()
    );
}

#[test]
fn fallible_plans_distinguish_nonrepresentability_without_panicking() {
    assert!(RecordReservation::<()>::try_from_dimensions(usize::MAX, usize::MAX).is_none());
    let capacity = usize::MAX / 2 + 1;
    assert!(RecordReservation::<u8>::try_from_dimensions(capacity, capacity).is_none());
    let capacity = isize::MAX as usize / size_of::<u64>();
    assert!(RecordReservation::<u64>::try_from_dimensions(capacity, capacity).is_none());

    for capacity in [0, 1, 3, 17, 1024] {
        for len in 0..=capacity {
            let checked = RecordReservation::<u64>::try_from_dimensions(len, capacity).unwrap();
            let original = RecordReservation::<u64>::from_dimensions(len, capacity);
            assert_eq!(checked.requested_capacity, original.requested_capacity);
        }
    }
}

#[test]
fn all_member_plans_can_be_checked_before_any_reservation() {
    let mut records = vec![1_u64].into_boxed_slice().into_vec();
    let before = (records.clone(), records.capacity());
    let capacity = isize::MAX as usize / size_of::<u64>();

    let result = catch_unwind(AssertUnwindSafe(|| {
        let first = RecordReservation::for_append(&records);
        let _second = RecordReservation::<u64>::from_dimensions(capacity, capacity);
        first.reserve(&mut records);
    }));

    assert!(result.is_err());
    assert_eq!((records.clone(), records.capacity()), before);
}

#[test]
fn stale_plans_refuse_before_reservation() {
    let mut records = Vec::with_capacity(4);
    let length_plan = RecordReservation::for_append(&records);
    records.push(1_u64);
    let before = (records.clone(), records.capacity());
    assert!(catch_unwind(AssertUnwindSafe(|| length_plan.reserve(&mut records))).is_err());
    assert_eq!((records.clone(), records.capacity()), before);

    let capacity_plan = RecordReservation::for_append(&records);
    records.reserve_exact(records.capacity() + 1);
    let before = (records.clone(), records.capacity());
    assert!(catch_unwind(AssertUnwindSafe(|| capacity_plan.reserve(&mut records))).is_err());
    assert_eq!((records.clone(), records.capacity()), before);
}

#[test]
fn zero_sized_records_do_not_reserve_and_observation_unwind_is_isolated() {
    let mut records = Vec::<()>::new();
    let (_, observations) = observe(|| {
        RecordReservation::for_append(&records).reserve(&mut records);
        records.push(());
    });
    assert_append(&observations[0], records.len(), records.capacity());
    assert_eq!(observations[0].requested_capacity, None);

    assert!(catch_unwind(|| observe(|| panic!("observation interrupted"))).is_err());
    let (_, observations) = observe(|| {
        RecordReservation::for_append(&records).reserve(&mut records);
    });
    assert_eq!(observations.len(), 1);
}
