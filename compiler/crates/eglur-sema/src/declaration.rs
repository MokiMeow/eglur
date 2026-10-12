use std::collections::BTreeMap;
use std::fmt;

use eglur_diagnostics::{DiagnosticStoreError, Poison};
use eglur_hir::{
    EnumVariantPayload, HirFile, ImportSource, ItemId, ItemKind, Name, OriginId, Visibility,
};
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    ResolveCapability,
};
use crate::{
    DefId, IdentityOverflow, ImportId, ModuleAddress, ModuleId, NameKey, Namespace,
    SemanticLocation, SemanticSource,
};

#[cfg(test)]
pub(crate) mod tests;

/// Declaration role retained independently from later canonical type facts.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DeclarationKind {
    Function,
    ExternFunction,
    IntrinsicConstructor,
    Trait,
    Impl,
    Struct,
    Enum,
    OpaqueType,
    EnumVariant,
    EnumVariantField,
    TypeAlias,
    Const,
    AssociatedType,
    AssociatedTypeBinding,
    StorageField,
    Property,
    GenericParameter,
    PrimitiveType,
    IntrinsicType,
}

impl DeclarationKind {
    pub(crate) const fn is_variant_constructor(self) -> bool {
        matches!(self, Self::EnumVariant | Self::IntrinsicConstructor)
    }

    #[must_use]
    pub const fn namespace(self) -> Option<Namespace> {
        match self {
            Self::Function | Self::ExternFunction | Self::IntrinsicConstructor | Self::Const => {
                Some(Namespace::Value)
            }
            Self::Trait
            | Self::Struct
            | Self::Enum
            | Self::OpaqueType
            | Self::TypeAlias
            | Self::PrimitiveType
            | Self::IntrinsicType => Some(Namespace::Type),
            Self::Impl
            | Self::EnumVariant
            | Self::EnumVariantField
            | Self::AssociatedType
            | Self::AssociatedTypeBinding
            | Self::StorageField
            | Self::Property
            | Self::GenericParameter => None,
        }
    }

    /// Reports whether declarations of this kind may form an overload family.
    #[must_use]
    pub const fn permits_overload_family(self) -> bool {
        matches!(self, Self::Function)
    }
}

/// Visibility fact copied from one source declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DeclarationVisibility {
    Private,
    Public,
}

impl DeclarationVisibility {
    #[must_use]
    pub const fn is_public(self) -> bool {
        matches!(self, Self::Public)
    }
}

impl From<Visibility> for DeclarationVisibility {
    fn from(value: Visibility) -> Self {
        match value {
            Visibility::Private => Self::Private,
            Visibility::Public => Self::Public,
        }
    }
}

/// Canonical graph-local source declaration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Declaration {
    id: DefId,
    parent: Option<DefId>,
    name: Option<NameKey>,
    kind: DeclarationKind,
    visibility: DeclarationVisibility,
    overload: bool,
    origin: OriginId,
    item: Option<ItemId>,
}

impl Declaration {
    #[must_use]
    pub const fn id(&self) -> DefId {
        self.id
    }

    #[must_use]
    pub const fn parent(&self) -> Option<DefId> {
        self.parent
    }

    #[must_use]
    pub const fn name(&self) -> Option<&NameKey> {
        self.name.as_ref()
    }

    #[must_use]
    pub const fn kind(&self) -> DeclarationKind {
        self.kind
    }

    #[must_use]
    pub const fn visibility(&self) -> DeclarationVisibility {
        self.visibility
    }

    #[must_use]
    pub const fn is_overload(&self) -> bool {
        self.overload
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn item(&self) -> Option<ItemId> {
        self.item
    }
}

/// One source import binding request before target resolution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportRecord {
    id: ImportId,
    item: ItemId,
    target: Box<[NameKey]>,
    alias: Option<NameKey>,
    visibility: DeclarationVisibility,
    origin: OriginId,
    alias_origin: Option<OriginId>,
}

