#!/bin/bash

# AIKIT Test Runner Script
# Runs build, fmt, clippy, and tests (cargo-nextest); captures results and generates statistics.
# Matches CI: full workspace build (catches compile errors in all crates e.g. aikit-py), fmt, clippy, then tests with retries.
#
# Usage: ./run-tests.sh [OPTIONS]
#
# Options:
#   -o, --output FILE    Output markdown report file (default: test_results.md)
#   -j, --json FILE      JSON results file (default: test_results.json)
#   -d, --output-dir DIR Directory for raw fmt/clippy/test .txt outputs (default: .github/test-outputs)
#   -r, --retries N      Number of retries for flaky tests (default: 3)
#   -h, --help           Show this help message

set -e  # Exit on any error

# Default values
OUTPUT_FILE="test_results.md"
JSON_FILE="test_results.json"
RETRIES=3
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to print error and exit
error_exit() {
    echo -e "${RED}Error: $1${NC}" >&2
    echo "Usage: $0 [OPTIONS]" >&2
    echo "" >&2
    echo "Options:" >&2
    echo "  -o, --output FILE    Output markdown report file (default: test_results.md)" >&2
    echo "  -j, --json FILE      JSON results file (default: test_results.json)" >&2
    echo "  -d, --output-dir DIR Directory for raw fmt/clippy/test .txt outputs (default: .github/test-outputs)" >&2
    echo "  -r, --retries N      Number of retries for flaky tests (default: 3)" >&2
    echo "  -h, --help           Show this help message" >&2
    exit 1
}

# Function to check if command exists
check_command() {
    local cmd=$1
    local description=$2
    if ! command -v "$cmd" >/dev/null 2>&1; then
        error_exit "$description ($cmd) is not installed or not in PATH. Please install it first."
    fi
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -o|--output)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        -j|--json)
            JSON_FILE="$2"
            shift 2
            ;;
        -d|--output-dir)
            TEST_OUTPUT_DIR_ARG="$2"
            shift 2
            ;;
        -r|--retries)
            RETRIES="$2"
            shift 2
            ;;
        -h|--help)
            echo "AIKIT Test Runner Script"
            echo ""
            echo "Runs fmt, clippy, and tests (cargo-nextest); captures results and generates statistics"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -o, --output FILE    Output markdown report file (default: test_results.md)"
            echo "  -j, --json FILE      JSON results file (default: test_results.json)"
            echo "  -d, --output-dir DIR Directory for raw fmt/clippy/test .txt outputs (default: .github/test-outputs)"
            echo "  -r, --retries N      Number of retries for flaky tests (default: 3)"
            echo "  -h, --help           Show this help message"
            echo ""
            echo "Requirements:"
            echo "  - rustfmt, clippy (same as CI), cargo-nextest: cargo install cargo-nextest"
            echo ""
            echo "Install requirements:"
            echo "  cargo install cargo-nextest"
            exit 0
            ;;
        *)
            error_exit "Unknown option: $1"
            ;;
    esac
done

# Check dependencies
echo -e "${YELLOW}Checking dependencies...${NC}" >&2

check_command "cargo" "Cargo (Rust package manager)"
check_command "cargo-nextest" "cargo-nextest (install with: cargo install cargo-nextest)"

echo -e "${GREEN}All dependencies found!${NC}" >&2
echo "" >&2

# Change to the AIKIT directory (assuming script is run from there)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AIKIT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${YELLOW}Running in: $AIKIT_DIR${NC}" >&2
cd "$AIKIT_DIR"

# Ensure output directory exists for test results
if [[ -n "${TEST_OUTPUT_DIR_ARG:-}" ]]; then
    TEST_OUTPUT_DIR="$TEST_OUTPUT_DIR_ARG"
else
    TEST_OUTPUT_DIR=".github/test-outputs"
fi
mkdir -p "$TEST_OUTPUT_DIR"
echo -e "${YELLOW}Test outputs will be saved to: $TEST_OUTPUT_DIR${NC}" >&2
echo "" >&2

