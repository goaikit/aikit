//! Package installation commands
//!
//! This module contains CLI commands for package installation management:
//! - install: Install package from URL
//! - update: Update installed package
//! - remove: Remove installed package
//! - list: List installed packages

use crate::error::AikError;
use crate::github::api::GitHubClient;
use atty;
use cli_framework::command::{FromArgValueMap, IntoCommandSpec};
use cli_framework::spec::arg_spec::{ArgKind, ArgSpec, ArgValueType, Cardinality};
use cli_framework::spec::command_tree::CommandSpec;
use cli_framework::spec::value::ArgValue;
use std::collections::HashMap;
use std::path::PathBuf;
use toml;

/// Source type for package installation
#[derive(Debug, Clone)]
pub enum SourceType {
    LocalFolder(PathBuf),
    GitHubRepo {
        owner: String,
        repo: String,
        version: String,
    },
}

/// Arguments for install command
#[derive(Debug)]
pub struct InstallArgs {
    pub source: String,
    pub install_version: Option<String>,
    pub token: Option<String>,
    pub force: bool,
    #[allow(dead_code)]
    pub yes: bool,
    pub ai: Option<String>,
}

impl InstallArgs {
    pub fn detect_source_type(&self) -> Result<SourceType, AikError> {
        let path = std::path::Path::new(&self.source);

        // Check if it's an existing local directory
        if path.exists() && path.is_dir() {
            // Validate it contains aikit.toml
            let aikit_toml = path.join("aikit.toml");
            if !aikit_toml.exists() {
                return Err(AikError::InvalidSource(format!(
                    "Directory '{}' does not contain aikit.toml",
                    self.source
                )));
            }
            return Ok(SourceType::LocalFolder(path.to_path_buf()));
        }

        // Check if it's a GitHub URL or owner/repo format
        if self.looks_like_github_source() {
            let (owner, repo, version) =
                parse_github_url(&self.source, self.install_version.as_deref())?;
            return Ok(SourceType::GitHubRepo {
                owner,
                repo,
                version,
            });
        }

        // Provide helpful error
        Err(AikError::InvalidSource(format!(
            "Invalid source '{}'. Expected:\n  - Local directory path (must exist and contain aikit.toml)\n  - GitHub URL: github.com/owner/repo or https://github.com/owner/repo\n  - Short format: owner/repo",
            self.source
        )))
    }

    fn looks_like_github_source(&self) -> bool {
        let source = &self.source;

        // Exclude relative and absolute paths
        if source.starts_with("./") || source.starts_with("../") {
            return false;
        }
        if std::path::Path::new(source).is_absolute() {
            return false;
        }

        let source_lower = source.to_lowercase();
        source_lower.contains("github.com")
            || (source_lower.contains('/')
                && source.split('/').count() == 2
                && !std::path::Path::new(source).exists())
    }
}

/// Arguments for update command
#[derive(Debug)]
pub struct UpdateArgs {
    pub package: String,
    pub breaking: bool,
}

/// Arguments for remove command
#[derive(Debug)]
pub struct RemoveArgs {
    pub package: String,
    pub force: bool,
}

/// Arguments for list command
#[derive(Debug, Default)]
pub struct ListArgs {
    pub author: Option<String>,
    pub detailed: bool,
}

