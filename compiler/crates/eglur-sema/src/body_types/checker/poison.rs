use super::{BodyTypingError, Poison};

#[derive(Default)]
pub(super) struct PendingPoison {
    pending: Option<eglur_diagnostics::PoisonAccumulator>,
}

impl PendingPoison {
    pub(super) fn include(&mut self, incoming: &Poison) -> Result<(), BodyTypingError> {
        #[cfg(test)]
        tests::observe(tests::Step::Input);

        // Taking the slot preserves the caller's empty state after a foreign-pool refusal.
        let mut pending = self.pending.take().unwrap_or_default();
        pending.include(incoming)?;
        self.pending = Some(pending);
        Ok(())
    }

    pub(super) fn take(&mut self) -> Option<Poison> {
        let poison = self.pending.take()?.finish();

        #[cfg(test)]
        tests::observe(tests::Step::Complete);

        poison
    }
}

#[cfg(test)]
pub(super) mod tests;
