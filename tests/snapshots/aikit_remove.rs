use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_command_basic() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_remove(super::RemoveArgs {
                package: "test-package".to_string(),
                force: false,
            }));

        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_command_force() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_remove(super::RemoveArgs {
                package: "test-package".to_string(),
                force: true,
            }));

        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_command_invalid_package_name() {
        assert!(crate::core::validation::validate_package_name("invalid-name").is_err());
    }

    #[test]
    fn test_remove_command_package_not_found() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_remove(super::RemoveArgs {
                package: "nonexistent-package".to_string(),
                force: true,
            }));

        assert!(result.is_err());
    }
}