# Run build, fmt, clippy, and tests; capture output and exit codes (do not exit on first failure)
set +e

echo -e "${YELLOW}Running cargo build --workspace --all-targets --all-features...${NC}" >&2
BUILD_OUTPUT=$(cargo build --workspace --all-targets --all-features 2>&1)
BUILD_EXIT=$?
echo "$BUILD_OUTPUT" > "$TEST_OUTPUT_DIR/build-output.txt"
if [ "$BUILD_EXIT" -ne 0 ]; then
    echo -e "${RED}Workspace build failed (compile errors in any crate, e.g. aikit-py, will fail CI).${NC}" >&2
fi

echo -e "${YELLOW}Running cargo fmt --check...${NC}" >&2
FMT_OUTPUT=$(cargo fmt --check 2>&1)
FMT_EXIT=$?
echo "$FMT_OUTPUT" > "$TEST_OUTPUT_DIR/fmt-output.txt"

echo -e "${YELLOW}Running cargo clippy --workspace --all-targets --all-features -- -D warnings...${NC}" >&2
CLIPPY_OUTPUT=$(cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1)
CLIPPY_EXIT=$?
echo "$CLIPPY_OUTPUT" > "$TEST_OUTPUT_DIR/clippy-output.txt"

echo -e "${YELLOW}Running tests with cargo-nextest (retries: $RETRIES, per-test timeout: 60s)...${NC}" >&2
# Use nextest config from .config/nextest.toml for timeout settings
# --test-threads=1 to avoid resource contention in integration tests
TEST_OUTPUT=$(cargo nextest run --all-features --retries "$RETRIES" --fail-fast --test-threads=1 2>&1)
TEST_EXIT=$?
echo "$TEST_OUTPUT" > "$TEST_OUTPUT_DIR/test-output.txt"

echo -e "${YELLOW}Running cargo test --lib --release (CI multiplatform job)...${NC}" >&2
LIB_RELEASE_OUTPUT=$(cargo test --lib --release -- --test-threads=1 2>&1)
LIB_RELEASE_EXIT=$?
echo "$LIB_RELEASE_OUTPUT" > "$TEST_OUTPUT_DIR/test-lib-release-output.txt"
if [ "$LIB_RELEASE_EXIT" -ne 0 ]; then
    echo -e "${RED}Lib release tests failed (same command as CI test-multiplatform on Windows/macOS).${NC}" >&2
fi

set -e

# Overall pass only if build, fmt, clippy, and tests passed
EXIT_CODE=0
[ "$BUILD_EXIT" -ne 0 ] && EXIT_CODE=1
[ "$FMT_EXIT" -ne 0 ] && EXIT_CODE=1
[ "$CLIPPY_EXIT" -ne 0 ] && EXIT_CODE=1
[ "$TEST_EXIT" -ne 0 ] && EXIT_CODE=1
[ "$LIB_RELEASE_EXIT" -ne 0 ] && EXIT_CODE=1

if [ "$EXIT_CODE" -eq 0 ]; then
    echo -e "${GREEN}All checks and tests passed.${NC}" >&2
else
    [ "$BUILD_EXIT" -ne 0 ] && echo -e "${RED}Workspace build failed.${NC}" >&2
    [ "$FMT_EXIT" -ne 0 ] && echo -e "${RED}Format check failed.${NC}" >&2
    [ "$CLIPPY_EXIT" -ne 0 ] && echo -e "${RED}Clippy failed.${NC}" >&2
    [ "$TEST_EXIT" -ne 0 ] && echo -e "${RED}Some tests failed.${NC}" >&2
    [ "$LIB_RELEASE_EXIT" -ne 0 ] && echo -e "${RED}Lib release tests failed.${NC}" >&2
fi
echo "" >&2

# Parse test results from output
echo -e "${YELLOW}Parsing test results...${NC}" >&2

# Check if there are compilation errors (no tests were run)
if echo "$TEST_OUTPUT" | grep -q "error\[" || echo "$TEST_OUTPUT" | grep -q "could not compile"; then
    echo -e "${YELLOW}Compilation errors detected - no tests could run${NC}" >&2
    PASSED=0
    FAILED=0
    SKIPPED=0
    TOTAL=0
    PASSING_PERCENTAGE=0
    STATS_AVAILABLE=false
    COMPILATION_FAILED=true
