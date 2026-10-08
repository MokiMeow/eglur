use std::collections::BTreeMap;

use eglur_hir::{EnumVariant, HirFile, ItemId, ItemKind, OriginId};
use eglur_source::FileId;

#[derive(Default)]
pub(super) struct SourceEnumVariantIndex<'hir> {
    by_location: BTreeMap<(FileId, ItemId, OriginId), &'hir EnumVariant>,
}

impl<'hir> SourceEnumVariantIndex<'hir> {
    pub(super) fn new(hirs: impl IntoIterator<Item = &'hir HirFile>) -> Self {
        let mut index = Self {
            by_location: BTreeMap::new(),
        };
        for hir in hirs {
            for (item, value) in hir.items() {
                #[cfg(test)]
                tests::observe_item();

                if let ItemKind::Enum(value) = value.kind() {
                    index.insert_enum(hir.file(), item, &value.variants);
                }
            }
        }
        index
    }

    fn insert_enum(&mut self, file: FileId, item: ItemId, variants: &'hir [EnumVariant]) {
        for variant in variants {
            #[cfg(test)]
            tests::observe_variant();

            self.by_location
                .entry((file, item, variant.origin))
                .or_insert(variant);
        }
    }

    pub(super) fn get(
        &self,
        file: FileId,
        item: ItemId,
        origin: OriginId,
    ) -> Option<&'hir EnumVariant> {
        #[cfg(test)]
        tests::observe_query();

        self.by_location.get(&(file, item, origin)).copied()
    }
}

#[cfg(test)]
pub(super) mod tests;
