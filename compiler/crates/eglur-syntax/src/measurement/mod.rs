mod metadata;
mod report;

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eglur_source::{FileId, SourceFile, SourceValidationError, ValidatedSource};

use crate::event::Event;
use crate::parser::{ParseEntryPoint, parse_lexed};
use crate::test_corpus::{Corpus, EntryPoint, OutcomeKind, corpus_path, read_corpus, source_bytes};
use crate::tree::{GreenTree, build_tree, error_tree_events};
use crate::{Lexed, SyntaxDiagnostic, SyntaxPhase, lex};

use self::metadata::{execution_context, git_object_identity};
use self::report::{
    CorpusContext, Distribution, MeasurementConfig, REPORT_SCHEMA_VERSION, TimingPhase,
    TimingPhases, TimingReport,
};

#[cfg(feature = "allocation-measurement")]
use self::report::{AllocationMetrics, AllocationPhase, AllocationPhases, AllocationReport};

#[cfg(feature = "allocation-measurement")]
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, Stats, StatsAlloc};
#[cfg(feature = "allocation-measurement")]
use std::alloc::System;

const TIMING_WARMUP_RUNS: usize = 5;
const TIMING_SAMPLE_RUNS: usize = 30;
#[cfg(feature = "allocation-measurement")]
const ALLOCATION_WARMUP_RUNS: usize = 2;
#[cfg(feature = "allocation-measurement")]
const ALLOCATION_SAMPLE_RUNS: usize = 10;

const LEXER_SCOPE: &str =
    "all UTF-8-valid corpus sources; source validation and output drop excluded";
const PARSER_SCOPE: &str =
    "lex-success non-token sources; lexing, tree construction, and output drop excluded";
const TREE_SCOPE: &str =
    "the parser phase input set; lexing, parser-event construction, and output drop excluded";

#[cfg(feature = "allocation-measurement")]
#[global_allocator]
static GLOBAL_ALLOCATOR: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

struct PreparedCase {
    label: String,
    source: SourceFile,
    entrypoint: EntryPoint,
    expected_kind: OutcomeKind,
    expected_code: Option<String>,
}

struct PreparedCorpus {
    schema_version: String,
    input_digest: String,
    cases: Vec<PreparedCase>,
    counts: CorpusCounts,
}

#[derive(Clone, Copy)]
struct CorpusCounts {
    source_validation_rejections: usize,
    token_only_cases: usize,
    lexer_cases: usize,
    parser_cases: usize,
    tree_build_cases: usize,
}

#[derive(Clone, Copy)]
struct ValidatedCase<'source> {
    source: ValidatedSource<'source>,
    entrypoint: Option<ParseEntryPoint>,
}

struct LexProduct<'source> {
    entrypoint: Option<ParseEntryPoint>,
    lexed: Lexed<'source>,
}

struct ParseProduct {
    lexed_index: usize,
    events: Vec<Event>,
    diagnostics: Box<[SyntaxDiagnostic]>,
}

struct TreeProduct {
    tree: GreenTree,
    diagnostics: Box<[SyntaxDiagnostic]>,
}

#[derive(Clone, Copy)]
struct TimingSample {
    lexer_ns: u64,
    parser_ns: u64,
    tree_ns: u64,
}

impl PreparedCorpus {
    fn load_and_verify() -> Self {
        let Corpus {
            schema_version,
            input_digest,
            cases,
        } = read_corpus();
        assert_eq!(schema_version, "0.1");
        assert!(input_digest.starts_with("sha256:"));

        let cases = cases
            .into_iter()
            .enumerate()
            .map(|(index, case)| PreparedCase {
                label: case.origin.label(),
                source: SourceFile::new(
                    FileId::new(u32::try_from(index).expect("fixture count should fit u32")),
                    source_bytes(case.source),
                ),
                entrypoint: case.entrypoint,
                expected_kind: case.expectation.kind,
                expected_code: case.expectation.diagnostic_code,
            })
            .collect::<Vec<_>>();
        let counts = verify_cases(&cases);

        Self {
            schema_version,
            input_digest,
            cases,
            counts,
        }
    }

