use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use eglur_diagnostics::{
    Diagnostic, DiagnosticCode, DiagnosticFailure, DiagnosticPhase, DiagnosticRoot,
    DiagnosticRootError, DiagnosticStore, DiagnosticStoreError, PrimarySpan, RelatedOrder,
    RelatedSpan, Severity, SourceLabel, SourceSpan,
};
use eglur_hir::{HirFile, OriginId};
use eglur_source::{FileId, SourceFile};

use crate::{
    BodyTypes, CallableSignatureId, CapabilityTrace, DeclarationCollection, DeclarationKind,
    DeclarationSignature, DeclarationSignatures, DefId, EffectFacts, FactState, ImportPlan,
    InhabitanceFacts, IntrinsicNominal, ModuleAddress, NameResolution, NominalHead, OwnershipFacts,
    PhaseProduct, PrimitiveType, ResolvedImports, SemanticFingerprints, TraitEnvironment,
    TraitEvidence, TypeId, TypeKind, TypeStoreError,
};

/// Invocation semantics that determine whether a source entrypoint is required.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramMode {
    Executable,
    Library,
    Test,
}

impl ProgramMode {
    #[must_use]
    pub const fn requires_entry(self) -> bool {
        matches!(self, Self::Executable)
    }
}

/// Owned source and HIR retained by a successfully checked program.
pub struct ProgramSource {
    label: SourceLabel,
    source: SourceFile,
    module: ModuleAddress,
    hir: HirFile,
    origin_spans: BTreeMap<OriginId, SourceSpan>,
}

impl ProgramSource {
    pub fn new(
        label: SourceLabel,
        source: SourceFile,
        module: ModuleAddress,
        hir: HirFile,
        origin_spans: impl IntoIterator<Item = (OriginId, SourceSpan)>,
    ) -> Result<Self, ProgramSourceError> {
        if hir.file() != source.id() {
            return Err(ProgramSourceError::HirFileMismatch);
        }
        let mut retained_spans = BTreeMap::new();
        for (origin, span) in origin_spans {
            if retained_spans.insert(origin, span).is_some() {
                return Err(ProgramSourceError::DuplicateOriginSpan);
            }
        }
        let expected_origins = hir
            .origins()
            .map(|(origin, _)| origin)
            .collect::<BTreeSet<_>>();
        if retained_spans.keys().copied().collect::<BTreeSet<_>>() != expected_origins {
            return Err(ProgramSourceError::OriginSpanCoverage);
        }
        if retained_spans
            .values()
            .any(|span| span.file_span().file() != source.id())
        {
            return Err(ProgramSourceError::SpanFileMismatch);
        }
        Ok(Self {
            label,
            source,
            module,
            hir,
            origin_spans: retained_spans,
        })
    }

    #[must_use]
    pub const fn file(&self) -> FileId {
        self.source.id()
    }

    #[must_use]
    pub const fn label(&self) -> &SourceLabel {
        &self.label
    }

    #[must_use]
    pub const fn source(&self) -> &SourceFile {
        &self.source
    }

    #[must_use]
    pub const fn module(&self) -> &ModuleAddress {
        &self.module
    }

    #[must_use]
    pub const fn hir(&self) -> &HirFile {
        &self.hir
    }

    #[must_use]
    pub fn origin_span(&self, origin: OriginId) -> Option<&SourceSpan> {
        self.origin_spans.get(&origin)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramSourceError {
    HirFileMismatch,
    DuplicateOriginSpan,
    OriginSpanCoverage,
    SpanFileMismatch,
}

impl fmt::Display for ProgramSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HirFileMismatch => {
                formatter.write_str("checked-program HIR belongs to another file")
            }
            Self::DuplicateOriginSpan => {
                formatter.write_str("checked-program source repeated a HIR origin span")
            }
            Self::OriginSpanCoverage => {
                formatter.write_str("checked-program source spans do not exactly cover HIR origins")
            }
            Self::SpanFileMismatch => {
                formatter.write_str("checked-program source span belongs to another file")
            }
        }
    }
}

