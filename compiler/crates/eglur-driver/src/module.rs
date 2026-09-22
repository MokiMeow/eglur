use std::fmt;

use eglur_syntax::is_identifier_spelling;

/// Source-addressable canonical module path derived from one source location.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModulePath {
    components: Box<[Box<str>]>,
    text: Box<str>,
}

impl ModulePath {
    /// Returns canonical components in source order.
    pub fn components(&self) -> impl ExactSizeIterator<Item = &str> {
        self.components.iter().map(AsRef::as_ref)
    }

    /// Returns the canonical source spelling joined by `::`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.text.fmt(formatter)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ModulePathCandidate {
    components: Box<[Box<str>]>,
}

impl ModulePathCandidate {
    pub(crate) fn from_relative_source(path: &str) -> Self {
        let mut components = path.split('/').map(Box::<str>::from).collect::<Vec<_>>();
        let final_component = components
            .last_mut()
            .expect("canonical source path has at least one component");
        *final_component = final_component
            .strip_suffix(".eglur")
            .expect("candidate is created only for .eglur sources")
            .into();
        Self {
            components: components.into_boxed_slice(),
        }
    }

    pub(crate) fn has_reserved_root(&self) -> bool {
        self.components
            .first()
            .is_some_and(|root| root.as_ref() == "std")
    }

    pub(crate) fn first_invalid_component(&self) -> Option<(usize, &str)> {
        self.components
            .iter()
            .enumerate()
            .find(|(index, component)| !is_valid_component(*index, component))
            .map(|(index, component)| (index, component.as_ref()))
    }

    pub(crate) fn into_valid(self) -> ModulePath {
        debug_assert!(!self.has_reserved_root());
        debug_assert!(self.first_invalid_component().is_none());
        let text = self
            .components
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>()
            .join("::")
            .into_boxed_str();
        ModulePath {
            components: self.components,
            text,
        }
    }
}

fn is_valid_component(index: usize, component: &str) -> bool {
    is_identifier_spelling(component) && (index != 0 || !matches!(component, "self" | "super"))
}

#[cfg(test)]
mod tests {
    use super::ModulePathCandidate;

    #[test]
    fn module_components_preserve_spelling_and_source_addressability() {
        let valid = ModulePathCandidate::from_relative_source("closed/self.eglur");
        assert!(valid.first_invalid_component().is_none());
        assert_eq!(valid.into_valid().as_str(), "closed::self");

        for (path, expected) in [
            ("_/item.eglur", (0, "_")),
            ("self/item.eglur", (0, "self")),
            ("ok/bad-name.eglur", (1, "bad-name")),
            ("match/item.eglur", (0, "match")),
            ("delta/δ.eglur", (1, "δ")),
        ] {
            let candidate = ModulePathCandidate::from_relative_source(path);
            assert_eq!(candidate.first_invalid_component(), Some(expected));
        }
    }

    #[test]
    fn reserved_root_is_classified_before_later_invalid_components() {
        let candidate = ModulePathCandidate::from_relative_source("std/bad-name.eglur");
        assert!(candidate.has_reserved_root());
        assert_eq!(candidate.first_invalid_component(), Some((1, "bad-name")));
    }
}
