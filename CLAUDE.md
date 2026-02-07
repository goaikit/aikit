# Contributing to AIKIT

We love your input! We want to make contributing to AIKIT as easy and transparent as possible, whether it's:

- Reporting a bug
- Discussing the current state of the code
- Submitting a fix
- Proposing new features
- Becoming a maintainer

## Development Process

We use GitHub to host code, to track issues and feature requests, as well as accept pull requests.

1. Fork the repo and create your branch from `main`
2. If you've added code that should be tested, add tests
3. If you've changed APIs, update the documentation
4. Ensure the test suite passes
5. Make sure your code lints and is formatted
6. Issue that pull request!

## Project Structure

This project follows a specific structure to maintain consistency:

```
./goaikit/aikit/
├── src/                      # Source code
│   ├── cli/                  # CLI command implementations
│   │   ├── check.rs         # Tool checking command
│   │   ├── init.rs          # Project initialization command
│   │   ├── package.rs       # Package generation command
│   │   ├── release.rs       # Release creation command
│   │   ├── version.rs       # Version display command
│   │   └── mod.rs           # CLI module root
│   ├── core/                 # Core business logic
│   │   ├── agent.rs         # Agent configuration and validation
│   │   ├── git.rs           # Git repository operations
│   │   ├── package.rs       # Package generation logic
│   │   ├── template.rs      # Template download and extraction
│   │   ├── tools.rs         # Tool detection and checking
│   │   └── mod.rs           # Core module root
│   ├── fs/                   # File system operations
│   │   ├── merge.rs         # File merging logic (JSON deep merge)
│   │   ├── permissions.rs   # File permission handling
│   │   └── mod.rs           # FS utilities with cross-platform support
│   ├── github/               # GitHub API client
│   │   ├── api.rs           # GitHub API requests/responses
│   │   ├── rate_limit.rs    # Rate limit detection and formatting
│   │   └── mod.rs           # GitHub module root
│   ├── tui/                  # Terminal UI components
│   │   ├── agent_select.rs  # Interactive agent selection
│   │   ├── output.rs        # Formatted output utilities
│   │   └── mod.rs           # TUI module root
│   ├── config/               # Configuration management
│   │   └── agent_config.rs  # Agent configuration parsing
│   └── main.rs              # Application entry point
├── specs/                    # Specification documents
│   └── 002-rust-spec-kit-complete/  # Complete specification
│       ├── spec.md          # Feature specification
│       ├── plan.md          # Implementation plan
│       ├── tasks.md         # Task breakdown
│       └── contracts/       # API contracts
├── tests/                    # Integration tests (if any)
├── repomix-output.xml        # Complete source code summary
├── Cargo.toml                # Rust project manifest
├── rustfmt.toml              # Rust formatting configuration
├── .clippy.toml              # Clippy linter configuration
├── .gitignore                # Git ignore patterns
└── README.md                 # This file
```

## Development Environment Setup

### Prerequisites

- **Rust**: 1.75 or higher (stable toolchain)
- **Git**: Latest version
- **Cargo**: Comes with Rust toolchain
- **GitHub CLI** (optional): For testing release functionality

### Local Development Setup

1. **Clone the repository**

   ```bash
   git clone https://github.com/goaikit/aikit.git
   cd aikit
   ```

2. **Install Rust** (if not already installed)

   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **Build the project**

   ```bash
   cargo build
   ```

4. **Run tests**

   ```bash
   cargo test
   ```

5. **Install development dependencies**

   The project uses standard Rust tooling. All dependencies are specified in `Cargo.toml`.

### Understanding the Codebase

Before making changes, understand the project structure by:

1. **Read the repomix-output.xml file** - This file contains a complete summary of all source code files, their contents, and relationships. It's automatically generated and provides a comprehensive view of the entire codebase.

