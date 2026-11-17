mod traversal;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_diagnostics::{DiagnosticStore, DiagnosticStoreError, Poison, SourceSpan};
use eglur_hir::{
    AttributeMeta, AttributeValue, Body, BodyId, ExprId, ExprKind, FunctionBody, HirFile, ItemId,
    ItemKind, Name, OriginId, Path, PatternElement, PatternId, PatternKind, StmtId, TypeRefId,
    TypeRefKind,
};
use eglur_source::FileId;

use crate::capability::{
    CompileCapability, CompileSupport, DemandRecorder, PhaseAnalysis, PhaseProduct,
    ResolveCapability, TypecheckCapability,
};
use crate::diagnostic::{
    SemanticDiagnosticError, record_local_redeclaration, record_name_ambiguous,
    record_name_not_found, record_visibility_violation,
};
use crate::{
    CanonicalTarget, DeclarationCollection, DeclarationKind, DefId, DuplicateFact, ExternalDefId,
    ExternalNameEnvironment, FactState, FactTable, IdentityOverflow, LocalBindingId, ModuleAddress,
    ModuleId, NameKey, Namespace, ResolvedImports, ScopeId, SemanticFact, SemanticLocation,
    SemanticSource,
};

/// HIR occurrence whose name-resolution operation has an independent fact.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ResolutionKey {
    Expression {
        module: ModuleId,
        expression: ExprId,
    },
    Type {
        module: ModuleId,
        ty: TypeRefId,
    },
    Pattern {
        module: ModuleId,
        pattern: PatternId,
    },
}

/// Canonical result of one source name lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedName {
    Local(LocalBindingId),
    Declarations(Box<[CanonicalTarget]>),
    TypeMemberOwner(CanonicalTarget),
}

/// One unqualified lookup retained until a contextual expression commits its meaning.
#[derive(Clone, Debug)]
pub(crate) struct NameLookupPlan {
    requirements: Box<[CompileCapability]>,
    name: NameKey,
    decision: NameLookupDecision,
}

impl NameLookupPlan {
    pub(crate) fn requirements(&self) -> &[CompileCapability] {
        &self.requirements
    }

    pub(crate) const fn name(&self) -> &NameKey {
        &self.name
    }

    pub(crate) const fn decision(&self) -> &NameLookupDecision {
        &self.decision
    }
}

#[derive(Clone, Debug)]
pub(crate) enum NameLookupDecision {
    Resolved(ResolvedName),
    Poisoned(Poison),
    Unperformed,
    NotApplicable,
    NotFound,
    Ambiguous(Box<[String]>),
    VisibilityViolation,
    MissingCapabilityInput(CompileCapability),
}

pub(crate) enum NameLookupMaterialization {
    Resolved(ResolvedName),
    Poisoned(Poison),
    Unperformed,
    NotApplicable,
    MissingCapabilityInput(CompileCapability),
}

pub(crate) fn materialize_name_lookup(
    diagnostics: &mut DiagnosticStore,
    span: SourceSpan,
    name: &NameKey,
    decision: &NameLookupDecision,
    not_found_is_not_applicable: bool,
) -> Result<NameLookupMaterialization, SemanticDiagnosticError> {
    Ok(match decision {
        NameLookupDecision::Resolved(resolution) => {
            NameLookupMaterialization::Resolved(resolution.clone())
        }
        NameLookupDecision::Poisoned(poison) => NameLookupMaterialization::Poisoned(poison.clone()),
        NameLookupDecision::Unperformed => NameLookupMaterialization::Unperformed,
        NameLookupDecision::NotApplicable => NameLookupMaterialization::NotApplicable,
        NameLookupDecision::NotFound if not_found_is_not_applicable => {
            NameLookupMaterialization::NotApplicable
        }
        NameLookupDecision::NotFound => NameLookupMaterialization::Poisoned(record_name_not_found(
            diagnostics,
            span,
            &name.spelling(),
        )?),
        NameLookupDecision::Ambiguous(candidates) => {
            NameLookupMaterialization::Poisoned(record_name_ambiguous(
                diagnostics,
                span,
                &name.spelling(),
                candidates.iter().cloned(),
            )?)
        }
        NameLookupDecision::VisibilityViolation => {
            NameLookupMaterialization::Poisoned(record_visibility_violation(diagnostics, span)?)
        }
        NameLookupDecision::MissingCapabilityInput(capability) => {
            NameLookupMaterialization::MissingCapabilityInput(*capability)
        }
    })
}

/// Source role that established one lexical scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeKind {
    CallableParameters,
    LambdaParameters,
    LambdaBody,
    Block,
    ForBinding,
    MatchArm,
    ConstInitializer,
    FieldInitializer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScopeRecord {
    id: ScopeId,
    parent: Option<ScopeId>,
    kind: ScopeKind,
}

impl ScopeRecord {
    #[must_use]
    pub const fn id(self) -> ScopeId {
        self.id
    }

    #[must_use]
    pub const fn parent(self) -> Option<ScopeId> {
        self.parent
    }

