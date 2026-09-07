use std::fmt;

use eglur_hir::{BodyId, Name, OriginId};

macro_rules! graph_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            pub(crate) fn from_index(index: usize) -> Result<Self, IdentityOverflow> {
                u32::try_from(index)
                    .map(Self)
                    .map_err(|_| IdentityOverflow::new(stringify!($name), index))
            }

            /// Returns the graph-local integer projection.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }

            pub(crate) const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

graph_id!(ModuleId);
graph_id!(TypeId);
graph_id!(SubstId);
graph_id!(CallableSignatureId);
graph_id!(CallableInstanceId);
graph_id!(EvidenceEnvironmentId);

/// One declaration identity within a compilation graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DefId {
    module: ModuleId,
    local: u32,
}

impl DefId {
    pub(crate) fn from_index(module: ModuleId, index: usize) -> Result<Self, IdentityOverflow> {
        Ok(Self {
            module,
            local: u32::try_from(index).map_err(|_| IdentityOverflow::new("DefId", index))?,
        })
    }

    /// Returns the module that owns this declaration.
    #[must_use]
    pub const fn module(self) -> ModuleId {
        self.module
    }

    /// Returns the declaration's module-local integer projection.
    #[must_use]
    pub const fn local(self) -> u32 {
        self.local
    }

    pub(crate) const fn index(self) -> usize {
        self.local as usize
    }
}

/// One lexical scope identity within a semantic body.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScopeId {
    module: ModuleId,
    body: BodyId,
    local: u32,
}

impl ScopeId {
    pub(crate) fn from_index(
        module: ModuleId,
        body: BodyId,
        index: usize,
    ) -> Result<Self, IdentityOverflow> {
        Ok(Self {
            module,
            body,
            local: u32::try_from(index).map_err(|_| IdentityOverflow::new("ScopeId", index))?,
        })
    }

    #[must_use]
    pub const fn module(self) -> ModuleId {
        self.module
    }

    #[must_use]
    pub const fn body(self) -> BodyId {
        self.body
    }

    #[must_use]
    pub const fn local(self) -> u32 {
        self.local
    }
}

/// One local value binding independent of its source spelling.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalBindingId {
    module: ModuleId,
    body: BodyId,
    local: u32,
}

/// One declaration-local trait witness consumed by checked generic dispatch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WitnessSlotId {
    owner: DefId,
    local: u32,
}

impl WitnessSlotId {
    pub(crate) fn from_index(owner: DefId, index: usize) -> Result<Self, IdentityOverflow> {
        Ok(Self {
            owner,
            local: u32::try_from(index)
                .map_err(|_| IdentityOverflow::new("WitnessSlotId", index))?,
        })
    }

    /// Returns the declaration whose checked environment owns this slot.
    #[must_use]
    pub const fn owner(self) -> DefId {
        self.owner
    }

    /// Returns the owner-local integer projection.
    #[must_use]
    pub const fn local(self) -> u32 {
        self.local
    }
}

impl LocalBindingId {
    pub(crate) fn from_index(
        module: ModuleId,
        body: BodyId,
        index: usize,
    ) -> Result<Self, IdentityOverflow> {
        Ok(Self {
            module,
            body,
            local: u32::try_from(index)
                .map_err(|_| IdentityOverflow::new("LocalBindingId", index))?,
        })
    }

    #[must_use]
    pub const fn module(self) -> ModuleId {
        self.module
    }

    #[must_use]
    pub const fn body(self) -> BodyId {
        self.body
    }

    #[must_use]
    pub const fn local(self) -> u32 {
        self.local
    }
}

/// One source import entry, including entries expanded from a group import.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ImportId {
    module: ModuleId,
    local: u32,
}

impl ImportId {
    pub(crate) fn from_index(module: ModuleId, index: usize) -> Result<Self, IdentityOverflow> {
        Ok(Self {
            module,
            local: u32::try_from(index).map_err(|_| IdentityOverflow::new("ImportId", index))?,
        })
    }

    #[must_use]
    pub const fn module(self) -> ModuleId {
        self.module
    }

    #[must_use]
    pub const fn local(self) -> u32 {
        self.local
    }
}

/// Exact identifier spelling, including effect-marker identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NameKey {
    text: Box<str>,
    bang: bool,
}

impl NameKey {
    /// Creates one canonical name key from a marker-free non-empty spelling.
    #[must_use]
    pub fn new(text: impl Into<Box<str>>, bang: bool) -> Option<Self> {
        let text = text.into();
        (!text.is_empty() && !text.contains('!')).then_some(Self { text, bang })
    }

    pub(crate) fn from_hir(name: &Name) -> Option<Self> {
        Some(Self {
            text: name.text()?.into(),
            bang: name.is_bang(),
        })
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub const fn is_bang(&self) -> bool {
        self.bang
    }

    /// Returns the complete source spelling, including `!` when present.
    #[must_use]
    pub fn spelling(&self) -> String {
        if self.bang {
            format!("{}!", self.text)
        } else {
            self.text.to_string()
        }
    }
}

impl fmt::Display for NameKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.text.fmt(formatter)?;
        if self.bang {
            formatter.write_str("!")?;
        }
        Ok(())
    }
}

/// Independent declaration lookup namespaces.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Namespace {
    Value,
    Type,
}

/// Canonical module path copied from the driver-owned source mapping.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ModuleAddress {
    components: Box<[Box<str>]>,
    text: Box<str>,
}

impl ModuleAddress {
    /// Creates a non-empty canonical module address.
    pub fn new(components: impl IntoIterator<Item = Box<str>>) -> Result<Self, ModuleAddressError> {
        let components = components.into_iter().collect::<Vec<_>>();
        if components.is_empty()
            || components
                .iter()
                .any(|component| component.is_empty() || component.contains("::"))
        {
            return Err(ModuleAddressError);
        }
        let text = components
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&str>>()
            .join("::")
            .into_boxed_str();
        Ok(Self {
            components: components.into_boxed_slice(),
            text,
        })
    }

    #[must_use]
    pub fn components(&self) -> impl ExactSizeIterator<Item = &str> {
        self.components.iter().map(AsRef::as_ref)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for ModuleAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.text.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleAddressError;

impl fmt::Display for ModuleAddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("module address must contain non-empty delimiter-free components")
    }
}

impl std::error::Error for ModuleAddressError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityOverflow {
    kind: &'static str,
    count: usize,
}

impl IdentityOverflow {
    const fn new(kind: &'static str, count: usize) -> Self {
        Self { kind, count }
    }
}

impl fmt::Display for IdentityOverflow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} allocation count {} exceeds graph identity capacity",
            self.kind, self.count
        )
    }
}

impl std::error::Error for IdentityOverflow {}

/// Source occurrence used only as deterministic capability-demand provenance.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SemanticLocation {
    file: eglur_source::FileId,
    origin: Option<OriginId>,
}

impl SemanticLocation {
    #[must_use]
    pub const fn source(file: eglur_source::FileId) -> Self {
        Self { file, origin: None }
    }

    #[must_use]
    pub const fn origin(file: eglur_source::FileId, origin: OriginId) -> Self {
        Self {
            file,
            origin: Some(origin),
        }
    }

    #[must_use]
    pub const fn file(self) -> eglur_source::FileId {
        self.file
    }

    #[must_use]
    pub const fn hir_origin(self) -> Option<OriginId> {
        self.origin
    }
}