impl ImportRecord {
    #[must_use]
    pub const fn id(&self) -> ImportId {
        self.id
    }

    #[must_use]
    pub const fn item(&self) -> ItemId {
        self.item
    }

    #[must_use]
    pub fn target(&self) -> &[NameKey] {
        &self.target
    }

    #[must_use]
    pub const fn alias(&self) -> Option<&NameKey> {
        self.alias.as_ref()
    }

    #[must_use]
    pub const fn visibility(&self) -> DeclarationVisibility {
        self.visibility
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origin
    }

    #[must_use]
    pub const fn alias_origin(&self) -> Option<OriginId> {
        self.alias_origin
    }
}

/// Complete declaration and import inventory for one canonical module.
pub struct ModuleDeclarations {
    id: ModuleId,
    file: FileId,
    address: ModuleAddress,
    declarations: Vec<Declaration>,
    item_defs: BTreeMap<ItemId, DefId>,
    module_bindings: BTreeMap<(Namespace, NameKey), Vec<DefId>>,
    direct_declarations: BTreeMap<(Option<DefId>, DeclarationKind, Option<NameKey>), Vec<DefId>>,
    children_by_parent: BTreeMap<DefId, Vec<DefId>>,
    children_by_kind: BTreeMap<(DefId, DeclarationKind), Vec<DefId>>,
    source_generic_indices: BTreeMap<DefId, usize>,
    imports: Vec<ImportRecord>,
    declaration_poison: Option<Poison>,
    import_poison: Option<Poison>,
    owner_poisons: BTreeMap<DefId, Poison>,
}

impl ModuleDeclarations {
    #[must_use]
    pub const fn id(&self) -> ModuleId {
        self.id
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.file
    }

    #[must_use]
    pub const fn address(&self) -> &ModuleAddress {
        &self.address
    }

    pub fn declarations(&self) -> impl ExactSizeIterator<Item = &Declaration> {
        #[cfg(test)]
        let declarations = self
            .declarations
            .iter()
            .inspect(|_| tests::observe_declaration_visit());
        #[cfg(not(test))]
        let declarations = self.declarations.iter();

        declarations
    }

    #[must_use]
    pub fn declaration(&self, id: DefId) -> Option<&Declaration> {
        (id.module() == self.id).then(|| self.declarations.get(id.index()))?
    }

    #[must_use]
    pub fn item_def(&self, item: ItemId) -> Option<DefId> {
        self.item_defs.get(&item).copied()
    }

    #[must_use]
    pub fn candidates(&self, namespace: Namespace, name: &NameKey) -> &[DefId] {
        self.module_bindings
            .get(&(namespace, name.clone()))
            .map_or(&[], Vec::as_slice)
    }

    pub(crate) fn direct_candidates(
        &self,
        parent: Option<DefId>,
        name: Option<&NameKey>,
        kind: DeclarationKind,
    ) -> &[DefId] {
        self.direct_declarations
            .get(&(parent, kind, name.cloned()))
            .map_or(&[], Vec::as_slice)
    }

    /// Immediate children in allocation order, without visibility or role filtering.
    pub(crate) fn children(&self, parent: DefId) -> impl ExactSizeIterator<Item = &Declaration> {
        #[cfg(test)]
        tests::observe_child_query();

        let children = self
            .children_by_parent
            .get(&parent)
            .map_or(&[][..], Vec::as_slice);
        #[cfg(test)]
        let children = children.iter().inspect(|_| tests::observe_child_visit());
        #[cfg(not(test))]
        let children = children.iter();

        children.map(|id| &self.declarations[id.index()])
    }

    pub(crate) fn child_ids(&self, parent: DefId, kind: DeclarationKind) -> &[DefId] {
        #[cfg(test)]
        tests::observe_role_query();

        self.children_by_kind
            .get(&(parent, kind))
            .map_or(&[], Vec::as_slice)
    }

    /// Position in the owner's HIR generic list, independent of name validity.
    pub(crate) fn source_generic_index(&self, generic: DefId) -> Option<usize> {
        #[cfg(test)]
        tests::observe_generic_index_query();

        self.source_generic_indices.get(&generic).copied()
    }

