use eglur_diagnostics::{DiagnosticStoreError, Poison};

use crate::{
    CanonicalTarget, DeclarationCollection, ExternalNameEnvironment, FactState, ImportId,
    ImportedBinding, ModuleAddress, ModuleDeclarations, NameKey, Namespace, ResolvedImports,
};

#[derive(Debug)]
pub(crate) enum TypeNameLookupError {
    InvalidImportFact(ImportId),
    DiagnosticStore(DiagnosticStoreError),
}

impl From<DiagnosticStoreError> for TypeNameLookupError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

/// Complete result of a bounded lookup in the type namespace.
pub(crate) enum TypeNameLookup {
    Unique(CanonicalTarget),
    Ambiguous(Box<[CanonicalTarget]>),
    Poisoned(Poison),
    Unperformed,
    VisibilityViolation,
    NotFound,
}

/// Resolves one type name from a known source module without searching for modules.
pub(crate) fn lookup_source_module_type(
    collection: &DeclarationCollection,
    imports: &ResolvedImports,
    module: &ModuleDeclarations,
    name: &NameKey,
    cross_module: bool,
) -> Result<TypeNameLookup, TypeNameLookupError> {
    if let Some(poison) = module.declaration_poison() {
        return Ok(TypeNameLookup::Poisoned(poison.clone()));
    }

    let declarations = module.candidates(Namespace::Type, name);
    if !declarations.is_empty() {
        if cross_module
            && declarations.iter().any(|&candidate| {
                collection
                    .declaration(candidate)
                    .is_some_and(|declaration| !declaration.visibility().is_public())
            })
        {
            return Ok(TypeNameLookup::VisibilityViolation);
        }
        return Ok(match declarations {
            [declaration] => TypeNameLookup::Unique(CanonicalTarget::Source(*declaration)),
            _ => TypeNameLookup::Ambiguous(
                declarations
                    .iter()
                    .copied()
                    .map(CanonicalTarget::Source)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        });
    }

    if let Some(poison) = module.import_poison() {
        return Ok(TypeNameLookup::Poisoned(poison.clone()));
    }

    lookup_imported_type(imports, module, name, cross_module)
}

fn lookup_imported_type(
    imports: &ResolvedImports,
    module: &ModuleDeclarations,
    name: &NameKey,
    cross_module: bool,
) -> Result<TypeNameLookup, TypeNameLookupError> {
    let mut candidates = Vec::<&ImportedBinding>::new();
    let mut poison = None;
    let mut unperformed = false;
    let mut private = false;

    for &import in imports.lookup(module.id(), Namespace::Type, name) {
        match imports.fact(import) {
            FactState::Known(bindings) => {
                for binding in bindings.iter().filter(|binding| {
                    binding.namespace() == Namespace::Type && binding.local_name() == name
                }) {
                    if cross_module && !binding.is_public() {
                        private = true;
                    } else {
                        candidates.push(binding);
                    }
                }
            }
            FactState::Poisoned(found) => merge_poison(&mut poison, found)?,
            FactState::Unperformed => unperformed = true,
            FactState::NotApplicable => {
                return Err(TypeNameLookupError::InvalidImportFact(import));
            }
        }
    }

    if let Some(poison) = poison {
        return Ok(TypeNameLookup::Poisoned(poison));
    }
    if unperformed {
        return Ok(TypeNameLookup::Unperformed);
    }
    if candidates.len() > 1 {
        return Ok(TypeNameLookup::Ambiguous(
            candidates
                .into_iter()
                .flat_map(ImportedBinding::targets)
                .cloned()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ));
    }
    if let Some(binding) = candidates.pop() {
        return Ok(match binding.targets() {
            [target] => TypeNameLookup::Unique(target.clone()),
            targets => TypeNameLookup::Ambiguous(targets.to_vec().into_boxed_slice()),
        });
    }
    if private {
        return Ok(TypeNameLookup::VisibilityViolation);
    }
    Ok(TypeNameLookup::NotFound)
}

pub(crate) fn lookup_external_module_type(
    external: Option<&ExternalNameEnvironment>,
    module: &ModuleAddress,
    name: &NameKey,
) -> TypeNameLookup {
    if module.as_str() == "std::prelude"
        && let Some(builtin) = builtin_prelude_binding(name)
    {
        return TypeNameLookup::Unique(builtin);
    }
    external.map_or(TypeNameLookup::Unperformed, |external| {
        external_lookup(
            external.module_candidates(module, Namespace::Type, name),
            CanonicalTarget::External,
        )
    })
}

pub(crate) fn lookup_prelude_type(
    external: Option<&ExternalNameEnvironment>,
    name: &NameKey,
) -> TypeNameLookup {
    if let Some(builtin) = builtin_prelude_binding(name) {
        return TypeNameLookup::Unique(builtin);
    }
    external.map_or(TypeNameLookup::Unperformed, |external| {
        external_lookup(
            external.prelude_candidates(Namespace::Type, name),
            CanonicalTarget::External,
        )
    })
}

pub(crate) fn primitive_prelude_binding(name: &NameKey) -> Option<crate::PrimitiveType> {
    (!name.is_bang())
        .then(|| crate::PrimitiveType::from_spelling(name.text()))
        .flatten()
}

pub(crate) fn builtin_prelude_binding(name: &NameKey) -> Option<CanonicalTarget> {
    if name.is_bang() {
        return None;
    }
    primitive_prelude_binding(name)
        .map(CanonicalTarget::Primitive)
        .or_else(|| {
            crate::IntrinsicNominal::from_spelling(name.text()).map(CanonicalTarget::Intrinsic)
        })
}

fn external_lookup<T: Clone>(
    candidates: &[T],
    wrap: impl Fn(T) -> CanonicalTarget,
) -> TypeNameLookup {
    match candidates {
        [] => TypeNameLookup::NotFound,
        [candidate] => TypeNameLookup::Unique(wrap(candidate.clone())),
        _ => TypeNameLookup::Ambiguous(
            candidates
                .iter()
                .cloned()
                .map(wrap)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
    }
}

fn merge_poison(
    retained: &mut Option<Poison>,
    incoming: &Poison,
) -> Result<(), DiagnosticStoreError> {
    *retained = Some(match retained.take() {
        Some(existing) => existing.merge(incoming)?,
        None => incoming.clone(),
    });
    Ok(())
}
