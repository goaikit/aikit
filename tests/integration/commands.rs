//! Integration tests for aikit CLI commands
//!
//! This module contains integration tests to verify command help messages
//! and ensure they match expected output.

use std::process::Command;

#[test]
fn test_aikit_init_help() {
    let output = Command::new("aikit")
        .arg("init")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit init --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    // Verify key message components are present
    assert!(output_str.contains("Initialize a new Spec-Driven Development project"));
    assert!(output_str.contains("[PROJECT_NAME]"));
    assert!(output_str.contains("--ai"));
    assert!(output_str.contains("--script"));
    assert!(output_str.contains("--here"));
}

#[test]
fn test_aikit_check_help() {
    let output = Command::new("aikit")
        .arg("check")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit check --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Check installed tools and AI agent CLIs"));
}

#[test]
fn test_aikit_version_help() {
    let output = Command::new("aikit")
        .arg("version")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit version --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Display version information"));
    assert!(output_str.contains("--github-token"));
}

#[test]
fn test_aikit_release_help() {
    let output = Command::new("aikit")
        .arg("release")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit release --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Create GitHub release with package files"));
    assert!(output_str.contains("[VERSION]"));
    assert!(output_str.contains("--notes-file"));
}

#[test]
fn test_aikit_package_help() {
    let output = Command::new("aikit")
        .arg("package")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit package --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Package management commands"));
    assert!(output_str.contains("Build template zip archives"));
}

#[test]
fn test_aikit_install_help() {
    let output = Command::new("aikit")
        .arg("install")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit install --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Install package from GitHub URL"));
    assert!(output_str.contains("Package source"));
    assert!(output_str.contains("--version"));
    assert!(output_str.contains("--token"));
}

#[test]
fn test_aikit_update_help() {
    let output = Command::new("aikit")
        .arg("update")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit update --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Update installed package"));
    assert!(output_str.contains("[package]"));
    assert!(output_str.contains("--breaking"));
}

#[test]
fn test_aikit_remove_help() {
    let output = Command::new("aikit")
        .arg("remove")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit remove --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Remove installed package"));
    assert!(output_str.contains("[package]"));
    assert!(output_str.contains("--force"));
}

#[test]
fn test_aikit_list_help() {
    let output = Command::new("aikit")
        .arg("list")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit list --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("List installed packages"));
    assert!(output_str.contains("--author"));
    assert!(output_str.contains("--detailed"));
}

#[test]
fn test_aikit_package_init_help() {
    let output = Command::new("aikit")
        .arg("package")
        .arg("init")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit package init --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Initialize a new package with aikit.toml"));
    assert!(output_str.contains("[name]"));
    assert!(output_str.contains("--description"));
    assert!(output_str.contains("--author"));
}

#[test]
fn test_aikit_package_build_help() {
    let output = Command::new("aikit")
        .arg("package")
        .arg("build")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit package build --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Build package for distribution"));
    assert!(output_str.contains("[OPTIONS]"));
    assert!(output_str.contains("--agents"));
}

#[test]
fn test_aikit_package_publish_help() {
    let output = Command::new("aikit")
        .arg("package")
        .arg("publish")
        .arg("--help")
        .output()
        .expect("Failed to execute aikit package publish --help");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    assert!(output_str.contains("Publish package to registry"));
    assert!(output_str.contains("[repo]"));
    assert!(output_str.contains("--package"));
    assert!(output_str.contains("--tag"));
}

#[test]
fn test_aikit_version_command_displays_version() {
    let output = Command::new("aikit")
        .arg("version")
        .output()
        .expect("Failed to execute aikit version");

    assert!(output.status.success());
    let output_str = String::from_utf8(output.stdout).unwrap();

    // Version should be displayed (either from Cargo.toml or from system)
    assert!(output_str.contains("CLI") || output_str.contains("AIKit"));
}
