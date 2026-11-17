use crate::OriginId;

/// Reserved value-constructor spellings, not a restriction on binding names.
#[must_use]
pub fn is_residual_constructor_name(name: &str) -> bool {
    matches!(name, "Some" | "None" | "Ok" | "Err")
}

/// Source name spelling and effect marker.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Name {
    text: Box<str>,
    bang: bool,
    valid: bool,
    origin: OriginId,
}

impl Name {
    pub(crate) fn valid(text: Box<str>, bang: bool, origin: OriginId) -> Self {
        Self {
            text,
            bang,
            valid: true,
            origin,
        }
    }

    pub(crate) fn invalid(origin: OriginId) -> Self {
        Self {
            text: "".into(),
            bang: false,
            valid: false,
            origin,
        }
    }

    /// Returns the marker-free name when this occurrence is structurally valid.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.valid.then_some(&self.text)
    }

    /// Reports whether the written identifier carries the effect marker.
    #[must_use]
    pub const fn is_bang(&self) -> bool {
        self.bang
    }

    /// Reports whether a physical identifier was present.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        self.valid
    }

    /// Returns the source origin of this occurrence.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// One qualified source path.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Path {
    segments: Box<[Name]>,
    origin: OriginId,
}

impl Path {
    pub(crate) fn new(segments: Box<[Name]>, origin: OriginId) -> Self {
        Self { segments, origin }
    }

    /// Returns path segments in source order.
    #[must_use]
    pub fn segments(&self) -> &[Name] {
        &self.segments
    }

    /// Returns the exact `::`-joined spelling when every segment is valid.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        self.segments
            .iter()
            .map(Name::text)
            .collect::<Option<Vec<_>>>()
            .map(|segments| segments.join("::"))
    }

    /// Returns the final segment when present.
    #[must_use]
    pub fn last(&self) -> Option<&Name> {
        self.segments.last()
    }

    /// Reports whether the path contains more than one segment.
    #[must_use]
    pub fn is_qualified(&self) -> bool {
        self.segments.len() > 1
    }

    /// Identifies heads whose match-pattern role is independent of name lookup.
    #[must_use]
    pub fn is_unqualified_residual_constructor(&self) -> bool {
        let [name] = self.segments.as_ref() else {
            return false;
        };
        !name.is_bang() && name.text().is_some_and(is_residual_constructor_name)
    }

    /// Returns the complete written path origin.
    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

/// Source literal family, including structurally invalid recovery occurrences.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LiteralKind {
    Integer,
    Float,
    String,
    FormatString,
    Bool,
    AxisName,
    AxisIndex,
    Invalid,
}

/// Literal spelling for semantic decoding, including a directly applied integer minus.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Literal {
    kind: LiteralKind,
    spelling: Box<str>,
    origin: OriginId,
}

impl Literal {
    pub(crate) fn new(kind: LiteralKind, spelling: Box<str>, origin: OriginId) -> Self {
        Self {
            kind,
            spelling,
            origin,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> LiteralKind {
        self.kind
    }

    #[must_use]
    pub fn spelling(&self) -> &str {
        &self.spelling
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Mutability {
    Immutable,
    Mutable,
}

#[cfg(test)]
mod tests {
    use super::{Name, Path};
    use crate::OriginId;

    #[test]
    fn path_text_requires_every_segment_to_be_valid() {
        let origin = OriginId::from_index(0);
        let valid = Path::new(
            Box::new([
                Name::valid("app".into(), false, origin),
                Name::valid("main".into(), false, origin),
            ]),
            origin,
        );
        assert_eq!(valid.text().as_deref(), Some("app::main"));

        let invalid = Path::new(
            Box::new([
                Name::valid("app".into(), false, origin),
                Name::invalid(origin),
            ]),
            origin,
        );
        assert!(invalid.text().is_none());
    }
}
