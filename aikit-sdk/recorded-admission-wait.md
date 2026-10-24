# Bounded local wait before recorded admission

2026-10-24, parent SDK `a7db6b6`.

`SessionBinding::admit_recorded_with_wait(projected_through, lock_wait_budget)`
adds an explicit file-lock acquisition budget of 0–250 ms. It then performs the
same observed-session, installation, cursor and transaction checks and returns
the existing `RecordedSessionGuard`. Pending observations still require explicit
reconciliation; detached, ended and stale contexts are not admitted.

The original `admit_recorded` remains immediate/nonblocking by delegating with
zero budget. No default caller changes behavior. A successful guard still fences
journal/detach/installation writers through the consumer commit. Waiting occurs
before admission; no application mutation, native call or message is retried.
SQLite acquisition remains nonblocking. The budget is not a universal wall-clock
deadline or a native idle/session lease.

This addresses an SDK usability gap exposed while investigating a consumer
failure after ordinary Claude compaction: the first feedback `get_context`
returned a combined busy/changed-observations error. That original error does not
identify which condition occurred, so the native failure alone is not proof of
lock contention or proof that this API fixes the whole path. A fresh native
consumer trial is required. The failed original trial must remain retained.

Linux Rust 1.88 locked/offline validation: four admission tests pass, followed by
176 integration tests. The new test covers exhausted/invalid budgets, a short
writer, no observation advance, continued exclusion while holding the guard,
pending observations and detachment. Existing immediate-admission and status-wait
tests remain unchanged. Two existing unused-guard test warnings remain; stable
rustfmt reports unsupported repository configuration options. No Windows or
strict lint qualification is claimed. No schema/dependency/capability changes.

Local source/test logs: `.local/feedback-input-snapshot-20261024/` in the consumer
qualification workspace. Consumer native results are reported separately.

## Fresh consumer comparison

Codelaya [source `526077a`](https://github.com/aroff/codelaya/commit/526077a)
pins this SDK runtime `de7e171`. Twenty-four scoped consumer tests and matching
CLI/fixture builds pass. A separate fresh ordinary Claude 2.1.308 session
completes authoring, manual compaction, one frozen owner review and feedback in
the original session, preserving the original Completed Turn. Complete native
framing appears in the pre-dispatch API snapshots and all three feedback main
requests. Seven altered-evidence cases are rejected.

The [consumer report](https://github.com/aroff/codelaya/blob/526077a/docs/native-feedback-input-snapshot.md)
retains the first failed admission trial, request-classification premise failures,
exact binary identities, evidence archive hashes and limits. The positive path
supports further use of existing native APIs; it does not prove the original
failure was lock contention or establish a general production receipt.
App/SDK delivery remains Unknown, with no acknowledgement/consumption or
production readiness promotion. Windows checks remain deferred.
