use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

mod traversal;

use crate::type_namespace::{
    TypeNameLookup, TypeNameLookupError, lookup_external_module_type, lookup_prelude_type,
    lookup_source_module_type,
};
use crate::{
    CallableSignature, CanonicalTarget, CanonicalTypeStore, DeclarationCollection, DeclarationKind,
    DefId, ExternalNameEnvironment, GenericParameterId, ModuleId, NameKey, Namespace, NominalHead,
    RawPointerMutability, ReferenceMutability, ResolvedImports, TraitRef,
    TrustedSignatureEnvironment, TrustedSignatureKind, TypeId, TypeKind, TypeStoreError,
};

pub(crate) trait DiagnosticTypeFacts {
    fn types(&self) -> &CanonicalTypeStore;
    fn generic_parameter_name(&self, parameter: &GenericParameterId) -> Option<&str>;
    fn generic_parameter_bounds(&self, parameter: &GenericParameterId) -> Option<&[TraitRef]>;
}

pub(crate) struct DiagnosticTypeNames {
    target_names: BTreeMap<CanonicalTarget, Box<str>>,
    generic_names_by_owner: BTreeMap<DefId, Box<[Box<str>]>>,
    source_bindings: BTreeMap<(ModuleId, CanonicalTarget), Box<[NameKey]>>,
    source_occupied: BTreeSet<(ModuleId, NameKey)>,
    prelude_bindings: BTreeMap<CanonicalTarget, Box<[NameKey]>>,
    prelude_blocked_modules: BTreeSet<ModuleId>,
    paths: BTreeMap<CanonicalTarget, Box<[TypePathCandidate]>>,
}