else
    COMPILATION_FAILED=false

    # Look for summary line like: "Summary [   0.039s] 20 tests run: 20 passed, 0 failed, 0 skipped"
    SUMMARY_LINE=$(echo "$TEST_OUTPUT" | grep "Summary.*tests run:" | head -1)

    if [ -n "$SUMMARY_LINE" ]; then
        # Extract numbers from summary line
        PASSED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) passed.*/\1/p')
        FAILED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) failed.*/\1/p')
        SKIPPED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) skipped.*/\1/p')

        # If parsing failed, try alternative format
        if [ -z "$PASSED" ]; then
            # Try format: "Summary [   0.039s] 20 tests run: 20 passed (0 slow), 0 failed, 0 skipped"
            PASSED=$(echo "$SUMMARY_LINE" | sed -n 's/.*: \([0-9]*\) passed.*/\1/p')
            FAILED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) failed.*/\1/p')
            SKIPPED=$(echo "$SUMMARY_LINE" | sed -n 's/.* \([0-9]*\) skipped.*/\1/p')
        fi

        # Calculate total and percentage
        TOTAL=$((PASSED + FAILED + SKIPPED))

        if [ "$TOTAL" -gt 0 ]; then
            PASSING_PERCENTAGE=$((PASSED * 100 / TOTAL))
        else
            PASSING_PERCENTAGE=0
        fi

        STATS_AVAILABLE=true
    else
        # Fallback: try to parse from individual test results
        PASSED_COUNT=$(echo "$TEST_OUTPUT" | grep -c "PASS\|✓")
        FAILED_COUNT=$(echo "$TEST_OUTPUT" | grep -c "FAIL\|✗")
        SKIPPED_COUNT=$(echo "$TEST_OUTPUT" | grep -c "SKIP")

        PASSED=${PASSED_COUNT:-0}
        FAILED=${FAILED_COUNT:-0}
        SKIPPED=${SKIPPED_COUNT:-0}
        TOTAL=$((PASSED + FAILED + SKIPPED))

        if [ "$TOTAL" -gt 0 ]; then
            PASSING_PERCENTAGE=$((PASSED * 100 / TOTAL))
        else
            PASSING_PERCENTAGE=0
        fi

        STATS_AVAILABLE=true
    fi
fi

# Get failed test names (if any)
FAILED_TESTS=""
if [ "$FAILED" -gt 0 ]; then
    # Extract failed test names from output
    FAILED_TESTS=$(echo "$TEST_OUTPUT" | grep -A 5 -B 1 "FAIL\|✗" | grep "^\s*[^-]*test.*" | sed 's/.*--- \(.*\) ---.*/\1/' | grep -v "^\s*$" | head -10)
fi

# Create structured JSON output
echo -e "${YELLOW}Generating JSON output...${NC}" >&2
BUILD_STATUS="$([ "$BUILD_EXIT" -eq 0 ] && echo ok || echo failed)"
FMT_STATUS="$([ "$FMT_EXIT" -eq 0 ] && echo ok || echo failed)"
CLIPPY_STATUS="$([ "$CLIPPY_EXIT" -eq 0 ] && echo ok || echo failed)"
if [ "$COMPILATION_FAILED" = true ] || [ "$BUILD_EXIT" -ne 0 ]; then
    # Create JSON for compilation errors
    cat > "$JSON_FILE" << EOF
{
  "status": "compilation_failed",
  "timestamp": "$TIMESTAMP",
  "command": "$0",
  "exit_code": $EXIT_CODE,
  "checks": { "build": "$BUILD_STATUS", "fmt": "$FMT_STATUS", "clippy": "$CLIPPY_STATUS", "lib_release": "$([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo ok || echo failed)" },
  "test_statistics": {
    "total": 0,
    "passed": 0,
    "failed": 0,
    "skipped": 0,
    "passing_percentage": 0
  }
}
EOF
else
    # Create JSON for successful test runs
    cat > "$JSON_FILE" << EOF
{
  "status": "completed",
  "timestamp": "$TIMESTAMP",
  "command": "$0",
  "exit_code": $EXIT_CODE,
  "checks": { "build": "$BUILD_STATUS", "fmt": "$FMT_STATUS", "clippy": "$CLIPPY_STATUS", "lib_release": "$([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo ok || echo failed)" },
  "test_statistics": {
    "total": $TOTAL,
    "passed": ${PASSED:-0},
    "failed": ${FAILED:-0},
    "skipped": ${SKIPPED:-0},
    "passing_percentage": $PASSING_PERCENTAGE
  }
}
EOF
fi