    fn validated_cases(&self) -> Vec<ValidatedCase<'_>> {
        self.cases
            .iter()
            .filter_map(|case| match case.source.validate() {
                Ok(source) => Some(ValidatedCase {
                    source,
                    entrypoint: parse_entrypoint(case.entrypoint),
                }),
                Err(SourceValidationError::InvalidUtf8(_)) => None,
                Err(error @ SourceValidationError::TooLarge { .. }) => {
                    panic!("{}: unexpected source rejection: {error}", case.label)
                }
            })
            .collect()
    }

    fn context(&self, repository_root: &Path) -> CorpusContext {
        let (git_object_format, git_object_id) =
            git_object_identity(repository_root, &corpus_path())
                .expect("corpus identity should be available");
        CorpusContext {
            schema_version: self.schema_version.clone(),
            input_digest: self.input_digest.clone(),
            git_object_format,
            git_object_id,
            total_cases: self.cases.len(),
            source_validation_rejections: self.counts.source_validation_rejections,
            token_only_cases: self.counts.token_only_cases,
            lexer_cases: self.counts.lexer_cases,
            parser_cases: self.counts.parser_cases,
            tree_build_cases: self.counts.tree_build_cases,
        }
    }
}

fn verify_cases(cases: &[PreparedCase]) -> CorpusCounts {
    let mut source_validation_rejections = 0;
    let mut token_only_cases = 0;
    let mut lexer_cases = 0;
    let mut parser_cases = 0;
    let mut tree_build_cases = 0;

    for case in cases {
        if case.entrypoint == EntryPoint::Tokens {
            token_only_cases += 1;
        }
        let source = match case.source.validate() {
            Ok(source) => source,
            Err(SourceValidationError::InvalidUtf8(_)) => {
                source_validation_rejections += 1;
                assert_eq!(
                    case.expected_kind,
                    OutcomeKind::LexicalFailure,
                    "{}",
                    case.label
                );
                assert_eq!(
                    case.expected_code.as_deref(),
                    Some("LEX_INVALID_UTF8"),
                    "{}",
                    case.label
                );
                continue;
            }
            Err(error @ SourceValidationError::TooLarge { .. }) => {
                panic!("{}: unexpected source rejection: {error}", case.label)
            }
        };

        lexer_cases += 1;
        let lexed = lex(source);
        if !lexed.is_success() {
            assert_expected_diagnostic(
                case,
                OutcomeKind::LexicalFailure,
                SyntaxPhase::Lex,
                lexed.diagnostics(),
            );
            if case.entrypoint != EntryPoint::Tokens {
                let tree = build_tree(&lexed, error_tree_events(&lexed));
                assert_eq!(tree.root(source.id()).text().as_bytes(), source.bytes());
            }
            continue;
        }

        if case.entrypoint == EntryPoint::Tokens {
            assert_eq!(case.expected_kind, OutcomeKind::Success, "{}", case.label);
            continue;
        }

        parser_cases += 1;
        tree_build_cases += 1;
        let entrypoint =
            parse_entrypoint(case.entrypoint).expect("non-token case has an entrypoint");
        let (events, diagnostics) = parse_lexed(&lexed, entrypoint);
        match case.expected_kind {
            OutcomeKind::Success => assert!(diagnostics.is_empty(), "{}", case.label),
            OutcomeKind::ParseFailure => assert_expected_diagnostic(
                case,
                OutcomeKind::ParseFailure,
                SyntaxPhase::Parse,
                &diagnostics,
            ),
            OutcomeKind::LexicalFailure => {
                panic!(
                    "{}: expected lexical failure but lexing succeeded",
                    case.label
                )
            }
        }
        let tree = build_tree(&lexed, events);
        assert_eq!(tree.root(source.id()).text().as_bytes(), source.bytes());
    }

    CorpusCounts {
        source_validation_rejections,
        token_only_cases,
        lexer_cases,
        parser_cases,
        tree_build_cases,
    }
}

