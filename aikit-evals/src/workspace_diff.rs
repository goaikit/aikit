//! Unified diff of a scratch workspace against its seeded state.
//!
//! Every isolated trial records `workspace.diff` (fastskill spec
//! `eval-judge`, R10): what the agent wrote, taken before its scratch
//! workspace is discarded. Text files diff as unified hunks; binary and
//! oversized files are named without their contents; an untouched workspace
//! is an empty string.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Files larger than this are named, never diffed: a build tree or a
/// downloaded archive is evidence of *what* was written, not worth its bytes.
pub(crate) const MAX_TEXT_BYTES: u64 = 1024 * 1024;

/// Once the rendered diff is this large, further changed files are listed by
/// name only, so a runaway agent (`npm install` inside the workspace) cannot
/// turn the artifact into a gigabyte.
pub(crate) const MAX_DIFF_BYTES: usize = 4 * 1024 * 1024;

/// What one regular file looked like at snapshot time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileState {
    /// UTF-8 without NUL bytes and at most [`MAX_TEXT_BYTES`] long.
    Text(String),
    /// Anything else: compared by size and content hash, shown by name.
    Opaque { len: u64, digest: u64 },
}

/// Every regular file under a root, keyed by its `/`-joined relative path.
/// Symlinks are not entries and are not followed.
pub(crate) type TreeSnapshot = BTreeMap<String, FileState>;

/// Snapshot the tree under `root`.
pub(crate) fn snapshot_tree(root: &Path) -> io::Result<TreeSnapshot> {
    let mut out = TreeSnapshot::new();
    for (key, path) in list_files(root)? {
        out.insert(key, read_state(&path)?);
    }
    Ok(out)
}

/// Unified diff of the tree under `root` as it stands now against `seed`.
pub(crate) fn diff_against_seed(seed: &TreeSnapshot, root: &Path) -> io::Result<String> {
    diff_with_cap(seed, root, MAX_DIFF_BYTES)
}

fn diff_with_cap(seed: &TreeSnapshot, root: &Path, cap: usize) -> io::Result<String> {
    let now: BTreeMap<String, PathBuf> = list_files(root)?.into_iter().collect();
    let keys: BTreeSet<&String> = seed.keys().chain(now.keys()).collect();

    let mut out = String::new();
    for key in keys {
        let before = seed.get(key);
        let after = match now.get(key) {
            Some(path) => Some(read_state(path)?),
            None => None,
        };
        if before == after.as_ref() {
            continue;
        }
        if out.len() >= cap {
            out.push_str(&format!("Omitted past size cap: {key}\n"));
            continue;
        }
        render_change(&mut out, key, before, after.as_ref());
    }
    Ok(out)
}

fn render_change(
    out: &mut String,
    key: &str,
    before: Option<&FileState>,
    after: Option<&FileState>,
) {
    let a = format!("a/{key}");
    let b = format!("b/{key}");
    match (before, after) {
        (None, Some(FileState::Text(new))) => out.push_str(&hunks("", new, "/dev/null", &b)),
        (Some(FileState::Text(old)), None) => out.push_str(&hunks(old, "", &a, "/dev/null")),
        (Some(FileState::Text(old)), Some(FileState::Text(new))) => {
            out.push_str(&hunks(old, new, &a, &b))
        }
        (None, Some(_)) => out.push_str(&format!("Binary files /dev/null and {b} differ\n")),
        (Some(_), None) => out.push_str(&format!("Binary files {a} and /dev/null differ\n")),
        (Some(_), Some(_)) => out.push_str(&format!("Binary files {a} and {b} differ\n")),
        (None, None) => {}
    }
}

fn hunks(old: &str, new: &str, old_name: &str, new_name: &str) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    diff.unified_diff()
        .context_radius(3)
        .header(old_name, new_name)
        .to_string()
}

/// Regular files under `root` as `(relative key, absolute path)`, sorted by
/// key. Directories are descended; symlinks are skipped, never followed.
fn list_files(root: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                continue;
            }
            let path = entry.path();
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() {
                out.push((relative_key(root, &path), path));
            }
        }
    }
    out.sort_by(|x, y| x.0.cmp(&y.0));
    Ok(out)
}

fn relative_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn read_state(path: &Path) -> io::Result<FileState> {
    let len = std::fs::metadata(path)?.len();
    if len <= MAX_TEXT_BYTES {
        let bytes = std::fs::read(path)?;
        if bytes.contains(&0) {
            return Ok(FileState::Opaque {
                len,
                digest: digest(&bytes),
            });
        }
        return Ok(match String::from_utf8(bytes) {
            Ok(text) => FileState::Text(text),
            Err(e) => FileState::Opaque {
                len,
                digest: digest(e.as_bytes()),
            },
        });
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = DefaultHasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.write(&buf[..n]);
    }
    Ok(FileState::Opaque {
        len,
        digest: hasher.finish(),
    })
}

