use serde::Serialize;

pub(super) const REPORT_SCHEMA_VERSION: &str = "0.1";

#[derive(Debug, Serialize)]
pub(super) struct RevisionContext {
    pub(super) commit: String,
    pub(super) dirty: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct ToolchainContext {
    pub(super) rustc_command: String,
    pub(super) rustc_verbose: String,
    pub(super) rustc_host: String,
    pub(super) cargo_profile: &'static str,
    pub(super) cargo_lock_object_format: String,
    pub(super) cargo_lock_object_id: String,
}

#[derive(Debug, Serialize)]
pub(super) struct HostContext {
    pub(super) os: &'static str,
    pub(super) architecture: &'static str,
    pub(super) kernel: Option<String>,
    pub(super) cpu_model: Option<String>,
    pub(super) logical_parallelism: Option<usize>,
    pub(super) memory_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(super) struct ExecutionContext {
    pub(super) captured_at_unix_seconds: u64,
    pub(super) revision: RevisionContext,
    pub(super) toolchain: ToolchainContext,
    pub(super) host: HostContext,
}

#[derive(Debug, Serialize)]
pub(super) struct CorpusContext {
    pub(super) schema_version: String,
    pub(super) input_digest: String,
    pub(super) git_object_format: String,
    pub(super) git_object_id: String,
    pub(super) total_cases: usize,
    pub(super) source_validation_rejections: usize,
    pub(super) token_only_cases: usize,
    pub(super) lexer_cases: usize,
    pub(super) parser_cases: usize,
    pub(super) tree_build_cases: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct MeasurementConfig {
    pub(super) warmup_runs: usize,
    pub(super) sample_runs: usize,
    pub(super) corpus_iterations_per_run: usize,
    pub(super) outlier_filtering: &'static str,
    pub(super) percentile_method: &'static str,
}

#[derive(Debug, Serialize)]
pub(super) struct Distribution {
    unit: &'static str,
    samples: Vec<u64>,
    min: u64,
    median: u64,
    p95: u64,
    max: u64,
}

impl Distribution {
    pub(super) fn new(unit: &'static str, samples: Vec<u64>) -> Self {
        assert!(
            !samples.is_empty(),
            "measurement distributions need samples"
        );
        let mut ordered = samples.clone();
        ordered.sort_unstable();

        Self {
            unit,
            min: ordered[0],
            median: nearest_rank(&ordered, 50),
            p95: nearest_rank(&ordered, 95),
            max: ordered[ordered.len() - 1],
            samples,
        }
    }
}

fn nearest_rank(ordered: &[u64], percentile: usize) -> u64 {
    let rank = percentile
        .checked_mul(ordered.len())
        .and_then(|product| product.checked_add(99))
        .expect("sample rank should fit usize")
        / 100;
    ordered[rank.saturating_sub(1)]
}

#[derive(Debug, Serialize)]
pub(super) struct TimingPhase {
    pub(super) scope: &'static str,
    pub(super) measured_cases: usize,
    pub(super) duration: Distribution,
}

#[derive(Debug, Serialize)]
pub(super) struct TimingPhases {
    pub(super) lexer: TimingPhase,
    pub(super) parser_events: TimingPhase,
    pub(super) tree_build: TimingPhase,
}

#[derive(Debug, Serialize)]
pub(super) struct TimingReport {
    pub(super) schema_version: &'static str,
    pub(super) kind: &'static str,
    pub(super) claim_scope: &'static str,
    pub(super) execution: ExecutionContext,
    pub(super) corpus: CorpusContext,
    pub(super) configuration: MeasurementConfig,
    pub(super) phases: TimingPhases,
}

#[derive(Debug, Serialize)]
#[cfg(feature = "allocation-measurement")]
pub(super) struct AllocationMetrics {
    pub(super) allocations: Distribution,
    pub(super) deallocations: Distribution,
    pub(super) reallocations: Distribution,
    pub(super) bytes_allocated: Distribution,
    pub(super) bytes_deallocated: Distribution,
}

#[derive(Debug, Serialize)]
#[cfg(feature = "allocation-measurement")]
pub(super) struct AllocationPhase {
    pub(super) scope: &'static str,
    pub(super) measured_cases: usize,
    pub(super) metrics: AllocationMetrics,
}

#[derive(Debug, Serialize)]
#[cfg(feature = "allocation-measurement")]
pub(super) struct AllocationPhases {
    pub(super) lexer: AllocationPhase,
    pub(super) parser_events: AllocationPhase,
    pub(super) tree_build: AllocationPhase,
}

#[derive(Debug, Serialize)]
#[cfg(feature = "allocation-measurement")]
pub(super) struct AllocationReport {
    pub(super) schema_version: &'static str,
    pub(super) kind: &'static str,
    pub(super) claim_scope: &'static str,
    pub(super) allocator: &'static str,
    pub(super) execution: ExecutionContext,
    pub(super) corpus: CorpusContext,
    pub(super) configuration: MeasurementConfig,
    pub(super) phases: AllocationPhases,
}

#[cfg(test)]
mod tests {
    use super::Distribution;

    #[test]
    fn distribution_preserves_samples_and_uses_nearest_rank() {
        let distribution = Distribution::new("items", vec![50, 10, 40, 20, 30]);

        assert_eq!(distribution.samples, vec![50, 10, 40, 20, 30]);
        assert_eq!(distribution.min, 10);
        assert_eq!(distribution.median, 30);
        assert_eq!(distribution.p95, 50);
        assert_eq!(distribution.max, 50);
    }
}
