use std::collections::BTreeMap;
use std::rc::Rc;

use super::direct_generic_parameters;
use crate::{
    DeclarationCollection, DeclarationSignatures, DefId, GenericParameterId, StableDeclarationError,
};

#[cfg(test)]
mod tests;

pub(crate) struct DeclarationBinderContexts<'a> {
    collection: &'a DeclarationCollection,
    signatures: &'a DeclarationSignatures,
    completed: BTreeMap<DefId, Rc<BinderContext>>,
    empty: Rc<BinderContext>,
}

#[derive(Debug)]
pub(crate) struct BinderContext {
    parent: Option<Rc<Self>>,
    local: BTreeMap<GenericParameterId, u32>,
    distinct_count: usize,
    parameter_count: usize,
}

impl<'a> DeclarationBinderContexts<'a> {
    pub(crate) fn new(
        collection: &'a DeclarationCollection,
        signatures: &'a DeclarationSignatures,
    ) -> Self {
        Self {
            collection,
            signatures,
            completed: BTreeMap::new(),
            empty: Rc::new(BinderContext::empty()),
        }
    }

    pub(crate) fn context(
        &mut self,
        declaration: DefId,
    ) -> Result<Rc<BinderContext>, StableDeclarationError> {
        #[cfg(test)]
        tests::observe_query();

        let mut owners = Vec::new();
        let mut cursor = Some(declaration);
        let mut context = Rc::clone(&self.empty);

        // Missing declarations take precedence over signature failures.
        while let Some(owner) = cursor {
            if let Some(completed) = self.completed.get(&owner) {
                context = Rc::clone(completed);
                break;
            }
            let declaration = self
                .collection
                .declaration(owner)
                .ok_or(StableDeclarationError::UnknownDeclaration(owner))?;
            #[cfg(test)]
            tests::observe_owner();

            owners.push(owner);
            cursor = declaration.parent();
        }
        owners.reverse();

        let parameters = owners
            .iter()
            .map(|&owner| direct_generic_parameters(self.signatures, owner))
            .collect::<Result<Vec<_>, _>>()?;
        let parameter_count = parameters
            .iter()
            .fold(context.parameter_count, |count, parameters| {
                count + parameters.len()
            });
        let mut completed = Vec::new();
        for (owner, parameters) in owners.into_iter().zip(parameters) {
            context = context.extend(parameters, parameter_count)?;
            completed.push((owner, Rc::clone(&context)));
        }

        // Publish prefixes only after the entire requested context succeeds.
        self.completed.extend(completed);
        Ok(context)
    }
}

impl BinderContext {
    pub(crate) const fn empty() -> Self {
        Self {
            parent: None,
            local: BTreeMap::new(),
            distinct_count: 0,
            parameter_count: 0,
        }
    }

    pub(crate) fn ordinal(&self, parameter: &GenericParameterId) -> Option<u32> {
        #[cfg(test)]
        tests::observe_lookup();

        let mut cursor = Some(self);
        while let Some(context) = cursor {
            #[cfg(test)]
            tests::observe_frame();

            if let Some(&index) = context.local.get(parameter) {
                return Some(index);
            }
            cursor = context.parent.as_deref();
        }
        None
    }

    fn extend(
        self: &Rc<Self>,
        parameters: &[GenericParameterId],
        parameter_count: usize,
    ) -> Result<Rc<Self>, StableDeclarationError> {
        if parameters.is_empty() {
            return Ok(Rc::clone(self));
        }

        let mut local = BTreeMap::new();
        let mut distinct_count = self.distinct_count;
        for parameter in parameters {
            #[cfg(test)]
            tests::observe_parameter();

            // Flattened ordinals use the visible map size before each insertion,
            // including duplicate slots that overwrite an inherited binding.
            let index = binder_index(distinct_count, parameter_count)?;
            if !local.contains_key(parameter) && self.ordinal(parameter).is_none() {
                distinct_count += 1;
            }
            local.insert(parameter.clone(), index);
        }

        Ok(Rc::new(Self {
            parent: Some(Rc::clone(self)),
            local,
            distinct_count,
            parameter_count: self.parameter_count + parameters.len(),
        }))
    }
}

fn binder_index(length: usize, parameter_count: usize) -> Result<u32, StableDeclarationError> {
    u32::try_from(length)
        .map_err(|_| StableDeclarationError::GenericParameterOverflow(parameter_count))
}