impl DiagnosticTypeNames {
    pub(crate) fn new(
        collection: &DeclarationCollection,
        imports: &ResolvedImports,
        external: Option<&ExternalNameEnvironment>,
        trusted: Option<&TrustedSignatureEnvironment>,
    ) -> Result<Self, TypeNameLookupError> {
        let mut target_names = BTreeMap::new();
        let mut generic_names_by_owner = BTreeMap::<DefId, Vec<Box<str>>>::new();
        let mut source_bindings = BTreeMap::<(ModuleId, CanonicalTarget), Vec<NameKey>>::new();
        let mut source_occupied = BTreeSet::new();
        let mut prelude_bindings = BTreeMap::<CanonicalTarget, Vec<NameKey>>::new();
        let mut prelude_blocked_modules = BTreeSet::new();
        let mut paths = BTreeMap::<CanonicalTarget, Vec<TypePathCandidate>>::new();

        for module in collection.modules() {
            if module.declaration_poison().is_some() || module.import_poison().is_some() {
                prelude_blocked_modules.insert(module.id());
            }
            for declaration in module.declarations() {
                if let Some(name) = declaration.name() {
                    target_names.insert(
                        CanonicalTarget::Source(declaration.id()),
                        Box::<str>::from(name.text()),
                    );
                    if declaration.kind() == DeclarationKind::GenericParameter
                        && let Some(owner) = declaration.parent()
                    {
                        generic_names_by_owner
                            .entry(owner)
                            .or_default()
                            .push(Box::<str>::from(name.text()));
                    }
                }
            }

            let mut names = module
                .declarations()
                .filter(|declaration| declaration.kind().namespace() == Some(Namespace::Type))
                .filter_map(|declaration| declaration.name().cloned())
                .collect::<BTreeSet<_>>();
            names.extend(
                module.imports().filter_map(|import| {
                    import.alias().or_else(|| import.target().last()).cloned()
                }),
            );

            for name in names {
                let local = lookup_source_module_type(collection, imports, module, &name, false)?;
                if !matches!(local, TypeNameLookup::NotFound) {
                    source_occupied.insert((module.id(), name.clone()));
                }
                let local_target = match &local {
                    TypeNameLookup::Unique(target) => {
                        source_bindings
                            .entry((module.id(), target.clone()))
                            .or_default()
                            .push(name.clone());
                        Some(target.clone())
                    }
                    _ => None,
                };

                let public = lookup_source_module_type(collection, imports, module, &name, true)?;
                let public_target = match public {
                    TypeNameLookup::Unique(target) => Some(target),
                    _ => None,
                };
                if let Some(target) = &public_target {
                    push_path(
                        &mut paths,
                        target.clone(),
                        module.address().as_str(),
                        module.address().components().len(),
                        &name,
                        PathScope::Public,
                    );
                }
                if let Some(target) = local_target
                    && public_target.as_ref() != Some(&target)
                {
                    push_path(
                        &mut paths,
                        target,
                        module.address().as_str(),
                        module.address().components().len(),
                        &name,
                        PathScope::Module(module.id()),
                    );
                }
            }
        }

        if let Some(external) = external {
            for declaration in external.declarations() {
                let target = CanonicalTarget::External(declaration.id().clone());
                target_names.insert(target.clone(), Box::<str>::from(declaration.name().text()));
                if declaration.kind().namespace() != Some(Namespace::Type) {
                    continue;
                }
                if matches!(
                    lookup_external_module_type(
                        Some(external),
                        declaration.module(),
                        declaration.name()
                    ),
                    TypeNameLookup::Unique(ref found) if found == &target
                ) {
                    push_path(
                        &mut paths,
                        target.clone(),
                        declaration.module().as_str(),
                        declaration.module().components().len(),
                        declaration.name(),
                        PathScope::Public,
                    );
                }
                if matches!(
                    lookup_prelude_type(Some(external), declaration.name()),
                    TypeNameLookup::Unique(ref found) if found == &target
                ) {
                    prelude_bindings
                        .entry(target)
                        .or_default()
                        .push(declaration.name().clone());
                }
            }
        }

        if let Some(trusted) = trusted {
            for owner in trusted.inherent_owners() {
                for member in &owner.members {
                    target_names.insert(
                        CanonicalTarget::External(member.callable.clone()),
                        Box::<str>::from(member.name.text()),
                    );
                }
            }
            for declaration in trusted.declarations() {
                if let TrustedSignatureKind::Trait {
                    associated_types, ..
                } = declaration.kind()
                {
                    for associated in associated_types {
                        target_names
                            .entry(CanonicalTarget::External(associated.id.clone()))
                            .or_insert_with(|| associated.name.clone());
                    }
                    for method in declaration.trait_methods() {
                        target_names
                            .entry(CanonicalTarget::External(method.declaration.clone()))
                            .or_insert_with(|| Box::<str>::from(method.name.text()));
                    }
                }
            }
        }

        remove_ambiguous_paths(&mut paths);
        Ok(Self {
            target_names,
            generic_names_by_owner: freeze_strings(generic_names_by_owner),
            source_bindings: freeze_names(source_bindings),
            source_occupied,
            prelude_bindings: freeze_names(prelude_bindings),
            prelude_blocked_modules,
            paths: freeze_paths(paths),
        })
    }

    pub(crate) fn target_name(&self, target: &CanonicalTarget) -> Option<&str> {
        self.target_names.get(target).map(AsRef::as_ref)
    }

    fn generic_names_at(
        &self,
        collection: &DeclarationCollection,
        owner: DefId,
    ) -> BTreeSet<Box<str>> {
        let mut names = BTreeSet::new();
        let mut cursor = Some(owner);
        while let Some(current) = cursor {
            if let Some(scoped) = self.generic_names_by_owner.get(&current) {
                names.extend(scoped.iter().cloned());
            }
            cursor = collection
                .declaration(current)
                .and_then(|declaration| declaration.parent());
        }
        names
    }

