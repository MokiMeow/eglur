use std::collections::{BTreeMap, BTreeSet};

use eglur_diagnostics::Poison;

use super::{DeclarationSignature, DeclarationSignatures, TypeOccurrenceId};
use crate::{
    CanonicalTarget, DeclarationCollection, DefId, ExternalDefId, FactState, GenericParameterId,
    SemanticFact, TraitObligation, TypeId, TypeStoreError,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum TypeApplicationSite {
    Source {
        owner: DefId,
        occurrence: TypeOccurrenceId,
        region: TypeApplicationRegion,
    },
    Trusted(ExternalDefId),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum TypeApplicationRegion {
    Declaration,
    Body,
}

impl TypeApplicationSite {
    pub(crate) fn owner(&self) -> CanonicalTarget {
        match self {
            Self::Source { owner, .. } => CanonicalTarget::Source(*owner),
            Self::Trusted(owner) => CanonicalTarget::External(owner.clone()),
        }
    }

    pub(crate) fn is_body(&self) -> bool {
        matches!(
            self,
            Self::Source {
                region: TypeApplicationRegion::Body,
                ..
            }
        )
    }
}

// Written applications survive transparent alias expansion; canonical types do not retain them.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TypeApplication {
    site: TypeApplicationSite,
    target: CanonicalTarget,
    arguments: Box<[TypeId]>,
}

impl TypeApplication {
    pub(super) fn new(
        site: TypeApplicationSite,
        target: CanonicalTarget,
        arguments: Vec<TypeId>,
    ) -> Self {
        Self {
            site,
            target,
            arguments: arguments.into_boxed_slice(),
        }
    }

    pub(crate) const fn site(&self) -> &TypeApplicationSite {
        &self.site
    }

    pub(crate) const fn target(&self) -> &CanonicalTarget {
        &self.target
    }

    pub(crate) fn arguments(&self) -> &[TypeId] {
        &self.arguments
    }
}

#[derive(Default)]
pub(super) struct TypeApplicationFacts {
    pub(super) applications: BTreeSet<TypeApplication>,
    poisons: BTreeMap<CanonicalTarget, Poison>,
}

impl DeclarationSignatures {
    pub(crate) fn type_applications(&self) -> impl Iterator<Item = &TypeApplication> {
        self.type_applications.applications.iter()
    }

    pub(super) fn application_poison(&self, target: &CanonicalTarget) -> Option<&Poison> {
        self.type_applications.poisons.get(target)
    }

    pub(super) fn has_invalid_applications(&self) -> bool {
        !self.type_applications.poisons.is_empty()
    }

    pub(crate) fn poison_type_application_owner(
        &mut self,
        owner: CanonicalTarget,
        poison: Poison,
    ) -> bool {
        if self.type_applications.poisons.contains_key(&owner) {
            return false;
        }
        self.type_applications.poisons.insert(owner, poison);
        true
    }

    pub(crate) fn type_parameters(
        &self,
        target: &CanonicalTarget,
    ) -> FactState<'_, Box<[GenericParameterId]>> {
        match self.declaration(target) {
            FactState::Known(signature) => match signature {
                DeclarationSignature::Trait {
                    generic_parameters, ..
                }
                | DeclarationSignature::Struct {
                    generic_parameters, ..
                }
                | DeclarationSignature::Nominal { generic_parameters }
                | DeclarationSignature::Enum {
                    generic_parameters, ..
                }
                | DeclarationSignature::TypeAlias {
                    generic_parameters, ..
                } => FactState::Known(generic_parameters),
                DeclarationSignature::Callable(id) => {
                    FactState::Known(&self.callables[id.index()].generic_parameters)
                }
                DeclarationSignature::Constructor(constructor) => {
                    FactState::Known(&constructor.generic_parameters)
                }
                _ => match self.impl_header(target) {
                    FactState::Known(header) => FactState::Known(&header.generic_parameters),
                    _ => FactState::NotApplicable,
                },
            },
            FactState::Poisoned(poison) => FactState::Poisoned(poison),
            FactState::Unperformed => FactState::Unperformed,
            FactState::NotApplicable => FactState::NotApplicable,
        }
    }

    pub(crate) fn application_obligations(
        &mut self,
        application: &TypeApplication,
        arguments: &[TypeId],
    ) -> Result<SemanticFact<Vec<TraitObligation>>, TypeStoreError> {
        let parameters = match self.type_parameters(application.target()) {
            FactState::Known(parameters) => parameters.to_vec(),
            FactState::Poisoned(poison) => return Ok(SemanticFact::Poisoned(poison.clone())),
            FactState::Unperformed | FactState::NotApplicable => {
                return Ok(SemanticFact::NotApplicable);
            }
        };
        debug_assert_eq!(parameters.len(), arguments.len());
        let replacements = parameters
            .iter()
            .cloned()
            .zip(arguments.iter().copied())
            .collect();
        let mut obligations = Vec::new();
        for parameter in &parameters {
            let signature = match self.generic_parameter(parameter) {
                FactState::Known(signature) => signature.clone(),
                FactState::Poisoned(poison) => return Ok(SemanticFact::Poisoned(poison.clone())),
                FactState::Unperformed | FactState::NotApplicable => {
                    return Ok(SemanticFact::NotApplicable);
                }
            };
            obligations.extend(
                signature
                    .instantiate_bounds(&mut self.types, &replacements)?
                    .into_iter()
                    .map(|bound| bound.into_obligation()),
            );
        }
        Ok(SemanticFact::Known(obligations))
    }

    pub(crate) fn application_dependencies(
        &self,
        collection: &DeclarationCollection,
    ) -> BTreeMap<CanonicalTarget, BTreeSet<CanonicalTarget>> {
        let mut dependents = BTreeMap::<_, BTreeSet<_>>::new();
        for application in self.type_applications() {
            dependents
                .entry(application.target.clone())
                .or_default()
                .insert(application.site.owner());
        }
        for declaration in collection
            .modules()
            .flat_map(|module| module.declarations())
        {
            if let Some(parent) = declaration.parent() {
                let parent = CanonicalTarget::Source(parent);
                let child = CanonicalTarget::Source(declaration.id());
                dependents
                    .entry(parent.clone())
                    .or_default()
                    .insert(child.clone());
                dependents.entry(child).or_default().insert(parent);
            }
        }
        dependents
    }
}
