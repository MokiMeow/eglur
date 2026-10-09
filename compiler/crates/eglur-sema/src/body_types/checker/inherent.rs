use super::*;
use crate::{
    CanonicalTypeStore, ConcreteOwnerHead, InherentFamilySignature, InherentMemberForm,
    InherentMemberSignature, TypeStoreError,
};

#[cfg(test)]
mod tests;

pub(super) enum InherentMethodFamily {
    Source(Vec<DefId>),
    Trusted(InherentFamilySignature),
}

#[derive(Clone)]
pub(super) struct InherentOwnerConstraint {
    pattern: TypeId,
    subject: TypeId,
}

impl InherentOwnerConstraint {
    pub(super) fn matches(
        &self,
        types: &mut CanonicalTypeStore,
        replacements: &BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<bool, TypeStoreError> {
        Ok(types.substitute(self.pattern, replacements)? == self.subject)
    }

    pub(super) fn pattern(&self) -> TypeId {
        self.pattern
    }

    pub(super) fn subject(&self) -> TypeId {
        self.subject
    }
}

pub(super) enum InherentMethodTemplate {
    Source(CallableSignature),
    Trusted {
        callable: CallableSignature,
        member: InherentMemberSignature,
    },
}

impl InherentMethodTemplate {
    pub(super) fn callable(&self) -> &CallableSignature {
        match self {
            Self::Source(callable) | Self::Trusted { callable, .. } => callable,
        }
    }

    pub(super) fn has_receiver(&self) -> bool {
        match self {
            Self::Source(_) => true,
            Self::Trusted { member, .. } => member.form() != InherentMemberForm::Associated,
        }
    }

    pub(super) fn explicit_parameters(&self) -> &[GenericParameterId] {
        match self {
            Self::Source(callable) => callable.generic_parameters(),
            Self::Trusted { callable, member } => {
                &callable.generic_parameters()[member.owner_parameter_count()..]
            }
        }
    }
}

impl<'input, 'hir, 'support> BodyChecker<'input, 'hir, 'support> {
    pub(super) fn inherent_method_family(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        owner: &CanonicalTarget,
        name: &NameKey,
    ) -> Result<Outcome<Option<InherentMethodFamily>>, BodyTypingError> {
        match owner {
            CanonicalTarget::Primitive(owner) => {
                self.recorder.refuse_input(
                    crate::UnavailableCompileInput::PrimitiveInherentInventory { owner: *owner },
                    SemanticLocation::origin(source.file(), origin),
                );
                Ok(Outcome::Unperformed)
            }
            CanonicalTarget::Source(owner) => {
                let targets =
                    self.collection
                        .child_candidates(*owner, name, DeclarationKind::Function);
                Ok(Outcome::Known(
                    (!targets.is_empty()).then_some(InherentMethodFamily::Source(targets)),
                ))
            }
            CanonicalTarget::External(external) => {
                let head = ConcreteOwnerHead::Nominal(NominalHead::Declaration(owner.clone()));
                let Some(inventory) = self.signatures.trusted_inherent_owner(&head) else {
                    let location = SemanticLocation::origin(source.file(), origin);
                    self.recorder.refuse_input(
                        crate::UnavailableCompileInput::TrustedInherentInventory {
                            owner: external.clone(),
                        },
                        location,
                    );
                    self.recorder.require(
                        CompileCapability::Typecheck(
                            TypecheckCapability::ResolveTrustedInherentMethods,
                        ),
                        location,
                    );
                    return Ok(Outcome::Unperformed);
                };
                let family = inventory.family(name).cloned();
                if !self.recorder.require(
                    CompileCapability::Typecheck(
                        TypecheckCapability::ResolveTrustedInherentMethods,
                    ),
                    SemanticLocation::origin(source.file(), origin),
                ) {
                    return Ok(Outcome::Unperformed);
                }
                Ok(Outcome::Known(family.map(InherentMethodFamily::Trusted)))
            }
        }
    }

    pub(super) fn receiver_inherent_family(
        &mut self,
        source: &SemanticSource<'_>,
        origin: OriginId,
        subject: TypeId,
        name: &NameKey,
    ) -> Result<Outcome<Option<InherentMethodFamily>>, BodyTypingError> {
        let TypeKind::Nominal {
            head: NominalHead::Declaration(owner),
            ..
        } = self.signatures.types().kind(subject)?
        else {
            return Ok(Outcome::Known(None));
        };
        let owner = owner.clone();
        self.inherent_method_family(source, origin, &owner, name)
    }

