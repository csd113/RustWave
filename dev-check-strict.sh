#!/usr/bin/env bash
# dev-check.sh — full Rust quality gate
# Runs: fmt · fix · clippy (pedantic+nursery) · tests · audit · deny · dupes
# Produces per-file clustered clippy reports in clippy_reports/

set -Eeuo pipefail

# ─── Colours ──────────────────────────────────────────────────────────────────

if [[ -t 1 ]]; then
    RED='\033[0;31m'; YELLOW='\033[0;33m'; GREEN='\033[0;32m'
    CYAN='\033[0;36m'; BOLD='\033[1m'; DIM='\033[2m'; RESET='\033[0m'
else
    RED=''; YELLOW=''; GREEN=''; CYAN=''; BOLD=''; DIM=''; RESET=''
fi

# ─── Globals ──────────────────────────────────────────────────────────────────

SCRIPT_START=$SECONDS
PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
FAILED_STEPS=()

REPORT_DIR="clippy_reports"
RAW_FILE="$REPORT_DIR/clippy_raw.txt"
CLUSTER_DIR="$REPORT_DIR/clusters"
SUMMARY_FILE="$REPORT_DIR/summary.txt"

# ─── Helpers ──────────────────────────────────────────────────────────────────

command_exists() { command -v "$1" >/dev/null 2>&1; }

step() {
    echo ""
    echo -e "${BOLD}${CYAN}════ $1 ════${RESET}"
}

pass() {
    echo -e "  ${GREEN}✓${RESET} $1"
    (( PASS_COUNT++ )) || true
}

fail() {
    echo -e "  ${RED}✗${RESET} $1"
    (( FAIL_COUNT++ )) || true
    FAILED_STEPS+=("$1")
}

skip() {
    echo -e "  ${DIM}–${RESET} $1 ${DIM}(skipped — tool not installed)${RESET}"
    (( SKIP_COUNT++ )) || true
}

warn() { echo -e "  ${YELLOW}⚠${RESET}  $1"; }

require_tool() {
    local tool="$1" install_hint="$2"
    if ! command_exists "$tool"; then
        echo -e "${RED}Error:${RESET} required tool '${BOLD}$tool${RESET}' is not installed."
        echo -e "       Install with: ${DIM}$install_hint${RESET}"
        exit 1
    fi
}

optional_tool() {
    local tool="$1" install_hint="$2"
    if ! command_exists "$tool"; then
        warn "'$tool' not installed — step will be skipped."
        warn "Install with: $install_hint"
    fi
}

elapsed() {
    local secs=$(( SECONDS - SCRIPT_START ))
    printf '%dm%02ds' $(( secs / 60 )) $(( secs % 60 ))
}

# ─── Header ───────────────────────────────────────────────────────────────────

echo -e "${BOLD}"
echo "╔══════════════════════════════════════════════════╗"
echo "║          Rust Full Quality Gate Check            ║"
echo "╚══════════════════════════════════════════════════╝"
echo -e "${RESET}"

# ─── CPU cores ────────────────────────────────────────────────────────────────

if command_exists sysctl; then
    export CARGO_BUILD_JOBS; CARGO_BUILD_JOBS=$(sysctl -n hw.ncpu 2>/dev/null || echo 4)
elif command_exists nproc; then
    export CARGO_BUILD_JOBS; CARGO_BUILD_JOBS=$(nproc)
else
    export CARGO_BUILD_JOBS=4
fi
echo -e "  ${DIM}Using ${BOLD}${CARGO_BUILD_JOBS}${RESET}${DIM} CPU cores${RESET}"

# ─── Required tools ───────────────────────────────────────────────────────────

step "Verifying required tools"

require_tool "cargo"         "https://rustup.rs"
require_tool "rustfmt"       "rustup component add rustfmt"
require_tool "clippy-driver" "rustup component add clippy"
pass "cargo · rustfmt · clippy all present"

optional_tool "cargo-audit" "cargo install cargo-audit"
optional_tool "cargo-deny"  "cargo install cargo-deny"
optional_tool "cargo-udeps" "cargo install cargo-udeps"
optional_tool "cargo-msrv"  "cargo install cargo-msrv"

# ─── Prepare report directory ─────────────────────────────────────────────────

rm -rf "$REPORT_DIR"
mkdir -p "$CLUSTER_DIR"

# ─── Optional: update deps ────────────────────────────────────────────────────

if [[ "${1:-}" == "--update" ]]; then
    step "Updating dependency index"
    if cargo update 2>&1; then pass "cargo update"; else fail "cargo update"; fi
fi

# ─── 1. Format ────────────────────────────────────────────────────────────────

step "1 · Formatting  (cargo fmt)"

if cargo fmt --all 2>&1; then
    pass "cargo fmt --all"
else
    fail "cargo fmt --all"
fi

# Verify nothing was left dirty (useful in CI)
if git diff --quiet 2>/dev/null; then
    pass "No unstaged format changes"
else
    warn "cargo fmt changed files — commit the formatted code"
fi