fn digest(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn untouched_workspace_is_an_empty_diff() {
        let ws = tempfile::tempdir().unwrap();
        write(ws.path(), "notes.txt", b"one\ntwo\n");
        write(ws.path(), "sub/blob.bin", &[0, 1, 2]);
        let seed = snapshot_tree(ws.path()).unwrap();
        assert_eq!(diff_against_seed(&seed, ws.path()).unwrap(), "");
    }

    #[test]
    fn edited_text_file_yields_a_unified_hunk() {
        let ws = tempfile::tempdir().unwrap();
        write(ws.path(), "notes.txt", b"one\ntwo\nthree\n");
        let seed = snapshot_tree(ws.path()).unwrap();
        write(ws.path(), "notes.txt", b"one\n2\nthree\n");
        let diff = diff_against_seed(&seed, ws.path()).unwrap();
        assert!(
            diff.starts_with("--- a/notes.txt\n+++ b/notes.txt\n@@"),
            "{diff}"
        );
        assert!(diff.contains("\n-two\n+2\n"), "{diff}");
    }

    #[test]
    fn added_and_removed_files_use_dev_null_headers() {
        let ws = tempfile::tempdir().unwrap();
        write(ws.path(), "gone.txt", b"bye\n");
        let seed = snapshot_tree(ws.path()).unwrap();
        std::fs::remove_file(ws.path().join("gone.txt")).unwrap();
        write(ws.path(), "new.txt", b"hello\n");
        let diff = diff_against_seed(&seed, ws.path()).unwrap();
        assert!(diff.contains("--- a/gone.txt\n+++ /dev/null\n"), "{diff}");
        assert!(diff.contains("\n-bye\n"), "{diff}");
        assert!(diff.contains("--- /dev/null\n+++ b/new.txt\n"), "{diff}");
        assert!(diff.contains("\n+hello\n"), "{diff}");
    }

    #[test]
    fn binary_files_are_named_without_their_contents() {
        let ws = tempfile::tempdir().unwrap();
        write(ws.path(), "blob.bin", &[0, 1, 2, 3]);
        let seed = snapshot_tree(ws.path()).unwrap();
        write(ws.path(), "blob.bin", &[0, 1, 2, 4]);
        write(ws.path(), "image.png", b"\x89PNG\0\0 secret-bytes");
        let diff = diff_against_seed(&seed, ws.path()).unwrap();
        assert_eq!(
            diff,
            "Binary files a/blob.bin and b/blob.bin differ\n\
             Binary files /dev/null and b/image.png differ\n"
        );
    }

    #[test]
    fn files_over_the_text_limit_are_named_not_diffed() {
        let ws = tempfile::tempdir().unwrap();
        let seed = snapshot_tree(ws.path()).unwrap();
        write(
            ws.path(),
            "big.log",
            &vec![b'a'; MAX_TEXT_BYTES as usize + 1],
        );
        let diff = diff_against_seed(&seed, ws.path()).unwrap();
        assert_eq!(diff, "Binary files /dev/null and b/big.log differ\n");
    }

    #[test]
    fn nested_paths_are_slash_joined_and_sorted() {
        let ws = tempfile::tempdir().unwrap();
        let seed = snapshot_tree(ws.path()).unwrap();
        write(ws.path(), "z/last.txt", b"z\n");
        write(ws.path(), "a/deep/first.txt", b"a\n");
        let diff = diff_against_seed(&seed, ws.path()).unwrap();
        let first = diff.find("+++ b/a/deep/first.txt\n").expect(&diff);
        let last = diff.find("+++ b/z/last.txt\n").expect(&diff);
        assert!(first < last, "{diff}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_neither_followed_nor_listed() {
        let ws = tempfile::tempdir().unwrap();
        write(ws.path(), "real.txt", b"x\n");
        let seed = snapshot_tree(ws.path()).unwrap();
        std::os::unix::fs::symlink("real.txt", ws.path().join("link.txt")).unwrap();
        std::os::unix::fs::symlink(".", ws.path().join("loop")).unwrap();
        assert_eq!(diff_against_seed(&seed, ws.path()).unwrap(), "");
    }

    #[test]
    fn past_the_size_cap_changed_files_are_listed_by_name() {
        let ws = tempfile::tempdir().unwrap();
        write(ws.path(), "kept.txt", b"k\n");
        let seed = snapshot_tree(ws.path()).unwrap();
        write(ws.path(), "a.txt", b"a\n");
        write(ws.path(), "b.txt", b"b\n");
        write(ws.path(), "c.txt", b"c\n");
        let diff = diff_with_cap(&seed, ws.path(), 1).unwrap();
        assert!(diff.contains("+++ b/a.txt\n"), "{diff}");
        assert!(!diff.contains("+++ b/b.txt"), "{diff}");
        assert!(diff.contains("Omitted past size cap: b.txt\n"), "{diff}");
        assert!(diff.contains("Omitted past size cap: c.txt\n"), "{diff}");
        assert!(
            !diff.contains("kept.txt"),
            "unchanged files are never listed: {diff}"
        );
    }
}
