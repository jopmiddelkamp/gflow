#!/usr/bin/env bash
# Stop-hook TDD gate. Blocks ending a turn when Rust code changed and either
# tests fail, coverage is incomplete, or line coverage falls below the baseline.
# The baseline (.claude/hooks/coverage-baseline.txt) only ever moves up;
# lowering it requires explicit user approval (see CLAUDE.md, TDD Policy).
set -u
export LC_ALL=C
cd "${CLAUDE_PROJECT_DIR:-$(pwd)}" || exit 0
git rev-parse --git-dir >/dev/null 2>&1 || exit 0
command -v jq >/dev/null 2>&1 || exit 0

BASELINE_FILE=.claude/hooks/coverage-baseline.txt
CACHE_FILE=.claude/hooks/.tdd-gate-pass

# The cache covers committed inputs, pending Rust changes, and gate policy.
# Untracked paths need content hashes: git diff omits them, and status reports
# only their names. NUL-delimited paths preserve spaces and newlines.
state_hash() {
  {
    git rev-parse HEAD:src HEAD:tests HEAD:Cargo.toml HEAD:Cargo.lock 2>/dev/null
    git diff HEAD -- '*.rs' Cargo.toml Cargo.lock 2>/dev/null
    git status --porcelain -- '*.rs' Cargo.toml Cargo.lock 2>/dev/null
    git ls-files --others --exclude-standard -z -- '*.rs' Cargo.toml Cargo.lock 2>/dev/null |
      while IFS= read -r -d '' path; do
        shasum -a 256 -- "$path"
      done
    shasum -a 256 -- "$0"
  } | shasum -a 256 | cut -d' ' -f1
}

INPUT_HASH=$(state_hash)
BASELINE=$(cat "$BASELINE_FILE" 2>/dev/null || echo 0)
HASH="$INPUT_HASH:$BASELINE"
if [ -f "$CACHE_FILE" ] && [ "$(cat "$CACHE_FILE")" = "$HASH" ]; then
  exit 0
fi

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  jq -n '{systemMessage: "tdd-gate: cargo-llvm-cov not installed (brew install cargo-llvm-cov) — coverage gate SKIPPED"}'
  exit 0
fi

ERR_FILE=$(mktemp)
trap 'rm -f "$ERR_FILE"' EXIT
COV_JSON=$(cargo llvm-cov --json --summary-only 2>"$ERR_FILE")
if [ $? -ne 0 ]; then
  jq -n --arg tail "$(tail -40 "$ERR_FILE")" \
    '{decision: "block", reason: ("TDD gate: `cargo llvm-cov` failed — tests are red or the build is broken. Fix it before finishing (TDD: tests must be green at turn end).\n\n" + $tail)}'
  exit 0
fi

if ! printf '%s' "$COV_JSON" | jq -e '
  .data[0].totals | [.lines, .functions, .regions] |
  all(.[];
    (.count | type) == "number" and .count >= 0 and .count == (.count | floor) and
    .covered == .count and
    (.percent | type) == "number" and .percent >= 0 and .percent <= 100
  )
' >/dev/null 2>&1; then
  jq -n '{decision: "block", reason: "TDD gate: lines, functions, and regions must each have every item covered (exactly 100%). Coverage counts are incomplete, missing, or malformed. Add tests and re-run `cargo llvm-cov --summary-only` before finishing."}'
  exit 0
fi

PCT=$(printf '%s' "$COV_JSON" | jq -r '.data[0].totals.lines.percent')

if awk "BEGIN{exit !($PCT < $BASELINE - 0.005)}"; then
  GAPS=$(cargo llvm-cov report --summary-only 2>/dev/null | awk 'NF>10 && $1!="Filename" && $1!~/^-+$/ && $10+0<100 {printf "  %s  %s lines\n", $1, $10}')
  jq -n --arg pct "$(printf '%.2f' "$PCT")" --arg base "$BASELINE" --arg gaps "$GAPS" \
    '{decision: "block", reason: ("TDD gate: total line coverage " + $pct + "% is below the ratchet baseline " + $base + "%. Coverage may never decrease. Add tests (test-first, per CLAUDE.md TDD Policy) until it is back at or above the baseline. Do NOT edit the baseline file. Files under 100%:\n" + $gaps + "\nIf this is genuinely impossible (e.g. only adapter code changed), stop and explain the situation to the user instead of looping.")}'
  exit 0
fi

if awk "BEGIN{exit !($PCT > $BASELINE + 0.005)}"; then
  printf '%.2f\n' "$PCT" > "$BASELINE_FILE"
  printf '%s:%.2f\n' "$INPUT_HASH" "$PCT" > "$CACHE_FILE"
  jq -n --arg pct "$(printf '%.2f' "$PCT")" '{systemMessage: ("tdd-gate: coverage ratcheted up to " + $pct + "%")}'
  exit 0
fi

echo "$HASH" > "$CACHE_FILE"
exit 0
