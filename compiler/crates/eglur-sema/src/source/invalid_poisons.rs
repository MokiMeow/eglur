use eglur_diagnostics::{DiagnosticStoreError, Poison, PoisonAccumulator};

pub(super) enum InvalidPoisonGroup {
    One(Poison),
    Two(Poison),
    Many(PoisonAccumulator),
}

impl InvalidPoisonGroup {
    pub(super) fn include(self, poison: Poison) -> Result<Self, DiagnosticStoreError> {
        match self {
            Self::One(first) => {
                #[cfg(test)]
                tests::observe(tests::Operation::Pair);

                Ok(Self::Two(first.merge(&poison)?))
            }
            Self::Two(joined) => {
                let mut accumulator = PoisonAccumulator::new();

                #[cfg(test)]
                tests::observe(tests::Operation::Include);

                accumulator.include(&joined)?;

                #[cfg(test)]
                tests::observe(tests::Operation::Include);

                accumulator.include(&poison)?;
                Ok(Self::Many(accumulator))
            }
            Self::Many(mut accumulator) => {
                #[cfg(test)]
                tests::observe(tests::Operation::Include);

                accumulator.include(&poison)?;
                Ok(Self::Many(accumulator))
            }
        }
    }

    pub(super) fn finish(self) -> Poison {
        match self {
            Self::One(poison) | Self::Two(poison) => poison,
            Self::Many(accumulator) => {
                #[cfg(test)]
                tests::observe(tests::Operation::Finish);

                accumulator.finish().expect("nonempty invalid poison group")
            }
        }
    }
}

#[cfg(test)]
mod tests;
