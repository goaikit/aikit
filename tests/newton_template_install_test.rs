//! Integration test for Newton template installation
//!
//! Tests the complete flow of installing a Newton template package
//! and verifying that artifacts are correctly copied to .newton/

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test installing Newton template and verifying .newton/ layout
    #[test]
    fn test_install_newton_template() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        // Get the path to the Newton template fixture
        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        // Install the Newton template for the newton agent
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args(["install", &fixture_path, "--ai", "newton", "--yes"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Installing package"))
            .stdout(predicate::str::contains("installed successfully"));

        // Verify .newton/ directory was created
        let newton_dir = work.join(".newton");
        assert!(newton_dir.exists(), ".newton/ directory should exist");

        // Verify README.md exists
        let readme = newton_dir.join("README.md");
        assert!(readme.exists(), ".newton/README.md should exist");

        // Verify scripts directory exists
        let scripts_dir = newton_dir.join("scripts");
        assert!(scripts_dir.exists(), ".newton/scripts/ should exist");

        // Verify all scripts exist
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

            // Verify script is non-empty
            let content = fs::read_to_string(&script_path)?;
            assert!(
                !content.trim().is_empty(),
                ".newton/scripts/{} should not be empty",
                script
            );

            // Verify script is executable (has shebang)
            assert!(
                content.starts_with("#!/"),
                ".newton/scripts/{} should be executable",
                script
            );
        }

        Ok(())
    }

    /// Test that Newton template README content is correct
    #[test]
    fn test_install_newton_template_readme_content() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args(["install", &fixture_path, "--ai", "newton", "--yes"])
            .assert()
            .success();

        let readme = fs::read_to_string(work.join(".newton/README.md"))?;

        assert!(readme.contains("Newton Workspace Template"));
        assert!(readme.contains("advisor.sh"));
        assert!(readme.contains("evaluator.sh"));
        assert!(readme.contains("post-success.sh"));
        assert!(readme.contains("post-failure.sh"));

        Ok(())
    }

    /// Test that Newton template scripts have correct content
    #[test]
    fn test_install_newton_template_scripts_content() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args(["install", &fixture_path, "--ai", "newton", "--yes"])
            .assert()
            .success();

        let scripts_dir = work.join(".newton/scripts");

        // Test advisor.sh
        let advisor = fs::read_to_string(scripts_dir.join("advisor.sh"))?;
        assert!(advisor.contains("Advisor"));
        assert!(advisor.contains("planning phase"));

        // Test evaluator.sh
        let evaluator = fs::read_to_string(scripts_dir.join("evaluator.sh"))?;
        assert!(evaluator.contains("Evaluator"));
        assert!(evaluator.contains("plan progress"));

        // Test post-success.sh
        let post_success = fs::read_to_string(scripts_dir.join("post-success.sh"))?;
        assert!(post_success.contains("Post-Success"));
        assert!(post_success.contains("successful"));

        // Test post-failure.sh
        let post_failure = fs::read_to_string(scripts_dir.join("post-failure.sh"))?;
        assert!(post_failure.contains("Post-Failure"));
        assert!(post_failure.contains("failed"));

        Ok(())
    }

    /// Test installing Newton template with force flag
    #[test]
    fn test_install_newton_template_force() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempdir()?;
        let work = temp.path();

        let fixture_path = format!(
            "{}/tests/fixtures/newton-template",
            env!("CARGO_MANIFEST_DIR")
        );

        // Install first time
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args(["install", &fixture_path, "--ai", "newton", "--yes"])
            .assert()
            .success();

        // Install again with force flag - should succeed without confirmation prompt
        cargo_bin_cmd!("aikit")
            .current_dir(work)
            .args([
                "install",
                &fixture_path,
                "--ai",
                "newton",
                "--force",
                "--yes",
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains("installed successfully"));

        // Verify .newton/ still exists
        assert!(work.join(".newton").exists());

        Ok(())
    }
}
