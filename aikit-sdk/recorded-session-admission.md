# Recorded session admission

`SessionBinding::admit_recorded(projected_through)` returns an owned
`RecordedSessionGuard`. This closes a recorded journal check/commit race for
application consumers. It does **not** authenticate the process invoking the
application, or establish native liveness, notification delivery or control.

## Existing mechanisms reused

The guard acquires the installer's existing writer lock, then reserves a SQLite
write transaction on the existing integration database. It checks installation
ownership/configuration, binding revocation and the full recorded generation
under those reservations. It refuses a projection cursor ahead of the journal
and any relevant observation newer than the application's projected cursor.
Foreign sessions do not supply the selected binding's observation evidence.

No new database, table, credential, provider protocol or schema is introduced.
Drop explicitly rolls back the reservation before releasing the installation
lock, including during unwinding. Busy acquisition refuses instead of waiting
for an application callback. Native journal and detach writers cannot commit
while the guard is held, and installation writers cannot replace the receipt.

## Consumer order

1. Hold the application's normal operation guard and reconcile observations.
2. Prepare data and stage the application SQLite transaction. Do not hold the
   SDK reservation while waiting for application SQLite or doing Git/file work.
3. Acquire recorded admission using the projected cursor. Failure rolls back the
   staged application transaction. Reconcile before retrying the same operation.
4. Commit the application transaction while retaining the returned guard.
5. Drop the guard before more SDK calls. For immutable-result replay or a read,
   use the guard only for the final admission validation before returning.

The guard does not commit application data, supply an exactly-once transport or
turn a stored callback decision into native acceptance. Public status/installer
methods also take the installer lock and must not be called under this guard.

Native writers retain their existing deadlines. This mechanism does not buffer
notifications: long application commits can create native contention, so callers
must keep the reservation short. Effective native timing, process/generation
authentication and notification recovery still require qualification. External
programs editing settings directly do not honor SDK locks; readiness must not be
promoted from this recorded-state primitive.

## Controlled validation

Rust 1.88 integration suites passed 81 tests on native Windows and 83 in isolated
Linux Docker. Three added cases exercise journal/detach/installer exclusion,
pending observations, ahead cursors, ended/stale/detached bindings and unwind
release. An initial nested installer-lock acquisition failed these new tests;
status validation now reuses the guarded connection/internal status path.

```text
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked --offline integration::
```

Use the dependency-lock reproduction in [completion-policy.md](completion-policy.md).
These suites use contract fixtures, not real native caller authentication or
interactive Windows sessions.

## Bounded-wait follow-up

The additive [bounded local admission wait](recorded-admission-wait.md) lets an
explicit caller wait up to 250 ms for the existing file lock before the same
cursor/lifecycle checks. The original nonblocking method and the guarantees
described above remain unchanged. Neither method is a native admission lease.
