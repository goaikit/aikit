# Recorded status under short journal contention

2026-10-23. Source baseline `b25493e`; this change is SDK-local behavior, not a
native provider capability qualification.

`SessionBinding::status()` previously failed immediately when a competing SDK
journal writer held `integration.lock`. Codelaya's original verification also
mislabeled that error as Scope; its separate `016e63e` fix now reports unreadable
state as Unavailable. That deterministic defect does not establish the cause of
the earlier intermittent Claude feedback failure.

Status now waits up to 250 ms **for lock acquisition only**, reusing the existing
bounded lock helper already used for retaining native handoff returns. A writer
released within the budget permits an ordinary fresh status read. Persistent
contention remains an error; detach/replacement/end are never assumed away.
SQLite/filesystem read time is separate; this is not a hard hook deadline or a
native idle lease. No operation, callback, transport or native send is retried.

`admit_recorded` still uses the zero-wait writer guard and refuses contention.
Mutations still require their original scope/revision/cursor checks. No public
method, serialized field, journal schema, dependency or capability report changes.

## Validation

Linux x86-64, Rust 1.88, existing workspace Cargo.lock, offline:

```sh
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked --offline status_waits_for_a_short_writer_but_admission_still_refuses_immediately
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked --offline integration::
```

The regression failed on the original immediate status error, then passed after
the change. A real recorded-admission guard occupies both existing locks. Another
thread's mutation admission is refused; its status call waits until the writer
is released. A separate held writer exhausts the budget and still fails closed.
After release/detach, status returns Detached rather than a cached Observed value.
All 153 integration tests pass; this is not the entire SDK suite. Two existing
unused-guard warnings in `install_tests.rs` are retained. The initial archive
omitted ignored Cargo.lock and the first test draft lacked a test-only import;
neither preparation failure counts as a regression or pass. The existing lockfile
was then copied unchanged, and compilation/tests ran locked and offline.

Strict production Clippy is **not green**. The first invocation failed on 17
style errors in unchanged `aikit-agent` dependency code. `--no-deps` then failed
on 101 warnings treated as errors elsewhere in the SDK (including legacy
formatting in `pipeline.rs`). Logs for both attempts are retained; no lint pass
or cleanup of that unrelated code is claimed. Consumer validation is separate.

Stable rustfmt 1.88 emits warnings for the repository's nightly-only formatting
options. Only the new regression's formatting was carried back to the source;
unrelated formatting was preserved. Raw logs are local under
`.local/sdk-status-contention-20261023/` in the Codelaya task workspace.

Codelaya must pin the published revision and rerun affected consumer tests before
consuming this change. Earlier native runs retain their original SDK hashes and
do not qualify the new revision. Native authority, effective policy, atomic idle
admission and authoritative acknowledgement/consumption remain separate blockers.
