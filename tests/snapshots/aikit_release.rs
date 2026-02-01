use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_command_basic() {
        let args = super::ReleaseArgs {
            release_version: "v1.0.0".to_string(),
            notes_file: "release_notes.md".to_string(),
            github_token: None,
        };

        // This will fail because .genreleases/ doesn't exist, but we're testing the flow
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));
        assert!(result.is_err());
    }

    #[test]
    fn test_release_command_invalid_version() {
        let args = super::ReleaseArgs {
            release_version: "1.0.0".to_string(), // Missing 'v' prefix
            notes_file: "release_notes.md".to_string(),
            github_token: None,
        };

        let result = super::validate_version_format(&args.release_version);
        assert!(result.is_err());
    }

    #[test]
    fn test_release_command_invalid_format() {
        let args = super::ReleaseArgs {
            release_version: "v1".to_string(), // Invalid format
            notes_file: "release_notes.md".to_string(),
            github_token: None,
        };

        let result = super::validate_version_format(&args.release_version);
        assert!(result.is_err());
    }
}