2. **Review the specification documents** in `specs/002-rust-spec-kit-complete/`:
   - `spec.md`: Complete feature specification
   - `plan.md`: Implementation plan and architecture
   - `tasks.md`: Detailed task breakdown
   - `contracts/`: API contracts for CLI and GitHub integration

3. **Check existing tests** in the source files (using `#[cfg(test)]` modules) to understand expected behavior.

4. **Look at open issues** to see what features or fixes are needed.

## Code Standards

### Style Guidelines

- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `rustfmt` for code formatting (configuration in `rustfmt.toml`)
- Run `cargo fmt` before committing
- Write doc comments (`///`) for all public functions and types
- Use meaningful variable and function names
- Keep functions focused and single-purpose

### Documentation Standards

- Update doc comments when changing function signatures
- Update README.md when adding new features
- Keep comments concise and meaningful
- Use proper markdown formatting in documentation
- Include examples in doc comments where helpful

### Testing Requirements

- Write tests for new functionality in `#[cfg(test)]` modules
- Ensure all tests pass before submitting PR: `cargo test`
- Aim for high test coverage where practical
- Use descriptive test function names (`test_<functionality>`)
- Test both success and error cases

### Linting

- Run `cargo clippy` before committing
- Fix all clippy warnings (some are allowed via `.clippy.toml`)
- The project uses `-D warnings` in CI, so code must be warning-free

## Git Hooks

AIKit provides optional git hooks to automatically enforce code quality standards before commits and pushes. These hooks mirror the CI checks and help catch issues early in development.

### Available Hooks

- **`pre-commit`**: Runs fast checks before each commit
  - Code formatting validation (`cargo fmt --check`)
  - Linting with Clippy (`cargo clippy -- -D warnings`)
  - Quick unit tests (`cargo test --lib`)

- **`pre-push`**: Runs comprehensive checks before pushing
  - Full test suite including integration tests (`cargo test`)
  - Documentation build validation (`cargo doc`)

- **`commit-msg`**: Validates commit message format (optional)
  - Checks for conventional commit format
  - Warns but allows override for flexibility

### Installation

To install the git hooks locally:

```bash
# From the project root (aikit directory)
./scripts/install-git-hooks.sh
```

This will copy the hooks from `.githooks/` to `.git/hooks/` and make them executable.

### Benefits

- **Early feedback**: Catch formatting and linting issues before pushing
- **Consistent quality**: Ensures all contributors follow the same standards
- **CI alignment**: Local checks match GitHub Actions pipeline
- **Optional**: Hooks can be disabled if needed for specific workflows

### Disabling Hooks

If you need to bypass hooks temporarily:

```bash
# Skip all hooks for a commit
git commit --no-verify

# Skip hooks for a push
git push --no-verify

# Disable specific hooks
chmod -x .git/hooks/pre-commit
chmod -x .git/hooks/pre-push
chmod -x .git/hooks/commit-msg
```

## Pull Request Process

1. **Create a feature branch**

   ```bash
   git checkout -b feature/amazing-feature
   ```

2. **Make your changes** following the code standards above

3. **Run the test suite**

   ```bash
   cargo test
   ```

4. **Check code style and formatting**

   ```bash
   cargo fmt --check
   cargo clippy -- -D warnings
   ```

5. **Format your code**

   ```bash
   cargo fmt
   ```

6. **Update documentation** if needed

