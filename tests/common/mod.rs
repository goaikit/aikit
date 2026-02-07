//! Common test utilities and helpers
//!
//! This module provides shared utilities for all test types to reduce duplication
//! and ensure consistent test setup and teardown.

use assert_cmd::cargo::cargo_bin_cmd;
use mockito::Server;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Test environment setup utility
pub struct TestEnv {
    temp_dir: TempDir,
    original_dir: PathBuf,
}

impl TestEnv {
    /// Create a new test environment in a temporary directory
    pub fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();

        // Change to temp directory
        std::env::set_current_dir(temp_dir.path()).unwrap();

        Self {
            temp_dir,
            original_dir,
        }
    }

    /// Get the path to the temporary directory
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Create a test package in the current directory
    pub fn create_test_package(&self, name: &str) -> PathBuf {
        let package_path = self.path().join(name);

        let mut cmd = cargo_bin_cmd!("aikit");
        cmd.args([
            "package", "init", name,
            "--description", &format!("Test package: {}", name),
            "--yes"
        ])
        .assert()
        .success();

        assert!(package_path.exists());
        package_path
    }

    /// Build a package in the specified directory
    pub fn build_package(&self, package_path: &Path) {
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(package_path).unwrap();

        let mut cmd = cargo_bin_cmd!("aikit");
        cmd.args(["package", "build"])
            .assert()
            .success();

        std::env::set_current_dir(original_dir).unwrap();
    }

    /// Install a package from the specified path
    pub fn install_package(&self, package_path: &Path) {
        let mut cmd = cargo_bin_cmd!("aikit");
        cmd.args([
            "install",
            &package_path.to_string_lossy(),
            "--yes"
        ])
        .assert()
        .success();
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Restore original directory when TestEnv goes out of scope
        let _ = std::env::set_current_dir(&self.original_dir);
    }
}

/// Create a basic test package with minimal structure
pub fn create_minimal_test_package(dir: &Path, name: &str) -> PathBuf {
    let package_path = dir.join(name);

    let mut cmd = cargo_bin_cmd!("aikit");
    cmd.args([
        "package", "init", name,
        "--description", "Minimal test package",
        "--yes"
    ])
    .assert()
    .success();

    package_path
}

/// Create a test package with custom version and author
pub fn create_custom_test_package(dir: &Path, name: &str, version: &str, author: &str) -> PathBuf {
    let package_path = dir.join(name);

    let mut cmd = cargo_bin_cmd!("aikit");
    cmd.args([
        "package", "init", name,
        "--description", &format!("Custom test package: {}", name),
        "--package-version", version,
        "--author", author,
        "--yes"
    ])
    .assert()
    .success();

    package_path
}

/// Build a package and return the path to the built ZIP
pub fn build_test_package(package_path: &Path) -> PathBuf {
    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(package_path).unwrap();

    let mut cmd = cargo_bin_cmd!("aikit");
    cmd.args(["package", "build"])
        .assert()
        .success();

    std::env::set_current_dir(original_dir).unwrap();

    // Return path to built ZIP
    let package_name = package_path.file_name().unwrap().to_string_lossy();
    let toml_path = package_path.join("aikit.toml");
    let toml_content = fs::read_to_string(toml_path).unwrap();
    let version_line = toml_content.lines()
        .find(|line| line.starts_with("version = "))
        .unwrap();
    let version = version_line
        .split('"')
        .nth(1)
        .unwrap();

    package_path.join("dist").join(format!("{}-{}.zip", package_name, version))
}

/// Setup mock GitHub server for testing GitHub API calls
pub async fn setup_github_mock() -> Server {
    let mut server = mockito::Server::new_async().await;

    // Mock GitHub API endpoints that the CLI uses
    // These can be extended as needed for specific tests

    // Mock repository manifest endpoint
    server.mock("GET", "/repos/test-owner/test-repo/contents/aikit.toml")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{
            "name": "aikit.toml",
            "path": "aikit.toml",
            "sha": "abc123",
            "size": 1024,
            "content": "W3BhY2thZ2VdCm5hbWUgPSAidGVzdC1yZXBvIgp2ZXJzaW9uID0gIjEuMC4wIgpkZXNjcmlwdGlvbiA9ICJUZXN0IHBhY2thZ2UiCg=="
        }"#)
        .create();

    // Mock archive download endpoint
    server.mock("GET", "/repos/test-owner/test-repo/zipball/v1.0.0")
        .with_status(200)
        .with_header("content-type", "application/zip")
        .with_body("fake zip content")
        .create();

    server
}

/// Setup mock server for search API
pub async fn setup_search_mock() -> Server {
    let mut server = mockito::Server::new_async().await;

    // Mock search endpoint
    server.mock("GET", "/search/code")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{
            "total_count": 1,
            "items": [{
                "name": "test-repo",
                "full_name": "test-owner/test-repo",
                "html_url": "https://github.com/test-owner/test-repo",
                "description": "Test repository for search"
            }]
        }"#)
        .create();

    server
}

/// Clean up test artifacts and restore environment
pub fn cleanup_test_artifacts() {
    // Remove any .aikit directories that might have been created
    if Path::new(".aikit").exists() {
        let _ = fs::remove_dir_all(".aikit");
    }

    // Clean up any dist directories
    if Path::new("dist").exists() {
        let _ = fs::remove_dir_all("dist");
    }
}

/// Assert that a directory contains the expected package structure
pub fn assert_package_structure(package_path: &Path, package_name: &str) {
    assert!(package_path.exists(), "Package directory should exist");

    let expected_files = vec![
        "aikit.toml",
        "README.md",
    ];

    let expected_dirs = vec![
        "templates",
        "scripts",
        "docs",
    ];

    for file in expected_files {
        assert!(package_path.join(file).exists(),
               "Package should contain {} file", file);
    }

    for dir in expected_dirs {
        assert!(package_path.join(dir).exists(),
               "Package should contain {} directory", dir);
        assert!(fs::read_dir(package_path.join(dir)).unwrap().next().is_some(),
               "{} directory should not be empty", dir);
    }

    // Verify aikit.toml contains correct name
    let toml_content = fs::read_to_string(package_path.join("aikit.toml")).unwrap();
    assert!(toml_content.contains(&format!("name = \"{}\"", package_name)));
}

/// Assert that a ZIP file exists and is valid
pub fn assert_zip_exists(zip_path: &Path) {
    assert!(zip_path.exists(), "ZIP file should exist at {:?}", zip_path);

    // Basic validation that it's a ZIP file by checking file size > 0
    let metadata = fs::metadata(zip_path).unwrap();
    assert!(metadata.len() > 0, "ZIP file should not be empty");
}

/// Helper to run a command and get its output as string
pub fn run_command(args: &[&str]) -> (String, String) {
    let mut cmd = cargo_bin_cmd!("aikit");
    let output = cmd.args(args)
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    (stdout, stderr)
}

/// Helper to check if a package is installed
pub fn is_package_installed(package_name: &str) -> bool {
    let (stdout, _) = run_command(&["list", "--detailed"]);
    stdout.contains(package_name)
}

/// Get the version of an installed package
pub fn get_installed_package_version(package_name: &str) -> Option<String> {
    let (stdout, _) = run_command(&["list", "--detailed"]);

    for line in stdout.lines() {
        if line.contains(package_name) {
            // Extract version from line like "  package-name v1.2.3"
            if let Some(version_part) = line.split('v').nth(1) {
                return Some(version_part.split_whitespace().next().unwrap_or("").to_string());
            }
        }
    }

    None
}