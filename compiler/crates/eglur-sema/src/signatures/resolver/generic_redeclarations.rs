use std::collections::{BTreeMap, BTreeSet};

use crate::types::is_reserved_type_name;
use crate::{DeclarationCollection, DeclarationKind, DefId};

enum ScopeStep<'name> {
    Enter(DefId),
    Leave(BTreeSet<&'name str>),
}

#[derive(Default)]
struct ScopeNames<'name> {
    counts: BTreeMap<&'name str, usize>,
}

impl<'name> ScopeNames<'name> {
    fn contains(&self, name: &str) -> bool {
        #[cfg(test)]
        tests::observe_membership();

        self.counts.contains_key(name)
    }

    fn enter(&mut self, names: &BTreeSet<&'name str>) {
        for &name in names {
            *self.counts.entry(name).or_default() += 1;
            #[cfg(test)]
            tests::observe_activation(self.counts.len());
        }
    }

    fn leave(&mut self, names: &BTreeSet<&'name str>) {
        for &name in names {
            let count = self.counts.get_mut(name).expect("scope name was activated");
            // An invalid redeclaring child must not erase its parent's name.
            if *count == 1 {
                self.counts.remove(name);
            } else {
                *count -= 1;
            }
            #[cfg(test)]
            tests::observe_deactivation();
        }
    }
}

pub(super) fn classify(collection: &DeclarationCollection) -> BTreeSet<DefId> {
    let mut redeclarations = BTreeSet::new();
    for module in collection.modules() {
        let mut by_owner = BTreeMap::<DefId, Vec<(DefId, &str)>>::new();
        let mut pending = Vec::new();
        for declaration in module.declarations() {
            #[cfg(test)]
            tests::observe_inventory();

            if declaration.parent().is_none() {
                pending.push(ScopeStep::Enter(declaration.id()));
            }
            if declaration.kind() != DeclarationKind::GenericParameter {
                continue;
            }
            let (Some(owner), Some(name)) = (declaration.parent(), declaration.name()) else {
                continue;
            };
            if is_reserved_type_name(name.text()) {
                continue;
            }
            by_owner
                .entry(owner)
                .or_default()
                .push((declaration.id(), name.text()));
            #[cfg(test)]
            tests::observe_raw_row();
        }
        if by_owner.is_empty() {
            continue;
        }
        pending.reverse();

        let mut active = ScopeNames::default();
        while let Some(step) = pending.pop() {
            match step {
                ScopeStep::Enter(owner) => {
                    #[cfg(test)]
                    tests::observe_enter();

                    let mut local_names = BTreeSet::new();
                    if let Some(parameters) = by_owner.remove(&owner) {
                        for (generic, name) in parameters {
                            #[cfg(test)]
                            tests::observe_direct();

                            if !local_names.insert(name) || active.contains(name) {
                                redeclarations.insert(generic);
                                #[cfg(test)]
                                tests::observe_output();
                            }
                        }
                    }
                    active.enter(&local_names);
                    pending.push(ScopeStep::Leave(local_names));

                    let first_child = pending.len();
                    for child in module.children(owner) {
                        #[cfg(test)]
                        tests::observe_child();

                        pending.push(ScopeStep::Enter(child.id()));
                    }
                    pending[first_child..].reverse();
                }
                ScopeStep::Leave(names) => {
                    active.leave(&names);
                    #[cfg(test)]
                    tests::observe_leave();
                }
            }
        }
        debug_assert!(active.counts.is_empty());
        debug_assert!(by_owner.is_empty());
    }
    redeclarations
}

#[cfg(test)]
mod tests;