# Generate comprehensive report
echo -e "${YELLOW}Generating report: $OUTPUT_FILE${NC}" >&2

{
    echo "# AIKIT Test Results Report"
    echo "Generated: $TIMESTAMP"
    echo "Command: $0"
    echo "Output File: $OUTPUT_FILE"
    echo "JSON File: $JSON_FILE"
    echo ""

    echo "## Overall Status"
    if [ "$EXIT_CODE" -eq 0 ]; then
        echo "✅ **PASSED** - build, fmt, clippy, nextest, and lib-release all passed"
    else
        echo "❌ **FAILED** - One or more checks failed"
    fi
    echo ""

    echo "## Checks"
    echo "- **build (cargo build --workspace --all-targets --all-features):** $([ "$BUILD_EXIT" -eq 0 ] && echo '✅ PASSED' || echo '❌ FAILED')"
    echo "- **fmt (cargo fmt --check):** $([ "$FMT_EXIT" -eq 0 ] && echo '✅ PASSED' || echo '❌ FAILED')"
    echo "- **clippy (cargo clippy --workspace --all-targets --all-features -- -D warnings):** $([ "$CLIPPY_EXIT" -eq 0 ] && echo '✅ PASSED' || echo '❌ FAILED')"
    echo "- **tests (cargo nextest run):** $([ "$TEST_EXIT" -eq 0 ] && echo '✅ PASSED' || echo '❌ FAILED')"
    echo "- **lib release (cargo test --lib --release, CI multiplatform):** $([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo '✅ PASSED' || echo '❌ FAILED')"
    echo ""
    if [ "$BUILD_EXIT" -ne 0 ] && [ -n "$BUILD_OUTPUT" ]; then
        echo "### Build failure output"
        echo ""
        echo "\`\`\`"
        echo "$BUILD_OUTPUT"
        echo "\`\`\`"
        echo ""
    fi
    if [ "$FMT_EXIT" -ne 0 ] && [ -n "$FMT_OUTPUT" ]; then
        echo "### fmt failure output"
        echo ""
        echo "\`\`\`"
        echo "$FMT_OUTPUT"
        echo "\`\`\`"
        echo ""
    fi
    if [ "$CLIPPY_EXIT" -ne 0 ] && [ -n "$CLIPPY_OUTPUT" ]; then
        echo "### clippy failure output"
        echo ""
        echo "\`\`\`"
        echo "$CLIPPY_OUTPUT"
        echo "\`\`\`"
        echo ""
    fi
    if [ "$LIB_RELEASE_EXIT" -ne 0 ] && [ -n "$LIB_RELEASE_OUTPUT" ]; then
        echo "### lib release (CI multiplatform) failure output"
        echo ""
        echo "\`\`\`"
        echo "$LIB_RELEASE_OUTPUT"
        echo "\`\`\`"
        echo ""
    fi

    echo "## Test Statistics"
    if [ "$COMPILATION_FAILED" = true ]; then
        echo "- **Status:** Compilation failed - no tests executed"
        echo "- **Total Tests:** N/A"
        echo "- **Passed:** N/A"
        echo "- **Failed:** N/A"
        echo "- **Skipped:** N/A"
        echo "- **Passing Rate:** N/A"
    else
        echo "- **Total Tests:** $TOTAL"
        echo "- **Passed:** $PASSED"
        echo "- **Failed:** $FAILED"
        echo "- **Skipped:** $SKIPPED"
        echo "- **Passing Rate:** ${PASSING_PERCENTAGE}%"
    fi
    echo ""

    # Progress bar visualization
    if [ "$COMPILATION_FAILED" = true ]; then
        echo "## Progress Visualization"
        echo "\`\`\`"
        echo "[░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] COMPILATION FAILED"
        echo "\`\`\`"
        echo ""
    elif [ "$TOTAL" -gt 0 ]; then
        echo "## Progress Visualization"
        BAR_WIDTH=30
        FILLED=$((PASSED * BAR_WIDTH / TOTAL))
        EMPTY=$((BAR_WIDTH - FILLED))

        echo "\`\`\`"
        printf "["
        for ((i=0; i<FILLED; i++)); do printf "█"; done
        for ((i=0; i<EMPTY; i++)); do printf "░"; done
        printf "] %d%% (%d/%d)\n" "$PASSING_PERCENTAGE" "$PASSED" "$TOTAL"
        echo "\`\`\`"
        echo ""
    else
        echo "## Progress Visualization"
        echo "\`\`\`"
        echo "[░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] No tests found"
        echo "\`\`\`"
        echo ""
    fi

    # Failed tests section
    if [ -n "$FAILED_TESTS" ] && [ "$FAILED" -gt 0 ]; then
        echo "## Failed Tests"
        echo ""
        echo "The following tests failed:"
        echo ""
        echo "\`\`\`"
        echo "$FAILED_TESTS"
        echo "\`\`\`"
        echo ""
    fi

    # Test duration (if available in summary)
    DURATION_LINE=$(echo "$TEST_OUTPUT" | grep "Summary.*\[" | head -1)
    if [ -n "$DURATION_LINE" ]; then
        DURATION=$(echo "$DURATION_LINE" | sed -n 's/.*\[\s*\([0-9.]*\)s\].*/\1/p')
        if [ -n "$DURATION" ]; then
            echo "## Performance"
            echo "- **Test Duration:** ${DURATION}s"
            echo ""
        fi
    fi

    echo "## Files"
    echo "- **Raw Test Output:** \`$JSON_FILE\`"
    echo "- **Markdown Report:** \`$OUTPUT_FILE\`"
    echo ""

    echo "## Raw Test Output"
    echo "Complete test output is saved in: \`$JSON_FILE\`"
    echo ""
    echo "You can analyze it with standard Unix tools:"
    echo "\`\`\`bash"
    echo "# Count total tests"
    echo "grep -c 'PASS\\|FAIL\\|SKIP' $JSON_FILE"
    echo ""
    echo "# Show failed tests"
    echo "grep -A 2 -B 2 'FAIL' $JSON_FILE"
    echo "\`\`\`"

} > "$OUTPUT_FILE"

# Console output summary
echo -e "${GREEN}Report generated successfully!${NC}" >&2
echo "" >&2

echo "📊 Summary:" >&2
echo "  build:       $([ "$BUILD_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
echo "  fmt:         $([ "$FMT_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
echo "  clippy:      $([ "$CLIPPY_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
echo "  lib-release: $([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
if [ "$COMPILATION_FAILED" = true ]; then
    echo "  tests:  COMPILATION FAILED - no tests executed" >&2
    echo -e "${RED}❌ Check $OUTPUT_FILE for details.${NC}" >&2
else
    echo "  tests:  $TOTAL total, $PASSED passed, $FAILED failed (${PASSING_PERCENTAGE}%)" >&2
    if [ "$EXIT_CODE" -eq 0 ]; then
        echo -e "${GREEN}✅ All checks and tests passed.${NC}" >&2
    else
        echo -e "${RED}❌ One or more failed. Check $OUTPUT_FILE for details.${NC}" >&2
    fi
fi

echo "" >&2
echo "📁 Files created:" >&2
echo "  Markdown report: $OUTPUT_FILE" >&2
echo "  Raw output: $JSON_FILE" >&2

exit $EXIT_CODE