use std::collections::BTreeMap;

use eglur_sema::{CanonicalTarget, PrimitiveType};

/// One exact integer or integer-valued domain-scalar runtime value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntegerValue {
    primitive: PrimitiveType,
    value: i128,
}

impl IntegerValue {
    #[must_use]
    pub(crate) const fn new(primitive: PrimitiveType, value: i128) -> Self {
        Self { primitive, value }
    }

    #[must_use]
    pub const fn primitive(self) -> PrimitiveType {
        self.primitive
    }

    #[must_use]
    pub const fn value(self) -> i128 {
        self.value
    }
}

/// Runtime diagnostic retained outside compile diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDiagnostic {
    code: Box<str>,
    message: Box<str>,
    data: BTreeMap<Box<str>, Box<str>>,
}

impl RuntimeDiagnostic {
    #[must_use]
    pub(crate) fn new(
        code: impl Into<Box<str>>,
        message: impl Into<Box<str>>,
        data: impl IntoIterator<Item = (Box<str>, Box<str>)>,
    ) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            data: data.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub const fn data(&self) -> &BTreeMap<Box<str>, Box<str>> {
        &self.data
    }
}

/// Public language value; evaluator places and control tokens cannot escape here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    Unit,
    Bool(bool),
    Integer(IntegerValue),
    Str(Box<str>),
    Tuple(Box<[Value]>),
    Struct {
        declaration: CanonicalTarget,
        fields: Box<[(CanonicalTarget, Value)]>,
    },
    Variant {
        declaration: CanonicalTarget,
        payload: Box<[Value]>,
    },
    NumericError(RuntimeDiagnostic),
}

impl Value {
    #[must_use]
    pub fn variant_declaration(&self) -> Option<&CanonicalTarget> {
        match self {
            Self::Variant { declaration, .. } => Some(declaration),
            _ => None,
        }
    }

    #[must_use]
    pub fn variant_payload(&self) -> Option<&[Value]> {
        match self {
            Self::Variant { payload, .. } => Some(payload),
            _ => None,
        }
    }

    pub(crate) fn collect_value_diagnostics(&self, diagnostics: &mut Vec<RuntimeDiagnostic>) {
        match self {
            Self::NumericError(diagnostic) => diagnostics.push(diagnostic.clone()),
            Self::Tuple(values)
            | Self::Variant {
                payload: values, ..
            } => {
                for value in values {
                    value.collect_value_diagnostics(diagnostics);
                }
            }
            Self::Struct { fields, .. } => {
                for (_, value) in fields {
                    value.collect_value_diagnostics(diagnostics);
                }
            }
            Self::Unit | Self::Bool(_) | Self::Integer(_) | Self::Str(_) => {}
        }
    }
}

/// Ordered evaluator-observable test effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeEvent {
    Emitted(Box<str>),
}
