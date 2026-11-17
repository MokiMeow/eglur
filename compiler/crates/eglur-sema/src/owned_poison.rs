use eglur_diagnostics::{DiagnosticStoreError, Poison, PoisonAccumulator};

pub(crate) struct OwnedPoisonUnion {
    state: State,
}

enum State {
    One(Poison),
    Two(Poison),
    Many(PoisonAccumulator),
}

impl OwnedPoisonUnion {
    pub(crate) fn new(poison: Poison) -> Self {
        Self {
            state: State::One(poison),
        }
    }

    pub(crate) fn include(self, poison: Poison) -> Result<Self, DiagnosticStoreError> {
        let state = match self.state {
            State::One(first) => {
                #[cfg(test)]
                test_support::observe(test_support::Operation::Pair);

                State::Two(first.merge(&poison)?)
            }
            State::Two(joined) => {
                let mut accumulator = PoisonAccumulator::new();

                #[cfg(test)]
                test_support::observe(test_support::Operation::Include);

                accumulator.include(&joined)?;

                #[cfg(test)]
                test_support::observe(test_support::Operation::Include);

                accumulator.include(&poison)?;
                State::Many(accumulator)
            }
            State::Many(mut accumulator) => {
                #[cfg(test)]
                test_support::observe(test_support::Operation::Include);

                accumulator.include(&poison)?;
                State::Many(accumulator)
            }
        };
        Ok(Self { state })
    }

    pub(crate) fn finish(self) -> Poison {
        match self.state {
            State::One(poison) | State::Two(poison) => poison,
            State::Many(accumulator) => {
                #[cfg(test)]
                test_support::observe(test_support::Operation::Finish);

                accumulator.finish().expect("nonempty owned poison union")
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
