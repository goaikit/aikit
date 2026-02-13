# Testing Guide for AIKIT

This document describes the testing strategy, configuration, and best practices for AIKIT.

## Test Structure

AIKIT uses multiple levels of testing:

- **Unit tests**: Located within source files using `#[cfg(test)]` modules
- **Integration tests**: Located in `tests/` directory
- **CLI integration tests**: Test the actual binary behavior using `assert_cmd`

## Test Runner

We use **cargo-nextest** as the test runner for better performance and timeout handling.

### Installation

```bash
cargo install cargo-nextest
```

### Running Tests

```bash
# Run all tests (excluding ignored tests)
./scripts/run-tests.sh

# Run all tests with cargo-nextest directly
cargo nextest run --all-features

# Run specific test
cargo nextest run test_package_init_basic

# Run ignored tests (requires API credentials)
cargo nextest run --ignored

# Run with verbose output
cargo nextest run -v
```

## Timeout Configuration

Tests have global timeouts configured in `.config/nextest.toml` to prevent hanging:

- **Default timeout**: 60 seconds per test
- **CI timeout**: 30 seconds per test (stricter)
- **Slow test timeout**: 90 seconds for network-dependent tests

### Per-Test Timeout Overrides

Specific tests can override the default timeout in `.config/nextest.toml`:

```toml
[[profile.default.overrides]]
filter = "test(test_name)"
slow-timeout = { period = "120s", terminate-after = 2 }
```

## Test Categories

### 1. Fast Unit Tests

These tests run quickly and don't require external resources:

```bash
cargo test --lib
```

### 2. Integration Tests

Tests that verify CLI behavior, command parsing, and file operations:

```bash
cargo nextest run --all-features
```

### 3. Ignored Tests (Require External Resources)

Tests that require:
- API credentials (`ANTHROPIC_API_KEY`, `GITHUB_TOKEN`)
- Network access
- External services

Run with:
```bash
cargo nextest run --ignored
```

## Dry-Run Mode for Testing

The `aikit run` command supports a hidden `--dry-run` flag for testing without API calls:

```bash
# Validate command configuration without executing
echo "test prompt" | aikit run --agent opencode --dry-run
```

This allows integration tests to verify the CLI behavior without requiring API credentials.

## Writing Tests

### Best Practices

1. **Always add timeouts for tests that make external calls**
   ```rust
   #[test]
   #[ignore] // Mark as ignored if requires credentials
   fn test_api_call() {
       // Use timeout logic or rely on nextest timeout
   }
   ```

2. **Use `#[ignore]` for tests requiring external resources**
   ```rust
   #[test]
   #[ignore] // Requires API credentials
   fn test_with_api() {
       if std::env::var("API_KEY").is_err() {
           eprintln!("Skipping: API_KEY not set");
           return;
       }
       // test code
   }
   ```

3. **Prefer dry-run mode for CLI tests**
   ```rust
   #[test]
   fn test_run_command() {
       cargo_bin_cmd!("aikit")
           .args(["run", "--agent", "opencode", "--dry-run"])
           .assert()
           .success();
   }
   ```

4. **Use temporary directories for file operations**
   ```rust
   use tempfile::tempdir;

   #[test]
   fn test_file_operation() -> Result<(), Box<dyn std::error::Error>> {
       let temp = tempdir()?;
       let work = temp.path();
       // test code using work directory
       Ok(())
   }
   ```

## Continuous Integration

The CI pipeline runs:

1. **Format check**: `cargo fmt --check`
2. **Linting**: `cargo clippy -- -D warnings`
3. **Build**: `cargo build --workspace --all-targets --all-features`
4. **Tests**: `cargo nextest run --all-features --fail-fast`
5. **Release tests**: `cargo test --lib --release`

### CI Timeout Settings

CI uses stricter timeouts defined in `.config/nextest.toml`:

```toml
[profile.ci]
slow-timeout = { period = "30s", terminate-after = 2 }
fail-fast = true
```

Run CI profile locally:
```bash
cargo nextest run --profile ci
```

## Troubleshooting

### Tests Hanging

If tests hang:

1. Check `.config/nextest.toml` timeout settings
2. Verify the test doesn't make unbounded external calls
3. Add `#[ignore]` if the test requires external resources
4. Use `--dry-run` flag for CLI tests that call external APIs

### Test Failures in CI

1. Run with CI profile locally: `cargo nextest run --profile ci`
2. Check for race conditions (use `--test-threads=1`)
3. Verify test cleanup (temp directories, environment variables)

### Timeout Debugging

To see which tests are slow:

```bash
cargo nextest run --all-features -v
```

The output shows test duration and identifies slow tests.

## Test Output Files

Test results are saved to:

- **Markdown report**: `.github/test-outputs/test_results.md` (or custom with `-o`)
- **JSON results**: `.github/test-outputs/test_results.json` (or custom with `-j`)
- **Raw outputs**: `.github/test-outputs/{build,fmt,clippy,test}-output.txt`

These files are gitignored and used for debugging test failures.

## Examples

### Run specific test with output
```bash
cargo nextest run test_package_init_basic -v
```

### Run all tests except ignored
```bash
./scripts/run-tests.sh
```

### Run only ignored tests (requires credentials)
```bash
cargo nextest run --ignored
```

### Run with custom retry count
```bash
./scripts/run-tests.sh --retries 5
```

### Generate test report
```bash
./scripts/run-tests.sh -o report.md -j report.json
```

## Resources

- [cargo-nextest documentation](https://nexte.st/)
- [assert_cmd documentation](https://docs.rs/assert_cmd/)
- [tempfile documentation](https://docs.rs/tempfile/)
