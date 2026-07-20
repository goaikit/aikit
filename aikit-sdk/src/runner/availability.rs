use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::argv::{is_runnable, runnable_agents};
use super::backend::Backend;
use super::types::{AgentAvailabilityReason, AgentStatus, ChildTimeoutExt};

/// Timeout for agent availability probing in milliseconds.
pub(super) const PROBE_TIMEOUT_MS: u64 = 1500;

/// BUG-5: how long a computed `get_agent_status()` snapshot stays valid
/// before the next call re-probes every backend binary. Without this, a
/// hot loop of callers (e.g. `aikit serve`'s `POST /messages`, which
/// resolves the runnable-agent list on every request) re-spawns a
/// `--version` probe per backend candidate on every call, stalling
/// whichever executor drives it for up to ~1.5s per candidate.
const STATUS_CACHE_TTL: Duration = Duration::from_secs(45);

struct StatusCache {
    at: Instant,
    status: BTreeMap<String, AgentStatus>,
}

fn status_cache() -> &'static Mutex<Option<StatusCache>> {
    static CACHE: OnceLock<Mutex<Option<StatusCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Test-only hook: force the next [`get_agent_status`] call to re-probe,
/// regardless of TTL. Needed because the cache is a process-global static
/// and test order/parallelism must not leak a stale snapshot between tests
/// that assert on fresh probes.
#[cfg(test)]
pub(super) fn reset_status_cache_for_test() {
    if let Ok(mut guard) = status_cache().lock() {
        *guard = None;
    }
}

/// Test-only hook: count of real `probe_binary_with_timeout` invocations,
/// used to prove the TTL cache actually suppresses re-probing.
#[cfg(test)]
pub(super) static PROBE_CALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Gets the binary candidates for an agent key. Empty for unknown keys and for
/// the in-process `aikit` Backend.
pub(super) fn get_binary_candidates(agent_key: &str) -> &'static [&'static str] {
    match Backend::from_key(agent_key) {
        Some(backend) => backend.binary_candidates(),
        None => &[],
    }
}

/// Probes a binary with a --version check under timeout.
///
/// Returns Ok(true) if binary responds successfully to --version,
/// Ok(false) if binary exists but --version fails,
/// Err if binary not found or timeout occurs.
pub(super) fn probe_binary_with_timeout(binary: &str) -> Result<bool, AgentAvailabilityReason> {
    #[cfg(test)]
    PROBE_CALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    let resolved_binary = crate::command_resolve::resolve_command(binary);
    let mut cmd = Command::new(resolved_binary);
    cmd.arg("--version");
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|_| AgentAvailabilityReason::BinaryNotFound)?;

    let timeout = Duration::from_millis(PROBE_TIMEOUT_MS);

    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status.success()),
        Ok(None) => {
            // BUG-5: a timed-out probe must still be reaped, or it
            // accumulates as a zombie for the lifetime of the server.
            let _ = child.kill();
            let _ = child.wait();
            Err(AgentAvailabilityReason::TimedOut)
        }
        Err(_) => Err(AgentAvailabilityReason::BinaryNotFound),
    }
}

/// Checks if an agent is available (installed and responds to --version).
///
/// Returns false for non-runnable agents.
/// For runnable agents, probes each binary candidate and returns true
/// if any responds successfully to --version.
pub fn is_agent_available(agent_key: &str) -> bool {
    if !is_runnable(agent_key) {
        return false;
    }
    if agent_key == "aikit" {
        return true;
    }

    let candidates = get_binary_candidates(agent_key);
    for binary in candidates {
        if probe_binary_with_timeout(binary).unwrap_or(false) {
            return true;
        }
    }

    false
}

/// Gets the list of installed and available runnable agents.
///
/// Returns sorted list of agent keys that are runnable and available.
pub fn get_installed_agents() -> Vec<String> {
    let mut agents: Vec<String> = runnable_agents()
        .iter()
        .filter(|&&key: &&&str| is_agent_available(key))
        .map(|s: &&str| s.to_string())
        .collect();
    agents.sort();
    agents
}

/// Gets the status for all runnable agents.
///
/// Returns BTreeMap for stable ordering. Includes all runnable agents
/// with their availability status and reason if unavailable.
///
/// BUG-5: cached with a TTL (see [`STATUS_CACHE_TTL`]) so a hot caller
/// (e.g. `aikit serve`'s `POST /messages`, which resolves the runnable-agent
/// list on every request) does not re-spawn a `--version` probe per backend
/// candidate on every call.
pub fn get_agent_status() -> BTreeMap<String, AgentStatus> {
    {
        let guard = status_cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref cached) = *guard {
            if cached.at.elapsed() < STATUS_CACHE_TTL {
                return cached.status.clone();
            }
        }
    }
    let fresh = compute_agent_status();
    let mut guard = status_cache().lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(StatusCache {
        at: Instant::now(),
        status: fresh.clone(),
    });
    fresh
}

