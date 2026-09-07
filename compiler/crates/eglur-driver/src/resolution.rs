use eglur_diagnostics::{DiagnosticStore, Poison};
use eglur_sema::{
    BodyTypes, CapabilityTrace, CompileCapability, DeclarationCollection, EffectFacts,
    ImportAnalysisUnit, ImportPlan, ModuleAddress, NameResolution, OwnershipFacts, PhaseProduct,
    ProgramMode, ResolvedDeclarationSemantics, ResolvedImports, SemanticFingerprints,
    SemanticResolution, SemanticSource, SignatureResolutionInputs, TraitEnvironment, TraitEvidence,
    TrustedDeclarationInputs, TypecheckCapability, analyze_effects, analyze_ownership,
    build_trait_environment, collect_declarations, plan_imports, resolve_declaration_signatures,
    resolve_imports, resolve_names, resolve_trait_evidence, typecheck_bodies,
};
use eglur_source::FileId;

use crate::catalog::TrustedCatalog;
use crate::fingerprint::{AnalysisFingerprints, compute_analysis_fingerprints};
use crate::observation::{ObservationCollector, Phase};
use crate::pipeline::SemanticPipeline;
use crate::schedule::{ImportDependency, build_import_schedule};
use crate::session::{LoweredSource, SessionError};
use crate::support::SupportProfile;
use crate::support_manifest::SupportManifest;

enum ResolutionAnalysisState {
    Resolved(Box<SemanticResolution>),
    Unperformed,
}

/// One session's resolver result and exact implementation-support evidence.
pub struct ResolutionAnalysis {
    mode: ProgramMode,
    state: ResolutionAnalysisState,
    fingerprints: Option<AnalysisFingerprints>,
    trace: CapabilityTrace,
    manifest: SupportManifest,
}

impl ResolutionAnalysis {
    #[must_use]
    pub const fn mode(&self) -> ProgramMode {
        self.mode
    }

    #[must_use]
    pub fn resolution(&self) -> Option<&SemanticResolution> {
        match &self.state {
            ResolutionAnalysisState::Resolved(resolution) => Some(resolution),
            ResolutionAnalysisState::Unperformed => None,
        }
    }

    #[must_use]
    pub const fn fingerprints(&self) -> Option<&AnalysisFingerprints> {
        self.fingerprints.as_ref()
    }

    #[must_use]
    pub const fn trace(&self) -> &CapabilityTrace {
        &self.trace
    }

    #[must_use]
    pub const fn manifest(&self) -> &SupportManifest {
        &self.manifest
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        ProgramMode,
        Option<SemanticResolution>,
        CapabilityTrace,
        SupportManifest,
    ) {
        let resolution = match self.state {
            ResolutionAnalysisState::Resolved(resolution) => Some(*resolution),
            ResolutionAnalysisState::Unperformed => None,
        };
        (self.mode, resolution, self.trace, self.manifest)
    }

    fn resolved(
        mode: ProgramMode,
        resolution: SemanticResolution,
        fingerprints: Option<AnalysisFingerprints>,
        trace: CapabilityTrace,
        manifest: SupportManifest,
    ) -> Self {
        Self {
            mode,
            state: ResolutionAnalysisState::Resolved(Box::new(resolution)),
            fingerprints,
            trace,
            manifest,
        }
    }

