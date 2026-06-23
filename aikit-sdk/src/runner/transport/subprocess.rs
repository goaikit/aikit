//! The subprocess-stdout-lines Transport — the one Transport implemented in
//! Phase A. It spawns a Backend's CLI, writes the prompt to stdin, and streams
//! newline-delimited output back over an mpsc channel.
//!
//! The delicate drain / watchdog / reap logic stays in `runner::mod`; this
//! module owns only channel establishment (spawn + stdin + reader threads),
//! i.e. the Transport's `connect` step.

use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::runner::backend::Backend;
use crate::runner::types::{AgentEventStream, ReaderMsg, RunError, RunOptions};

/// A live subprocess connection: the child (shared with the watchdog), the
/// inbound line channel, and the reader-thread handles to join on teardown.
pub(crate) struct SubprocessConnection {
    pub child: Arc<Mutex<Child>>,
    pub rx: mpsc::Receiver<ReaderMsg>,
    pub stdout_thread: thread::JoinHandle<()>,
    pub stderr_thread: thread::JoinHandle<()>,
    /// argv used, retained for diagnostics.
    pub argv: Vec<OsString>,
}

/// Spawn the Backend's CLI, write the prompt to stdin (and close it), and start
/// reader threads for stdout/stderr.
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

    let mut child = cmd.spawn().map_err(RunError::SpawnFailed)?;

    // Write prompt and close stdin before reading output. A child that exits
    // (or closes stdin) before reading yields BrokenPipe — that is not a
    // failure: its output is still captured, so we proceed rather than abort.
    if let Some(mut stdin) = child.stdin.take() {
        match stdin.write_all(prompt.as_bytes()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                tracing::debug!(
                    target: "aikit_sdk::runner",
                    agent_key = %backend.key(),
                    "stdin closed by child before prompt write (BrokenPipe); continuing"
                );
            }
            Err(e) => return Err(RunError::StdinFailed(e)),
        }
    }

    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");

    // Wrap child in Arc<Mutex> so the watchdog thread can call child.kill()
    // while the main thread retains access for child.wait() after the loop.
    let child = Arc::new(Mutex::new(child));

    let (tx, rx) = mpsc::channel::<ReaderMsg>();
    let stdout_thread = spawn_reader_thread(stdout_pipe, AgentEventStream::Stdout, tx.clone());
    let stderr_thread = spawn_reader_thread(stderr_pipe, AgentEventStream::Stderr, tx);

    Ok(SubprocessConnection {
        child,
        rx,
        stdout_thread,
        stderr_thread,
        argv,
    })
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
