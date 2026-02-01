use std::fs;
use std::path::PathBuf;

pub fn get_test_project_dir() -> PathBuf {
    // Create a test project directory in tests/fixtures
    let fixtures_dir = PathBuf::from("tests/fixtures");
    fs::create_dir_all(&fixtures_dir).ok();
    fixtures_dir
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::agent::get_agent_configs;
    use crate::core::template::ProjectPath;
    use tempfile::TempDir;

    #[test]
    fn test_init_command_basic() {
        let fixtures_dir = get_test_project_dir();
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("test-init");

        let args = super::InitArgs {
            project_name: Some(test_path.file_name().unwrap().to_str().unwrap().to_string()),
            ai: Some("copilot".to_string()),
            script: Some("sh".to_string()),
            here: false,
            force: false,
            no_git: true,
            github_token: None,
            skip_tls: false,
            debug: false,
            ignore_agent_tools: false,
        };

        let result = crate::cli::init::execute(args);
        assert!(result.is_ok());

        // Create snapshot of expected output
        let output = format!(
            "✓ Project initialized successfully at {}\n  Agent: {}\n  Script type: sh",
            test_path.display(),
            get_agent_configs()
                .find(|a| a.key == "copilot")
                .unwrap()
                .name
        );

        // This would be saved as a snapshot in real testing
        // For now, we just verify it succeeds
        assert!(test_path.exists());
        assert!(test_path.join(".aikit").exists());
    }

    #[test]
    fn test_init_command_current_directory() {
        let fixtures_dir = get_test_project_dir();
        let temp_dir = TempDir::new().unwrap();

        // Save current directory
        let current_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let args = super::InitArgs {
            project_name: Some(".".to_string()),
            ai: Some("claude".to_string()),
            script: None,
            here: true,
            force: false,
            no_git: false,
            github_token: None,
            skip_tls: false,
            debug: false,
            ignore_agent_tools: false,
        };

        let result = crate::cli::init::execute(args);
        assert!(result.is_ok());

        // Restore current directory
        std::env::set_current_dir(&current_dir).unwrap();

        let project_path = temp_dir.path().join(".aikit");
        assert!(project_path.exists());
    }

    #[test]
    fn test_init_command_no_git() {
        let fixtures_dir = get_test_project_dir();
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("test-no-git");

        let args = super::InitArgs {
            project_name: Some(test_path.file_name().unwrap().to_str().unwrap().to_string()),
            ai: Some("copilot".to_string()),
            script: None,
            here: false,
            force: false,
            no_git: true,
            github_token: None,
            skip_tls: false,
            debug: false,
            ignore_agent_tools: false,
        };

        let result = crate::cli::init::execute(args);
        assert!(result.is_ok());

        let project_path = ProjectPath::new(test_path);
        assert!(project_path.path.exists());
        assert!(!project_path.path.join(".git").exists());
    }

    #[test]
    fn test_init_command_ps_script() {
        let fixtures_dir = get_test_project_dir();
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join("test-ps");

        let args = super::InitArgs {
            project_name: Some(test_path.file_name().unwrap().to_str().unwrap().to_string()),
            ai: Some("copilot".to_string()),
            script: Some("ps".to_string()),
            here: false,
            force: false,
            no_git: true,
            github_token: None,
            skip_tls: false,
            debug: false,
            ignore_agent_tools: false,
        };

        let result = crate::cli::init::execute(args);
        assert!(result.is_ok());

        assert!(test_path.exists());
    }
}
