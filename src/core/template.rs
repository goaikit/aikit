//! Template processing and extraction utilities

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Project path information
pub struct ProjectPath {
    pub path: PathBuf,
}

impl ProjectPath {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

/// Select the release asset matching the requested agent + script variant.
///
/// Release assets produced by `aikit package` / `template_package::execute`
/// (see `src/core/package.rs::generate_package`) are named
/// `spec-kit-template-{agent_key}-{script_variant}-{version}.zip`. We match on
/// that filename convention against the basename of each candidate asset URL
/// (case-insensitively, since GitHub asset URLs are not guaranteed to preserve
/// case and callers may pass agent keys in any case).
pub fn select_template_asset(
    assets: &[String],
    agent_key: &str,
    script_variant: &str,
) -> Option<String> {
    let prefix = format!(
        "spec-kit-template-{}-{}-",
        agent_key.to_lowercase(),
        script_variant.to_lowercase()
    );

    assets
        .iter()
        .find(|url| {
            let basename = url
                .rsplit('/')
                .next()
                .unwrap_or(url.as_str())
                .to_lowercase();
            basename.starts_with(&prefix) && basename.ends_with(".zip")
        })
        .cloned()
}

/// Extract a ZIP archive to `dest_path`, guarding against zip-slip (path
/// traversal / absolute-path entries) and flattening a single common
/// top-level wrapping directory if the archive has one (as produced by
/// GitHub-style zipballs; a no-op for the flat archives this CLI itself
/// produces via `generate_package`).
///
/// The actual extraction — including all zip-slip/absolute-path/symlink
/// hardening — is delegated to `aikit_sdk::extract_zip`, the single
/// canonical zip extractor (ARCH-1; previously duplicated here). This
/// function extracts into a staging directory and then adds the flattening
/// behavior on top, which the SDK extractor itself doesn't need (its own
/// callers locate the package root inside a wrapping directory instead of
/// stripping it — see `aikit_sdk::installed_package_root`).
pub fn extract_and_flatten_zip(
    zip_data: &[u8],
    dest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Preserve the "reject a genuinely empty archive" guard: the SDK
    // extractor treats an empty archive as a harmless no-op, but an empty
    // *template* is a user-facing error here.
    {
        let cursor = Cursor::new(zip_data);
        let archive = zip::ZipArchive::new(cursor)
            .map_err(|e| format!("Invalid or corrupt template zip archive: {}", e))?;
        if archive.is_empty() {
            return Err("Template zip archive is empty".into());
        }
    }

    // Extract into a staging directory first so a single common top-level
    // wrapping directory (GitHub-zipball style) can be detected and
    // stripped without the extractor itself needing to know about it.
    let staging = tempfile::tempdir()?;
    aikit_sdk::extract_zip(zip_data, staging.path())?;

    fs::create_dir_all(dest_path)?;

    let children: Vec<PathBuf> = fs::read_dir(staging.path())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();

    if children.len() == 1 && children[0].is_dir() {
        // Single top-level directory: flatten by copying its *contents* up
        // into dest_path rather than the wrapper itself.
        aikit_sdk::copy_dir(&children[0], dest_path)?;
    } else {
        aikit_sdk::copy_dir(staging.path(), dest_path)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{FileOptions, ZipWriter};

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = Cursor::new(&mut buf);
            let mut zip = ZipWriter::new(cursor);
            let options = FileOptions::default();
            for (name, content) in entries {
                if name.ends_with('/') {
                    zip.add_directory(*name, options).unwrap();
                } else {
                    zip.start_file(*name, options).unwrap();
                    zip.write_all(content).unwrap();
                }
            }
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn select_template_asset_matches_agent_and_script() {
        let assets = vec![
            "https://example.com/spec-kit-template-claude-sh-v1.2.3.zip".to_string(),
            "https://example.com/spec-kit-template-claude-ps-v1.2.3.zip".to_string(),
            "https://example.com/spec-kit-template-copilot-sh-v1.2.3.zip".to_string(),
        ];

        let picked = select_template_asset(&assets, "claude", "sh").unwrap();
        assert_eq!(
            picked,
            "https://example.com/spec-kit-template-claude-sh-v1.2.3.zip"
        );

        let picked_ps = select_template_asset(&assets, "claude", "ps").unwrap();
        assert_eq!(
            picked_ps,
            "https://example.com/spec-kit-template-claude-ps-v1.2.3.zip"
        );
    }

    #[test]
    fn select_template_asset_case_insensitive() {
        let assets = vec!["https://example.com/Spec-Kit-Template-Claude-SH-v1.0.0.ZIP".to_string()];
        assert!(select_template_asset(&assets, "claude", "sh").is_some());
    }

    #[test]
    fn select_template_asset_returns_none_when_no_match() {
        let assets =
            vec!["https://example.com/spec-kit-template-copilot-sh-v1.0.0.zip".to_string()];
        assert!(select_template_asset(&assets, "claude", "sh").is_none());
    }

    #[test]
    fn extract_and_flatten_zip_writes_expected_files() {
        let zip_data = build_zip(&[
            (".specify/memory/constitution.md", b"# Constitution"),
            ("templates/spec-template.md", b"# Spec"),
            ("scripts/bash/setup.sh", b"#!/bin/sh\necho hi"),
        ]);

        let temp_dir = tempfile::tempdir().unwrap();
        extract_and_flatten_zip(&zip_data, temp_dir.path()).unwrap();

        assert_eq!(
            fs::read_to_string(temp_dir.path().join(".specify/memory/constitution.md")).unwrap(),
            "# Constitution"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("templates/spec-template.md")).unwrap(),
            "# Spec"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("scripts/bash/setup.sh")).unwrap(),
            "#!/bin/sh\necho hi"
        );
    }