    fn unperformed(mode: ProgramMode, trace: CapabilityTrace, manifest: SupportManifest) -> Self {
        Self {
            mode,
            state: ResolutionAnalysisState::Unperformed,
            fingerprints: None,
            trace,
            manifest,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ResolutionInvocation {
    entry: FileId,
    mode: ProgramMode,
}

impl ResolutionInvocation {
    pub(crate) const fn new(entry: FileId, mode: ProgramMode) -> Self {
        Self { entry, mode }
    }
}

pub(crate) fn resolve_sources(
    sources: &[LoweredSource],
    source_ids: &[FileId],
    invocation: ResolutionInvocation,
    profile: &SupportProfile,
    catalog: Option<&TrustedCatalog>,
    diagnostics: &mut DiagnosticStore,
    observations: &mut ObservationCollector,
) -> Result<ResolutionAnalysis, SessionError> {
    let ResolutionInvocation { entry, mode } = invocation;
    let semantic_sources = sources
        .iter()
        .map(normalize_source)
        .collect::<Result<Vec<_>, _>>()?;
    let hir_count = sources
        .iter()
        .filter(|source| source.hir().is_some())
        .count();
    let external = catalog.map(TrustedCatalog::names);
    let mut pipeline = SemanticPipeline::new(profile, observations);

    let product = pipeline.run(
        Phase::DeclarationCollection,
        sources.len(),
        || collect_declarations(&semantic_sources, profile),
        declaration_count,
    )?;
    let PhaseProduct::Performed(declarations) = product else {
        return Ok(ResolutionAnalysis::unperformed(
            mode,
            pipeline.finish()?,
            SupportManifest::from_profile(profile),
        ));
    };

    let import_count = declarations.import_count();
    let product = pipeline.run(
        Phase::ImportPlanning,
        import_count,
        || {
            plan_imports(
                &declarations,
                &semantic_sources,
                profile,
                external,
                diagnostics,
            )
        },
        import_target_count,
    )?;
    let import_plan = require_performed(product, Phase::ImportPlanning)?;

    let dependencies = import_plan
        .dependencies()
        .iter()
        .map(|dependency| ImportDependency::new(dependency.importer(), dependency.imported()))
        .collect::<Vec<_>>();
    let schedule = pipeline.measure(
        Phase::ImportScc,
        source_ids.len(),
        || build_import_schedule(source_ids, dependencies.iter().copied()),
        |schedule| schedule.units().len(),
    )?;
    let units = schedule
        .units()
        .iter()
        .map(|unit| ImportAnalysisUnit::new(unit.members().iter().copied()))
        .collect::<Result<Vec<_>, _>>()?;

    let product = pipeline.run(
        Phase::ImportResolution,
        import_count,
        || {
            resolve_imports(
                &declarations,
                &import_plan,
                &units,
                &semantic_sources,
                profile,
                external,
                diagnostics,
            )
        },
        import_binding_count,
    )?;
    let imports = require_performed(product, Phase::ImportResolution)?;

    let product = pipeline.run(
        Phase::NameResolution,
        hir_count,
        || {
            resolve_names(
                &declarations,
                &imports,
                &semantic_sources,
                profile,
                external,
                diagnostics,
            )
        },
        name_fact_count,
    )?;
    let names = require_performed(product, Phase::NameResolution)?;

    let declaration_count = declarations.declaration_count();
    let product = pipeline.run(
        Phase::DeclarationSignatures,
        declaration_count,
        || {
            let inputs = SignatureResolutionInputs::new(
                external,
                catalog.map(|catalog| {
                    TrustedDeclarationInputs::new(catalog.signatures(), Some(catalog.inhabitance()))
                        .with_language_items(catalog.language_items())
                }),
            );
            let inputs = if mode.requires_entry() {
                inputs.with_deferred_entry_source(entry)
            } else {
                inputs
            };
            resolve_declaration_signatures(
                &declarations,
                &imports,
                &semantic_sources,
                profile,
                inputs,
                diagnostics,
            )
        },
        signature_fact_count,
    )?;
    let PhaseProduct::Performed(mut declaration_semantics) = product else {
        return Ok(ResolutionAnalysis::unperformed(
            mode,
            pipeline.finish()?,
            SupportManifest::from_profile(profile),
        ));
    };
    let product = pipeline.run(
        Phase::TraitEnvironment,
        declaration_count,
        || {
            build_trait_environment(
                &declarations,
                &semantic_sources,
                profile,
                declaration_semantics.signatures_mut(),
                diagnostics,
            )
        },
        trait_environment_fact_count,
    )?;
    let PhaseProduct::Performed(mut trait_environment) = product else {
        return Ok(ResolutionAnalysis::unperformed(
            mode,
            pipeline.finish()?,
            SupportManifest::from_profile(profile),
        ));
    };

    let product = pipeline.run(
        Phase::BodyTyping,
        declaration_count,
        || {
            typecheck_bodies(
                &declarations,
                &imports,
                &names,
                external,
                &semantic_sources,
                profile,
                &mut declaration_semantics,
                &mut trait_environment,
                diagnostics,
            )
        },
        body_type_fact_count,
    )?;
    let PhaseProduct::Performed(body_types) = product else {
        return Ok(ResolutionAnalysis::unperformed(
            mode,
            pipeline.finish()?,
            SupportManifest::from_profile(profile),
        ));
    };
    let ownership_facts = if mode != ProgramMode::Library
        || profile.supports(CompileCapability::Typecheck(
            TypecheckCapability::CheckOwnershipFlow,
        )) {
        pipeline.run(
            Phase::OwnershipAnalysis,
            declaration_count,
            || {
                analyze_ownership(
                    &declarations,
                    &names,
                    &semantic_sources,
                    profile,
                    declaration_semantics.signatures(),
                    &body_types,
                    diagnostics,
                )
            },
            ownership_fact_count,
        )?
    } else {
        PhaseProduct::Unperformed
    };
    let trait_evidence = pipeline.run(
        Phase::TraitEvidence,
        body_types.invocations().len(),
        || {
            resolve_trait_evidence(
                &declarations,
                &semantic_sources,
                profile,
                declaration_semantics.signatures_mut(),
                &mut trait_environment,
                &body_types,
                diagnostics,
            )
        },
        trait_evidence_fact_count,
    )?;
    let effect_facts = match &trait_evidence {
        PhaseProduct::Performed(trait_evidence)
            if profile.supports(CompileCapability::Typecheck(
                TypecheckCapability::ResolveTraitEvidence,
            )) =>
        {
            pipeline.run(
                Phase::EffectAnalysis,
                declaration_count,
                || {
                    analyze_effects(
                        &declarations,
                        &names,
                        &semantic_sources,
                        profile,
                        declaration_semantics.signatures(),
                        &body_types,
                        trait_evidence,
                        catalog.map(TrustedCatalog::effects),
                        diagnostics,
                    )
                },
                effect_fact_count,
            )?
        }
        PhaseProduct::Performed(_) | PhaseProduct::Unperformed => PhaseProduct::Unperformed,
    };
    let (signatures, inhabitance) = declaration_semantics.into_parts();
    let semantic_fingerprints = SemanticFingerprints::compute(
        &declarations,
        &imports,
        &names,
        &signatures,
        &semantic_sources,
        &body_types,
        &trait_environment,
        &trait_evidence,
        &effect_facts,
    )?;
    let analysis_fingerprints = semantic_fingerprints
        .as_ref()
        .map(|fingerprints| {
            compute_analysis_fingerprints(
                sources,
                &semantic_sources,
                &schedule,
                &dependencies,
                catalog,
                fingerprints,
            )
        })
        .transpose()?;

    Ok(ResolutionAnalysis::resolved(
        mode,
        SemanticResolution::new(
            declarations,
            import_plan,
            imports,
            names,
            signatures,
            inhabitance,
            trait_environment,
            body_types,
            ownership_facts,
            trait_evidence,
            effect_facts,
            semantic_fingerprints,
        ),
        analysis_fingerprints,
        pipeline.finish()?,
        SupportManifest::from_profile(profile),
    ))
}

fn normalize_source(source: &LoweredSource) -> Result<SemanticSource<'_>, SessionError> {
    let module = ModuleAddress::new(source.module_path().components().map(Box::<str>::from))?;
    let origin_spans = source.hir().into_iter().flat_map(|hir| {
        hir.origins().filter_map(|(origin, _)| {
            source
                .origin_span(origin)
                .cloned()
                .map(|span| (origin, span))
        })
    });
    let invalid_poisons = source
        .invalid_facts()
        .map(|fact| (fact.occurrence().origin(), fact.poison().clone()));
    Ok(SemanticSource::new(
        source.file(),
        module,
        source.hir(),
        origin_spans,
        invalid_poisons,
        merge_source_poison(source)?,
    )?)
}

fn merge_source_poison(source: &LoweredSource) -> Result<Option<Poison>, SessionError> {
    match (source.source_poison(), source.header_poison()) {
        (Some(source), Some(header)) => Ok(Some(source.merge(header)?)),
        (Some(poison), None) | (None, Some(poison)) => Ok(Some(poison.clone())),
        (None, None) => Ok(None),
    }
}

fn declaration_count(collection: &DeclarationCollection) -> usize {
    collection.declaration_count()
}

fn import_target_count(plan: &ImportPlan) -> usize {
    plan.target_fact_count()
}

fn import_binding_count(imports: &ResolvedImports) -> usize {
    imports.binding_fact_count()
}

fn name_fact_count(names: &NameResolution) -> usize {
    names.fact_count()
}

fn signature_fact_count(resolved: &ResolvedDeclarationSemantics) -> usize {
    resolved.signatures().declaration_fact_count()
}

fn body_type_fact_count(body_types: &BodyTypes) -> usize {
    body_types.fact_count()
}

fn trait_environment_fact_count(environment: &TraitEnvironment) -> usize {
    environment.fact_count()
}

fn trait_evidence_fact_count(evidence: &TraitEvidence) -> usize {
    evidence.fact_count()
}

fn ownership_fact_count(facts: &OwnershipFacts) -> usize {
    facts.fact_count()
}

fn effect_fact_count(facts: &EffectFacts) -> usize {
    facts.fact_count()
}

fn require_performed<T>(product: PhaseProduct<T>, phase: Phase) -> Result<T, SessionError> {
    match product {
        PhaseProduct::Performed(product) => Ok(product),
        PhaseProduct::Unperformed => Err(SessionError::UnexpectedUnperformedPhase(phase)),
    }
}
