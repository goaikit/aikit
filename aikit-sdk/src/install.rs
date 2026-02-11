use crate::fetch::TemplateSource;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Error types for install operations.
#[derive(Debug)]
pub enum InstallError {
    /// Filesystem operation failed
    Io(io::Error),
    /// Package not found (base directory does not exist)
    NotFound,
    /// Fetch/download failed (HTTP or download errors)
    FetchFailed(String),
    /// Invalid or unsupported source
    InvalidSource(String),
    /// Manifest parsing error (aikit.toml missing or invalid TOML)
    ManifestParse(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Io(err) => write!(f, "Filesystem error: {}", err),
            InstallError::NotFound => write!(f, "Package not found"),
            InstallError::FetchFailed(msg) => write!(f, "Fetch failed: {}", msg),
            InstallError::InvalidSource(msg) => write!(f, "Invalid source: {}", msg),
            InstallError::ManifestParse(msg) => write!(f, "Manifest parse error: {}", msg),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstallError::Io(err) => Some(err),
            InstallError::NotFound => None,
            InstallError::FetchFailed(_) => None,
            InstallError::InvalidSource(_) => None,
            InstallError::ManifestParse(_) => None,
        }
    }
}

impl From<io::Error> for InstallError {
    fn from(err: io::Error) -> Self {
        InstallError::Io(err)
    }
}

impl From<zip::result::ZipError> for InstallError {
    fn from(err: zip::result::ZipError) -> Self {
        InstallError::Io(io::Error::other(err.to_string()))
    }
}

/// Resolve the installed package root: where aikit.toml and sources live.
///
/// If {packages_dir}/{name}-{version}/aikit.toml exists, return that dir.
/// Else if there is exactly one child directory and it contains aikit.toml,
/// return that child (zipball case).
/// Otherwise return base path.
///
/// # Arguments
///
/// * `packages_dir` - The packages installation directory
/// * `package_name` - Name of the package
/// * `version` - Version of the package
///
/// # Returns
///
/// The path to the package root directory.
pub fn installed_package_root(
    packages_dir: &Path,
    package_name: &str,
    version: &str,
) -> Result<PathBuf, InstallError> {
    let base = packages_dir.join(format!("{}-{}", package_name, version));

    // Check if base directory exists
    if !base.exists() {
        return Err(InstallError::NotFound);
    }

    let manifest = base.join("aikit.toml");
    if manifest.exists() {
        return Ok(base);
    }

    // Look for single child directory with aikit.toml (zipball case)
    let children: Vec<PathBuf> = fs::read_dir(&base)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();

    if children.len() == 1 {
        let child = &children[0];
        if child.join("aikit.toml").exists() {
            return Ok(child.clone());
        }
    }

    Ok(base)
}

/// Copy artifact mappings from installed package root to project.
///
/// For each (pattern_str, dest_str) in mappings:
/// - Build glob from pattern_str
/// - Compute prefix (e.g., before ** or *)
/// - Walk package_root with WalkDir
/// - For each matching file, strip prefix and write to dest_dir/subpath
///
/// # Arguments
///
/// * `package_root` - The package root directory
/// * `project_root` - The project root directory
/// * `mappings` - HashMap of glob patterns to destination directories
///
/// # Returns
///
/// Ok(()) if all artifacts were copied successfully.
pub fn copy_artifacts(
    package_root: &Path,
    project_root: &Path,
    mappings: &HashMap<String, String>,
) -> Result<(), InstallError> {
    use glob::Pattern;
    use walkdir::WalkDir;

    for (pattern_str, dest_str) in mappings {
        let glob_pattern = Pattern::new(pattern_str).map_err(|e| {
            InstallError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid glob pattern '{}': {}", pattern_str, e),
            ))
        })?;

        // Extract the prefix: the part before the first glob pattern
        // This handles patterns like "newton/**" (prefix = "newton/")
        // and "templates/*.md" (prefix = "templates/")
        let prefix = if pattern_str.contains("**") {
            pattern_str.split("**").next().unwrap_or("").to_string()
        } else if pattern_str.contains('*') {
            pattern_str.split('*').next().unwrap_or("").to_string()
        } else {
            // No glob pattern, use the whole string as prefix
            pattern_str.clone()
        };

        let dest_dir = project_root.join(dest_str.trim_end_matches('/'));

        for entry in WalkDir::new(package_root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let relative = path
                .strip_prefix(package_root)
                .map_err(|_| InstallError::Io(io::Error::other("Failed to strip prefix")))?;
            // Normalize to forward slashes so glob patterns (e.g. "newton/**") match on Windows
            let path_str: String = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if !glob_pattern.matches(&path_str) {
                continue;
            }
            let prefix_path = Path::new(prefix.trim_end_matches('/'));
            let subpath = if prefix.is_empty() || prefix.trim_end_matches('/').is_empty() {
                relative.to_path_buf()
            } else if let Ok(s) = relative.strip_prefix(prefix_path) {
                PathBuf::from(s)
            } else {
                relative.to_path_buf()
            };
            let dest_file = dest_dir.join(&subpath);
            if let Some(p) = dest_file.parent() {
                fs::create_dir_all(p)?;
            }
            fs::copy(path, &dest_file)?;
        }
    }
    Ok(())
}

