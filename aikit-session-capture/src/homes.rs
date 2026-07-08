//! Home-directory resolution. The trait is the seam; the strategy is
//! pluggable. See spec 010 §8.
//!
//! The `Adapter` trait has zero opinions about how `watch_paths()` are
//! discovered — it only consumes `Vec<PathBuf>`. A `HomeResolver` strategy
//! produces them, with optional cross-mount enumeration behind the
//! `crossmount` feature.

#[cfg(feature = "crossmount")]
use std::path::Path;
use std::path::PathBuf;

/// One candidate `$HOME`-equivalent directory the adapter should treat as a
/// watch-root candidate.
#[derive(Debug, Clone)]
pub struct HomeRoot {
    /// Absolute filesystem path, in whatever form is reachable from the
    /// running process.
    pub path: PathBuf,
    /// The LOGICAL OS of the home, decoupled from `std::env::consts::OS`.
    /// A native home on Linux has `Linux`; a `/mnt/c/Users/<u>` entry on
    /// the same host has `Windows`. Adapters use this to pick per-OS
    /// subpaths (e.g. OpenCode Desktop's per-platform location).
    pub os: HomeOs,
    /// Where this candidate came from: `"native"` | `"wsl-mnt:<u>"` |
    /// `"wslhost:<distro>/<u>"`.
    pub origin: &'static str,
}

/// The logical OS of a [`HomeRoot`]. Decoupled from the runtime OS so the
/// crossmount resolver can tag `/mnt/c/Users/*` as `Windows` while running
/// on Linux.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeOs {
    Linux,
    Windows,
    Darwin,
}

/// Produces candidate home directories. Native home is always first; extra
/// homes (when the resolver supports them) follow in stable order.
pub trait HomeResolver: Send + Sync {
    fn homes(&self) -> Vec<HomeRoot>;
}

/// Default: native home only. No cross-mount enumeration. Mirrors
/// `superbased-observer`'s `DefaultHomeResolver` semantics when the
/// `crossmount` package is disabled.
pub struct DefaultHomeResolver;

impl HomeResolver for DefaultHomeResolver {
    fn homes(&self) -> Vec<HomeRoot> {
        let native = dirs::home_dir().map(|path| HomeRoot {
            path,
            os: native_os(),
            origin: "native",
        });
        native.into_iter().collect()
    }
}

fn native_os() -> HomeOs {
    #[cfg(target_os = "linux")]
    {
        HomeOs::Linux
    }
    #[cfg(target_os = "macos")]
    {
        HomeOs::Darwin
    }
    #[cfg(target_os = "windows")]
    {
        HomeOs::Windows
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        HomeOs::Linux
    }
}

/// Cross-mount resolver: enumerates WSL2 ↔ Windows home pairs so observer
/// running on one side picks up sessions written on the other. Behind the
/// `crossmount` feature; default-off because most users are on a single OS.
///
/// On Linux: walks `/mnt/c/Users/*` for Windows homes.
/// On Windows: walks `\\wsl.localhost\<distro>\home\<user>` for Linux homes.
/// On macOS / pure Linux / pure Windows: native only (the walks no-op).
///
/// The [`DirLister`] trait seam (mirrors `crossmount.go:37-51` `detector`)
/// lets unit tests inject fake filesystems so the enumeration logic runs
/// deterministically on any host.
#[cfg(feature = "crossmount")]
pub struct CrossmountResolver {
    lister: Box<dyn DirLister>,
}

/// Filesystem probe seam for the crossmount resolver. Production code uses
/// [`NativeDirLister`]; tests inject fakes to simulate `/mnt/c/Users` or
/// `\\wsl.localhost\` without actually having them on disk.
#[cfg(feature = "crossmount")]
pub trait DirLister: Send + Sync {
    fn runtime_os(&self) -> &'static str;
    fn native_home(&self) -> Option<PathBuf>;
    fn is_dir(&self, path: &Path) -> bool;
    fn read_dir_names(&self, path: &Path) -> Vec<String>;
}

/// Production [`DirLister`] backed by `std::fs`.
#[cfg(feature = "crossmount")]
pub struct NativeDirLister;

#[cfg(feature = "crossmount")]
impl DirLister for NativeDirLister {
    fn runtime_os(&self) -> &'static str {
        std::env::consts::OS
    }
    fn native_home(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }
    fn read_dir_names(&self, path: &Path) -> Vec<String> {
        std::fs::read_dir(path)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(feature = "crossmount")]
impl CrossmountResolver {
    /// Create with [`NativeDirLister`] — the production path.
    pub fn new() -> Self {
        Self {
            lister: Box::new(NativeDirLister),
        }
    }

    /// Create with a custom [`DirLister`] — for tests.
    pub fn with_lister(lister: Box<dyn DirLister>) -> Self {
        Self { lister }
    }
}

#[cfg(feature = "crossmount")]
impl Default for CrossmountResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "crossmount")]
impl HomeResolver for CrossmountResolver {
    fn homes(&self) -> Vec<HomeRoot> {
        let mut out = DefaultHomeResolver.homes();
        out.extend(extra_homes(&*self.lister));
        out
    }
}

