# Owned Claude context compaction

An ordinary interactive Linux Claude Code 2.1.308 run reproduces a lifecycle
mismatch: `/compact` emits a classic `SessionStart` with `source: compact`, but
does not reload the owned module or change its invocation ID. Treating that
notification as a new generation rejects the reused ID with `session_replaced`
and shows a native `SessionStart:compact hook error`. Two subsequent named,
accepted no-edit Turns still persist correctly; the warning is nevertheless a
real integration defect, not evidence of a healthy qualified session.

## Change

For a Claude installation with owned module context, acknowledge this specific
notification only after validating its recorded current invocation/revision and
current owned configuration under the existing journal reservation/deadline.
Return empty stdout, status 0 and no request ID. No synthetic SessionStart,
new binding, baseline, decision or compaction receipt is appended. Queued
operations keep their original reference. The next native decision still
revalidates normally.

Compaction cannot bootstrap an unobserved invocation. Missing/foreign/replaced
or ended context and configuration drift refuse. Ordinary startup, resume, clear,
fork and unknown source values retain existing new-start checks, including reuse
rejection. Bare legacy installations without owned invocation context retain
their earlier behavior. No public type, wire version, schema, generated module
or caller-supplied readiness option is added.

The private dispatch test seam supplies environment lookup without mutating the
test process's global environment. Production still reads only the environment
key selected by its owned installation receipt, then validates it as before.

## Validation scope

Linux Rust 1.88.0, SDK integration feature without defaults: 174 integration tests
pass. New cases cover repeated exact compaction acknowledgement, unchanged cursor
and bound reference, new-start reuse rejection, missing/foreign/unseen/replaced
and ended epochs, drift, and legacy behavior. Existing provider and deadline
tests remain in that suite. The downstream native control and fresh retest must
remain separately recorded; these unit tests are not native execution.

This is recorded-context correlation. It does not authenticate the native
process, establish liveness or effective protection, prove model consumption,
activate an application or qualify Windows. Required capabilities remain gated.
