//! End-to-end tests for package publish workflow
//!
//! These tests verify the complete workflow from package initialization
//! through building and publishing with asset upload.

use std::fs;
use tempfile::TempDir;

use aikit::cli::commands::package as pkg_cmd;
use aikit::models::package::Package;

#[tokio::test]
async fn test_complete_publish_workflow_e2e() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let package_name = "e2e-test-package";
    let package_version = "1.0.0";

    let package_dir = temp_dir.path().join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");

    let scripts_dir = package_dir.join("scripts");
    fs::create_dir_all(&scripts_dir).expect("Failed to create scripts directory");

    let docs_dir = package_dir.join("docs");
    fs::create_dir_all(&docs_dir).expect("Failed to create docs directory");

    let manifest_content = format!(
        r#"[package]
name = "{}"
version = "{}"
description = "End-to-end test package"
authors = ["test@example.com"]

[commands]
help = {{ description = "Show help information" }}
"#,
        package_name, package_version
    );
    fs::write(package_dir.join("aikit.toml"), manifest_content)
        .expect("Failed to write aikit.toml");

    fs::write(
        templates_dir.join("help.md"),
        "# Help\n\nThis is a help file for the E2E test package.",
    )
    .expect("Failed to write help.md");

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

    let build_args = pkg_cmd::PackageBuildArgs {
        output: "dist".to_string(),
        agents: None,
        include_sources: false,
    };

    let build_result = pkg_cmd::execute_build(build_args).await;

    assert!(
        build_result.is_ok(),
        "Build failed: {:?}",
        build_result.err()
    );

    let dist_dir = package_dir.join("dist");
    assert!(dist_dir.exists(), "Dist directory should exist after build");

    let expected_zip = dist_dir.join(format!("{}-{}.zip", package_name, package_version));
    assert!(
        expected_zip.exists(),
        "Package ZIP should exist after build: {:?}",
        expected_zip
    );

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
        "e2e_complete_publish_workflow",
        publish_result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_e2e_publish_with_no_release_flag() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let package_name = "e2e-test-package-no-release";
    let package_version = "1.0.0";

    let package_dir = temp_dir.path().join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");

    let manifest_content = format!(
        r#"[package]
name = "{}"
version = "{}"
description = "E2E test package for no-release flag"
authors = ["test@example.com"]
"#,
        package_name, package_version
    );
    fs::write(package_dir.join("aikit.toml"), manifest_content)
        .expect("Failed to write aikit.toml");

    fs::write(templates_dir.join("help.md"), "# Help\n\nE2E test help.")
        .expect("Failed to write help.md");

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

    let build_args = pkg_cmd::PackageBuildArgs {
        output: "dist".to_string(),
        agents: None,
        include_sources: false,
    };

    let build_result = pkg_cmd::execute_build(build_args).await;

    assert!(
        build_result.is_ok(),
        "Build failed: {:?}",
        build_result.err()
    );

    let publish_args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: Some("v1.0.0".to_string()),
        title: None,
        notes: None,
        token: Some("test_token".to_string()),
        no_release: true,
    };

    let publish_result = pkg_cmd::execute_publish(publish_args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    insta::assert_snapshot!(
        "e2e_publish_no_release",
        publish_result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_e2e_publish_with_custom_package_path() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let package_name = "e2e-test-custom-path";
    let package_version = "1.0.0";

    let package_dir = temp_dir.path().join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");

    let manifest_content = format!(
        r#"[package]
name = "{}"
version = "{}"
description = "E2E test package for custom path"
authors = ["test@example.com"]
"#,
        package_name, package_version
    );
    fs::write(package_dir.join("aikit.toml"), &manifest_content)
        .expect("Failed to write aikit.toml");

    fs::write(templates_dir.join("help.md"), "# Help\n\nE2E test help.")
        .expect("Failed to write help.md");

    let custom_zip_path = temp_dir.path().join("custom-package.zip");

    use std::fs::File;
    use std::io::Write;
    use zip::write::ZipWriter;
    use zip::CompressionMethod;

    let file = File::create(&custom_zip_path).expect("Failed to create custom ZIP");
    let mut zip = ZipWriter::new(file);

    zip.start_file(
        "aikit.toml",
        zip::write::FileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .expect("Failed to start file in zip");
    zip.write_all(manifest_content.as_bytes())
        .expect("Failed to write to zip");

    zip.finish().expect("Failed to finish zip");

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

    let publish_args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: Some(custom_zip_path.to_string_lossy().to_string()),
        tag: Some("v1.0.0".to_string()),
        title: Some("Custom Path Release".to_string()),
        notes: Some("Released with custom package path".to_string()),
        token: Some("test_token".to_string()),
        no_release: false,
    };

    let publish_result = pkg_cmd::execute_publish(publish_args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    insta::assert_snapshot!(
        "e2e_publish_custom_path",
        publish_result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_e2e_publish_with_env_token() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let package_name = "e2e-test-env-token";
    let package_version = "1.0.0";

    let package_dir = temp_dir.path().join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");

    let manifest_content = format!(
        r#"[package]
name = "{}"
version = "{}"
description = "E2E test package for env token"
authors = ["test@example.com"]
"#,
        package_name, package_version
    );
    fs::write(package_dir.join("aikit.toml"), manifest_content)
        .expect("Failed to write aikit.toml");

    fs::write(templates_dir.join("help.md"), "# Help\n\nE2E test help.")
        .expect("Failed to write help.md");

    let build_args = pkg_cmd::PackageBuildArgs {
        output: "dist".to_string(),
        agents: None,
        include_sources: false,
    };

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

    let build_result = pkg_cmd::execute_build(build_args).await;
    assert!(
        build_result.is_ok(),
        "Build failed: {:?}",
        build_result.err()
    );

    std::env::set_var("GITHUB_TOKEN", "env_test_token_12345");

    let publish_args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: None,
        title: None,
        notes: None,
        token: None,
        no_release: false,
    };

    let publish_result = pkg_cmd::execute_publish(publish_args).await;

    std::env::remove_var("GITHUB_TOKEN");
    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    insta::assert_snapshot!(
        "e2e_publish_env_token",
        publish_result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_e2e_validate_version_number_availability() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let package = Package::create_template(
        "version-test-package".to_string(),
        Some("Test version number availability".to_string()),
        Some("test@example.com".to_string()),
        Some("2.3.4".to_string()),
    );

    let package_dir = temp_dir.path().join("version-test-package");
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");

    package
        .to_toml_file(&package_dir.join("aikit.toml"))
        .expect("Failed to write aikit.toml");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");
    fs::write(templates_dir.join("help.md"), "# Help\n").expect("Failed to write help.md");

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

    let build_args = pkg_cmd::PackageBuildArgs {
        output: "dist".to_string(),
        agents: None,
        include_sources: false,
    };

    let build_result = pkg_cmd::execute_build(build_args).await;

    assert!(
        build_result.is_ok(),
        "Build failed: {:?}",
        build_result.err()
    );

    let dist_dir = package_dir.join("dist");
    let expected_zip = dist_dir.join("version-test-package-2.3.4.zip");

    assert!(
        expected_zip.exists(),
        "Package ZIP should exist with correct version: {:?}",
        expected_zip
    );

    let publish_args = pkg_cmd::PackagePublishArgs {
        repo: "test-owner/test-repo".to_string(),
        package: None,
        tag: Some("v2.3.4".to_string()),
        title: Some("Release 2.3.4".to_string()),
        notes: Some("Release notes for version 2.3.4".to_string()),
        token: Some("test_token".to_string()),
        no_release: false,
    };

    let publish_result = pkg_cmd::execute_publish(publish_args).await;

    std::env::set_current_dir(orig_cwd).expect("Failed to restore original CWD");

    insta::assert_snapshot!(
        "e2e_validate_version_number",
        publish_result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_e2e_publish_with_multiple_commands() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");

    let package_name = "e2e-multi-cmd-package";
    let package_version = "1.5.0";

    let package_dir = temp_dir.path().join(package_name);
    fs::create_dir_all(&package_dir).expect("Failed to create package directory");

    let templates_dir = package_dir.join("templates");
    fs::create_dir_all(&templates_dir).expect("Failed to create templates directory");

    let manifest_content = format!(
        r#"[package]
name = "{}"
version = "{}"
description = "E2E test package with multiple commands"
authors = ["test@example.com"]

[commands.build]
description = "Build the project"

[commands.test]
description = "Run tests"

[commands.deploy]
description = "Deploy to production"
"#,
        package_name, package_version
    );
    fs::write(package_dir.join("aikit.toml"), manifest_content)
        .expect("Failed to write aikit.toml");

    for cmd in ["build", "test", "deploy"] {
        fs::write(
            templates_dir.join(format!("{}.md", cmd)),
            format!("# {}\n\nCommand documentation.", cmd),
        )
        .expect("Failed to write command template");
    }

    let orig_cwd = std::env::current_dir().expect("Failed to get original CWD");
    std::env::set_current_dir(&package_dir).expect("Failed to set CWD for test");

    let build_args = pkg_cmd::PackageBuildArgs {
        output: "dist".to_string(),
        agents: None,
        include_sources: false,
    };

    let build_result = pkg_cmd::execute_build(build_args).await;

    assert!(
        build_result.is_ok(),
        "Build failed: {:?}",
        build_result.err()
    );

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
        "e2e_publish_multiple_commands",
        publish_result.unwrap_err().to_string()
    );
}