impl std::error::Error for ProgramSourceError {}

/// Complete resolver products for one semantic compilation graph.
pub struct SemanticResolution {
    declarations: DeclarationCollection,
    import_plan: ImportPlan,
    imports: ResolvedImports,
    names: NameResolution,
    signatures: DeclarationSignatures,
    inhabitance: InhabitanceFacts,
    trait_environment: TraitEnvironment,
    body_types: BodyTypes,
    ownership_facts: PhaseProduct<OwnershipFacts>,
    trait_evidence: PhaseProduct<TraitEvidence>,
    effect_facts: PhaseProduct<EffectFacts>,
    fingerprints: Option<SemanticFingerprints>,
}

impl SemanticResolution {
    #[allow(
        clippy::too_many_arguments,
        reason = "the aggregate preserves each independently owned semantic product"
    )]
    pub fn new(
        declarations: DeclarationCollection,
        import_plan: ImportPlan,
        imports: ResolvedImports,
        names: NameResolution,
        signatures: DeclarationSignatures,
        inhabitance: InhabitanceFacts,
        trait_environment: TraitEnvironment,
        body_types: BodyTypes,
        ownership_facts: PhaseProduct<OwnershipFacts>,
        trait_evidence: PhaseProduct<TraitEvidence>,
        effect_facts: PhaseProduct<EffectFacts>,
        fingerprints: Option<SemanticFingerprints>,
    ) -> Self {
        Self {
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
            fingerprints,
        }
    }

    #[must_use]
    pub const fn declarations(&self) -> &DeclarationCollection {
        &self.declarations
    }

    #[must_use]
    pub const fn import_plan(&self) -> &ImportPlan {
        &self.import_plan
    }

    #[must_use]
    pub const fn imports(&self) -> &ResolvedImports {
        &self.imports
    }

    #[must_use]
    pub const fn names(&self) -> &NameResolution {
        &self.names
    }

    #[must_use]
    pub const fn signatures(&self) -> &DeclarationSignatures {
        &self.signatures
    }

    #[must_use]
    pub const fn inhabitance(&self) -> &InhabitanceFacts {
        &self.inhabitance
    }

    #[must_use]
    pub const fn trait_environment(&self) -> &TraitEnvironment {
        &self.trait_environment
    }

    #[must_use]
    pub const fn body_types(&self) -> &BodyTypes {
        &self.body_types
    }

    #[must_use]
    pub const fn ownership_facts(&self) -> &PhaseProduct<OwnershipFacts> {
        &self.ownership_facts
    }

    #[must_use]
    pub const fn trait_evidence(&self) -> &PhaseProduct<TraitEvidence> {
        &self.trait_evidence
    }

    #[must_use]
    pub const fn effect_facts(&self) -> &PhaseProduct<EffectFacts> {
        &self.effect_facts
    }

    #[must_use]
    pub const fn fingerprints(&self) -> Option<&SemanticFingerprints> {
        self.fingerprints.as_ref()
    }

    fn validate_complete(&self) -> Result<(), ProgramSealError> {
        if self.declarations.has_poisoned_declarations()
            || !self.import_plan.is_complete_for(&self.declarations)
            || !self.imports.is_complete_for(&self.declarations)
            || self.names.has_poisoned_facts()
            || self.signatures.has_poisoned_facts()
            || self.trait_environment.has_poisoned_implementations()
            || self.body_types.has_poisoned_facts()
        {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        }

        for module in self.declarations.modules() {
            for declaration in module.declarations() {
                match self.signatures.source_declaration(declaration.id()) {
                    FactState::Known(DeclarationSignature::Callable(signature)) => {
                        let signature = self
                            .signatures
                            .callable(*signature)
                            .ok_or(ProgramSealError::MissingCallableSignature(*signature))?;
                        if signature.body().is_some()
                            && matches!(
                                self.body_types.body(declaration.id()),
                                FactState::Unperformed | FactState::Poisoned(_)
                            )
                        {
                            return Err(ProgramSealError::IncompleteSemanticFacts);
                        }
                    }
                    FactState::Known(DeclarationSignature::Const { .. })
                        if matches!(
                            self.body_types.body(declaration.id()),
                            FactState::Unperformed | FactState::Poisoned(_)
                        ) =>
                    {
                        return Err(ProgramSealError::IncompleteSemanticFacts);
                    }
                    FactState::Poisoned(_) => {
                        return Err(ProgramSealError::IncompleteSemanticFacts);
                    }
                    FactState::Unperformed
                        if declaration.kind() != DeclarationKind::GenericParameter =>
                    {
                        return Err(ProgramSealError::IncompleteSemanticFacts);
                    }
                    FactState::Known(_) | FactState::Unperformed | FactState::NotApplicable => {}
                }
            }
        }

        let PhaseProduct::Performed(ownership) = &self.ownership_facts else {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        };
        if !ownership.is_complete() || ownership.has_poisoned_facts() {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        }
        let PhaseProduct::Performed(evidence) = &self.trait_evidence else {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        };
        if !evidence.is_complete_for(&self.body_types) {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        }
        let PhaseProduct::Performed(effects) = &self.effect_facts else {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        };
        if effects.has_poisoned_facts() {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        }
        if self.fingerprints.is_none() {
            return Err(ProgramSealError::IncompleteSemanticFacts);
        }
        for module in self.declarations.modules() {
            for declaration in module.declarations().filter(|declaration| {
                matches!(
                    declaration.kind(),
                    DeclarationKind::Function | DeclarationKind::ExternFunction
                )
            }) {
                if matches!(
                    effects.source_summary(declaration.id()),
                    FactState::Unperformed | FactState::Poisoned(_)
                ) {
                    return Err(ProgramSealError::IncompleteSemanticFacts);
                }
            }
        }
        Ok(())
    }
}

