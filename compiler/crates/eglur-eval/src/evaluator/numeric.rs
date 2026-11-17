use super::*;

pub(super) fn numeric_fault(
    fault: NumericFaultCause,
    operator: CheckedNumericOperator,
    primitive: PrimitiveType,
) -> RuntimeDiagnostic {
    let (code, message) = match fault {
        NumericFaultCause::Overflow => ("INT_OVERFLOW", "integer overflow"),
        NumericFaultCause::DivisionByZero => ("INT_DIV_BY_ZERO", "integer division by zero"),
        NumericFaultCause::RemainderByZero => ("INT_REM_BY_ZERO", "integer remainder by zero"),
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
