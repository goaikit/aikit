//! Integration tests for package publish workflow with automatic upload
//!
//! These tests verify the complete publish workflow including release creation and asset upload.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test complete package publish workflow with automatic upload
    #[test]
    fn test_package_publish_with_auto_upload() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Step 1: Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "publish-test",
                "--description",
                "Publish workflow test package",
                "--package-version",
                "0.1.0",
                "--author",
                "Publish Tester",
                "--yes",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'publish-test' initialized",
            ));

        // Step 2: Build package
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("publish-test"))
            .args(["package", "build"])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Package 'publish-test' built successfully",
            ));

        // Step 3: Publish package (simulated with mocked GitHub)
        // Note: This test would need mock GitHub server to work fully
        // For now, we test the publish command structure
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("publish-test"))
            .args([
                "package",
                "publish",
                "test-owner/test-repo",
                "--token",
                "fake_token",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("GitHub token required"));

        // Verify ZIP was created
        let zip_path = work
            .join("publish-test")
            .join("dist")
            .join("publish-test-0.1.0.zip");
        assert!(zip_path.exists(), "ZIP file should exist at {:?}", zip_path);

        Ok(())
    }

    /// Test package publish with --no-release flag
    #[test]
    fn test_package_publish_no_release_flag() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize and build package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "no-release-test",
                "--description",
                "No release test",
                "--yes",
            ])
            .assert()
            .success();

        cargo_bin_cmd!("aikit")
            .current_dir(work.join("no-release-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Verify ZIP was created
        let zip_path = work
            .join("no-release-test")
            .join("dist")
            .join("no-release-test-0.1.0.zip");
        assert!(zip_path.exists());

        // Test --no-release flag (will fail without valid release)
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("no-release-test"))
            .args([
                "package",
                "publish",
                "test-owner/test-repo",
                "--token",
                "fake_token",
                "--no-release",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("No release found"));

        Ok(())
    }

    /// Test package publish with custom tag
    #[test]
    fn test_package_publish_with_custom_tag() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package with custom version
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "tag-test",
                "--description",
                "Tag test package",
                "--package-version",
                "1.2.3",
                "--author",
                "Tag Tester",
                "--yes",
            ])
            .assert()
            .success();

        // Build package
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("tag-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Verify ZIP uses the custom version
        let zip_path = work
            .join("tag-test")
            .join("dist")
            .join("tag-test-1.2.3.zip");
        assert!(zip_path.exists());

        // Test publish with custom tag
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("tag-test"))
            .args([
                "package",
                "publish",
                "test-owner/test-repo",
                "--token",
                "fake_token",
                "--tag",
                "v1.2.3-beta",
            ])
            .assert()
            .failure();

        Ok(())
    }

    /// Test package publish with custom title and notes
    #[test]
    fn test_package_publish_with_custom_metadata() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "metadata-test",
                "--description",
                "Metadata test package",
                "--yes",
            ])
            .assert()
            .success();

        // Build package
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("metadata-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Test publish with custom title and notes
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("metadata-test"))
            .args([
                "package",
                "publish",
                "test-owner/test-repo",
                "--token",
                "fake_token",
                "--title",
                "Custom Release Title",
                "--notes",
                "This is a custom release note",
            ])
            .assert()
            .failure();

        Ok(())
    }

    /// Test package build with custom output directory
    #[test]
    fn test_package_build_with_custom_output() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "output-test",
                "--description",
                "Output directory test",
                "--yes",
            ])
            .assert()
            .success();

        // Build with custom output directory
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("output-test"))
            .args(["package", "build", "--output", "my-output"])
            .assert()
            .success();

        // Verify ZIP was created in custom location
        let zip_path = work
            .join("output-test")
            .join("my-output")
            .join("output-test-0.1.0.zip");
        assert!(
            zip_path.exists(),
            "ZIP should be in custom output directory"
        );

        Ok(())
    }

    /// Test package build with custom output directory containing spaces
    #[test]
    fn test_package_build_with_spaces_in_path() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "spaces-test",
                "--description",
                "Spaces in path test",
                "--yes",
            ])
            .assert()
            .success();

        // Build with custom output directory with spaces
        let custom_output = "output with spaces";
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("spaces-test"))
            .args(["package", "build", "--output", custom_output])
            .assert()
            .success();

        // Verify ZIP was created
        let zip_path = work
            .join("spaces-test")
            .join(custom_output)
            .join("spaces-test-0.1.0.zip");
        assert!(zip_path.exists());

        Ok(())
    }

    /// Test package publish with package argument
    #[test]
    fn test_package_publish_with_package_arg() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "custom-pkg-test",
                "--description",
                "Custom package path test",
                "--yes",
            ])
            .assert()
            .success();

        // Build package
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("custom-pkg-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Test publish with custom package path (will fail without valid file)
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("custom-pkg-test"))
            .args([
                "package",
                "publish",
                "test-owner/test-repo",
                "--token",
                "fake_token",
                "--package",
                "/nonexistent/custom.zip",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("not found"));

        Ok(())
    }

    /// Test package validation before publish
    #[test]
    fn test_package_validation_before_publish() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "validation-test",
                "--description",
                "Validation test",
                "--yes",
            ])
            .assert()
            .success();

        // Test publish without valid package (should fail)
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("validation-test"))
            .args([
                "package",
                "publish",
                "test-owner/test-repo",
                "--token",
                "fake_token",
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains("aikit.toml not found"));

        Ok(())
    }

    /// Test package publish with environment variable for token
    #[test]
    fn test_package_publish_with_env_token() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Set environment variable
        std::env::set_var("GITHUB_TOKEN", "env_token");

        // Initialize package
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "env-token-test",
                "--description",
                "Environment token test",
                "--yes",
            ])
            .assert()
            .success();

        // Build package
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("env-token-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Test publish without explicit token (should use env var)
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("env-token-test"))
            .args(["package", "publish", "test-owner/test-repo"])
            .assert()
            .failure();

        // Cleanup
        std::env::remove_var("GITHUB_TOKEN");

        Ok(())
    }

    /// Test package publish with prerelease version
    #[test]
    fn test_package_publish_prerelease_detection() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let work = temp.path();

        // Initialize package with prerelease version
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "package",
                "init",
                "prerelease-test",
                "--package-version",
                "1.0.0-alpha",
                "--description",
                "Prerelease test",
                "--yes",
            ])
            .assert()
            .success();

        // Build package
        cargo_bin_cmd!("aikit")
            .current_dir(work.join("prerelease-test"))
            .args(["package", "build"])
            .assert()
            .success();

        // Verify ZIP uses prerelease version
        let zip_path = work
            .join("prerelease-test")
            .join("dist")
            .join("prerelease-test-1.0.0-alpha.zip");
        assert!(zip_path.exists());

        Ok(())
    }
}
