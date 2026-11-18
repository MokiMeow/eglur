use super::*;

#[derive(Default)]
pub(in super::super) struct PatternBindingScratch {
    modules: Vec<Vec<Option<BodyScratch>>>,
}

struct BodyScratch {
    root: PatternId,
    slots: Vec<BindingSlot>,
    touched: Vec<usize>,
    duplicate: Option<DuplicateBinding>,
}

#[derive(Clone, Copy, Default)]
struct BindingSlot {
    seen: bool,
    ty: Option<TypeId>,
}

#[derive(Clone, Copy)]
pub(in super::super) struct DuplicateBinding {
    pub(in super::super) binding: LocalBindingId,
    pub(in super::super) pattern: PatternId,
    pub(in super::super) origin: OriginId,
}

impl PatternBindingScratch {
    pub(in super::super) fn begin(&mut self, module: ModuleId, root: PatternId) {
        if self.modules.len() <= module.index() {
            self.modules.resize_with(module.index() + 1, Vec::new);
        }
        let bodies = &mut self.modules[module.index()];
        let body = root.body().get() as usize;
        if bodies.len() <= body {
            bodies.resize_with(body + 1, || None);
        }
        let row = bodies[body].get_or_insert_with(|| BodyScratch {
            root,
            slots: Vec::new(),
            touched: Vec::new(),
            duplicate: None,
        });
        for local in row.touched.drain(..) {
            row.slots[local] = BindingSlot::default();
            #[cfg(test)]
            tests::observe_reset();
        }
        row.root = root;
        row.duplicate = None;
    }

    pub(in super::super) fn observe(
        &mut self,
        binding: LocalBindingId,
        pattern: PatternId,
        origin: OriginId,
    ) -> Result<(), BodyTypingError> {
        let row = self.row_mut(binding)?;
        if pattern.body() != row.root.body() {
            return Err(BodyTypingError::InconsistentPatternBinding(binding));
        }
        let local = binding.local() as usize;
        if row.slots.len() <= local {
            row.slots.resize(local + 1, BindingSlot::default());
        }
        let slot = &mut row.slots[local];
        if slot.seen {
            row.duplicate.get_or_insert(DuplicateBinding {
                binding,
                pattern,
                origin,
            });
        } else {
            slot.seen = true;
            row.touched.push(local);
        }
        #[cfg(test)]
        tests::observe_occurrence();
        Ok(())
    }

    pub(in super::super) fn type_binding(
        &mut self,
        binding: LocalBindingId,
        ty: TypeId,
    ) -> Result<(), BodyTypingError> {
        let slot = self
            .row_mut(binding)?
            .slots
            .get_mut(binding.local() as usize)
            .filter(|slot| slot.seen)
            .ok_or(BodyTypingError::InconsistentPatternBinding(binding))?;
        slot.ty.get_or_insert(ty);
        #[cfg(test)]
        tests::observe_type();
        Ok(())
    }

    pub(in super::super) fn ty(&self, binding: LocalBindingId) -> Result<TypeId, BodyTypingError> {
        self.row(binding.module(), binding.body())
            .and_then(|row| row.slots.get(binding.local() as usize))
            .and_then(|slot| slot.ty)
            .ok_or(BodyTypingError::InconsistentPatternBinding(binding))
    }

    pub(in super::super) fn duplicate(
        &self,
        module: ModuleId,
        root: PatternId,
    ) -> Option<DuplicateBinding> {
        self.row(module, root.body())
            .filter(|row| row.root == root)?
            .duplicate
    }

    pub(in super::super) fn first_binding(
        &self,
        module: ModuleId,
        root: PatternId,
    ) -> Option<LocalBindingId> {
        let row = self
            .row(module, root.body())
            .filter(|row| row.root == root)?;
        LocalBindingId::from_index(module, root.body(), *row.touched.first()?).ok()
    }

    fn row(&self, module: ModuleId, body: BodyId) -> Option<&BodyScratch> {
        self.modules
            .get(module.index())?
            .get(body.get() as usize)?
            .as_ref()
    }

    fn row_mut(&mut self, binding: LocalBindingId) -> Result<&mut BodyScratch, BodyTypingError> {
        self.modules
            .get_mut(binding.module().index())
            .and_then(|bodies| bodies.get_mut(binding.body().get() as usize))
            .and_then(Option::as_mut)
            .ok_or(BodyTypingError::InconsistentPatternBinding(binding))
    }
}

#[cfg(test)]
mod tests;
