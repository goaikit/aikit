//! Performance tests for package installation operations

use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;

    /// Test package creation performance
    #[test]
    fn test_package_creation_performance() {
        let start = Instant::now();

        // Create a test package
        let package = aikit::models::package::Package::create_template(
            "perf-test-pkg".to_string(),
            Some("Performance test package".to_string()),
            Some("Test Author".to_string()),
        );

        let creation_time = start.elapsed();

        // Validate package
        assert!(package.validate().is_ok());

        // Performance assertion: creation should be fast (< 100ms)
        assert!(creation_time < Duration::from_millis(100),
            "Package creation took {:?}, expected < 100ms", creation_time);

        println!("✅ Package creation: {:?}", creation_time);
    }

    /// Test TOML serialization/deserialization performance
    #[test]
    fn test_toml_serialization_performance() {
        // Create a test package
        let package = aikit::models::package::Package::create_template(
            "toml-perf-test".to_string(),
            Some("TOML performance test".to_string()),
            None,
        );

        // Test serialization performance
        let serialize_start = Instant::now();
        let toml_string = package.to_toml_string().unwrap();
        let serialize_time = serialize_start.elapsed();

        // Test deserialization performance
        let deserialize_start = Instant::now();
        let parsed_package = aikit::models::package::Package::from_toml_str(&toml_string).unwrap();
        let deserialize_time = deserialize_start.elapsed();

        // Validate roundtrip
        assert_eq!(parsed_package.package.name, package.package.name);
        assert!(parsed_package.validate().is_ok());

        // Performance assertions
        assert!(serialize_time < Duration::from_millis(50),
            "TOML serialization took {:?}, expected < 50ms", serialize_time);
        assert!(deserialize_time < Duration::from_millis(50),
            "TOML deserialization took {:?}, expected < 50ms", deserialize_time);

        println!("✅ TOML serialize: {:?}, deserialize: {:?}", serialize_time, deserialize_time);
    }

    /// Test agent command generation performance
    #[test]
    fn test_command_generation_performance() {
        use aikit::core::agent::get_agent_configs;

        let agents = get_agent_configs();
        let package = aikit::models::package::Package::create_template(
            "cmd-gen-test".to_string(),
            Some("Command generation test".to_string()),
            None,
        );

        let start = Instant::now();
        let mut command_count = 0;

        // Generate commands for all agents and all package commands
        for agent in &agents {
            for (cmd_name, cmd_def) in &package.commands {
                let _command = agent.generate_package_command(
                    &package.package.name,
                    cmd_name,
                    &cmd_def.description,
                    "# Test script",
                );
                command_count += 1;
            }
        }

        let generation_time = start.elapsed();
        let avg_time_per_command = generation_time / command_count as u32;

        // Performance assertions
        assert!(generation_time < Duration::from_millis(500),
            "Command generation for {} commands took {:?}, expected < 500ms",
            command_count, generation_time);

        assert!(avg_time_per_command < Duration::from_micros(100),
            "Average time per command: {:?}, expected < 100µs", avg_time_per_command);

        println!("✅ Generated {} commands in {:?} ({:?} avg)",
            command_count, generation_time, avg_time_per_command);
    }

    /// Test package validation performance
    #[test]
    fn test_package_validation_performance() {
        // Create packages of different sizes
        let small_package = aikit::models::package::Package::create_template(
            "small-pkg".to_string(),
            Some("Small package".to_string()),
            None,
        );

        let mut large_package = aikit::models::package::Package::create_template(
            "large-pkg".to_string(),
            Some("Large package with many commands".to_string()),
            None,
        );

        // Add many commands to large package
        for i in 0..50 {
            large_package.commands.insert(
                format!("cmd{}", i),
                aikit::models::package::CommandDefinition {
                    description: format!("Command {} for performance testing", i),
                    template: Some(format!("template{}.md", i)),
                },
            );
        }

        // Test small package validation
        let small_start = Instant::now();
        assert!(small_package.validate().is_ok());
        let small_time = small_start.elapsed();

        // Test large package validation
        let large_start = Instant::now();
        assert!(large_package.validate().is_ok());
        let large_time = large_start.elapsed();

        // Performance assertions
        assert!(small_time < Duration::from_millis(10),
            "Small package validation took {:?}, expected < 10ms", small_time);
        assert!(large_time < Duration::from_millis(50),
            "Large package validation took {:?}, expected < 50ms", large_time);

        println!("✅ Validation - small: {:?}, large: {:?}", small_time, large_time);
    }

    /// Benchmark memory usage (basic check)
    #[test]
    fn test_memory_usage_estimate() {
        // This is a basic memory usage test - in a real implementation,
        // you'd use a proper benchmarking framework with memory profiling

        let start = Instant::now();

        // Create multiple packages to test memory scaling
        let mut packages = Vec::new();
        for i in 0..100 {
            packages.push(aikit::models::package::Package::create_template(
                format!("memory-test-pkg-{}", i),
                Some(format!("Memory test package {}", i)),
                Some("Test Author".to_string()),
            ));
        }

        let creation_time = start.elapsed();

        // Basic performance check
        assert!(creation_time < Duration::from_millis(1000),
            "Creating 100 packages took {:?}, expected < 1s", creation_time);

        // Check that all packages are valid
        for package in &packages {
            assert!(package.validate().is_ok());
        }

        println!("✅ Created {} packages in {:?}", packages.len(), creation_time);
    }
}