    pub fn imports(&self) -> impl ExactSizeIterator<Item = &ImportRecord> {
        self.imports.iter()
    }

    #[must_use]
    pub const fn declaration_poison(&self) -> Option<&Poison> {
        self.declaration_poison.as_ref()
    }

    #[must_use]
    pub const fn import_poison(&self) -> Option<&Poison> {
        self.import_poison.as_ref()
    }

    #[must_use]
    pub fn owner_poison(&self, owner: DefId) -> Option<&Poison> {
        self.owner_poisons.get(&owner)
    }
}

/// Canonical declaration graph, independent of source enumeration order.
pub struct DeclarationCollection {
    modules: Box<[ModuleDeclarations]>,
    module_by_file: BTreeMap<FileId, ModuleId>,
    module_by_address: BTreeMap<ModuleAddress, ModuleId>,
}

impl DeclarationCollection {
    pub fn modules(&self) -> impl ExactSizeIterator<Item = &ModuleDeclarations> {
        self.modules.iter()
    }

    #[must_use]
    pub fn module(&self, id: ModuleId) -> Option<&ModuleDeclarations> {
        self.modules
            .get(id.index())
            .filter(|module| module.id == id)
    }

    #[must_use]
    pub fn module_for_file(&self, file: FileId) -> Option<ModuleId> {
        self.module_by_file.get(&file).copied()
    }

    #[must_use]
    pub fn module_for_address(&self, address: &ModuleAddress) -> Option<ModuleId> {
        self.module_by_address.get(address).copied()
    }

    #[must_use]
    pub fn declaration(&self, id: DefId) -> Option<&Declaration> {
        self.module(id.module())?.declaration(id)
    }

    pub(crate) fn has_externally_reachable_type_contract(&self, id: DefId) -> bool {
        let Some(declaration) = self.declaration(id) else {
            return false;
        };
        let parent = declaration
            .parent()
            .and_then(|parent| self.declaration(parent));
        let public_owner = parent.is_some_and(|parent| parent.visibility().is_public());
        match declaration.kind() {
            DeclarationKind::Function | DeclarationKind::ExternFunction => match parent {
                None => declaration.visibility().is_public(),
                Some(parent) if parent.kind() == DeclarationKind::Trait => public_owner,
                Some(parent) if parent.kind() == DeclarationKind::Struct => {
                    public_owner && declaration.visibility().is_public()
                }
                Some(_) => false,
            },
            DeclarationKind::Trait
            | DeclarationKind::Struct
            | DeclarationKind::Enum
            | DeclarationKind::TypeAlias
            | DeclarationKind::Const => parent.is_none() && declaration.visibility().is_public(),
            DeclarationKind::StorageField | DeclarationKind::Property => {
                parent.is_some_and(|parent| parent.kind() == DeclarationKind::Struct)
                    && public_owner
                    && declaration.visibility().is_public()
            }
            DeclarationKind::EnumVariant => {
                parent.is_some_and(|parent| parent.kind() == DeclarationKind::Enum) && public_owner
            }
            DeclarationKind::EnumVariantField => parent
                .and_then(Declaration::parent)
                .and_then(|owner| self.declaration(owner))
                .is_some_and(|owner| {
                    owner.kind() == DeclarationKind::Enum && owner.visibility().is_public()
                }),
            DeclarationKind::Impl
            | DeclarationKind::PrimitiveType
            | DeclarationKind::IntrinsicType
            | DeclarationKind::OpaqueType
            | DeclarationKind::IntrinsicConstructor
            | DeclarationKind::AssociatedType
            | DeclarationKind::AssociatedTypeBinding
            | DeclarationKind::GenericParameter => false,
        }
    }

