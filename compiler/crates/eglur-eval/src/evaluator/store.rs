use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RuntimeValue {
    Value(Value),
    Reference(Place),
    Callable(RuntimeCallable),
    Tuple(Box<[RuntimeValue]>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RuntimeCallable {
    pub(super) target: CanonicalTarget,
    pub(super) witnesses: Witnesses,
}

impl RuntimeValue {
    pub(super) fn tuple(values: impl IntoIterator<Item = Self>) -> Self {
        Self::Tuple(values.into_iter().collect::<Vec<_>>().into_boxed_slice())
    }

    pub(super) fn tuple_len(&self) -> Option<usize> {
        match self {
            Self::Tuple(values) => Some(values.len()),
            Self::Value(Value::Tuple(values)) => Some(values.len()),
            Self::Value(_) | Self::Reference(_) | Self::Callable(_) => None,
        }
    }

    pub(super) fn tuple_element(&self, index: usize) -> Option<Self> {
        match self {
            Self::Tuple(values) => values.get(index).cloned(),
            Self::Value(Value::Tuple(values)) => values.get(index).cloned().map(Self::Value),
            Self::Value(_) | Self::Reference(_) | Self::Callable(_) => None,
        }
    }

    pub(super) fn into_tuple(self) -> Option<Box<[Self]>> {
        match self {
            Self::Tuple(values) => Some(values),
            Self::Value(Value::Tuple(values)) => Some(
                values
                    .into_vec()
                    .into_iter()
                    .map(Self::Value)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            Self::Value(_) | Self::Reference(_) | Self::Callable(_) => None,
        }
    }

    pub(super) fn into_public(self) -> Option<Value> {
        match self {
            Self::Value(value) => Some(value),
            Self::Reference(_) | Self::Callable(_) => None,
            Self::Tuple(values) => values
                .into_vec()
                .into_iter()
                .map(Self::into_public)
                .collect::<Option<Vec<_>>>()
                .map(|values| Value::Tuple(values.into_boxed_slice())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Place {
    pub(super) root: usize,
    pub(super) projections: Vec<Projection>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Projection {
    Field(CanonicalTarget),
    Tuple(usize),
}

#[derive(Default)]
pub(super) struct Store {
    pub(super) slots: Vec<RuntimeValue>,
}

impl Store {
    pub(super) fn borrow_temporary(&mut self, value: RuntimeValue) -> RuntimeValue {
        RuntimeValue::Reference(Place {
            root: self.allocate(value),
            projections: Vec::new(),
        })
    }

    pub(super) fn allocate(&mut self, value: RuntimeValue) -> usize {
        let slot = self.slots.len();
        self.slots.push(value);
        slot
    }

    pub(super) fn read(&self, place: &Place) -> Option<RuntimeValue> {
        let mut value = self.slots.get(place.root)?.clone();
        for projection in &place.projections {
            value = match projection {
                Projection::Field(field) => {
                    let RuntimeValue::Value(Value::Struct { fields, .. }) = value else {
                        return None;
                    };
                    RuntimeValue::Value(
                        fields
                            .iter()
                            .find(|(candidate, _)| candidate == field)?
                            .1
                            .clone(),
                    )
                }
                Projection::Tuple(index) => value.tuple_element(*index)?,
            };
        }
        Some(value)
    }

    pub(super) fn write(&mut self, place: &Place, value: RuntimeValue) -> Option<()> {
        fn write_projection(
            current: &mut RuntimeValue,
            projections: &[Projection],
            value: RuntimeValue,
        ) -> Option<()> {
            let Some((head, tail)) = projections.split_first() else {
                *current = value;
                return Some(());
            };
            match head {
                Projection::Field(field) => {
                    let RuntimeValue::Value(Value::Struct { fields, .. }) = current else {
                        return None;
                    };
                    let (_, child) = fields
                        .iter_mut()
                        .find(|(candidate, _)| candidate == field)?;
                    let mut runtime = RuntimeValue::Value(child.clone());
                    write_projection(&mut runtime, tail, value)?;
                    *child = runtime.into_public()?;
                    Some(())
                }
                Projection::Tuple(index) => match current {
                    RuntimeValue::Tuple(values) => {
                        write_projection(values.get_mut(*index)?, tail, value)
                    }
                    RuntimeValue::Value(Value::Tuple(values)) => {
                        let child = values.get_mut(*index)?;
                        let mut runtime = RuntimeValue::Value(child.clone());
                        write_projection(&mut runtime, tail, value)?;
                        *child = runtime.into_public()?;
                        Some(())
                    }
                    RuntimeValue::Value(_)
                    | RuntimeValue::Reference(_)
                    | RuntimeValue::Callable(_) => None,
                },
            }
        }
        write_projection(self.slots.get_mut(place.root)?, &place.projections, value)
    }
}

#[cfg(test)]
mod tests {
    use super::{Place, RuntimeValue};
    use crate::Value;

    #[test]
    fn public_projection_is_recursive_and_rejects_nested_places() {
        let public = RuntimeValue::tuple([
            RuntimeValue::Value(Value::Unit),
            RuntimeValue::tuple([RuntimeValue::Value(Value::Bool(true))]),
        ]);
        assert_eq!(
            public.into_public(),
            Some(Value::Tuple(Box::new([
                Value::Unit,
                Value::Tuple(Box::new([Value::Bool(true)])),
            ])))
        );

        let reference =
            RuntimeValue::tuple([RuntimeValue::tuple([RuntimeValue::Reference(Place {
                root: 0,
                projections: Vec::new(),
            })])]);
        assert_eq!(reference.into_public(), None);
    }
}
