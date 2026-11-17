use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{CanonicalTarget, ExternalDefId};

/// Closed semantic roles supplied by a trusted language environment.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum LanguageItem {
    Tensor,
    NumericError,
    OptSome,
    OptNone,
    ResOk,
    ResErr,
}

/// Driver-independent mapping from language roles to trusted declarations.
pub struct TrustedLanguageItemEnvironment {
    items: BTreeMap<LanguageItem, ExternalDefId>,
}

impl TrustedLanguageItemEnvironment {
    pub fn new(
        items: impl IntoIterator<Item = (LanguageItem, ExternalDefId)>,
    ) -> Result<Self, TrustedLanguageItemError> {
        let mut normalized = BTreeMap::new();
        let mut declarations = BTreeSet::new();
        for (item, declaration) in items {
            if normalized.insert(item, declaration.clone()).is_some() {
                return Err(TrustedLanguageItemError::DuplicateItem(item));
            }
            if !declarations.insert(declaration.clone()) {
                return Err(TrustedLanguageItemError::DuplicateDeclaration(declaration));
            }
        }
        Ok(Self { items: normalized })
    }

    #[must_use]
    pub fn declaration(&self, item: LanguageItem) -> Option<&ExternalDefId> {
        self.items.get(&item)
    }

    pub fn items(&self) -> impl ExactSizeIterator<Item = (LanguageItem, &ExternalDefId)> + '_ {
        self.items
            .iter()
            .map(|(&item, declaration)| (item, declaration))
    }
}

/// Canonical declaration identities used by semantic analysis.
#[derive(Default)]
pub struct LanguageItems {
    items: BTreeMap<LanguageItem, CanonicalTarget>,
}

impl LanguageItems {
    pub(crate) fn from_trusted(environment: Option<&TrustedLanguageItemEnvironment>) -> Self {
        let items = environment.map_or_else(BTreeMap::new, |environment| {
            environment
                .items()
                .map(|(item, declaration)| (item, CanonicalTarget::External(declaration.clone())))
                .collect()
        });
        Self { items }
    }

    #[must_use]
    pub fn declaration(&self, item: LanguageItem) -> Option<&CanonicalTarget> {
        self.items.get(&item)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedLanguageItemError {
    DuplicateItem(LanguageItem),
    DuplicateDeclaration(ExternalDefId),
}

impl fmt::Display for TrustedLanguageItemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateItem(item) => {
                write!(
                    formatter,
                    "trusted language item {item:?} is assigned twice"
                )
            }
            Self::DuplicateDeclaration(declaration) => write!(
                formatter,
                "trusted declaration {} has multiple language-item roles",
                declaration.as_str()
            ),
        }
    }
}

impl std::error::Error for TrustedLanguageItemError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn external(name: &str) -> ExternalDefId {
        ExternalDefId::new(name).expect("test language-item identity is valid")
    }

    #[test]
    fn trusted_environment_rejects_duplicate_roles() {
        let error = TrustedLanguageItemEnvironment::new([
            (LanguageItem::OptSome, external("std::opt::Some")),
            (LanguageItem::OptSome, external("std::other::Some")),
        ])
        .err()
        .expect("duplicate semantic roles must be rejected");

        assert_eq!(
            error,
            TrustedLanguageItemError::DuplicateItem(LanguageItem::OptSome)
        );
    }

    #[test]
    fn trusted_environment_rejects_one_declaration_with_multiple_roles() {
        let constructor = external("std::opt::Constructor");
        let error = TrustedLanguageItemEnvironment::new([
            (LanguageItem::OptSome, constructor.clone()),
            (LanguageItem::OptNone, constructor.clone()),
        ])
        .err()
        .expect("one declaration cannot satisfy multiple semantic roles");

        assert_eq!(
            error,
            TrustedLanguageItemError::DuplicateDeclaration(constructor)
        );
    }
}