    /// Returns matching declarations directly owned by another declaration.
    #[must_use]
    pub fn child_candidates(
        &self,
        parent: DefId,
        name: &NameKey,
        kind: DeclarationKind,
    ) -> Vec<DefId> {
        self.module(parent.module())
            .map_or_else(Vec::new, |module| {
                module
                    .direct_candidates(Some(parent), Some(name), kind)
                    .to_vec()
            })
    }

    #[must_use]
    pub fn declaration_count(&self) -> usize {
        self.modules
            .iter()
            .map(|module| module.declarations.len())
            .sum()
    }

    #[must_use]
    pub fn import_count(&self) -> usize {
        self.modules.iter().map(|module| module.imports.len()).sum()
    }

    pub(crate) fn has_poisoned_declarations(&self) -> bool {
        self.modules.iter().any(|module| {
            module.declaration_poison.is_some()
                || module.import_poison.is_some()
                || !module.owner_poisons.is_empty()
        })
    }
}

/// Collects graph-local module, declaration, member, and import identities.
pub fn collect_declarations(
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
) -> Result<PhaseAnalysis<DeclarationCollection>, CollectionError> {
    let mut recorder = DemandRecorder::new(support);
    let mut ordered = sources.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|source| source.file());
    let mut supported = true;
    for source in &ordered {
        supported &= recorder.require(
            CompileCapability::Resolve(ResolveCapability::CollectDeclarations),
            SemanticLocation::source(source.file()),
        );
    }
    if !supported {
        return Ok(recorder.finish(PhaseProduct::Unperformed));
    }

    for pair in ordered.windows(2) {
        if pair[0].file() == pair[1].file() {
            return Err(CollectionError::DuplicateFile(pair[0].file()));
        }
    }

    let mut modules = Vec::with_capacity(ordered.len());
    let mut module_by_file = BTreeMap::new();
    let mut module_by_address = BTreeMap::new();
    for source in ordered {
        let id = ModuleId::from_index(modules.len())?;
        if module_by_address
            .insert(source.module().clone(), id)
            .is_some()
        {
            return Err(CollectionError::DuplicateModule(source.module().clone()));
        }
        module_by_file.insert(source.file(), id);
        let mut module = ModuleDeclarations {
            id,
            file: source.file(),
            address: source.module().clone(),
            declarations: Vec::new(),
            item_defs: BTreeMap::new(),
            module_bindings: BTreeMap::new(),
            direct_declarations: BTreeMap::new(),
            children_by_parent: BTreeMap::new(),
            children_by_kind: BTreeMap::new(),
            source_generic_indices: BTreeMap::new(),
            imports: Vec::new(),
            declaration_poison: source.source_poison().cloned(),
            import_poison: source.source_poison().cloned(),
            owner_poisons: BTreeMap::new(),
        };
        if let Some(hir) = source.hir() {
            for &item in hir.roots() {
                collect_item(source, hir, &mut module, item, None)?;
            }
        }
        modules.push(module);
    }

    Ok(
        recorder.finish(PhaseProduct::Performed(DeclarationCollection {
            modules: modules.into_boxed_slice(),
            module_by_file,
            module_by_address,
        })),
    )
}

fn collect_item(
    source: &SemanticSource<'_>,
    hir: &HirFile,
    module: &mut ModuleDeclarations,
    item_id: ItemId,
    parent: Option<DefId>,
) -> Result<Option<DefId>, CollectionError> {
    let item = hir.item(item_id);
    match item.kind() {
        ItemKind::Import(import) => {
            if parent.is_none() {
                collect_import(source, module, item_id, item.visibility(), import)?;
            }
            Ok(None)
        }
        ItemKind::Invalid => {
            let poison = required_poison(source, item.origin())?;
            if parent.is_none() {
                merge_poison(&mut module.declaration_poison, poison)?;
                merge_poison(&mut module.import_poison, poison)?;
            } else if let Some(parent) = parent {
                merge_owner_poison(&mut module.owner_poisons, parent, poison)?;
            }
            Ok(None)
        }
        kind => {
            let (name, declaration_kind) = item_name_and_kind(kind);
            let name = name
                .map(|name| collect_name(source, module, parent, name))
                .transpose()?
                .flatten();
            let declaration = allocate_declaration(
                module,
                parent,
                name,
                declaration_kind,
                item.visibility().into(),
                item.origin(),
                Some(item_id),
            )?;
            if let ItemKind::Function(function) = kind {
                module.declarations[declaration.index()].overload = function.overload;
            }
            module.item_defs.insert(item_id, declaration);
            collect_item_children(source, hir, module, item_id, declaration)?;
            Ok(Some(declaration))
        }
    }
}

