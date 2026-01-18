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
    fn test_complete_package_workflow() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Step 1: Initialize package
        Command::cargo_bin("aikit")?
            .current_dir(work)
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
        assert!(work.join("workflow-test").exists());
        assert!(work.join("workflow-test").join("aikit.toml").exists());

        // Step 2: Build package in the package directory
        Command::cargo_bin("aikit")?
            .current_dir(work.join("workflow-test"))
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'workflow-test' built successfully",
            ));

        // Verify ZIP was created
        let zip_path = work
            .join("workflow-test")
            .join("dist/workflow-test-1.0.0.zip");
        assert!(zip_path.exists(), "ZIP file should exist at {:?}", zip_path);

        // Step 3: Install the package from the parent directory
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["install", "./workflow-test", "--yes", "--ai", "claude"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Installing"));

        // Step 4: Verify package appears in list
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["list", "--detailed"])
            .assert()
            .success()
            .stdout(predicate::str::contains("workflow-test"))
            .stdout(predicate::str::contains("1.0.0"))
            .stdout(predicate::str::contains("Workflow Tester"));

        Ok(())
    }

    /// Test package installation workflow from local directory
    #[test]
    fn test_package_installation_workflow() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Create a package to install
        Command::cargo_bin("aikit")?
            .current_dir(work)
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
        Command::cargo_bin("aikit")?
            .current_dir(work.join("install-workflow-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Install the package
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args([
                "install",
                "./install-workflow-test",
                "--yes",
                "--ai",
                "claude",
            ])
            .assert()
            .success();

        // Verify installation by checking list output
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["list", "--detailed"])
            .assert()
            .success()
            .stdout(predicate::str::contains("install-workflow-test"))
            .stdout(predicate::str::contains("0.1.0")); // Default version

        // Verify .aikit directory was created
        let aikit_dir = work.join(".aikit");
        assert!(aikit_dir.exists(), ".aikit directory should exist");
        assert!(
            aikit_dir.join("packages").exists(),
            "packages directory should exist"
        );
        assert!(
            aikit_dir.join("registry.toml").exists(),
            "registry should exist"
        );

        Ok(())
    }

    /// Test package update workflow (simulated)
    #[test]
    fn test_package_update_workflow() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Step 1: Create and install initial package v1.0.0
        Command::cargo_bin("aikit")?
            .current_dir(work)
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

        Command::cargo_bin("aikit")?
            .current_dir(work.join("update-test"))
            .args(["package", "build"])
            .assert()
            .success();

        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["install", "./update-test", "--yes", "--ai", "claude"])
            .assert()
            .success();

        // Verify initial installation
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["list"])
            .assert()
            .success()
            .stdout(predicate::str::contains("update-test"));

        // Step 2: Create updated package v2.0.0
        Command::cargo_bin("aikit")?
            .current_dir(work)
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

        Command::cargo_bin("aikit")?
            .current_dir(work.join("update-test-v2"))
            .args(["package", "build"])
            .assert()
            .success();

        // Note: In a real scenario, we'd test `aikit update update-test`
        // But since we don't have a registry for this test, we just verify
        // that the package structure is correct for updates
        let update_zip = work
            .join("update-test-v2")
            .join("dist")
            .join("update-test-v2-2.0.0.zip");
        assert!(update_zip.exists());

        Ok(())
    }

    /// Test error recovery workflow
    #[test]
    fn test_error_recovery_workflow() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Try to build package without aikit.toml (should fail)
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["package", "build"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("aikit.toml not found"));

        // Now create a valid package and try again
        Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["package", "init", "recovery-test", "--yes"])
            .assert()
            .success();

        // Now build should succeed
        Command::cargo_bin("aikit")?
            .current_dir(work.join("recovery-test"))
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains("built successfully"));

        Ok(())
    }

    /// Test multiple package workflow
    #[test]
    fn test_multiple_package_workflow() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Create multiple packages
        let package_names = vec!["package-a", "package-b", "package-c"];

        for package_name in &package_names {
            Command::cargo_bin("aikit")?
                .current_dir(work)
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
            Command::cargo_bin("aikit")?
                .current_dir(work.join(package_name))
                .args(["package", "build"])
                .assert()
                .success();

            // Install each package
            Command::cargo_bin("aikit")?
                .current_dir(work)
                .args([
                    "install",
                    &format!("./{}", package_name),
                    "--yes",
                    "--ai",
                    "claude",
                ])
                .assert()
                .success();
        }

        // Verify all packages are listed
        let output = Command::cargo_bin("aikit")?
            .current_dir(work)
            .args(["list", "--detailed"])
            .output()?;
        assert!(output.status.success());

        let stdout = String::from_utf8_lossy(&output.stdout);
        for package_name in &package_names {
            assert!(
                stdout.contains(package_name),
                "Package {} should be in list output",
                package_name
            );
        }

        Ok(())
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
            vec!["release", "--help"],
        ];

        for help_cmd in help_commands {
            let mut cmd = Command::cargo_bin("aikit").unwrap();
            cmd.args(&help_cmd)
                .assert()
                .success()
                .stdout(predicate::str::contains("Usage:"));
        }
    }
}
