//! End-to-End Workflow Tests
//!
//! These tests verify complete workflows from start to finish,
//! ensuring all components work together correctly.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test complete package creation workflow: init -> build -> install -> list
    #[test]
    fn test_complete_package_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        // Change to temp directory for test
        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Step 1: Initialize package
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args([
                "package",
                "init",
                "workflow-test",
                "--description",
                "End-to-end workflow test package",
                "--package-version",
                "1.0.0",
                "--author",
                "Workflow Tester",
                "--yes",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'workflow-test' initialized",
            ));

        // Verify package structure was created
        assert!(temp_dir.path().join("workflow-test").exists());
        assert!(temp_dir
            .path()
            .join("workflow-test")
            .join("aikit.toml")
            .exists());

        // Step 2: Change to package directory and build
        std::env::set_current_dir(temp_dir.path().join("workflow-test")).unwrap();

        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'workflow-test' built successfully",
            ));

        // Verify ZIP was created
        let zip_path = Path::new("dist/workflow-test-1.0.0.zip");
        assert!(zip_path.exists(), "ZIP file should exist at {:?}", zip_path);

        // Step 3: Go back to parent directory and install the package
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let mut install_cmd = Command::cargo_bin("aikit").unwrap();
        install_cmd
            .args(["install", "./workflow-test", "--yes"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Installing"));

        // Step 4: Verify package appears in list
        let mut list_cmd = Command::cargo_bin("aikit").unwrap();
        list_cmd
            .args(["list", "--detailed"])
            .assert()
            .success()
            .stdout(predicate::str::contains("workflow-test"))
            .stdout(predicate::str::contains("1.0.0"))
            .stdout(predicate::str::contains("Workflow Tester"));

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test package installation workflow from local directory
    #[test]
    fn test_package_installation_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create a package to install
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args([
                "package",
                "init",
                "install-workflow-test",
                "--description",
                "Installation workflow test",
                "--yes",
            ])
            .assert()
            .success();

        // Build the package first
        std::env::set_current_dir(temp_dir.path().join("install-workflow-test")).unwrap();
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd.args(["package", "build"]).assert().success();

        // Go back and install
        std::env::set_current_dir(temp_dir.path()).unwrap();
        let mut install_cmd = Command::cargo_bin("aikit").unwrap();
        install_cmd
            .args(["install", "./install-workflow-test", "--yes"])
            .assert()
            .success();

        // Verify installation by checking list output
        let mut list_cmd = Command::cargo_bin("aikit").unwrap();
        list_cmd
            .args(["list", "--detailed"])
            .assert()
            .success()
            .stdout(predicate::str::contains("install-workflow-test"))
            .stdout(predicate::str::contains("0.1.0")); // Default version

        // Verify .aikit directory was created
        let aikit_dir = temp_dir.path().join(".aikit");
        assert!(aikit_dir.exists(), ".aikit directory should exist");
        assert!(
            aikit_dir.join("packages").exists(),
            "packages directory should exist"
        );
        assert!(
            aikit_dir.join("registry.json").exists(),
            "registry should exist"
        );

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test project initialization workflow
    #[test]
    fn test_project_initialization_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Initialize a new project
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args(["init", "test-project", "--force"]) // --force skips git checks
            .assert()
            .success()
            .stdout(predicate::str::contains("Initialized project"));

        // Verify project structure
        assert!(temp_dir.path().join("aikit.toml").exists());
        assert!(temp_dir.path().join(".aikit").exists());
        assert!(temp_dir.path().join(".aikit").join("config.toml").exists());

        // Verify the project can be used for package operations
        let mut package_init_cmd = Command::cargo_bin("aikit").unwrap();
        package_init_cmd
            .args(["package", "init", "project-package", "--yes"])
            .assert()
            .success();

        // Build the package
        std::env::set_current_dir(temp_dir.path().join("project-package")).unwrap();
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd.args(["package", "build"]).assert().success();

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test package update workflow (simulated)
    #[test]
    fn test_package_update_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Step 1: Create and install initial package v1.0.0
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args([
                "package",
                "init",
                "update-test",
                "--package-version",
                "1.0.0",
                "--yes",
            ])
            .assert()
            .success();

        std::env::set_current_dir(temp_dir.path().join("update-test")).unwrap();
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd.args(["package", "build"]).assert().success();

        std::env::set_current_dir(temp_dir.path()).unwrap();
        let mut install_cmd = Command::cargo_bin("aikit").unwrap();
        install_cmd
            .args(["install", "./update-test", "--yes"])
            .assert()
            .success();

        // Verify initial installation
        let mut list_cmd = Command::cargo_bin("aikit").unwrap();
        list_cmd
            .args(["list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("update-test"));

        // Step 2: Create updated package v2.0.0
        let mut update_init_cmd = Command::cargo_bin("aikit").unwrap();
        update_init_cmd
            .args([
                "package",
                "init",
                "update-test-v2",
                "--package-version",
                "2.0.0",
                "--yes",
            ])
            .assert()
            .success();

        std::env::set_current_dir(temp_dir.path().join("update-test-v2")).unwrap();
        let mut update_build_cmd = Command::cargo_bin("aikit").unwrap();
        update_build_cmd
            .args(["package", "build"])
            .assert()
            .success();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Note: In a real scenario, we'd test `aikit update update-test`
        // But since we don't have a registry for this test, we just verify
        // that the package structure is correct for updates
        let update_zip = temp_dir
            .path()
            .join("update-test-v2")
            .join("dist")
            .join("update-test-v2-2.0.0.zip");
        assert!(update_zip.exists());

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test error recovery workflow
    #[test]
    fn test_error_recovery_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Try to build package without aikit.toml (should fail)
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd
            .args(["package", "build"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("aikit.toml not found"));

        // Now create a valid package and try again
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args(["package", "init", "recovery-test", "--yes"])
            .assert()
            .success();

        std::env::set_current_dir(temp_dir.path().join("recovery-test")).unwrap();

        // Now build should succeed
        let mut build_cmd = Command::cargo_bin("aikit").unwrap();
        build_cmd
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains("built successfully"));

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test multiple package workflow
    #[test]
    fn test_multiple_package_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Create multiple packages
        let package_names = vec!["package-a", "package-b", "package-c"];

        for package_name in &package_names {
            let mut init_cmd = Command::cargo_bin("aikit").unwrap();
            init_cmd
                .args([
                    "package",
                    "init",
                    package_name,
                    "--description",
                    &format!("Test package {}", package_name),
                    "--yes",
                ])
                .assert()
                .success();

            // Build each package
            std::env::set_current_dir(temp_dir.path().join(package_name)).unwrap();
            let mut build_cmd = Command::cargo_bin("aikit").unwrap();
            build_cmd.args(["package", "build"]).assert().success();

            std::env::set_current_dir(temp_dir.path()).unwrap();

            // Install each package
            let mut install_cmd = Command::cargo_bin("aikit").unwrap();
            install_cmd
                .args(["install", &format!("./{}", package_name), "--yes"])
                .assert()
                .success();
        }

        // Verify all packages are listed
        let mut list_cmd = Command::cargo_bin("aikit").unwrap();
        let output = list_cmd.args(["list", "--detailed"]).output().unwrap();
        assert!(output.status.success());

        let stdout = String::from_utf8_lossy(&output.stdout);
        for package_name in &package_names {
            assert!(
                stdout.contains(package_name),
                "Package {} should be in list output",
                package_name
            );
        }

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test configuration persistence workflow
    #[test]
    fn test_configuration_workflow() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        std::env::set_current_dir(temp_dir.path()).unwrap();

        // Initialize project (creates .aikit/config.toml)
        let mut init_cmd = Command::cargo_bin("aikit").unwrap();
        init_cmd
            .args(["init", "config-test", "--force"])
            .assert()
            .success();

        // Verify config file exists
        let config_path = temp_dir.path().join(".aikit").join("config.toml");
        assert!(config_path.exists());

        // Read config content
        let config_content = fs::read_to_string(&config_path).unwrap();
        assert!(config_content.contains("[agent]"));
        assert!(config_content.contains("[registry]"));

        // Verify that subsequent operations use this config
        let mut check_cmd = Command::cargo_bin("aikit").unwrap();
        check_cmd.arg("check").assert().success();

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Test help system workflow
    #[test]
    fn test_help_system_workflow() {
        // Test that help is accessible for all major commands
        let help_commands = vec![
            vec!["--help"],
            vec!["package", "--help"],
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

        for help_cmd in help_commands {
            let mut cmd = Command::cargo_bin("aikit").unwrap();
            cmd.args(&help_cmd)
                .assert()
                .success()
                .stdout(predicate::str::contains("USAGE"));
        }
    }
}
