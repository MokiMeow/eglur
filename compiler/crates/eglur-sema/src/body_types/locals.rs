use std::num::NonZeroUsize;

use eglur_hir::BodyId;

use crate::{DuplicateFact, FactState, LocalBindingId, SemanticFact};

use super::LocalTypeFact;

/// Local facts indexed by canonical module, body, then local binding ordinal.
/// Empty directory slots retain only a position, not a full fact payload.
#[derive(Debug)]
pub(super) struct LocalTypeFacts {
    slots: Vec<Vec<Option<BodyLocalSlots>>>,
    records: Vec<SemanticFact<LocalTypeFact>>,
}

#[derive(Debug)]
struct BodyLocalSlots {
    body: BodyId,
    positions: Vec<Option<NonZeroUsize>>,
}

impl LocalTypeFacts {
    pub(super) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            records: Vec::new(),
        }
    }

    pub(super) fn insert(
        &mut self,
        binding: LocalBindingId,
        fact: SemanticFact<LocalTypeFact>,
    ) -> Result<(), DuplicateFact> {
        if self.position(&binding).is_some() {
            return Err(DuplicateFact);
        }

        let module = binding.module().index();
        let body = binding.body().get() as usize;
        let local = binding.local() as usize;
        if self.slots.len() <= module {
            self.slots.resize_with(module + 1, Vec::new);
        }
        let bodies = &mut self.slots[module];
        if bodies.len() <= body {
            bodies.resize_with(body + 1, || None);
        }
        let row = bodies[body].get_or_insert_with(|| BodyLocalSlots {
            body: binding.body(),
            positions: Vec::new(),
        });
        let locals = &mut row.positions;
        if locals.len() <= local {
            locals.resize(local + 1, None);
        }

        // Nonzero-sized Vec elements bound len below usize::MAX.
        let position = NonZeroUsize::new(self.records.len() + 1).unwrap();
        self.records.push(fact);
        locals[local] = Some(position);
        Ok(())
    }

    pub(super) fn state(&self, binding: &LocalBindingId) -> FactState<'_, LocalTypeFact> {
        self.recorded_state(binding)
            .unwrap_or(FactState::Unperformed)
    }

    pub(super) fn recorded_state(
        &self,
        binding: &LocalBindingId,
    ) -> Option<FactState<'_, LocalTypeFact>> {
        self.position(binding)
            .map(|position| match &self.records[position] {
                SemanticFact::Known(value) => FactState::Known(value),
                SemanticFact::Poisoned(poison) => FactState::Poisoned(poison),
                SemanticFact::NotApplicable => FactState::NotApplicable,
            })
    }

    pub(super) fn len(&self) -> usize {
        self.records.len()
    }

    pub(super) fn has_poisoned(&self) -> bool {
        self.records
            .iter()
            .any(|fact| matches!(fact, SemanticFact::Poisoned(_)))
    }

    fn position(&self, binding: &LocalBindingId) -> Option<usize> {
        let row = self
            .slots
            .get(binding.module().index())?
            .get(binding.body().get() as usize)?
            .as_ref()?;
        debug_assert_eq!(row.body, binding.body());
        let position = row.positions.get(binding.local() as usize)?;
        position.map(|position| position.get() - 1)
    }

    #[cfg(test)]
    pub(super) fn facts(&self) -> impl ExactSizeIterator<Item = &SemanticFact<LocalTypeFact>> {
        self.records.iter()
    }

    #[cfg(test)]
    pub(super) fn entries(
        &self,
    ) -> impl Iterator<Item = (LocalBindingId, &SemanticFact<LocalTypeFact>)> {
        self.slots
            .iter()
            .enumerate()
            .flat_map(move |(index, bodies)| {
                let module = crate::ModuleId::from_index(index).unwrap();
                bodies.iter().flatten().flat_map(move |row| {
                    row.positions
                        .iter()
                        .enumerate()
                        .filter_map(move |(local, position)| {
                            position.map(|position| {
                                let binding =
                                    LocalBindingId::from_index(module, row.body, local).unwrap();
                                (binding, &self.records[position.get() - 1])
                            })
                        })
                })
            })
    }
}

#[cfg(test)]
mod tests;