/// Normalized result contract of a validated executable entrypoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckedEntryResult {
    Unit,
    Result { success: TypeId, error: TypeId },
    Never,
}

/// Sole selected executable entrypoint after signature validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CheckedEntry {
    declaration: DefId,
    signature: CallableSignatureId,
    result: CheckedEntryResult,
}

impl CheckedEntry {
    #[must_use]
    pub const fn declaration(self) -> DefId {
        self.declaration
    }

    #[must_use]
    pub const fn signature(self) -> CallableSignatureId {
        self.signature
    }

    #[must_use]
    pub const fn result(self) -> CheckedEntryResult {
        self.result
    }
}

/// Evaluator authorization capability containing only complete checked semantics.
pub struct CheckedProgram {
    mode: ProgramMode,
    sources: Box<[ProgramSource]>,
    resolution: Box<SemanticResolution>,
    entry: Option<CheckedEntry>,
}

impl CheckedProgram {
    #[must_use]
    pub const fn mode(&self) -> ProgramMode {
        self.mode
    }

    #[must_use]
    pub fn sources(&self) -> &[ProgramSource] {
        &self.sources
    }

    #[must_use]
    pub const fn resolution(&self) -> &SemanticResolution {
        &self.resolution
    }

    #[must_use]
    pub const fn entry(&self) -> Option<CheckedEntry> {
        self.entry
    }
}

/// Validates and consumes all facts required to authorize evaluator access.
pub fn seal_checked_program(
    mode: ProgramMode,
    entry_source: FileId,
    sources: Box<[ProgramSource]>,
    resolution: SemanticResolution,
    trace: &CapabilityTrace,
    diagnostics: &mut DiagnosticStore,
) -> Result<Option<CheckedProgram>, ProgramSealError> {
    if diagnostics.has_errors() || trace.has_missing() {
        return Ok(None);
    }
    validate_source_set(&sources, &resolution)?;
    let entry = if mode.requires_entry() {
        validate_entry(entry_source, &sources, &resolution, diagnostics)?
    } else {
        None
    };
    if diagnostics.has_errors() {
        return Ok(None);
    }
    resolution.validate_complete()?;
    Ok(Some(CheckedProgram {
        mode,
        sources,
        resolution: Box::new(resolution),
        entry,
    }))
}

