use super::*;
use crate::AnalysisWorkFailure;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SignatureFlow {
    Internal(SignatureResolutionError),
    Interrupted,
}

pub(super) struct CompletedSignatureTraversal(());

impl From<SignatureResolutionError> for SignatureFlow {
    fn from(error: SignatureResolutionError) -> Self {
        Self::Internal(error)
    }
}

impl From<AnalysisWorkFailure> for SignatureFlow {
    fn from(error: AnalysisWorkFailure) -> Self {
        match error {
            AnalysisWorkFailure::Claim(error) => {
                Self::Internal(SignatureResolutionError::AnalysisRootClaim(error))
            }
            AnalysisWorkFailure::Incomplete(_) => Self::Interrupted,
        }
    }
}

impl From<crate::AnalysisSourceInputFailure> for SignatureFlow {
    fn from(error: crate::AnalysisSourceInputFailure) -> Self {
        match error {
            crate::AnalysisSourceInputFailure::Work(error) => error.into(),
            other => SignatureResolutionError::AnalysisSourceInput(other).into(),
        }
    }
}

macro_rules! internal_conversion {
    ($($ty:ty),* $(,)?) => {
        $(impl From<$ty> for SignatureFlow {
            fn from(error: $ty) -> Self { SignatureResolutionError::from(error).into() }
        })*
    };
}

internal_conversion!(
    crate::TypeStoreError,
    crate::type_rendering::DiagnosticTypeRenderingError,
    crate::DuplicateFact,
    crate::diagnostic::SemanticDiagnosticError,
    eglur_diagnostics::DiagnosticStoreError,
    crate::type_namespace::TypeNameLookupError,
    TrustedSignatureError,
    crate::InhabitanceError,
    crate::IdentityOverflow,
);

impl SignatureResolver<'_, '_> {
    pub(super) fn ensure_analysis_active(&self) -> Result<(), SignatureFlow> {
        if self
            .analysis_work
            .as_ref()
            .is_some_and(|work| work.accounting.incomplete().is_some())
        {
            return Err(SignatureFlow::Interrupted);
        }
        Ok(())
    }

    pub(super) fn execute(
        mut self,
        recorder: DemandRecorder<'_>,
    ) -> Result<SignatureExecution, SignatureResolutionError> {
        let traversal = self.run_passes();
        self.complete_execution(traversal, recorder)
    }

    fn run_passes(&mut self) -> Result<InhabitanceFacts, SignatureFlow> {
        macro_rules! pass {
            ($method:ident) => {
                self.ensure_analysis_active()?;
                self.$method()?;
            };
        }
        pass!(resolve_trusted);
        pass!(resolve_trusted_inherent_owners);
        pass!(validate_language_items);
        pass!(resolve_source_generics);
        pass!(prime_source_impl_headers);
        pass!(validate_source_impl_headers);
        pass!(resolve_source_declarations);
        pass!(validate_source_overload_families);
        pass!(resolve_body_type_occurrences);
        pass!(index_enum_variant_names);
        self.ensure_analysis_active()?;
        let trusted = self.resolve_trusted_inhabitance()?;
        self.ensure_analysis_active()?;
        Ok(InhabitanceFacts::new(trusted)?)
    }

    fn complete_execution(
        self,
        traversal: Result<InhabitanceFacts, SignatureFlow>,
        recorder: DemandRecorder<'_>,
    ) -> Result<SignatureExecution, SignatureResolutionError> {
        if let Some(stop) = self
            .analysis_work
            .as_ref()
            .and_then(|work| work.accounting.incomplete())
        {
            let (inhabitance, internal_failures) = match traversal {
                Ok(facts) => (Some(facts), Vec::new()),
                Err(SignatureFlow::Internal(error)) => (None, vec![error]),
                Err(SignatureFlow::Interrupted) => (None, Vec::new()),
            };
            let prefix = SignatureResolutionPrefix {
                type_applications: self.type_applications,
                types: self.types,
                diagnostic_type_names: self.diagnostic_type_names,
                declarations: self.declarations,
                enum_variant_names: self.enum_variant_names,
                overload_family_poisons: self.overload_family_poisons,
                generic_parameters: self.generic_parameters,
                occurrences: self.occurrences,
                callables: self.callables,
                impl_headers: self.retained_impl_headers,
                rejected_impl_headers: self.rejected_impl_headers,
                impl_later_roots: self.impl_later_roots,
                trait_methods: self.trait_methods,
                trait_method_owners: self.trait_method_owners,
                declared_impl_methods: self.declared_impl_methods,
                trait_adapters: self.trait_adapters,
                trusted_inherent_owners: self.trusted_inherent_owners,
                prelude_traits: self.prelude_traits,
                language_items: self.language_items,
                inhabitance,
            };
            return Ok(SignatureExecution::Interrupted(Box::new(
                InterruptedSignatures {
                    stop,
                    prefix: Some(prefix),
                    demands: recorder.into_demands(),
                    internal_failures,
                },
            )));
        }

        match traversal {
            Ok(inhabitance) => {
                let signatures = self.finish(CompletedSignatureTraversal(()));
                Ok(SignatureExecution::Completed(Box::new(recorder.finish(
                    PhaseProduct::Performed(ResolvedDeclarationSemantics {
                        signatures,
                        inhabitance,
                    }),
                ))))
            }
            Err(SignatureFlow::Internal(error)) => Err(error),
            Err(SignatureFlow::Interrupted) => Err(SignatureResolutionError::MissingAnalysisStop),
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