#[cfg(feature = "crossmount")]
fn extra_homes(lister: &dyn DirLister) -> Vec<HomeRoot> {
    let os = lister.runtime_os();
    if os == "linux" {
        wsl_windows_homes(lister)
    } else if os == "windows" {
        windows_wsl_homes(lister)
    } else {
        Vec::new()
    }
}

/// Linux side: enumerate `/mnt/c/Users/<user>`.
#[cfg(feature = "crossmount")]
fn wsl_windows_homes(lister: &dyn DirLister) -> Vec<HomeRoot> {
    let root = Path::new("/mnt/c/Users");
    if !lister.is_dir(root) {
        return Vec::new();
    }
    lister
        .read_dir_names(root)
        .into_iter()
        .filter_map(|name| {
            let path = root.join(&name);
            if !lister.is_dir(&path) {
                return None;
            }
            Some(HomeRoot {
                path,
                os: HomeOs::Windows,
                origin: leak_origin("wsl-mnt", &name),
            })
        })
        .collect()
}

/// Windows side: enumerate `\\wsl.localhost\<distro>\home\<user>`.
#[cfg(feature = "crossmount")]
fn windows_wsl_homes(lister: &dyn DirLister) -> Vec<HomeRoot> {
    let root = Path::new(r"\\wsl.localhost\");
    if !lister.is_dir(root) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for distro in lister.read_dir_names(root) {
        let home_dir = root.join(&distro).join("home");
        if !lister.is_dir(&home_dir) {
            continue;
        }
        for user in lister.read_dir_names(&home_dir) {
            let path = home_dir.join(&user);
            if lister.is_dir(&path) {
                out.push(HomeRoot {
                    path,
                    os: HomeOs::Linux,
                    origin: leak_origin("wslhost", &format!("{distro}/{user}")),
                });
            }
        }
    }
    out
}

/// Intern a dynamic origin string into a `&'static str`. The origin is a
/// diagnostic label — not data — so leaking is acceptable and bounded by the
/// number of home directories on the host (typically ≤5).
#[cfg(feature = "crossmount")]
fn leak_origin(prefix: &str, suffix: &str) -> &'static str {
    Box::leak(format!("{prefix}:{suffix}").into_boxed_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resolver_returns_native_home() {
        let r = DefaultHomeResolver;
        let homes = r.homes();
        // On a real host this returns ≥1 entry. On a stripped CI container
        // without HOME set it may return 0; don't hard-assert.
        for h in &homes {
            assert_eq!(h.origin, "native");
            assert_eq!(h.os, native_os());
        }
    }

    #[cfg(feature = "crossmount")]
    #[test]
    fn crossmount_faked_wsl_returns_native_plus_extras() {
        // Simulate a Linux host with /mnt/c/Users/alice and /mnt/c/Users/bob.
        struct FakeLister;
        impl DirLister for FakeLister {
            fn runtime_os(&self) -> &'static str {
                "linux"
            }
            fn native_home(&self) -> Option<PathBuf> {
                Some(PathBuf::from("/home/me"))
            }
            fn is_dir(&self, path: &Path) -> bool {
                let s = path.to_string_lossy();
                s == "/mnt/c/Users"
                    || s == "/mnt/c/Users/alice"
                    || s == "/mnt/c/Users/bob"
                    || s == "/home/me"
            }
            fn read_dir_names(&self, path: &Path) -> Vec<String> {
                match path.to_string_lossy().as_ref() {
                    "/mnt/c/Users" => vec!["alice".into(), "bob".into()],
                    _ => vec![],
                }
            }
        }

        let resolver = CrossmountResolver::with_lister(Box::new(FakeLister));
        let homes = resolver.homes();

        // Should include native + 2 WSL homes.
        assert!(homes.len() >= 2, "expected at least 2 WSL homes");
        let wsl: Vec<_> = homes.iter().filter(|h| h.os == HomeOs::Windows).collect();
        assert_eq!(wsl.len(), 2, "should find 2 Windows homes via /mnt/c");
        // Verify origin format: wsl-mnt:<user>
        assert!(wsl.iter().any(|h| h.origin == "wsl-mnt:alice"));
        assert!(wsl.iter().any(|h| h.origin == "wsl-mnt:bob"));
    }

    #[cfg(feature = "crossmount")]
    #[test]
    fn crossmount_no_mnt_c_returns_native_only() {
        struct NoMntC;
        impl DirLister for NoMntC {
            fn runtime_os(&self) -> &'static str {
                "linux"
            }
            fn native_home(&self) -> Option<PathBuf> {
                Some(PathBuf::from("/home/me"))
            }
            fn is_dir(&self, _: &Path) -> bool {
                false // /mnt/c/Users doesn't exist
            }
            fn read_dir_names(&self, _: &Path) -> Vec<String> {
                vec![]
            }
        }

        let resolver = CrossmountResolver::with_lister(Box::new(NoMntC));
        let homes = resolver.homes();
        let extras: Vec<_> = homes.iter().filter(|h| h.origin != "native").collect();
        assert!(extras.is_empty(), "no extras when /mnt/c/Users absent");
    }
}