/// Options for installing a template to a path.
pub struct InstallTemplateOptions {
    /// Packages directory
    pub packages_dir: PathBuf,
    /// Package name
    pub package_name: String,
    /// Package version
    pub version: String,
    /// Project root directory
    pub project_root: PathBuf,
    /// Artifact mappings (glob pattern -> destination directory)
    pub artifact_mappings: HashMap<String, String>,
    /// Optional agent key (for logging/future use only)
    pub agent_key: Option<String>,
}

/// Options for installing a template from a source (GitHub, URL, or local path)
pub struct InstallTemplateFromSourceOptions {
    /// Source of the template package
    pub source: TemplateSource,
    /// Project root directory where artifacts will be installed
    pub project_root: PathBuf,
    /// Optional packages directory to cache fetched packages
    ///
    /// - If Some(path): fetch into `packages_dir/{name}-{version}` and keep after install
    /// - If None: use a temporary directory for fetch, delete after install
    pub packages_dir: Option<PathBuf>,
}

/// Install a template to a project path.
///
/// This is a convenience function that combines resolving the package root
/// and copying artifacts.
///
/// # Arguments
///
/// * `options` - Installation options including package info and mappings
///
/// # Returns
///
/// Ok(()) if the template was installed successfully.
pub fn install_template_to_path(options: InstallTemplateOptions) -> Result<(), InstallError> {
    let package_root = installed_package_root(
        &options.packages_dir,
        &options.package_name,
        &options.version,
    )?;
    copy_artifacts(
        &package_root,
        &options.project_root,
        &options.artifact_mappings,
    )
}

/// Install a template from a source (GitHub, URL, or local path) to a project.
///
/// This function:
/// 1. Fetches the template package from the source
/// 2. Parses the manifest (aikit.toml) to get artifact mappings
/// 3. Copies artifacts to the project root
///
/// # Arguments
///
/// * `options` - Installation options including source and project root
///
/// # Returns
///
/// Ok(()) if the template was installed successfully.
///
/// # Errors
///
/// Returns InstallError for:
/// - Fetch/download failures
/// - Invalid source or manifest
/// - Filesystem errors during copy
pub fn install_template_from_source(
    options: InstallTemplateFromSourceOptions,
) -> Result<(), InstallError> {
    // Use temp directory to fetch the package
    let temp_dir = tempfile::tempdir().map_err(|e| {
        InstallError::FetchFailed(format!("Failed to create temp directory: {}", e))
    })?;
    let fetch_dir = temp_dir.path().to_path_buf();

    // Fetch package to temp directory
    let (manifest, package_root) = crate::fetch::fetch_package_to_dir(&options.source, &fetch_dir)?;

    // If packages_dir is specified, copy the package there
    if let Some(ref packages_dir) = options.packages_dir {
        let package_dir = packages_dir.join(format!(
            "{}-{}",
            manifest.package.name, manifest.package.version
        ));
        fs::create_dir_all(&package_dir)?;

        // Copy package files to packages_dir
        copy_dir_recursive(&package_root, &package_dir)?;

        // Update package_root to the saved location
        let package_root = package_dir;

        // Get artifact mappings from manifest
        let artifact_mappings = manifest.artifact_mappings();

        // Copy artifacts to project root
        copy_artifacts(&package_root, &options.project_root, artifact_mappings)?;
    } else {
        // Get artifact mappings from manifest
        let artifact_mappings = manifest.artifact_mappings();

        // Copy artifacts to project root
        copy_artifacts(&package_root, &options.project_root, artifact_mappings)?;

        // Clean up temp directory
        let _ = fs::remove_dir_all(fetch_dir);
    }

    Ok(())
}