fn validate_source_set(
    sources: &[ProgramSource],
    resolution: &SemanticResolution,
) -> Result<(), ProgramSealError> {
    if sources.len() != resolution.declarations.modules().len() {
        return Err(ProgramSealError::SourceSetMismatch);
    }
    let mut files = BTreeSet::new();
    for source in sources {
        if !files.insert(source.file()) {
            return Err(ProgramSealError::DuplicateProgramSource(source.file()));
        }
        let module = resolution
            .declarations
            .module_for_file(source.file())
            .and_then(|module| resolution.declarations.module(module))
            .ok_or(ProgramSealError::SourceSetMismatch)?;
        if module.address() != source.module() {
            return Err(ProgramSealError::SourceSetMismatch);
        }
    }
    Ok(())
}

fn validate_entry(
    entry_source: FileId,
    sources: &[ProgramSource],
    resolution: &SemanticResolution,
    diagnostics: &mut DiagnosticStore,
) -> Result<Option<CheckedEntry>, ProgramSealError> {
    let source = sources
        .iter()
        .find(|source| source.file() == entry_source)
        .ok_or(ProgramSealError::MissingEntrySource(entry_source))?;
    let module = resolution
        .declarations
        .module_for_file(entry_source)
        .and_then(|module| resolution.declarations.module(module))
        .ok_or(ProgramSealError::MissingEntryModule(entry_source))?;
    let candidates = module
        .declarations()
        .filter(|declaration| {
            declaration.parent().is_none()
                && declaration.kind() == DeclarationKind::Function
                && declaration
                    .name()
                    .is_some_and(|name| name.text() == "main" && name.is_bang())
        })
        .collect::<Vec<_>>();

    if candidates.is_empty() {
        record_program_error(
            diagnostics,
            "PROGRAM_ENTRY_MISSING",
            "program entry is missing",
            SourceSpan::source_start(source.label.clone(), &source.source),
            [],
        )?;
        return Ok(None);
    }
    if candidates.len() > 1 {
        let first = required_origin_span(source, candidates[0].origin())?;
        let second = required_origin_span(source, candidates[1].origin())?;
        let related = RelatedSpan::new(first, "first program entry declared here")
            .expect("static related-span label is non-empty");
        record_program_error(
            diagnostics,
            "PROGRAM_ENTRY_DUPLICATE",
            "duplicate program entry declaration",
            second,
            [related],
        )?;
        return Ok(None);
    }

    let candidate = candidates[0];
    let signature_id = match resolution.signatures.source_declaration(candidate.id()) {
        FactState::Known(DeclarationSignature::Callable(signature)) => *signature,
        FactState::Known(_)
        | FactState::Poisoned(_)
        | FactState::Unperformed
        | FactState::NotApplicable => return Err(ProgramSealError::IncompleteSemanticFacts),
    };
    let signature = resolution
        .signatures
        .callable(signature_id)
        .ok_or(ProgramSealError::MissingCallableSignature(signature_id))?;
    let result = classify_entry_result(resolution.signatures.types(), signature.result())?;
    if !signature.generic_parameters().is_empty()
        || !signature.parameters().is_empty()
        || result.is_none()
    {
        record_program_error(
            diagnostics,
            "PROGRAM_ENTRY_SIGNATURE_INVALID",
            "program entry signature is invalid",
            required_origin_span(source, candidate.origin())?,
            [],
        )?;
        return Ok(None);
    }
    Ok(Some(CheckedEntry {
        declaration: candidate.id(),
        signature: signature_id,
        result: result.expect("entry result was checked above"),
    }))
}

