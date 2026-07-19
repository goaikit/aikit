//! The subprocess-stdout-lines Transport — the one Transport implemented in
//! Phase A. It spawns a Backend's CLI, writes the prompt to stdin, and streams
//! newline-delimited output back over an mpsc channel.
//!
//! The delicate drain / watchdog / reap logic stays in `runner::mod`; this
//! module owns channel establishment (spawn + reader threads + stdin writer
//! thread), i.e. the Transport's `connect` step, plus the Unix process-group
//! kill escalation ([`kill_process_group`]) that both the timeout watchdog
//! and the external [`crate::runner::RunCancelHandle`] drive (ADR 0014).

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::runner::backend::Backend;
use crate::runner::types::{AgentEventStream, ReaderMsg, RunError, RunOptions};

/// A live subprocess connection: the child (shared with the watchdog/cancel
/// handle), the inbound line channel, and the reader/writer-thread handles to
/// join on teardown.
pub(crate) struct SubprocessConnection {
    pub child: Arc<Mutex<Child>>,
    pub rx: mpsc::Receiver<ReaderMsg>,
    pub stdout_thread: thread::JoinHandle<()>,
    pub stderr_thread: thread::JoinHandle<()>,
    /// Writes the prompt to the child's stdin and drops it (EOF) when done.
    /// Runs on its own thread so `connect` never blocks on a full pipe (see
    /// BUG-1 note on [`connect`]). `Ok(())` also covers a `BrokenPipe` from a
    /// child that closed stdin early — that is not a failure.
    pub stdin_thread: thread::JoinHandle<io::Result<()>>,
    /// argv used, retained for diagnostics.
    pub argv: Vec<OsString>,
}

/// Spawn the Backend's CLI, start the stdout/stderr reader threads, and start
/// the stdin writer thread (which writes the prompt and then drops stdin to
/// signal EOF).
///
/// Ordering matters (BUG-1 / ADR 0014): the prompt is NOT written on this
/// (the calling) thread before returning. A large prompt (the documented
/// "use stdin to avoid ARG_MAX" path) can exceed the OS pipe buffer before
/// the child has consumed it; if the child also produces enough output to
/// fill *its* stdout/stderr pipe before draining stdin, a synchronous
/// caller-thread write deadlocks against nothing reading that output. So the
/// stdout/stderr readers are spawned FIRST (draining output as soon as the
/// child produces it), and the prompt write happens on its own dedicated
/// thread. This also lets `connect` return almost immediately, so the
/// caller's run timeout / cancel watchdog is armed before the write could
/// possibly block for any meaningful duration.
pub(crate) fn connect(
    backend: Backend,
    prompt: &str,
    options: &RunOptions,
    events_mode: bool,
) -> Result<SubprocessConnection, RunError> {
    debug_assert!(
        !backend.is_in_process(),
        "subprocess transport called for in-process backend"
    );

    let sid = options.session_id.as_deref();
    let argv = crate::runner::argv::build_argv(
        backend.key(),
        options.model.as_ref(),
        options.yolo,
        options.stream,
        events_mode,
        sid,
    );

    let argv_display: Vec<String> = argv
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    tracing::debug!(
        target: "aikit_sdk::runner",
        agent_key = %backend.key(),
        argv = ?argv_display,
        cwd = ?options.current_dir.as_ref().map(|p| p.display().to_string()),
        timeout = ?options.timeout.map(|d| format!("{}s", d.as_secs())),
        events_mode,
        yolo = options.yolo,
        stream = options.stream,
        "spawning agent child process"
    );

    let binary = &argv[0];
    let args = &argv[1..];

    let resolved_program = crate::command_resolve::resolve_command(&binary.to_string_lossy());
    tracing::debug!(
        target: "aikit_sdk::runner",
        resolved_program = ?resolved_program,
        "resolved executable path"
    );
    let mut cmd = Command::new(resolved_program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ref dir) = options.current_dir {
        cmd.current_dir(dir);
    }

    // BUG-4 (ADR 0014): spawn the agent CLI as the leader of its own process
    // group. Termination is escalated over the whole group (see
    // `kill_process_group` below), which reaps grandchildren (tools the
    // agent spawned) and guarantees the pipes reach EOF so a run can never
    // hang on an orphaned descendant.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let mut child = cmd.spawn().map_err(RunError::SpawnFailed)?;

    let stdin_pipe = child.stdin.take().expect("stdin was piped");
    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    // Wrap child in Arc<Mutex> so the watchdog/cancel-handle thread can kill
    // the process group while the main thread retains access for
    // child.wait() after the drain loop.
    let child = Arc::new(Mutex::new(child));

    // Readers first (see the `connect` doc comment for why this ordering is
    // load-bearing): stdout/stderr are actively drained from this point on.
    let (tx, rx) = mpsc::channel::<ReaderMsg>();
    let stdout_thread = spawn_reader_thread(stdout_pipe, AgentEventStream::Stdout, tx.clone());
    let stderr_thread = spawn_reader_thread(stderr_pipe, AgentEventStream::Stderr, tx);

    // Prompt write happens on its own thread, off the calling thread. A
    // child that exits (or closes stdin) before reading the full prompt
    // yields BrokenPipe — that is not a failure: its output is still
    // captured, so the thread reports success rather than an error.
    let agent_key = backend.key();
    let prompt_owned = prompt.to_string();
    let stdin_thread = thread::spawn(move || -> io::Result<()> {
        let mut stdin = stdin_pipe;
        let result = match stdin.write_all(prompt_owned.as_bytes()) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                tracing::debug!(
                    target: "aikit_sdk::runner",
                    agent_key,
                    "stdin closed by child before prompt write (BrokenPipe); continuing"
                );
                Ok(())
            }
            Err(e) => Err(e),
        };
        // `stdin` drops here regardless of outcome, closing the write end
        // and signalling EOF to the child.
        result
    });

    Ok(SubprocessConnection {
        child,
        rx,
        stdout_thread,
        stderr_thread,
        stdin_thread,
        argv,
    })
}

