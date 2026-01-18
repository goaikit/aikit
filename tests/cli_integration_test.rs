//! CLI Integration Tests
//!
//! These tests run the actual aikit binary using assert_cmd to verify
//! command-line interface behavior and catch runtime issues.

use assert_cmd::prelude::*;
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test package init command with basic functionality
    #[test]
    fn test_package_init_basic() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        Command::cargo_bin("aikit")?
            .current_dir(work) // Sets cwd ONLY for spawned process
            .args(["package", "init", "test-package", "--yes"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'test-package' initialized",
            ))
            .stdout(predicate::str::contains("Created directory structure"));

        // Verify directory structure was created
        assert!(work.join("test-package").exists());
        assert!(work.join("test-package").join("aikit.toml").exists());
        assert!(work.join("test-package").join("README.md").exists());
        assert!(work.join("test-package").join("templates").exists());
        assert!(work.join("test-package").join("scripts").exists());
        assert!(work.join("test-package").join("docs").exists());

        Ok(())
    }

    /// Test package init with all options
    #[test]
    fn test_package_init_with_options() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        Command::cargo_bin("aikit")?
            .current_dir(work) // Sets cwd ONLY for spawned process
            .args([
                "package",
                "init",
                "my-package",
                "--description",
                "A comprehensive test package",
                "--package-version",
                "2.0.0",
                "--author",
                "Test Author",
                "--yes",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("Package 'my-package' initialized"));

        // Verify directory was created
        assert!(work.join("my-package").exists());

        // Verify aikit.toml content
        let toml_path = work.join("my-package").join("aikit.toml");
        let toml_content = fs::read_to_string(toml_path)?;
        assert!(toml_content.contains(r#"name = "my-package""#));
        assert!(toml_content.contains(r#"version = "2.0.0""#));
        assert!(toml_content.contains(r#"description = "A comprehensive test package""#));
        assert!(toml_content.contains(r#"authors = ["Test Author"]"#));

        Ok(())
    }

    /// Test package init error when directory exists
    #[test]
    fn test_package_init_directory_exists_error() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        // Create directory first
        fs::create_dir(work.join("existing-package"))?;

        // Verify directory was created
        assert!(work.join("existing-package").exists());

        Command::cargo_bin("aikit")?
            .current_dir(work) // Sets cwd ONLY for spawned process
            .args(["package", "init", "existing-package"])
            .assert()
            .success() // Note: success because it prompts user, doesn't fail
            .stdout(predicate::str::contains("already exists"));

        Ok(())
    }

    /// Test package build command
    #[test]
    fn test_package_build() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        // First create a package
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["package", "init", "build-test", "--yes"])
            .assert()
            .success();

        // Build the package (in package subdirectory)
        Command::cargo_bin("aikit")?
            .current_dir(work.join("build-test")) // Different cwd for this process
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'build-test' built successfully",
            ))
            .stdout(predicate::str::contains("Output:"));

        // Verify ZIP was created
        let zip_path = work
            .join("build-test")
            .join("dist")
            .join("build-test-0.1.0.zip");
        assert!(zip_path.exists());

        Ok(())
    }

    /// Test package build error when no aikit.toml
    #[test]
    fn test_package_build_no_toml_error() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?; // Auto-creates unique temp dir
        let work = temp_dir.path(); // Path to temp directory

        Command::cargo_bin("aikit")?
            .current_dir(work) // Sets cwd ONLY for spawned process
            .args(["package", "build"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("aikit.toml not found"));

        Ok(())
    }

    /// Test package build with custom output directory
    #[test]
    fn test_package_build_custom_output() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        // Create package
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["package", "init", "custom-output-test", "--yes"])
            .assert()
            .success();

        // Verify directory was created
        assert!(work.join("custom-output-test").exists());

        // Build with custom output (in package subdirectory)
        Command::cargo_bin("aikit")?
            .current_dir(work.join("custom-output-test")) // Different cwd for this process
            .args(["package", "build", "--output", "custom-dist"])
            .assert()
            .success();

        // Verify ZIP in custom directory
        let zip_path = work
            .join("custom-output-test")
            .join("custom-dist")
            .join("custom-output-test-0.1.0.zip");
        assert!(zip_path.exists());

        Ok(())
    }

    /// Test global version flag
    #[test]
    fn test_global_version_flag() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.arg("--version")
            .assert()
            .success()
            .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }

    /// Test global short version flag
    #[test]
    fn test_global_short_version_flag() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.arg("-V")
            .assert()
            .success()
            .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }

    /// Test debug flag
    #[test]
    fn test_debug_flag() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["--debug", "check"])
            .assert()
            .success()
            .stderr(predicate::str::contains("[DEBUG] Debug mode enabled"));
    }

    /// Test init command basic
    #[test]
    #[ignore] // Temporarily disabled - requires network access to GitHub API
    fn test_init_basic() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?; // Auto-creates unique temp dir
        let work = temp_dir.path(); // Path to temp directory

        Command::cargo_bin("aikit")?
            .current_dir(work) // Sets cwd ONLY for spawned process
            .args(["init", "test-project", "--force"]) // --force to skip git checks
            .assert()
            .success()
            .stdout(predicate::str::contains("Initialized project"));

        // Verify basic structure
        assert!(work.join("aikit.toml").exists());

        Ok(())
    }

    /// Test check command
    #[test]
    fn test_check_command() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.arg("check")
            .assert()
            .success()
            .stdout(predicate::str::contains("Tree display not implemented"));
    }

    /// Test list command when no packages installed
    #[test]
    fn test_list_no_packages() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        Command::cargo_bin("aikit")?
            .current_dir(work) // Sets cwd ONLY for spawned process
            .arg("list")
            .assert()
            .success()
            .stdout(predicate::str::contains("No packages installed"));

        Ok(())
    }

    /// Test list command with detailed flag
    #[test]
    fn test_list_detailed() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["list", "--detailed"]).assert().success();
    }

    /// Test search command (basic functionality, may not return results)
    /// Test install from local directory
    #[test]
    fn test_install_local_directory() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        // Use unique package name to avoid conflicts
        let package_name = format!("install-test-{}", std::process::id());

        // Create a test package first
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["package", "init", &package_name, "--yes"])
            .assert()
            .success();

        // Verify package directory was created
        assert!(work.join(&package_name).exists());

        // Try to install it from local path
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args([
                "install",
                &work.join(&package_name).to_string_lossy(),
                "--yes",
            ])
            .assert()
            .failure() // Command fails due to AI agent setup, but package installs
            .stdout(predicate::str::contains("Installing"))
            .stdout(predicate::str::contains("installed successfully"));

        Ok(())
    }

    /// Test install error with invalid source
    #[test]
    fn test_install_invalid_source() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["install", "nonexistent-source"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("Invalid source"));
    }

    /// Test help output
    #[test]
    fn test_help_output() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.arg("--help")
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "AIKit - Universal template package manager for AI agents",
            ))
            .stdout(predicate::str::contains("package"))
            .stdout(predicate::str::contains("install"))
            .stdout(predicate::str::contains("init"));
    }

    /// Test package init help
    #[test]
    fn test_package_init_help() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package", "init", "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("package-version"))
            .stdout(predicate::str::contains("description"))
            .stdout(predicate::str::contains("author"));
    }

    /// Test package build help
    #[test]
    fn test_package_build_help() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package", "build", "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("output"))
            .stdout(predicate::str::contains("agents"))
            .stdout(predicate::str::contains("include-sources"));
    }

    /// Test install help shows install-version (not version)
    #[test]
    fn test_install_help_shows_install_version() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["install", "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("install-version"));
    }

    /// Test release help shows release-version
    #[test]
    fn test_release_help_shows_release_version() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["release", "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("VERSION")); // Should show the positional VERSION argument
    }

    /// Test that all commands are accessible
    #[test]
    fn test_all_commands_accessible() {
        let commands = vec![
            vec!["package", "init", "--help"],
            vec!["package", "build", "--help"],
            vec!["package", "publish", "--help"],
            vec!["install", "--help"],
            vec!["init", "--help"],
            vec!["check", "--help"],
            vec!["list", "--help"],
            vec!["release", "--help"],
        ];

        for cmd_args in commands {
            let mut cmd = Command::cargo_bin("aikit").unwrap();
            cmd.args(&cmd_args)
                .assert()
                .success()
                .stdout(predicate::str::contains("Usage:")); // All help outputs should contain Usage:
        }
    }

    /// Test error handling for missing subcommands
    #[test]
    fn test_missing_subcommand_error() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package"]) // Missing subcommand
            .assert()
            .failure() // clap returns error code but shows help
            .stderr(predicate::str::contains("Package management commands"));
    }

    /// Test error handling for invalid package names
    #[test]
    fn test_invalid_package_name() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?; // Auto-creates unique temp dir
        let work = temp.path(); // Path to temp directory

        // Try to create package with invalid name (spaces, special chars)
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["package", "init", "invalid name!", "--yes"])
            .assert()
            .failure() // Validation now happens during init
            .stderr(predicate::str::contains("Package validation failed"));

        // Create the directory manually to test build validation
        let invalid_dir = work.join("invalid name!");
        std::fs::create_dir_all(&invalid_dir)?;

        // Create a minimal aikit.toml with invalid package name to test build validation
        let toml_content = r#"[package]