    #[must_use]
    pub const fn kind(self) -> ScopeKind {
        self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBinding {
    id: LocalBindingId,
    scope: ScopeId,
    name: NameKey,
    origins: Box<[OriginId]>,
}

impl LocalBinding {
    #[must_use]
    pub const fn id(&self) -> LocalBindingId {
        self.id
    }

    #[must_use]
    pub const fn scope(&self) -> ScopeId {
        self.scope
    }

    #[must_use]
    pub const fn name(&self) -> &NameKey {
        &self.name
    }

    #[must_use]
    pub const fn origin(&self) -> OriginId {
        self.origins[0]
    }

    #[must_use]
    pub fn origins(&self) -> &[OriginId] {
        &self.origins
    }
}

/// Resolution facts and lexical identities established by this phase.
pub struct NameResolution {
    facts: FactTable<ResolutionKey, ResolvedName>,
    deferred: BTreeMap<ResolutionKey, NameLookupPlan>,
    scopes: Box<[ScopeRecord]>,
    locals: Box<[LocalBinding]>,
    local_by_origin: BTreeMap<(ModuleId, BodyId, OriginId), LocalBindingId>,
}

impl NameResolution {
    #[must_use]
    pub fn fact(&self, key: ResolutionKey) -> FactState<'_, ResolvedName> {
        self.facts.state(&key)
    }

    pub fn facts(
        &self,
    ) -> impl ExactSizeIterator<Item = (&ResolutionKey, &SemanticFact<ResolvedName>)> {
        self.facts.iter()
    }

    pub(crate) fn deferred(&self, key: ResolutionKey) -> Option<&NameLookupPlan> {
        self.deferred.get(&key)
    }

    /// Returns the number of source-name facts established by resolution.
    #[must_use]
    pub fn fact_count(&self) -> usize {
        self.facts.iter().len()
    }

    pub(crate) fn has_poisoned_facts(&self) -> bool {
        self.facts.has_poisoned()
    }

    #[must_use]
    pub fn scopes(&self) -> &[ScopeRecord] {
        &self.scopes
    }

    #[must_use]
    pub fn locals(&self) -> &[LocalBinding] {
        &self.locals
    }

    /// Returns the canonical local introduced at one source binding origin.
    #[must_use]
    pub fn local_binding(
        &self,
        module: ModuleId,
        body: BodyId,
        origin: OriginId,
    ) -> Option<LocalBindingId> {
        self.local_by_origin.get(&(module, body, origin)).copied()
    }

    pub(crate) fn pattern_bindings(
        &self,
        module: ModuleId,
        body: &Body,
        pattern: PatternId,
    ) -> Result<BTreeSet<LocalBindingId>, PatternBindingQueryError> {
        let mut bindings = BTreeSet::new();
        self.collect_pattern_bindings(module, body, pattern, &mut bindings)?;
        Ok(bindings)
    }

    fn collect_pattern_bindings(
        &self,
        module: ModuleId,
        body: &Body,
        pattern_id: PatternId,
        bindings: &mut BTreeSet<LocalBindingId>,
    ) -> Result<(), PatternBindingQueryError> {
        let pattern = body
            .pattern(pattern_id)
            .ok_or(PatternBindingQueryError::WrongBodyPattern(pattern_id))?;
        match pattern.kind() {
            PatternKind::Binding(name) => {
                bindings.insert(self.required_local_binding(module, body.id(), name.origin())?);
            }
            PatternKind::Alias { name, pattern } => {
                bindings.insert(self.required_local_binding(module, body.id(), name.origin())?);
                self.collect_pattern_bindings(module, body, *pattern, bindings)?;
            }
            PatternKind::Group(inner) => {
                self.collect_pattern_bindings(module, body, *inner, bindings)?;
            }
            PatternKind::Or(alternatives) => {
                for &alternative in alternatives.iter() {
                    self.collect_pattern_bindings(module, body, alternative, bindings)?;
                }
            }
            PatternKind::Tuple(elements) => {
                for element in elements.iter() {
                    if let PatternElement::Pattern(pattern) = element {
                        self.collect_pattern_bindings(module, body, *pattern, bindings)?;
                    }
                }
            }
            PatternKind::Variant { payload, .. } => match payload {
                eglur_hir::PatternPayload::Absent => {}
                eglur_hir::PatternPayload::Positional(elements) => {
                    for element in elements.iter() {
                        if let PatternElement::Pattern(pattern) = element {
                            self.collect_pattern_bindings(module, body, *pattern, bindings)?;
                        }
                    }
                }
                eglur_hir::PatternPayload::Named(fields) => {
                    for field in fields.iter() {
                        if let eglur_hir::FieldPattern::Field {
                            pattern: Some(pattern),
                            ..
                        } = field
                        {
                            self.collect_pattern_bindings(module, body, *pattern, bindings)?;
                        }
                    }
                }
            },
            PatternKind::Struct { fields, .. } => {
                for field in fields.iter() {
                    if let eglur_hir::FieldPattern::Field {
                        pattern: Some(pattern),
                        ..
                    } = field
                    {
                        self.collect_pattern_bindings(module, body, *pattern, bindings)?;
                    }
                }
            }
            PatternKind::Wildcard
            | PatternKind::Literal(_)
            | PatternKind::Unit
            | PatternKind::Invalid => {}
        }
        Ok(())
    }

    fn required_local_binding(
        &self,
        module: ModuleId,
        body: BodyId,
        origin: OriginId,
    ) -> Result<LocalBindingId, PatternBindingQueryError> {
        self.local_binding(module, body, origin).ok_or(
            PatternBindingQueryError::MissingLocalOrigin {
                module,
                body,
                origin,
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PatternBindingQueryError {
    WrongBodyPattern(PatternId),
    MissingLocalOrigin {
        module: ModuleId,
        body: BodyId,
        origin: OriginId,
    },
}

/// Resolves declaration and body names without performing downstream type semantics.
pub fn resolve_names(
    collection: &DeclarationCollection,
    imports: &ResolvedImports,
    sources: &[SemanticSource<'_>],
    support: &dyn CompileSupport,
    external: Option<&ExternalNameEnvironment>,
    diagnostics: &mut DiagnosticStore,
) -> Result<PhaseAnalysis<NameResolution>, NameResolutionError> {
    let mut source_map = BTreeMap::new();
    for source in sources {
        if source_map.insert(source.file(), source).is_some() {
            return Err(NameResolutionError::DuplicateSource(source.file()));
        }
    }
    let work = collection
        .modules()
        .map(|module| {
            source_map
                .get(&module.file())
                .copied()
                .map(|source| (module.id(), source))
                .ok_or(NameResolutionError::MissingSource(module.file()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut resolver = Resolver {
        collection,
        imports,
        external,
        diagnostics,
        recorder: DemandRecorder::new(support),
        facts: FactTable::new(),
        deferred: BTreeMap::new(),
        scopes: Vec::new(),
        locals: Vec::new(),
        local_by_origin: BTreeMap::new(),
        active_scopes: Vec::new(),
        current_owner: None,
        next_scope: BTreeMap::new(),
        next_local: BTreeMap::new(),
    };
    for (module, source) in work {
        if let Some(hir) = source.hir() {
            for &item in hir.roots() {
                resolver.visit_item(module, source, hir, item)?;
            }
        }
    }
    Ok(resolver.finish())
}

struct Resolver<'analysis, 'support> {
    collection: &'analysis DeclarationCollection,
    imports: &'analysis ResolvedImports,
    external: Option<&'analysis ExternalNameEnvironment>,
    diagnostics: &'analysis mut DiagnosticStore,
    recorder: DemandRecorder<'support>,
    facts: FactTable<ResolutionKey, ResolvedName>,
    deferred: BTreeMap<ResolutionKey, NameLookupPlan>,
    scopes: Vec<ScopeRecord>,
    locals: Vec<LocalBinding>,
    local_by_origin: BTreeMap<(ModuleId, BodyId, OriginId), LocalBindingId>,
    active_scopes: Vec<ActiveScope>,
    current_owner: Option<crate::DefId>,
    next_scope: BTreeMap<(ModuleId, BodyId), usize>,
    next_local: BTreeMap<(ModuleId, BodyId), usize>,
}

impl Resolver<'_, '_> {
    fn finish(self) -> PhaseAnalysis<NameResolution> {
        self.recorder
            .finish(PhaseProduct::Performed(NameResolution {
                facts: self.facts,
                deferred: self.deferred,
                scopes: self.scopes.into_boxed_slice(),
                locals: self.locals.into_boxed_slice(),
                local_by_origin: self.local_by_origin,
            }))
    }

    fn visit_item(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        item_id: ItemId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let previous_owner = self.current_owner;
        self.current_owner = self
            .collection
            .module(module)
            .and_then(|module| module.item_def(item_id))
            .or(previous_owner);
        let result = self.visit_item_contents(module, source, hir, item_id);
        self.current_owner = previous_owner;
        result
    }

    fn visit_item_contents(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        item_id: ItemId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let item = hir.item(item_id);
        let mut outcome = self.visit_attributes(source, item.attributes())?;
        outcome = outcome.merge(match item.kind() {
            ItemKind::Import(_) => VisitOutcome::Known,
            ItemKind::Function(function) => {
                let signatures = self
                    .visit_generics(module, source, hir, &function.generic_parameters)?
                    .merge(self.visit_optional_type(
                        module,
                        source,
                        hir,
                        function.return_type,
                    )?)?;
                let body = hir.body(function.body);
                let callable = self.visit_callable(
                    module,
                    source,
                    hir,
                    body,
                    &function.parameters,
                    match function.body_form {
                        FunctionBody::Absent => None,
                        FunctionBody::Expression(expression) | FunctionBody::Block(expression) => {
                            Some(expression)
                        }
                    },
                )?;
                signatures.merge(callable)?
            }
            ItemKind::Extern(function) => {
                let signatures =
                    self.visit_optional_type(module, source, hir, function.return_type)?;
                let body = hir.body(function.body);
                signatures.merge(self.visit_callable(
                    module,
                    source,
                    hir,
                    body,
                    &function.parameters,
                    None,
                )?)?
            }
            ItemKind::Trait(declaration) => {
                let mut nested =
                    self.visit_generics(module, source, hir, &declaration.generic_parameters)?;
                for &child in &declaration.items {
                    nested = nested.merge(self.visit_item(module, source, hir, child)?)?;
                }
                nested
            }
            ItemKind::Impl(declaration) => {
                let mut nested =
                    self.visit_generics(module, source, hir, &declaration.generic_parameters)?;
                nested = nested
                    .merge(self.visit_type(module, source, hir, declaration.trait_ref)?)?
                    .merge(self.visit_type(module, source, hir, declaration.target)?)?;
                for &child in &declaration.items {
                    nested = nested.merge(self.visit_item(module, source, hir, child)?)?;
                }
                nested
            }
            ItemKind::Struct(declaration) => {
                let mut nested =
                    self.visit_generics(module, source, hir, &declaration.generic_parameters)?;
                nested = nested.merge(self.visit_optional_type(
                    module,
                    source,
                    hir,
                    declaration.base,
                )?)?;
                for &child in &declaration.items {
                    nested = nested.merge(self.visit_item(module, source, hir, child)?)?;
                }
                nested
            }
            ItemKind::Enum(declaration) => {
                let mut nested =
                    self.visit_generics(module, source, hir, &declaration.generic_parameters)?;
                for variant in &declaration.variants {
                    nested = nested.merge(self.visit_attributes(source, &variant.attributes)?)?;
                    match &variant.payload {
                        eglur_hir::EnumVariantPayload::Absent => {}
                        eglur_hir::EnumVariantPayload::Tuple(types) => {
                            for &ty in types {
                                nested = nested.merge(self.visit_type(module, source, hir, ty)?)?;
                            }
                        }
                        eglur_hir::EnumVariantPayload::Named(fields) => {
                            for field in fields {
                                nested = nested
                                    .merge(self.visit_type(module, source, hir, field.ty)?)?;
                            }
                        }
                    }
                }
                nested
            }
            ItemKind::TypeAlias(declaration) => self
                .visit_generics(module, source, hir, &declaration.generic_parameters)?
                .merge(self.visit_type(module, source, hir, declaration.value)?)?,
            ItemKind::Const(declaration) => {
                let types = self.visit_type(module, source, hir, declaration.ty)?;
                let body = hir.body(declaration.body);
                self.push_scope(module, body.id(), ScopeKind::ConstInitializer)?;
                let value = self.visit_expr(module, source, hir, body, declaration.value)?;
                self.pop_scope();
                types.merge(value)?
            }
            ItemKind::AssociatedType(_) => VisitOutcome::Known,
            ItemKind::AssociatedTypeBinding(binding) => {
                self.visit_type(module, source, hir, binding.value)?
            }
            ItemKind::StorageField(field) => {
                let types = self.visit_type(module, source, hir, field.ty)?;
                if let Some(initializer) = field.initializer {
                    let body = hir.body(initializer.body);
                    self.push_scope(module, body.id(), ScopeKind::FieldInitializer)?;
                    let value =
                        self.visit_expr(module, source, hir, body, initializer.expression)?;
                    self.pop_scope();
                    types.merge(value)?
                } else {
                    types
                }
            }
            ItemKind::Property(property) => {
                let mut nested = self.visit_type(module, source, hir, property.ty)?;
                for &accessor in &property.accessors {
                    nested = nested.merge(self.visit_item(module, source, hir, accessor)?)?;
                }
                nested
            }
            ItemKind::Invalid => self.invalid_outcome(source, item.origin())?,
        })?;
        Ok(outcome)
    }

    fn visit_attributes(
        &mut self,
        source: &SemanticSource<'_>,
        attributes: &[eglur_hir::Attribute],
    ) -> Result<VisitOutcome, NameResolutionError> {
        let mut outcome = VisitOutcome::Known;
        for attribute in attributes {
            outcome = outcome.merge(self.visit_attribute_meta(source, &attribute.meta)?)?;
        }
        Ok(outcome)
    }

    fn visit_attribute_meta(
        &mut self,
        source: &SemanticSource<'_>,
        meta: &AttributeMeta,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let mut outcome = if meta.path.text().is_some() {
            self.demand_unimplemented(
                CompileCapability::Resolve(ResolveCapability::ResolveAttributePaths),
                SemanticLocation::origin(source.file(), meta.path.origin()),
            )?
        } else {
            self.invalid_path_outcome(source, &meta.path)?
        };
        for argument in &meta.arguments {
            if let Some(name) = &argument.name
                && !name.is_valid()
            {
                outcome = outcome.merge(self.invalid_outcome(source, name.origin())?)?;
            }
            match &argument.value {
                AttributeValue::Meta(nested) => {
                    outcome = outcome.merge(self.visit_attribute_meta(source, nested)?)?;
                }
                AttributeValue::Invalid(origin) => {
                    outcome = outcome.merge(self.invalid_outcome(source, *origin)?)?;
                }
                AttributeValue::Literal(_) => {}
            }
        }
        Ok(outcome)
    }

    fn visit_generics(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        parameters: &[eglur_hir::GenericParameter],
    ) -> Result<VisitOutcome, NameResolutionError> {
        let mut outcome = VisitOutcome::Known;
        for parameter in parameters {
            for &bound in &parameter.bounds {
                outcome = outcome.merge(self.visit_type(module, source, hir, bound)?)?;
            }
        }
        Ok(outcome)
    }

    fn visit_optional_type(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        ty: Option<TypeRefId>,
    ) -> Result<VisitOutcome, NameResolutionError> {
        ty.map_or(Ok(VisitOutcome::Known), |ty| {
            self.visit_type(module, source, hir, ty)
        })
    }

    fn visit_type(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        ty_id: TypeRefId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let ty = hir.type_ref(ty_id);
        match ty.kind() {
            TypeRefKind::Reference { referent, .. }
            | TypeRefKind::Pointer { referent, .. }
            | TypeRefKind::Dynamic {
                trait_ref: referent,
            }
            | TypeRefKind::Group(referent) => self.visit_type(module, source, hir, *referent),
            TypeRefKind::Path { path, arguments } => {
                let mut outcome = if path.text().is_some() {
                    VisitOutcome::Known
                } else {
                    self.invalid_path_outcome(source, path)?
                };
                for &argument in arguments {
                    outcome = outcome.merge(self.visit_type(module, source, hir, argument)?)?;
                }
                Ok(outcome)
            }
            TypeRefKind::Tuple(types) => {
                let mut outcome = VisitOutcome::Known;
                for &ty in types {
                    outcome = outcome.merge(self.visit_type(module, source, hir, ty)?)?;
                }
                Ok(outcome)
            }
            TypeRefKind::Function { parameters, result } => {
                let mut outcome = VisitOutcome::Known;
                for &parameter in parameters {
                    outcome = outcome.merge(self.visit_type(module, source, hir, parameter)?)?;
                }
                outcome.merge(self.visit_type(module, source, hir, *result)?)
            }
            TypeRefKind::Invalid => {
                let outcome = self.invalid_outcome(source, ty.origin())?;
                self.record_fact(ResolutionKey::Type { module, ty: ty_id }, outcome.clone())?;
                Ok(outcome)
            }
        }
    }

    fn visit_callable(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        hir: &HirFile,
        body: &Body,
        parameters: &[eglur_hir::Parameter],
        root: Option<ExprId>,
    ) -> Result<VisitOutcome, NameResolutionError> {
        self.push_scope(module, body.id(), ScopeKind::CallableParameters)?;
        let mut outcome = VisitOutcome::Known;
        for parameter in parameters {
            let group = self.pattern_bindings(source, body, parameter.pattern)?;
            if let Some(collision) = self.first_collision(&group) {
                outcome = outcome.merge(self.record_binding_collision(source, collision)?)?;
            } else {
                outcome = outcome.merge(self.bind_group(source, group, None)?)?;
            }
            outcome =
                outcome.merge(self.visit_optional_type(module, source, hir, parameter.ty)?)?;
        }
        if let Some(root) = root {
            outcome = outcome.merge(self.visit_expr(module, source, hir, body, root)?)?;
        }
        self.pop_scope();
        Ok(outcome)
    }

    fn visit_pattern_heads(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        body: &Body,
        pattern_id: PatternId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let pattern = body
            .pattern(pattern_id)
            .ok_or(NameResolutionError::WrongBodyPattern(pattern_id))?;
        let mut outcome = match pattern.kind() {
            PatternKind::Variant { path, .. } if path.is_unqualified_residual_constructor() => {
                let name = &path.segments()[0];
                let name_key =
                    NameKey::from_hir(name).expect("residual constructor has a valid name");
                let plan = self.plan_unqualified(
                    module,
                    &name_key,
                    UnqualifiedLookup::ResidualConstructor,
                )?;
                let outcome = self.materialize_lookup_plan(
                    source,
                    name.origin(),
                    LookupPurpose::Ordinary,
                    &plan,
                )?;
                self.record_fact(
                    ResolutionKey::Pattern {
                        module,
                        pattern: pattern_id,
                    },
                    outcome.clone(),
                )?;
                outcome
            }
            PatternKind::Variant {
                path,
                payload: eglur_hir::PatternPayload::Named(_),
            }
            | PatternKind::Struct {
                path: Some(path), ..
            } => self.resolve_path(
                module,
                source,
                path,
                PathLookup::BracedConstructor,
                ResolutionKey::Pattern {
                    module,
                    pattern: pattern_id,
                },
                LookupPurpose::Ordinary,
            )?,
            PatternKind::Variant { path, .. } => self.resolve_path(
                module,
                source,
                path,
                PathLookup::Namespace(Namespace::Value),
                ResolutionKey::Pattern {
                    module,
                    pattern: pattern_id,
                },
                LookupPurpose::Ordinary,
            )?,
            PatternKind::Invalid => self.invalid_outcome(source, pattern.origin())?,
            _ => VisitOutcome::Known,
        };
        match pattern.kind() {
            PatternKind::Or(patterns) => {
                for pattern in patterns {
                    outcome =
                        outcome.merge(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                }
            }
            PatternKind::Tuple(elements) => {
                for element in elements {
                    let PatternElement::Pattern(pattern) = element else {
                        continue;
                    };
                    outcome =
                        outcome.merge(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                }
            }
            PatternKind::Alias { pattern, .. } | PatternKind::Group(pattern) => {
                outcome =
                    outcome.merge(self.visit_pattern_heads(module, source, body, *pattern)?)?;
            }
            PatternKind::Variant { payload, .. } => match payload {
                eglur_hir::PatternPayload::Positional(elements) => {
                    for element in elements {
                        if let PatternElement::Pattern(pattern) = element {
                            outcome = outcome
                                .merge(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                        }
                    }
                }
                eglur_hir::PatternPayload::Named(fields) => {
                    for field in fields {
                        if let eglur_hir::FieldPattern::Field {
                            pattern: Some(pattern),
                            ..
                        } = field
                        {
                            outcome = outcome
                                .merge(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                        }
                    }
                }
                eglur_hir::PatternPayload::Absent => {}
            },
            PatternKind::Struct { fields, .. } => {
                for field in fields {
                    if let eglur_hir::FieldPattern::Field {
                        pattern: Some(pattern),
                        ..
                    } = field
                    {
                        outcome = outcome
                            .merge(self.visit_pattern_heads(module, source, body, *pattern)?)?;
                    }
                }
            }
            PatternKind::Binding(_)
            | PatternKind::Wildcard
            | PatternKind::Literal(_)
            | PatternKind::Unit
            | PatternKind::Invalid => {}
        }
        Ok(outcome)
    }

    fn resolve_name(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        name: &Name,
        namespace: Namespace,
        key: ResolutionKey,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let Some(name_key) = NameKey::from_hir(name) else {
            let outcome = self.invalid_outcome(source, name.origin())?;
            self.record_fact(key, outcome.clone())?;
            return Ok(outcome);
        };
        let outcome =
            self.lookup_unqualified(module, source, &name_key, namespace, name.origin(), purpose)?;
        self.record_fact(key, outcome.clone())?;
        Ok(outcome)
    }

    fn defer_candidate_name(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        body: &Body,
        expression: ExprId,
    ) -> Result<(), NameResolutionError> {
        let occurrence = body
            .expression(expression)
            .ok_or(NameResolutionError::WrongBodyExpression(expression))?;
        let ExprKind::Name(path) = occurrence.kind() else {
            return Err(NameResolutionError::WrongBodyExpression(expression));
        };
        let key = ResolutionKey::Expression { module, expression };
        let plan = if path.segments().len() == 1 {
            let name = &path.segments()[0];
            if let Some(name) = NameKey::from_hir(name) {
                self.plan_unqualified(module, &name, UnqualifiedLookup::Ordinary(Namespace::Value))?
            } else {
                let poison = source.invalid_poison(name.origin()).cloned().ok_or(
                    NameResolutionError::MissingInvalidPoison {
                        file: source.file(),
                        origin: name.origin(),
                    },
                )?;
                NameLookupPlan {
                    requirements: Box::new([]),
                    name: NameKey::new("invalid", false).expect("static fallback name is valid"),
                    decision: NameLookupDecision::Poisoned(poison),
                }
            }
        } else {
            return Err(NameResolutionError::WrongBodyExpression(expression));
        };
        if self.deferred.insert(key, plan).is_some() {
            return Err(NameResolutionError::DuplicateFact(DuplicateFact));
        }
        Ok(())
    }

    fn resolve_path(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        path: &Path,
        lookup: PathLookup,
        key: ResolutionKey,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let outcome = self.lookup_path(module, source, path, lookup, purpose)?;
        self.record_fact(key, outcome.clone())?;
        Ok(outcome)
    }

    fn lookup_path(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        path: &Path,
        lookup: PathLookup,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        if path.segments().len() == 1 {
            let name = &path.segments()[0];
            let Some(name_key) = NameKey::from_hir(name) else {
                return self.invalid_outcome(source, name.origin());
            };
            return self.lookup_unqualified(
                module,
                source,
                &name_key,
                lookup.direct_namespace(),
                name.origin(),
                purpose,
            );
        }
        let Some(segments) = path
            .segments()
            .iter()
            .map(NameKey::from_hir)
            .collect::<Option<Vec<_>>>()
        else {
            return self.invalid_path_outcome(source, path);
        };
        let capability = CompileCapability::Resolve(ResolveCapability::ResolveQualifiedValues);
        let location = SemanticLocation::origin(source.file(), path.origin());
        if !self.recorder.require(capability, location) {
            return Ok(VisitOutcome::Unperformed);
        }

        for prefix_len in (1..segments.len()).rev() {
            if segments[..prefix_len].iter().any(NameKey::is_bang) {
                continue;
            }
            let address = ModuleAddress::new(
                segments[..prefix_len]
                    .iter()
                    .map(|segment| Box::<str>::from(segment.text())),
            )
            .expect("qualified path prefixes are non-empty");
            if let Some(target_module) = self.collection.module_for_address(&address) {
                let outcome = self.lookup_source_module_path(
                    source,
                    target_module,
                    &segments[prefix_len..],
                    lookup,
                    path.origin(),
                    purpose,
                )?;
                return Ok(outcome);
            }
        }

        match self.lookup_external_module_path(source, &segments, lookup, path.origin(), purpose)? {
            ExternalModuleLookup::Matched(outcome) => {
                return Ok(outcome);
            }
            ExternalModuleLookup::NoModule => {}
        }

        if segments.len() == 2 && lookup.permits_variant() {
            let owner = self.lookup_unqualified(
                module,
                source,
                &segments[0],
                Namespace::Type,
                path.segments()[0].origin(),
                purpose,
            )?;
            let outcome = self.lookup_variant_from_owner(
                source,
                owner,
                &segments[1],
                path.origin(),
                purpose,
            )?;
            return Ok(outcome);
        }

        let name = segments
            .last()
            .expect("qualified path contains at least two segments");
        self.not_found(source, name, path.origin(), purpose)
    }

    fn lookup_unqualified(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        name: &NameKey,
        namespace: Namespace,
        origin: OriginId,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let plan = self.plan_unqualified(module, name, UnqualifiedLookup::Ordinary(namespace))?;
        self.materialize_lookup_plan(source, origin, purpose, &plan)
    }

    fn plan_unqualified(
        &self,
        module: ModuleId,
        name: &NameKey,
        lookup: UnqualifiedLookup,
    ) -> Result<NameLookupPlan, NameResolutionError> {
        let namespace = lookup.namespace();
        let mut requirements = Vec::with_capacity(3);
        if namespace == Namespace::Value {
            requirements.push(CompileCapability::Resolve(
                ResolveCapability::ResolveLexicalValues,
            ));
            for scope in self.active_scopes.iter().rev() {
                if let Some(barrier) = &scope.barrier {
                    return Ok(NameLookupPlan {
                        requirements: requirements.into_boxed_slice(),
                        name: name.clone(),
                        decision: Self::lookup_decision(barrier.outcome())?,
                    });
                }
                if matches!(lookup, UnqualifiedLookup::Ordinary(_))
                    && let Some(binding) = scope.bindings.get(name)
                {
                    return Ok(NameLookupPlan {
                        requirements: requirements.into_boxed_slice(),
                        name: name.clone(),
                        decision: Self::lookup_decision(binding.outcome())?,
                    });
                }
            }
        }

        requirements.push(CompileCapability::Resolve(
            ResolveCapability::ResolveModuleValues,
        ));
        let module_declarations = self
            .collection
            .module(module)
            .ok_or(NameResolutionError::UnknownModule(module))?;
        let decision = if let Some(poison) = module_declarations.declaration_poison() {
            NameLookupDecision::Poisoned(poison.clone())
        } else if namespace == Namespace::Type
            && matches!(lookup, UnqualifiedLookup::Ordinary(_))
            && let Some(target) = self.generic_type_binding(name)
        {
            NameLookupDecision::Resolved(ResolvedName::Declarations(Box::new([
                CanonicalTarget::Source(target),
            ])))
        } else {
            let candidates = module_declarations.candidates(namespace, name);
            if matches!(lookup, UnqualifiedLookup::Ordinary(_)) && !candidates.is_empty() {
                self.source_candidate_decision(candidates, false)
            } else if let Some(poison) = module_declarations.import_poison() {
                NameLookupDecision::Poisoned(poison.clone())
            } else if matches!(lookup, UnqualifiedLookup::Ordinary(_))
                && let Some(decision) = self.plan_imported_name(module, namespace, name, false)?
            {
                decision
            } else if namespace == Namespace::Type
                && let Some(builtin) = crate::type_namespace::builtin_prelude_binding(name)
            {
                NameLookupDecision::Resolved(ResolvedName::Declarations(Box::new([builtin])))
            } else {
                requirements.push(CompileCapability::Resolve(
                    ResolveCapability::ResolveTrustedExternalNames,
                ));
                match self.external {
                    Some(external) => {
                        let candidates = external.prelude_candidates(namespace, name).to_vec();
                        if candidates.len() == 1 {
                            NameLookupDecision::Resolved(ResolvedName::Declarations(Box::new([
                                CanonicalTarget::External(candidates[0].clone()),
                            ])))
                        } else if candidates.len() > 1
                            && Self::external_candidates_are_callable(external, &candidates)
                        {
                            NameLookupDecision::Resolved(ResolvedName::Declarations(
                                candidates
                                    .into_iter()
                                    .map(CanonicalTarget::External)
                                    .collect::<Vec<_>>()
                                    .into_boxed_slice(),
                            ))
                        } else if candidates.len() > 1 {
                            NameLookupDecision::Ambiguous(
                                candidates
                                    .iter()
                                    .map(|candidate| candidate.as_str().to_owned())
                                    .collect::<Vec<_>>()
                                    .into_boxed_slice(),
                            )
                        } else {
                            NameLookupDecision::NotFound
                        }
                    }
                    None => NameLookupDecision::MissingCapabilityInput(CompileCapability::Resolve(
                        ResolveCapability::ResolveTrustedExternalNames,
                    )),
                }
            }
        };
        Ok(NameLookupPlan {
            requirements: requirements.into_boxed_slice(),
            name: name.clone(),
            decision,
        })
    }

    fn generic_type_binding(&self, name: &NameKey) -> Option<crate::DefId> {
        let mut owner = self.current_owner;
        while let Some(current) = owner {
            let candidates =
                self.collection
                    .child_candidates(current, name, DeclarationKind::GenericParameter);
            // Signature validation diagnoses duplicates; they still occupy this tier.
            if let Some(parameter) = candidates.first() {
                return Some(*parameter);
            }
            owner = self.collection.declaration(current)?.parent();
        }
        None
    }

    fn lookup_imported_name(
        &mut self,
        module: ModuleId,
        source: &SemanticSource<'_>,
        namespace: Namespace,
        name: &NameKey,
        origin: OriginId,
        cross_module: bool,
    ) -> Result<Option<VisitOutcome>, NameResolutionError> {
        let Some(decision) = self.plan_imported_name(module, namespace, name, cross_module)? else {
            return Ok(None);
        };
        self.materialize_lookup_decision(source, origin, LookupPurpose::Ordinary, name, &decision)
            .map(Some)
    }

    fn plan_imported_name(
        &self,
        module: ModuleId,
        namespace: Namespace,
        name: &NameKey,
        cross_module: bool,
    ) -> Result<Option<NameLookupDecision>, NameResolutionError> {
        let imported = self.imports.lookup(module, namespace, name);
        if imported.is_empty() {
            return Ok(None);
        }
        let mut candidates = Vec::new();
        let mut poison: Option<Poison> = None;
        let mut unperformed = false;
        let mut private = false;
        for &import in imported {
            match self.imports.fact(import) {
                FactState::Known(bindings) => {
                    if let Some(binding) = bindings.iter().find(|binding| {
                        binding.namespace() == namespace && binding.local_name() == name
                    }) {
                        if cross_module && !binding.is_public() {
                            private = true;
                        } else {
                            candidates
                                .push((import, binding.targets().to_vec().into_boxed_slice()));
                        }
                    }
                }
                FactState::Poisoned(next) => {
                    poison = Some(match poison {
                        Some(existing) => existing.merge(next)?,
                        None => next.clone(),
                    });
                }
                FactState::Unperformed => unperformed = true,
                FactState::NotApplicable => {
                    return Err(NameResolutionError::InvalidImportFact);
                }
            }
        }
        if let Some(poison) = poison {
            return Ok(Some(NameLookupDecision::Poisoned(poison)));
        }
        if unperformed {
            return Ok(Some(NameLookupDecision::Unperformed));
        }
        if candidates.len() > 1 {
            return Ok(Some(NameLookupDecision::Ambiguous(
                candidates
                    .iter()
                    .map(|(candidate, _)| format!("{candidate:?}"))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )));
        }
        if let Some((_, targets)) = candidates.pop() {
            return Ok(Some(NameLookupDecision::Resolved(
                ResolvedName::Declarations(targets),
            )));
        }
        if private {
            return Ok(Some(NameLookupDecision::VisibilityViolation));
        }
        Ok(None)
    }

    fn lookup_source_module_name(
        &mut self,
        source: &SemanticSource<'_>,
        target_module: ModuleId,
        name: &NameKey,
        lookup: PathLookup,
        origin: OriginId,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let target = self
            .collection
            .module(target_module)
            .ok_or(NameResolutionError::UnknownModule(target_module))?;
        if let Some(poison) = target.declaration_poison() {
            return Ok(VisitOutcome::Poisoned(poison.clone()));
        }
        let current_module = self
            .collection
            .module_for_file(source.file())
            .ok_or(NameResolutionError::MissingSource(source.file()))?;
        let cross_module = current_module != target_module;
        let namespace = lookup.direct_namespace();
        let candidates = target.candidates(namespace, name);
        if !candidates.is_empty() {
            return self.select_source_candidates(
                source,
                name,
                origin,
                candidates,
                purpose,
                cross_module,
            );
        }
        if let Some(poison) = target.import_poison() {
            return Ok(VisitOutcome::Poisoned(poison.clone()));
        }
        if let Some(outcome) =
            self.lookup_imported_name(target_module, source, namespace, name, origin, cross_module)?
        {
            return Ok(outcome);
        }
        if matches!(lookup, PathLookup::BracedConstructor)
            && self.imported_name_may_be_variant(target_module, name)?
            && let Some(outcome) = self.lookup_imported_name(
                target_module,
                source,
                Namespace::Value,
                name,
                origin,
                cross_module,
            )?
        {
            return Ok(outcome);
        }
        self.not_found(source, name, origin, purpose)
    }

    fn imported_name_may_be_variant(
        &self,
        module: ModuleId,
        name: &NameKey,
    ) -> Result<bool, NameResolutionError> {
        let Some(decision) = self.plan_imported_name(module, Namespace::Value, name, false)? else {
            return Ok(false);
        };
        Ok(match decision {
            NameLookupDecision::Resolved(ResolvedName::Declarations(targets)) => {
                targets.iter().all(|target| {
                    let kind = match target {
                        CanonicalTarget::Primitive(_) => Some(DeclarationKind::PrimitiveType),
                        CanonicalTarget::Intrinsic(_) => Some(DeclarationKind::IntrinsicType),
                        CanonicalTarget::Source(id) => self
                            .collection
                            .declaration(*id)
                            .map(|declaration| declaration.kind()),
                        CanonicalTarget::External(id) => self
                            .external
                            .and_then(|external| external.declaration(id))
                            .map(|declaration| declaration.kind()),
                    };
                    kind.is_none_or(DeclarationKind::is_variant_constructor)
                })
            }
            NameLookupDecision::Resolved(ResolvedName::Local(_))
            | NameLookupDecision::Resolved(ResolvedName::TypeMemberOwner(_))
            | NameLookupDecision::NotApplicable
            | NameLookupDecision::NotFound => false,
            NameLookupDecision::Poisoned(_)
            | NameLookupDecision::Unperformed
            | NameLookupDecision::Ambiguous(_)
            | NameLookupDecision::VisibilityViolation
            | NameLookupDecision::MissingCapabilityInput(_) => true,
        })
    }

    fn lookup_source_module_path(
        &mut self,
        source: &SemanticSource<'_>,
        target_module: ModuleId,
        remainder: &[NameKey],
        lookup: PathLookup,
        origin: OriginId,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        if remainder.len() == 1 {
            return self.lookup_source_module_name(
                source,
                target_module,
                &remainder[0],
                lookup,
                origin,
                purpose,
            );
        }
        if remainder.len() == 2 && lookup.permits_variant() {
            let owner = self.lookup_source_module_name(
                source,
                target_module,
                &remainder[0],
                PathLookup::Namespace(Namespace::Type),
                origin,
                purpose,
            )?;
            return self.lookup_variant_from_owner(source, owner, &remainder[1], origin, purpose);
        }
        self.demand_unimplemented(
            CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
            SemanticLocation::origin(source.file(), origin),
        )
    }

    fn lookup_external_module_path(
        &mut self,
        source: &SemanticSource<'_>,
        segments: &[NameKey],
        lookup: PathLookup,
        origin: OriginId,
        purpose: LookupPurpose,
    ) -> Result<ExternalModuleLookup, NameResolutionError> {
        if let [root, module, name] = segments
            && root.text() == "std"
            && !root.is_bang()
            && module.text() == "prelude"
            && !module.is_bang()
            && let Some(builtin) = crate::type_namespace::builtin_prelude_binding(name)
        {
            if lookup.direct_namespace() == Namespace::Type {
                return Ok(ExternalModuleLookup::Matched(VisitOutcome::Resolved(
                    ResolvedName::Declarations(Box::new([builtin])),
                )));
            }
            let prelude =
                ModuleAddress::new([Box::<str>::from("std"), Box::<str>::from("prelude")])
                    .expect("builtin prelude address is non-empty");
            if !self.external.is_some_and(|external| {
                !lookup
                    .external_candidates(external, &prelude, name)
                    .is_empty()
            }) {
                return self
                    .not_found(source, name, origin, purpose)
                    .map(ExternalModuleLookup::Matched);
            }
        }
        let Some(external) = self.require_external(source, origin)? else {
            return Ok(ExternalModuleLookup::Matched(VisitOutcome::Unperformed));
        };
        for prefix_len in (1..segments.len()).rev() {
            if segments[..prefix_len].iter().any(NameKey::is_bang) {
                continue;
            }
            let module = ModuleAddress::new(
                segments[..prefix_len]
                    .iter()
                    .map(|segment| Box::<str>::from(segment.text())),
            )
            .expect("qualified path prefix is non-empty");
            if !external.has_module(&module) {
                continue;
            }
            if segments.len() - prefix_len != 1 {
                return self
                    .demand_unimplemented(
                        CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
                        SemanticLocation::origin(source.file(), origin),
                    )
                    .map(ExternalModuleLookup::Matched);
            }
            let name = &segments[prefix_len];
            let candidates = lookup.external_candidates(external, &module, name).to_vec();
            if candidates.len() == 1 {
                return Ok(ExternalModuleLookup::Matched(VisitOutcome::Resolved(
                    ResolvedName::Declarations(Box::new([CanonicalTarget::External(
                        candidates[0].clone(),
                    )])),
                )));
            }
            if candidates.len() > 1 && Self::external_candidates_are_callable(external, &candidates)
            {
                return Ok(ExternalModuleLookup::Matched(VisitOutcome::Resolved(
                    ResolvedName::Declarations(
                        candidates
                            .into_iter()
                            .map(CanonicalTarget::External)
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                )));
            }
            if candidates.len() > 1 {
                return self
                    .ambiguous(
                        source,
                        name,
                        origin,
                        candidates
                            .iter()
                            .map(|candidate| candidate.as_str().to_owned()),
                    )
                    .map(ExternalModuleLookup::Matched);
            }
            if !external.standard_library_is_complete()
                && module.components().next() == Some("std")
                && !external.has_module_name(&module, name)
            {
                return self
                    .demand_unimplemented(
                        CompileCapability::Resolve(
                            ResolveCapability::ResolveCompleteStandardLibraryNames,
                        ),
                        SemanticLocation::origin(source.file(), origin),
                    )
                    .map(ExternalModuleLookup::Matched);
            }
            return self
                .not_found(source, name, origin, purpose)
                .map(ExternalModuleLookup::Matched);
        }
        if !external.standard_library_is_complete()
            && segments
                .first()
                .is_some_and(|segment| !segment.is_bang() && segment.text() == "std")
        {
            return self
                .demand_unimplemented(
                    CompileCapability::Resolve(
                        ResolveCapability::ResolveCompleteStandardLibraryNames,
                    ),
                    SemanticLocation::origin(source.file(), origin),
                )
                .map(ExternalModuleLookup::Matched);
        }
        Ok(ExternalModuleLookup::NoModule)
    }

    fn lookup_variant_from_owner(
        &mut self,
        source: &SemanticSource<'_>,
        owner: VisitOutcome,
        variant: &NameKey,
        origin: OriginId,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let VisitOutcome::Resolved(ResolvedName::Declarations(owners)) = owner else {
            return Ok(owner);
        };
        if owners.len() != 1 {
            return self.ambiguous(
                source,
                variant,
                origin,
                owners.iter().map(|owner| format!("{owner:?}")),
            );
        }
        let CanonicalTarget::Source(owner) = owners[0] else {
            return self.demand_unimplemented(
                CompileCapability::Typecheck(TypecheckCapability::ResolveMembers),
                SemanticLocation::origin(source.file(), origin),
            );
        };
        let declaration = self
            .collection
            .declaration(owner)
            .ok_or(NameResolutionError::UnknownDeclaration(owner))?;
        if declaration.kind() != DeclarationKind::Enum {
            return self.not_found(source, variant, origin, purpose);
        }
        if let Some(poison) = self
            .collection
            .module(owner.module())
            .and_then(|module| module.owner_poison(owner))
        {
            return Ok(VisitOutcome::Poisoned(poison.clone()));
        }
        let candidates =
            self.collection
                .child_candidates(owner, variant, DeclarationKind::EnumVariant);
        if candidates.len() == 1 {
            return Ok(VisitOutcome::Resolved(ResolvedName::Declarations(
                Box::new([CanonicalTarget::Source(candidates[0])]),
            )));
        }
        if candidates.len() > 1 {
            return self.ambiguous(
                source,
                variant,
                origin,
                candidates.iter().map(|candidate| format!("{candidate:?}")),
            );
        }
        self.not_found(source, variant, origin, purpose)
    }

    fn select_source_candidates(
        &mut self,
        source: &SemanticSource<'_>,
        name: &NameKey,
        origin: OriginId,
        candidates: &[DefId],
        purpose: LookupPurpose,
        cross_module: bool,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let decision = self.source_candidate_decision(candidates, cross_module);
        self.materialize_lookup_decision(source, origin, purpose, name, &decision)
    }

    fn source_candidate_decision(
        &self,
        candidates: &[DefId],
        cross_module: bool,
    ) -> NameLookupDecision {
        if candidates.is_empty() {
            return NameLookupDecision::NotFound;
        }
        if cross_module
            && candidates.iter().any(|&candidate| {
                self.collection
                    .declaration(candidate)
                    .is_some_and(|declaration| !declaration.visibility().is_public())
            })
        {
            return NameLookupDecision::VisibilityViolation;
        }
        if candidates.len() > 1
            && !candidates.iter().all(|&candidate| {
                self.collection
                    .declaration(candidate)
                    .is_some_and(|declaration| declaration.kind().permits_overload_family())
            })
        {
            return NameLookupDecision::Ambiguous(
                candidates
                    .iter()
                    .map(|candidate| format!("{candidate:?}"))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
        }
        NameLookupDecision::Resolved(ResolvedName::Declarations(
            candidates
                .iter()
                .copied()
                .map(CanonicalTarget::Source)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ))
    }

    fn materialize_lookup_plan(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        purpose: LookupPurpose,
        plan: &NameLookupPlan,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let location = SemanticLocation::origin(source.file(), origin);
        for &capability in plan.requirements() {
            if !self.recorder.require(capability, location) {
                return Ok(VisitOutcome::Unperformed);
            }
        }
        self.materialize_lookup_decision(source, origin, purpose, plan.name(), plan.decision())
    }

    fn materialize_lookup_decision(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        purpose: LookupPurpose,
        name: &NameKey,
        decision: &NameLookupDecision,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let span = self.required_span(source, origin)?;
        match materialize_name_lookup(
            self.diagnostics,
            span,
            name,
            decision,
            purpose.suppresses_not_found(),
        )? {
            NameLookupMaterialization::Resolved(resolution) => {
                Ok(VisitOutcome::Resolved(resolution))
            }
            NameLookupMaterialization::Poisoned(poison) => Ok(VisitOutcome::Poisoned(poison)),
            NameLookupMaterialization::Unperformed => Ok(VisitOutcome::Unperformed),
            NameLookupMaterialization::NotApplicable => Ok(VisitOutcome::NotApplicable),
            NameLookupMaterialization::MissingCapabilityInput(capability) => {
                Err(NameResolutionError::MissingCapabilityInput(capability))
            }
        }
    }

    fn lookup_decision(outcome: VisitOutcome) -> Result<NameLookupDecision, NameResolutionError> {
        Ok(match outcome {
            VisitOutcome::Resolved(resolution) => NameLookupDecision::Resolved(resolution),
            VisitOutcome::Poisoned(poison) => NameLookupDecision::Poisoned(poison),
            VisitOutcome::Unperformed => NameLookupDecision::Unperformed,
            VisitOutcome::NotApplicable => NameLookupDecision::NotApplicable,
            VisitOutcome::Known => return Err(NameResolutionError::InvalidLookupPlanOutcome),
        })
    }

    fn ambiguous(
        &mut self,
        source: &SemanticSource<'_>,
        name: &NameKey,
        origin: OriginId,
        candidates: impl IntoIterator<Item = String>,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let poison = record_name_ambiguous(
            self.diagnostics,
            self.required_span(source, origin)?,
            &name.spelling(),
            candidates,
        )?;
        Ok(VisitOutcome::Poisoned(poison))
    }

    fn not_found(
        &mut self,
        source: &SemanticSource<'_>,
        name: &NameKey,
        origin: OriginId,
        purpose: LookupPurpose,
    ) -> Result<VisitOutcome, NameResolutionError> {
        if purpose.suppresses_not_found() {
            return Ok(VisitOutcome::NotApplicable);
        }
        let poison = record_name_not_found(
            self.diagnostics,
            self.required_span(source, origin)?,
            &name.spelling(),
        )?;
        Ok(VisitOutcome::Poisoned(poison))
    }

    fn require_external(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<Option<&ExternalNameEnvironment>, NameResolutionError> {
        let capability = CompileCapability::Resolve(ResolveCapability::ResolveTrustedExternalNames);
        let location = SemanticLocation::origin(source.file(), origin);
        if !self.recorder.require(capability, location) {
            return Ok(None);
        }
        self.external
            .map(Some)
            .ok_or(NameResolutionError::MissingCapabilityInput(capability))
    }

    fn demand_external(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        self.require_external(source, origin)
            .map(|external| external.map_or(VisitOutcome::Unperformed, |_| VisitOutcome::Known))
    }

    fn external_candidates_are_callable(
        external: &ExternalNameEnvironment,
        candidates: &[crate::ExternalDefId],
    ) -> bool {
        candidates.iter().all(|candidate| {
            external
                .declaration(candidate)
                .is_some_and(|declaration| declaration.kind().permits_overload_family())
        })
    }

    fn demand_unimplemented(
        &mut self,
        capability: CompileCapability,
        location: SemanticLocation,
    ) -> Result<VisitOutcome, NameResolutionError> {
        if self.recorder.require(capability, location) {
            Err(NameResolutionError::UnsupportedCapabilityClaim(capability))
        } else {
            Ok(VisitOutcome::Unperformed)
        }
    }

    fn pattern_bindings(
        &mut self,
        source: &SemanticSource<'_>,
        body: &Body,
        pattern: PatternId,
    ) -> Result<BindingGroup, NameResolutionError> {
        enum Step<'hir> {
            Pattern(PatternId),
            Name(&'hir Name),
        }

        let mut pending = vec![Step::Pattern(pattern)];
        let mut introductions = Vec::new();
        while let Some(step) = pending.pop() {
            let pattern = match step {
                Step::Name(name) => {
                    match self.binding_introduction(source, name)? {
                        BindingName::Known(introduction) => introductions.push(introduction),
                        BindingName::Poisoned(poison) => {
                            return Ok(BindingGroup::Poisoned(poison));
                        }
                    }
                    continue;
                }
                Step::Pattern(pattern) => body
                    .pattern(pattern)
                    .ok_or(NameResolutionError::WrongBodyPattern(pattern))?,
            };
            match pattern.kind() {
                PatternKind::Binding(name) => pending.push(Step::Name(name)),
                PatternKind::Alias { name, pattern } => {
                    pending.push(Step::Pattern(*pattern));
                    pending.push(Step::Name(name));
                }
                PatternKind::Group(pattern) => pending.push(Step::Pattern(*pattern)),
                PatternKind::Tuple(elements)
                | PatternKind::Variant {
                    payload: eglur_hir::PatternPayload::Positional(elements),
                    ..
                } => {
                    for element in elements.iter().rev() {
                        if let PatternElement::Pattern(pattern) = element {
                            pending.push(Step::Pattern(*pattern));
                        }
                    }
                }
                PatternKind::Or(patterns) => {
                    pending.extend(patterns.iter().rev().copied().map(Step::Pattern));
                }
                PatternKind::Struct { fields, .. }
                | PatternKind::Variant {
                    payload: eglur_hir::PatternPayload::Named(fields),
                    ..
                } => {
                    for field in fields.iter().rev() {
                        match field {
                            eglur_hir::FieldPattern::Field {
                                name,
                                pattern: None,
                                ..
                            } => pending.push(Step::Name(name)),
                            eglur_hir::FieldPattern::Field {
                                pattern: Some(pattern),
                                ..
                            } => pending.push(Step::Pattern(*pattern)),
                            eglur_hir::FieldPattern::Rest(_) => {}
                        }
                    }
                }
                PatternKind::Invalid => {
                    return Ok(BindingGroup::Poisoned(
                        source.invalid_poison(pattern.origin()).cloned().ok_or(
                            NameResolutionError::MissingInvalidPoison {
                                file: source.file(),
                                origin: pattern.origin(),
                            },
                        )?,
                    ));
                }
                PatternKind::Wildcard
                | PatternKind::Literal(_)
                | PatternKind::Unit
                | PatternKind::Variant {
                    payload: eglur_hir::PatternPayload::Absent,
                    ..
                } => {}
            }
        }
        Ok(BindingGroup::known(introductions))
    }

    fn single_name_group(
        &self,
        source: &SemanticSource<'_>,
        name: &Name,
    ) -> Result<BindingGroup, NameResolutionError> {
        Ok(match self.binding_introduction(source, name)? {
            BindingName::Known(introduction) => BindingGroup::known(vec![introduction]),
            BindingName::Poisoned(poison) => BindingGroup::Poisoned(poison),
        })
    }

    fn binding_introduction(
        &self,
        source: &SemanticSource<'_>,
        name: &Name,
    ) -> Result<BindingName, NameResolutionError> {
        let Some(name_key) = NameKey::from_hir(name) else {
            return Ok(BindingName::Poisoned(
                source.invalid_poison(name.origin()).cloned().ok_or(
                    NameResolutionError::MissingInvalidPoison {
                        file: source.file(),
                        origin: name.origin(),
                    },
                )?,
            ));
        };
        Ok(BindingName::Known(BindingIntroduction {
            name: name_key,
            origins: vec![name.origin()],
        }))
    }

    fn first_collision(&self, group: &BindingGroup) -> Option<BindingCollision> {
        let BindingGroup::Known(introductions) = group else {
            return None;
        };
        let scope = self.active_scopes.last()?;
        introductions.iter().find_map(|introduction| {
            scope
                .bindings
                .get(&introduction.name)
                .map(|existing| BindingCollision {
                    name: introduction.name.clone(),
                    first: existing.origin,
                    second: introduction.origin(),
                })
        })
    }

    fn record_binding_collision(
        &mut self,
        source: &SemanticSource<'_>,
        collision: BindingCollision,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let poison = record_local_redeclaration(
            self.diagnostics,
            self.required_span(source, collision.second)?,
            self.required_span(source, collision.first)?,
            &collision.name.spelling(),
        )?;
        Ok(VisitOutcome::Poisoned(poison))
    }

    fn bind_group(
        &mut self,
        _source: &SemanticSource<'_>,
        group: BindingGroup,
        initializer: Option<VisitOutcome>,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let state = match (&group, initializer.as_ref()) {
            (BindingGroup::Known(_), Some(VisitOutcome::Poisoned(poison))) => {
                BindingActivation::Poisoned(poison.clone())
            }
            (BindingGroup::Known(_), Some(VisitOutcome::Unperformed)) => {
                BindingActivation::Unperformed
            }
            (BindingGroup::Known(_), _) => BindingActivation::Known,
            (BindingGroup::Poisoned(poison), _) => BindingActivation::Poisoned(poison.clone()),
        };
        let introductions = match group {
            BindingGroup::Known(introductions) => introductions,
            BindingGroup::Poisoned(poison) => {
                self.set_scope_barrier(Some(ScopeBarrier::Poisoned(poison.clone())))?;
                return Ok(VisitOutcome::Poisoned(poison));
            }
        };
        for introduction in introductions {
            self.activate_binding(introduction, state.clone())?;
        }
        Ok(state.outcome())
    }

    fn activate_binding(
        &mut self,
        introduction: BindingIntroduction,
        state: BindingActivation,
    ) -> Result<(), NameResolutionError> {
        let scope = self
            .active_scopes
            .last()
            .ok_or(NameResolutionError::MissingActiveScope)?
            .id;
        if self
            .active_scopes
            .last()
            .is_some_and(|active| active.bindings.contains_key(&introduction.name))
        {
            return Err(NameResolutionError::UncheckedLocalCollision);
        }
        let id = self.allocate_local(scope)?;
        let origin = introduction.origin();
        let binding = LocalBinding {
            id,
            scope,
            name: introduction.name.clone(),
            origins: introduction.origins.clone().into_boxed_slice(),
        };
        for &origin in &introduction.origins {
            if self
                .local_by_origin
                .insert((id.module(), id.body(), origin), id)
                .is_some()
            {
                return Err(NameResolutionError::DuplicateLocalOrigin {
                    module: id.module(),
                    body: id.body(),
                    origin,
                });
            }
        }
        self.locals.push(binding);
        let active = self
            .active_scopes
            .last_mut()
            .ok_or(NameResolutionError::MissingActiveScope)?;
        if active
            .bindings
            .insert(introduction.name, ActiveBinding { id, origin, state })
            .is_some()
        {
            return Err(NameResolutionError::UncheckedLocalCollision);
        }
        Ok(())
    }

    fn push_scope(
        &mut self,
        module: ModuleId,
        body: BodyId,
        kind: ScopeKind,
    ) -> Result<(), NameResolutionError> {
        let next = self.next_scope.entry((module, body)).or_default();
        let id = ScopeId::from_index(module, body, *next)?;
        *next += 1;
        let parent = self.active_scopes.last().map(|scope| scope.id);
        self.scopes.push(ScopeRecord { id, parent, kind });
        self.active_scopes.push(ActiveScope {
            id,
            bindings: BTreeMap::new(),
            barrier: None,
        });
        Ok(())
    }

    fn pop_scope(&mut self) {
        self.active_scopes
            .pop()
            .expect("scope traversal pops exactly one active scope");
    }

    fn allocate_local(&mut self, scope: ScopeId) -> Result<LocalBindingId, NameResolutionError> {
        let key = (scope.module(), scope.body());
        let next = self.next_local.entry(key).or_default();
        let id = LocalBindingId::from_index(scope.module(), scope.body(), *next)?;
        *next += 1;
        Ok(id)
    }

    fn set_scope_barrier(
        &mut self,
        barrier: Option<ScopeBarrier>,
    ) -> Result<(), NameResolutionError> {
        let Some(barrier) = barrier else {
            return Ok(());
        };
        let scope = self
            .active_scopes
            .last_mut()
            .ok_or(NameResolutionError::MissingActiveScope)?;
        scope.barrier = Some(match (&scope.barrier, barrier) {
            (Some(ScopeBarrier::Poisoned(left)), ScopeBarrier::Poisoned(right)) => {
                ScopeBarrier::Poisoned(left.merge(&right)?)
            }
            (Some(ScopeBarrier::Poisoned(poison)), _) => ScopeBarrier::Poisoned(poison.clone()),
            (_, ScopeBarrier::Poisoned(poison)) => ScopeBarrier::Poisoned(poison),
            _ => ScopeBarrier::Unperformed,
        });
        Ok(())
    }

    fn invalid_outcome(
        &self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<VisitOutcome, NameResolutionError> {
        source
            .invalid_poison(origin)
            .cloned()
            .map(VisitOutcome::Poisoned)
            .ok_or(NameResolutionError::MissingInvalidPoison {
                file: source.file(),
                origin,
            })
    }

    fn invalid_path_outcome(
        &self,
        source: &SemanticSource<'_>,
        path: &Path,
    ) -> Result<VisitOutcome, NameResolutionError> {
        let mut poison: Option<Poison> = None;
        for name in path.segments().iter().filter(|name| !name.is_valid()) {
            let next = source.invalid_poison(name.origin()).ok_or(
                NameResolutionError::MissingInvalidPoison {
                    file: source.file(),
                    origin: name.origin(),
                },
            )?;
            poison = Some(match poison {
                Some(existing) => existing.merge(next)?,
                None => next.clone(),
            });
        }
        poison
            .map(VisitOutcome::Poisoned)
            .ok_or(NameResolutionError::MissingInvalidPoison {
                file: source.file(),
                origin: path.origin(),
            })
    }

    fn record_fact(
        &mut self,
        key: ResolutionKey,
        outcome: VisitOutcome,
    ) -> Result<(), NameResolutionError> {
        match outcome {
            VisitOutcome::Resolved(value) => {
                self.facts.insert(key, SemanticFact::Known(value))?;
            }
            VisitOutcome::Poisoned(poison) => {
                self.facts.insert(key, SemanticFact::Poisoned(poison))?;
            }
            VisitOutcome::NotApplicable => {
                self.facts.insert(key, SemanticFact::NotApplicable)?;
            }
            VisitOutcome::Known | VisitOutcome::Unperformed => {}
        }
        Ok(())
    }

    fn required_span(
        &self,
        source: &SemanticSource<'_>,
        origin: OriginId,
    ) -> Result<SourceSpan, NameResolutionError> {
        source
            .origin_span(origin)
            .cloned()
            .ok_or(NameResolutionError::MissingOriginSpan {
                file: source.file(),
                origin,
            })
    }
}

#[derive(Clone)]
enum VisitOutcome {
    Known,
    Resolved(ResolvedName),
    Poisoned(Poison),
    Unperformed,
    NotApplicable,
}

impl VisitOutcome {
    fn merge(self, other: Self) -> Result<Self, NameResolutionError> {
        match (self, other) {
            (Self::Poisoned(left), Self::Poisoned(right)) => {
                Ok(Self::Poisoned(left.merge(&right)?))
            }
            (Self::Poisoned(poison), _) | (_, Self::Poisoned(poison)) => Ok(Self::Poisoned(poison)),
            (Self::Unperformed, _) | (_, Self::Unperformed) => Ok(Self::Unperformed),
            (Self::NotApplicable, value) | (value, Self::NotApplicable) => Ok(value),
            (Self::Resolved(_), Self::Resolved(_)) => Ok(Self::Known),
            (Self::Resolved(_), Self::Known) | (Self::Known, Self::Resolved(_)) => Ok(Self::Known),
            (Self::Known, Self::Known) => Ok(Self::Known),
        }
    }
}

#[derive(Clone)]
enum ScopeBarrier {
    Poisoned(Poison),
    Unperformed,
}

impl ScopeBarrier {
    fn outcome(&self) -> VisitOutcome {
        match self {
            Self::Poisoned(poison) => VisitOutcome::Poisoned(poison.clone()),
            Self::Unperformed => VisitOutcome::Unperformed,
        }
    }

    fn from_outcome(outcome: &VisitOutcome) -> Option<Self> {
        match outcome {
            VisitOutcome::Poisoned(poison) => Some(Self::Poisoned(poison.clone())),
            VisitOutcome::Unperformed => Some(Self::Unperformed),
            _ => None,
        }
    }
}

struct ActiveScope {
    id: ScopeId,
    bindings: BTreeMap<NameKey, ActiveBinding>,
    barrier: Option<ScopeBarrier>,
}

struct ActiveBinding {
    id: LocalBindingId,
    origin: OriginId,
    state: BindingActivation,
}

impl ActiveBinding {
    fn outcome(&self) -> VisitOutcome {
        match &self.state {
            BindingActivation::Known => VisitOutcome::Resolved(ResolvedName::Local(self.id)),
            BindingActivation::Poisoned(poison) => VisitOutcome::Poisoned(poison.clone()),
            BindingActivation::Unperformed => VisitOutcome::Unperformed,
        }
    }
}

#[derive(Clone)]
enum BindingActivation {
    Known,
    Poisoned(Poison),
    Unperformed,
}

impl BindingActivation {
    fn outcome(&self) -> VisitOutcome {
        match self {
            Self::Known => VisitOutcome::Known,
            Self::Poisoned(poison) => VisitOutcome::Poisoned(poison.clone()),
            Self::Unperformed => VisitOutcome::Unperformed,
        }
    }
}

struct BindingIntroduction {
    name: NameKey,
    origins: Vec<OriginId>,
}

impl BindingIntroduction {
    fn origin(&self) -> OriginId {
        self.origins[0]
    }
}

enum BindingName {
    Known(BindingIntroduction),
    Poisoned(Poison),
}

enum BindingGroup {
    Known(Vec<BindingIntroduction>),
    Poisoned(Poison),
}

impl BindingGroup {
    fn known(introductions: Vec<BindingIntroduction>) -> Self {
        // Name resolution shares one identity; typed pattern validation decides
        // whether repeated occurrences are alternatives or illegal duplicates.
        let mut canonical = Vec::<BindingIntroduction>::new();
        let mut positions = BTreeMap::<NameKey, usize>::new();
        for introduction in introductions {
            #[cfg(test)]
            tests::record_origin_visits(introduction.origins.len());

            if let Some(&index) = positions.get(&introduction.name) {
                canonical[index].origins.extend(introduction.origins);
            } else {
                positions.insert(introduction.name.clone(), canonical.len());
                canonical.push(introduction);
            }
        }
        Self::Known(canonical)
    }
}

struct BindingCollision {
    name: NameKey,
    first: OriginId,
    second: OriginId,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LookupPurpose {
    Ordinary,
    AssignmentTarget,
    Probe,
}

impl LookupPurpose {
    const fn suppresses_not_found(self) -> bool {
        !matches!(self, Self::Ordinary)
    }
}

#[derive(Clone, Copy)]
enum UnqualifiedLookup {
    Ordinary(Namespace),
    ResidualConstructor,
}

impl UnqualifiedLookup {
    const fn namespace(self) -> Namespace {
        match self {
            Self::Ordinary(namespace) => namespace,
            Self::ResidualConstructor => Namespace::Value,
        }
    }
}

#[derive(Clone, Copy)]
enum PathLookup {
    Namespace(Namespace),
    BracedConstructor,
}

impl PathLookup {
    const fn direct_namespace(self) -> Namespace {
        match self {
            Self::Namespace(namespace) => namespace,
            Self::BracedConstructor => Namespace::Type,
        }
    }

    const fn permits_variant(self) -> bool {
        matches!(
            self,
            Self::Namespace(Namespace::Value) | Self::BracedConstructor
        )
    }

    fn external_candidates<'environment>(
        self,
        external: &'environment ExternalNameEnvironment,
        module: &ModuleAddress,
        name: &NameKey,
    ) -> &'environment [ExternalDefId] {
        let direct = external.module_candidates(module, self.direct_namespace(), name);
        if !matches!(self, Self::BracedConstructor) || !direct.is_empty() {
            return direct;
        }

        let variants = external.module_candidates(module, Namespace::Value, name);
        if variants.iter().all(|id| {
            external
                .declaration(id)
                .is_some_and(|declaration| declaration.kind().is_variant_constructor())
        }) {
            variants
        } else {
            direct
        }
    }
}

enum ExternalModuleLookup {
    Matched(VisitOutcome),
    NoModule,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NameResolutionError {
    DuplicateSource(FileId),
    MissingSource(FileId),
    UnknownModule(ModuleId),
    UnknownDeclaration(DefId),
    WrongBodyExpression(ExprId),
    WrongBodyStatement(StmtId),
    WrongBodyPattern(PatternId),
    DuplicateLocalOrigin {
        module: ModuleId,
        body: BodyId,
        origin: OriginId,
    },
    MissingOriginSpan {
        file: FileId,
        origin: OriginId,
    },
    MissingInvalidPoison {
        file: FileId,
        origin: OriginId,
    },
    MissingCapabilityInput(CompileCapability),
    UnsupportedCapabilityClaim(CompileCapability),
    InvalidImportFact,
    InvalidLookupPlanOutcome,
    MissingActiveScope,
    UncheckedLocalCollision,
    IdentityOverflow(IdentityOverflow),
    DuplicateFact(DuplicateFact),
    DiagnosticRoot(eglur_diagnostics::DiagnosticRootError),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for NameResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSource(file) => write!(formatter, "duplicate semantic source {file}"),
            Self::MissingSource(file) => write!(formatter, "missing semantic source {file}"),
            Self::UnknownModule(module) => write!(formatter, "unknown semantic module {module:?}"),
            Self::UnknownDeclaration(declaration) => {
                write!(formatter, "unknown semantic declaration {declaration:?}")
            }
            Self::WrongBodyExpression(expression) => {
                write!(
                    formatter,
                    "expression {expression:?} belongs to another body"
                )
            }
            Self::WrongBodyStatement(statement) => {
                write!(formatter, "statement {statement:?} belongs to another body")
            }
            Self::WrongBodyPattern(pattern) => {
                write!(formatter, "pattern {pattern:?} belongs to another body")
            }
            Self::DuplicateLocalOrigin {
                module,
                body,
                origin,
            } => write!(
                formatter,
                "module {module:?} body {body:?} repeats local origin {}",
                origin.get()
            ),
            Self::MissingOriginSpan { file, origin } => write!(
                formatter,
                "semantic source {file} omitted origin span {}",
                origin.get()
            ),
            Self::MissingInvalidPoison { file, origin } => write!(
                formatter,
                "invalid HIR origin {} in {file} has no retained poison",
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
            Self::InvalidImportFact => {
                formatter.write_str("name lookup encountered not-applicable import fact")
            }
            Self::InvalidLookupPlanOutcome => {
                formatter.write_str("name lookup planning produced no name-resolution fact")
            }
            Self::MissingActiveScope => {
                formatter.write_str("local binding activation has no lexical scope")
            }
            Self::UncheckedLocalCollision => {
                formatter.write_str("local binding collision bypassed precedence validation")
            }
            Self::IdentityOverflow(error) => error.fmt(formatter),
            Self::DuplicateFact(error) => error.fmt(formatter),
            Self::DiagnosticRoot(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for NameResolutionError {}

impl From<IdentityOverflow> for NameResolutionError {
    fn from(error: IdentityOverflow) -> Self {
        Self::IdentityOverflow(error)
    }
}

impl From<DuplicateFact> for NameResolutionError {
    fn from(error: DuplicateFact) -> Self {
        Self::DuplicateFact(error)
    }
}

impl From<DiagnosticStoreError> for NameResolutionError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

impl From<SemanticDiagnosticError> for NameResolutionError {
    fn from(error: SemanticDiagnosticError) -> Self {
        match error {
            SemanticDiagnosticError::Root(error) => Self::DiagnosticRoot(error),
            SemanticDiagnosticError::Store(error) => Self::DiagnosticStore(error),
        }
    }
}
