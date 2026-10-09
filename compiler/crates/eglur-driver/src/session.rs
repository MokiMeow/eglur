use std::fmt;
use std::time::Instant;

use eglur_diagnostics::{
    DiagnosticRootError, DiagnosticSnapshot, DiagnosticStore, DiagnosticStoreError, ErrorLimit,
    Poison, SourceLabel, SourceProjectionError, SourceSpan,
};
use eglur_hir::{HirFile, InvalidCategory, InvalidOccurrence, OriginId, SyntaxCauseId, lower};
use eglur_sema::{
    BodyTypingError, CapabilityTrace, CapabilityTraceError, CheckedProgram, CollectionError,
    EffectAnalysisError, ImportResolutionError, ModuleAddress, ModuleAddressError,
    NameResolutionError, OwnershipAnalysisError, ProgramMode, ProgramSealError, ProgramSource,
    ProgramSourceError, SemanticFingerprintError, SignatureResolutionError, SourceInputError,
    TraitEnvironmentError, TraitEvidenceError, seal_checked_program,
};
use eglur_source::{FileId, LineIndex, SourceFile, SourceValidationError};
use eglur_syntax::{Parse, parse_validated, parse_validated_observed};

use crate::catalog::TrustedCatalog;
use crate::diagnostic::{
    record_invalid_utf8, record_module_component_invalid, record_module_path_mismatch,
    record_module_root_reserved, record_syntax,
};
use crate::fingerprint::AnalysisFingerprintError;
use crate::input::{FileSetRequest, is_source_path};
use crate::module::{ModulePath, ModulePathCandidate};
use crate::observation::{ObservationCollector, Phase, PhaseObservation};
use crate::resolution::{ResolutionAnalysis, ResolutionInvocation, resolve_sources};
use crate::schedule::{
    ImportDependency, ImportSchedule, ImportScheduleError, build_observed_import_schedule,
};
use crate::support::SupportProfile;
use crate::support_manifest::SupportManifest;

/// Session construction options that cannot affect semantic output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionOptions {
    observe_phases: bool,
}

impl SessionOptions {
    /// Creates options with phase observation either enabled or disabled.
    #[must_use]
    pub const fn new(observe_phases: bool) -> Self {
        Self { observe_phases }
    }

    /// Reports whether phase evidence is collected.
    #[must_use]
    pub const fn observes_phases(self) -> bool {
        self.observe_phases
    }
}

/// One parsed and optionally lowered source compilation unit.
pub struct LoweredSource {
    path: Box<str>,
    label: SourceLabel,
    module_path: ModulePath,
    source: SourceFile,
    parse: Option<Parse>,
    hir: Option<HirFile>,
    source_poison: Option<Poison>,
    syntax_poisons: Box<[Poison]>,
    invalid_poisons: Box<[Poison]>,
    origin_spans: Box<[SourceSpan]>,
    header_poison: Option<Poison>,
}

impl LoweredSource {
    /// Returns this compilation unit's stable session identity.
    #[must_use]
    pub const fn file(&self) -> FileId {
        self.source.id()
    }

    /// Returns the canonical mapped path used for module and file identity.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the distinct diagnostic source identity.
    #[must_use]
    pub const fn label(&self) -> &SourceLabel {
        &self.label
    }

    /// Returns the source-location-derived canonical module path.
    #[must_use]
    pub const fn module_path(&self) -> &ModulePath {
        &self.module_path
    }

    /// Returns the exactly-once parser product when source validation succeeded.
    #[must_use]
    pub const fn parse(&self) -> Option<&Parse> {
        self.parse.as_ref()
    }

    /// Returns the exactly-once HIR product when source validation succeeded.
    #[must_use]
    pub const fn hir(&self) -> Option<&HirFile> {
        self.hir.as_ref()
    }

    pub(crate) fn source_bytes(&self) -> &[u8] {
        self.source.bytes()
    }

    /// Returns source-bound rejection provenance when no parse was possible.
    #[must_use]
    pub const fn source_poison(&self) -> Option<&Poison> {
        self.source_poison.as_ref()
    }

