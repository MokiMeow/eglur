#!/bin/sh

set -eu

usage() {
  echo "usage: compiler/tools/measure_frontend.sh timing|allocation" >&2
  exit 64
}

test "$#" -eq 1 || usage

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repository_root=$(CDPATH= cd -- "$script_directory/../.." && pwd)
log_file=$(mktemp "${TMPDIR:-/tmp}/eglur-frontend-measurement.XXXXXX")
trap 'rm -f "$log_file"' EXIT HUP INT TERM

case "$1" in
  timing)
    test_name=measurement::record_frontend_timing
    feature_arguments=
    ;;
  allocation)
    test_name=measurement::record_frontend_allocations
    feature_arguments=allocation-measurement
    ;;
  *)
    usage
    ;;
esac

cd "$repository_root"

if test -n "$feature_arguments"; then
  if ! cargo test --locked --release -p eglur-syntax --lib \
    --features "$feature_arguments" \
    "$test_name" -- \
    --ignored --exact --nocapture --test-threads=1 >"$log_file" 2>&1; then
    cat "$log_file" >&2
    exit 1
  fi
else
  if ! cargo test --locked --release -p eglur-syntax --lib \
    "$test_name" -- \
    --ignored --exact --nocapture --test-threads=1 >"$log_file" 2>&1; then
    cat "$log_file" >&2
    exit 1
  fi
fi

prefix=EGLUR_FRONTEND_MEASUREMENT_JSON=
match_count=$(grep -o "$prefix" "$log_file" | wc -l | tr -d ' ')
if test "$match_count" -ne 1; then
  cat "$log_file" >&2
  echo "expected exactly one front-end measurement report, found $match_count" >&2
  exit 1
fi

sed "/$prefix/d" "$log_file" >&2
sed -n "s/.*$prefix//p" "$log_file"
