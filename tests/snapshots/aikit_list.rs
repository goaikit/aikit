use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_command_basic() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_list(super::ListArgs {
                author: None,
                detailed: false,
            }));

        assert!(result.is_ok());
    }

    #[test]
    fn test_list_command_detailed() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_list(super::ListArgs {
                author: None,
                detailed: true,
            }));

        assert!(result.is_ok());
    }

    #[test]
    fn test_list_command_by_author() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_list(super::ListArgs {
                author: Some("test-author".to_string()),
                detailed: false,
            }));

        assert!(result.is_ok());
    }

    #[test]
    fn test_list_command_no_packages_installed() {
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(super::execute_list(super::ListArgs {
                author: None,
                detailed: false,
            }));

        assert!(result.is_ok());
    }
}
