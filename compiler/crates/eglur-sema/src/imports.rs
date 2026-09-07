use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_diagnostics::{DiagnosticStore, DiagnosticStoreError, Poison};
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    ResolveCapability,
};
use crate::diagnostic::{
    SemanticDiagnosticError, record_import_error, record_name_ambiguous,
    record_visibility_violation,
};
use crate::types::is_reserved_type_name;
use crate::{
    DeclarationCollection, DefId, DuplicateFact, ExternalDefId, ExternalNameEnvironment, FactState,
    FactTable, ImportId, ImportRecord, ModuleAddress, ModuleId, NameKey, Namespace, SemanticFact,
    SemanticLocation, SemanticSource,
};

/// One source-unit dependency discovered from a resolved local import module.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SemanticImportDependency {
    importer: FileId,
    imported: FileId,
}

impl SemanticImportDependency {
    #[must_use]
    pub const fn importer(self) -> FileId {
        self.importer
    }

    #[must_use]
    pub const fn imported(self) -> FileId {
        self.imported
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportModuleTarget {
    Source(ModuleId),
    External(ModuleAddress),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedImport {
    module: ImportModuleTarget,
    symbols: Box<[NameKey]>,
}

impl PlannedImport {
    #[must_use]
    pub const fn module(&self) -> &ImportModuleTarget {
        &self.module
    }

    #[must_use]
    pub fn symbols(&self) -> &[NameKey] {
        &self.symbols
    }
}

/// Module-target facts and the exact local dependency edges they establish.
pub struct ImportPlan {
    targets: FactTable<ImportId, PlannedImport>,
    dependencies: Box<[SemanticImportDependency]>,
}

impl ImportPlan {
    #[must_use]
    pub fn target(&self, import: ImportId) -> FactState<'_, PlannedImport> {
        self.targets.state(&import)
    }

    #[must_use]
    pub fn dependencies(&self) -> &[SemanticImportDependency] {
        &self.dependencies
    }

    /// Returns the number of import-target facts established by planning.
    #[must_use]
    pub fn target_fact_count(&self) -> usize {
        self.targets.iter().len()
    }

    pub(crate) fn is_complete_for(&self, collection: &DeclarationCollection) -> bool {
        self.target_fact_count() == collection.import_count() && self.targets.all_known()
    }
}

/// Resolves import module prefixes without performing symbol lookup or SCC analysis.
pub fn plan_imports(
    collection: &DeclarationCollection,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    external: Option<&ExternalNameEnvironment>,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<ImportPlan>, ImportResolutionError> {
    let mut recorder = DemandRecorder::new(support);
    let sources = sources_by_file(sources)?;
    let mut targets = FactTable::new();
    let mut dependencies = BTreeSet::new();

    for module in collection.modules() {
        let source = sources
            .get(&module.file())
            .copied()
            .ok_or(ImportResolutionError::MissingSource(module.file()))?;
        for import in module.imports() {
            let location = SemanticLocation::origin(module.file(), import.origin());
            if !recorder.require(
                CompileCapability::Resolve(ResolveCapability::ResolveFileSetImports),
                location,
            ) {
                continue;
            }
            let lookup = match locate_import_module(collection, module.id(), import, None)? {
                ModuleLookup::NeedsExternal => {
                    let capability =
                        CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames);
                    if !recorder.require(capability, location) {
                        continue;
                    }
                    let environment = external
                        .ok_or(ImportResolutionError::MissingCapabilityInput(capability))?;
                    locate_import_module(collection, module.id(), import, Some(environment))?
                }
                lookup => lookup,
            };
            match lookup {
                ModuleLookup::Found(planned) => {
                    if let ImportModuleTarget::Source(target) = planned.module {
                        let target_file = collection
                            .module(target)
                            .expect("planned source module belongs to collection")
                            .file();
                        dependencies.insert(SemanticImportDependency {
                            importer: module.file(),
                            imported: target_file,
                        });
                    }
                    targets.insert(import.id(), SemanticFact::Known(planned))?;
                }
                ModuleLookup::NeedsExternal => unreachable!("external input was supplied"),
                ModuleLookup::UnrepresentedStandardLibrary => {
                    let capability = CompileCapability::Resolve(
                        ResolveCapability::ResolveCompleteStandardLibraryNames,
                    );
                    if recorder.require(capability, location) {
                        return Err(ImportResolutionError::UnsupportedCapabilityClaim(
                            capability,
                        ));
                    }
                }
                ModuleLookup::SuperAtRoot => {
                    let span = required_span(source, import.origin())?;
                    let poison = record_import_error(
                        diagnostics,
                        "IMPORT_SUPER_AT_ROOT",
                        "super import used at module root",
                        span,
                    )?;
                    targets.insert(import.id(), SemanticFact::Poisoned(poison))?;
                }
                ModuleLookup::NotFound => {
                    let poison = record_import_not_found(diagnostics, source, import)?;
                    targets.insert(import.id(), SemanticFact::Poisoned(poison))?;
                }
            }
        }
    }

    Ok(recorder.finish(PhaseProduct::Performed(ImportPlan {
        targets,
        dependencies: dependencies
            .into_iter()
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    })))
}

enum ModuleLookup {
    Found(PlannedImport),
    NeedsExternal,
    UnrepresentedStandardLibrary,
    SuperAtRoot,
    NotFound,
}

fn locate_import_module(
    collection: &DeclarationCollection,
    importer: ModuleId,
    import: &ImportRecord,
    external: Option<&ExternalNameEnvironment>,
) -> Result<ModuleLookup, ImportResolutionError> {
    let importer = collection
        .module(importer)
        .expect("import record owner belongs to declaration collection");
    let target = import.target();
    let first = target
        .first()
        .expect("declaration collection rejects empty import paths");
    if !first.is_bang() && first.text() == "self" {
        let base = importer
            .address()
            .components()
            .map(Box::<str>::from)
            .collect::<Vec<_>>();
        return Ok(
            find_relative_module(collection, &base, &target[1..]).unwrap_or(ModuleLookup::NotFound)
        );
    }
    if !first.is_bang() && first.text() == "super" {
        let mut base = importer
            .address()
            .components()
            .map(Box::<str>::from)
            .collect::<Vec<_>>();
        if base.len() == 1 {
            return Ok(ModuleLookup::SuperAtRoot);
        }
        base.pop();
        return Ok(
            find_relative_module(collection, &base, &target[1..]).unwrap_or(ModuleLookup::NotFound)
        );
    }

    let external_only = !first.is_bang() && first.text() == "std";
    for prefix_len in (1..target.len()).rev() {
        if target[..prefix_len].iter().any(NameKey::is_bang) {
            continue;
        }
        let address = ModuleAddress::new(
            target[..prefix_len]
                .iter()
                .map(|part| Box::<str>::from(part.text())),
        )
        .expect("canonical import components are non-empty");
        let symbols = target[prefix_len..].to_vec().into_boxed_slice();
        if !external_only && let Some(module) = collection.module_for_address(&address) {
            return Ok(ModuleLookup::Found(PlannedImport {
                module: ImportModuleTarget::Source(module),
                symbols,
            }));
        }
        if external.is_some_and(|environment| environment.has_module(&address)) {
            return Ok(ModuleLookup::Found(PlannedImport {
                module: ImportModuleTarget::External(address),
                symbols,
            }));
        }
    }

    if external
        .is_some_and(|environment| external_only && !environment.standard_library_is_complete())
    {
        Ok(ModuleLookup::UnrepresentedStandardLibrary)
    } else if external.is_some() {
        Ok(ModuleLookup::NotFound)
    } else {
        Ok(ModuleLookup::NeedsExternal)
    }
}

fn find_relative_module(
    collection: &DeclarationCollection,
    base: &[Box<str>],
    relative: &[NameKey],
) -> Option<ModuleLookup> {
    if relative.is_empty() {
        return None;
    }
    for module_count in (0..relative.len()).rev() {
        if relative[..module_count].iter().any(NameKey::is_bang) {
            continue;
        }
        let address = ModuleAddress::new(
            base.iter().cloned().chain(
                relative[..module_count]
                    .iter()
                    .map(|part| Box::<str>::from(part.text())),
            ),
        )
        .expect("relative import base is a non-empty canonical module path");
        if let Some(module) = collection.module_for_address(&address) {
            return Some(ModuleLookup::Found(PlannedImport {
                module: ImportModuleTarget::Source(module),
                symbols: relative[module_count..].to_vec().into_boxed_slice(),
            }));
        }
    }
    None
}

/// One driver-established import SCC analysis unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportAnalysisUnit {
    members: Box<[FileId]>,
}

impl ImportAnalysisUnit {
    pub fn new(members: impl IntoIterator<Item = FileId>) -> Result<Self, ImportResolutionError> {
        let members = members.into_iter().collect::<Vec<_>>();
        if members.is_empty() || !members.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(ImportResolutionError::InvalidAnalysisUnit);
        }
        Ok(Self {
            members: members.into_boxed_slice(),
        })
    }