# ─── 2. Auto-fix ──────────────────────────────────────────────────────────────

step "2 · Automatic fixes  (cargo fix)"

if cargo fix --allow-dirty --allow-staged --allow-no-vcs --all-features 2>&1; then
    pass "cargo fix"
else
    warn "cargo fix had warnings (non-fatal)"
fi

# ─── 3. Clippy — strict pedantic+nursery ──────────────────────────────────────

step "3 · Lint  (cargo clippy — pedantic + nursery)"

CLIPPY_FLAGS=(
    # Hard errors
    "-D" "warnings"

    # Pedantic: correctness, performance, style improvements
    "-W" "clippy::pedantic"

    # Nursery: newer lints, some may be noisy — catches subtle bugs early
    "-W" "clippy::nursery"

    # Catch common correctness bugs missed by the default set
    "-W" "clippy::correctness"
    "-W" "clippy::suspicious"
    "-W" "clippy::complexity"
    "-W" "clippy::perf"

    # Panic/unwrap hygiene — forces explicit error handling
    "-W" "clippy::unwrap_used"
    "-W" "clippy::expect_used"
    "-W" "clippy::panic"
    "-W" "clippy::todo"
    "-W" "clippy::unimplemented"
    "-W" "clippy::unreachable"

    # Index panic risk
    "-W" "clippy::indexing_slicing"

    # Integer overflow in casts
    "-W" "clippy::cast_possible_truncation"
    "-W" "clippy::cast_possible_wrap"
    "-W" "clippy::cast_sign_loss"
    "-W" "clippy::cast_precision_loss"

    # Arithmetic that can panic
    "-W" "clippy::arithmetic_side_effects"

    # Formatting / style discipline
    "-W" "clippy::format_collect"
    "-W" "clippy::uninlined_format_args"
    "-W" "clippy::redundant_closure_for_method_calls"
    "-W" "clippy::map_unwrap_or"
    "-W" "clippy::manual_let_else"
    "-W" "clippy::single_match_else"
    "-W" "clippy::if_not_else"
    "-W" "clippy::option_if_let_else"
    "-W" "clippy::cloned_instead_of_copied"
    "-W" "clippy::doc_markdown"
    "-W" "clippy::redundant_else"
    "-W" "clippy::too_many_lines"
    "-W" "clippy::missing_errors_doc"
    "-W" "clippy::missing_panics_doc"
)

CLIPPY_CMD=(
    cargo clippy
    --all-targets
    --all-features
    --
    "${CLIPPY_FLAGS[@]}"
)

echo -e "  ${DIM}Running: ${CLIPPY_CMD[*]}${RESET}"
echo ""

CLIPPY_EXIT=0
"${CLIPPY_CMD[@]}" 2>&1 | tee "$RAW_FILE" || CLIPPY_EXIT=$?

# ── Cluster clippy output by source file ─────────────────────────────────────

echo ""
echo -e "  ${DIM}Clustering clippy output by file...${RESET}"

OUTFILE=""
while IFS= read -r line; do
    if [[ $line =~ ([a-zA-Z0-9_/.-]+\.rs):[0-9]+:[0-9]+ ]]; then
        FILE="${BASH_REMATCH[1]}"
        DIR=$(dirname "$FILE")
        if [[ "$DIR" == "." ]]; then
            CLUSTER="root"
        else
            CLUSTER=$(echo "$DIR" | tr '/' '_')
        fi
        OUTFILE="$CLUSTER_DIR/${CLUSTER}.txt"
        {
            echo ""
            echo "----------------------------------------"
            echo "FILE: $FILE"
            echo "----------------------------------------"
        } >> "$OUTFILE"
    fi
    if [[ -n "$OUTFILE" ]]; then
        echo "$line" >> "$OUTFILE"
    fi
done < "$RAW_FILE"

# ── Count errors and warnings ─────────────────────────────────────────────────

CLIPPY_ERRORS=$(grep -c '^error' "$RAW_FILE" 2>/dev/null || echo 0)
CLIPPY_WARNS=$(grep  -c '^warning' "$RAW_FILE" 2>/dev/null || echo 0)
CLUSTER_COUNT=$(find "$CLUSTER_DIR" -name '*.txt' | wc -l | tr -d ' ')

if [[ $CLIPPY_EXIT -eq 0 ]]; then
    pass "clippy clean  (${CLIPPY_WARNS} warnings, 0 errors)"