/// The uncached probe pass — always spawns a `--version` check per backend
/// candidate. Only [`get_agent_status`] (the cached wrapper) should be
/// called from outside this module; kept `fn` (not inlined) so the caching
/// logic above stays a thin, easily-audited wrapper.
fn compute_agent_status() -> BTreeMap<String, AgentStatus> {
    let mut status = BTreeMap::new();

    for &agent_key in runnable_agents() {
        if agent_key == "aikit" {
            status.insert(agent_key.to_string(), AgentStatus::available());
            continue;
        }

        if !is_runnable(agent_key) {
            status.insert(
                agent_key.to_string(),
                AgentStatus::unavailable(AgentAvailabilityReason::NotRunnable),
            );
            continue;
        }

        let candidates = get_binary_candidates(agent_key);
        let mut available = false;
        let mut last_error = AgentAvailabilityReason::BinaryNotFound;

        for binary in candidates {
            match probe_binary_with_timeout(binary) {
                Ok(true) => {
                    available = true;
                    break;
                }
                Ok(false) => {
                    last_error = AgentAvailabilityReason::VersionCheckFailed;
                }
                Err(e) => {
                    last_error = e;
                }
            }
        }

        if available {
            status.insert(agent_key.to_string(), AgentStatus::available());
        } else {
            status.insert(agent_key.to_string(), AgentStatus::unavailable(last_error));
        }
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::argv::runnable_agents;
    use std::sync::atomic::Ordering;
    use std::sync::Mutex as StdMutex;

    /// Serializes tests that touch the process-global status cache /
    /// probe-call counter so they don't race each other under `cargo test`'s
    /// default parallel execution within this binary.
    static CACHE_TEST_LOCK: StdMutex<()> = StdMutex::new(());

    // --- BUG-5: availability is cached across calls within the TTL ---

    #[test]
    fn test_get_agent_status_is_cached_across_calls() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_status_cache_for_test();
        PROBE_CALL_COUNT.store(0, Ordering::SeqCst);

        let first = get_agent_status();
        let probes_after_first = PROBE_CALL_COUNT.load(Ordering::SeqCst);
        assert!(
            probes_after_first > 0,
            "the first call must actually probe at least one backend"
        );

        let second = get_agent_status();
        let probes_after_second = PROBE_CALL_COUNT.load(Ordering::SeqCst);
        assert_eq!(
            probes_after_second, probes_after_first,
            "a second call within the TTL must be served from cache, not re-probe"
        );
        assert_eq!(first, second, "cached snapshot must match the fresh one");
    }

    #[test]
    fn test_get_agent_status_reprobes_after_cache_reset() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_status_cache_for_test();
        PROBE_CALL_COUNT.store(0, Ordering::SeqCst);

        let _ = get_agent_status();
        let probes_after_first = PROBE_CALL_COUNT.load(Ordering::SeqCst);

        // Simulate TTL expiry by forcing the cache to be treated as stale.
        reset_status_cache_for_test();
        let _ = get_agent_status();
        let probes_after_second = PROBE_CALL_COUNT.load(Ordering::SeqCst);

        assert!(
            probes_after_second > probes_after_first,
            "a cache-reset call must re-probe rather than reuse a stale snapshot"
        );
    }

    #[test]
    fn test_is_agent_available_false_for_non_runnable() {
        assert!(!is_agent_available("copilot"));
        assert!(!is_agent_available("cursor-agent"));
        assert!(!is_agent_available("unknown"));
    }

    #[test]
    fn test_get_agent_status_keys_match_runnable_agents() {
        let status = get_agent_status();
        let runnable_set: std::collections::HashSet<_> =
            runnable_agents().iter().copied().collect();
        let status_keys: std::collections::HashSet<_> = status.keys().map(|s| s.as_str()).collect();
        assert_eq!(runnable_set, status_keys);
    }

    #[test]
    fn test_get_installed_agents_is_subset_of_runnable_agents() {
        let installed = get_installed_agents();
        let runnable_set: std::collections::HashSet<_> =
            runnable_agents().iter().copied().collect();
        for agent in &installed {
            assert!(runnable_set.contains(agent.as_str()));
        }
    }

    #[test]
    fn test_get_installed_agents_sorted() {
        let installed = get_installed_agents();
        let mut sorted_installed = installed.clone();
        sorted_installed.sort();
        assert_eq!(installed, sorted_installed);
    }

    #[test]
    fn test_unavailable_statuses_have_reason() {
        let status = get_agent_status();
        for (agent_key, agent_status) in &status {
            if !agent_status.available {
                assert!(
                    agent_status.reason.is_some(),
                    "Agent {} is unavailable but has no reason",
                    agent_key
                );
            }
        }
    }

    #[test]
    fn test_binary_candidates_mapping() {
        assert_eq!(get_binary_candidates("codex"), &["codex"] as &[&str]);
        assert_eq!(get_binary_candidates("claude"), &["claude"]);
        assert_eq!(get_binary_candidates("gemini"), &["gemini"]);
        assert_eq!(
            get_binary_candidates("opencode"),
            &["opencode", "opencode-desktop"]
        );
        assert_eq!(
            get_binary_candidates("cursor"),
            &["cursor-agent", "agent"] as &[&str]
        );
        assert!(get_binary_candidates("agent").is_empty()); // renamed (ADR 0006)
        assert!(get_binary_candidates("aikit").is_empty()); // in-process
        assert!(get_binary_candidates("unknown").is_empty());
    }

    #[test]
    fn test_agent_status_available() {
        let status = AgentStatus::available();
        assert!(status.available);
        assert!(status.reason.is_none());
    }

    #[test]
    fn test_agent_status_unavailable() {
        let status = AgentStatus::unavailable(AgentAvailabilityReason::BinaryNotFound);
        assert!(!status.available);
        assert_eq!(status.reason, Some(AgentAvailabilityReason::BinaryNotFound));
    }

    #[test]
    fn test_agent_availability_reason_display() {
        assert_eq!(
            format!("{}", AgentAvailabilityReason::NotRunnable),
            "not_runnable"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::BinaryNotFound),
            "binary_not_found"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::VersionCheckFailed),
            "version_check_failed"
        );
        assert_eq!(
            format!("{}", AgentAvailabilityReason::TimedOut),
            "timed_out"
        );
    }
}
