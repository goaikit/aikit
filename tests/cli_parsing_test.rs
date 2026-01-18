//! Clap parsing tests
//!
//! These tests verify that `Cli::parse()` works correctly for all commands
//! and catches field name conflicts that could cause TypeId mismatch panics.

use aikit::cli::Cli;
use clap::Parser;

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that global version flag works (handled by clap)
    #[test]
    fn test_global_version_flag() {
        // --version is now handled by clap automatically, so parsing should fail
        // (clap exits with version info instead of returning parsed args)
        assert!(Cli::try_parse_from(["aikit", "--version"]).is_err());
    }

    /// Test that global short version flag works (handled by clap)
    #[test]
    fn test_global_short_version_flag() {
        // -V is now handled by clap automatically, so parsing should fail
        assert!(Cli::try_parse_from(["aikit", "-V"]).is_err());
    }

    /// Test debug flag works
    #[test]
    fn test_debug_flag() {
        let cli = Cli::try_parse_from(["aikit", "--debug", "check"]).unwrap();
        assert!(cli.debug);
        assert!(cli.command.is_some());
    }

    /// Test package init command parsing
    #[test]
    fn test_package_init_basic() {
        let cli = Cli::try_parse_from(["aikit", "package", "init", "test-pkg"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Package(pkg_cmd) => match pkg_cmd {
                aikit::cli::commands::package::PackageCommands::Init(args) => {
                    assert_eq!(args.name, "test-pkg");
                    assert_eq!(args.package_version, "0.1.0"); // default
                    assert!(args.description.is_none());
                    assert!(args.author.is_none());
                    assert!(!args.yes);
                }
                _ => panic!("Expected PackageCommands::Init"),
            },
            _ => panic!("Expected Commands::Package"),
        }
    }

    /// Test package init with all options
    #[test]
    fn test_package_init_with_options() {
        let cli = Cli::try_parse_from([
            "aikit",
            "package",
            "init",
            "my-package",
            "--description",
            "A test package",
            "--package-version",
            "2.1.0",
            "--author",
            "Test Author",
            "--yes",
        ])
        .unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Package(pkg_cmd) => match pkg_cmd {
                aikit::cli::commands::package::PackageCommands::Init(args) => {
                    assert_eq!(args.name, "my-package");
                    assert_eq!(args.package_version, "2.1.0");
                    assert_eq!(args.description.as_deref(), Some("A test package"));
                    assert_eq!(args.author.as_deref(), Some("Test Author"));
                    assert!(args.yes);
                }
                _ => panic!("Expected PackageCommands::Init"),
            },
            _ => panic!("Expected Commands::Package"),
        }
    }

    /// Test package build command parsing
    #[test]
    fn test_package_build_basic() {
        let cli = Cli::try_parse_from(["aikit", "package", "build"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Package(pkg_cmd) => match pkg_cmd {
                aikit::cli::commands::package::PackageCommands::Build(args) => {
                    assert_eq!(args.output, "dist"); // default
                    assert!(args.agents.is_none());
                    assert!(!args.include_sources);
                }
                _ => panic!("Expected PackageCommands::Build"),
            },
            _ => panic!("Expected Commands::Package"),
        }
    }

    /// Test package build with options
    #[test]
    fn test_package_build_with_options() {
        let cli = Cli::try_parse_from([
            "aikit",
            "package",
            "build",
            "--output",
            "build",
            "--agents",
            "claude,copilot",
            "--include-sources",
        ])
        .unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Package(pkg_cmd) => match pkg_cmd {
                aikit::cli::commands::package::PackageCommands::Build(args) => {
                    assert_eq!(args.output, "build");
                    assert_eq!(args.agents.as_deref(), Some("claude,copilot"));
                    assert!(args.include_sources);
                }
                _ => panic!("Expected PackageCommands::Build"),
            },
            _ => panic!("Expected Commands::Package"),
        }
    }

    /// Test install command parsing
    #[test]
    fn test_install_basic() {
        let cli = Cli::try_parse_from(["aikit", "install", "test-package"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Install(args) => {
                assert_eq!(args.source, "test-package");
                assert!(args.install_version.is_none());
                assert!(args.token.is_none());
                assert!(!args.force);
                assert!(!args.yes);
                assert!(args.ai.is_none());
            }
            _ => panic!("Expected Commands::Install"),
        }
    }

    /// Test install with install-version flag (renamed from version)
    #[test]
    fn test_install_with_install_version() {
        let cli = Cli::try_parse_from([
            "aikit",
            "install",
            "test-package",
            "--install-version",
            "1.2.3",
            "--force",
            "--yes",
        ])
        .unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Install(args) => {
                assert_eq!(args.source, "test-package");
                assert_eq!(args.install_version.as_deref(), Some("1.2.3"));
                assert!(args.force);
                assert!(args.yes);
            }
            _ => panic!("Expected Commands::Install"),
        }
    }

    /// Test init command parsing
    #[test]
    fn test_init_basic() {
        let cli = Cli::try_parse_from(["aikit", "init", "my-project"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Init(args) => {
                assert_eq!(args.project_name.as_deref(), Some("my-project"));
                assert!(args.ai.is_none());
                assert!(!args.here);
                assert!(!args.force);
                assert!(!args.no_git);
            }
            _ => panic!("Expected Commands::Init"),
        }
    }

    /// Test init with options
    #[test]
    fn test_init_with_options() {
        let cli = Cli::try_parse_from([
            "aikit",
            "init",
            "my-project",
            "--ai",
            "claude",
            "--here",
            "--force",
        ])
        .unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Init(args) => {
                assert_eq!(args.project_name.as_deref(), Some("my-project"));
                assert_eq!(args.ai.as_deref(), Some("claude"));
                assert!(args.here);
                assert!(args.force);
            }
            _ => panic!("Expected Commands::Init"),
        }
    }

    /// Test check command parsing
    #[test]
    fn test_check_basic() {
        // Check command has no arguments, just verify it parses
        let cli = Cli::try_parse_from(["aikit", "check"]).unwrap();
        match cli.command.unwrap() {
            aikit::cli::Commands::Check(_) => {
                // Success - command parsed correctly
            }
            _ => panic!("Expected Commands::Check"),
        }
    }

    /// Test list command parsing
    #[test]
    fn test_list_basic() {
        let cli = Cli::try_parse_from(["aikit", "list"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::List(args) => {
                assert!(!args.detailed);
            }
            _ => panic!("Expected Commands::List"),
        }
    }

    /// Test list with detailed flag
    #[test]
    fn test_list_detailed() {
        let cli = Cli::try_parse_from(["aikit", "list", "--detailed"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::List(args) => {
                assert!(args.detailed);
            }
            _ => panic!("Expected Commands::List"),
        }
    }

    /// Test search command parsing
    #[test]


    /// Test release command parsing (renamed version field)
    #[test]
    fn test_release_basic() {
        let cli = Cli::try_parse_from(["aikit", "release", "v1.0.0"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Release(args) => {
                assert_eq!(args.release_version, "v1.0.0");
                assert_eq!(args.notes_file, "release_notes.md"); // default
                assert!(args.github_token.is_none());
            }
            _ => panic!("Expected Commands::Release"),
        }
    }

    /// Test release with options
    #[test]
    fn test_release_with_options() {
        let cli = Cli::try_parse_from([
            "aikit",
            "release",
            "v2.1.0",
            "--notes-file",
            "custom_notes.md",
            "--github-token",
            "ghp_123456",
        ])
        .unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Release(args) => {
                assert_eq!(args.release_version, "v2.1.0");
                assert_eq!(args.notes_file, "custom_notes.md");
                assert_eq!(args.github_token.as_deref(), Some("ghp_123456"));
            }
            _ => panic!("Expected Commands::Release"),
        }
    }

    /// Test package publish command parsing
    #[test]
    fn test_package_publish_basic() {
        let cli = Cli::try_parse_from(["aikit", "package", "publish", "owner/repo"]).unwrap();

        match cli.command.unwrap() {
            aikit::cli::Commands::Package(pkg_cmd) => match pkg_cmd {
                aikit::cli::commands::package::PackageCommands::Publish(args) => {
                    assert_eq!(args.repo, "owner/repo");
                    assert!(args.package.is_none());
                    assert!(args.tag.is_none());
                    assert!(args.title.is_none());
                    assert!(args.notes.is_none());
                    assert!(args.token.is_none());
                    assert!(!args.no_release);
                }
                _ => panic!("Expected PackageCommands::Publish"),
            },
            _ => panic!("Expected Commands::Package"),
        }
    }

    /// Test that all subcommands can be parsed without conflicts
    #[test]
    fn test_all_subcommands_parseable() {
        let test_cases = vec![
            vec!["aikit", "--version"],
            vec!["aikit", "package", "init", "test"],
            vec!["aikit", "package", "build"],
            vec!["aikit", "package", "publish", "owner/repo"],
            vec!["aikit", "install", "test-pkg"],
            vec!["aikit", "init", "test"],
            vec!["aikit", "check"],
            vec!["aikit", "list"],
            vec!["aikit", "search", "query"],
            vec!["aikit", "release", "v1.0.0"],
        ];

        for args in test_cases {
            // This should not panic - if it does, we have a parsing conflict
            let _cli = Cli::try_parse_from(&args).unwrap_or_else(|e| {
                panic!("Failed to parse args {:?}: {}", args, e);
            });
        }
    }

    /// Test error cases for malformed arguments
    #[test]
    fn test_parsing_errors() {
        // Missing required package name for init
        assert!(Cli::try_parse_from(["aikit", "package", "init"]).is_err());

        // Missing required repo for publish
        assert!(Cli::try_parse_from(["aikit", "package", "publish"]).is_err());

        // Missing required source for install
        assert!(Cli::try_parse_from(["aikit", "install"]).is_err());

        // Missing required query for search
        assert!(Cli::try_parse_from(["aikit", "search"]).is_err());

        // Missing required version for release
        assert!(Cli::try_parse_from(["aikit", "release"]).is_err());
    }

    /// Test that no arguments triggers help (clap exits with help)
    #[test]
    fn test_no_arguments_triggers_help() {
        // When no command is provided, clap should show help and exit
        // This means parsing will fail because clap calls std::process::exit()
        // But in tests, we can verify that arg_required_else_help is working
        // by checking that the parsing succeeds for valid commands but fails
        // for no arguments (due to required arguments)

        // This should fail because no command is provided and arg_required_else_help is true
        assert!(Cli::try_parse_from(["aikit"]).is_err());
    }
}
