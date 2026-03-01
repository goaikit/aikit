#!/bin/bash

# AIKIT Test Runner Script
# Runs build, fmt, clippy, and tests (cargo-nextest); captures results and emits report to stdout (text or JSON).
# Matches CI: full workspace build, fmt, clippy, then tests with retries.
#
# Usage: ./run-tests.sh [OPTIONS]
#
# Options:
#   -f, --format FORMAT  Output format: text (default) or json. Report goes to stdout.
#   -o, --output FILE    Optional: write markdown report to FILE.
#   -j, --json FILE      Optional: write JSON results to FILE.
#   -d, --output-dir DIR Directory for raw fmt/clippy/test .txt outputs (default: .github/test-outputs)
#   -r, --retries N      Number of retries for flaky tests (default: 3)
#   -h, --help           Show this help message.
#
# Default: text report to stdout only. Use -o/-j to write to files.

set -e  # Exit on any error

TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
OUTPUT_FORMAT="text"
OUTPUT_FILE=""
JSON_FILE=""
RETRIES=3

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
    echo "  -f, --format FORMAT   Output format: text (default) or json. Report to stdout." >&2
    echo "  -o, --output FILE     Optional: write markdown report to FILE." >&2
    echo "  -j, --json FILE       Optional: write JSON results to FILE." >&2
    echo "  -d, --output-dir DIR  Directory for raw fmt/clippy/test .txt outputs (default: .github/test-outputs)" >&2
    echo "  -r, --retries N       Number of retries for flaky tests (default: 3)" >&2
    echo "  -h, --help            Show this help message." >&2
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
        -f|--format)
            if [[ "$2" != "text" && "$2" != "json" ]]; then
                error_exit "Format must be 'text' or 'json', got: $2"
            fi
            OUTPUT_FORMAT="$2"
            shift 2
            ;;
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
            echo "Runs build, fmt, clippy, and tests (cargo-nextest); captures results and emits report to stdout (text or JSON)."
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -f, --format FORMAT   Output format: text (default) or json. Report goes to stdout."
            echo "  -o, --output FILE     Optional: write markdown report to FILE."
            echo "  -j, --json FILE       Optional: write JSON results to FILE."
            echo "  -d, --output-dir DIR  Directory for raw fmt/clippy/test .txt outputs (default: .github/test-outputs)"
            echo "  -r, --retries N       Number of retries for flaky tests (default: 3)"
            echo "  -h, --help            Show this help message."
            echo ""
            echo "Default: text report to stdout only. -o and -j are optional and only write when a path is given."
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
if [ "${FAILED:-0}" -gt 0 ]; then
    # Extract failed test names from output
    FAILED_TESTS=$(echo "$TEST_OUTPUT" | grep -A 5 -B 1 "FAIL\|✗" | grep "^\s*[^-]*test.*" | sed 's/.*--- \(.*\) ---.*/\1/' | grep -v "^\s*$" | head -10)
fi

# Build JSON content (for stdout and/or file)
BUILD_STATUS="$([ "$BUILD_EXIT" -eq 0 ] && echo ok || echo failed)"
FMT_STATUS="$([ "$FMT_EXIT" -eq 0 ] && echo ok || echo failed)"
CLIPPY_STATUS="$([ "$CLIPPY_EXIT" -eq 0 ] && echo ok || echo failed)"
LIB_RELEASE_STATUS="$([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo ok || echo failed)"
if [ "$COMPILATION_FAILED" = true ] || [ "$BUILD_EXIT" -ne 0 ]; then
    JSON_CONTENT=$(cat << EOF
{
  "status": "compilation_failed",
  "timestamp": "$TIMESTAMP",
  "command": "$0",
  "exit_code": $EXIT_CODE,
  "checks": { "build": "$BUILD_STATUS", "fmt": "$FMT_STATUS", "clippy": "$CLIPPY_STATUS", "lib_release": "$LIB_RELEASE_STATUS" },
  "test_statistics": {
    "total": 0,
    "passed": 0,
    "failed": 0,
    "skipped": 0,
    "passing_percentage": 0
  }
}
EOF
)
else
    JSON_CONTENT=$(cat << EOF
{
  "status": "completed",
  "timestamp": "$TIMESTAMP",
  "command": "$0",
  "exit_code": $EXIT_CODE,
  "checks": { "build": "$BUILD_STATUS", "fmt": "$FMT_STATUS", "clippy": "$CLIPPY_STATUS", "lib_release": "$LIB_RELEASE_STATUS" },
  "test_statistics": {
    "total": $TOTAL,
    "passed": ${PASSED:-0},
    "failed": ${FAILED:-0},
    "skipped": ${SKIPPED:-0},
    "passing_percentage": $PASSING_PERCENTAGE
  }
}
EOF
)
fi