7. **Commit your changes**

   ```bash
   git add .
   git commit -m "Add some amazing feature"
   ```

   Follow [Conventional Commits](https://www.conventionalcommits.org/) format:
   - `feat:` for new features
   - `fix:` for bug fixes
   - `docs:` for documentation changes
   - `refactor:` for code refactoring
   - `test:` for test additions/changes

8. **Push to your fork**

   ```bash
   git push origin feature/amazing-feature
   ```

9. **Open a Pull Request** on GitHub

### Pull Request Checklist

- [ ] Tests added/updated for new functionality
- [ ] All tests pass (`cargo test`)
- [ ] Code follows project style guidelines
- [ ] Code is formatted (`cargo fmt`)
- [ ] No clippy warnings (`cargo clippy`)
- [ ] Documentation updated
- [ ] Branch is up to date with main
- [ ] PR title follows conventional commit format

## Code Quality Checks

Before submitting a PR, ensure all checks pass:

```bash
# Format check
cargo fmt --check

# Build check
cargo build --release

# Lint check
cargo clippy -- -A clippy::too_many_arguments -A clippy::module_name_repetitions -A dead_code -D warnings

# Test check
cargo test
```

## Issue Reporting

Report bugs using GitHub's [issue tracker](https://github.com/aroff/aikit/issues).

**Great Bug Reports** tend to have:

- A quick summary and/or background
- Steps to reproduce
  - Be specific!
  - Include command-line examples if possible
- What you expected would happen
- What actually happens
- Environment details (OS, Rust version, etc.)
- Notes (possibly including why you think this might be happening, or stuff you tried that didn't work)

## Architecture Overview

AIKIT is a Rust CLI application with the following key components:

- **CLI Layer** (`src/cli/`): Command implementations using `clap`
- **Core Logic** (`src/core/`): Business logic for agents, templates, Git, packages
- **File System** (`src/fs/`): Cross-platform file operations and merging
- **GitHub Integration** (`src/github/`): API client with rate limit handling
- **TUI Components** (`src/tui/`): Interactive terminal UI using `ratatui`

For detailed architecture documentation, see [architecture.md](architecture.md).

## Project-Specific Skills

This project follows Spec-Driven Development (SDD) methodology. When working on this project:

- Review the specification documents in `specs/002-rust-spec-kit-complete/`
- Ensure implementations match the Python reference behavior exactly
- Test against the acceptance criteria in `spec.md`
- Follow the task breakdown in `tasks.md` for implementation order

## Community Guidelines

- Be respectful and inclusive
- Use welcoming and inclusive language
- Be collaborative
- Focus on what is best for the community
- Show empathy towards other community members

## Recognition

Contributors will be recognized in the project README and changelog. Thank you for your contributions!

## CI/CD Pipeline

This project uses GitHub Actions for continuous integration and automated releases. The CI pipeline ensures code quality and automatically creates releases with pre-built binaries.

### CI Workflow

The CI pipeline (`.github/workflows/ci.yml`) runs on every push and pull request:

- **Test Job**: Runs `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`
- **Security Job**: Runs `cargo audit` for dependency vulnerability scanning
- **Release Job**: Automatically creates GitHub releases with Linux binaries when code is pushed to main

### Release Process

When code is merged to the `main` branch:

1. Tests and security checks run automatically
2. If all checks pass, a release is created with:
   - Linux GNU binary (`aikit-x86_64-unknown-linux-gnu.tar.gz`)
   - Linux MUSL binary (`aikit-x86_64-unknown-linux-musl.tar.gz`)
3. The release is published to GitHub Releases for download

### Local Development Setup

Before pushing code, ensure it passes all CI checks:

```bash
# Run tests
cargo test

# Check code style and formatting
cargo fmt --check
cargo clippy -- -D warnings

# Run security audit
cargo audit  # (if cargo-audit is installed)
```

## Agent Guidelines

Contributors and automated agents working on this project MUST adhere to the following quality standards:

- Code solutions MUST prioritize quality and robustness over workarounds or quick fixes
- Implementations MUST follow established patterns and best practices rather than creating temporary solutions
- Code MUST be maintainable, readable, and well-documented
- Proper error handling and validation MUST be implemented
- Tests MUST be comprehensive and cover edge cases
- Quick fixes that compromise code quality FORBIDDEN unless explicitly approved with documentation explaining the trade-off

## License

By contributing, you agree that your contributions will be licensed under the same license as the original project (Apache License, Version 2.0).

## Questions?

Feel free to contact the maintainers or open an issue for any questions about contributing.

Ensure test outputs and temporary files are in github