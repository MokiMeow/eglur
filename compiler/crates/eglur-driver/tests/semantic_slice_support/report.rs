use std::path::Path;

use serde::Serialize;

use super::metadata::{ExecutionContext, execution_context, git_object_identity};
use super::model::SemanticFixtureCorpus;
use super::runner::SliceRun;

pub const PROBE_GATE_ENVIRONMENT: &str = "EGLUR_SEMANTIC_SLICE_PROBES_PASSED";
pub const PROBE_GATE_VALUE: &str = "verified-by-semantic-slice-runner-v1";

#[derive(Debug, Serialize)]
struct CorpusContext {
    path: String,
    schema_version: String,
    input_digest: String,
    git_object_format: String,
    git_object_id: String,
    fixture_count: usize,
}

#[derive(Debug, Serialize)]
struct ProbeRecord {
    id: &'static str,
    contract: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct RunSummary {
    fixtures: usize,
    succeeded: usize,
    rejected: usize,
}

#[derive(Debug, Serialize)]
struct SemanticSliceObservation<'run> {
    kind: &'static str,
    schema_version: &'static str,
    claim_scope: &'static str,
    execution: ExecutionContext,
    corpus: CorpusContext,
    probes: [ProbeRecord; 5],
    summary: RunSummary,
    run: &'run SliceRun,
}

pub fn observation_json(
    repository_root: &Path,
    corpus: &SemanticFixtureCorpus,
    run: &SliceRun,
) -> Result<String, String> {
    if std::env::var(PROBE_GATE_ENVIRONMENT).as_deref() != Ok(PROBE_GATE_VALUE) {
        return Err("semantic-slice observation requires the five-probe runner".into());
    }

    let (git_object_format, git_object_id) = git_object_identity(repository_root, &corpus.path)?;
    let relative_path = corpus
        .path
        .strip_prefix(repository_root)
        .map_err(|_| "semantic corpus is outside the repository".to_owned())?;
    let succeeded = run
        .fixtures
        .iter()
        .filter(|fixture| fixture.disposition == "succeeded")
        .count();
    let rejected = run.fixtures.len() - succeeded;
    let report = SemanticSliceObservation {
        kind: "eglur_semantic_slice_observation",
        schema_version: "0.1",
        claim_scope: "bounded semantic-core evidence; not v0.1 completeness or implementation conformance",
        execution: execution_context(repository_root)?,
        corpus: CorpusContext {
            path: relative_path.to_string_lossy().into_owned(),
            schema_version: corpus.schema_version.clone(),
            input_digest: corpus.input_digest.clone(),
            git_object_format,
            git_object_id,
            fixture_count: corpus.fixtures.len(),
        },
        probes: probe_records(),
        summary: RunSummary {
            fixtures: run.fixtures.len(),
            succeeded,
            rejected,
        },
        run,
    };

    serde_json::to_string(&report)
        .map(|json| format!("{json}\n"))
        .map_err(|error| format!("failed to serialize semantic-slice observation: {error}"))
}

fn probe_records() -> [ProbeRecord; 5] {
    [
        ProbeRecord {
            id: "import_scc",
            contract: "stable source identity, SCC scheduling, and import-cycle fingerprints",
            status: "passed",
        },
        ProbeRecord {
            id: "recursive_effect_scc",
            contract: "pure and effectful recursive SCC fixed points",
            status: "passed",
        },
        ProbeRecord {
            id: "diagnostic_roots_and_poison",
            contract: "independent roots, exact poison provenance, and cascade suppression",
            status: "passed",
        },
        ProbeRecord {
            id: "profile_and_fingerprint",
            contract: "closed support partition and semantic fingerprint stability",
            status: "passed",
        },
        ProbeRecord {
            id: "tensor_non_interference",
            contract: "tensor syntax remains an explicit downstream capability gap",
            status: "passed",
        },
    ]
}
