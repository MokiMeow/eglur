use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IntegerFault {
    Overflow,
    DivideByZero,
    RemainderByZero,
}

pub(super) enum IntegerEvaluation {
    Value(IntegerValue),
    Fault(IntegerFault),
    Invalid,
}

pub(super) fn checked_integer(
    operator: CheckedNumericOperator,
    destination: PrimitiveType,
    left: IntegerValue,
    right: Option<IntegerValue>,
) -> IntegerEvaluation {
    if !is_fixed_width_integer(destination)
        || (operator == CheckedNumericOperator::Negate) != right.is_none()
        || (operator == CheckedNumericOperator::Negate && left.primitive() != destination)
    {
        return IntegerEvaluation::Invalid;
    }
    let left = match convert_integer(left, destination) {
        Some(left) => left,
        None => return IntegerEvaluation::Fault(IntegerFault::Overflow),
    };
    let right = match right {
        Some(right) => match convert_integer(right, destination) {
            Some(right) => Some(right.value()),
            None => return IntegerEvaluation::Fault(IntegerFault::Overflow),
        },
        None => None,
    };
    let value = match operator {
        CheckedNumericOperator::Add => left.value().checked_add(match right {
            Some(right) => right,
            None => return IntegerEvaluation::Invalid,
        }),
        CheckedNumericOperator::Subtract => left.value().checked_sub(match right {
            Some(right) => right,
            None => return IntegerEvaluation::Invalid,
        }),
        CheckedNumericOperator::Multiply => left.value().checked_mul(match right {
            Some(right) => right,
            None => return IntegerEvaluation::Invalid,
        }),
        CheckedNumericOperator::Divide => {
            let Some(right) = right else {
                return IntegerEvaluation::Invalid;
            };
            if right == 0 {
                return IntegerEvaluation::Fault(IntegerFault::DivideByZero);
            }
            left.value().checked_div(right)
        }
        CheckedNumericOperator::Remainder => {
            let Some(right) = right else {
                return IntegerEvaluation::Invalid;
            };
            if right == 0 {
                return IntegerEvaluation::Fault(IntegerFault::RemainderByZero);
            }
            left.value().checked_rem(right)
        }
        CheckedNumericOperator::Negate => {
            if right.is_some() {
                return IntegerEvaluation::Invalid;
            }
            left.value().checked_neg()
        }
    };
    let Some(value) = value else {
        return IntegerEvaluation::Fault(IntegerFault::Overflow);
    };
    if integer_in_range(destination, value) {
        IntegerEvaluation::Value(IntegerValue::new(destination, value))
    } else {
        IntegerEvaluation::Fault(IntegerFault::Overflow)
    }
}

fn convert_integer(value: IntegerValue, destination: PrimitiveType) -> Option<IntegerValue> {
    is_fixed_width_integer(value.primitive())
        .then_some(value.value())
        .filter(|value| integer_in_range(destination, *value))
        .map(|value| IntegerValue::new(destination, value))
}

pub(super) const fn is_fixed_width_integer(primitive: PrimitiveType) -> bool {
    matches!(
        primitive,
        PrimitiveType::I8
            | PrimitiveType::I16
            | PrimitiveType::I32
            | PrimitiveType::I64
            | PrimitiveType::U8
            | PrimitiveType::U16
            | PrimitiveType::U32
            | PrimitiveType::U64
    )
}

fn integer_in_range(primitive: PrimitiveType, value: i128) -> bool {
    match primitive {
        PrimitiveType::I8 => i8::try_from(value).is_ok(),
        PrimitiveType::I16 => i16::try_from(value).is_ok(),
        PrimitiveType::I32 => i32::try_from(value).is_ok(),
        PrimitiveType::I64 => i64::try_from(value).is_ok(),
        PrimitiveType::U8 => u8::try_from(value).is_ok(),
        PrimitiveType::U16 => u16::try_from(value).is_ok(),
        PrimitiveType::U32 => u32::try_from(value).is_ok(),
        PrimitiveType::U64 => u64::try_from(value).is_ok(),
        PrimitiveType::Unit
        | PrimitiveType::Bool
        | PrimitiveType::Str
        | PrimitiveType::F16
        | PrimitiveType::Bf16
        | PrimitiveType::F32
        | PrimitiveType::F64
        | PrimitiveType::Complex64
        | PrimitiveType::Complex128
        | PrimitiveType::Nat
        | PrimitiveType::PositiveNat => false,
    }
}

