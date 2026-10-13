use std::num::NonZeroU64;

use super::{
    AnalysisWork, AnalysisWorkCategory, AnalysisWorkOverflow, AnalysisWorkPricing,
    AnalysisWorkUnits,
};

const ZERO: AnalysisWork = AnalysisWork {
    steps: 0,
    inspected_bytes: 0,
    transferred_bytes: 0,
    reserved_bytes: 0,
};

fn fixture_profile(step: u64, processed: u64, reserved: u64) -> AnalysisWorkPricing {
    AnalysisWorkPricing {
        ticks_per_step: NonZeroU64::new(step).unwrap(),
        ticks_per_processed_byte: NonZeroU64::new(processed).unwrap(),
        ticks_per_reserved_byte: NonZeroU64::new(reserved).unwrap(),
    }
}

fn work(values: [u64; 4]) -> AnalysisWork {
    AnalysisWork {
        steps: values[0],
        inspected_bytes: values[1],
        transferred_bytes: values[2],
        reserved_bytes: values[3],
    }
}

fn wide_price(profile: AnalysisWorkPricing, work: AnalysisWork) -> Option<u64> {
    let coefficients = [
        profile.ticks_per_step.get(),
        profile.ticks_per_processed_byte.get(),
        profile.ticks_per_processed_byte.get(),
        profile.ticks_per_reserved_byte.get(),
    ];
    let quantities = [
        work.steps,
        work.inspected_bytes,
        work.transferred_bytes,
        work.reserved_bytes,
    ];

    let exact = coefficients.into_iter().zip(quantities).try_fold(
        0_u128,
        |total, (coefficient, quantity)| {
            total.checked_add(u128::from(coefficient) * u128::from(quantity))
        },
    )?;
    u64::try_from(exact).ok()
}

