use eglur_sema::{IntegerValue, PrimitiveType};

#[test]
fn exhaustive_eight_bit_wrapping_agrees_with_modular_arithmetic() {
    for (dtype, minimum, maximum) in [(PrimitiveType::I8, -128, 127), (PrimitiveType::U8, 0, 255)] {
        for left in minimum..=maximum {
            for right in minimum..=maximum {
                let a = IntegerValue::new(dtype, left).unwrap();
                let b = IntegerValue::new(dtype, right).unwrap();
                for (actual, mathematical) in [
                    (a.wrapping_add(b), left + right),
                    (a.wrapping_sub(b), left - right),
                    (a.wrapping_mul(b), left * right),
                ] {
                    let expected = (mathematical - minimum).rem_euclid(256) + minimum;
                    assert_eq!(actual, IntegerValue::new(dtype, expected));
                }
            }
        }
    }
}

#[test]
fn all_width_boundaries_and_full_unsigned_products_wrap() {
    for dtype in [
        PrimitiveType::I8,
        PrimitiveType::I16,
        PrimitiveType::I32,
        PrimitiveType::I64,
        PrimitiveType::U8,
        PrimitiveType::U16,
        PrimitiveType::U32,
        PrimitiveType::U64,
    ] {
        let (min, max) = dtype.integer_bounds().unwrap();
        let minimum = IntegerValue::new(dtype, min).unwrap();
        let maximum = IntegerValue::new(dtype, max).unwrap();
        let one = IntegerValue::new(dtype, 1).unwrap();
        let zero = IntegerValue::new(dtype, 0).unwrap();
        assert_eq!(maximum.wrapping_add(one), Some(minimum));
        assert_eq!(minimum.wrapping_sub(one), Some(maximum));
        assert_eq!(maximum.wrapping_mul(maximum), Some(one));
        assert_eq!(minimum.wrapping_mul(minimum), Some(zero));
        if min < 0 {
            let minus_one = IntegerValue::new(dtype, -1).unwrap();
            assert_eq!(minimum.wrapping_mul(minus_one), Some(minimum));
        }
    }
}

#[test]
fn domain_scalars_and_mixed_integer_types_are_rejected_at_the_value_boundary() {
    for (left, right) in [
        (PrimitiveType::Nat, PrimitiveType::Nat),
        (PrimitiveType::PositiveNat, PrimitiveType::PositiveNat),
        (PrimitiveType::I8, PrimitiveType::U8),
        (PrimitiveType::U16, PrimitiveType::I8),
    ] {
        let left = IntegerValue::new(left, 1).unwrap();
        let right = IntegerValue::new(right, 1).unwrap();
        assert_eq!(left.wrapping_add(right), None);
        assert_eq!(left.wrapping_sub(right), None);
        assert_eq!(left.wrapping_mul(right), None);
    }
}