    /// Resolves one HIR syntax-cause identity to retained root provenance.
    #[must_use]
    pub fn syntax_poison(&self, cause: SyntaxCauseId) -> Option<&Poison> {
        self.syntax_poisons.get(cause.get() as usize)
    }

    /// Iterates every recovered invalid occurrence with exact root provenance.
    pub fn invalid_facts(&self) -> impl ExactSizeIterator<Item = InvalidFact<'_>> {
        let occurrences = self
            .hir
            .as_ref()
            .map_or(&[][..], HirFile::invalid_occurrences);
        occurrences
            .iter()
            .zip(&self.invalid_poisons)
            .map(|(occurrence, poison)| InvalidFact { occurrence, poison })
    }

    /// Resolves one HIR origin to its retained canonical diagnostic span.
    #[must_use]
    pub fn origin_span(&self, origin: OriginId) -> Option<&SourceSpan> {
        self.origin_spans.get(origin.get() as usize)
    }

    /// Returns mismatch provenance for a valid but incorrect module header.
    #[must_use]
    pub const fn header_poison(&self) -> Option<&Poison> {
        self.header_poison.as_ref()
    }

    fn into_program_source(self) -> Result<ProgramSource, SessionError> {
        let hir = self
            .hir
            .ok_or(SessionError::MissingCheckedHir(self.source.id()))?;
        let module = ModuleAddress::new(self.module_path.components().map(Box::<str>::from))?;
        let origin_spans = hir
            .origins()
            .map(|(origin, _)| origin)
            .zip(self.origin_spans)
            .collect::<Vec<_>>();
        Ok(ProgramSource::new(
            self.label,
            self.source,
            module,
            hir,
            origin_spans,
        )?)
    }
}

/// Frozen semantic analysis before tool-disposition and output projection.
pub struct SealedCompilation {
    mode: ProgramMode,
    diagnostics: DiagnosticSnapshot,
    trace: CapabilityTrace,
    manifest: SupportManifest,
    observations: Box<[PhaseObservation]>,
    checked_program: Option<CheckedProgram>,
}

impl SealedCompilation {
    #[must_use]
    pub const fn mode(&self) -> ProgramMode {
        self.mode
    }

    #[must_use]
    pub const fn diagnostics(&self) -> &DiagnosticSnapshot {
        &self.diagnostics
    }

    #[must_use]
    pub const fn trace(&self) -> &CapabilityTrace {
        &self.trace
    }

    #[must_use]
    pub const fn manifest(&self) -> &SupportManifest {
        &self.manifest
    }

    #[must_use]
    pub fn observations(&self) -> &[PhaseObservation] {
        &self.observations
    }

    #[must_use]
    pub const fn checked_program(&self) -> Option<&CheckedProgram> {
        self.checked_program.as_ref()
    }

    #[must_use]
    pub fn into_checked_program(self) -> Option<CheckedProgram> {
        self.checked_program
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        ProgramMode,
        DiagnosticSnapshot,
        CapabilityTrace,
        SupportManifest,
        Box<[PhaseObservation]>,
        Option<CheckedProgram>,
    ) {
        (
            self.mode,
            self.diagnostics,
            self.trace,
            self.manifest,
            self.observations,
            self.checked_program,
        )
    }
}

/// One derivative HIR invalid fact tied to already-retained syntax roots.
#[derive(Clone, Copy)]
pub struct InvalidFact<'source> {
    occurrence: &'source InvalidOccurrence,
    poison: &'source Poison,
}

impl<'source> InvalidFact<'source> {
    /// Returns the structurally invalid HIR occurrence.
    #[must_use]
    pub const fn occurrence(self) -> &'source InvalidOccurrence {
        self.occurrence
    }

    /// Returns the exact merged syntax-root provenance.
    #[must_use]
    pub const fn poison(self) -> &'source Poison {
        self.poison
    }
}

/// Mutable compilation lifecycle over one canonical in-memory file set.
pub struct CompilerSession {
    entry: FileId,
    source_ids: Box<[FileId]>,
    sources: Box<[LoweredSource]>,
    diagnostics: DiagnosticStore,
    observations: ObservationCollector,
    build_input_blocked: bool,
    resolution_stage: ResolutionStage,
}

