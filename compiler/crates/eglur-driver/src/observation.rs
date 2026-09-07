use std::time::{Duration, Instant};

use eglur_syntax::{FrontendCountUnit, FrontendPhase, FrontendPhaseObservation};

/// Stable compiler phase represented by one session observation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Phase {
    Lexer,
    ParserEvents,
    TreeBuild,
    HirLowering,
    DeclarationCollection,
    ImportPlanning,
    ImportScc,
    ImportResolution,
    NameResolution,
    DeclarationSignatures,
    TraitEnvironment,
    BodyTyping,
    OwnershipAnalysis,
    TraitEvidence,
    EffectAnalysis,
}

impl Phase {
    /// Returns the stable phase spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexer => "lexer",
            Self::ParserEvents => "parser_events",
            Self::TreeBuild => "tree_build",
            Self::HirLowering => "hir_lowering",
            Self::DeclarationCollection => "declaration_collection",
            Self::ImportPlanning => "import_planning",
            Self::ImportScc => "import_scc",
            Self::ImportResolution => "import_resolution",
            Self::NameResolution => "name_resolution",
            Self::DeclarationSignatures => "declaration_signatures",
            Self::TraitEnvironment => "trait_environment",
            Self::BodyTyping => "body_typing",
            Self::OwnershipAnalysis => "ownership_analysis",
            Self::TraitEvidence => "trait_evidence",
            Self::EffectAnalysis => "effect_analysis",
        }
    }

    /// Returns the unit attached to this phase's input count.
    #[must_use]
    pub const fn input_unit(self) -> CountUnit {
        match self {
            Self::Lexer => CountUnit::Bytes,
            Self::ParserEvents => CountUnit::Tokens,
            Self::TreeBuild => CountUnit::Events,
            Self::HirLowering => CountUnit::SyntaxTrees,
            Self::DeclarationCollection => CountUnit::SourceFiles,
            Self::ImportPlanning => CountUnit::ImportRequests,
            Self::ImportScc => CountUnit::SourceFiles,
            Self::ImportResolution => CountUnit::ImportRequests,
            Self::NameResolution => CountUnit::HirFiles,
            Self::DeclarationSignatures => CountUnit::Declarations,
            Self::TraitEnvironment => CountUnit::Declarations,
            Self::BodyTyping => CountUnit::Declarations,
            Self::OwnershipAnalysis => CountUnit::Declarations,
            Self::TraitEvidence => CountUnit::Invocations,
            Self::EffectAnalysis => CountUnit::Declarations,
        }
    }

    /// Returns the unit attached to this phase's output count.
    #[must_use]
    pub const fn output_unit(self) -> CountUnit {
        match self {
            Self::Lexer => CountUnit::Tokens,
            Self::ParserEvents => CountUnit::Events,
            Self::TreeBuild => CountUnit::SyntaxTrees,
            Self::HirLowering => CountUnit::HirFiles,
            Self::DeclarationCollection => CountUnit::Declarations,
            Self::ImportPlanning => CountUnit::ImportTargetFacts,
            Self::ImportScc => CountUnit::ImportSccs,
            Self::ImportResolution => CountUnit::ImportBindingFacts,
            Self::NameResolution => CountUnit::NameFacts,
            Self::DeclarationSignatures => CountUnit::SignatureFacts,
            Self::TraitEnvironment => CountUnit::TraitFacts,
            Self::BodyTyping => CountUnit::BodyTypeFacts,
            Self::OwnershipAnalysis => CountUnit::OwnershipFacts,
            Self::TraitEvidence => CountUnit::TraitEvidenceFacts,
            Self::EffectAnalysis => CountUnit::EffectFacts,
        }
    }
}

/// Closed count-unit vocabulary for compiler phase observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountUnit {
    Bytes,
    Tokens,
    Events,
    SyntaxTrees,
    HirFiles,
    SourceFiles,
    Declarations,
    ImportRequests,
    ImportTargetFacts,
    ImportSccs,
    ImportBindingFacts,
    NameFacts,
    SignatureFacts,
    TraitFacts,
    BodyTypeFacts,
    OwnershipFacts,
    Invocations,
    TraitEvidenceFacts,
    EffectFacts,
}