    #[must_use]
    pub fn members(&self) -> &[FileId] {
        &self.members
    }
}

/// Canonical declaration identity selected by source or trusted metadata.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CanonicalTarget {
    Source(DefId),
    External(ExternalDefId),
}

/// One namespace binding introduced by a resolved import entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportedBinding {
    local_name: NameKey,
    namespace: Namespace,
    targets: Box<[CanonicalTarget]>,
    public: bool,
}

impl ImportedBinding {
    #[must_use]
    pub const fn local_name(&self) -> &NameKey {
        &self.local_name
    }

    #[must_use]
    pub const fn namespace(&self) -> Namespace {
        self.namespace
    }

    #[must_use]
    pub fn targets(&self) -> &[CanonicalTarget] {
        &self.targets
    }

    #[must_use]
    pub const fn is_public(&self) -> bool {
        self.public
    }
}

/// Import binding facts after fixed-point resolution inside driver-owned SCCs.
pub struct ResolvedImports {
    facts: FactTable<ImportId, Box<[ImportedBinding]>>,
    lookup: BTreeMap<(ModuleId, Namespace, NameKey), Box<[ImportId]>>,
}

impl ResolvedImports {
    #[must_use]
    pub fn fact(&self, import: ImportId) -> FactState<'_, Box<[ImportedBinding]>> {
        self.facts.state(&import)
    }

    #[must_use]
    pub fn lookup(&self, module: ModuleId, namespace: Namespace, name: &NameKey) -> &[ImportId] {
        self.lookup
            .get(&(module, namespace, name.clone()))
            .map_or(&[], AsRef::as_ref)
    }

    /// Returns the number of import-binding facts established by resolution.
    #[must_use]
    pub fn binding_fact_count(&self) -> usize {
        self.facts.iter().len()
    }

    pub(crate) fn is_complete_for(&self, collection: &DeclarationCollection) -> bool {
        self.binding_fact_count() == collection.import_count() && self.facts.all_known()
    }
}

