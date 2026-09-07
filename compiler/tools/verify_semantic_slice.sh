#!/bin/sh

set -eu

usage() {
  echo "usage: compiler/tools/verify_semantic_slice.sh verify|snapshot|report" >&2
  exit 64
}

test "$#" -eq 1 || usage

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/../.." && pwd)
log_file=$(mktemp "${TMPDIR:-/tmp}/eglur-semantic-slice.XXXXXX")
trap 'rm -f "$log_file"' EXIT HUP INT TERM

run_group() {
  name=$1
  shift
  printf '\n[%s]\n' "$name" >&2
  "$@" >&2
}

verify() {
  run_group corpus \
    cargo test --locked -p eglur-driver --test semantic_slice \
    core_v1_corpus_matches_frozen_attempts_reproducibly -- --exact

  run_group import-scc-source-identity \
    cargo test --locked -p eglur-driver --test session \
    shuffled_file_sets_preserve_source_identity_and_scc_schedule -- --exact
  run_group import-scc-fingerprint \
    cargo test --locked -p eglur-driver --test fingerprints \
    import_cycle_has_one_nonrecursive_analysis_key -- --exact

  run_group recursive-effect-pure \
    cargo test --locked -p eglur-driver --test effects \
    pure_mutual_recursion_converges_to_one_pure_scc -- --exact
  run_group recursive-effect-effectful \
    cargo test --locked -p eglur-driver --test effects \
    recursive_scc_propagates_io_to_every_member -- --exact

  run_group diagnostic-roots \
    cargo test --locked -p eglur-diagnostics --test store
  run_group diagnostic-poison \
    cargo test --locked -p eglur-driver --test session \
    parser_recovery_retains_valid_declarations_and_exact_poison -- --exact

  run_group support-profile \
    cargo test --locked -p eglur-driver --test support
  run_group semantic-fingerprints \
    cargo test --locked -p eglur-driver --test fingerprints

  run_group non-tensor-non-interference \
    cargo test --locked -p eglur-driver --test semantic \
    ordinary_tensor_contextual_spellings_do_not_activate_tensor_analysis -- --exact
  run_group tensor-activation-boundary \
    cargo test --locked -p eglur-driver --test semantic \
    tensor_syntax_demands_downstream_analysis_without_a_tensor_semantic_product -- --exact
}

emit_report() {
  prefix=$1
  test_name=$2
  shift 2

  if ! "$@" cargo test --locked -p eglur-driver --test semantic_slice \
    "$test_name" -- --ignored --exact --nocapture --test-threads=1 \
    >"$log_file" 2>&1; then
    cat "$log_file" >&2
    exit 1
  fi

  match_count=$(grep -o "$prefix" "$log_file" | wc -l | tr -d ' ')
  if test "$match_count" -ne 1; then
    cat "$log_file" >&2
    echo "expected exactly one $prefix report, found $match_count" >&2
    exit 1
  fi

  sed "/$prefix/d" "$log_file" >&2
  sed -n "s/.*$prefix//p" "$log_file"
}

cd "$repository_root"

case "$1" in
  verify)
    verify
    ;;
  snapshot)
    emit_report \
      EGLUR_SEMANTIC_SLICE_SNAPSHOT_JSON= \
      record_core_v1_snapshot \
      env
    ;;
  report)
    verify
    emit_report \
      EGLUR_SEMANTIC_SLICE_OBSERVATION_JSON= \
      record_core_v1_observation \
      env EGLUR_SEMANTIC_SLICE_PROBES_PASSED=verified-by-semantic-slice-runner-v1
    ;;
  *)
    usage
    ;;
esac