fn item_name_and_kind(kind: &ItemKind) -> (Option<&Name>, DeclarationKind) {
    match kind {
        ItemKind::Function(value) => (Some(&value.name), DeclarationKind::Function),
        ItemKind::Extern(value) => (Some(&value.name), DeclarationKind::ExternFunction),
        ItemKind::Trait(value) => (Some(&value.name), DeclarationKind::Trait),
        ItemKind::Impl(_) => (None, DeclarationKind::Impl),
        ItemKind::Struct(value) => (Some(&value.name), DeclarationKind::Struct),
        ItemKind::Enum(value) => (Some(&value.name), DeclarationKind::Enum),
        ItemKind::TypeAlias(value) => (Some(&value.name), DeclarationKind::TypeAlias),
        ItemKind::Const(value) => (Some(&value.name), DeclarationKind::Const),
        ItemKind::AssociatedType(value) => (Some(&value.name), DeclarationKind::AssociatedType),
        ItemKind::AssociatedTypeBinding(value) => {
            (Some(&value.name), DeclarationKind::AssociatedTypeBinding)
        }
        ItemKind::StorageField(value) => (Some(&value.name), DeclarationKind::StorageField),
        ItemKind::Property(value) => (Some(&value.name), DeclarationKind::Property),
        ItemKind::Import(_) | ItemKind::Invalid => unreachable!("handled before classification"),
    }
}

fn collect_item_children(
    source: &SemanticSource<'_>,
    hir: &HirFile,
    module: &mut ModuleDeclarations,
    item_id: ItemId,
    owner: DefId,
) -> Result<(), CollectionError> {
    match hir.item(item_id).kind() {
        ItemKind::Function(value) => {
            collect_generics(source, module, owner, &value.generic_parameters)?;
        }
        ItemKind::Trait(value) => {
            collect_generics(source, module, owner, &value.generic_parameters)?;
            for &item in &value.items {
                collect_item(source, hir, module, item, Some(owner))?;
            }
        }
        ItemKind::Impl(value) => {
            collect_generics(source, module, owner, &value.generic_parameters)?;
            for &item in &value.items {
                collect_item(source, hir, module, item, Some(owner))?;
            }
        }
        ItemKind::Struct(value) => {
            collect_generics(source, module, owner, &value.generic_parameters)?;
            for &item in &value.items {
                collect_item(source, hir, module, item, Some(owner))?;
            }
        }
        ItemKind::Enum(value) => {
            collect_generics(source, module, owner, &value.generic_parameters)?;
            for variant in &value.variants {
                let name = collect_name(source, module, Some(owner), &variant.name)?;
                let variant_id = allocate_declaration(
                    module,
                    Some(owner),
                    name,
                    DeclarationKind::EnumVariant,
                    variant.visibility.into(),
                    variant.origin,
                    None,
                )?;
                if let EnumVariantPayload::Named(fields) = &variant.payload {
                    for field in fields {
                        let name = collect_name(source, module, Some(variant_id), &field.name)?;
                        allocate_declaration(
                            module,
                            Some(variant_id),
                            name,
                            DeclarationKind::EnumVariantField,
                            DeclarationVisibility::Private,
                            field.origin,
                            None,
                        )?;
                    }
                }
            }
        }
        ItemKind::TypeAlias(value) => {
            collect_generics(source, module, owner, &value.generic_parameters)?;
        }
        ItemKind::Property(value) => {
            for &item in &value.accessors {
                collect_item(source, hir, module, item, Some(owner))?;
            }
        }
        ItemKind::Extern(_)
        | ItemKind::Const(_)
        | ItemKind::AssociatedType(_)
        | ItemKind::AssociatedTypeBinding(_)
        | ItemKind::StorageField(_) => {}
        ItemKind::Import(_) | ItemKind::Invalid => unreachable!("not allocated as a declaration"),
    }
    Ok(())
}