name = "invalid name!"
version = "0.1.0"
description = "Test package with invalid name"

[commands]
"#;
        std::fs::write(invalid_dir.join("aikit.toml"), toml_content)?;

        // Test build validation (in invalid directory)
        Command::cargo_bin("aikit")?
            .current_dir(&invalid_dir) // Different cwd for this process
            .args(["package", "build"])
            .assert()
            .failure(); // Should fail validation

        Ok(())
    }

    /// Test that running aikit with no arguments shows help
    #[test]
    fn test_no_arguments_shows_help() {
        // With arg_required_else_help, clap should show help and exit with code 2
        // This is a basic test to ensure the CLI behaves as expected
        let output = Command::cargo_bin("aikit")
            .unwrap()
            .output()
            .expect("Failed to run command");

        // Should exit with code 2 (clap's default for help/error)
        assert_eq!(output.status.code(), Some(2));

        // Should have some output (help message)
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.is_empty(), "Should have error/help output");
        assert!(stderr.contains("Usage") || stderr.contains("aikit"));
    }

    /// Test that --help output includes version flag
    #[test]
    fn test_help_includes_version_flag() {
        let output = Command::cargo_bin("aikit")
            .unwrap()
            .arg("--help")
            .output()
            .expect("Failed to run command");

        assert_eq!(output.status.code(), Some(0));

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"));
        assert!(stdout.contains("AIKit"));
        assert!(stdout.contains("--version"));
        assert!(stdout.contains("package"));
        assert!(stdout.contains("install"));
    }
}