# Progress message
if [ -n "$OUTPUT_FILE" ] || [ -n "$JSON_FILE" ]; then
    echo -e "${YELLOW}Generating report...${NC}" >&2
    [ -n "$OUTPUT_FILE" ] && echo -e "${YELLOW}  Markdown: $OUTPUT_FILE${NC}" >&2
    [ -n "$JSON_FILE" ] && echo -e "${YELLOW}  JSON: $JSON_FILE${NC}" >&2
else
    echo -e "${YELLOW}Generating report (stdout, format: $OUTPUT_FORMAT)...${NC}" >&2
fi

# Emit report: stdout and/or files
if [ "$OUTPUT_FORMAT" = "json" ]; then
    echo "$JSON_CONTENT"
    if [ -n "$JSON_FILE" ]; then
        mkdir -p "$(dirname "$JSON_FILE")"
        echo "$JSON_CONTENT" > "$JSON_FILE"
    fi
else
    print_text_report() {
        echo "# AIKIT Test Results Report"
        echo "Generated: $TIMESTAMP"
        echo "Command: $0"
        echo ""

        echo "## Overall Status"
        if [ "$EXIT_CODE" -eq 0 ]; then
            echo "PASSED - build, fmt, clippy, nextest, and lib-release all passed"
        else
            echo "FAILED - One or more checks failed"
        fi
        echo ""

        echo "## Checks"
        echo "- build: $([ "$BUILD_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')"
        echo "- fmt: $([ "$FMT_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')"
        echo "- clippy: $([ "$CLIPPY_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')"
        echo "- tests (nextest): $([ "$TEST_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')"
        echo "- lib release: $([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')"
        echo ""
        if [ "$BUILD_EXIT" -ne 0 ] && [ -n "$BUILD_OUTPUT" ]; then
            echo "### Build failure output"
            echo ""
            echo "$BUILD_OUTPUT"
            echo ""
        fi
        if [ "$FMT_EXIT" -ne 0 ] && [ -n "$FMT_OUTPUT" ]; then
            echo "### fmt failure output"
            echo ""
            echo "$FMT_OUTPUT"
            echo ""
        fi
        if [ "$CLIPPY_EXIT" -ne 0 ] && [ -n "$CLIPPY_OUTPUT" ]; then
            echo "### clippy failure output"
            echo ""
            echo "$CLIPPY_OUTPUT"
            echo ""
        fi
        if [ "$LIB_RELEASE_EXIT" -ne 0 ] && [ -n "$LIB_RELEASE_OUTPUT" ]; then
            echo "### lib release failure output"
            echo ""
            echo "$LIB_RELEASE_OUTPUT"
            echo ""
        fi

        echo "## Test Statistics"
        if [ "$COMPILATION_FAILED" = true ]; then
            echo "- Status: Compilation failed - no tests executed"
            echo "- Total Tests: N/A"
            echo "- Passed: N/A"
            echo "- Failed: N/A"
            echo "- Skipped: N/A"
            echo "- Passing Rate: N/A"
        else
            echo "- Total Tests: $TOTAL"
            echo "- Passed: ${PASSED:-0}"
            echo "- Failed: ${FAILED:-0}"
            echo "- Skipped: ${SKIPPED:-0}"
            echo "- Passing Rate: ${PASSING_PERCENTAGE}%"
        fi
        echo ""

        if [ "$COMPILATION_FAILED" = true ]; then
            echo "## Progress Visualization"
            echo "[..............................] COMPILATION FAILED"
            echo ""
        elif [ "$TOTAL" -gt 0 ]; then
            echo "## Progress Visualization"
            BAR_WIDTH=30
            FILLED=$((PASSED * BAR_WIDTH / TOTAL))
            EMPTY=$((BAR_WIDTH - FILLED))
            printf "["
            for ((i=0; i<FILLED; i++)); do printf "#"; done
            for ((i=0; i<EMPTY; i++)); do printf "."; done
            printf "] %d%% (%d/%d)\n" "$PASSING_PERCENTAGE" "${PASSED:-0}" "$TOTAL"
            echo ""
        else
            echo "## Progress Visualization"
            echo "[..............................] No tests found"
            echo ""
        fi

        if [ -n "$FAILED_TESTS" ] && [ "${FAILED:-0}" -gt 0 ]; then
            echo "## Failed Tests"
            echo ""
            echo "$FAILED_TESTS"
            echo ""
        fi

        DURATION_LINE=$(echo "$TEST_OUTPUT" | grep "Summary.*\[" | head -1)
        if [ -n "$DURATION_LINE" ]; then
            DURATION=$(echo "$DURATION_LINE" | sed -n 's/.*\[\s*\([0-9.]*\)s\].*/\1/p')
            if [ -n "$DURATION" ]; then
                echo "## Performance"
                echo "- Test Duration: ${DURATION}s"
                echo ""
            fi
        fi
    }

    print_text_report
    if [ -n "$OUTPUT_FILE" ]; then
        mkdir -p "$(dirname "$OUTPUT_FILE")"
        {
            print_text_report
            echo "## Files"
            echo "- Markdown Report: $OUTPUT_FILE"
            [ -n "$JSON_FILE" ] && echo "- JSON results: $JSON_FILE"
            echo ""
        } > "$OUTPUT_FILE"
    fi
    if [ -n "$JSON_FILE" ]; then
        mkdir -p "$(dirname "$JSON_FILE")"
        echo "$JSON_CONTENT" > "$JSON_FILE"
    fi
