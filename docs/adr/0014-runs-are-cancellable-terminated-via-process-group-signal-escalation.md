# Runs are cancellable; termination escalates SIGTERM→SIGKILL over the process group

## Status

accepted

## Context

aikit had no real notion of *stopping* a run. The subprocess Transport spawned the agent
CLI without process-group isolation, and the only termination path was a watchdog calling
`child.kill()` on the direct child (`aikit-sdk/src/runner/mod.rs:260-266`). Three P1 bugs
share this root (audit `specs/ISSUES-2026-07-07.md`):

- **BUG-1** — the prompt is written to stdin on the calling thread *before* the reader
  threads and watchdog exist, so a large prompt (the documented "use stdin to avoid
  ARG_MAX" path) deadlocks against a full stdout pipe with no timeout armed.
- **BUG-2** — on timeout the serve layer emits a `run_timeout` event but does not stop the
  run or close the response; the record is set `Idle` while the run keeps executing, so a
  new turn passes the busy check and two runs execute on one session, corrupting the
  session file.
- **BUG-4** — `child.kill()` targets only the direct child; grandchildren (tools the agent
  spawned) are orphaned and keep running, and if any inherited the stdout/stderr write end
  the reader threads never see EOF and the run *never returns*.

## Decision

**1. stdin is written off the calling thread, after readers exist and the watchdog is
armed.** The subprocess Transport spawns the stdout/stderr reader threads first, then
writes the prompt on a dedicated writer thread (dropping `ChildStdin` to signal EOF), and
the run timeout is armed before the write can block. This removes the BUG-1 deadlock.

**2. Cancellation is a first-class primitive.** A single cancel token is threaded into a
run and can be triggered by (a) the run timeout, (b) client disconnect, and (c) a future
explicit interrupt — one mechanism, not three ad-hoc kill paths. This token is the seam
that ARCH-3's `ControlHandle` later subsumes; it is built now to fix the P1s and is not
throwaway.

**3. Termination escalates over the process group.** Agent CLIs are spawned in their own
process group (`setsid` / `process_group(0)`). Cancellation sends `SIGTERM` to the group,
waits a short grace (~3s) so a well-behaved CLI can flush a session/checkpoint, then
`SIGKILL`s the group. `kill_on_drop(true)` backstops dropped handles. Killing the *group*
reaps grandchildren and guarantees the pipes reach EOF, so the run always returns.

**4. On termination the run reaches a terminal state and the response closes.** The record
becomes `Failed`/`Closed` (never `Idle`), the channel is closed so both sync-drain and SSE
clients unblock promptly, and a terminal error frame (e.g. `run_timeout`) is the last thing
the client sees — no late frames from a zombie run, and the busy check cannot admit a
second concurrent run on the same session.

## Consequences

- BUG-1, BUG-2, BUG-4, and the client-disconnect leak in BUG-7 are fixed by one shared
  mechanism rather than four point patches.
- The timeout test must exercise a *real* slow run, not the current instant stub, or the
  behaviour stays unverified.
- The cancel token is forward-compatible with the `Session`/`ControlHandle` trait from the
  ARCH-3 direction; when that lands, cancellation becomes one control op among several
  (interrupt, set-permission, disconnect) over the same seam.
- Process-group spawning is Unix-shaped (`setsid`); the Windows path (job objects) is noted
  as follow-up but not required for the primary sandboxed-Linux deployment.
