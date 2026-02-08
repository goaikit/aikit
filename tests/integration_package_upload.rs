//! Integration tests for package upload functionality
//!
//! These tests verify the integration between package building,
//! release creation, and asset uploading.

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use aikit::cli::commands::package as pkg_cmd;
use aikit::core::git::GitHubClient;
use aikit::models::package::Package;

async fn create_test_package(temp_dir: &Path) -> Package {
    let package = Package::create_template(
        "integration-test-package".to_string(),
        Some("Integration test package".to_string()),
        Some("test@example.com".to_string()),
        Some("1.0.0".to_string()),
    );

    let package_dir = temp_dir.join("test-package");
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");
    package
        .to_toml_file(&package_dir.join("aikit.toml"))
        .expect("Failed to write aikit.toml");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");
    fs::write(
        templates_dir.join("help.md"),
        "# Help\n\nThis is a help file.",
    )
    .expect("Failed to write help.md");

    let dist_dir = package_dir.join("dist");
    fs::create_dir_all(&dist_dir).expect("Failed to create dist directory");

    package
}

fn create_test_zip_file(path: &Path) {
    use std::fs::File;
    use std::io::Write;
    use zip::write::ZipWriter;
    use zip::CompressionMethod;

    let file = File::create(path).expect("Failed to create test file");
    let mut zip = ZipWriter::new(file);

    zip.start_file(
        "aikit.toml",
        zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .expect("Failed to start file in zip");
    zip.write_all(
        b"[package]\nname = \"integration-test-package\"\nversion = \"1.0.0\"\ndescription = \"Integration test package\"\n",
    )
    .expect("Failed to write to zip");

    zip.finish().expect("Failed to finish zip");
}

#[tokio::test]
async fn test_package_publish_with_upload_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let _package = create_test_package(temp_dir.path()).await;

    let zip_path = temp_dir
        .path()
        .join("test-package/dist/integration-test-package-1.0.0.zip");
    create_test_zip_file(&zip_path);

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(temp_dir.path().join("test-package"))
        .expect("Failed to set CWD for test");

    let args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: None,
        title: None,
        notes: None,
        token: Some("test_token".to_string()),
        no_release: false,
    };

    let result = pkg_cmd::execute_publish(args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    assert!(result.is_err());
    insta::assert_snapshot!(
        "integration_publish_with_upload",
        result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_package_publish_no_release_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let _package = create_test_package(temp_dir.path()).await;

    let zip_path = temp_dir
        .path()
        .join("test-package/dist/integration-test-package-1.0.0.zip");
    create_test_zip_file(&zip_path);

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(temp_dir.path().join("test-package"))
        .expect("Failed to set CWD for test");

    let args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: Some("v1.0.0".to_string()),
        title: None,
        notes: None,
        token: Some("test_token".to_string()),
        no_release: true,
    };

    let result = pkg_cmd::execute_publish(args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    assert!(result.is_err());
    insta::assert_snapshot!(
        "integration_publish_no_release",
        result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_package_upload_asset_without_token_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let test_file = temp_dir.path().join("test-upload.zip");
    create_test_zip_file(&test_file);

    let client = GitHubClient::new(None);

    let result = client
        .upload_release_asset("test-owner", "test-repo", 123, &test_file)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("token"));
}

#[tokio::test]
async fn test_package_upload_file_not_found_integration() {
    let client = GitHubClient::new(Some("test_token".to_string()));
    let nonexistent_file = PathBuf::from("/nonexistent/path/file.zip");

    let result = client
        .upload_release_asset("test-owner", "test-repo", 123, &nonexistent_file)
        .await;

    assert!(result.is_err());
    insta::assert_snapshot!(
        "integration_upload_file_not_found",
        result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_package_build_and_publish_workflow_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let _package = create_test_package(temp_dir.path()).await;

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(temp_dir.path().join("test-package"))
        .expect("Failed to set CWD for test");

    let build_args = pkg_cmd::PackageBuildArgs {
        output: "dist".to_string(),
        agents: None,
        include_sources: false,
    };

    let build_result = pkg_cmd::execute_build(build_args).await;

    assert!(build_result.is_ok());

    let publish_args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: None,
        title: None,
        notes: None,
        token: Some("test_token".to_string()),
        no_release: false,
    };

    let publish_result = pkg_cmd::execute_publish(publish_args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    insta::assert_snapshot!(
        "integration_build_and_publish",
        publish_result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_package_publish_with_custom_release_notes_integration() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let _package = create_test_package(temp_dir.path()).await;

    let zip_path = temp_dir
        .path()
        .join("test-package/dist/integration-test-package-1.0.0.zip");
    create_test_zip_file(&zip_path);

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(temp_dir.path().join("test-package"))
        .expect("Failed to set CWD for test");

    let args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: Some("v1.0.0".to_string()),
        title: Some("Custom Release Title".to_string()),
        notes: Some("Custom release notes for testing".to_string()),
        token: Some("test_token".to_string()),
        no_release: false,
    };

    let result = pkg_cmd::execute_publish(args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    assert!(result.is_err());
    insta::assert_snapshot!(
        "integration_publish_custom_notes",
        result.unwrap_err().to_string()
    );
}
