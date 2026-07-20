//! Template processing and extraction utilities

use std::fs;
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

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
/// Entries are never materialized as OS-level symlinks: each entry's bytes
/// are always written into a regular file at the computed, validated
/// destination path, regardless of what type bits the archive stores for it.
pub fn extract_and_flatten_zip(
    zip_data: &[u8],
    dest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(dest_path)?;

    // Canonicalize dest_path once for comparison against every entry's
    // resolved output path.
    let dest_canonical = dest_path.canonicalize().map_err(|e| {
        format!(
            "Failed to canonicalize destination path {}: {}",
            dest_path.display(),
            e
        )
    })?;

    let cursor = Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| format!("Invalid or corrupt template zip archive: {}", e))?;

    if archive.is_empty() {
        return Err("Template zip archive is empty".into());
    }

    let common_prefix = find_common_top_level_dir(&mut archive)?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to access zip entry {}: {}", i, e))?;

        let entry_name = file.name().to_string();
        let is_dir_entry = entry_name.ends_with('/');

        // Reject path traversal before any further processing.
        if entry_name.contains("..") {
            return Err(format!("Path traversal detected in zip entry: {}", entry_name).into());
        }

        // Reject absolute paths (POSIX `/`, Windows `\`, or drive-letter forms
        // like `C:`) portably, since zip entry names are not necessarily
        // interpreted as this platform's native path semantics.
        if has_absolute_prefix(&entry_name) {
            return Err(format!("Absolute path detected in zip entry: {}", entry_name).into());
        }

        // Resolve `.`/`..` components in-memory and strip a shared top-level
        // wrapping directory, if one was detected across the whole archive.
        let normalized = normalize_path(Path::new(&entry_name));
        let relative = match strip_common_prefix(&normalized, common_prefix.as_deref()) {
            Some(rel) if !rel.as_os_str().is_empty() => rel,
            // Entry *is* the wrapping directory itself (or normalizes to
            // nothing) — nothing to write.
            _ => continue,
        };

        if !relative.is_relative() {
            return Err(format!("Absolute path detected in zip entry: {}", entry_name).into());
        }

        let outpath = dest_path.join(&relative);

        // Validate that the resolved output path stays under dest_path even
        // after any symlink resolution.
        let outpath_canonical = if outpath.exists() {
            outpath
                .canonicalize()
                .map_err(|e| format!("Failed to canonicalize output path: {}", e))?
        } else {
            dest_canonical.join(&relative)
        };

        if !outpath_canonical.starts_with(&dest_canonical) {
            return Err(format!("Path traversal detected in zip entry: {}", entry_name).into());
        }

        if is_dir_entry {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            // Always write a regular file: even if this entry stores unix
            // symlink mode bits, `std::io::copy` here materializes its
            // content as plain bytes, never as an OS-level symlink.
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }

    Ok(())
}

/// If every entry in the archive shares the same single top-level path
/// component, return that component so callers can strip it (flattening a
/// GitHub-zipball-style wrapping directory). Returns `None` when the archive
/// has no common top-level directory (including the normal case where it was
/// produced flat, as this CLI's own `generate_package` does).
fn find_common_top_level_dir(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut common: Option<String> = None;

    for i in 0..archive.len() {
        let file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to access zip entry {}: {}", i, e))?;
        let name = file.name();
        if name.contains("..") || has_absolute_prefix(name) {
            // Let the main extraction loop produce the real, entry-specific
            // error; here we just bail out of prefix detection.
            return Ok(None);
        }

        let is_dir_entry = name.ends_with('/');
        let trimmed = name.trim_end_matches('/');

        let top = match trimmed.split('/').next() {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => return Ok(None),
        };

        // A *file* entry with no `/` at all sits directly at the archive
        // root, which means the archive is already flat (no wrapping dir to
        // strip). A directory entry with no `/` (e.g. `top/` itself, the
        // wrapping directory's own entry) is not evidence of flatness — it's
        // exactly the wrapping directory candidate.
        if !trimmed.contains('/') && !is_dir_entry {
            return Ok(None);
        }

        match &common {
            None => common = Some(top),
            Some(existing) if *existing == top => {}
            Some(_) => return Ok(None),
        }
    }

    Ok(common)
}

/// Strip `prefix/` from the front of `path` if `prefix` is set and matches.
fn strip_common_prefix(path: &Path, prefix: Option<&str>) -> Option<PathBuf> {
    match prefix {
        None => Some(path.to_path_buf()),
        Some(prefix) => {
            let mut components = path.components();
            match components.next() {
                Some(Component::Normal(c)) if c.to_string_lossy() == prefix => {
                    Some(components.as_path().to_path_buf())
                }
                _ => Some(path.to_path_buf()),
            }
        }
    }
}

/// Normalize a path by resolving `.`/`..` components in memory (does not
/// touch the filesystem).
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                result.push(component);
            }
            Component::CurDir => {
                // Skip `.` - it doesn't change anything.
            }
            Component::ParentDir => {
                // Pop the last normal component if possible; never allowed
                // to escape past what's already been pushed.
                result.pop();
            }
            Component::Normal(c) => {
                result.push(c);
            }
        }
    }
    result
}

/// Zip entry names are POSIX-oriented; a plain `Path` may not treat
/// `/etc/...` or `C:\...` as absolute on every platform, so check explicitly.
fn has_absolute_prefix(entry_name: &str) -> bool {
    if entry_name.is_empty() {
        return false;
    }
    if entry_name.starts_with('/') || entry_name.starts_with('\\') {
        return true;
    }
    let mut it = entry_name.chars();
    matches!(
        (it.next(), it.next()),
        (Some(d), Some(':')) if d.is_ascii_alphabetic()
    )
}

/// Copy directory recursively (moved from fs module to avoid conflicts)
#[allow(dead_code)]
pub fn copy_directory(from: &Path, to: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in walkdir::WalkDir::new(from)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let source_path = entry.path();
        let relative_path = source_path.strip_prefix(from)?;
        let dest_path = to.join(relative_path);

        if source_path.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source_path, dest_path)?;
        }
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
