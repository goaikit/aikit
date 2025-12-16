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

## Release Automation Setup

This project uses automated CI/CD pipelines for releases, including cross-platform binary builds and automatic package manager updates. The release process is powered by reusable GitHub Actions and a GitHub App for secure cross-repository operations.

### GitHub App Configuration

The project uses a GitHub App (`aikit-release-automation`) for automated package manager updates during releases. This provides secure, scoped access to update Homebrew and Scoop repositories without using personal access tokens.

#### App Creation Steps

1. **Create GitHub App** in Organization Settings → Developer settings → GitHub Apps:
   - **Name**: `aikit-release-automation`
   - **Description**: `Automates package manager updates for aikit releases`
   - **Homepage URL**: `https://github.com/goaikit/aikit`
   - **Webhook URL**: Leave blank (not needed for API-only operations)
   - **Webhook secret**: Leave blank

2. **Repository Permissions**:
   - ✅ **Contents**: Read and write
   - ✅ **Pull requests**: Read and write
   - ✅ **Metadata**: Read-only

3. **Install the App**:
   - Install on the `goaikit` organization
   - Grant access to: `aikit`, `homebrew-cli`, `scoop-bucket`

4. **Generate Private Key**:
   - Download the private key (PEM file)
   - This will be used to generate JWT tokens

#### Repository Secrets Configuration

Add these secrets to the `goaikit/aikit` repository (Settings → Secrets and variables → Actions):

- `GH_APP_ID`: The GitHub App ID (numeric value from app settings)
- `GH_APP_PRIVATE_KEY`: The complete PEM content of the downloaded private key

#### Workflow Integration

The release workflow uses `actions/create-github-app-token@v1` to generate short-lived tokens:

```yaml
- name: Generate GitHub App Token
  id: app-token
  uses: actions/create-github-app-token@v1
  with:
    app-id: ${{ secrets.GH_APP_ID }}
    private-key: ${{ secrets.GH_APP_PRIVATE_KEY }}
    owner: goaikit

# Use the token for cross-repository operations
env:
  GH_TOKEN: ${{ steps.app-token.outputs.token }}
```

### Release Process Architecture

The CI/CD pipeline consists of reusable components:

#### Directory Structure
```
.github/
├── release-config.yml          # Project-specific configuration
├── workflows/
│   ├── auto-release.yml        # Automatic versioning and tagging
│   ├── release-build.yml       # Cross-platform binary building
│   ├── release-publish.yml     # GitHub release creation
│   └── release-package-managers.yml  # Homebrew/Scoop updates
└── actions/                     # Reusable composite actions
    ├── detect-version/         # Version detection logic
    ├── build-rust-binary/      # Cross-platform Rust builds
    ├── update-homebrew/        # Homebrew formula updates
    └── update-scoop/           # Scoop bucket updates
```

#### Configuration File

The `release-config.yml` contains all project-specific values:

```yaml
project:
  name: aikit
  binary_name: aikit
  description: "AIKIT - Rust Spec Kit CLI"
  homepage: "https://github.com/goaikit/aikit"
  license: "Apache-2.0"

repositories:
  main: goaikit/aikit
  homebrew: goaikit/homebrew-cli
  scoop: goaikit/scoop-bucket

build:
  targets:
    - target: x86_64-unknown-linux-gnu
      binary_name: aikit
      archive_format: tar.gz
    - target: x86_64-unknown-linux-musl
      binary_name: aikit
      archive_format: tar.gz
    - target: x86_64-pc-windows-msvc
      binary_name: aikit.exe
      archive_format: zip
```

#### Reusability for New Projects

To reuse this setup for other Rust CLI projects:

1. **Copy the `.github/` folder** (contains all workflows and actions)
2. **Update `release-config.yml`** with project-specific values
3. **Set up GitHub App** with appropriate repository permissions
4. **Configure repository secrets** (`GH_APP_ID`, `GH_APP_PRIVATE_KEY`)

### Security Considerations

- **Short-lived tokens**: GitHub App tokens expire after 1 hour
- **Scoped permissions**: App only has access to necessary repositories and operations
- **Audit trail**: All app operations are logged in organization audit logs
- **No personal credentials**: Not dependent on individual developer accounts
- **Automatic rotation**: Tokens are generated fresh for each workflow run

### Troubleshooting

**Common Issues:**

1. **"Permission denied to github-actions[bot]"**
   - Solution: Ensure GitHub App is installed and has repository access

2. **"Can't find action.yml" errors**
   - Solution: Ensure composite actions are committed and workflows checkout code first

3. **Package manager updates failing**
   - Solution: Check GitHub App permissions and repository access

**Debugging Steps:**

1. Check workflow run logs for detailed error messages
2. Verify GitHub App installation and permissions
3. Test repository secrets are correctly configured
4. Ensure target repositories exist and are accessible

## License

By contributing, you agree that your contributions will be licensed under the same license as the original project (Apache License, Version 2.0).

## Questions?

Feel free to contact the maintainers or open an issue for any questions about contributing.

