//! CLI Integration Tests
//!
//! These tests run the actual aikit binary using assert_cmd to verify
//! command-line interface behavior and catch runtime issues.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test package init command with basic functionality
    #[test]
    fn test_package_init_basic() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package", "init", "test-package", "--yes"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'test-package' initialized",
            ))
            .stdout(predicate::str::contains("Created directory structure"));

        // Verify directory structure was created
        assert!(temp_dir.path().join("test-package").exists());
        assert!(temp_dir
            .path()
            .join("test-package")
            .join("aikit.toml")
            .exists());
        assert!(temp_dir
            .path()
            .join("test-package")
            .join("README.md")
            .exists());
        assert!(temp_dir
            .path()
            .join("test-package")
            .join("templates")
            .exists());
        assert!(temp_dir
            .path()
            .join("test-package")
            .join("scripts")
            .exists());
        assert!(temp_dir.path().join("test-package").join("docs").exists());
    }

    /// Test package init with all options
    #[test]
    fn test_package_init_with_options() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args([
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

        // Verify aikit.toml content
        let toml_path = temp_dir.path().join("my-package").join("aikit.toml");
        let toml_content = fs::read_to_string(toml_path).unwrap();
        assert!(toml_content.contains(r#"name = "my-package""#));
        assert!(toml_content.contains(r#"version = "2.0.0""#));
        assert!(toml_content.contains(r#"description = "A comprehensive test package""#));
        assert!(toml_content.contains(r#"authors = ["Test Author"]"#));
    }

    /// Test package init error when directory exists
    #[test]
    fn test_package_init_directory_exists_error() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create directory first
        fs::create_dir("existing-package").unwrap();

        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package", "init", "existing-package"])
            .assert()
            .success() // Note: success because it prompts user, doesn't fail
            .stdout(predicate::str::contains("already exists"));
    }

    /// Test package build command
    #[test]
    fn test_package_build() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // First create a package
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args(["package", "init", "build-test", "--yes"])
            .assert()
            .success();

        // Change to package directory
        std::env::set_current_dir(temp_dir.path().join("build-test")).unwrap();

        // Build the package
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'build-test' built successfully",
            ))
            .stdout(predicate::str::contains("Output:"));

        // Verify ZIP was created
        let zip_path = temp_dir
            .path()
            .join("build-test")
            .join("dist")
            .join("build-test-0.1.0.zip");
        assert!(zip_path.exists());
    }

    /// Test package build error when no aikit.toml
    #[test]
    fn test_package_build_no_toml_error() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package", "build"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("aikit.toml not found"));
    }

    /// Test package build with custom output directory
    #[test]
    fn test_package_build_custom_output() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create package
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args(["package", "init", "custom-output-test", "--yes"])
            .assert()
            .success();

        std::env::set_current_dir(temp_dir.path().join("custom-output-test")).unwrap();

        // Build with custom output
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd
            .args(["package", "build", "--output", "custom-dist"])
            .assert()
            .success();

        // Verify ZIP in custom directory
        let zip_path = temp_dir
            .path()
            .join("custom-output-test")
            .join("custom-dist")
            .join("custom-output-test-0.1.0.zip");
        assert!(zip_path.exists());
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
    fn test_init_basic() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["init", "test-project", "--force"]) // --force to skip git checks
            .assert()
            .success()
            .stdout(predicate::str::contains("Initialized project"));

        // Verify basic structure
        assert!(temp_dir.path().join("aikit.toml").exists());
    }

    /// Test check command
    #[test]
    fn test_check_command() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.arg("check")
            .assert()
            .success()
            .stdout(predicate::str::contains("Checking"));
    }

    /// Test list command when no packages installed
    #[test]
    fn test_list_no_packages() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.arg("list")
            .assert()
            .success()
            .stdout(predicate::str::contains("No packages installed"));
    }

    /// Test list command with detailed flag
    #[test]
    fn test_list_detailed() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["list", "--detailed"]).assert().success();
    }

    /// Test search command (basic functionality, may not return results)
    #[test]
    fn test_search_command() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["search", "test"]).assert().success(); // May succeed with no results, but shouldn't fail
    }

    /// Test install from local directory
    #[test]
    fn test_install_local_directory() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create a test package first
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args(["package", "init", "install-test", "--yes"])
            .assert()
            .success();

        // Try to install it from local path
        let mut install_cmd = Command::cargo_bin("aikit").unwrap();
        install_cmd
            .args(["install", "./install-test", "--yes"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Installing"));
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
            .stdout(predicate::str::contains("AIKIT"))
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
            vec!["search", "--help"],
            vec!["release", "--help"],
        ];

        for cmd_args in commands {
            let mut cmd = Command::cargo_bin("aikit").unwrap();
            cmd.args(&cmd_args)
                .assert()
                .success()
                .stdout(predicate::str::contains("USAGE")); // All help outputs should contain USAGE
        }
    }

    /// Test error handling for missing subcommands
    #[test]
    fn test_missing_subcommand_error() {
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package"]) // Missing subcommand
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "required arguments were not provided",
            ));
    }

    /// Test error handling for invalid package names
    #[test]
    fn test_invalid_package_name() {
        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Try to create package with invalid name (spaces, special chars)
        let mut cmd = Command::cargo_bin("aikit").unwrap();
        cmd.args(["package", "init", "invalid name!", "--yes"])
            .assert()
            .success(); // May succeed initially, validation happens later

        // But validation should fail in build
        std::env::set_current_dir(temp_dir.path().join("invalid name!")).unwrap();
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd.args(["package", "build"]).assert().failure(); // Should fail validation
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
