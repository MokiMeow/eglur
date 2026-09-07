use crate::{CheckedNumericOperator, NumericFaultCause, PrimitiveType};

/// Exact scalar integer, validated against its fixed-width or domain-scalar range.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntegerValue {
    primitive: PrimitiveType,
    value: i128,
}

impl IntegerValue {
    #[must_use]
    pub fn new(primitive: PrimitiveType, value: i128) -> Option<Self> {
        let (minimum, maximum) = primitive.integer_bounds()?;
        (minimum <= value && value <= maximum).then_some(Self { primitive, value })
    }

    #[must_use]
    pub const fn primitive(self) -> PrimitiveType {
        self.primitive
    }

    #[must_use]
    pub const fn value(self) -> i128 {
        self.value
    }

    /// Evaluates after the type checker has selected the destination dtype.
    #[must_use]
    pub fn checked(
        self,
        operator: CheckedNumericOperator,
        destination: PrimitiveType,
        right: Option<Self>,
    ) -> IntegerEvaluation {
        if !destination.is_fixed_width_integer()
            || !self.primitive.is_fixed_width_integer()
            || right.is_some_and(|value| !value.primitive.is_fixed_width_integer())
            || (operator == CheckedNumericOperator::Negate) != right.is_none()
            || (operator == CheckedNumericOperator::Negate && self.primitive != destination)
        {
            return IntegerEvaluation::Invalid;
        }
        if Self::new(destination, self.value).is_none()
            || right.is_some_and(|value| Self::new(destination, value.value).is_none())
        {
            return IntegerEvaluation::Fault(NumericFaultCause::Overflow);
        }
        let value = match (operator, right) {
            (CheckedNumericOperator::Add, Some(right)) => self.value.checked_add(right.value),
            (CheckedNumericOperator::Subtract, Some(right)) => self.value.checked_sub(right.value),
            (CheckedNumericOperator::Multiply, Some(right)) => self.value.checked_mul(right.value),
            (CheckedNumericOperator::Divide, Some(right)) => {
                if right.value == 0 {
                    return IntegerEvaluation::Fault(NumericFaultCause::DivisionByZero);
                }
                self.value.checked_div(right.value)
            }
            (CheckedNumericOperator::Remainder, Some(right)) => {
                if right.value == 0 {
                    return IntegerEvaluation::Fault(NumericFaultCause::RemainderByZero);
                }
                self.value.checked_rem(right.value)
            }
            (CheckedNumericOperator::Negate, None) => self.value.checked_neg(),
            _ => return IntegerEvaluation::Invalid,
        };
        match value.and_then(|value| Self::new(destination, value)) {
            Some(value) => IntegerEvaluation::Value(value),
            None => IntegerEvaluation::Fault(NumericFaultCause::Overflow),
        }
    }

    #[must_use]
    pub fn wrapping_add(self, right: Self) -> Option<Self> {
        if self.primitive != right.primitive {
            return None;
        }
        let value = match self.primitive {
            PrimitiveType::I8 => i128::from((self.value as i8).wrapping_add(right.value as i8)),
            PrimitiveType::I16 => i128::from((self.value as i16).wrapping_add(right.value as i16)),
            PrimitiveType::I32 => i128::from((self.value as i32).wrapping_add(right.value as i32)),
            PrimitiveType::I64 => i128::from((self.value as i64).wrapping_add(right.value as i64)),
            PrimitiveType::U8 => i128::from((self.value as u8).wrapping_add(right.value as u8)),
            PrimitiveType::U16 => i128::from((self.value as u16).wrapping_add(right.value as u16)),
            PrimitiveType::U32 => i128::from((self.value as u32).wrapping_add(right.value as u32)),
            PrimitiveType::U64 => i128::from((self.value as u64).wrapping_add(right.value as u64)),
            _ => return None,
        };
        Self::new(self.primitive, value)
    }
}