enum ResolutionStage {
    Pending,
    Complete(Box<ResolutionAnalysis>),
    Failed,
}

impl CompilerSession {
    /// Constructs the deterministic source, parse, HIR, and diagnostic session.
    pub fn build(request: FileSetRequest, options: SessionOptions) -> Result<Self, SessionError> {
        let mut diagnostics = DiagnosticStore::new();
        let mut observations = ObservationCollector::new(options.observe_phases);
        let mut pending = prepare_sources(&request)?;
        pending.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        for (index, source) in pending.iter_mut().enumerate() {
            source.file = FileId::new(
                u32::try_from(index).map_err(|_| SessionError::TooManySources(index + 1))?,
            );
        }
        let entry = pending
            .iter()
            .find(|source| source.path.as_ref() == request.entry())
            .map(|source| source.file)
            .expect("validated entry is one of the prepared sources");
        let source_ids = pending
            .iter()
            .map(|source| source.file)
            .collect::<Vec<_>>()
            .into_boxed_slice();

        if let Some(source) = pending
            .iter()
            .find(|source| source.module.has_reserved_root())
        {
            let file = source.source_file();
            record_module_root_reserved(&mut diagnostics, &source.label(), &file)?;
            return Ok(Self::blocked(entry, source_ids, diagnostics, observations));
        }
        if let Some((source, index, name)) = pending.iter().find_map(|source| {
            source
                .module
                .first_invalid_component()
                .map(|(index, name)| (source, index, name))
        }) {
            let file = source.source_file();
            record_module_component_invalid(&mut diagnostics, &source.label(), &file, name, index)?;
            return Ok(Self::blocked(entry, source_ids, diagnostics, observations));
        }

        let mut sources = Vec::with_capacity(pending.len());
        for source in pending {
            sources.push(lower_source(source, &mut diagnostics, &mut observations)?);
        }
        Ok(Self {
            entry,
            source_ids,
            sources: sources.into_boxed_slice(),
            diagnostics,
            observations,
            build_input_blocked: false,
            resolution_stage: ResolutionStage::Pending,
        })
    }

    /// Returns the designated entry compilation unit.
    #[must_use]
    pub const fn entry(&self) -> FileId {
        self.entry
    }

    /// Returns compilation units in canonical path and `FileId` order.
    #[must_use]
    pub fn sources(&self) -> &[LoweredSource] {
        &self.sources
    }

    /// Finds one compilation unit by session-local source identity.
    #[must_use]
    pub fn source(&self, file: FileId) -> Option<&LoweredSource> {
        self.sources
            .get(file.get() as usize)
            .filter(|source| source.file() == file)
    }

    /// Reports whether build-input diagnostics prevented all front-end work.
    #[must_use]
    pub const fn build_input_blocked(&self) -> bool {
        self.build_input_blocked
    }

    /// Returns phase observations in stable phase order.
    #[must_use]
    pub fn observations(&self) -> &[PhaseObservation] {
        self.observations.as_slice()
    }

    /// Returns the completed semantic resolver analysis, if this stage ran.
    #[must_use]
    pub const fn resolution_analysis(&self) -> Option<&ResolutionAnalysis> {
        match &self.resolution_stage {
            ResolutionStage::Complete(analysis) => Some(analysis),
            ResolutionStage::Pending | ResolutionStage::Failed => None,
        }
    }

    /// Resolves declarations, imports, names, and declaration signatures exactly once.
    pub fn resolve(
        &mut self,
        profile: &SupportProfile,
        catalog: Option<&TrustedCatalog>,
    ) -> Result<&ResolutionAnalysis, SessionError> {
        self.resolve_for(ProgramMode::Library, profile, catalog)
    }