    fn shortest_path(
        &self,
        target: &CanonicalTarget,
        module: ModuleId,
        generic_names: &BTreeSet<Box<str>>,
    ) -> Option<String> {
        if let CanonicalTarget::Intrinsic(intrinsic) = target {
            return Some(intrinsic.spelling().to_owned());
        }
        if let CanonicalTarget::Primitive(primitive) = target {
            let name =
                NameKey::new(primitive.spelling(), false).expect("primitive type names are valid");
            let same_binding = self
                .source_bindings
                .get(&(module, target.clone()))
                .is_some_and(|names| names.contains(&name));
            let prelude_visible = !self.prelude_blocked_modules.contains(&module)
                && !self.source_occupied.contains(&(module, name));
            return Some(
                if !generic_names.contains(primitive.spelling())
                    && (same_binding || prelude_visible)
                {
                    primitive.spelling().to_owned()
                } else {
                    format!("std::prelude::{}", primitive.spelling())
                },
            );
        }
        let mut candidates = Vec::<(usize, String)>::new();
        if let Some(names) = self.source_bindings.get(&(module, target.clone())) {
            candidates.extend(
                names
                    .iter()
                    .filter(|name| !generic_names.contains(name.text()))
                    .map(|name| (1, name.spelling())),
            );
        }
        if !self.prelude_blocked_modules.contains(&module)
            && let Some(names) = self.prelude_bindings.get(target)
        {
            candidates.extend(
                names
                    .iter()
                    .filter(|name| {
                        !generic_names.contains(name.text())
                            && !self.source_occupied.contains(&(module, (*name).clone()))
                    })
                    .map(|name| (1, name.spelling())),
            );
        }
        if let Some(paths) = self.paths.get(target) {
            candidates.extend(
                paths
                    .iter()
                    .filter(|candidate| candidate.scope.is_visible_from(module))
                    .map(|candidate| (candidate.components, candidate.text.to_string())),
            );
        }
        candidates
            .into_iter()
            .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
            .map(|(_, text)| text)
    }
}

pub(crate) struct DiagnosticTypeRenderer<'a> {
    collection: &'a DeclarationCollection,
    names: &'a DiagnosticTypeNames,
    facts: &'a dyn DiagnosticTypeFacts,
    owner: DefId,
    generic_names: BTreeSet<Box<str>>,
    self_binding: Option<(CanonicalTarget, TypeId)>,
}

impl<'a> DiagnosticTypeRenderer<'a> {
    pub(crate) fn new(
        collection: &'a DeclarationCollection,
        names: &'a DiagnosticTypeNames,
        facts: &'a dyn DiagnosticTypeFacts,
        owner: DefId,
    ) -> Self {
        let generic_names = names.generic_names_at(collection, owner);
        Self {
            collection,
            names,
            facts,
            owner,
            generic_names,
            self_binding: None,
        }
    }

    pub(crate) fn with_self_binding(mut self, owner: CanonicalTarget, replacement: TypeId) -> Self {
        self.self_binding = Some((owner, replacement));
        self
    }

    pub(crate) fn render_trait_ref(
        &self,
        trait_ref: &TraitRef,
    ) -> Result<String, DiagnosticTypeRenderingError> {
        let name = self.render_target_path(trait_ref.declaration())?;
        let arguments = self.facts.types().substitution(trait_ref.substitution())?;
        self.render_application(name, arguments)
    }

