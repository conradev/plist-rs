#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "$0")/../.." && pwd)"

if rg --line-number \
  --glob '*.rs' \
  '(^|[^[:alnum:]_])(assume|admit)[[:space:]]*!?[[:space:]]*\(|assume_specification|external_(fn|type|trait)_specification|#\[verifier::external_body\]|#\[verifier::external\]|#\[verifier::assume_termination\]|#\[verifier::exec_allows_no_decreases_clause\]|^[[:space:]]*(pub[[:space:]]+)?(broadcast[[:space:]]+)?axiom[[:space:]]+fn' \
  "$root_dir/verification/verus"; then
  echo 'The Verus port contains a forbidden trusted shortcut.' >&2
  exit 1
fi

printf 'No trusted shortcut or termination bypass found.\n'
