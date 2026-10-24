//! Kernel peer/ancestry inspection, not a native session or policy attestation.
use super::IntegrationError;
use nix::{
    errno::Errno,
    poll::{poll, PollFd, PollFlags},
    sys::socket::{self, sockopt, AddressFamily, SockFlag, SockType, UnixAddr},
    unistd::geteuid,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    os::{
        fd::AsFd,
        unix::fs::{FileTypeExt, MetadataExt},
    },
    path::Path,
};

/// Observed Linux process relationship only. No session ID, module generation,
/// effective handler chain, idle lease or receipt authority is established.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LinuxNativeCaller {
    pub pid: u32,
    pub uid: u32,
    pub start_ticks: u64,
    pub executable_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Process {
    pid: u32,
    parent: u32,
    start: u64,
}

fn invalid() -> IntegrationError {
    IntegrationError::Invalid(
        "native Linux peer/ancestor identity is unavailable or inconsistent".into(),
    )
}

fn parse_stat(pid: u32, text: &str) -> Result<Process, IntegrationError> {
    if text
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<u32>().ok())
        != Some(pid)
    {
        return Err(invalid());
    }
    let (_, rest) = text.rsplit_once(')').ok_or_else(invalid)?;
    let fields: Vec<_> = rest.split_whitespace().collect();
    let parent = fields
        .get(1)
        .and_then(|v| v.parse().ok())
        .ok_or_else(invalid)?;
    let start = fields
        .get(19)
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    Ok(Process { pid, parent, start })
}

fn process(pid: u32) -> Result<Process, IntegrationError> {
    let mut text = String::new();
    fs::File::open(format!("/proc/{pid}/stat"))
        .map_err(|_| invalid())?
        .take(8193)
        .read_to_string(&mut text)
        .map_err(|_| invalid())?;
    if text.len() > 8192 {
        return Err(invalid());
    }
    parse_stat(pid, &text)
}

/// Inspect the calling process against an existing local socket's kernel peer.
/// The peer must have this effective UID, an independently selected executable
/// SHA-256, and a stable PID/birth time in the caller's actual /proc ancestry.
/// An unrelated same-user process with copied environment claims is refused.
///
/// Connects without sending any bytes or reading a token. Connection completion
/// is polled for at most 250 ms; filesystem/hash I/O is not a hard wall-clock
/// deadline. Reads at most 32 ancestors and 512 MiB of executable bytes. Call
/// outside journal guards. No process is spawned, signalled or retained.
///
/// This Linux primitive cannot establish which native session/module generation
/// the process owns, whether mandatory hooks ran, or delivery/consumption. It
/// grants no integration capability. The caller must obtain the expected hash
/// independently; copying a hash from the process being inspected proves none
/// of the provider/version qualification. Other platforms remain unqualified.
pub fn inspect_linux_native_caller(
    address: &str,
    expected_executable_sha256: &str,
) -> Result<LinuxNativeCaller, IntegrationError> {
    if expected_executable_sha256.len() != 64
        || !expected_executable_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    let path = Path::new(address.strip_prefix("uds:").unwrap_or(address));
    if !path.is_absolute() {
        return Err(invalid());
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid())?;
    let uid = geteuid().as_raw();
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(invalid());
    }
    let descriptor = socket::socket(
        AddressFamily::Unix,
        SockType::Stream,
        SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
        None,
    )
    .map_err(|_| invalid())?;
    let socket_address = UnixAddr::new(path).map_err(|_| invalid())?;
    use std::os::fd::AsRawFd;
    match socket::connect(descriptor.as_raw_fd(), &socket_address) {
        Ok(()) => {}
        Err(Errno::EINPROGRESS) => {
            let mut fds = [PollFd::new(descriptor.as_fd(), PollFlags::POLLOUT)];
            if poll(&mut fds, 250u16).map_err(|_| invalid())? != 1
                || socket::getsockopt(&descriptor, sockopt::SocketError).map_err(|_| invalid())?
                    != 0
            {
                return Err(invalid());
            }
        }
        Err(_) => return Err(invalid()),
    }
    let peer = socket::getsockopt(&descriptor, sockopt::PeerCredentials).map_err(|_| invalid())?;
    if peer.uid() != uid || peer.pid() <= 0 {
        return Err(invalid());
    }
    let native = process(peer.pid() as u32)?;
    inspect_ancestor(native, uid, expected_executable_sha256)
}

