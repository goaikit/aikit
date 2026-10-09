//! Deploy or remove a single skill entry in a skills folder, leaving every
//! other entry in that folder alone.
//!
//! Unlike [`crate::deploy_skill`], which writes a skill's files into an
//! agent's project skills directory, [`deploy_skill_entry`] places one
//! existing skill folder into any skills folder (for example one returned by
//! [`crate::user_skills_dirs`]) either as a symbolic link to the source or as
//! a verified copy.
//!
//! The new entry is always built beside the old one under a temporary name in
//! the same folder and then renamed into place, so an existing link or folder
//! at `<target>/<id>` is never written through: a link is replaced without
//! following it, so the folder it pointed to is never modified.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::paths::{copy_dir, is_safe_id};

/// How [`deploy_skill_entry`] places a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployMode {
    /// A symbolic link to the source folder.
    Link,
    /// A copy of the source folder, verified against the source afterwards.
    Copy,
    /// [`DeployMode::Link`] on Unix. On Windows a link is tried first and a
    /// copy is made when the link cannot be created (e.g. without the
    /// privilege to create symbolic links). Elsewhere, a copy.
    Auto,
}

/// What [`deploy_skill_entry`] placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployedEntry {
    /// The entry's path, `<target_dir>/<id>`.
    pub path: PathBuf,
    /// The mode actually used: [`DeployMode::Link`] or [`DeployMode::Copy`],
    /// never [`DeployMode::Auto`].
    pub mode: DeployMode,
}

/// Error from [`deploy_skill_entry`] or [`remove_skill_entry`].
#[derive(Debug)]
pub enum SkillEntryError {
    /// The id is not a single safe path component (see [`is_safe_id`]).
    UnsafeId(String),
    /// The source is not a directory.
    SourceNotDirectory(PathBuf),
    /// A copy was requested but the source contains a symbolic link.
    SourceContainsSymlink(PathBuf),
    /// The copied tree does not match the source.
    VerificationFailed {
        /// Path (relative to the skill folder) where the mismatch was found.
        path: PathBuf,
        /// What did not match.
        reason: String,
    },
    /// A filesystem operation failed.
    Io(io::Error),
}

impl fmt::Display for SkillEntryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillEntryError::UnsafeId(id) => write!(f, "unsafe skill id: {id:?}"),
            SkillEntryError::SourceNotDirectory(p) => {
                write!(f, "skill source is not a directory: {}", p.display())
            }
            SkillEntryError::SourceContainsSymlink(p) => write!(
                f,
                "skill source contains a symbolic link, refusing to copy: {}",
                p.display()
            ),
            SkillEntryError::VerificationFailed { path, reason } => write!(
                f,
                "copied skill does not match its source at {}: {reason}",
                path.display()
            ),
            SkillEntryError::Io(e) => write!(f, "filesystem error: {e}"),
        }
    }
}

impl Error for SkillEntryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            SkillEntryError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for SkillEntryError {
    fn from(e: io::Error) -> Self {
        SkillEntryError::Io(e)
    }
}