    /// Resolves semantics under one invocation mode exactly once.
    pub fn resolve_for(
        &mut self,
        mode: ProgramMode,
        profile: &SupportProfile,
        catalog: Option<&TrustedCatalog>,
    ) -> Result<&ResolutionAnalysis, SessionError> {
        if self.build_input_blocked {
            return Err(SessionError::FrontendBlocked);
        }
        if !matches!(self.resolution_stage, ResolutionStage::Pending) {
            return Err(SessionError::ResolutionAlreadyAttempted);
        }
        self.resolution_stage = ResolutionStage::Failed;
        let analysis = resolve_sources(
            &self.sources,
            &self.source_ids,
            ResolutionInvocation::new(self.entry, mode),
            profile,
            catalog,
            &mut self.diagnostics,
            &mut self.observations,
        )?;
        self.resolution_stage = ResolutionStage::Complete(Box::new(analysis));
        let ResolutionStage::Complete(analysis) = &self.resolution_stage else {
            unreachable!("semantic resolution was just committed")
        };
        Ok(analysis)
    }

    /// Consumes all semantic state into the sole checked-program completion gate.
    pub fn seal(self, limit: Option<ErrorLimit>) -> Result<SealedCompilation, SessionError> {
        let Self {
            entry,
            sources,
            mut diagnostics,
            observations,
            resolution_stage,
            ..
        } = self;
        let ResolutionStage::Complete(analysis) = resolution_stage else {
            return Err(SessionError::ResolutionNotCompleted);
        };
        let (mode, resolution, trace, manifest) = (*analysis).into_parts();
        let checked_program = if diagnostics.has_errors() || trace.has_unsupported() {
            None
        } else if let Some(resolution) = resolution {
            let sources = sources
                .into_vec()
                .into_iter()
                .map(LoweredSource::into_program_source)
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            seal_checked_program(mode, entry, sources, resolution, &trace, &mut diagnostics)?
        } else {
            None
        };
        Ok(SealedCompilation {
            mode,
            diagnostics: diagnostics.finalize(limit),
            trace,
            manifest,
            observations: observations.into_boxed_slice(),
            checked_program,
        })
    }

    /// Schedules already-resolved import edges without performing resolution.
    pub fn plan_imports(
        &mut self,
        dependencies: impl IntoIterator<Item = ImportDependency>,
    ) -> Result<ImportSchedule, ImportScheduleError> {
        if self.build_input_blocked {
            return Err(ImportScheduleError::FrontendBlocked);
        }
        build_observed_import_schedule(&self.source_ids, dependencies, &mut self.observations)
    }

    /// Consumes the live diagnostic lifecycle into its ordered snapshot.
    #[must_use]
    pub fn into_diagnostics(self, limit: Option<ErrorLimit>) -> DiagnosticSnapshot {
        self.diagnostics.finalize(limit)
    }

    fn blocked(
        entry: FileId,
        source_ids: Box<[FileId]>,
        diagnostics: DiagnosticStore,
        observations: ObservationCollector,
    ) -> Self {
        Self {
            entry,
            source_ids,
            sources: Box::new([]),
            diagnostics,
            observations,
            build_input_blocked: true,
            resolution_stage: ResolutionStage::Pending,
        }
    }
}

struct PendingSource {
    file: FileId,
    path: Box<str>,
    label: SourceLabel,
    relative_path: Box<str>,
    bytes: std::sync::Arc<[u8]>,
    module: ModulePathCandidate,
}

impl PendingSource {
    fn label(&self) -> SourceLabel {
        self.label.clone()
    }

    fn source_file(&self) -> SourceFile {
        SourceFile::new(self.file, self.bytes.clone())
    }
}

fn prepare_sources(request: &FileSetRequest) -> Result<Vec<PendingSource>, SessionError> {
    request
        .files()
        .iter()
        .filter(|file| is_source_path(file.path.as_str()))
        .map(|source| {
            let relative = request
                .root()
                .relative_source_path(&source.path)
                .expect("validated request keeps every source below its root");
            Ok(PendingSource {
                file: FileId::new(0),
                path: source.path.as_str().into(),
                label: source
                    .source_label
                    .clone()
                    .expect("validated source mapping has a diagnostic label"),
                relative_path: relative.into(),
                bytes: source.bytes.clone(),
                module: ModulePathCandidate::from_relative_source(relative),
            })
        })
        .collect()
}