/// Arithmetic failure is distinct from an invalid compiler-supplied operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntegerEvaluation {
    Value(IntegerValue),
    Fault(NumericFaultCause),
    Invalid,
}

impl PrimitiveType {
    #[must_use]
    pub const fn is_fixed_width_integer(self) -> bool {
        matches!(
            self,
            Self::I8
                | Self::I16
                | Self::I32
                | Self::I64
                | Self::U8
                | Self::U16
                | Self::U32
                | Self::U64
        )
    }

    #[must_use]
    pub const fn integer_bounds(self) -> Option<(i128, i128)> {
        Some(match self {
            Self::I8 => (i8::MIN as i128, i8::MAX as i128),
            Self::I16 => (i16::MIN as i128, i16::MAX as i128),
            Self::I32 => (i32::MIN as i128, i32::MAX as i128),
            Self::I64 => (i64::MIN as i128, i64::MAX as i128),
            Self::U8 => (0, u8::MAX as i128),
            Self::U16 => (0, u16::MAX as i128),
            Self::U32 => (0, u32::MAX as i128),
            Self::U64 => (0, u64::MAX as i128),
            Self::Nat => (0, i64::MAX as i128),
            Self::PositiveNat => (1, i64::MAX as i128),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integer(dtype: PrimitiveType, value: i128) -> IntegerValue {
        IntegerValue::new(dtype, value).unwrap()
    }

    #[test]
    fn construction_enforces_fixed_width_and_domain_scalar_bounds() {
        for dtype in [
            PrimitiveType::I8,
            PrimitiveType::I16,
            PrimitiveType::I32,
            PrimitiveType::I64,
            PrimitiveType::U8,
            PrimitiveType::U16,
            PrimitiveType::U32,
            PrimitiveType::U64,
            PrimitiveType::Nat,
            PrimitiveType::PositiveNat,
        ] {
            let (min, max) = dtype.integer_bounds().unwrap();
            assert_eq!(integer(dtype, min).value(), min);
            assert_eq!(integer(dtype, max).value(), max);
            assert!(IntegerValue::new(dtype, min - 1).is_none());
            assert!(IntegerValue::new(dtype, max + 1).is_none());
        }
        for dtype in [
            PrimitiveType::Unit,
            PrimitiveType::Bool,
            PrimitiveType::Str,
            PrimitiveType::F16,
            PrimitiveType::Bf16,
            PrimitiveType::F32,
            PrimitiveType::F64,
            PrimitiveType::Complex64,
            PrimitiveType::Complex128,
        ] {
            assert!(IntegerValue::new(dtype, 0).is_none());
        }
    }

    #[test]
    fn all_eight_bit_binary_inputs_match_mathematical_integer_rules() {
        for (dtype, minimum, maximum) in
            [(PrimitiveType::I8, -128, 127), (PrimitiveType::U8, 0, 255)]
        {
            for lhs in minimum..=maximum {
                for rhs in minimum..=maximum {
                    for operator in [
                        CheckedNumericOperator::Add,
                        CheckedNumericOperator::Subtract,
                        CheckedNumericOperator::Multiply,
                        CheckedNumericOperator::Divide,
                        CheckedNumericOperator::Remainder,
                    ] {
                        let expected = match operator {
                            CheckedNumericOperator::Divide if rhs == 0 => {
                                IntegerEvaluation::Fault(NumericFaultCause::DivisionByZero)
                            }
                            CheckedNumericOperator::Remainder if rhs == 0 => {
                                IntegerEvaluation::Fault(NumericFaultCause::RemainderByZero)
                            }
                            _ => {
                                let value = match operator {
                                    CheckedNumericOperator::Add => lhs + rhs,
                                    CheckedNumericOperator::Subtract => lhs - rhs,
                                    CheckedNumericOperator::Multiply => lhs * rhs,
                                    CheckedNumericOperator::Divide => lhs / rhs,
                                    CheckedNumericOperator::Remainder => lhs % rhs,
                                    CheckedNumericOperator::Negate => unreachable!(),
                                };
                                if (minimum..=maximum).contains(&value) {
                                    IntegerEvaluation::Value(integer(dtype, value))
                                } else {
                                    IntegerEvaluation::Fault(NumericFaultCause::Overflow)
                                }
                            }
                        };
                        assert_eq!(
                            integer(dtype, lhs).checked(operator, dtype, Some(integer(dtype, rhs))),
                            expected,
                            "{dtype:?} {lhs} {operator:?} {rhs}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn all_width_boundaries_and_wrapping_follow_the_destination() {
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
            let zero = integer(dtype, 0);
            let one = integer(dtype, 1);
            let minimum = integer(dtype, min);
            let maximum = integer(dtype, max);
            assert_eq!(
                maximum.checked(CheckedNumericOperator::Add, dtype, Some(one)),
                IntegerEvaluation::Fault(NumericFaultCause::Overflow)
            );
            assert_eq!(maximum.wrapping_add(one), Some(minimum));
            assert_eq!(
                zero.checked(CheckedNumericOperator::Negate, dtype, None),
                IntegerEvaluation::Value(zero)
            );
            let overflowing_negation = if min == 0 { one } else { minimum };
            assert_eq!(
                overflowing_negation.checked(CheckedNumericOperator::Negate, dtype, None),
                IntegerEvaluation::Fault(NumericFaultCause::Overflow)
            );
            if min < 0 {
                let minus_one = integer(dtype, -1);
                assert_eq!(
                    minimum.checked(CheckedNumericOperator::Divide, dtype, Some(minus_one)),
                    IntegerEvaluation::Fault(NumericFaultCause::Overflow)
                );
                assert_eq!(
                    minimum.checked(CheckedNumericOperator::Remainder, dtype, Some(minus_one)),
                    IntegerEvaluation::Value(zero)
                );
            }
        }
        let maximum = integer(PrimitiveType::U64, i128::from(u64::MAX));
        assert_eq!(
            maximum.checked(
                CheckedNumericOperator::Multiply,
                PrimitiveType::U64,
                Some(maximum)
            ),
            IntegerEvaluation::Fault(NumericFaultCause::Overflow)
        );
    }

    #[test]
    fn promotion_conversion_precedes_arithmetic_and_invalid_requests_stay_distinct() {
        let zero = integer(PrimitiveType::I8, 0);
        assert_eq!(
            zero.checked(
                CheckedNumericOperator::Add,
                PrimitiveType::I8,
                Some(integer(PrimitiveType::U8, 127))
            ),
            IntegerEvaluation::Value(integer(PrimitiveType::I8, 127))
        );
        assert_eq!(
            zero.checked(
                CheckedNumericOperator::Add,
                PrimitiveType::I8,
                Some(integer(PrimitiveType::U8, 255))
            ),
            IntegerEvaluation::Fault(NumericFaultCause::Overflow)
        );
        assert_eq!(
            zero.checked(CheckedNumericOperator::Add, PrimitiveType::I8, None),
            IntegerEvaluation::Invalid
        );
        assert_eq!(
            zero.checked(
                CheckedNumericOperator::Negate,
                PrimitiveType::I8,
                Some(zero)
            ),
            IntegerEvaluation::Invalid
        );
        assert_eq!(
            zero.checked(CheckedNumericOperator::Negate, PrimitiveType::I16, None),
            IntegerEvaluation::Invalid
        );
        let nat = integer(PrimitiveType::Nat, 1);
        assert_eq!(
            nat.checked(CheckedNumericOperator::Negate, PrimitiveType::Nat, None),
            IntegerEvaluation::Invalid
        );
        assert_eq!(
            zero.checked(CheckedNumericOperator::Add, PrimitiveType::I8, Some(nat)),
            IntegerEvaluation::Invalid
        );
        assert!(nat.wrapping_add(nat).is_none());
        assert!(zero.wrapping_add(integer(PrimitiveType::U8, 0)).is_none());
    }
}