/// Places the skill folder `source_dir` at `<target_dir>/<id>` as a link or a
/// verified copy, replacing whatever entry was there and touching nothing
/// else in `target_dir`.
///
/// - `id` must pass [`is_safe_id`], so it is exactly one path component.
/// - `target_dir` is created if missing.
/// - The new entry is built under a temporary name (`.<id>.tmp-<random>`) in
///   `target_dir` and renamed into place. An existing symbolic link or file
///   is replaced by a single rename where the platform allows it; an
///   existing directory (or any entry the platform cannot rename over) is
///   first moved aside, the new entry renamed in, then the old one removed.
///   An existing link is never followed.
/// - A link points at `source_dir` made absolute.
/// - A copy refuses a source containing symbolic links, and is compared to
///   the source after copying (same set of files and folders, same bytes);
///   a mismatch fails the deploy and leaves the previous entry in place.
pub fn deploy_skill_entry(
    source_dir: &Path,
    target_dir: &Path,
    id: &str,
    mode: DeployMode,
) -> Result<DeployedEntry, SkillEntryError> {
    if !is_safe_id(id) {
        return Err(SkillEntryError::UnsafeId(id.to_string()));
    }
    if !source_dir.is_dir() {
        return Err(SkillEntryError::SourceNotDirectory(
            source_dir.to_path_buf(),
        ));
    }
    let source = if source_dir.is_absolute() {
        source_dir.to_path_buf()
    } else {
        std::env::current_dir()?.join(source_dir)
    };
    if mode == DeployMode::Copy {
        reject_symlinks(&source)?;
    }

    fs::create_dir_all(target_dir)?;
    let dest = target_dir.join(id);
    let tmp = target_dir.join(format!(".{id}.tmp-{}", uuid::Uuid::new_v4().simple()));

    let used = match build_entry(&source, &tmp, mode) {
        Ok(used) => used,
        Err(e) => {
            let _ = remove_entry(&tmp);
            return Err(e);
        }
    };

    if let Err(e) = swap_into_place(&tmp, &dest, target_dir, id) {
        let _ = remove_entry(&tmp);
        return Err(e.into());
    }

    Ok(DeployedEntry {
        path: dest,
        mode: used,
    })
}

/// Removes only the entry `<target_dir>/<id>`. A symbolic link is unlinked
/// without following it, so the folder it points to is left intact. Removing
/// an entry that does not exist succeeds.
pub fn remove_skill_entry(target_dir: &Path, id: &str) -> Result<(), SkillEntryError> {
    if !is_safe_id(id) {
        return Err(SkillEntryError::UnsafeId(id.to_string()));
    }
    match remove_entry(&target_dir.join(id)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other.map_err(SkillEntryError::Io),
    }
}

/// Creates the new entry at `tmp`, returning the mode actually used.
fn build_entry(source: &Path, tmp: &Path, mode: DeployMode) -> Result<DeployMode, SkillEntryError> {
    match mode {
        DeployMode::Link => {
            symlink_dir(source, tmp)?;
            Ok(DeployMode::Link)
        }
        DeployMode::Copy => {
            copy_verified(source, tmp)?;
            Ok(DeployMode::Copy)
        }
        DeployMode::Auto => {
            match symlink_dir(source, tmp) {
                Ok(()) => return Ok(DeployMode::Link),
                Err(e) if cfg!(unix) => return Err(e.into()),
                Err(_) => {}
            }
            reject_symlinks(source)?;
            copy_verified(source, tmp)?;
            Ok(DeployMode::Copy)
        }
    }
}

fn copy_verified(source: &Path, dest: &Path) -> Result<(), SkillEntryError> {
    fs::create_dir_all(dest)?;
    copy_dir(source, dest)?;
    verify_copy(source, dest)
}

/// Moves `tmp` to `dest`, replacing any existing entry at `dest`.
fn swap_into_place(tmp: &Path, dest: &Path, target_dir: &Path, id: &str) -> io::Result<()> {
    let old = match fs::symlink_metadata(dest) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return fs::rename(tmp, dest),
        Err(e) => return Err(e),
    };

    // A non-directory entry (link or file) can be replaced by one rename on
    // Unix when the new entry is not a directory either: rename replaces the
    // link itself, never its target.
    let new_is_dir = fs::symlink_metadata(tmp)?.is_dir();
    if cfg!(unix) && !old.is_dir() && !new_is_dir && fs::rename(tmp, dest).is_ok() {
        return Ok(());
    }

    // Otherwise move the old entry aside, rename the new one in, then remove
    // the old one; restore it if the new one cannot be renamed in.
    let aside = target_dir.join(format!(".{id}.old-{}", uuid::Uuid::new_v4().simple()));
    fs::rename(dest, &aside)?;
    if let Err(e) = fs::rename(tmp, dest) {
        let _ = fs::rename(&aside, dest);
        return Err(e);
    }
    remove_entry(&aside)
}