fn lower_source(
    source: PendingSource,
    diagnostics: &mut DiagnosticStore,
    observations: &mut ObservationCollector,
) -> Result<LoweredSource, SessionError> {
    let label = source.label();
    let source_file = source.source_file();
    let module_path = source.module.into_valid();
    let validated = match source_file.validate() {
        Ok(validated) => validated,
        Err(SourceValidationError::InvalidUtf8(span)) => {
            let poison = record_invalid_utf8(diagnostics, &label, span)?;
            return Ok(LoweredSource {
                path: source.path,
                label,
                module_path,
                source: source_file,
                parse: None,
                hir: None,
                source_poison: Some(poison),
                syntax_poisons: Box::new([]),
                invalid_poisons: Box::new([]),
                origin_spans: Box::new([]),
                header_poison: None,
            });
        }
        Err(SourceValidationError::TooLarge {
            actual_len,
            max_len,
            ..
        }) => {
            return Err(SessionError::SourceTooLarge {
                path: source.path,
                actual_len,
                max_len,
            });
        }
    };
    let lines = LineIndex::new(validated);
    let (parse, frontend_observations) = if observations.enabled() {
        let observed = parse_validated_observed(validated);
        observed.into_parts()
    } else {
        (
            parse_validated(validated),
            Box::<[eglur_syntax::FrontendPhaseObservation]>::default(),
        )
    };
    for observation in frontend_observations.iter().copied() {
        observations.record_frontend(observation);
    }
    let syntax_poisons = parse
        .diagnostics()
        .iter()
        .map(|&diagnostic| record_syntax(diagnostics, &label, &lines, diagnostic))
        .collect::<Result<Vec<_>, _>>()?;

    let hir = if observations.enabled() {
        let started = Instant::now();
        let hir = lower(&parse);
        observations.record(Phase::HirLowering, 1, 1, started.elapsed());
        hir
    } else {
        lower(&parse)
    };
    let invalid_poisons = hir
        .invalid_occurrences()
        .iter()
        .map(|occurrence| invalid_poison(&source.path, occurrence, &syntax_poisons))
        .collect::<Result<Vec<_>, _>>()?;
    let origin_spans = hir
        .origins()
        .map(|(_, origin)| SourceSpan::from_origin(label.clone(), origin, &lines))
        .collect::<Result<Vec<_>, _>>()?;
    let header_poison = hir
        .module()
        .and_then(|module| module.path().text())
        .filter(|declared| declared != module_path.as_str())
        .map(|_| {
            let module = hir
                .module()
                .expect("declared module path came from a module declaration");
            record_module_path_mismatch(
                diagnostics,
                &label,
                &lines,
                hir.origin(module.origin()).source(),
            )
        })
        .transpose()?;

    drop(lines);
    Ok(LoweredSource {
        path: source.path,
        label,
        module_path,
        source: source_file,
        parse: Some(parse),
        hir: Some(hir),
        source_poison: None,
        syntax_poisons: syntax_poisons.into_boxed_slice(),
        invalid_poisons: invalid_poisons.into_boxed_slice(),
        origin_spans: origin_spans.into_boxed_slice(),
        header_poison,
    })
}

fn invalid_poison(
    path: &str,
    occurrence: &InvalidOccurrence,
    syntax_poisons: &[Poison],
) -> Result<Poison, SessionError> {
    let mut causes = occurrence.syntax_causes().iter().copied();
    let first = causes
        .next()
        .ok_or_else(|| SessionError::UnreportedHirInvalid {
            path: path.into(),
            category: occurrence.category(),
        })?;
    let mut poison = syntax_poisons
        .get(first.get() as usize)
        .cloned()
        .ok_or_else(|| SessionError::MissingSyntaxCause {
            path: path.into(),
            cause: first,
        })?;
    for cause in causes {
        let next = syntax_poisons.get(cause.get() as usize).ok_or_else(|| {
            SessionError::MissingSyntaxCause {
                path: path.into(),
                cause,
            }
        })?;
        poison = poison.merge(next)?;
    }
    Ok(poison)
}