/// Escalate termination of `child`'s entire process group: `SIGTERM`, then
/// (after a ~3s grace period so a well-behaved CLI can flush a
/// session/checkpoint) `SIGKILL`. Killing the *group* — not just the direct
/// child — reaps grandchildren the agent spawned and guarantees the child's
/// stdout/stderr pipes reach EOF, so the reader threads always finish and a
/// run never hangs (BUG-4 / ADR 0014). Idempotent and safe to call on an
/// already-exited child (signals to a dead/reused pgid are best-effort and
/// errors are ignored).
///
/// Shared by the run timeout watchdog and [`crate::runner::RunCancelHandle`]
/// — the one mechanism ADR 0014 requires in place of ad-hoc kill paths.
#[cfg(unix)]
pub(crate) fn kill_process_group(child: &Arc<Mutex<Child>>) {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};

    let pid = {
        let guard = child.lock().unwrap_or_else(|e| e.into_inner());
        guard.id() as i32
    };
    if pid <= 0 {
        return;
    }
    // `process_group(0)` at spawn made the child the leader of its own
    // group, so its pid IS the process-group id; negating targets the group.
    let pgid = Pid::from_raw(-pid);
    let _ = kill(pgid, Signal::SIGTERM);

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        {
            let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(guard.try_wait(), Ok(Some(_))) {
                return;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let _ = kill(pgid, Signal::SIGKILL);
    let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
    let _ = guard.wait();
}

/// Non-Unix fallback: kill the direct child only. Process-group / job-object
/// semantics on Windows are noted in ADR 0014 as follow-up, not required for
/// the primary sandboxed-Linux deployment.
#[cfg(not(unix))]
pub(crate) fn kill_process_group(child: &Arc<Mutex<Child>>) {
    let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
}

/// Spawns a reader thread that reads lines (delimited by `\n`) from `reader`
/// and sends raw byte chunks (including the newline) to `tx`.
/// Non-UTF-8 and partial final lines are sent as-is.
/// I/O errors are sent as `ReaderMsg::Err` and the thread exits.
fn spawn_reader_thread<R>(
    reader: R,
    stream: AgentEventStream,
    tx: mpsc::Sender<ReaderMsg>,
) -> thread::JoinHandle<()>
where
    R: io::Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = io::BufReader::new(reader);
        let mut buf: Vec<u8> = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if tx
                        .send(ReaderMsg::Chunk {
                            stream,
                            raw: buf.clone(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(ReaderMsg::Err { stream, source: e });
                    break;
                }
            }
        }
    })
}