fn assert_expected_diagnostic(
    case: &PreparedCase,
    expected_kind: OutcomeKind,
    expected_phase: SyntaxPhase,
    diagnostics: &[SyntaxDiagnostic],
) {
    assert_eq!(case.expected_kind, expected_kind, "{}", case.label);
    assert_eq!(diagnostics.len(), 1, "{}", case.label);
    assert_eq!(diagnostics[0].phase(), expected_phase, "{}", case.label);
    assert_eq!(
        Some(diagnostics[0].kind().code()),
        case.expected_code.as_deref(),
        "{}",
        case.label
    );
}

fn parse_entrypoint(entrypoint: EntryPoint) -> Option<ParseEntryPoint> {
    match entrypoint {
        EntryPoint::CompilationUnit => Some(ParseEntryPoint::CompilationUnit),
        EntryPoint::Tokens => None,
        EntryPoint::Expression => Some(ParseEntryPoint::Expression),
        EntryPoint::ExpressionSequence => Some(ParseEntryPoint::ExpressionSequence),
        EntryPoint::ControlHead => Some(ParseEntryPoint::ControlHead),
    }
}

fn lex_into<'source>(cases: &[ValidatedCase<'source>], products: &mut Vec<LexProduct<'source>>) {
    for case in cases {
        let lexed = lex(black_box(case.source));
        black_box(&lexed);
        products.push(LexProduct {
            entrypoint: case.entrypoint,
            lexed,
        });
    }
}

