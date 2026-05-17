//! Integration test for Newton template installation
//!
//! Tests the complete flow of installing a Newton template package
//! and verifying that artifacts are correctly copied to .newton/

use assert_cmd::cargo::cargo_bin_cmd;
use std::fs;
use tempfile::tempdir;

/// Walk a directory and return a sorted list of relative paths for diagnostics.
fn walk_tree(root: &std::path::Path) -> Vec<String> {
    let mut entries = Vec::new();
    if !root.exists() {
        return entries;
    }
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if let Ok(rel) = entry.path().strip_prefix(root) {
            let s = rel.display().to_string();
            if !s.is_empty() {
                entries.push(s);
            }
        }
    }
    entries.sort();
    entries
}

/// Run the install command and print diagnostics on failure.
fn run_install(work: &std::path::Path, fixture_path: &str, extra_args: &[&str]) {
    // Pre-create .aikit/ inside the tempdir so AikDirectory::find() short-circuits here
    // instead of walking up and discovering an unrelated .aikit/ left somewhere above
    // (observed on the Windows CI runner — install would then write artifacts to that
    // foreign project root and the tempdir would stay empty).
    let aikit_dir = work.join(".aikit");
    if !aikit_dir.exists() {
        fs::create_dir_all(&aikit_dir).expect("failed to pre-create .aikit/ in tempdir");
    }

    let mut args: Vec<&str> = vec!["install", fixture_path, "--ai", "newton", "--yes"];
    args.extend_from_slice(extra_args);

    let output = cargo_bin_cmd!("aikit")
        .current_dir(work)
        .args(&args)
        .output()
        .expect("failed to spawn aikit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!("--- aikit install diagnostics ---");
    println!("work = {}", work.display());
    println!("fixture = {}", fixture_path);
    println!("exit status = {}", output.status);
    println!("--- stdout ---\n{}", stdout);
    println!("--- stderr ---\n{}", stderr);
    println!("--- work tree after install ---");
    for p in walk_tree(work) {
        println!("  {}", p);
    }
    println!("--- end diagnostics ---");

    assert!(output.status.success(), "aikit install failed");
    assert!(
        stdout.contains("Installing package") || stdout.contains("installed successfully"),
        "expected install output not found"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test installing Newton template and verifying .newton/ layout
    #[test]
    fn test_install_newton_template() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        run_install(work, &fixture_path, &[]);

        let newton_dir = work.join(".newton");
        assert!(newton_dir.exists(), ".newton/ directory should exist");

        let readme = newton_dir.join("README.md");
        assert!(readme.exists(), ".newton/README.md should exist");

        let scripts_dir = newton_dir.join("scripts");
        assert!(scripts_dir.exists(), ".newton/scripts/ should exist");

        let scripts = vec![
            "advisor.sh",
            "evaluator.sh",
            "post-success.sh",
            "post-failure.sh",
        ];
        for script in &scripts {
            let script_path = scripts_dir.join(script);
            assert!(
                script_path.exists(),
                ".newton/scripts/{} should exist",
                script
            );

            let content = fs::read_to_string(&script_path)?;
            assert!(
                !content.trim().is_empty(),
                ".newton/scripts/{} should not be empty",
                script
            );

            assert!(
                content.starts_with("#!/"),
                ".newton/scripts/{} should be executable",
                script
            );
        }

        Ok(())
    }

    #[test]
    fn test_install_newton_template_readme_content() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        run_install(work, &fixture_path, &[]);

        let readme = fs::read_to_string(work.join(".newton/README.md"))?;

        assert!(readme.contains("Newton Workspace Template"));
        assert!(readme.contains("advisor.sh"));
        assert!(readme.contains("evaluator.sh"));
        assert!(readme.contains("post-success.sh"));
        assert!(readme.contains("post-failure.sh"));

        Ok(())
    }

    #[test]
    fn test_install_newton_template_scripts_content() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        run_install(work, &fixture_path, &[]);

        let scripts_dir = work.join(".newton/scripts");

        let advisor = fs::read_to_string(scripts_dir.join("advisor.sh"))?;
        assert!(advisor.contains("Advisor"));
        assert!(advisor.contains("planning phase"));

        let evaluator = fs::read_to_string(scripts_dir.join("evaluator.sh"))?;
        assert!(evaluator.contains("Evaluator"));
        assert!(evaluator.contains("plan progress"));

        let post_success = fs::read_to_string(scripts_dir.join("post-success.sh"))?;
        assert!(post_success.contains("Post-Success"));
        assert!(post_success.contains("successful"));

        let post_failure = fs::read_to_string(scripts_dir.join("post-failure.sh"))?;
        assert!(post_failure.contains("Post-Failure"));
        assert!(post_failure.contains("failed"));

        Ok(())
    }

    #[test]
    fn test_install_newton_template_force() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        run_install(work, &fixture_path, &[]);
        run_install(work, &fixture_path, &["--force"]);

        assert!(work.join(".newton").exists());

        Ok(())
    }
}