/// Internal compiler-session lifecycle failure without a language diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionError {
    TooManySources(usize),
    SourceTooLarge {
        path: Box<str>,
        actual_len: usize,
        max_len: usize,
    },
    DiagnosticRoot(DiagnosticRootError),
    DiagnosticStore(DiagnosticStoreError),
    SourceProjection(SourceProjectionError),
    UnreportedHirInvalid {
        path: Box<str>,
        category: InvalidCategory,
    },
    MissingSyntaxCause {
        path: Box<str>,
        cause: SyntaxCauseId,
    },
    FrontendBlocked,
    ResolutionAlreadyAttempted,
    ResolutionNotCompleted,
    MissingCheckedHir(FileId),
    UnexpectedUnperformedPhase(Phase),
    ModuleAddress(ModuleAddressError),
    SemanticSource(SourceInputError),
    DeclarationCollection(CollectionError),
    ImportSchedule(ImportScheduleError),
    ImportResolution(ImportResolutionError),
    NameResolution(NameResolutionError),
    SignatureResolution(SignatureResolutionError),
    TraitEnvironment(TraitEnvironmentError),
    BodyTyping(BodyTypingError),
    OwnershipAnalysis(OwnershipAnalysisError),
    TraitEvidence(TraitEvidenceError),
    EffectAnalysis(EffectAnalysisError),
    SemanticFingerprint(SemanticFingerprintError),
    AnalysisFingerprint(AnalysisFingerprintError),
    CapabilityTrace(CapabilityTraceError),
    ProgramSource(ProgramSourceError),
    ProgramSeal(ProgramSealError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManySources(count) => {
                write!(formatter, "source count {count} exceeds FileId capacity")
            }
            Self::SourceTooLarge {
                path,
                actual_len,
                max_len,
            } => write!(
                formatter,
                "source `{path}` has {actual_len} bytes; maximum supported length is {max_len}"
            ),
            Self::DiagnosticRoot(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
            Self::SourceProjection(error) => error.fmt(formatter),
            Self::UnreportedHirInvalid { path, category } => write!(
                formatter,
                "HIR source `{path}` contains unreported {category:?} invalid structure"
            ),
            Self::MissingSyntaxCause { path, cause } => write!(
                formatter,
                "HIR source `{path}` references missing syntax cause {}",
                cause.get()
            ),
            Self::FrontendBlocked => {
                formatter.write_str("semantic analysis is unavailable after build-input failure")
            }
            Self::ResolutionAlreadyAttempted => {
                formatter.write_str("semantic resolution was already attempted for this session")
            }
            Self::ResolutionNotCompleted => {
                formatter.write_str("semantic resolution must complete before program sealing")
            }
            Self::MissingCheckedHir(file) => {
                write!(formatter, "checked source {file:?} has no HIR product")
            }
            Self::UnexpectedUnperformedPhase(phase) => write!(
                formatter,
                "semantic phase {} unexpectedly produced no phase product",
                phase.as_str()
            ),
            Self::ModuleAddress(error) => error.fmt(formatter),
            Self::SemanticSource(error) => error.fmt(formatter),
            Self::DeclarationCollection(error) => error.fmt(formatter),
            Self::ImportSchedule(error) => error.fmt(formatter),
            Self::ImportResolution(error) => error.fmt(formatter),
            Self::NameResolution(error) => error.fmt(formatter),
            Self::SignatureResolution(error) => error.fmt(formatter),
            Self::TraitEnvironment(error) => error.fmt(formatter),
            Self::BodyTyping(error) => error.fmt(formatter),
            Self::OwnershipAnalysis(error) => error.fmt(formatter),
            Self::TraitEvidence(error) => error.fmt(formatter),
            Self::EffectAnalysis(error) => error.fmt(formatter),
            Self::SemanticFingerprint(error) => error.fmt(formatter),
            Self::AnalysisFingerprint(error) => error.fmt(formatter),
            Self::CapabilityTrace(error) => error.fmt(formatter),
            Self::ProgramSource(error) => error.fmt(formatter),
            Self::ProgramSeal(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DiagnosticRoot(error) => Some(error),
            Self::DiagnosticStore(error) => Some(error),
            Self::SourceProjection(error) => Some(error),
            Self::ModuleAddress(error) => Some(error),
            Self::SemanticSource(error) => Some(error),
            Self::DeclarationCollection(error) => Some(error),
            Self::ImportSchedule(error) => Some(error),
            Self::ImportResolution(error) => Some(error),
            Self::NameResolution(error) => Some(error),
            Self::SignatureResolution(error) => Some(error),
            Self::TraitEnvironment(error) => Some(error),
            Self::BodyTyping(error) => Some(error),
            Self::OwnershipAnalysis(error) => Some(error),
            Self::TraitEvidence(error) => Some(error),
            Self::EffectAnalysis(error) => Some(error),
            Self::SemanticFingerprint(error) => Some(error),
            Self::AnalysisFingerprint(error) => Some(error),
            Self::CapabilityTrace(error) => Some(error),
            Self::ProgramSource(error) => Some(error),
            Self::ProgramSeal(error) => Some(error),
            Self::TooManySources(_)
            | Self::SourceTooLarge { .. }
            | Self::UnreportedHirInvalid { .. }
            | Self::MissingSyntaxCause { .. }
            | Self::FrontendBlocked
            | Self::ResolutionAlreadyAttempted
            | Self::ResolutionNotCompleted
            | Self::MissingCheckedHir(_)
            | Self::UnexpectedUnperformedPhase(_) => None,
        }
    }
}

