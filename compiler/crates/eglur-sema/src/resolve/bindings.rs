use std::num::NonZeroUsize;

use eglur_hir::{BodyId, PatternId};

use crate::{LocalBindingId, ModuleId};

#[derive(Clone, Copy)]
pub(crate) enum PatternBindingSite {
    Name(PatternId),
    Shorthand { pattern: PatternId, field: usize },
}

impl PatternBindingSite {
    fn pattern(self) -> PatternId {
        match self {
            Self::Name(pattern) | Self::Shorthand { pattern, .. } => pattern,
        }
    }
}

#[derive(Default)]
pub(super) struct ResolvedPatternBindings {
    modules: Vec<Vec<Option<BodyBindings>>>,
}

struct BodyBindings {
    body: BodyId,
    slots: Vec<PatternSlots>,
    sites: Vec<SiteBindings>,
    roots: Vec<Box<[LocalBindingId]>>,
}

#[derive(Clone, Copy, Default)]
struct PatternSlots {
    site: Option<NonZeroUsize>,
    root: Option<NonZeroUsize>,
}

enum SiteBindings {
    Name(LocalBindingId),
    Shorthands(Vec<Option<LocalBindingId>>),
}

pub(crate) struct BodyBindingView<'a> {
    bindings: &'a BodyBindings,
}

impl ResolvedPatternBindings {
    pub(super) fn body(&self, module: ModuleId, body: BodyId) -> Option<BodyBindingView<'_>> {
        let bindings = self
            .modules
            .get(module.index())?
            .get(body.get() as usize)?
            .as_ref()?;
        debug_assert_eq!(bindings.body, body);
        Some(BodyBindingView { bindings })
    }

    pub(super) fn insert(&mut self, site: PatternBindingSite, binding: LocalBindingId) {
        let pattern = site.pattern();
        assert_eq!(pattern.body(), binding.body());
        let body = self.body_mut(binding.module(), binding.body());
        body.ensure_pattern(pattern);
        let slot = &mut body.slots[pattern.local() as usize].site;
        match site {
            PatternBindingSite::Name(_) => {
                assert!(slot.is_none(), "binding sites activate once");
                *slot = Some(NonZeroUsize::new(body.sites.len() + 1).unwrap());
                body.sites.push(SiteBindings::Name(binding));
            }
            PatternBindingSite::Shorthand { field, .. } => {
                let position = *slot.get_or_insert_with(|| {
                    body.sites.push(SiteBindings::Shorthands(Vec::new()));
                    NonZeroUsize::new(body.sites.len()).unwrap()
                });
                let SiteBindings::Shorthands(fields) = &mut body.sites[position.get() - 1] else {
                    unreachable!("a pattern cannot be both a name and a field container")
                };
                if fields.len() <= field {
                    fields.resize(field + 1, None);
                }
                assert!(
                    fields[field].replace(binding).is_none(),
                    "binding sites activate once"
                );
            }
        }
    }

    pub(super) fn publish_root(
        &mut self,
        module: ModuleId,
        pattern: PatternId,
        bindings: Vec<LocalBindingId>,
    ) {
        let body = self.body_mut(module, pattern.body());
        body.ensure_pattern(pattern);
        let slot = &mut body.slots[pattern.local() as usize].root;
        if let Some(position) = *slot {
            // Repeated empty parameter records introduce no new identity.
            assert!(bindings.is_empty() && body.roots[position.get() - 1].is_empty());
            return;
        }
        *slot = Some(NonZeroUsize::new(body.roots.len() + 1).unwrap());
        body.roots.push(bindings.into_boxed_slice());
    }

    fn body_mut(&mut self, module: ModuleId, body: BodyId) -> &mut BodyBindings {
        if self.modules.len() <= module.index() {
            self.modules.resize_with(module.index() + 1, Vec::new);
        }
        let bodies = &mut self.modules[module.index()];
        if bodies.len() <= body.get() as usize {
            bodies.resize_with(body.get() as usize + 1, || None);
        }
        bodies[body.get() as usize].get_or_insert_with(|| BodyBindings {
            body,
            slots: Vec::new(),
            sites: Vec::new(),
            roots: Vec::new(),
        })
    }
}

impl BodyBindings {
    fn ensure_pattern(&mut self, pattern: PatternId) {
        if self.slots.len() <= pattern.local() as usize {
            self.slots
                .resize(pattern.local() as usize + 1, PatternSlots::default());
        }
    }
}

impl BodyBindingView<'_> {
    pub(crate) fn binding(&self, site: PatternBindingSite) -> Option<LocalBindingId> {
        let pattern = site.pattern();
        if pattern.body() != self.bindings.body {
            return None;
        }
        let position = self.bindings.slots.get(pattern.local() as usize)?.site?;
        match (&self.bindings.sites[position.get() - 1], site) {
            (SiteBindings::Name(binding), PatternBindingSite::Name(_)) => Some(*binding),
            (SiteBindings::Shorthands(fields), PatternBindingSite::Shorthand { field, .. }) => {
                fields.get(field).copied().flatten()
            }
            _ => None,
        }
    }

    pub(crate) fn root(&self, pattern: PatternId) -> Option<&[LocalBindingId]> {
        if pattern.body() != self.bindings.body {
            return None;
        }
        let position = self.bindings.slots.get(pattern.local() as usize)?.root?;
        Some(&self.bindings.roots[position.get() - 1])
    }
}

#[cfg(test)]
mod tests;