/// Aggregated opt-in evidence for one exact compiler phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhaseObservation {
    phase: Phase,
    input_count: usize,
    output_count: usize,
    elapsed: Duration,
}

impl PhaseObservation {
    /// Returns the observed phase.
    #[must_use]
    pub const fn phase(self) -> Phase {
        self.phase
    }

    /// Returns the aggregate input count in [`Phase::input_unit`].
    #[must_use]
    pub const fn input_count(self) -> usize {
        self.input_count
    }

    /// Returns the aggregate output count in [`Phase::output_unit`].
    #[must_use]
    pub const fn output_count(self) -> usize {
        self.output_count
    }

    /// Returns aggregate wall-clock time spent inside this phase.
    #[must_use]
    pub const fn elapsed(self) -> Duration {
        self.elapsed
    }
}

pub(crate) struct ObservationCollector {
    enabled: bool,
    observations: Vec<PhaseObservation>,
}

impl ObservationCollector {
    pub(crate) const fn new(enabled: bool) -> Self {
        Self {
            enabled,
            observations: Vec::new(),
        }
    }

    pub(crate) const fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn record_frontend(&mut self, observation: FrontendPhaseObservation) {
        debug_assert_eq!(
            map_frontend_unit(observation.phase().input_unit()),
            map_frontend_phase(observation.phase()).input_unit()
        );
        debug_assert_eq!(
            map_frontend_unit(observation.phase().output_unit()),
            map_frontend_phase(observation.phase()).output_unit()
        );
        self.record(
            map_frontend_phase(observation.phase()),
            observation.input_count(),
            observation.output_count(),
            observation.elapsed(),
        );
    }

    pub(crate) fn record(
        &mut self,
        phase: Phase,
        input_count: usize,
        output_count: usize,
        elapsed: Duration,
    ) {
        if !self.enabled {
            return;
        }
        if let Some(existing) = self
            .observations
            .iter_mut()
            .find(|observation| observation.phase == phase)
        {
            existing.input_count = existing
                .input_count
                .checked_add(input_count)
                .expect("session-owned observation input count must fit usize");
            existing.output_count = existing
                .output_count
                .checked_add(output_count)
                .expect("session-owned observation output count must fit usize");
            existing.elapsed = existing
                .elapsed
                .checked_add(elapsed)
                .expect("session observation duration must fit Duration");
            return;
        }
        self.observations.push(PhaseObservation {
            phase,
            input_count,
            output_count,
            elapsed,
        });
        self.observations
            .sort_by_key(|observation| observation.phase);
    }

    pub(crate) fn measure<T, E>(
        &mut self,
        phase: Phase,
        input_count: usize,
        operation: impl FnOnce() -> Result<T, E>,
        output_count: impl FnOnce(&T) -> usize,
    ) -> Result<T, E> {
        if !self.enabled {
            return operation();
        }
        let started = Instant::now();
        let result = operation()?;
        self.record(phase, input_count, output_count(&result), started.elapsed());
        Ok(result)
    }

    pub(crate) fn as_slice(&self) -> &[PhaseObservation] {
        &self.observations
    }

    pub(crate) fn into_boxed_slice(self) -> Box<[PhaseObservation]> {
        self.observations.into_boxed_slice()
    }
}

fn map_frontend_phase(phase: FrontendPhase) -> Phase {
    match phase {
        FrontendPhase::Lexer => Phase::Lexer,
        FrontendPhase::ParserEvents => Phase::ParserEvents,
        FrontendPhase::TreeBuild => Phase::TreeBuild,
    }
}

fn map_frontend_unit(unit: FrontendCountUnit) -> CountUnit {
    match unit {
        FrontendCountUnit::Bytes => CountUnit::Bytes,
        FrontendCountUnit::Tokens => CountUnit::Tokens,
        FrontendCountUnit::Events => CountUnit::Events,
        FrontendCountUnit::Trees => CountUnit::SyntaxTrees,
    }
}