fn parser_input_indices(lexed: &[LexProduct<'_>]) -> Vec<usize> {
    lexed
        .iter()
        .enumerate()
        .filter_map(|(index, product)| {
            (product.entrypoint.is_some() && product.lexed.is_success()).then_some(index)
        })
        .collect()
}

fn parse_into(lexed: &[LexProduct<'_>], input_indices: &[usize], products: &mut Vec<ParseProduct>) {
    for &lexed_index in input_indices {
        let product = &lexed[lexed_index];
        let entrypoint = product
            .entrypoint
            .expect("parser input selection requires an entrypoint");
        let (events, diagnostics) = parse_lexed(black_box(&product.lexed), entrypoint);
        black_box((&events, &diagnostics));
        products.push(ParseProduct {
            lexed_index,
            events,
            diagnostics,
        });
    }
}

fn build_trees(
    lexed: &[LexProduct<'_>],
    parsed: Vec<ParseProduct>,
    products: &mut Vec<TreeProduct>,
) {
    for product in parsed {
        let tree = build_tree(&lexed[product.lexed_index].lexed, product.events);
        black_box(&tree);
        products.push(TreeProduct {
            tree,
            diagnostics: product.diagnostics,
        });
    }
}

fn timing_sample(cases: &[ValidatedCase<'_>]) -> TimingSample {
    let mut lexed = Vec::with_capacity(cases.len());
    let lexer_started = Instant::now();
    lex_into(cases, &mut lexed);
    let lexer_ns = duration_ns(lexer_started.elapsed());

    let parser_inputs = parser_input_indices(&lexed);
    let mut parsed = Vec::with_capacity(parser_inputs.len());
    let parser_started = Instant::now();
    parse_into(&lexed, &parser_inputs, &mut parsed);
    let parser_ns = duration_ns(parser_started.elapsed());

    let mut trees = Vec::with_capacity(parsed.len());
    let tree_started = Instant::now();
    build_trees(&lexed, parsed, &mut trees);
    let tree_ns = duration_ns(tree_started.elapsed());
    observe_trees(&trees);

    TimingSample {
        lexer_ns,
        parser_ns,
        tree_ns,
    }
}

fn duration_ns(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).expect("one corpus phase should fit u64 nanoseconds")
}

fn observe_trees(products: &[TreeProduct]) {
    for product in products {
        black_box((&product.tree, &product.diagnostics));
    }
}

fn timing_report(corpus: &PreparedCorpus, repository_root: &Path) -> TimingReport {
    let cases = corpus.validated_cases();
    for _ in 0..TIMING_WARMUP_RUNS {
        black_box(timing_sample(&cases));
    }
    let samples = (0..TIMING_SAMPLE_RUNS)
        .map(|_| timing_sample(&cases))
        .collect::<Vec<_>>();

    TimingReport {
        schema_version: REPORT_SCHEMA_VERSION,
        kind: "frontend_phase_timing",
        claim_scope: "fixed-corpus regression evidence; not a throughput or release performance claim",
        execution: execution_context(repository_root)
            .expect("execution metadata should be available"),
        corpus: corpus.context(repository_root),
        configuration: configuration(TIMING_WARMUP_RUNS, TIMING_SAMPLE_RUNS),
        phases: TimingPhases {
            lexer: TimingPhase {
                scope: LEXER_SCOPE,
                measured_cases: corpus.counts.lexer_cases,
                duration: Distribution::new(
                    "nanoseconds_per_corpus_run",
                    samples.iter().map(|sample| sample.lexer_ns).collect(),
                ),
            },
            parser_events: TimingPhase {
                scope: PARSER_SCOPE,
                measured_cases: corpus.counts.parser_cases,
                duration: Distribution::new(
                    "nanoseconds_per_corpus_run",
                    samples.iter().map(|sample| sample.parser_ns).collect(),
                ),
            },
            tree_build: TimingPhase {
                scope: TREE_SCOPE,
                measured_cases: corpus.counts.tree_build_cases,
                duration: Distribution::new(
                    "nanoseconds_per_corpus_run",
                    samples.iter().map(|sample| sample.tree_ns).collect(),
                ),
            },
        },
    }
}

#[cfg(feature = "allocation-measurement")]
#[derive(Clone, Copy)]
struct AllocationSample {
    lexer: Stats,
    parser: Stats,
    tree: Stats,
}

#[cfg(feature = "allocation-measurement")]
fn allocation_sample(cases: &[ValidatedCase<'_>]) -> AllocationSample {
    let mut lexed = Vec::with_capacity(cases.len());
    let lexer_region = Region::new(GLOBAL_ALLOCATOR);
    lex_into(cases, &mut lexed);
    let lexer = lexer_region.change();

    let parser_inputs = parser_input_indices(&lexed);
    let mut parsed = Vec::with_capacity(parser_inputs.len());
    let parser_region = Region::new(GLOBAL_ALLOCATOR);
    parse_into(&lexed, &parser_inputs, &mut parsed);
    let parser = parser_region.change();

    let mut trees = Vec::with_capacity(parsed.len());
    let tree_region = Region::new(GLOBAL_ALLOCATOR);
    build_trees(&lexed, parsed, &mut trees);
    let tree = tree_region.change();
    observe_trees(&trees);

    AllocationSample {
        lexer,
        parser,
        tree,
    }
}

#[cfg(feature = "allocation-measurement")]
fn allocation_report(corpus: &PreparedCorpus, repository_root: &Path) -> AllocationReport {
    let cases = corpus.validated_cases();
    for _ in 0..ALLOCATION_WARMUP_RUNS {
        black_box(allocation_sample(&cases));
    }
    let samples = (0..ALLOCATION_SAMPLE_RUNS)
        .map(|_| allocation_sample(&cases))
        .collect::<Vec<_>>();

    AllocationReport {
        schema_version: REPORT_SCHEMA_VERSION,
        kind: "frontend_phase_allocations",
        claim_scope: "stats_alloc construction counters for fixed-corpus regression evidence; output drops, peak memory, RSS, and throughput are not measured",
        allocator: "stats_alloc 0.1.10 over std::alloc::System",
        execution: execution_context(repository_root)
            .expect("execution metadata should be available"),
        corpus: corpus.context(repository_root),
        configuration: configuration(ALLOCATION_WARMUP_RUNS, ALLOCATION_SAMPLE_RUNS),
        phases: AllocationPhases {
            lexer: allocation_phase(LEXER_SCOPE, corpus.counts.lexer_cases, &samples, |sample| {
                sample.lexer
            }),
            parser_events: allocation_phase(
                PARSER_SCOPE,
                corpus.counts.parser_cases,
                &samples,
                |sample| sample.parser,
            ),
            tree_build: allocation_phase(
                TREE_SCOPE,
                corpus.counts.tree_build_cases,
                &samples,
                |sample| sample.tree,
            ),
        },
    }
}

#[cfg(feature = "allocation-measurement")]
fn allocation_phase(
    scope: &'static str,
    measured_cases: usize,
    samples: &[AllocationSample],
    select: impl Fn(AllocationSample) -> Stats,
) -> AllocationPhase {
    let selected = samples.iter().copied().map(select).collect::<Vec<_>>();
    AllocationPhase {
        scope,
        measured_cases,
        metrics: AllocationMetrics {
            allocations: allocation_distribution("operations_per_corpus_run", &selected, |stats| {
                stats.allocations
            }),
            deallocations: allocation_distribution(
                "operations_per_corpus_run",
                &selected,
                |stats| stats.deallocations,
            ),
            reallocations: allocation_distribution(
                "operations_per_corpus_run",
                &selected,
                |stats| stats.reallocations,
            ),
            bytes_allocated: allocation_distribution("bytes_per_corpus_run", &selected, |stats| {
                stats.bytes_allocated
            }),
            bytes_deallocated: allocation_distribution(
                "bytes_per_corpus_run",
                &selected,
                |stats| stats.bytes_deallocated,
            ),
        },
    }
}

#[cfg(feature = "allocation-measurement")]
fn allocation_distribution(
    unit: &'static str,
    samples: &[Stats],
    select: impl Fn(Stats) -> usize,
) -> Distribution {
    Distribution::new(
        unit,
        samples
            .iter()
            .copied()
            .map(select)
            .map(|value| u64::try_from(value).expect("allocation counter should fit u64"))
            .collect(),
    )
}

fn configuration(warmup_runs: usize, sample_runs: usize) -> MeasurementConfig {
    MeasurementConfig {
        warmup_runs,
        sample_runs,
        corpus_iterations_per_run: 1,
        outlier_filtering: "none",
        percentile_method: "nearest_rank",
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("syntax crate should be nested under repository/compiler/crates")
        .to_path_buf()
}

fn emit_report(report: &impl serde::Serialize) {
    let payload = serde_json::to_string(report).expect("measurement report should serialize");
    println!("EGLUR_FRONTEND_MEASUREMENT_JSON={payload}");
}

fn assert_release_profile() {
    assert!(
        release_profile_enabled(),
        "front-end evidence must be recorded with cargo test --release"
    );
}

#[cfg(debug_assertions)]
fn release_profile_enabled() -> bool {
    false
}

#[cfg(not(debug_assertions))]
fn release_profile_enabled() -> bool {
    true
}

#[cfg(feature = "allocation-measurement")]
fn timing_allocator_is_instrumented() -> bool {
    true
}

#[cfg(not(feature = "allocation-measurement"))]
fn timing_allocator_is_instrumented() -> bool {
    false
}

#[test]
#[ignore = "manual fixed-corpus timing evidence"]
fn record_frontend_timing() {
    assert_release_profile();
    assert!(
        !timing_allocator_is_instrumented(),
        "timing evidence must not enable allocator instrumentation"
    );
    let corpus = PreparedCorpus::load_and_verify();
    emit_report(&timing_report(&corpus, &repository_root()));
}

#[cfg(feature = "allocation-measurement")]
#[test]
#[ignore = "manual fixed-corpus allocation evidence"]
fn record_frontend_allocations() {
    assert_release_profile();
    let corpus = PreparedCorpus::load_and_verify();
    emit_report(&allocation_report(&corpus, &repository_root()));
}