fn classify_entry_result(
    types: &crate::CanonicalTypeStore,
    result: TypeId,
) -> Result<Option<CheckedEntryResult>, ProgramSealError> {
    match types.kind(result)? {
        TypeKind::Never => Ok(Some(CheckedEntryResult::Never)),
        TypeKind::Primitive(PrimitiveType::Unit) => Ok(Some(CheckedEntryResult::Unit)),
        TypeKind::Nominal {
            head: NominalHead::Intrinsic(IntrinsicNominal::Res),
            substitution,
        } => {
            let arguments = types.substitution(*substitution)?;
            let [success, error] = arguments else {
                return Err(ProgramSealError::InvalidResultArity(arguments.len()));
            };
            if types.contains_reference(*success)?
                || types.contains_reference(*error)?
                || types.contains_ownership(*success)?
                || types.contains_ownership(*error)?
            {
                return Ok(None);
            }
            Ok(Some(CheckedEntryResult::Result {
                success: *success,
                error: *error,
            }))
        }
        _ => Ok(None),
    }
}

fn required_origin_span(
    source: &ProgramSource,
    origin: OriginId,
) -> Result<SourceSpan, ProgramSealError> {
    source
        .origin_span(origin)
        .cloned()
        .ok_or(ProgramSealError::MissingOriginSpan {
            file: source.file(),
            origin,
        })
}

fn record_program_error(
    diagnostics: &mut DiagnosticStore,
    code: &'static str,
    message: &'static str,
    primary: SourceSpan,
    related: impl IntoIterator<Item = RelatedSpan>,
) -> Result<(), ProgramSealError> {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::new(code).expect("program diagnostic code is statically valid"),
        Severity::Error,
        DiagnosticPhase::new("program").expect("program is a valid diagnostic phase"),
        message,
        PrimarySpan::Source(primary),
    )
    .with_related(related, RelatedOrder::CodeDefined);
    let root = DiagnosticRoot::source(diagnostic)?;
    diagnostics.record_failure(DiagnosticFailure::Root(root))?;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramSealError {
    DuplicateProgramSource(FileId),
    SourceSetMismatch,
    MissingEntrySource(FileId),
    MissingEntryModule(FileId),
    MissingOriginSpan { file: FileId, origin: OriginId },
    MissingCallableSignature(CallableSignatureId),
    InvalidResultArity(usize),
    IncompleteSemanticFacts,
    TypeStore(TypeStoreError),
    DiagnosticRoot(DiagnosticRootError),
    DiagnosticStore(DiagnosticStoreError),
}

impl fmt::Display for ProgramSealError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateProgramSource(file) => {
                write!(formatter, "duplicate checked-program source {file:?}")
            }
            Self::SourceSetMismatch => formatter
                .write_str("checked-program sources do not match semantic resolution modules"),
            Self::MissingEntrySource(file) => write!(formatter, "missing entry source {file:?}"),
            Self::MissingEntryModule(file) => {
                write!(formatter, "missing entry module for {file:?}")
            }
            Self::MissingOriginSpan { file, origin } => {
                write!(
                    formatter,
                    "missing source span for {file:?} origin {origin:?}"
                )
            }
            Self::MissingCallableSignature(signature) => {
                write!(formatter, "missing callable signature {signature:?}")
            }
            Self::InvalidResultArity(actual) => {
                write!(formatter, "intrinsic Res has invalid arity {actual}")
            }
            Self::IncompleteSemanticFacts => {
                formatter.write_str("semantic products are not complete enough to seal a program")
            }
            Self::TypeStore(error) => error.fmt(formatter),
            Self::DiagnosticRoot(error) => error.fmt(formatter),
            Self::DiagnosticStore(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ProgramSealError {}

impl From<TypeStoreError> for ProgramSealError {
    fn from(error: TypeStoreError) -> Self {
        Self::TypeStore(error)
    }
}

impl From<DiagnosticRootError> for ProgramSealError {
    fn from(error: DiagnosticRootError) -> Self {
        Self::DiagnosticRoot(error)
    }
}

impl From<DiagnosticStoreError> for ProgramSealError {
    fn from(error: DiagnosticStoreError) -> Self {
        Self::DiagnosticStore(error)
    }
}