impl From<DiagnosticRootError> for SessionError {
    fn from(error: DiagnosticRootError) -> Self {
        Self::DiagnosticRoot(error)
    }
}

impl From<DiagnosticStoreError> for SessionError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}

impl From<SourceProjectionError> for SessionError {
    fn from(error: SourceProjectionError) -> Self {
        Self::SourceProjection(error)
    }
}

impl From<ModuleAddressError> for SessionError {
    fn from(error: ModuleAddressError) -> Self {
        Self::ModuleAddress(error)
    }
}

impl From<SourceInputError> for SessionError {
    fn from(error: SourceInputError) -> Self {
        Self::SemanticSource(error)
    }
}

impl From<ImportScheduleError> for SessionError {
    fn from(error: ImportScheduleError) -> Self {
        Self::ImportSchedule(error)
    }
}

impl From<CollectionError> for SessionError {
    fn from(error: CollectionError) -> Self {
        Self::DeclarationCollection(error)
    }
}

impl From<ImportResolutionError> for SessionError {
    fn from(error: ImportResolutionError) -> Self {
        Self::ImportResolution(error)
    }
}

impl From<NameResolutionError> for SessionError {
    fn from(error: NameResolutionError) -> Self {
        Self::NameResolution(error)
    }
}

impl From<SignatureResolutionError> for SessionError {
    fn from(error: SignatureResolutionError) -> Self {
        Self::SignatureResolution(error)
    }
}

impl From<SemanticFingerprintError> for SessionError {
    fn from(error: SemanticFingerprintError) -> Self {
        Self::SemanticFingerprint(error)
    }
}

impl From<AnalysisFingerprintError> for SessionError {
    fn from(error: AnalysisFingerprintError) -> Self {
        Self::AnalysisFingerprint(error)
    }
}

impl From<BodyTypingError> for SessionError {
    fn from(error: BodyTypingError) -> Self {
        Self::BodyTyping(error)
    }
}

impl From<OwnershipAnalysisError> for SessionError {
    fn from(error: OwnershipAnalysisError) -> Self {
        Self::OwnershipAnalysis(error)
    }
}

impl From<TraitEnvironmentError> for SessionError {
    fn from(error: TraitEnvironmentError) -> Self {
        Self::TraitEnvironment(error)
    }
}

impl From<TraitEvidenceError> for SessionError {
    fn from(error: TraitEvidenceError) -> Self {
        Self::TraitEvidence(error)
    }
}

impl From<EffectAnalysisError> for SessionError {
    fn from(error: EffectAnalysisError) -> Self {
        Self::EffectAnalysis(error)
    }
}

impl From<CapabilityTraceError> for SessionError {
    fn from(error: CapabilityTraceError) -> Self {
        Self::CapabilityTrace(error)
    }
}

impl From<ProgramSourceError> for SessionError {
    fn from(error: ProgramSourceError) -> Self {
        Self::ProgramSource(error)
    }
}

impl From<ProgramSealError> for SessionError {
    fn from(error: ProgramSealError) -> Self {
        Self::ProgramSeal(error)
    }
}