    pub(crate) fn render_callable_signature(
        &self,
        callable: &CallableSignature,
    ) -> Result<String, DiagnosticTypeRenderingError> {
        let alpha_names = callable
            .generic_parameters()
            .iter()
            .enumerate()
            .map(|(index, parameter)| (parameter.clone(), format!("T{index}").into_boxed_str()))
            .collect::<BTreeMap<_, _>>();
        let alpha_facts = AlphaDiagnosticTypeFacts {
            base: self.facts,
            names: &alpha_names,
        };
        let mut renderer = DiagnosticTypeRenderer {
            collection: self.collection,
            names: self.names,
            facts: &alpha_facts,
            owner: self.owner,
            generic_names: self.names.generic_names_at(self.collection, self.owner),
            self_binding: self.self_binding.clone(),
        };
        renderer.generic_names.extend(alpha_names.values().cloned());

        let generic_parameters = callable
            .generic_parameters()
            .iter()
            .map(|parameter| {
                let name = alpha_names
                    .get(parameter)
                    .expect("alpha name exists for every callable generic parameter");
                let bounds = self
                    .facts
                    .generic_parameter_bounds(parameter)
                    .ok_or_else(|| {
                        DiagnosticTypeRenderingError::UnavailableGenericParameter(parameter.clone())
                    })?;
                let mut bounds = bounds
                    .iter()
                    .map(|bound| renderer.render_trait_ref(bound))
                    .collect::<Result<Vec<_>, _>>()?;
                bounds.sort();
                Ok(if bounds.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}: {}", bounds.join(" + "))
                })
            })
            .collect::<Result<Vec<_>, DiagnosticTypeRenderingError>>()?;
        let generic_parameters = if generic_parameters.is_empty() {
            String::new()
        } else {
            format!("[{}]", generic_parameters.join(", "))
        };
        let parameters = callable
            .parameters()
            .iter()
            .enumerate()
            .map(|(index, parameter)| {
                let name = parameter
                    .name()
                    .map_or_else(|| format!("#{index}"), str::to_owned);
                Ok(format!("{name}: {}", renderer.render_type(parameter.ty())?))
            })
            .collect::<Result<Vec<_>, DiagnosticTypeRenderingError>>()?;
        let mut name = self
            .names
            .target_name(callable.declaration())
            .ok_or_else(|| {
                DiagnosticTypeRenderingError::MissingTargetName(callable.declaration().clone())
            })?
            .to_owned();
        if callable.is_bang() {
            name.push('!');
        }
        Ok(format!(
            "fn {name}{generic_parameters}({}) -> {}",
            parameters.join(", "),
            renderer.render_type(callable.result())?
        ))
    }

    fn render_application(
        &self,
        name: String,
        arguments: &[TypeId],
    ) -> Result<String, DiagnosticTypeRenderingError> {
        if arguments.is_empty() {
            return Ok(name);
        }
        Ok(format!(
            "{name}[{}]",
            self.render_types(arguments)?.join(", ")
        ))
    }

    fn render_types(&self, types: &[TypeId]) -> Result<Vec<String>, DiagnosticTypeRenderingError> {
        types.iter().map(|&ty| self.render_type(ty)).collect()
    }

    fn render_target_path(
        &self,
        target: &CanonicalTarget,
    ) -> Result<String, DiagnosticTypeRenderingError> {
        self.names
            .shortest_path(target, self.owner.module(), &self.generic_names)
            .ok_or_else(|| DiagnosticTypeRenderingError::UnrenderableTarget {
                target: target.clone(),
                owner: self.owner,
            })
    }

    fn owner_contains_target(&self, target: &CanonicalTarget) -> bool {
        let CanonicalTarget::Source(target) = target else {
            return false;
        };
        let mut cursor = Some(self.owner);
        while let Some(current) = cursor {
            if current == *target {
                return true;
            }
            cursor = self
                .collection
                .declaration(current)
                .and_then(|declaration| declaration.parent());
        }
        false
    }
}

struct AlphaDiagnosticTypeFacts<'a> {
    base: &'a dyn DiagnosticTypeFacts,
    names: &'a BTreeMap<GenericParameterId, Box<str>>,
}

impl DiagnosticTypeFacts for AlphaDiagnosticTypeFacts<'_> {
    fn types(&self) -> &CanonicalTypeStore {
        self.base.types()
    }

    fn generic_parameter_name(&self, parameter: &GenericParameterId) -> Option<&str> {
        self.names
            .get(parameter)
            .map(AsRef::as_ref)
            .or_else(|| self.base.generic_parameter_name(parameter))
    }

    fn generic_parameter_bounds(&self, parameter: &GenericParameterId) -> Option<&[TraitRef]> {
        self.base.generic_parameter_bounds(parameter)
    }
}

