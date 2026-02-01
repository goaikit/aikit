use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_command_basic() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(super::VersionArgs { github_token: None }));

        assert!(result.is_ok());
    }

    #[test]
    fn test_version_command_with_token() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(super::VersionArgs {
                github_token: Some("test-token".to_string()),
            }));

        assert!(result.is_ok());
    }
}