// Discovery needs a live native inbox. Later callbacks already have an immutable
// kernel-qualified process record, so revalidate that original process directly.
// In particular native shutdown may close the inbox before invoking SessionEnd.
pub(super) fn inspect_recorded_ancestor(
    expected: &LinuxNativeCaller,
    expected_hash: &str,
) -> Result<LinuxNativeCaller, IntegrationError> {
    let uid = geteuid().as_raw();
    if expected.uid != uid
        || expected.pid == 0
        || expected.start_ticks == 0
        || expected.executable_sha256 != expected_hash
    {
        return Err(invalid());
    }
    let native = process(expected.pid)?;
    if native.start != expected.start_ticks {
        return Err(invalid());
    }
    let actual = inspect_ancestor(native, uid, expected_hash)?;
    if &actual != expected {
        return Err(invalid());
    }
    Ok(actual)
}

fn inspect_ancestor(
    native: Process,
    uid: u32,
    expected_executable_sha256: &str,
) -> Result<LinuxNativeCaller, IntegrationError> {
    if fs::metadata(format!("/proc/{}", native.pid))
        .map_err(|_| invalid())?
        .uid()
        != uid
    {
        return Err(invalid());
    }
    let mut chain = Vec::new();
    let mut current = std::process::id();
    for _ in 0..32 {
        let value = process(current)?;
        if chain.iter().any(|old: &Process| old.pid == value.pid) {
            return Err(invalid());
        }
        current = value.parent;
        let found = value == native;
        chain.push(value);
        if found || current == 0 {
            break;
        }
    }
    if chain.last() != Some(&native) || chain.windows(2).any(|pair| pair[0].start < pair[1].start) {
        return Err(invalid());
    }
    let executable = fs::File::open(format!("/proc/{}/exe", native.pid)).map_err(|_| invalid())?;
    let before_executable = executable.metadata().map_err(|_| invalid())?;
    let mut executable = executable.take(512 * 1024 * 1024 + 1);
    let mut hash = Sha256::new();
    let mut size = 0usize;
    let mut buffer = [0; 65536];
    loop {
        let count = executable.read(&mut buffer).map_err(|_| invalid())?;
        if count == 0 {
            break;
        }
        size += count;
        if size > 512 * 1024 * 1024 {
            return Err(invalid());
        }
        hash.update(&buffer[..count]);
    }
    let actual_hash = format!("{:x}", hash.finalize());
    let after_executable =
        fs::metadata(format!("/proc/{}/exe", native.pid)).map_err(|_| invalid())?;
    if actual_hash != expected_executable_sha256
        || fs::metadata(format!("/proc/{}", native.pid))
            .map_err(|_| invalid())?
            .uid()
            != uid
        || before_executable.dev() != after_executable.dev()
        || before_executable.ino() != after_executable.ino()
        || chain
            .iter()
            .any(|old| !process(old.pid).is_ok_and(|actual| actual == *old))
    {
        return Err(invalid());
    }
    Ok(LinuxNativeCaller {
        pid: native.pid,
        uid,
        start_ticks: native.start,
        executable_sha256: actual_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_parent_and_birth_time_with_parentheses_in_process_name() {
        let tail = std::iter::once("S")
            .chain(std::iter::once("7"))
            .chain(std::iter::repeat("0").take(17))
            .chain(std::iter::once("123"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            parse_stat(42, &format!("42 (native (worker)) {tail}")).unwrap(),
            Process {
                pid: 42,
                parent: 7,
                start: 123
            }
        );
        assert!(parse_stat(43, &format!("42 (native) {tail}")).is_err());
        assert!(parse_stat(42, "42 (native) S 7").is_err());
    }
    #[test]
    fn rejects_missing_or_unqualified_endpoint_without_native_io() {
        assert!(inspect_linux_native_caller("relative", &"a".repeat(64)).is_err());
        assert!(
            inspect_linux_native_caller("/tmp/absent-aikit-native-peer", "unqualified").is_err()
        );
    }

    #[test]
    fn recorded_ancestor_refuses_changed_birth_or_uid_before_hashing() {
        let mut expected = LinuxNativeCaller {
            pid: std::process::id(),
            uid: geteuid().as_raw(),
            start_ticks: process(std::process::id()).unwrap().start + 1,
            executable_sha256: "a".repeat(64),
        };
        assert!(inspect_recorded_ancestor(&expected, &"a".repeat(64)).is_err());
        expected.start_ticks -= 1;
        expected.uid = expected.uid.wrapping_add(1);
        assert!(inspect_recorded_ancestor(&expected, &"a".repeat(64)).is_err());
    }
}