fn collect_generics(
    source: &SemanticSource<'_>,
    module: &mut ModuleDeclarations,
    owner: DefId,
    parameters: &[eglur_hir::GenericParameter],
) -> Result<(), CollectionError> {
    for (index, parameter) in parameters.iter().enumerate() {
        let name = collect_name(source, module, Some(owner), &parameter.name)?;
        let generic = allocate_declaration(
            module,
            Some(owner),
            name,
            DeclarationKind::GenericParameter,
            DeclarationVisibility::Private,
            parameter.origin,
            None,
        )?;
        #[cfg(test)]
        tests::observe_generic_index_insertion();
        module.source_generic_indices.insert(generic, index);
    }
    Ok(())
}

fn allocate_declaration(
    module: &mut ModuleDeclarations,
    parent: Option<DefId>,
    name: Option<NameKey>,
    kind: DeclarationKind,
    visibility: DeclarationVisibility,
    origin: OriginId,
    item: Option<ItemId>,
) -> Result<DefId, CollectionError> {
    let id = DefId::from_index(module.id, module.declarations.len())?;
    module.declarations.push(Declaration {
        id,
        parent,
        name: name.clone(),
        kind,
        visibility,
        overload: false,
        origin,
        item,
    });
    module
        .direct_declarations
        .entry((parent, kind, name.clone()))
        .or_default()
        .push(id);
    if let Some(parent) = parent {
        #[cfg(test)]
        tests::observe_child_insertion();
        module
            .children_by_parent
            .entry(parent)
            .or_default()
            .push(id);
        #[cfg(test)]
        tests::observe_role_insertion();
        module
            .children_by_kind
            .entry((parent, kind))
            .or_default()
            .push(id);
    }
    if parent.is_none()
        && let (Some(name), Some(namespace)) = (name, kind.namespace())
    {
        module
            .module_bindings
            .entry((namespace, name))
            .or_default()
            .push(id);
    }
    Ok(id)
}

fn collect_name(
    source: &SemanticSource<'_>,
    module: &mut ModuleDeclarations,
    parent: Option<DefId>,
    name: &Name,
) -> Result<Option<NameKey>, CollectionError> {
    if let Some(name) = NameKey::from_hir(name) {
        return Ok(Some(name));
    }
    let poison = required_poison(source, name.origin())?;
    if parent.is_none() {
        merge_poison(&mut module.declaration_poison, poison)?;
    } else if let Some(parent) = parent {
        merge_owner_poison(&mut module.owner_poisons, parent, poison)?;
    }
    Ok(None)
}

fn collect_import(
    source: &SemanticSource<'_>,
    module: &mut ModuleDeclarations,
    item: ItemId,
    visibility: Visibility,
    import: &eglur_hir::ImportDecl,
) -> Result<(), CollectionError> {
    match &import.source {
        ImportSource::Single { path, alias } => {
            let Some(target) = collect_path(source, &mut module.import_poison, path)? else {
                return Ok(());
            };
            let alias_origin = alias.as_ref().map(Name::origin);
            let alias = alias
                .as_ref()
                .map(|name| collect_import_name(source, &mut module.import_poison, name))
                .transpose()?
                .flatten();
            let origin = alias_origin.unwrap_or(path.origin());
            push_import(
                module,
                item,
                target,
                alias,
                visibility.into(),
                origin,
                alias_origin,
            )?;
        }
        ImportSource::Group { prefix, entries } => {
            let Some(prefix) = collect_path(source, &mut module.import_poison, prefix)? else {
                return Ok(());
            };
            for entry in entries {
                let Some(name) =
                    collect_import_name(source, &mut module.import_poison, &entry.name)?
                else {
                    continue;
                };
                let alias = entry
                    .alias
                    .as_ref()
                    .map(|name| collect_import_name(source, &mut module.import_poison, name))
                    .transpose()?
                    .flatten();
                let mut target = prefix.clone();
                target.push(name);
                push_import(
                    module,
                    item,
                    target,
                    alias.clone(),
                    visibility.into(),
                    entry.origin,
                    entry.alias.as_ref().map(Name::origin),
                )?;
            }
        }
    }
    Ok(())
}