/// Resolves imported symbols and re-exports in the exact driver-provided SCC schedule.
pub fn resolve_imports(
    collection: &DeclarationCollection,
    plan: &ImportPlan,
    units: &[ImportAnalysisUnit],
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    external: Option<&ExternalNameEnvironment>,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<ResolvedImports>, ImportResolutionError> {
    validate_schedule(collection, plan, units)?;
    let sources = sources_by_file(sources)?;
    let mut recorder = DemandRecorder::new(support);
    let mut facts = FactTable::new();
    let mut lookup = BTreeMap::<(ModuleId, Namespace, NameKey), Vec<ImportId>>::new();
    let mut declared_imports = BTreeMap::<(ModuleId, NameKey), Vec<ImportId>>::new();
    for module in collection.modules() {
        for import in module.imports() {
            let local = import
                .alias()
                .or_else(|| import.target().last())
                .expect("import path is non-empty")
                .clone();
            declared_imports
                .entry((module.id(), local.clone()))
                .or_default()
                .push(import.id());
            for namespace in [Namespace::Value, Namespace::Type] {
                lookup
                    .entry((module.id(), namespace, local.clone()))
                    .or_default()
                    .push(import.id());
            }
        }
    }
    let mut unperformed = BTreeSet::new();

    for unit in units {
        let mut pending = unit
            .members()
            .iter()
            .filter_map(|&file| collection.module_for_file(file))
            .flat_map(|module| {
                collection
                    .module(module)
                    .expect("scheduled module belongs to collection")
                    .imports()
                    .map(ImportRecord::id)
            })
            .collect::<BTreeSet<_>>();

        loop {
            let mut progress = false;
            let current = pending.iter().copied().collect::<Vec<_>>();
            for import_id in current {
                let module = collection
                    .module(import_id.module())
                    .expect("import identity owner belongs to collection");
                let import = module
                    .imports()
                    .find(|record| record.id() == import_id)
                    .expect("import identity belongs to owner inventory");
                let source = sources
                    .get(&module.file())
                    .copied()
                    .ok_or(ImportResolutionError::MissingSource(module.file()))?;
                let location = SemanticLocation::origin(module.file(), import.origin());
                if !recorder.require(
                    CompileCapability::Resolve(ResolveCapability::ResolveModuleValues),
                    location,
                ) {
                    unperformed.insert(import_id);
                    pending.remove(&import_id);
                    progress = true;
                    continue;
                }
                match attempt_import(
                    collection,
                    plan,
                    &facts,
                    &unperformed,
                    &declared_imports,
                    import,
                    external,
                )? {
                    ImportAttempt::Ready(bindings) => {
                        if let Some(poison) =
                            validate_reserved_type_alias(diagnostics, source, import, &bindings)?
                        {
                            facts.insert(import_id, SemanticFact::Poisoned(poison))?;
                            pending.remove(&import_id);
                            progress = true;
                            continue;
                        }
                        if let Some(poison) = validate_import_bang(
                            collection,
                            external,
                            diagnostics,
                            source,
                            import,
                            &bindings,
                        )? {
                            facts.insert(import_id, SemanticFact::Poisoned(poison))?;
                            pending.remove(&import_id);
                            progress = true;
                            continue;
                        }
                        facts
                            .insert(import_id, SemanticFact::Known(bindings.into_boxed_slice()))?;
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::Poisoned(poison) => {
                        facts.insert(import_id, SemanticFact::Poisoned(poison))?;
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::Visibility => {
                        let poison = record_visibility_violation(
                            diagnostics,
                            required_span(source, import.origin())?,
                        )?;
                        facts.insert(import_id, SemanticFact::Poisoned(poison))?;
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::Ambiguous(candidates) => {
                        let name = import
                            .target()
                            .last()
                            .expect("import target is non-empty")
                            .spelling();
                        let poison = record_name_ambiguous(
                            diagnostics,
                            required_span(source, import.origin())?,
                            &name,
                            candidates,
                        )?;
                        facts.insert(import_id, SemanticFact::Poisoned(poison))?;
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::NotFound => {
                        let poison = record_import_not_found(diagnostics, source, import)?;
                        facts.insert(import_id, SemanticFact::Poisoned(poison))?;
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::UnrepresentedStandardLibrary => {
                        let capability = CompileCapability::Resolve(
                            ResolveCapability::ResolveCompleteStandardLibraryNames,
                        );
                        if recorder.require(capability, location) {
                            return Err(ImportResolutionError::UnsupportedCapabilityClaim(
                                capability,
                            ));
                        }
                        unperformed.insert(import_id);
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::Unperformed => {
                        unperformed.insert(import_id);
                        pending.remove(&import_id);
                        progress = true;
                    }
                    ImportAttempt::Pending => {}
                }
            }
            if !progress {
                break;
            }
        }

        for import_id in pending {
            if unperformed.contains(&import_id)
                || matches!(plan.target(import_id), FactState::Unperformed)
            {
                continue;
            }
            let module = collection
                .module(import_id.module())
                .expect("pending import owner belongs to collection");
            let import = module
                .imports()
                .find(|record| record.id() == import_id)
                .expect("pending import belongs to owner inventory");
            let source = sources
                .get(&module.file())
                .copied()
                .ok_or(ImportResolutionError::MissingSource(module.file()))?;
            let poison = if let Some(poison) = module.import_poison() {
                poison.clone()
            } else {
                record_import_not_found(diagnostics, source, import)?
            };
            facts.insert(import_id, SemanticFact::Poisoned(poison))?;
        }
    }

    let lookup = lookup
        .into_iter()
        .map(|(key, mut imports)| {
            imports.sort();
            imports.dedup();
            (key, imports.into_boxed_slice())
        })
        .collect();
    Ok(recorder.finish(PhaseProduct::Performed(ResolvedImports { facts, lookup })))
}

enum ImportAttempt {
    Ready(Vec<ImportedBinding>),
    Poisoned(Poison),
    Visibility,
    Ambiguous(Vec<String>),
    NotFound,
    UnrepresentedStandardLibrary,
    Unperformed,
    Pending,
}

fn attempt_import(
    collection: &DeclarationCollection,
    plan: &ImportPlan,
    facts: &FactTable<ImportId, Box<[ImportedBinding]>>,
    unperformed: &BTreeSet<ImportId>,
    declared_imports: &BTreeMap<(ModuleId, NameKey), Vec<ImportId>>,
    import: &ImportRecord,
    external: Option<&ExternalNameEnvironment>,
) -> Result<ImportAttempt, ImportResolutionError> {
    let planned = match plan.target(import.id()) {
        FactState::Known(planned) => planned,
        FactState::Poisoned(poison) => return Ok(ImportAttempt::Poisoned(poison.clone())),
        FactState::Unperformed => return Ok(ImportAttempt::Unperformed),
        FactState::NotApplicable => return Err(ImportResolutionError::InvalidPlanFact),
    };
    if planned.symbols().len() != 1 {
        return Ok(ImportAttempt::NotFound);
    }
    let symbol = &planned.symbols()[0];
    let local_name = import.alias().unwrap_or(symbol).clone();
    let mut bindings = Vec::new();
    match planned.module() {
        ImportModuleTarget::External(module) => {
            let Some(environment) = external else {
                return Err(ImportResolutionError::MissingCapabilityInput(
                    CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames),
                ));
            };
            for namespace in [Namespace::Value, Namespace::Type] {
                let candidates = environment.module_candidates(module, namespace, symbol);
                if !candidates.is_empty() {
                    bindings.push(ImportedBinding {
                        local_name: local_name.clone(),
                        namespace,
                        targets: candidates
                            .iter()
                            .cloned()
                            .map(CanonicalTarget::External)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        public: import.visibility().is_public(),
                    });
                }
            }
            if bindings.is_empty()
                && !environment.standard_library_is_complete()
                && module.components().next() == Some("std")
            {
                return Ok(ImportAttempt::UnrepresentedStandardLibrary);
            }
        }
        ImportModuleTarget::Source(target_module) => {
            let target = collection
                .module(*target_module)
                .expect("planned source target belongs to collection");
            if let Some(poison) = target.declaration_poison() {
                return Ok(ImportAttempt::Poisoned(poison.clone()));
            }
            for namespace in [Namespace::Value, Namespace::Type] {
                let declarations = target.candidates(namespace, symbol);
                if !declarations.is_empty() {
                    if (import.id().module() != *target_module || import.visibility().is_public())
                        && declarations.iter().any(|&candidate| {
                            !collection
                                .declaration(candidate)
                                .expect("module candidate belongs to collection")
                                .visibility()
                                .is_public()
                        })
                    {
                        return Ok(ImportAttempt::Visibility);
                    }
                    if !is_single_candidate_or_callable_family(collection, declarations) {
                        return Ok(ImportAttempt::Ambiguous(
                            declarations
                                .iter()
                                .map(|candidate| format!("{candidate:?}"))
                                .collect(),
                        ));
                    }
                    bindings.push(ImportedBinding {
                        local_name: local_name.clone(),
                        namespace,
                        targets: declarations
                            .iter()
                            .copied()
                            .map(CanonicalTarget::Source)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                        public: import.visibility().is_public(),
                    });
                    continue;
                }

                let imported = declared_imports
                    .get(&(*target_module, symbol.clone()))
                    .map_or(&[][..], Vec::as_slice);
                let mut targets = Vec::new();
                let mut saw_pending = false;
                for &candidate in imported {
                    if unperformed.contains(&candidate) {
                        return Ok(ImportAttempt::Unperformed);
                    }
                    match facts.state(&candidate) {
                        FactState::Known(candidate_bindings) => {
                            for candidate_binding in candidate_bindings.iter().filter(|binding| {
                                binding.namespace == namespace
                                    && binding.local_name == *symbol
                                    && (import.id().module() == *target_module || binding.public)
                            }) {
                                if (import.id().module() != *target_module
                                    || import.visibility().is_public())
                                    && candidate_binding
                                        .targets
                                        .iter()
                                        .any(|target| !target_is_public(collection, target))
                                {
                                    return Ok(ImportAttempt::Visibility);
                                }
                                targets.extend(candidate_binding.targets.iter().cloned());
                            }
                        }
                        FactState::Poisoned(poison) => {
                            return Ok(ImportAttempt::Poisoned(poison.clone()));
                        }
                        FactState::Unperformed => saw_pending = true,
                        FactState::NotApplicable => {
                            return Err(ImportResolutionError::InvalidImportFact);
                        }
                    }
                }
                targets.sort();
                targets.dedup();
                if !targets.is_empty() {
                    bindings.push(ImportedBinding {
                        local_name: local_name.clone(),
                        namespace,
                        targets: targets.into_boxed_slice(),
                        public: import.visibility().is_public(),
                    });
                } else if saw_pending {
                    return Ok(ImportAttempt::Pending);
                }
            }
            if bindings.is_empty()
                && let Some(poison) = target.import_poison()
            {
                return Ok(ImportAttempt::Poisoned(poison.clone()));
            }
        }
    }
    if bindings.is_empty() {
        Ok(ImportAttempt::NotFound)
    } else {
        Ok(ImportAttempt::Ready(bindings))
    }
}

fn is_single_candidate_or_callable_family(
    collection: &DeclarationCollection,
    candidates: &[DefId],
) -> bool {
    candidates.len() == 1
        || candidates.iter().all(|&candidate| {
            collection
                .declaration(candidate)
                .is_some_and(|declaration| declaration.kind().permits_overload_family())
        })
}

fn target_is_public(collection: &DeclarationCollection, target: &CanonicalTarget) -> bool {
    match target {
        CanonicalTarget::Source(id) => collection
            .declaration(*id)
            .is_some_and(|declaration| declaration.visibility().is_public()),
        CanonicalTarget::External(_) => true,
    }
}

fn validate_reserved_type_alias(
    diagnostics: &mut DiagnosticStore,
    source: &SemanticSource<'_>,
    import: &ImportRecord,
    bindings: &[ImportedBinding],
) -> Result<Option<Poison>, ImportResolutionError> {
    let Some(alias) = import.alias() else {
        return Ok(None);
    };
    if !is_reserved_type_name(alias.text())
        || !bindings
            .iter()
            .any(|binding| binding.namespace() == Namespace::Type)
    {
        return Ok(None);
    }
    let origin = import.alias_origin().unwrap_or(import.origin());
    Ok(Some(record_import_error(
        diagnostics,
        "RESERVED_BUILTIN_NAME",
        format!(
            "reserved built-in name `{}` cannot be introduced as a type import alias",
            alias.text()
        ),
        required_span(source, origin)?,
    )?))
}

fn validate_import_bang(
    collection: &DeclarationCollection,
    external: Option<&ExternalNameEnvironment>,
    diagnostics: &mut DiagnosticStore,
    source: &SemanticSource<'_>,
    import: &ImportRecord,
    bindings: &[ImportedBinding],
) -> Result<Option<Poison>, ImportResolutionError> {
    let target_is_bang = bindings
        .iter()
        .flat_map(ImportedBinding::targets)
        .any(|target| match target {
            CanonicalTarget::Source(id) => collection
                .declaration(*id)
                .and_then(|declaration| declaration.name())
                .is_some_and(NameKey::is_bang),
            CanonicalTarget::External(id) => external
                .and_then(|environment| environment.declaration(id))
                .is_some_and(|declaration| declaration.name().is_bang()),
        });
    if target_is_bang && import.alias().is_some_and(|alias| !alias.is_bang()) {
        let origin = import.alias_origin().unwrap_or(import.origin());
        return Ok(Some(record_import_error(
            diagnostics,
            "IMPORT_BANG_ALIAS_MUST_BANG",
            "bang import alias must include !",
            required_span(source, origin)?,
        )?));
    }
    Ok(None)
}

fn validate_schedule(
    collection: &DeclarationCollection,
    plan: &ImportPlan,
    units: &[ImportAnalysisUnit],
) -> Result<(), ImportResolutionError> {
    let expected = collection
        .modules()
        .map(|module| module.file())
        .collect::<BTreeSet<_>>();
    let actual = units
        .iter()
        .flat_map(|unit| unit.members.iter().copied())
        .collect::<Vec<_>>();
    let unique = actual.iter().copied().collect::<BTreeSet<_>>();
    if actual.len() != unique.len() || unique != expected {
        return Err(ImportResolutionError::InvalidScheduleCoverage);
    }
    let unit_by_file = units
        .iter()
        .enumerate()
        .flat_map(|(index, unit)| unit.members.iter().map(move |&file| (file, index)))
        .collect::<BTreeMap<_, _>>();
    for dependency in plan.dependencies() {
        let importer = unit_by_file[&dependency.importer()];
        let imported = unit_by_file[&dependency.imported()];
        if imported > importer {
            return Err(ImportResolutionError::InvalidScheduleOrder {
                importer: dependency.importer(),
                imported: dependency.imported(),
            });
        }
    }
    Ok(())
}

fn record_import_not_found(
    diagnostics: &mut DiagnosticStore,
    source: &SemanticSource<'_>,
    import: &ImportRecord,
) -> Result<Poison, ImportResolutionError> {
    Ok(record_import_error(
        diagnostics,
        "IMPORT_NOT_FOUND",
        "import not found",
        required_span(source, import.origin())?,
    )?)
}

fn required_span(
    source: &SemanticSource<'_>,
    origin: eglur_hir::OriginId,
) -> Result<eglur_diagnostics::SourceSpan, ImportResolutionError> {
    source
        .origin_span(origin)
        .cloned()
        .ok_or(ImportResolutionError::MissingOriginSpan {
            file: source.file(),
            origin,
        })
}

fn sources_by_file<'source, 'hir>(
    sources: &'source [SemanticSource<'hir>],
) -> Result<BTreeMap<FileId, &'source SemanticSource<'hir>>, ImportResolutionError> {
    let mut by_file = BTreeMap::new();
    for source in sources {
        if by_file.insert(source.file(), source).is_some() {
            return Err(ImportResolutionError::DuplicateSource(source.file()));
        }
    }
    Ok(by_file)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportResolutionError {
    DuplicateSource(FileId),
    MissingSource(FileId),
    MissingOriginSpan {
        file: FileId,
        origin: eglur_hir::OriginId,
    },
    MissingCapabilityInput(CompileCapability),
    UnsupportedCapabilityClaim(CompileCapability),
    InvalidAnalysisUnit,
    InvalidScheduleCoverage,
    InvalidScheduleOrder {
        importer: FileId,
        imported: FileId,
    },
    InvalidPlanFact,
    InvalidImportFact,
    DuplicateFact(DuplicateFact),
    DiagnosticRoot(eglur_diagnostics::DiagnosticRootError),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for ImportResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSource(file) => write!(formatter, "duplicate semantic source {file}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file}"),
            Self::MissingOriginSpan { file, origin } => write!(
                formatter,
                "semantic source {file} omitted origin span {}",
                origin.get()
            ),
            Self::MissingCapabilityInput(capability) => write!(
                formatter,
                "supported capability {} has no required boundary input",
                capability.stable_key()
            ),
            Self::UnsupportedCapabilityClaim(capability) => write!(
                formatter,
                "profile claims unimplemented capability {}",
                capability.stable_key()
            ),
            Self::InvalidAnalysisUnit => {
                formatter.write_str("import analysis unit must be non-empty, sorted, and unique")
            }
            Self::InvalidScheduleCoverage => {
                formatter.write_str("import schedule does not cover each source exactly once")
            }
            Self::InvalidScheduleOrder { importer, imported } => write!(
                formatter,
                "import schedule analyzes importer {importer} before dependency {imported}"
            ),
            Self::InvalidPlanFact => {
                formatter.write_str("import plan contains not-applicable fact")
            }
            Self::InvalidImportFact => {
                formatter.write_str("resolved import table contains not-applicable fact")
            }
            Self::DuplicateFact(error) => error.fmt(formatter),
            Self::DiagnosticRoot(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ImportResolutionError {}

impl From<DuplicateFact> for ImportResolutionError {
    fn from(error: DuplicateFact) -> Self {
        Self::DuplicateFact(error)
    }
}

impl From<SemanticDiagnosticError> for ImportResolutionError {
    fn from(error: SemanticDiagnosticError) -> Self {
        match error {
            SemanticDiagnosticError::Root(error) => Self::DiagnosticRoot(error),
            SemanticDiagnosticError::Store(error) => Self::DiagnosticStore(error),
        }
    }
}

impl From<DiagnosticStoreError> for ImportResolutionError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}