pub(super) fn wrapping_add(left: IntegerValue, right: IntegerValue) -> Option<IntegerValue> {
    let primitive = left.primitive();
    let value = match primitive {
        PrimitiveType::I8 => i128::from((left.value() as i8).wrapping_add(right.value() as i8)),
        PrimitiveType::I16 => i128::from((left.value() as i16).wrapping_add(right.value() as i16)),
        PrimitiveType::I32 => i128::from((left.value() as i32).wrapping_add(right.value() as i32)),
        PrimitiveType::I64 => i128::from((left.value() as i64).wrapping_add(right.value() as i64)),
        PrimitiveType::U8 => i128::from((left.value() as u8).wrapping_add(right.value() as u8)),
        PrimitiveType::U16 => i128::from((left.value() as u16).wrapping_add(right.value() as u16)),
        PrimitiveType::U32 => i128::from((left.value() as u32).wrapping_add(right.value() as u32)),
        PrimitiveType::U64 => i128::from((left.value() as u64).wrapping_add(right.value() as u64)),
        _ => return None,
    };
    Some(IntegerValue::new(primitive, value))
}

pub(super) fn numeric_fault(
    fault: IntegerFault,
    operator: CheckedNumericOperator,
    primitive: PrimitiveType,
) -> RuntimeDiagnostic {
    let (code, message) = match fault {
        IntegerFault::Overflow => ("INT_OVERFLOW", "integer overflow"),
        IntegerFault::DivideByZero => ("INT_DIV_BY_ZERO", "integer division by zero"),
        IntegerFault::RemainderByZero => ("INT_REM_BY_ZERO", "integer remainder by zero"),
    };
    RuntimeDiagnostic::new(
        code,
        message,
        [
            (
                Box::<str>::from("operation"),
                Box::<str>::from(match operator {
                    CheckedNumericOperator::Add => "add",
                    CheckedNumericOperator::Subtract => "subtract",
                    CheckedNumericOperator::Multiply => "multiply",
                    CheckedNumericOperator::Divide => "divide",
                    CheckedNumericOperator::Remainder => "remainder",
                    CheckedNumericOperator::Negate => "negate",
                }),
            ),
            (
                Box::<str>::from("dtype"),
                Box::<str>::from(primitive.spelling()),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn i8_value(value: i128) -> IntegerValue {
        IntegerValue::new(PrimitiveType::I8, value)
    }

    #[test]
    fn checked_integer_faults_distinguish_zero_and_signed_overflow() {
        assert!(matches!(
            checked_integer(
                CheckedNumericOperator::Divide,
                PrimitiveType::I8,
                i8_value(1),
                Some(i8_value(0)),
            ),
            IntegerEvaluation::Fault(IntegerFault::DivideByZero)
        ));
        assert!(matches!(
            checked_integer(
                CheckedNumericOperator::Remainder,
                PrimitiveType::I8,
                i8_value(1),
                Some(i8_value(0)),
            ),
            IntegerEvaluation::Fault(IntegerFault::RemainderByZero)
        ));
        assert!(matches!(
            checked_integer(
                CheckedNumericOperator::Divide,
                PrimitiveType::I8,
                i8_value(i128::from(i8::MIN)),
                Some(i8_value(-1)),
            ),
            IntegerEvaluation::Fault(IntegerFault::Overflow)
        ));
        assert!(matches!(
            checked_integer(
                CheckedNumericOperator::Remainder,
                PrimitiveType::I8,
                i8_value(i128::from(i8::MIN)),
                Some(i8_value(-1)),
            ),
            IntegerEvaluation::Value(value) if value.value() == 0
        ));
    }

    #[test]
    fn checked_integer_converts_operands_to_the_typed_destination_before_arithmetic() {
        let u8_value = |value| IntegerValue::new(PrimitiveType::U8, value);

        assert!(matches!(
            checked_integer(
                CheckedNumericOperator::Add,
                PrimitiveType::I8,
                i8_value(0),
                Some(u8_value(127)),
            ),
            IntegerEvaluation::Value(value)
                if value.primitive() == PrimitiveType::I8 && value.value() == 127
        ));
        assert!(matches!(
            checked_integer(
                CheckedNumericOperator::Add,
                PrimitiveType::I8,
                i8_value(0),
                Some(u8_value(255)),
            ),
            IntegerEvaluation::Fault(IntegerFault::Overflow)
        ));
    }
}