else
    fail "clippy reported ${CLIPPY_ERRORS} error(s) across ${CLUSTER_COUNT} file cluster(s)"
    echo ""
    echo -e "  ${BOLD}Cluster reports:${RESET}"
    for f in "$CLUSTER_DIR"/*.txt; do
        [[ -f "$f" ]] && echo -e "    ${DIM}$(basename "$f")${RESET}"
    done
    echo -e "  ${DIM}Full output: $RAW_FILE${RESET}"
fi

# ─── 4. Tests ─────────────────────────────────────────────────────────────────

step "4 · Tests  (cargo test)"

TEST_EXIT=0
cargo test --all --all-features 2>&1 || TEST_EXIT=$?

if [[ $TEST_EXIT -eq 0 ]]; then
    PASSED=$(grep -oP '\d+(?= passed)' <<< "$(cargo test --all --all-features 2>&1)" | tail -1 || echo "?")
    pass "All tests passed"
else
    fail "Test suite failed (exit $TEST_EXIT)"
fi

# ─── 5. Security audit ────────────────────────────────────────────────────────

step "5 · Security audit  (cargo audit)"

if command_exists cargo-audit; then
    AUDIT_EXIT=0
    cargo audit 2>&1 || AUDIT_EXIT=$?
    if [[ $AUDIT_EXIT -eq 0 ]]; then
        pass "No known vulnerabilities"
    else
        fail "cargo-audit found vulnerability/advisory — review output above"
    fi
else
    skip "cargo-audit  →  cargo install cargo-audit"
fi

# ─── 6. Dependency policy ─────────────────────────────────────────────────────

step "6 · Dependency policy  (cargo deny)"

if command_exists cargo-deny; then
    DENY_EXIT=0
    cargo deny check 2>&1 || DENY_EXIT=$?
    if [[ $DENY_EXIT -eq 0 ]]; then
        pass "cargo deny — all policies satisfied"
    else
        fail "cargo deny — policy violation(s) found"
    fi
else
    skip "cargo-deny  →  cargo install cargo-deny"
fi

# ─── 7. Unused dependencies ───────────────────────────────────────────────────

step "7 · Unused dependencies  (cargo udeps)"

if command_exists cargo-udeps; then
    UDEPS_EXIT=0
    cargo +nightly udeps --all-targets 2>&1 || UDEPS_EXIT=$?
    if [[ $UDEPS_EXIT -eq 0 ]]; then
        pass "No unused dependencies"
    else
        fail "Unused dependencies detected — remove them from Cargo.toml"
    fi
else
    skip "cargo-udeps  →  cargo install cargo-udeps  (requires nightly)"
fi

# ─── 8. MSRV check ────────────────────────────────────────────────────────────

step "8 · Minimum supported Rust version  (cargo msrv)"

if command_exists cargo-msrv; then
    MSRV_EXIT=0
    cargo msrv verify 2>&1 || MSRV_EXIT=$?
    if [[ $MSRV_EXIT -eq 0 ]]; then
        pass "MSRV satisfied"
    else
        warn "MSRV check failed — your rust-version in Cargo.toml may need updating"
    fi
else
    skip "cargo-msrv  →  cargo install cargo-msrv"
fi

# ─── 9. Duplicate dependencies ────────────────────────────────────────────────

step "9 · Duplicate dependencies  (cargo tree -d)"

DUPES=$(cargo tree -d 2>&1 || true)
if echo "$DUPES" | grep -q '\['; then
    warn "Duplicate crate versions detected:"
    echo "$DUPES" | grep '^\[' | sort -u | while read -r line; do
        echo -e "    ${YELLOW}$line${RESET}"
    done
else
    pass "No duplicate crate versions"
fi

# ─── 10. Build check (release) ────────────────────────────────────────────────

step "10 · Release build check  (cargo build --release)"

BUILD_EXIT=0
cargo build --release --all-features 2>&1 || BUILD_EXIT=$?
if [[ $BUILD_EXIT -eq 0 ]]; then
    pass "Release build clean"
else
    fail "Release build failed"
fi

# ─── Summary ──────────────────────────────────────────────────────────────────

TOTAL_SECS=$(( SECONDS - SCRIPT_START ))

{
    echo "dev-check summary — $(date)"
    echo "Duration: ${TOTAL_SECS}s"
    echo "Passed:   $PASS_COUNT"
    echo "Failed:   $FAIL_COUNT"
    echo "Skipped:  $SKIP_COUNT"
    if [[ ${#FAILED_STEPS[@]} -gt 0 ]]; then
        echo ""
        echo "Failed steps:"
        for s in "${FAILED_STEPS[@]}"; do echo "  - $s"; done
    fi
} | tee "$SUMMARY_FILE"

echo ""
if [[ $FAIL_COUNT -eq 0 ]]; then
    echo -e "${BOLD}${GREEN}"
    echo "╔══════════════════════════════════════════════════╗"
    echo "║  ✓  All checks passed  ($(elapsed()))                  ║"
    echo "╚══════════════════════════════════════════════════╝"
    echo -e "${RESET}"
    exit 0
else
    echo -e "${BOLD}${RED}"
    echo "╔══════════════════════════════════════════════════╗"
    echo "║  ✗  $FAIL_COUNT check(s) failed  ($(elapsed()))               ║"
    echo "╚══════════════════════════════════════════════════╝"
    echo -e "${RESET}"
    echo -e "${RED}Failed steps:${RESET}"
    for s in "${FAILED_STEPS[@]}"; do
        echo -e "  ${RED}•${RESET} $s"
    done
    echo ""
    echo -e "  ${DIM}Reports saved to: $REPORT_DIR/${RESET}"
    exit 1
fi
