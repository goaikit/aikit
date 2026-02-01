use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_init_command_basic() {
        let args = super::PackageInitArgs {
            name: "test-package".to_string(),
            description: Some("Test package".to_string()),
            package_version: "1.0.0".to_string(),
            author: Some("Test Author".to_string()),
            yes: true,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));

        assert!(result.is_ok());
    }

    #[test]
    fn test_package_init_command_default_version() {
        let args = super::PackageInitArgs {
            name: "test-package".to_string(),
            description: Some("Test package".to_string()),
            package_version: "0.1.0".to_string(),
            author: Some("Test Author".to_string()),
            yes: true,
        };

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute(args));

        assert!(result.is_ok());
    }
}