/// Recursively copy directory contents
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), InstallError> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fetch::TemplateSource;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_installed_package_root_with_manifest() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let packages_dir = temp.path();

        // Create package directory with aikit.toml at root
        let pkg_dir = packages_dir.join("test-pkg-1.0.0");
        fs::create_dir_all(&pkg_dir).map_err(InstallError::Io)?;
        fs::write(pkg_dir.join("aikit.toml"), "[package]\nname = \"test\"")
            .map_err(InstallError::Io)?;

        let result = installed_package_root(packages_dir, "test-pkg", "1.0.0")?;
        assert_eq!(result, pkg_dir);

        Ok(())
    }

    #[test]
    fn test_installed_package_root_with_single_child() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let packages_dir = temp.path();

        // Create package directory with single child containing aikit.toml (zipball case)
        let pkg_dir = packages_dir.join("test-pkg-1.0.0");
        let child_dir = pkg_dir.join("test-pkg-1.0.0");
        fs::create_dir_all(&child_dir).map_err(InstallError::Io)?;
        fs::write(child_dir.join("aikit.toml"), "[package]\nname = \"test\"")
            .map_err(InstallError::Io)?;

        let result = installed_package_root(packages_dir, "test-pkg", "1.0.0")?;
        assert_eq!(result, child_dir);

        Ok(())
    }

    #[test]
    fn test_installed_package_root_not_found() {
        let temp = TempDir::new().unwrap();
        let packages_dir = temp.path();

        let result = installed_package_root(packages_dir, "nonexistent", "1.0.0");
        assert!(matches!(result, Err(InstallError::NotFound)));
    }

    #[test]
    fn test_copy_artifacts_newton_template() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let work = temp.path();

        // Create package root with newton/ structure
        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/scripts")).map_err(InstallError::Io)?;

        // Create test files
        fs::write(package_root.join("newton/README.md"), "# Newton Template")
            .map_err(InstallError::Io)?;
        fs::write(
            package_root.join("newton/scripts/advisor.sh"),
            "#!/bin/sh\necho advisor",
        )
        .map_err(InstallError::Io)?;
        fs::write(
            package_root.join("newton/scripts/evaluator.sh"),
            "#!/bin/sh\necho evaluator",
        )
        .map_err(InstallError::Io)?;

        // Create project root
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).map_err(InstallError::Io)?;

        // Create artifact mappings
        let mut mappings = HashMap::new();
        mappings.insert("newton/**".to_string(), ".newton".to_string());

        // Copy artifacts
        copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify files were copied correctly
        assert!(project_root.join(".newton/README.md").exists());
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());
        assert!(project_root.join(".newton/scripts/evaluator.sh").exists());

        // Verify content
        let readme =
            fs::read_to_string(project_root.join(".newton/README.md")).map_err(InstallError::Io)?;
        assert!(readme.contains("Newton Template"));

        let advisor = fs::read_to_string(project_root.join(".newton/scripts/advisor.sh"))
            .map_err(InstallError::Io)?;
        assert!(advisor.contains("echo advisor"));

        Ok(())
    }

    #[test]
    fn test_copy_artifacts_nested_structure() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let work = temp.path();

        // Create package root with nested structure
        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/deeply/nested/dir"))
            .map_err(InstallError::Io)?;

        // Create files at various depths
        fs::write(package_root.join("newton/top.txt"), "top").map_err(InstallError::Io)?;
        fs::write(package_root.join("newton/deeply/nested/file.txt"), "nested")
            .map_err(InstallError::Io)?;

        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).map_err(InstallError::Io)?;

        let mut mappings = HashMap::new();
        mappings.insert("newton/**".to_string(), ".newton".to_string());

        copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify nested files were copied
        assert!(project_root.join(".newton/top.txt").exists());
        assert!(project_root.join(".newton/deeply/nested/file.txt").exists());

        Ok(())
    }

    #[test]
    fn test_copy_artifacts_glob_pattern() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let work = temp.path();

        let package_root = work.join("package_root");
        fs::create_dir_all(package_root.join("newton/scripts")).map_err(InstallError::Io)?;
        fs::create_dir_all(package_root.join("other")).map_err(InstallError::Io)?;

        // Create files in both directories
        fs::write(package_root.join("newton/scripts/advisor.sh"), "#!/bin/sh")
            .map_err(InstallError::Io)?;
        fs::write(package_root.join("other/ignore.txt"), "ignore").map_err(InstallError::Io)?;

        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).map_err(InstallError::Io)?;

        let mut mappings = HashMap::new();
        // Only copy newton/**, not other/**
        mappings.insert("newton/**".to_string(), ".newton".to_string());

        copy_artifacts(&package_root, &project_root, &mappings)?;

        // Verify only newton/** files were copied
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());
        assert!(!project_root.join("other/ignore.txt").exists());

        Ok(())
    }

    #[test]
    fn test_install_template_to_path() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let work = temp.path();

        // Create packages directory and package
        let packages_dir = work.join("packages");
        let pkg_dir = packages_dir.join("my-template-1.0.0");
        fs::create_dir_all(pkg_dir.join("templates")).map_err(InstallError::Io)?;
        fs::write(
            pkg_dir.join("aikit.toml"),
            "[package]\nname = \"my-template\"",
        )
        .map_err(InstallError::Io)?;
        fs::write(pkg_dir.join("templates/file.txt"), "content").map_err(InstallError::Io)?;

        // Create project root
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).map_err(InstallError::Io)?;

        // Create mappings
        let mut mappings = HashMap::new();
        mappings.insert("templates/**".to_string(), ".templates".to_string());

        // Install template
        let options = InstallTemplateOptions {
            packages_dir,
            package_name: "my-template".to_string(),
            version: "1.0.0".to_string(),
            project_root: project_root.clone(),
            artifact_mappings: mappings,
            agent_key: None,
        };
        install_template_to_path(options)?;

        // Verify file was copied
        assert!(project_root.join(".templates/file.txt").exists());

        Ok(())
    }

    #[test]
    fn test_install_template_from_source_path() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let work = temp.path();

        // Create source directory with template
        let source_dir = work.join("source");
        fs::create_dir_all(source_dir.join("newton/scripts")).map_err(InstallError::Io)?;

        // Create aikit.toml
        let aikit_toml_content = r#"