fn collect_path(
    source: &SemanticSource<'_>,
    poison: &mut Option<Poison>,
    path: &eglur_hir::Path,
) -> Result<Option<Vec<NameKey>>, CollectionError> {
    let mut collected = Vec::with_capacity(path.segments().len());
    for segment in path.segments() {
        let Some(segment) = NameKey::from_hir(segment) else {
            merge_poison(poison, required_poison(source, segment.origin())?)?;
            return Ok(None);
        };
        collected.push(segment);
    }
    if collected.is_empty() {
        merge_poison(poison, required_poison(source, path.origin())?)?;
        return Ok(None);
    }
    Ok(Some(collected))
}

fn collect_import_name(
    source: &SemanticSource<'_>,
    poison: &mut Option<Poison>,
    name: &Name,
) -> Result<Option<NameKey>, CollectionError> {
    if let Some(name) = NameKey::from_hir(name) {
        Ok(Some(name))
    } else {
        merge_poison(poison, required_poison(source, name.origin())?)?;
        Ok(None)
    }
}

fn push_import(
    module: &mut ModuleDeclarations,
    item: ItemId,
    target: Vec<NameKey>,
    alias: Option<NameKey>,
    visibility: DeclarationVisibility,
    origin: OriginId,
    alias_origin: Option<OriginId>,
) -> Result<(), CollectionError> {
    let id = ImportId::from_index(module.id, module.imports.len())?;
    module.imports.push(ImportRecord {
        id,
        item,
        target: target.into_boxed_slice(),
        alias,
        visibility,
        origin,
        alias_origin,
    });
    Ok(())
}

fn required_poison<'source>(
    source: &'source SemanticSource<'_>,
    origin: OriginId,
) -> Result<&'source Poison, CollectionError> {
    source
        .invalid_poison(origin)
        .ok_or(CollectionError::MissingInvalidPoison {
            file: source.file(),
            origin,
        })
}

fn merge_poison(slot: &mut Option<Poison>, poison: &Poison) -> Result<(), CollectionError> {
    *slot = Some(match slot {
        Some(existing) => existing.merge(poison)?,
        None => poison.clone(),
    });
    Ok(())
}

fn merge_owner_poison(
    poisons: &mut BTreeMap<DefId, Poison>,
    owner: DefId,
    poison: &Poison,
) -> Result<(), CollectionError> {
    if let Some(existing) = poisons.get(&owner) {
        poisons.insert(owner, existing.merge(poison)?);
    } else {
        poisons.insert(owner, poison.clone());
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CollectionError {
    DuplicateFile(FileId),
    DuplicateModule(ModuleAddress),
    MissingInvalidPoison { file: FileId, origin: OriginId },
    IdentityOverflow(IdentityOverflow),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for CollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateFile(file) => write!(formatter, "duplicate semantic source {file}"),
            Self::DuplicateModule(module) => {
                write!(formatter, "duplicate canonical module {module}")
            }
            Self::MissingInvalidPoison { file, origin } => write!(
                formatter,
                "invalid HIR origin {} in {file} has no retained syntax poison",
                origin.get()
            ),
            Self::IdentityOverflow(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CollectionError {}

impl From<IdentityOverflow> for CollectionError {
    fn from(error: IdentityOverflow) -> Self {
        Self::IdentityOverflow(error)
    }
}

impl From<DiagnosticStoreError> for CollectionError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}