    pub(super) fn inaccessible_inherent_family(
        &self,
        module: ModuleId,
        family: &InherentMethodFamily,
    ) -> Result<bool, BodyTypingError> {
        match family {
            InherentMethodFamily::Source(targets) => {
                self.inherent_family_is_inaccessible(module, targets)
            }
            InherentMethodFamily::Trusted(family) => Ok(!family.is_public()),
        }
    }

    pub(super) fn inherent_family_templates(
        &self,
        family: &InherentMethodFamily,
    ) -> Result<Outcome<Vec<InherentMethodTemplate>>, BodyTypingError> {
        let members = match family {
            InherentMethodFamily::Source(targets) => targets
                .iter()
                .map(|&target| (CanonicalTarget::Source(target), None))
                .collect::<Vec<_>>(),
            InherentMethodFamily::Trusted(family) => family
                .members()
                .iter()
                .map(|member| {
                    let callable = self
                        .signatures
                        .callable(member.callable())
                        .ok_or(BodyTypingError::UnknownCallable(member.callable()))?;
                    Ok((callable.declaration().clone(), Some(member.clone())))
                })
                .collect::<Result<Vec<_>, BodyTypingError>>()?,
        };
        let mut templates = Vec::with_capacity(members.len());
        for (target, member) in members {
            let callable = match self.signatures.declaration(&target) {
                FactState::Known(DeclarationSignature::Callable(id)) => self
                    .signatures
                    .callable(*id)
                    .cloned()
                    .ok_or(BodyTypingError::UnknownCallable(*id))?,
                FactState::Poisoned(poison) => return Ok(Outcome::Poisoned(poison.clone())),
                FactState::Unperformed => return Ok(Outcome::Unperformed),
                FactState::Known(_) | FactState::NotApplicable => continue,
            };
            templates.push(match member {
                Some(member) => InherentMethodTemplate::Trusted { callable, member },
                None => InherentMethodTemplate::Source(callable),
            });
        }
        Ok(Outcome::Known(templates))
    }

    pub(super) fn inherent_template_candidate(
        &mut self,
        template: InherentMethodTemplate,
        subject: Option<TypeId>,
    ) -> Result<Outcome<Option<MethodCandidate>>, BodyTypingError> {
        match template {
            InherentMethodTemplate::Source(callable) => {
                let Some(subject) = subject else {
                    return Ok(Outcome::Known(None));
                };
                let CanonicalTarget::Source(method) = callable.declaration() else {
                    unreachable!("source template has a source declaration");
                };
                self.inherent_method_candidates(subject, &[*method])
                    .map(|outcome| outcome.map(|mut candidates| candidates.pop()))
            }
            InherentMethodTemplate::Trusted { callable, member } => {
                let owner_parameters =
                    &callable.generic_parameters()[..member.owner_parameter_count()];
                let mut replacements = BTreeMap::new();
                if let Some(subject) = subject
                    && !self.match_inherent_owner_pattern(
                        member.owner_pattern(),
                        subject,
                        owner_parameters,
                        &mut replacements,
                    )?
                {
                    return Ok(Outcome::Known(None));
                }
                let explicit_parameters = callable.generic_parameters()
                    [member.owner_parameter_count()..]
                    .to_vec()
                    .into_boxed_slice();
                Ok(Outcome::Known(Some(MethodCandidate {
                    callable,
                    explicit_parameters,
                    preset_replacements: replacements,
                    owner_constraint: subject.map(|subject| InherentOwnerConstraint {
                        pattern: member.owner_pattern(),
                        subject,
                    }),
                    dispatch: MethodDispatchTemplate::Direct,
                })))
            }
        }
    }

    fn match_inherent_owner_pattern(
        &mut self,
        pattern: TypeId,
        subject: TypeId,
        parameters: &[GenericParameterId],
        replacements: &mut BTreeMap<GenericParameterId, TypeId>,
    ) -> Result<bool, BodyTypingError> {
        Ok(crate::overloads::match_inference_patterns(
            self.signatures.types_mut(),
            [(pattern, subject, false)],
            &parameters.iter().cloned().collect(),
            replacements,
        )?)
    }
}