[package]
name = "newton-template"
version = "1.0.0"
description = "Newton workspace template"

[artifacts]
"newton/**" = ".newton"
"#;
        fs::write(source_dir.join("aikit.toml"), aikit_toml_content).map_err(InstallError::Io)?;

        // Create template files
        fs::write(source_dir.join("newton/README.md"), "# Newton Template")
            .map_err(InstallError::Io)?;
        fs::write(
            source_dir.join("newton/scripts/advisor.sh"),
            "#!/bin/sh\necho advisor",
        )
        .map_err(InstallError::Io)?;

        // Create project root
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).map_err(InstallError::Io)?;

        // Install template from source
        let options = InstallTemplateFromSourceOptions {
            source: TemplateSource::Path(source_dir.clone()),
            project_root: project_root.clone(),
            packages_dir: None,
        };
        install_template_from_source(options)?;

        // Verify files were copied
        assert!(project_root.join(".newton/README.md").exists());
        assert!(project_root.join(".newton/scripts/advisor.sh").exists());

        // Verify content
        let readme =
            fs::read_to_string(project_root.join(".newton/README.md")).map_err(InstallError::Io)?;
        assert!(readme.contains("Newton Template"));

        Ok(())
    }

    #[test]
    fn test_install_template_from_source_with_packages_dir() -> Result<(), InstallError> {
        let temp = TempDir::new().map_err(InstallError::Io)?;
        let work = temp.path();

        // Create source directory
        let source_dir = work.join("source");
        fs::create_dir_all(source_dir.join("templates")).map_err(InstallError::Io)?;

        let aikit_toml_content = r#"
[package]
name = "my-pkg"
version = "2.0.0"

[artifacts]
"templates/**" = ".templates"
"#;
        fs::write(source_dir.join("aikit.toml"), aikit_toml_content).map_err(InstallError::Io)?;
        fs::write(source_dir.join("templates/file.txt"), "content").map_err(InstallError::Io)?;

        // Create packages directory and project root
        let packages_dir = work.join("packages");
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).map_err(InstallError::Io)?;

        // Install with packages_dir
        let options = InstallTemplateFromSourceOptions {
            source: TemplateSource::Path(source_dir.clone()),
            project_root: project_root.clone(),
            packages_dir: Some(packages_dir.clone()),
        };
        install_template_from_source(options)?;

        // Verify files were copied to project root
        assert!(project_root.join(".templates/file.txt").exists());

        // Verify package was saved to packages_dir
        let pkg_dir = packages_dir.join("my-pkg-2.0.0");
        assert!(pkg_dir.exists());
        assert!(pkg_dir.join("aikit.toml").exists());

        Ok(())
    }

    #[test]
    fn test_install_template_from_source_invalid_source() {
        let temp = TempDir::new().unwrap();
        let work = temp.path();
        let project_root = work.join("project_root");
        fs::create_dir_all(&project_root).unwrap();

        // Try to install from non-existent directory
        let options = InstallTemplateFromSourceOptions {
            source: TemplateSource::Path(PathBuf::from("/nonexistent/path")),
            project_root,
            packages_dir: None,
        };

        let result = install_template_from_source(options);
        assert!(result.is_err());
    }
}
