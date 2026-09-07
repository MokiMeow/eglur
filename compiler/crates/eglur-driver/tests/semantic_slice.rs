mod semantic_slice_support;

use std::fs;
use std::path::{Path, PathBuf};

use semantic_slice_support::model::SemanticFixtureCorpus;
use semantic_slice_support::report::observation_json;
use semantic_slice_support::runner::run_corpus;

const SNAPSHOT_PREFIX: &str = "EGLUR_SEMANTIC_SLICE_SNAPSHOT_JSON=";
const OBSERVATION_PREFIX: &str = "EGLUR_SEMANTIC_SLICE_OBSERVATION_JSON=";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

#[test]
fn core_v1_corpus_matches_frozen_attempts_reproducibly() {
    let repository_root = repository_root();
    let corpus = SemanticFixtureCorpus::load(&repository_root).unwrap();
    let first = run_corpus(&corpus).snapshot_json();
    let second = run_corpus(&corpus).snapshot_json();

    assert_eq!(
        first, second,
        "semantic-slice outcomes are not reproducible"
    );
    let snapshot_path = repository_root.join("tests/semantic/core-v1.observed.json");
    let frozen = fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", snapshot_path.display()));
    assert_eq!(first, frozen, "semantic-slice frozen attempts are stale");
}

#[test]
#[ignore = "emits the deterministic snapshot for an intentional review update"]
fn record_core_v1_snapshot() {
    let repository_root = repository_root();
    let corpus = SemanticFixtureCorpus::load(&repository_root).unwrap();
    let snapshot = run_corpus(&corpus).snapshot_json();

    println!("{SNAPSHOT_PREFIX}{snapshot}");
}

#[test]
#[ignore = "emits an environment-bound observation after the five-probe runner"]
fn record_core_v1_observation() {
    let repository_root = repository_root();
    let corpus = SemanticFixtureCorpus::load(&repository_root).unwrap();
    let run = run_corpus(&corpus);
    let observation = observation_json(&repository_root, &corpus, &run).unwrap();

    println!("{OBSERVATION_PREFIX}{observation}");
}
