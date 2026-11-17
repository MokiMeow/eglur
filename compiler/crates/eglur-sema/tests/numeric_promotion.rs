use eglur_sema::{
    CheckedNumericOperator, IntegerEvaluation, IntegerValue, NumericFaultCause, PrimitiveType,
};

use PrimitiveType::{
    Bf16, Bool, Complex64, Complex128, F16, F32, F64, I8, I16, I32, I64, Nat, PositiveNat, Str, U8,
    U16, U32, U64, Unit,
};

const NUMERIC: [PrimitiveType; 15] = [
    Bool, U8, I8, U16, I16, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
];

#[test]
fn every_ordered_numeric_pair_follows_the_specification_table() {
    // Rows and columns use NUMERIC order; this table is independent of enum ordering.
    let expected = [
        [
            U8, U8, I8, U16, I16, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            U8, U8, I8, U16, I16, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            I8, I8, I8, U16, I16, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            U16, U16, U16, U16, I16, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            I16, I16, I16, I16, I16, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            U32, U32, U32, U32, U32, U32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            I32, I32, I32, I32, I32, I32, I32, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            U64, U64, U64, U64, U64, U64, U64, U64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            I64, I64, I64, I64, I64, I64, I64, I64, I64, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            F16, F16, F16, F16, F16, F16, F16, F16, F16, F16, Bf16, F32, F64, Complex64, Complex128,
        ],
        [
            Bf16, Bf16, Bf16, Bf16, Bf16, Bf16, Bf16, Bf16, Bf16, Bf16, Bf16, F32, F64, Complex64,
            Complex128,
        ],
        [
            F32, F32, F32, F32, F32, F32, F32, F32, F32, F32, F32, F32, F64, Complex64, Complex128,
        ],
        [
            F64, F64, F64, F64, F64, F64, F64, F64, F64, F64, F64, F64, F64, Complex64, Complex128,
        ],
        [
            Complex64, Complex64, Complex64, Complex64, Complex64, Complex64, Complex64, Complex64,
            Complex64, Complex64, Complex64, Complex64, Complex64, Complex64, Complex128,
        ],
        [Complex128; 15],
    ];
    for (row, left) in NUMERIC.into_iter().enumerate() {
        for (column, right) in NUMERIC.into_iter().enumerate() {
            assert_eq!(
                left.promote_numeric(right),
                Some(expected[row][column]),
                "{left:?}, {right:?}"
            );
            assert_eq!(left.promote_numeric(right), right.promote_numeric(left));
        }
    }
}

#[test]
fn nonnumeric_and_domain_scalars_never_participate() {
    let excluded = [Unit, Str, Nat, PositiveNat];
    for left in excluded {
        for right in NUMERIC.into_iter().chain(excluded) {
            assert_eq!(left.promote_numeric(right), None);
            assert_eq!(right.promote_numeric(left), None);
        }
    }
}

#[test]
fn selected_integer_dtype_converts_before_arithmetic_for_every_pair() {
    for left in [U8, I8, U16, I16, U32, I32, U64, I64] {
        for right in [U8, I8, U16, I16, U32, I32, U64, I64] {
            let destination = left.promote_numeric(right).unwrap();
            let (minimum, maximum) = left.integer_bounds().unwrap();
            let (target_minimum, target_maximum) = destination.integer_bounds().unwrap();
            for value in [minimum, 0, maximum] {
                let value = IntegerValue::new(left, value).unwrap();
                let zero = IntegerValue::new(right, 0).unwrap();
                let fits = (target_minimum..=target_maximum).contains(&value.value());
                let expected = if fits {
                    IntegerEvaluation::Value(IntegerValue::new(destination, 0).unwrap())
                } else {
                    IntegerEvaluation::Fault(NumericFaultCause::Overflow)
                };
                // Multiplication by zero cannot erase an unrepresentable operand.
                assert_eq!(
                    value.checked(CheckedNumericOperator::Multiply, destination, Some(zero)),
                    expected
                );
                assert_eq!(
                    zero.checked(CheckedNumericOperator::Multiply, destination, Some(value)),
                    expected
                );
                let fault = if fits {
                    NumericFaultCause::DivisionByZero
                } else {
                    NumericFaultCause::Overflow
                };
                assert_eq!(
                    value.checked(CheckedNumericOperator::Divide, destination, Some(zero)),
                    IntegerEvaluation::Fault(fault)
                );
            }
        }
    }
}