fn push_path(
    paths: &mut BTreeMap<CanonicalTarget, Vec<TypePathCandidate>>,
    target: CanonicalTarget,
    module: &str,
    module_components: usize,
    name: &NameKey,
    scope: PathScope,
) {
    let text = format!("{module}::{}", name.spelling()).into_boxed_str();
    let candidates = paths.entry(target).or_default();
    if let Some(existing) = candidates
        .iter_mut()
        .find(|candidate| candidate.text == text)
    {
        if scope == PathScope::Public {
            existing.scope = PathScope::Public;
        }
        return;
    }
    candidates.push(TypePathCandidate {
        text,
        components: module_components + 1,
        scope,
    });
}

fn remove_ambiguous_paths(paths: &mut BTreeMap<CanonicalTarget, Vec<TypePathCandidate>>) {
    let mut targets_by_text = BTreeMap::<Box<str>, BTreeSet<CanonicalTarget>>::new();
    for (target, candidates) in paths.iter() {
        for candidate in candidates {
            targets_by_text
                .entry(candidate.text.clone())
                .or_default()
                .insert(target.clone());
        }
    }
    let ambiguous = targets_by_text
        .into_iter()
        .filter_map(|(text, targets)| (targets.len() > 1).then_some(text))
        .collect::<BTreeSet<_>>();
    for candidates in paths.values_mut() {
        candidates.retain(|candidate| !ambiguous.contains(&candidate.text));
    }
}

fn freeze_names<K: Ord>(source: BTreeMap<K, Vec<NameKey>>) -> BTreeMap<K, Box<[NameKey]>> {
    source
        .into_iter()
        .map(|(key, mut names)| {
            names.sort();
            names.dedup();
            (key, names.into_boxed_slice())
        })
        .collect()
}

fn freeze_strings<K: Ord>(source: BTreeMap<K, Vec<Box<str>>>) -> BTreeMap<K, Box<[Box<str>]>> {
    source
        .into_iter()
        .map(|(key, mut values)| {
            values.sort();
            values.dedup();
            (key, values.into_boxed_slice())
        })
        .collect()
}

fn freeze_paths(
    source: BTreeMap<CanonicalTarget, Vec<TypePathCandidate>>,
) -> BTreeMap<CanonicalTarget, Box<[TypePathCandidate]>> {
    source
        .into_iter()
        .map(|(target, mut paths)| {
            paths.sort_by(|left, right| {
                left.components
                    .cmp(&right.components)
                    .then_with(|| left.text.cmp(&right.text))
            });
            (target, paths.into_boxed_slice())
        })
        .collect()
}

struct TypePathCandidate {
    text: Box<str>,
    components: usize,
    scope: PathScope,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PathScope {
    Public,
    Module(ModuleId),
}

impl PathScope {
    fn is_visible_from(self, module: ModuleId) -> bool {
        match self {
            Self::Public => true,
            Self::Module(owner) => owner == module,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiagnosticTypeRenderingError {
    TypeStore(TypeStoreError),
    MissingTargetName(CanonicalTarget),
    UnrenderableTarget {
        target: CanonicalTarget,
        owner: DefId,
    },
    UnavailableGenericParameter(GenericParameterId),
    UnboundSelfType {
        target: CanonicalTarget,
        owner: DefId,
    },
    InvalidUnionArity(usize),
}

impl fmt::Display for DiagnosticTypeRenderingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TypeStore(error) => error.fmt(formatter),
            Self::MissingTargetName(target) => {
                write!(
                    formatter,
                    "diagnostic type target {target:?} has no source name"
                )
            }
            Self::UnrenderableTarget { target, owner } => write!(
                formatter,
                "diagnostic type target {target:?} has no source-writable path from {owner:?}"
            ),
            Self::UnavailableGenericParameter(parameter) => write!(
                formatter,
                "diagnostic type references unavailable generic parameter {parameter:?}"
            ),
            Self::UnboundSelfType { target, owner } => write!(
                formatter,
                "diagnostic Self type {target:?} is not bound at {owner:?}"
            ),
            Self::InvalidUnionArity(arity) => {
                write!(
                    formatter,
                    "diagnostic error union has invalid arity {arity}"
                )
            }
        }
    }
}

impl From<TypeStoreError> for DiagnosticTypeRenderingError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}