fi

# Console summary (stderr)
if [ -n "$OUTPUT_FILE" ] || [ -n "$JSON_FILE" ]; then
    echo -e "${GREEN}Report generated successfully!${NC}" >&2
else
    echo -e "${GREEN}Report written to stdout.${NC}" >&2
fi
echo "" >&2

echo "Summary:" >&2
echo "  build:       $([ "$BUILD_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
echo "  fmt:         $([ "$FMT_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
echo "  clippy:      $([ "$CLIPPY_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
echo "  lib-release: $([ "$LIB_RELEASE_EXIT" -eq 0 ] && echo 'PASSED' || echo 'FAILED')" >&2
if [ "$COMPILATION_FAILED" = true ]; then
    echo "  tests:  COMPILATION FAILED - no tests executed" >&2
    if [ -n "$OUTPUT_FILE" ]; then
        echo -e "${RED}Check $OUTPUT_FILE for details.${NC}" >&2
    else
        echo -e "${RED}See report above.${NC}" >&2
    fi
else
    echo "  tests:  $TOTAL total, ${PASSED:-0} passed, ${FAILED:-0} failed (${PASSING_PERCENTAGE}%)" >&2
    if [ "$EXIT_CODE" -eq 0 ]; then
        echo -e "${GREEN}All checks and tests passed.${NC}" >&2
    else
        if [ -n "$OUTPUT_FILE" ]; then
            echo -e "${RED}One or more failed. Check $OUTPUT_FILE for details.${NC}" >&2
        else
            echo -e "${RED}One or more failed. See report above.${NC}" >&2
        fi
    fi
fi

echo "" >&2
if [ -n "$OUTPUT_FILE" ] || [ -n "$JSON_FILE" ]; then
    echo "Files created:" >&2
    [ -n "$OUTPUT_FILE" ] && echo "  Markdown report: $OUTPUT_FILE" >&2
    [ -n "$JSON_FILE" ] && echo "  JSON: $JSON_FILE" >&2
fi

exit $EXIT_CODE