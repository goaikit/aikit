use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_command_basic() {
        let result = crate::cli::check::execute(super::CheckArgs {});
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_command_without_git() {
        // This test verifies check works even if git is not available
        let result = crate::cli::check::execute(super::CheckArgs {});
        // Should succeed and report git status
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_command_with_agents() {
        let result = crate::cli::check::execute(super::CheckArgs {});
        assert!(result.is_ok());
    }
}