/// Removes `path` without following it if it is a symbolic link.
fn remove_entry(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        remove_symlink(path)
    } else if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(unix)]
fn symlink_dir(source: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, link)
}

#[cfg(windows)]
fn symlink_dir(source: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(source, link)
}

#[cfg(not(any(unix, windows)))]
fn symlink_dir(_source: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symbolic links are not supported on this platform",
    ))
}

#[cfg(windows)]
fn remove_symlink(path: &Path) -> io::Result<()> {
    // A directory link on Windows is removed with remove_dir; a file link
    // with remove_file. Neither follows the link.
    fs::remove_dir(path).or_else(|_| fs::remove_file(path))
}

#[cfg(not(windows))]
fn remove_symlink(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

fn reject_symlinks(source: &Path) -> Result<(), SkillEntryError> {
    for entry in WalkDir::new(source).min_depth(1) {
        let entry = entry.map_err(io::Error::from)?;
        if entry.path_is_symlink() {
            return Err(SkillEntryError::SourceContainsSymlink(
                entry.path().to_path_buf(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Dir,
    File,
    Other,
}

/// Lists every entry under `root` (relative path to kind), never following
/// links.
fn tree(root: &Path) -> io::Result<BTreeMap<PathBuf, Kind>> {
    let mut out = BTreeMap::new();
    for entry in WalkDir::new(root).min_depth(1) {
        let entry = entry.map_err(io::Error::from)?;
        let ft = entry.file_type();
        let kind = if ft.is_symlink() {
            Kind::Other
        } else if ft.is_dir() {
            Kind::Dir
        } else if ft.is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        let rel = entry
            .path()
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_path_buf();
        out.insert(rel, kind);
    }
    Ok(out)
}

fn verify_copy(source: &Path, copy: &Path) -> Result<(), SkillEntryError> {
    let src = tree(source)?;
    let dst = tree(copy)?;
    let mismatch = |path: &Path, reason: &str| SkillEntryError::VerificationFailed {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    };
    for (rel, kind) in &src {
        match dst.get(rel) {
            None => return Err(mismatch(rel, "missing from the copy")),
            Some(k) if k != kind => return Err(mismatch(rel, "different entry type")),
            Some(Kind::File) => {
                if fs::read(source.join(rel))? != fs::read(copy.join(rel))? {
                    return Err(mismatch(rel, "different contents"));
                }
            }
            Some(_) => {}
        }
    }
    if let Some(rel) = dst.keys().find(|rel| !src.contains_key(*rel)) {
        return Err(mismatch(rel, "not present in the source"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("SKILL.md"), body).unwrap();
        fs::write(dir.join("scripts/run.sh"), b"#!/bin/sh\necho hi\n").unwrap();
        dir
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn copy_deploy_is_verified_and_creates_target() {
        let tmp = TempDir::new().unwrap();
        let src = make_skill(tmp.path(), "src", "# one");
        let target = tmp.path().join("home/.agents/skills");

        let got = deploy_skill_entry(&src, &target, "my-skill", DeployMode::Copy).unwrap();
        assert_eq!(got.path, target.join("my-skill"));
        assert_eq!(got.mode, DeployMode::Copy);
        assert!(!fs::symlink_metadata(&got.path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            fs::read_to_string(got.path.join("SKILL.md")).unwrap(),
            "# one"
        );
        verify_copy(&src, &got.path).unwrap();
        // No temporary entries left behind.
        assert_eq!(names(&target), vec!["my-skill"]);
    }

    #[test]
    fn verification_detects_differences() {
        let tmp = TempDir::new().unwrap();
        let src = make_skill(tmp.path(), "src", "# one");
        let other = make_skill(tmp.path(), "other", "# two");
        assert!(matches!(
            verify_copy(&src, &other),
            Err(SkillEntryError::VerificationFailed { .. })
        ));
        fs::write(other.join("SKILL.md"), "# one").unwrap();
        verify_copy(&src, &other).unwrap();
        fs::write(other.join("extra.txt"), "x").unwrap();
        assert!(matches!(
            verify_copy(&src, &other),
            Err(SkillEntryError::VerificationFailed { .. })
        ));
    }

    #[test]
    fn unsafe_id_is_refused() {
        let tmp = TempDir::new().unwrap();
        let src = make_skill(tmp.path(), "src", "# one");
        let target = tmp.path().join("t");
        for id in ["", ".", "..", "a/b", "../x", "/abs", "a\\b"] {
            assert!(
                matches!(
                    deploy_skill_entry(&src, &target, id, DeployMode::Copy),
                    Err(SkillEntryError::UnsafeId(_))
                ),
                "{id:?}"
            );
            assert!(matches!(
                remove_skill_entry(&target, id),
                Err(SkillEntryError::UnsafeId(_))
            ));
        }
    }

    #[test]
    fn missing_source_is_refused() {
        let tmp = TempDir::new().unwrap();
        let err = deploy_skill_entry(
            &tmp.path().join("nope"),
            &tmp.path().join("t"),
            "s",
            DeployMode::Copy,
        )
        .unwrap_err();
        assert!(matches!(err, SkillEntryError::SourceNotDirectory(_)));
    }

    #[test]
    fn copy_replaces_existing_copy_and_leaves_siblings() {
        let tmp = TempDir::new().unwrap();
        let v1 = make_skill(tmp.path(), "v1", "# v1");
        let v2 = tmp.path().join("v2");
        fs::create_dir_all(&v2).unwrap();
        fs::write(v2.join("SKILL.md"), "# v2").unwrap();
        let target = tmp.path().join("t");
        let sibling = make_skill(&target, "sibling", "# sib");

        deploy_skill_entry(&v1, &target, "s", DeployMode::Copy).unwrap();
        deploy_skill_entry(&v2, &target, "s", DeployMode::Copy).unwrap();

        let s = target.join("s");
        assert_eq!(fs::read_to_string(s.join("SKILL.md")).unwrap(), "# v2");
        assert!(!s.join("scripts").exists(), "old files must not linger");
        assert_eq!(
            fs::read_to_string(sibling.join("SKILL.md")).unwrap(),
            "# sib"
        );
        assert_eq!(names(&target), vec!["s", "sibling"]);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use std::os::unix::fs::symlink;

        #[test]
        fn link_deploy_points_at_source() {
            let tmp = TempDir::new().unwrap();
            let src = make_skill(tmp.path(), "src", "# one");
            let target = tmp.path().join("t");

            let got = deploy_skill_entry(&src, &target, "s", DeployMode::Link).unwrap();
            assert_eq!(got.mode, DeployMode::Link);
            assert!(fs::symlink_metadata(&got.path)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(fs::read_link(&got.path).unwrap(), src);
            assert_eq!(names(&target), vec!["s"]);
        }

        #[test]
        fn auto_links_on_unix() {
            let tmp = TempDir::new().unwrap();
            let src = make_skill(tmp.path(), "src", "# one");
            let got =
                deploy_skill_entry(&src, &tmp.path().join("t"), "s", DeployMode::Auto).unwrap();
            assert_eq!(got.mode, DeployMode::Link);
        }

        #[test]
        fn replacing_a_link_never_writes_through_it() {
            let tmp = TempDir::new().unwrap();
            let old = make_skill(tmp.path(), "old", "# old");
            let new = make_skill(tmp.path(), "new", "# new");
            let target = tmp.path().join("t");

            // Existing link replaced by a link.
            deploy_skill_entry(&old, &target, "s", DeployMode::Link).unwrap();
            deploy_skill_entry(&new, &target, "s", DeployMode::Link).unwrap();
            assert_eq!(fs::read_link(target.join("s")).unwrap(), new);

            // Existing link replaced by a copy: the link's target is untouched.
            deploy_skill_entry(&old, &target, "s", DeployMode::Copy).unwrap();
            let s = target.join("s");
            assert!(!fs::symlink_metadata(&s).unwrap().file_type().is_symlink());
            assert_eq!(fs::read_to_string(s.join("SKILL.md")).unwrap(), "# old");
            assert_eq!(
                fs::read_to_string(new.join("SKILL.md")).unwrap(),
                "# new",
                "former link target must be unchanged"
            );
            assert!(new.join("scripts/run.sh").exists());
            assert_eq!(names(&target), vec!["s"]);
        }

        #[test]
        fn replacing_a_directory_with_a_link() {
            let tmp = TempDir::new().unwrap();
            let src = make_skill(tmp.path(), "src", "# new");
            let target = tmp.path().join("t");
            make_skill(&target, "s", "# old dir");
            let sibling = make_skill(&target, "keep", "# keep");

            deploy_skill_entry(&src, &target, "s", DeployMode::Link).unwrap();
            assert_eq!(fs::read_link(target.join("s")).unwrap(), src);
            assert_eq!(names(&target), vec!["keep", "s"]);
            assert_eq!(
                fs::read_to_string(sibling.join("SKILL.md")).unwrap(),
                "# keep"
            );
        }

        #[test]
        fn replacing_a_dangling_link() {
            let tmp = TempDir::new().unwrap();
            let src = make_skill(tmp.path(), "src", "# new");
            let target = tmp.path().join("t");
            fs::create_dir_all(&target).unwrap();
            symlink(tmp.path().join("gone"), target.join("s")).unwrap();

            deploy_skill_entry(&src, &target, "s", DeployMode::Copy).unwrap();
            assert_eq!(
                fs::read_to_string(target.join("s/SKILL.md")).unwrap(),
                "# new"
            );
            assert!(!tmp.path().join("gone").exists());
        }

        #[test]
        fn copy_refuses_source_with_symlink() {
            let tmp = TempDir::new().unwrap();
            let src = make_skill(tmp.path(), "src", "# one");
            fs::write(tmp.path().join("secret"), "s").unwrap();
            symlink(tmp.path().join("secret"), src.join("leak")).unwrap();
            let target = tmp.path().join("t");

            let err = deploy_skill_entry(&src, &target, "s", DeployMode::Copy).unwrap_err();
            assert!(matches!(err, SkillEntryError::SourceContainsSymlink(_)));
            assert!(!target.join("s").exists(), "nothing deployed");
        }

        #[test]
        fn removing_a_link_keeps_its_target_and_siblings() {
            let tmp = TempDir::new().unwrap();
            let src = make_skill(tmp.path(), "src", "# one");
            let target = tmp.path().join("t");
            let sibling = make_skill(&target, "keep", "# keep");
            deploy_skill_entry(&src, &target, "s", DeployMode::Link).unwrap();

            remove_skill_entry(&target, "s").unwrap();
            assert!(fs::symlink_metadata(target.join("s")).is_err());
            assert_eq!(fs::read_to_string(src.join("SKILL.md")).unwrap(), "# one");
            assert!(src.join("scripts/run.sh").exists());
            assert!(sibling.join("SKILL.md").exists());

            // Removing again (missing entry) succeeds.
            remove_skill_entry(&target, "s").unwrap();
        }
    }

    #[test]
    fn removing_a_copy_keeps_siblings() {
        let tmp = TempDir::new().unwrap();
        let src = make_skill(tmp.path(), "src", "# one");
        let target = tmp.path().join("t");
        make_skill(&target, "keep", "# keep");
        deploy_skill_entry(&src, &target, "s", DeployMode::Copy).unwrap();

        remove_skill_entry(&target, "s").unwrap();
        assert_eq!(names(&target), vec!["keep"]);
        assert!(src.join("SKILL.md").exists());
    }
}