fn next(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[test]
fn every_category_has_its_own_quantity_and_price_contribution() {
    let profile = fixture_profile(16, 1, 3);
    assert_eq!(profile.price(ZERO).unwrap().ticks(), 0);

    for (index, price) in [16, 1, 1, 3].into_iter().enumerate() {
        let mut values = [0; 4];
        values[index] = 1;
        assert_eq!(profile.price(work(values)).unwrap().ticks(), price);
    }

    assert_eq!(profile.price(work([2, 3, 5, 7])).unwrap().ticks(), 61);
}

#[test]
fn fractional_byte_prices_are_preserved_across_request_partitioning() {
    let profile = fixture_profile(16, 1, 4);
    let byte = AnalysisWork {
        steps: 0,
        inspected_bytes: 1,
        transferred_bytes: 0,
        reserved_bytes: 0,
    };
    let step = AnalysisWork {
        steps: 1,
        inspected_bytes: 0,
        transferred_bytes: 0,
        reserved_bytes: 0,
    };
    let mut combined = ZERO;
    let mut partitioned_ticks = 0;

    for _ in 0..16 {
        combined = combined.checked_add(byte).unwrap();
        partitioned_ticks += profile.price(byte).unwrap().ticks();
    }

    assert_eq!(partitioned_ticks, 16);
    assert_eq!(profile.price(combined).unwrap().ticks(), partitioned_ticks);
    assert_eq!(profile.price(combined), profile.price(step));
}

#[test]
fn inspection_and_transfer_are_additive_not_interchangeable_counts() {
    let profile = fixture_profile(32, 2, 7);
    let inspected = work([0, 9, 0, 0]);
    let transferred = work([0, 0, 9, 0]);
    assert_eq!(profile.price(inspected), profile.price(transferred));
    assert_ne!(inspected, transferred);
    assert_eq!(
        profile
            .price(inspected.checked_add(transferred).unwrap())
            .unwrap()
            .ticks(),
        36
    );
}

#[test]
fn category_composition_is_checked_at_the_unsigned_boundary() {
    for (index, category) in [
        AnalysisWorkCategory::Steps,
        AnalysisWorkCategory::InspectedBytes,
        AnalysisWorkCategory::TransferredBytes,
        AnalysisWorkCategory::ReservedBytes,
    ]
    .into_iter()
    .enumerate()
    {
        let mut maximum = [0; 4];
        maximum[index] = u64::MAX;
        let mut one = [0; 4];
        one[index] = 1;
        assert_eq!(work(maximum).checked_add(ZERO), Ok(work(maximum)));
        assert_eq!(
            work(maximum).checked_add(work(one)),
            Err(AnalysisWorkOverflow::Quantity(category))
        );
        assert_eq!(
            work(one).checked_add(work(maximum)),
            Err(AnalysisWorkOverflow::Quantity(category))
        );
    }
}

#[test]
fn every_price_product_is_checked() {
    let profile = fixture_profile(2, 2, 2);
    for index in 0..4 {
        let mut values = [0; 4];
        values[index] = u64::MAX / 2;
        assert_eq!(profile.price(work(values)).unwrap().ticks(), u64::MAX - 1);
        values[index] += 1;
        assert_eq!(
            profile.price(work(values)),
            Err(AnalysisWorkOverflow::Price)
        );
    }
}

#[test]
fn representable_products_cannot_wrap_the_total_price() {
    let profile = fixture_profile(1, 1, 1);
    assert_eq!(
        profile.price(work([u64::MAX, 0, 0, 0])).unwrap().ticks(),
        u64::MAX
    );
    for index in 1..4 {
        let mut values = [u64::MAX, 0, 0, 0];
        values[index] = 1;
        assert_eq!(
            profile.price(work(values)),
            Err(AnalysisWorkOverflow::Price)
        );
    }
}

#[test]
fn generated_composition_is_commutative_associative_and_linear() {
    let profiles = [
        fixture_profile(1, 1, 1),
        fixture_profile(16, 1, 4),
        fixture_profile(32, 3, 11),
        fixture_profile(17, 23, 5),
    ];
    let mut state = 0x12bd_83fa_4451_210b;

    for _ in 0..8192 {
        let a = work(std::array::from_fn(|_| next(&mut state) % 1_000_000));
        let b = work(std::array::from_fn(|_| next(&mut state) % 1_000_000));
        let c = work(std::array::from_fn(|_| next(&mut state) % 1_000_000));
        let ab = a.checked_add(b).unwrap();
        let abc = ab.checked_add(c).unwrap();
        assert_eq!(a.checked_add(ZERO), Ok(a));
        assert_eq!(ZERO.checked_add(a), Ok(a));
        assert_eq!(b.checked_add(a), Ok(ab));
        assert_eq!(a.checked_add(b.checked_add(c).unwrap()), Ok(abc));

        for profile in profiles {
            assert_eq!(
                profile.price(abc).unwrap().ticks(),
                profile.price(a).unwrap().ticks()
                    + profile.price(b).unwrap().ticks()
                    + profile.price(c).unwrap().ticks()
            );
            assert_eq!(
                profile.price(abc).unwrap().ticks(),
                wide_price(profile, abc).unwrap()
            );
        }
    }
}

#[test]
fn generated_boundary_prices_match_the_wide_integer_oracle() {
    let mut state = 0x992e_604d_436d_0271;
    let profiles = [
        fixture_profile(1, 1, 1),
        fixture_profile(16, 1, 4),
        fixture_profile(u64::MAX, 1, 1),
        fixture_profile(1, u64::MAX, 1),
        fixture_profile(1, 1, u64::MAX),
        fixture_profile(u64::MAX, u64::MAX, u64::MAX),
    ];

    for _ in 0..4096 {
        let values = std::array::from_fn(|_| match next(&mut state) % 8 {
            0 => 0,
            1 => 1,
            2 => u64::MAX,
            3 => u64::MAX / 16,
            4 => u64::MAX / 16 + 1,
            5 => next(&mut state) % 1024,
            _ => next(&mut state),
        });

        for profile in profiles {
            assert_eq!(
                profile
                    .price(work(values))
                    .map(AnalysisWorkUnits::ticks)
                    .ok(),
                wide_price(profile, work(values))
            );
        }
    }
}

#[test]
fn estimates_can_compose_even_when_their_price_is_unrepresentable() {
    let profile = fixture_profile(16, 1, 4);
    let a = work([u64::MAX / 16, 0, 0, 0]);
    let b = work([1, 0, 0, 0]);
    assert!(profile.price(a).is_ok());
    assert!(profile.price(b).is_ok());
    assert_eq!(
        profile.price(a.checked_add(b).unwrap()),
        Err(AnalysisWorkOverflow::Price)
    );
}

#[test]
fn splitting_work_does_not_make_an_unrepresentable_total_representable() {
    let profile = fixture_profile(16, 1, 4);
    let a = work([u64::MAX / 16, 0, 0, 0]);
    let b = work([1, 0, 0, 0]);
    let left = profile.price(a).unwrap().ticks();
    let right = profile.price(b).unwrap().ticks();
    assert_eq!(left.checked_add(right), None);
    assert_eq!(
        profile.price(a.checked_add(b).unwrap()),
        Err(AnalysisWorkOverflow::Price)
    );
}