    #[test]
    fn extract_and_flatten_zip_strips_common_wrapping_dir() {
        // Simulates a GitHub-zipball-style archive with a single top-level
        // wrapping directory (e.g. `owner-repo-sha1234/...`).
        let zip_data = build_zip(&[
            ("owner-repo-abc123/", b""),
            ("owner-repo-abc123/README.md", b"hello"),
            ("owner-repo-abc123/nested/file.txt", b"nested content"),
        ]);

        let temp_dir = tempfile::tempdir().unwrap();
        extract_and_flatten_zip(&zip_data, temp_dir.path()).unwrap();

        assert!(!temp_dir.path().join("owner-repo-abc123").exists());
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("README.md")).unwrap(),
            "hello"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("nested/file.txt")).unwrap(),
            "nested content"
        );
    }

    #[test]
    fn extract_and_flatten_zip_rejects_relative_path_traversal() {
        let zip_data = build_zip(&[
            ("safe.txt", b"safe content"),
            ("../../../escape.txt", b"malicious content"),
        ]);

        let temp_dir = tempfile::tempdir().unwrap();
        let dest_dir = temp_dir.path().join("dest");
        let result = extract_and_flatten_zip(&zip_data, &dest_dir);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));

        // The escape file must not have landed outside dest_dir.
        assert!(!temp_dir.path().join("escape.txt").exists());
    }

    #[test]
    fn extract_and_flatten_zip_rejects_absolute_path_entry() {
        let zip_data = build_zip(&[("/etc/passwd", b"malicious content")]);

        let temp_dir = tempfile::tempdir().unwrap();
        let dest_dir = temp_dir.path().join("dest");
        let result = extract_and_flatten_zip(&zip_data, &dest_dir);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Absolute path"));
        assert!(!Path::new("/etc/passwd_pwned").exists());
    }

    #[test]
    fn extract_and_flatten_zip_symlink_entry_is_downgraded_to_regular_file() {
        // Even if a zip entry's stored metadata claims to be a symlink, the
        // extractor always materializes its content as a plain file — never
        // as an OS-level symlink — so a "symlink to /etc/passwd" entry just
        // becomes an inert regular file containing that string.
        let zip_data = build_zip(&[("link.txt", b"/etc/passwd")]);

        let temp_dir = tempfile::tempdir().unwrap();
        extract_and_flatten_zip(&zip_data, temp_dir.path()).unwrap();

        let out = temp_dir.path().join("link.txt");
        assert!(out.is_file());
        assert!(!out.symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(out).unwrap(), "/etc/passwd");
    }

    #[test]
    fn extract_and_flatten_zip_rejects_empty_archive() {
        let zip_data = build_zip(&[]);
        let temp_dir = tempfile::tempdir().unwrap();
        let result = extract_and_flatten_zip(&zip_data, temp_dir.path());
        assert!(result.is_err());
    }
}
