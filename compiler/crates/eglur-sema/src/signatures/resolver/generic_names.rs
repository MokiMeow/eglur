use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::{DeclarationCollection, DeclarationKind, DefId, GenericParameterId};

#[derive(Default)]
pub(super) struct SourceGenericNameIndex {
    by_owner: BTreeMap<DefId, BTreeMap<Box<str>, GenericParameterId>>,
}

impl SourceGenericNameIndex {
    pub(super) fn new(
        collection: &DeclarationCollection,
        redeclarations: &BTreeSet<DefId>,
    ) -> Self {
        let mut by_owner = BTreeMap::<DefId, BTreeMap<Box<str>, GenericParameterId>>::new();
        for declaration in collection
            .modules()
            .flat_map(|module| module.declarations())
        {
            #[cfg(test)]
            tests::observe_declaration();

            if declaration.kind() != DeclarationKind::GenericParameter
                || redeclarations.contains(&declaration.id())
            {
                continue;
            }
            let (Some(owner), Some(name)) = (declaration.parent(), declaration.name()) else {
                continue;
            };
            #[cfg(test)]
            tests::observe_entry();

            let names = by_owner.entry(owner).or_default();
            if let std::collections::btree_map::Entry::Vacant(entry) =
                names.entry(Box::<str>::from(name.text()))
            {
                entry.insert(GenericParameterId::Source(declaration.id()));
                #[cfg(test)]
                tests::observe_row();
            }
        }
        Self { by_owner }
    }

    pub(super) fn for_chain(self: &Rc<Self>, chain: Vec<DefId>) -> SourceGenericNames {
        #[cfg(test)]
        tests::observe_view();

        SourceGenericNames {
            index: Rc::clone(self),
            chain: chain.into(),
        }
    }
}

#[derive(Clone)]
pub(super) struct SourceGenericNames {
    index: Rc<SourceGenericNameIndex>,
    chain: Rc<[DefId]>,
}

impl SourceGenericNames {
    pub(super) fn get(&self, name: &str) -> Option<&GenericParameterId> {
        // Containing generic names take precedence over invalid redeclarations.
        self.chain.iter().find_map(|owner| {
            #[cfg(test)]
            tests::observe_lookup();

            self.index.by_owner.get(owner)?.get(name)
        })
    }

    pub(super) fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
}

#[cfg(test)]
mod tests;