impl IntoCommandSpec for InstallArgs {
    fn command_spec() -> CommandSpec {
        use crate::cli::{flag_spec, opt_spec, pos_req_spec};
        CommandSpec {
            summary: "Install package from GitHub URL or local path",
            syntax: Some("install <SOURCE>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("source", "Package source (GitHub URL or local directory)"),
                ArgSpec {
                    name: "install-version",
                    short: Some('i'),
                    long: Some("install-version"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "Specific version to install",
                    ..Default::default()
                },
                opt_spec("token", "GitHub token (or set GITHUB_TOKEN env var)"),
                flag_spec("force", "Force reinstall if already installed"),
                flag_spec("yes", "Skip .gitignore modification prompt"),
                ArgSpec {
                    name: "ai",
                    short: None,
                    long: Some("ai"),
                    kind: ArgKind::Option,
                    value_type: ArgValueType::String,
                    cardinality: Cardinality::Optional,
                    default: None,
                    conflicts_with: vec![],
                    requires: vec![],
                    help: "AI agent to install for (e.g., claude, copilot)",
                    ..Default::default()
                },
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for InstallArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        use crate::cli::{get_bool_val, get_opt_val, get_str_val};
        InstallArgs {
            source: get_str_val(map, "source"),
            install_version: get_opt_val(map, "install-version"),
            token: get_opt_val(map, "token"),
            force: get_bool_val(map, "force"),
            yes: get_bool_val(map, "yes"),
            ai: get_opt_val(map, "ai"),
        }
    }
}

impl IntoCommandSpec for UpdateArgs {
    fn command_spec() -> CommandSpec {
        use crate::cli::{flag_spec, pos_req_spec};
        CommandSpec {
            summary: "Update installed package",
            syntax: Some("update <PACKAGE>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("package", "Package name to update"),
                flag_spec("breaking", "Allow breaking changes"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for UpdateArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        use crate::cli::{get_bool_val, get_str_val};
        UpdateArgs {
            package: get_str_val(map, "package"),
            breaking: get_bool_val(map, "breaking"),
        }
    }
}

impl IntoCommandSpec for RemoveArgs {
    fn command_spec() -> CommandSpec {
        use crate::cli::{flag_spec, pos_req_spec};
        CommandSpec {
            summary: "Remove installed package",
            syntax: Some("remove <PACKAGE>"),
            category: Some("packages"),
            args: vec![
                pos_req_spec("package", "Package name to remove"),
                flag_spec("force", "Force removal without confirmation"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for RemoveArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        use crate::cli::{get_bool_val, get_str_val};
        RemoveArgs {
            package: get_str_val(map, "package"),
            force: get_bool_val(map, "force"),
        }
    }
}

impl IntoCommandSpec for ListArgs {
    fn command_spec() -> CommandSpec {
        use crate::cli::{flag_spec, opt_spec};
        CommandSpec {
            summary: "List installed packages",
            syntax: Some("list"),
            category: Some("packages"),
            args: vec![
                opt_spec("author", "Filter by author"),
                flag_spec("detailed", "Show detailed information"),
            ],
            ..CommandSpec::default()
        }
    }
}

impl FromArgValueMap for ListArgs {
    fn from_arg_value_map(map: &HashMap<String, ArgValue>) -> Self {
        use crate::cli::{get_bool_val, get_opt_val};
        ListArgs {
            author: get_opt_val(map, "author"),
            detailed: get_bool_val(map, "detailed"),
        }
    }
}

/// Directory the package lock file (`packages.lock`) lives in — the `.aikit`
/// directory itself. `AikDirectory` doesn't expose its base path directly, so
/// this derives it from another public accessor rather than adding one.
fn lock_dir_for(aik_dir: &crate::core::filesystem::AikDirectory) -> std::path::PathBuf {
    aik_dir
        .registry_path()
        .parent()
        .expect("registry_path() is always <aikit_dir>/registry.toml")
        .to_path_buf()
}

/// Minimal semantic-version comparator (major.minor.patch only — the same
/// grammar `validate_version_format` already enforces), just enough for
/// `aikit update`'s version-compare needs (FEAT-2 / spec 001 T047) without
/// pulling in a full semver crate dependency.
mod semver_lite {
    use std::cmp::Ordering;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SemVer {
        pub major: u64,
        pub minor: u64,
        pub patch: u64,
    }

    impl SemVer {
        /// Parse `"1.2.3"` or `"v1.2.3"`. Rejects anything with a different
        /// shape (pre-release/build metadata, fewer/more segments) — this
        /// mirrors `validate_version_format`'s strict `^v?\d+\.\d+\.\d+$`.
        pub fn parse(s: &str) -> Option<Self> {
            let s = s.strip_prefix('v').unwrap_or(s);
            let mut parts = s.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            if parts.next().is_some() {
                return None;
            }
            Some(Self {
                major,
                minor,
                patch,
            })
        }
    }

    impl PartialOrd for SemVer {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    impl Ord for SemVer {
        fn cmp(&self, other: &Self) -> Ordering {
            (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
        }
    }

    /// Result of comparing an installed version against a candidate version.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VersionComparison {
        /// `candidate` is newer than `current`. `major_bump` is set when the
        /// major component increased (a `--breaking`-gated change).
        Newer {
            major_bump: bool,
        },
        Same,
        Older,
    }

    /// Compare `current` (installed) against `candidate` (latest available).
    /// Returns `None` if either string isn't a parseable `major.minor.patch`
    /// version.
    pub fn compare(current: &str, candidate: &str) -> Option<VersionComparison> {
        let cur = SemVer::parse(current)?;
        let cand = SemVer::parse(candidate)?;
        Some(match cand.cmp(&cur) {
            Ordering::Greater => VersionComparison::Newer {
                major_bump: cand.major > cur.major,
            },
            Ordering::Equal => VersionComparison::Same,
            Ordering::Less => VersionComparison::Older,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_parse_plain() {
            let v = SemVer::parse("1.2.3").unwrap();
            assert_eq!(
                v,
                SemVer {
                    major: 1,
                    minor: 2,
                    patch: 3
                }
            );
        }

        #[test]
        fn test_parse_v_prefixed() {
            let v = SemVer::parse("v1.2.3").unwrap();
            assert_eq!(
                v,
                SemVer {
                    major: 1,
                    minor: 2,
                    patch: 3
                }
            );
        }

        #[test]
        fn test_parse_rejects_extra_segments() {
            assert!(SemVer::parse("1.2.3.4").is_none());
        }

        #[test]
        fn test_parse_rejects_malformed() {
            assert!(SemVer::parse("not-a-version").is_none());
            assert!(SemVer::parse("1.2").is_none());
            assert!(SemVer::parse("").is_none());
        }

        #[test]
        fn test_compare_newer_patch() {
            assert_eq!(
                compare("1.0.0", "1.0.1"),
                Some(VersionComparison::Newer { major_bump: false })
            );
        }

        #[test]
        fn test_compare_newer_minor() {
            assert_eq!(
                compare("1.0.0", "1.1.0"),
                Some(VersionComparison::Newer { major_bump: false })
            );
        }

        #[test]
        fn test_compare_newer_major_is_flagged() {
            assert_eq!(
                compare("1.9.9", "2.0.0"),
                Some(VersionComparison::Newer { major_bump: true })
            );
        }

        #[test]
        fn test_compare_same() {
            assert_eq!(compare("1.2.3", "1.2.3"), Some(VersionComparison::Same));
        }

        #[test]
        fn test_compare_older() {
            assert_eq!(compare("2.0.0", "1.9.9"), Some(VersionComparison::Older));
        }

        #[test]
        fn test_compare_v_prefix_mixed_with_plain() {
            assert_eq!(
                compare("v1.0.0", "1.1.0"),
                Some(VersionComparison::Newer { major_bump: false })
            );
        }

        #[test]
        fn test_compare_unparseable_returns_none() {
            assert_eq!(compare("not-a-version", "1.0.0"), None);
            assert_eq!(compare("1.0.0", "also-not-a-version"), None);
        }

        #[test]
        fn test_ordering_is_numeric_not_lexicographic() {
            // "9" < "10" numerically but ">" lexicographically as strings —
            // guards against a naive string-compare implementation.
            assert_eq!(
                compare("1.9.0", "1.10.0"),
                Some(VersionComparison::Newer { major_bump: false })
            );
        }
    }
}

/// Execute install command
pub async fn execute_install(args: InstallArgs) -> Result<(), AikError> {
    use crate::core::filesystem::AikDirectory;
    use crate::core::ux::{create_spinner, show_info, show_success, show_warning};
    use crate::models::registry::LocalRegistry;

    let spinner = create_spinner("Detecting source type...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    println!("Installing package from: {}", args.source);

    if let Some(version) = &args.install_version {
        println!("Version: {}", version);
    }

    // Find or create .aikit directory.
    // Use an absolute path so all downstream operations (WalkDir, glob matching,
    // fs::copy) behave consistently across platforms — relative paths combined
    // with an empty project_root caused silent artifact-copy failures on Windows.
    let aik_dir = match AikDirectory::find() {
        Ok(dir) => dir,
        Err(_) => {
            let cwd = std::env::current_dir().map_err(|e| {
                AikError::Generic(format!("Failed to read current directory: {}", e))
            })?;
            let aik_dir = AikDirectory::new(cwd.join(".aikit"));
            println!("Creating .aikit directory...");
            aik_dir
                .create()
                .map_err(|e| AikError::Generic(e.to_string()))?;
            aik_dir
        }
    };

    // Validate inputs
    if let Some(version) = &args.install_version {
        crate::core::validation::validate_version_format(version)?;
    }

    // Detect source type
    spinner.set_message("Detecting source type...");
    let source_type = args.detect_source_type()?;
    spinner.finish_with_message("Source type detected");

    // Keep temp dir alive for GitHub installs until after extraction (avoid "No such file or directory")
    // `resolved_commit_sha` (SEC-7): the mutable ref (e.g. "main") pinned to an
    // immutable commit at fetch time, for the lock file — `None` for local
    // installs or if resolution failed (best-effort, non-fatal).
    let (_temp_guard, package, archive_path, resolved_commit_sha): (
        Option<tempfile::TempDir>,
        crate::models::package::Package,
        Option<std::path::PathBuf>,
        Option<String>,
    ) = match source_type {
        SourceType::LocalFolder(path) => {
            let install_spinner = create_spinner(&format!(
                "Installing from local directory: {}",
                path.display()
            ));
            let result = install_from_local_directory(&path);
            install_spinner.finish_with_message("Local package loaded");
            let (pkg, path_opt) = result?;
            (None, pkg, path_opt, None)
        }
        SourceType::GitHubRepo {
            owner,
            repo,
            version,
        } => {
            show_info(&format!(
                "Installing from GitHub: {}/{}@{}",
                owner, repo, version
            ));

            // Initialize GitHub client with token resolution
            let github = GitHubClient::new(GitHubClient::resolve_token(args.token.clone()))
                .map_err(|e| AikError::Generic(e.to_string()))?;

            // Get package manifest
            let manifest_spinner = create_spinner(&format!(
                "Fetching package manifest from {}/{}...",
                owner, repo
            ));
            let manifest = github
                .get_package_manifest(&owner, &repo, Some(&version))
                .await
                .map_err(|e| AikError::Generic(e.to_string()))?;
            manifest_spinner.finish_with_message("Package manifest fetched");

            // Convert PackageManifest to TOML string for parsing
            let manifest_toml = toml::to_string(&manifest)?;
            let package = crate::models::package::Package::from_toml_str(&manifest_toml)
                .map_err(|e| AikError::Generic(format!("Failed to parse manifest: {}", e)))?;

            // SEC-7: pin the mutable ref to an immutable commit SHA for the lock
            // file. Best-effort — a resolution failure (e.g. rate limit) doesn't
            // block install, it just means the lock entry has no commit_sha.
            let resolved_commit_sha = match github.resolve_ref_to_sha(&owner, &repo, &version).await
            {
                Ok(sha) => Some(sha),
                Err(e) => {
                    eprintln!(
                        "Warning: could not resolve '{}' to a commit SHA: {}",
                        version, e
                    );
                    None
                }
            };

            // Download package archive (temp_dir must outlive install_package_from_archive)
            let download_spinner = create_spinner(&format!(
                "Downloading package {} v{}...",
                package.package.name, package.package.version
            ));

            let temp_dir = tempfile::tempdir().map_err(|e| {
                AikError::Generic(format!("Failed to create temp directory: {}", e))
            })?;
            let archive_path = temp_dir.path().join(format!(
                "{}-{}.zip",
                package.package.name, package.package.version
            ));

            github
                .download_archive(&owner, &repo, Some(&version), &archive_path)
                .await
                .map_err(|e| AikError::Generic(e.to_string()))?;
            download_spinner.finish_with_message("Package downloaded");

            (
                Some(temp_dir),
                package,
                Some(archive_path),
                resolved_commit_sha,
            )
        }
    };

    // Check if already installed
    let registry_path = aik_dir.registry_path();
    let mut registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    if registry.is_installed(&package.package.name) && !args.force {
        if crate::core::ux::confirm_action(&format!(
            "Package '{}' is already installed. Reinstall?",
            package.package.name
        ))? {
            // User confirmed reinstall
        } else {
            show_warning("Installation cancelled by user");
            return Ok(());
        }
    }

    // SEC-7: hash the downloaded archive and verify it against any existing
    // lock entry for this exact package+version *before* extracting anything.
    // A mismatch means the ref this was fetched from now points at different
    // content than what was locked at this version — refuse rather than
    // silently extract possibly-tampered/moved content.
    let resolved_checksum = match &archive_path {
        Some(path) => {
            let bytes = std::fs::read(path).map_err(|e| {
                crate::error::io_context("Failed to read downloaded archive", path, e)
            })?;
            Some(aikit_sdk::fetch::sha256_hex(&bytes))
        }
        None => None,
    };

    let lock_dir = lock_dir_for(&aik_dir);
    if let Some(checksum) = &resolved_checksum {
        let lock_manager = crate::core::lock::LockManager::new(&lock_dir);
        if let Err(mismatch) =
            lock_manager.verify_checksum(&package.package.name, &package.package.version, checksum)
        {
            return Err(AikError::Generic(format!(
                "Refusing to install '{}': {}",
                package.package.name, mismatch
            )));
        }
    }

    // Extract and install package
    let install_spinner = create_spinner("Installing package...");
    let install_result = if let Some(archive_path) = archive_path {
        // Remote installation - extract from downloaded archive
        install_package_from_archive(&package, &archive_path, &aik_dir, &args)
    } else {
        // Local installation - copy directly from source directory
        install_package_from_directory(&package, &args.source, &aik_dir, &args)
    };

    match install_result {
        Ok(_) => install_spinner.finish_with_message("Package installed successfully"),
        Err(e) => {
            install_spinner.finish_with_message("Installation failed");
            return Err(e);
        }
    }

    // Update registry
    use crate::models::package::InstalledPackage;
    let installed = InstalledPackage {
        package: package.package.clone(),
        installed_at: chrono::Utc::now(),
        source_url: args.source.clone(),
        install_path: format!(
            "packages/{}-{}",
            package.package.name, package.package.version
        ),
    };

    registry.add_package(installed.clone());
    registry
        .save_to_file(&registry_path)
        .map_err(|e| AikError::Generic(e.to_string()))?;

    // Wire the lock file (FEAT-4): record the resolved commit SHA + archive
    // checksum now that install has fully succeeded. Re-verifies against the
    // pre-extraction check above (cheap, and covers the local-folder path
    // which has no checksum at all — `add_package_with_integrity` with both
    // `None` is equivalent to the old dead `add_package`).
    let mut lock_manager = crate::core::lock::LockManager::new(&lock_dir);
    lock_manager
        .lock_package_with_integrity(&installed, resolved_commit_sha, resolved_checksum)
        .map_err(|e| AikError::Generic(format!("Failed to update lock file: {}", e)))?;

    // Handle .gitignore
    // Note: skip_gitignore field doesn't exist in InstallArgs, always prompt
    {
        use crate::core::filesystem::GitIgnoreManager;
        let gitignore = GitIgnoreManager::new(std::path::Path::new("."));
        if gitignore.prompt_user() {
            gitignore
                .add_aikit()
                .map_err(|e| AikError::Generic(e.to_string()))?;
            show_info("Added .aikit/ to .gitignore");
        }
    }

    show_success(&format!(
        "Package '{}' v{} installed successfully!",
        package.package.name, package.package.version
    ));

    // Determine which agent(s) to generate commands for
    let selected_agents = resolve_agent_selection(args.ai.as_deref())?;
    if selected_agents.is_empty() {
        return Ok(());
    }

    // Generate agent commands
    println!(
        "Generating agent commands for: {}",
        selected_agents.join(", ")
    );
    if let Err(e) = generate_agent_commands(&package, &aik_dir, &selected_agents) {
        eprintln!("Warning: Failed to generate agent commands: {}", e);
    }

    // Resolve installed package root (handles zipball top-level dir)
    let package_root = match aikit_sdk::installed_package_root(
        &aik_dir.packages_path(),
        &package.package.name,
        &package.package.version,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Warning: Could not resolve package root: {}", e);
            return Ok(());
        }
    };
    let project_root = aik_dir.project_root();

    // Deploy subagents and skills per agent, then copy artifacts
    for agent_key in &selected_agents {
        if let Some(agent_config) = crate::core::agent::get_agent_config(agent_key) {
            if let Err(e) = deploy_subagents_for_agent(
                &package,
                &package_root,
                &project_root,
                agent_key,
                &agent_config,
            ) {
                eprintln!("Warning: Deploy subagents for {}: {}", agent_key, e);
            }
            if let Err(e) = deploy_skills_for_agent(
                &package,
                &package_root,
                &project_root,
                agent_key,
                &agent_config,
            ) {
                eprintln!("Warning: Deploy skills for {}: {}", agent_key, e);
            }
        }
    }

    // Build mappings with agent scope (first selected agent, or None for default mappings)
    let agent_scope = selected_agents.first().map(|s| s.as_str());
    let mappings = package.get_artifact_mappings(agent_scope);

    // Artifact copy must succeed — otherwise the package isn't actually installed
    // from the user's perspective (e.g. `.newton/` would be missing for newton).
    aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)
        .map_err(|e| AikError::Generic(format!("Failed to copy artifacts: {}", e)))?;

    println!(
        "✅ Package '{}' v{} installed successfully!",
        package.package.name, package.package.version
    );
    println!(
        "📦 Installed to: .aikit/packages/{}-{}",
        package.package.name, package.package.version
    );

    Ok(())
}

/// Resolve which agent(s) to use for installation
///
/// - If `ai` is Some, validate and return that agent key.
/// - If a TTY is available, run interactive selection. Returns Ok(vec![]) on Cancelled.
/// - Otherwise (non-interactive, no --ai), return an error.
fn resolve_agent_selection(ai: Option<&str>) -> Result<Vec<String>, AikError> {
    if let Some(ai_arg) = ai {
        crate::core::agent::validate_agent_key(ai_arg)
            .map_err(|e| AikError::InvalidSource(format!("Invalid agent '{}': {}", ai_arg, e)))?;
        return Ok(vec![ai_arg.to_string()]);
    }

    if atty::is(atty::Stream::Stdin) {
        match crate::tui::agent_select::select_agent_interactive()
            .map_err(|e| AikError::Generic(format!("Interactive agent selection failed: {}", e)))?
        {
            crate::tui::agent_select::SelectionResult::Selected(key) => {
                return Ok(vec![key]);
            }
            crate::tui::agent_select::SelectionResult::Cancelled => {
                println!("Installation cancelled.");
                return Ok(vec![]);
            }
        }
    }

    // Non-interactive: require --ai flag
    Err(AikError::InvalidSource(
        "AI agent not specified. Use --ai <agent> to specify an agent, or run in interactive mode.\n\
         Available agents: claude, copilot, cursor, gemini, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob".to_string(),
    ))
}

/// Install package from local directory
fn install_from_local_directory(
    source_path: &std::path::Path,
) -> Result<(crate::models::package::Package, Option<std::path::PathBuf>), AikError> {
    use std::fs;
    use std::path::Path;

    let source_dir = Path::new(source_path);

    // Check if package.toml or aikit.toml exists
    let package_toml_path = source_dir.join("package.toml");
    let aikit_toml_path = source_dir.join("aikit.toml");

    let toml_path = if package_toml_path.exists() {
        package_toml_path
    } else if aikit_toml_path.exists() {
        aikit_toml_path
    } else {
        return Err(AikError::InvalidSource(format!(
            "package.toml or aikit.toml not found in directory: {}",
            source_path.display()
        )));
    };

    let package_toml_content = fs::read_to_string(&toml_path)
        .map_err(|e| crate::error::io_context("Failed to read package manifest", &toml_path, e))?;

    let package =
        crate::models::package::Package::from_toml_str(&package_toml_content).map_err(|e| {
            AikError::Generic(format!("Failed to parse {}: {}", toml_path.display(), e))
        })?;

    // Validate package
    package.validate().map_err(AikError::PackageValidation)?;

    // For local installation, we don't need to download an archive
    // We'll install directly from the source directory
    Ok((package, None))
}

/// Parse GitHub URL and extract owner, repo, and version.
///
/// Delegates the URL grammar to `aikit_sdk::parse_github_url` — the single
/// canonical GitHub-source parser (ARCH-1; this function used to have its
/// own, narrower, copy of the same grammar) — then re-validates
/// owner/repo/version against this CLI's stricter charset checks (defense
/// in depth) and applies this command's own default-version behavior
/// (`"main"`).
fn parse_github_url(
    source: &str,
    version: Option<&str>,
) -> Result<(String, String, String), AikError> {
    // Validate an explicit `--version` flag up front, same as before.
    if let Some(v) = version {
        crate::core::validation::validate_version_format(v)?;
    }

    // The SDK grammar carries an explicit version as an `@version` suffix on
    // the source string rather than as a separate parameter; fold this
    // command's `--version` flag in that way (unless the source already has
    // its own `@ref`) so both entry points share one parser.
    let combined = match version {
        Some(v) if !source.contains('@') => format!("{}@{}", source, v),
        _ => source.to_string(),
    };

    let parsed = aikit_sdk::parse_github_url(&combined)
        .map_err(|e| AikError::InvalidGitHubUrl(e.to_string()))?;

    let (owner, repo, parsed_version) = match parsed {
        aikit_sdk::TemplateSource::GitHub {
            owner,
            repo,
            version,
            ..
        } => (owner, repo, version),
        // `aikit_sdk::parse_github_url` only ever returns the `GitHub`
        // variant of `TemplateSource`.
        _ => unreachable!("parse_github_url always returns TemplateSource::GitHub"),
    };

    // Validate owner and repo names (this CLI's stricter charset checks,
    // defense in depth on top of the SDK parser's own non-empty check).
    crate::core::validation::validate_github_owner_name(&owner)?;
    crate::core::validation::validate_github_repo_name(&repo)?;

    Ok((
        owner,
        repo,
        parsed_version.unwrap_or_else(|| "main".to_string()),
    ))
}

/// Install package from downloaded archive
fn install_package_from_archive(
    package: &crate::models::package::Package,
    archive_path: &std::path::Path,
    aik_dir: &crate::core::filesystem::AikDirectory,
    _args: &InstallArgs,
) -> Result<(), AikError> {
    let install_path = aik_dir
        .install_package(
            &package.package.name,
            &package.package.version,
            archive_path.parent().unwrap_or(std::path::Path::new(".")),
        )
        .map_err(|e| AikError::Generic(e.to_string()))?;

    // Extraction — including all zip-slip / absolute-path / symlink
    // hardening — is delegated to aikit_sdk::extract_zip, the single
    // canonical zip extractor (ARCH-1; previously duplicated here).
    let zip_bytes = std::fs::read(archive_path)
        .map_err(|e| crate::error::io_context("Failed to read archive", archive_path, e))?;
    aikit_sdk::extract_zip(&zip_bytes, &install_path)
        .map_err(|e| AikError::Generic(format!("Failed to extract archive: {}", e)))?;

    Ok(())
}

/// Install package from local directory
fn install_package_from_directory(
    package: &crate::models::package::Package,
    source_dir: &str,
    aik_dir: &crate::core::filesystem::AikDirectory,
    _args: &InstallArgs,
) -> Result<(), AikError> {
    use std::fs;
    use std::path::Path;

    let source_path = Path::new(source_dir);
    let install_path = aik_dir.packages_path().join(format!(
        "{}-{}",
        package.package.name, package.package.version
    ));

    // Create package directory
    fs::create_dir_all(&install_path)?;

    // Copy only relevant files, excluding version control and build artifacts
    copy_package_files(source_path, &install_path).map_err(|e| AikError::Generic(e.to_string()))?;

    Ok(())
}

/// Directories to exclude when copying a whole project-shaped source tree
/// (version control, build artifacts, dependency caches).
const COPY_EXCLUDE_DIRS: &[&str] = &[
    "target",
    "build",
    "out",
    ".git",
    ".aikit",
    "node_modules",
    ".next",
    "dist",
];

/// Copy package files, excluding version control and build directories.
///
/// Delegates to `aikit_sdk::copy_dir_excluding`, the single canonical
/// recursive-copy implementation (ARCH-1; previously duplicated here) —
/// carries the same symlink-skip hardening as every other copy path in this
/// workspace, which matters since `from` may be an untrusted
/// downloaded/extracted package tree.
fn copy_package_files(
    from: &std::path::Path,
    to: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    aikit_sdk::copy_dir_excluding(from, to, COPY_EXCLUDE_DIRS)?;
    Ok(())
}

/// Generate agent-specific command files
fn generate_agent_commands(
    package: &crate::models::package::Package,
    aik_dir: &crate::core::filesystem::AikDirectory,
    agent_keys: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::agent::get_agent_config;

    for agent_key in agent_keys {
        if let Some(agent_config) = get_agent_config(agent_key) {
            generate_commands_for_agent(package, &agent_config, aik_dir)?;
        } else {
            return Err(format!("Unknown agent: {}", agent_key).into());
        }
    }

    Ok(())
}

/// Load template content from installed package directory
fn load_template_content(
    package: &crate::models::package::Package,
    command_name: &str,
    command_def: &crate::models::package::CommandDefinition,
    aik_dir: &crate::core::filesystem::AikDirectory,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::fs;

    let template_path_str = command_def.effective_source(command_name);
    let package_root = aikit_sdk::installed_package_root(
        &aik_dir.packages_path(),
        &package.package.name,
        &package.package.version,
    )
    .map_err(|e| format!("Failed to resolve package root: {}", e))?;
    let full_path = package_root.join(&template_path_str);

    fs::read_to_string(&full_path).map_err(|e| {
        format!(
            "Failed to load template '{}' from package '{}' (path: {}): {}",
            template_path_str,
            package.package.name,
            full_path.display(),
            e
        )
        .into()
    })
}

/// Generate commands for a specific agent
fn generate_commands_for_agent(
    package: &crate::models::package::Package,
    agent: &crate::core::agent::AgentConfig,
    aik_dir: &crate::core::filesystem::AikDirectory,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;

    // Create agent commands directory relative to project root (.aikit parent)
    let project_root = aik_dir.project_root();
    let commands_dir = project_root.join(&agent.output_dir);
    fs::create_dir_all(&commands_dir)?;

    // Generate command files for each package command
    for (command_name, command_def) in &package.commands {
        // Load actual template content from installed package
        let template_content = load_template_content(package, command_name, command_def, aik_dir)?;

        // Generate command content using loaded template
        let content = agent.generate_package_command(
            &package.package.name,
            command_name,
            &command_def.description,
            &template_content,
        );

        // Fix filename: use {package}.{command} instead of {package}.{agent_key}
        let filename = format!("{}.{}.md", package.package.name, command_name);
        let filepath = commands_dir.join(filename);

        fs::write(filepath, content)?;
    }

    Ok(())
}

/// Deploy package subagents for one agent (skips if agent has no agents_dir).
fn deploy_subagents_for_agent(
    package: &crate::models::package::Package,
    package_root: &std::path::Path,
    project_root: &std::path::Path,
    agent_key: &str,
    agent_config: &crate::core::agent::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let agents_dir = match &agent_config.agents_dir {
        Some(d) => d,
        None => return Ok(()),
    };
    let _ = agents_dir; // used by deploy_subagent via aikit-sdk
    for (name, def) in &package.subagents {
        // SEC-5: `source` is an attacker-controlled manifest field; `Package::validate`
        // already rejects absolute/`..` values at parse time, but re-validate at the join
        // site (defense in depth — this is where the read actually happens).
        let src_path = aikit_sdk::safe_join(package_root, &def.source).map_err(|e| {
            format!(
                "Subagent '{}' has an unsafe source path '{}': {}",
                name, def.source, e
            )
        })?;
        let content = std::fs::read_to_string(&src_path).map_err(|e| {
            format!(
                "Failed to read subagent '{}' from {}: {}",
                name,
                src_path.display(),
                e
            )
        })?;
        aikit_sdk::deploy_subagent(agent_key, project_root, name, &content)
            .map_err(|e| format!("deploy_subagent {}: {}", name, e))?;
    }
    Ok(())
}

/// Deploy package skills for one agent by copying each skill folder (skips if agent has no skills_dir).
fn deploy_skills_for_agent(
    package: &crate::models::package::Package,
    package_root: &std::path::Path,
    project_root: &std::path::Path,
    agent_key: &str,
    agent_config: &crate::core::agent::AgentConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let skills_dir = match &agent_config.skills_dir {
        Some(d) => d,
        None => return Ok(()),
    };
    let dest_base = project_root.join(skills_dir);
    for (name, def) in &package.skills {
        // SEC-5: same defense-in-depth re-validation as subagents, for both the
        // attacker-controlled `source` (read side) and `name` (write side — becomes a
        // directory segment under the agent's skills_dir).
        let src_dir = aikit_sdk::safe_join(package_root, &def.source).map_err(|e| {
            format!(
                "Skill '{}' has an unsafe source path '{}': {}",
                name, def.source, e
            )
        })?;
        if !src_dir.is_dir() {
            return Err(format!(
                "Skill '{}' source is not a directory: {}",
                name,
                src_dir.display()
            )
            .into());
        }
        let dest_dir = aikit_sdk::safe_join(&dest_base, name)
            .map_err(|e| format!("Skill '{}' has an unsafe name: {}", name, e))?;
        std::fs::create_dir_all(&dest_dir)?;
        copy_package_files(&src_dir, &dest_dir)?;
    }
    let _ = agent_key;
    Ok(())
}

/// Execute update command
///
/// Real flow (FEAT-2): parse the installed package's stored `source_url` →
/// fetch the manifest at the tracked ref from GitHub → compare the manifest's
/// declared version against what's installed → if newer (and not gated by a
/// major-version bump without `--breaking`), download, verify integrity
/// (SEC-7), extract, and update the registry + lock file; otherwise report
/// up to date honestly. This never claims an update happened when it didn't.
///
/// Known limitation: unlike `install`, this does not re-run per-agent
/// command/subagent/skill deployment (that needs an `--ai` selection this
/// command doesn't take); it does refresh agent-agnostic `[artifacts]`
/// mappings, matching `install`'s default (unscoped) artifact copy.
pub async fn execute_update(args: UpdateArgs) -> Result<(), AikError> {
    use crate::core::filesystem::AikDirectory;

    let github = GitHubClient::new(GitHubClient::resolve_token(None))
        .map_err(|e| AikError::Generic(e.to_string()))?;
    let aik_dir = AikDirectory::find().map_err(|_| {
        AikError::Installation("No packages installed (.aikit directory not found)".to_string())
    })?;
    execute_update_with_client(args, github, aik_dir).await
}

/// The actual `update` implementation, taking a [`GitHubClient`] and an
/// already-resolved [`AikDirectory`](crate::core::filesystem::AikDirectory)
/// so tests can inject a mocked client (see `GitHubClient::for_test`) and a
/// directly-constructed directory — avoiding both real network access *and*
/// `AikDirectory::find()`'s dependence on the process-global current
/// directory (which would otherwise race against every other test in this
/// binary that also changes it). [`execute_update`] is the production entry
/// point and always builds a real client + discovers `.aikit` from the CWD.
async fn execute_update_with_client(
    args: UpdateArgs,
    github: GitHubClient,
    aik_dir: crate::core::filesystem::AikDirectory,
) -> Result<(), AikError> {
    use crate::core::lock::LockManager;
    use crate::core::ux::show_info;
    use crate::models::package::InstalledPackage;
    use crate::models::registry::LocalRegistry;
    use semver_lite::{compare, VersionComparison};

    // Validate package name
    crate::core::validation::validate_package_name(&args.package)?;

    let registry_path = aik_dir.registry_path();
    let mut registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    // Check if package is installed
    let installed_package = registry
        .get_package(&args.package)
        .ok_or_else(|| AikError::PackageNotFound(args.package.clone()))?
        .clone();

    println!(
        "Checking for updates to '{}' (current: {})...",
        args.package, installed_package.package.version
    );

    // Reuse `InstallArgs::detect_source_type` (the same logic `aikit install`
    // uses) on the recorded source_url so local-folder vs GitHub sources are
    // told apart identically. Only GitHub sources have a remote authority to
    // check for updates against.
    let synthetic_args = InstallArgs {
        source: installed_package.source_url.clone(),
        install_version: None,
        token: None,
        force: false,
        yes: false,
        ai: None,
    };
    let (owner, repo, ref_) = match synthetic_args.detect_source_type() {
        Ok(SourceType::GitHubRepo {
            owner,
            repo,
            version,
        }) => (owner, repo, version),
        Ok(SourceType::LocalFolder(_)) => {
            return Err(AikError::InvalidSource(format!(
                "Package '{}' was installed from a local directory ('{}'); `aikit update` only \
                 supports GitHub-sourced packages.",
                args.package, installed_package.source_url
            )));
        }
        Err(_) => {
            return Err(AikError::InvalidSource(format!(
                "Package '{}' has an unrecognized source ('{}'); `aikit update` only supports \
                 GitHub-sourced packages.",
                args.package, installed_package.source_url
            )));
        }
    };

    let spinner = crate::core::ux::create_spinner(&format!(
        "Fetching latest manifest from {}/{}@{}...",
        owner, repo, ref_
    ));
    let manifest = github
        .get_package_manifest(&owner, &repo, Some(&ref_))
        .await
        .map_err(|e| AikError::Generic(format!("Failed to check for updates: {}", e)))?;
    spinner.finish_and_clear();

    let latest_version = manifest.package.version.clone();

    let comparison =
        compare(&installed_package.package.version, &latest_version).ok_or_else(|| {
            AikError::InvalidVersion(format!(
                "Cannot compare versions '{}' (installed) and '{}' (from {}/{}@{}): expected \
             semantic versions (e.g. 1.2.3)",
                installed_package.package.version, latest_version, owner, repo, ref_
            ))
        })?;

    match comparison {
        VersionComparison::Same | VersionComparison::Older => {
            let lock_dir = lock_dir_for(&aik_dir);
            let lock_manager = LockManager::new(&lock_dir);
            match lock_manager.get_locked_version(&args.package) {
                Some(locked_version) => println!(
                    "Package '{}' is already up to date (version {}, locked at {}).",
                    args.package, installed_package.package.version, locked_version
                ),
                None => println!(
                    "Package '{}' is already up to date (version {}).",
                    args.package, installed_package.package.version
                ),
            }
            return Ok(());
        }
        VersionComparison::Newer { major_bump } => {
            if major_bump && !args.breaking {
                return Err(AikError::Generic(format!(
                    "Update for '{}' is a major version bump ({} -> {}); pass --breaking to \
                     allow it.",
                    args.package, installed_package.package.version, latest_version
                )));
            }
            show_info(&format!(
                "Updating '{}': {} -> {}",
                args.package, installed_package.package.version, latest_version
            ));
        }
    }

    // SEC-7: pin the tracked ref to an immutable commit SHA for the lock
    // file. Best-effort — doesn't block the update if resolution fails.
    let resolved_commit_sha = match github.resolve_ref_to_sha(&owner, &repo, &ref_).await {
        Ok(sha) => Some(sha),
        Err(e) => {
            eprintln!(
                "Warning: could not resolve '{}' to a commit SHA: {}",
                ref_, e
            );
            None
        }
    };

    // Convert the fetched manifest into a Package the same way `install` does.
    let manifest_toml = toml::to_string(&manifest)?;
    let package = crate::models::package::Package::from_toml_str(&manifest_toml)
        .map_err(|e| AikError::Generic(format!("Failed to parse manifest: {}", e)))?;

    // Download the new version's archive.
    let temp_dir = tempfile::tempdir()
        .map_err(|e| AikError::Generic(format!("Failed to create temp directory: {}", e)))?;
    let archive_path = temp_dir
        .path()
        .join(format!("{}-{}.zip", package.package.name, latest_version));
    github
        .download_archive(&owner, &repo, Some(&ref_), &archive_path)
        .await
        .map_err(|e| AikError::Generic(e.to_string()))?;

    // SEC-7: verify archive integrity against any existing lock entry for
    // this exact package+version *before* extracting anything. This mostly
    // matters for `aikit update` run twice against the same latest version
    // (or a re-run after a partial failure): if the ref's content changed
    // underneath an unchanged manifest version, refuse rather than extract
    // possibly-tampered content.
    let zip_bytes = std::fs::read(&archive_path).map_err(|e| {
        crate::error::io_context("Failed to read downloaded archive", &archive_path, e)
    })?;
    let checksum = aikit_sdk::fetch::sha256_hex(&zip_bytes);

    let lock_dir = lock_dir_for(&aik_dir);
    let lock_manager_check = LockManager::new(&lock_dir);
    if let Err(mismatch) =
        lock_manager_check.verify_checksum(&package.package.name, &latest_version, &checksum)
    {
        return Err(AikError::Generic(format!(
            "Refusing to update '{}': {}",
            package.package.name, mismatch
        )));
    }

    // Extract the new version alongside (not over) the old one.
    let install_path = aik_dir
        .install_package(
            &package.package.name,
            &latest_version,
            archive_path.parent().unwrap_or(std::path::Path::new(".")),
        )
        .map_err(|e| AikError::Generic(e.to_string()))?;
    aikit_sdk::extract_zip(&zip_bytes, &install_path)
        .map_err(|e| AikError::Generic(format!("Failed to extract archive: {}", e)))?;

    // Refresh agent-agnostic `[artifacts]` mappings from the new version
    // (matches `install`'s default/unscoped artifact copy).
    if let Ok(package_root) = aikit_sdk::installed_package_root(
        &aik_dir.packages_path(),
        &package.package.name,
        &latest_version,
    ) {
        let project_root = aik_dir.project_root();
        let mappings = package.get_artifact_mappings(None);
        if let Err(e) = aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings) {
            eprintln!("Warning: Failed to refresh artifacts: {}", e);
        }
    }

    // Update the registry to point at the new version.
    let old_version = installed_package.package.version.clone();
    let updated = InstalledPackage {
        package: package.package.clone(),
        installed_at: chrono::Utc::now(),
        source_url: installed_package.source_url.clone(),
        install_path: format!("packages/{}-{}", package.package.name, latest_version),
    };
    registry.add_package(updated.clone());
    registry
        .save_to_file(&registry_path)
        .map_err(|e| AikError::Generic(e.to_string()))?;

    // Wire the lock file (FEAT-4): record the resolved commit SHA + checksum
    // now that the update fully succeeded.
    let mut lock_manager = LockManager::new(&lock_dir);
    lock_manager
        .lock_package_with_integrity(&updated, resolved_commit_sha, Some(checksum))
        .map_err(|e| AikError::Generic(format!("Failed to update lock file: {}", e)))?;

    // Best-effort cleanup of the old version's package directory — failing
    // to clean up doesn't make the update itself unsuccessful.
    if old_version != latest_version {
        if let Err(e) = aik_dir.remove_package(&package.package.name, &old_version) {
            eprintln!(
                "Warning: Failed to remove old version {} of '{}': {}",
                old_version, package.package.name, e
            );
        }
    }

    println!(
        "✅ Package '{}' updated: {} -> {}",
        package.package.name, old_version, latest_version
    );

    Ok(())
}

/// Execute remove command
pub async fn execute_remove(args: RemoveArgs) -> Result<(), AikError> {
    use crate::core::filesystem::AikDirectory;
    use crate::models::registry::LocalRegistry;

    // Validate package name
    crate::core::validation::validate_package_name(&args.package)?;

    let aik_dir = AikDirectory::find().map_err(|_| {
        AikError::Installation("No packages installed (.aikit directory not found)".to_string())
    })?;

    let registry_path = aik_dir.registry_path();
    let mut registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    // Check if package is installed
    if !registry.is_installed(&args.package) {
        return Err(AikError::PackageNotFound(args.package.clone()));
    }

    // Confirm removal unless forced
    if !args.force {
        println!(
            "Are you sure you want to remove package '{}'?",
            args.package
        );
        println!("This will delete all associated files and commands. (y/N): ");

        // For now, assume yes in automated context
        // TODO: Add interactive confirmation
    }

    // Get installed package info to determine version
    let installed_package = registry
        .get_package(&args.package)
        .ok_or_else(|| AikError::PackageNotFound(args.package.clone()))?;

    // Remove package files
    aik_dir
        .remove_package(&args.package, &installed_package.package.version)
        .map_err(|e| AikError::Generic(e.to_string()))?;

    // Remove from registry
    registry.remove_package(&args.package);
    registry
        .save_to_file(&registry_path)
        .map_err(|e| AikError::Generic(e.to_string()))?;

    // Remove from the lock file (FEAT-4) — an uninstalled package has no
    // integrity record to keep around, and leaving a stale entry would make
    // a future reinstall's SEC-7 checksum comparison meaningless.
    {
        let lock_dir = lock_dir_for(&aik_dir);
        let mut lock_manager = crate::core::lock::LockManager::new(&lock_dir);
        if lock_manager.is_locked(&args.package) {
            lock_manager
                .unlock_package(&args.package)
                .map_err(|e| AikError::Generic(format!("Failed to update lock file: {}", e)))?;
        }
    }

    // Remove agent commands
    remove_agent_commands(&args.package).map_err(|e| AikError::Generic(e.to_string()))?;

    println!("✅ Package '{}' removed successfully!", args.package);

    Ok(())
}

/// Remove agent commands for a package
fn remove_agent_commands(package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::agent::get_agent_configs;
    use std::fs;

    for agent in get_agent_configs() {
        let commands_dir = std::path::Path::new(&agent.output_dir);
        if commands_dir.exists() {
            // Remove command files that start with the package name
            for entry in fs::read_dir(commands_dir)? {
                let entry = entry?;
                let filename = entry.file_name().to_string_lossy().to_string();

                // Check if this is a command file for this package
                if filename
                    .to_lowercase()
                    .starts_with(&format!("{}.", package_name.to_lowercase()))
                    && filename.to_lowercase().ends_with(".md")
                {
                    fs::remove_file(entry.path())?;
                }
            }
        }
    }

    Ok(())
}

/// Execute list command
pub async fn execute_list(args: ListArgs) -> Result<(), Box<dyn std::error::Error>> {
    use crate::core::filesystem::AikDirectory;
    use crate::models::registry::LocalRegistry;

    let aik_dir = match AikDirectory::find() {
        Ok(dir) => dir,
        Err(_) => {
            println!("No packages installed (.aikit directory not found)");
            return Ok(());
        }
    };

    let registry_path = aik_dir.registry_path();
    let registry =
        LocalRegistry::load_from_file(&registry_path).unwrap_or_else(|_| LocalRegistry::new());

    let packages = if let Some(author) = &args.author {
        registry.packages_by_author(author)
    } else {
        registry.list_packages()
    };

    if packages.is_empty() {
        println!("No packages installed");
        return Ok(());
    }

    if args.detailed {
        println!("Installed packages:");
        println!(
            "{:<25} {:<12} {:<15} Description",
            "Name", "Version", "Author"
        );
        println!("{:-<80}", "");

        for package in packages {
            let author = package
                .package
                .authors
                .first()
                .unwrap_or(&"Unknown".to_string())
                .clone();
            println!(
                "{:<25} {:<12} {:<15} {}",
                package.package.name, package.package.version, author, package.package.description
            );
        }
    } else {
        println!("Installed packages:");
        for package in packages {
            println!(
                "  {} v{} - {}",
                package.package.name, package.package.version, package.package.description
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // Note: `execute_update_with_client` takes an already-resolved
    // `AikDirectory` rather than calling `AikDirectory::find()` itself
    // (unlike `execute_install`/`execute_remove`), specifically so these
    // tests never need to touch the process-global current directory — no
    // CWD guard/lock needed here.

    /// Build a minimal `.aikit/registry.toml` fixture recording one
    /// GitHub-sourced installed package, for `execute_update_with_client`
    /// tests. Returns the `AikDirectory`.
    fn fixture_installed_package(
        project_root: &std::path::Path,
        name: &str,
        version: &str,
        source_url: &str,
    ) -> crate::core::filesystem::AikDirectory {
        use crate::models::package::{InstalledPackage, PackageMetadata};
        use crate::models::registry::LocalRegistry;

        let aik_dir = crate::core::filesystem::AikDirectory::new(project_root.join(".aikit"));
        aik_dir.create().unwrap();

        let mut registry = LocalRegistry::new();
        registry.add_package(InstalledPackage {
            package: PackageMetadata {
                name: name.to_string(),
                version: version.to_string(),
                description: "Fixture package".to_string(),
                authors: vec![],
                license: None,
                homepage: None,
                repository: None,
            },
            installed_at: chrono::Utc::now(),
            source_url: source_url.to_string(),
            install_path: format!("packages/{}-{}", name, version),
        });
        registry.save_to_file(&aik_dir.registry_path()).unwrap();

        aik_dir
    }

    /// Minimal `aikit.toml`-shaped manifest body for a mocked
    /// raw.githubusercontent.com response.
    fn manifest_toml_body(name: &str, version: &str) -> String {
        format!(
            "[package]\nname = \"{}\"\nversion = \"{}\"\ndescription = \"Fixture package\"\nauthors = []\n",
            name, version
        )
    }

    /// A minimal valid zip archive (single file) for mocked
    /// `.../zipball/...` responses — extraction doesn't need `aikit.toml`
    /// since `execute_update` builds its `Package` from the already-fetched
    /// manifest, not from re-reading the extracted archive.
    fn minimal_zip_bytes() -> Vec<u8> {
        use std::io::Write;
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut buf);
            let options = FileOptions::default();
            zip.start_file("README.md", options).unwrap();
            zip.write_all(b"hello").unwrap();
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn test_detect_source_type_local_directory() {
        let temp_dir = TempDir::new().unwrap();
        let aikit_toml = temp_dir.path().join("aikit.toml");
        fs::write(
            &aikit_toml,
            "[package]\nname = \"test\"\nversion = \"1.0.0\"",
        )
        .unwrap();

        let args = InstallArgs {
            source: temp_dir.path().to_string_lossy().to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::LocalFolder(path) => {
                assert_eq!(path, temp_dir.path());
            }
            _ => panic!("Expected LocalFolder"),
        }
    }

    #[test]
    fn test_detect_source_type_github_url() {
        let args = InstallArgs {
            source: "https://github.com/owner/repo".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        // This should parse as a GitHub URL successfully
        assert!(result.is_ok());
        match result.unwrap() {
            SourceType::GitHubRepo {
                owner,
                repo,
                version,
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "repo");
                assert_eq!(version, "main");
            }
            _ => panic!("Expected GitHubRepo"),
        }
    }

    #[test]
    fn test_detect_source_type_invalid() {
        let args = InstallArgs {
            source: "not-a-valid-source".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        let result = args.detect_source_type();
        assert!(result.is_err());
    }

    /// Test artifact copy with Newton template pattern (newton/** -> .newton)
    #[test]
    fn test_copy_artifacts_newton_template() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = TempDir::new()?;
        let work = temp.path();

        // Create a mock package root with newton/ structure
        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/scripts"))?;

        // Create test files
        fs::write(package_root.join("newton/README.md"), "# Newton Template")?;
        fs::write(
            package_root.join("newton/scripts/advisor.sh"),
            "#!/bin/sh\necho advisor",
        )?;
        fs::write(
            package_root.join("newton/scripts/evaluator.sh"),
            "#!/bin/sh\necho evaluator",
        )?;

        // Create project root
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root)?;

        // Create a mock package with artifact mapping
        let mut package = crate::models::package::Package::new(
            "newton-templates".to_string(),
            "1.0.0".to_string(),
            "Newton workspace template".to_string(),
        );
        package
            .artifacts
            .insert("newton/**".to_string(), ".newton".to_string());

        // Build mappings and use SDK copy_artifacts
        let mappings = package.get_artifact_mappings(None);
        aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify files were copied correctly
        assert!(project_root.join(".newton/README.md").exists());
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());
        assert!(project_root.join(".newton/scripts/evaluator.sh").exists());

        // Verify content
        let readme = fs::read_to_string(project_root.join(".newton/README.md"))?;
        assert!(readme.contains("Newton Template"));

        let advisor = fs::read_to_string(project_root.join(".newton/scripts/advisor.sh"))?;
        assert!(advisor.contains("echo advisor"));

        Ok(())
    }

    /// Test artifact copy with nested directory structure
    #[test]
    fn test_copy_artifacts_nested_structure() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = TempDir::new()?;
        let work = temp.path();

        // Create package root with nested structure
        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/deeply/nested/dir"))?;

        // Create files at various depths
        fs::write(package_root.join("newton/top.txt"), "top")?;
        fs::write(package_root.join("newton/deeply/nested/file.txt"), "nested")?;

        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root)?;

        let mut package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test".to_string(),
        );
        package
            .artifacts
            .insert("newton/**".to_string(), ".newton".to_string());

        // Build mappings and use SDK copy_artifacts
        let mappings = package.get_artifact_mappings(None);
        aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify nested files were copied
        assert!(project_root.join(".newton/top.txt").exists());
        assert!(project_root.join(".newton/deeply/nested/file.txt").exists());

        Ok(())
    }

    /// Test artifact copy with glob pattern filtering
    #[test]
    fn test_copy_artifacts_glob_pattern() -> Result<(), Box<dyn std::error::Error>> {
        use std::fs;

        let temp = TempDir::new()?;
        let work = temp.path();

        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/scripts"))?;
        fs::create_dir_all(package_root.join("other"))?;

        // Create files in both directories
        fs::write(package_root.join("newton/scripts/advisor.sh"), "#!/bin/sh")?;
        fs::write(package_root.join("other/ignore.txt"), "ignore")?;

        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root)?;

        let mut package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test".to_string(),
        );
        // Only copy newton/**, not other/**
        package
            .artifacts
            .insert("newton/**".to_string(), ".newton".to_string());

        // Build mappings and use SDK copy_artifacts
        let mappings = package.get_artifact_mappings(None);
        aikit_sdk::copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify only newton/** files were copied
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());
        assert!(!project_root.join("other/ignore.txt").exists());

        Ok(())
    }

    /// Test that install_package_from_archive rejects path traversal attempts
    #[test]
    fn test_install_package_from_archive_path_traversal() {
        use std::fs::File;
        use std::io::Write;
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let temp_dir = TempDir::new().unwrap();
        let archive_path = temp_dir.path().join("package.zip");

        // Create a malicious zip with path traversal
        let file = File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default();

        // Add a benign file
        zip.start_file("safe.txt", options).unwrap();
        zip.write_all(b"safe content").unwrap();

        // Add a path traversal file (should be blocked)
        zip.start_file("../../../escape.txt", options).unwrap();
        zip.write_all(b"malicious content").unwrap();

        zip.finish().unwrap();

        // Create a mock package
        let package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test package".to_string(),
        );

        // Create a mock AikDirectory
        let aik_dir = crate::core::filesystem::AikDirectory::new(temp_dir.path().join(".aikit"));

        // Create the install directory
        let install_path = aik_dir.packages_path().join(format!(
            "{}-{}",
            package.package.name, package.package.version
        ));
        std::fs::create_dir_all(&install_path).unwrap();

        // Create InstallArgs (the function doesn't use args in the current implementation)
        let args = InstallArgs {
            source: "test".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        // Try to install from the malicious archive
        let result = install_package_from_archive(&package, &archive_path, &aik_dir, &args);

        // Should fail (either due to path traversal or file system error)
        assert!(
            result.is_err(),
            "Expected installation to fail, but it succeeded"
        );

        // Verify that the escape file was NOT created outside the install directory
        let escape_file = temp_dir.path().join("escape.txt");
        assert!(
            !escape_file.exists(),
            "Malicious file was created outside install directory!"
        );

        // Verify no files were created in the temp directory outside the install directory
        let files_outside = std::fs::read_dir(temp_dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.file_name().unwrap() != "package.zip")
            .collect::<Vec<_>>();

        assert!(
            files_outside.is_empty(),
            "Found unexpected files outside install directory: {:?}",
            files_outside
        );
    }

    /// Test that install_package_from_archive rejects absolute path attempts
    #[test]
    fn test_install_package_from_archive_absolute_path() {
        use std::fs::File;
        use std::io::Write;
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let temp_dir = TempDir::new().unwrap();
        let archive_path = temp_dir.path().join("package.zip");

        // Create a malicious zip with absolute path
        let file = File::create(&archive_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default();

        // Add an absolute path file first (should be blocked)
        zip.start_file("/etc/passwd", options).unwrap();
        zip.write_all(b"malicious content").unwrap();

        // Add a benign file (should not be created due to error above)
        zip.start_file("safe.txt", options).unwrap();
        zip.write_all(b"safe content").unwrap();

        zip.finish().unwrap();

        // Create a mock package
        let package = crate::models::package::Package::new(
            "test-pkg".to_string(),
            "1.0.0".to_string(),
            "Test package".to_string(),
        );

        // Create a mock AikDirectory
        let aik_dir = crate::core::filesystem::AikDirectory::new(temp_dir.path().join(".aikit"));

        // Create the install directory
        let install_path = aik_dir.packages_path().join(format!(
            "{}-{}",
            package.package.name, package.package.version
        ));
        std::fs::create_dir_all(&install_path).unwrap();

        // Create InstallArgs
        let args = InstallArgs {
            source: "test".to_string(),
            install_version: None,
            token: None,
            force: false,
            yes: false,
            ai: None,
        };

        // Try to install from the malicious archive
        let result = install_package_from_archive(&package, &archive_path, &aik_dir, &args);

        // Should fail due to absolute path
        assert!(
            result.is_err(),
            "Expected installation to fail, but it succeeded"
        );

        // Verify safe.txt was NOT created (since extraction stopped at the absolute path error)
        assert!(
            !install_path.join("safe.txt").exists(),
            "No files should have been created due to absolute path error"
        );
    }

    // -- ARCH-1: parse_github_url now delegates to aikit_sdk::parse_github_url
    // ------------------------------------------------------------------------

    #[test]
    fn test_parse_github_url_short_form_defaults_to_main() {
        let (owner, repo, version) = parse_github_url("owner/repo", None).unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
        assert_eq!(version, "main");
    }

    #[test]
    fn test_parse_github_url_github_com_prefix() {
        let (owner, repo, version) = parse_github_url("github.com/owner/repo", None).unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
        assert_eq!(version, "main");
    }

    #[test]
    fn test_parse_github_url_explicit_version_flag_is_applied() {
        let (owner, repo, version) = parse_github_url("owner/repo", Some("1.2.3")).unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn test_parse_github_url_release_asset_style_url_still_resolves_owner_repo() {
        // The old parser silently dropped everything past owner/repo (e.g. a
        // release-asset URL); the unified parser must still resolve the same
        // owner/repo rather than erroring on the extra segments.
        let (owner, repo, version) = parse_github_url(
            "https://github.com/owner/repo/releases/download/v1.0.0/package.zip",
            None,
        )
        .unwrap();
        assert_eq!(owner, "owner");
        assert_eq!(repo, "repo");
        assert_eq!(version, "main");
    }

    #[test]
    fn test_parse_github_url_rejects_invalid_owner_charset() {
        // Defense-in-depth validation (validate_github_owner_name) must still
        // reject an owner name the SDK's lighter-weight grammar would accept.
        let result = parse_github_url("not valid owner/repo", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_github_url_rejects_invalid_explicit_version() {
        let result = parse_github_url("owner/repo", Some("not-a-semver"));
        assert!(result.is_err());
    }

    // -- execute_update (FEAT-2 real flow), mocked GitHub, no real network --

    #[tokio::test]
    async fn test_execute_update_reports_up_to_date_honestly() {
        let temp = TempDir::new().unwrap();
        let aik_dir = fixture_installed_package(temp.path(), "demo", "1.0.0", "owner/repo");

        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/owner/repo/main/aikit.toml")
            .with_status(200)
            .with_body(manifest_toml_body("demo", "1.0.0"))
            .create_async()
            .await;

        let github = GitHubClient::for_test(server.url());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "demo".to_string(),
                breaking: false,
            },
            github,
            crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit")),
        )
        .await;

        assert!(result.is_ok(), "unexpected error: {:?}", result.err());

        // Must NOT have touched the registry — same version in, same out.
        let registry =
            crate::models::registry::LocalRegistry::load_from_file(&aik_dir.registry_path())
                .unwrap();
        assert_eq!(
            registry.get_package("demo").unwrap().package.version,
            "1.0.0"
        );
    }

    #[tokio::test]
    async fn test_execute_update_installs_when_newer() {
        let temp = TempDir::new().unwrap();
        let aik_dir = fixture_installed_package(temp.path(), "demo", "1.0.0", "owner/repo");

        // Pre-existing old-version package dir, to prove cleanup happens.
        let old_pkg_dir = aik_dir.packages_path().join("demo-1.0.0");
        fs::create_dir_all(&old_pkg_dir).unwrap();
        fs::write(old_pkg_dir.join("marker.txt"), "old").unwrap();

        let mut server = mockito::Server::new_async().await;
        let _manifest = server
            .mock("GET", "/owner/repo/main/aikit.toml")
            .with_status(200)
            .with_body(manifest_toml_body("demo", "1.1.0"))
            .create_async()
            .await;
        let _commit = server
            .mock("GET", "/repos/owner/repo/commits/main")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sha": "resolved-sha-123"}"#)
            .create_async()
            .await;
        let _zip = server
            .mock("GET", "/repos/owner/repo/zipball/main")
            .with_status(200)
            .with_body(minimal_zip_bytes())
            .create_async()
            .await;

        let github = GitHubClient::for_test(server.url());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "demo".to_string(),
                breaking: false,
            },
            github,
            crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit")),
        )
        .await;

        assert!(result.is_ok(), "unexpected error: {:?}", result.err());

        // Registry now points at the new version.
        let registry_path = aik_dir.registry_path();
        let registry =
            crate::models::registry::LocalRegistry::load_from_file(&registry_path).unwrap();
        assert_eq!(
            registry.get_package("demo").unwrap().package.version,
            "1.1.0"
        );

        // New version extracted to disk.
        assert!(aik_dir.packages_path().join("demo-1.1.0").exists());

        // Old version cleaned up.
        assert!(!old_pkg_dir.exists());

        // Lock file recorded the resolved commit SHA + a checksum for the
        // new version (SEC-7 / FEAT-4).
        let lock_dir = lock_dir_for(&aik_dir);
        let lock_manager = crate::core::lock::LockManager::new(&lock_dir);
        assert_eq!(lock_manager.get_locked_version("demo"), Some("1.1.0"));
    }

    #[tokio::test]
    async fn test_execute_update_major_bump_without_breaking_is_rejected() {
        let temp = TempDir::new().unwrap();
        let aik_dir = fixture_installed_package(temp.path(), "demo", "1.0.0", "owner/repo");

        let mut server = mockito::Server::new_async().await;
        let _manifest = server
            .mock("GET", "/owner/repo/main/aikit.toml")
            .with_status(200)
            .with_body(manifest_toml_body("demo", "2.0.0"))
            .create_async()
            .await;

        let github = GitHubClient::for_test(server.url());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "demo".to_string(),
                breaking: false,
            },
            github,
            crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit")),
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("--breaking"));

        // Registry must be untouched — a rejected major bump is not an update.
        let registry =
            crate::models::registry::LocalRegistry::load_from_file(&aik_dir.registry_path())
                .unwrap();
        assert_eq!(
            registry.get_package("demo").unwrap().package.version,
            "1.0.0"
        );
    }

    #[tokio::test]
    async fn test_execute_update_major_bump_with_breaking_succeeds() {
        let temp = TempDir::new().unwrap();
        let aik_dir = fixture_installed_package(temp.path(), "demo", "1.0.0", "owner/repo");

        let mut server = mockito::Server::new_async().await;
        let _manifest = server
            .mock("GET", "/owner/repo/main/aikit.toml")
            .with_status(200)
            .with_body(manifest_toml_body("demo", "2.0.0"))
            .create_async()
            .await;
        let _commit = server
            .mock("GET", "/repos/owner/repo/commits/main")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sha": "resolved-sha-456"}"#)
            .create_async()
            .await;
        let _zip = server
            .mock("GET", "/repos/owner/repo/zipball/main")
            .with_status(200)
            .with_body(minimal_zip_bytes())
            .create_async()
            .await;

        let github = GitHubClient::for_test(server.url());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "demo".to_string(),
                breaking: true,
            },
            github,
            crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit")),
        )
        .await;

        assert!(result.is_ok(), "unexpected error: {:?}", result.err());

        let registry =
            crate::models::registry::LocalRegistry::load_from_file(&aik_dir.registry_path())
                .unwrap();
        assert_eq!(
            registry.get_package("demo").unwrap().package.version,
            "2.0.0"
        );
    }

    #[tokio::test]
    async fn test_execute_update_rejects_on_checksum_mismatch() {
        // SEC-7: pre-seed the lock file with a checksum for the *target*
        // version that does NOT match what the mocked zip will hash to —
        // simulating a mutable ref whose content changed underneath an
        // unchanged manifest version. `execute_update` must refuse rather
        // than silently overwrite.
        let temp = TempDir::new().unwrap();
        let aik_dir = fixture_installed_package(temp.path(), "demo", "1.0.0", "owner/repo");

        {
            use crate::models::package::{InstalledPackage, PackageMetadata};
            let lock_dir = lock_dir_for(&aik_dir);
            let mut lock_manager = crate::core::lock::LockManager::new(&lock_dir);
            lock_manager
                .lock_package_with_integrity(
                    &InstalledPackage {
                        package: PackageMetadata {
                            name: "demo".to_string(),
                            version: "1.1.0".to_string(),
                            description: "Fixture package".to_string(),
                            authors: vec![],
                            license: None,
                            homepage: None,
                            repository: None,
                        },
                        installed_at: chrono::Utc::now(),
                        source_url: "owner/repo".to_string(),
                        install_path: "packages/demo-1.1.0".to_string(),
                    },
                    Some("some-other-sha".to_string()),
                    Some("THIS-WILL-NOT-MATCH-THE-FRESH-ARCHIVE-CHECKSUM".to_string()),
                )
                .unwrap();
        }

        let mut server = mockito::Server::new_async().await;
        let _manifest = server
            .mock("GET", "/owner/repo/main/aikit.toml")
            .with_status(200)
            .with_body(manifest_toml_body("demo", "1.1.0"))
            .create_async()
            .await;
        let _commit = server
            .mock("GET", "/repos/owner/repo/commits/main")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"sha": "some-other-sha"}"#)
            .create_async()
            .await;
        let _zip = server
            .mock("GET", "/repos/owner/repo/zipball/main")
            .with_status(200)
            .with_body(minimal_zip_bytes())
            .create_async()
            .await;

        let github = GitHubClient::for_test(server.url());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "demo".to_string(),
                breaking: false,
            },
            github,
            crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit")),
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("checksum mismatch"));

        // Registry must remain at the old version — the update was refused.
        let registry =
            crate::models::registry::LocalRegistry::load_from_file(&aik_dir.registry_path())
                .unwrap();
        assert_eq!(
            registry.get_package("demo").unwrap().package.version,
            "1.0.0"
        );

        // Nothing extracted to disk for the rejected version.
        assert!(!aik_dir.packages_path().join("demo-1.1.0").exists());
    }

    #[tokio::test]
    async fn test_execute_update_local_folder_source_is_rejected() {
        let temp = TempDir::new().unwrap();
        // A source_url that looks like a local (nonexistent) absolute path,
        // not a GitHub source.
        fixture_installed_package(
            temp.path(),
            "demo",
            "1.0.0",
            "/some/local/path/that/is/gone",
        );

        let github = GitHubClient::for_test("http://127.0.0.1:1".to_string());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "demo".to_string(),
                breaking: false,
            },
            github,
            crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit")),
        )
        .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only supports GitHub-sourced packages"));
    }

    #[tokio::test]
    async fn test_execute_update_package_not_found() {
        let temp = TempDir::new().unwrap();
        let aik_dir = crate::core::filesystem::AikDirectory::new(temp.path().join(".aikit"));
        aik_dir.create().unwrap();

        let github = GitHubClient::for_test("http://127.0.0.1:1".to_string());
        let result = execute_update_with_client(
            UpdateArgs {
                package: "does-not-exist".to_string(),
                breaking: false,
            },
            github,
            aik_dir,
        )
        .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AikError::PackageNotFound(name) => assert_eq!(name, "does-not-exist"),
            other => panic!("expected PackageNotFound, got {:?}", other),
        }
    }
}
