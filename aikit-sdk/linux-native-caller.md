# Linux native caller process inspection

2026-10-24. Reuse Claude's existing inbox socket, Linux SO_PEERCRED and /proc
ancestry to establish a narrow physical relationship without a new transport or
credential. This is not an effective-policy or complete native-session witness.
Windows remains deferred and has no qualified equivalent in this change.

## Reusable public surface

With `integration` on Linux:

```rust
inspect_linux_native_caller(socket_address, expected_executable_sha256)
    -> Result<LinuxNativeCaller, IntegrationError>
```

`LinuxNativeCaller` contains observed PID, effective UID, process-start ticks and
executable SHA-256. The helper:

1. Requires an absolute filesystem socket (optional `uds:` prefix), not a leaf
   symlink, with this effective UID. Requires an independently selected lowercase
   SHA-256, not one accepted merely because the peer reports it.
2. Connects nonblocking and polls connection completion for at most 250 ms.
   Reads kernel peer credentials without sending a message or authentication line.
3. Requires the native peer PID/birth time in the caller's actual /proc ancestry,
   bounded to 32 entries. Rejects cycles, changed ancestry and inconsistent birth
   ordering. An unrelated same-user process is refused even with the same address.
4. Reads/hashes at most 512 MiB of the peer's running executable. Rechecks device/
   inode and every sampled PID/parent/start tuple after hashing.

It reuses the existing `nix` dependency's socket/poll/user features, plus existing
SHA-256. No SQLite schema, message state, process supervisor or provider registry
is added. The diagnostic `native_caller_inspect` example only wires arguments and
prints facts. The helper itself never spawns/signals a process, writes files,
reads/forwards credentials or reserves/sends a message. No writer guard should
span this inspection. The connect budget does not make filesystem/hash I/O a
hard wall-clock deadline.

These facts do **not** identify the native session ID, installed module nonce,
Turn, effective mandatory handler chain, idle lease, acknowledgement or model
input. PID/ticks are local to a boot/PID namespace, not a durable identity token.
The helper is not yet wired into required context admission and no integration
capability is promoted. Keep the existing stale-generation/journal checks and
qualification failures. Do not use process inspection to grant readiness or
authorize a later callback without renewed context validation.

## Native qualification

[Predeclared protocol](../examples/claude-mod-qualification/native-child-protocol.md).
Two fresh ordinary interactive Linux Claude 2.1.308 sessions, both with normal
terminal trust, network none, no mounts and only the public dummy key:

- Python fact baseline: `17ce1226-f327-4454-886c-e19b981afd90`.
- Actual Rust helper: `6e5468e3-b708-4217-a968-2483430c95ca`.

No model prompt was submitted. A diagnostic SessionStart invokes the child
through existing native `process.run`. Independent Python kernel/proc reads
agree with Rust on the live peer's PID/UID/start ticks and executable hash.

| Native case | Result |
| --- | --- |
| Actual native child, pinned executable | Accepted with matching physical facts |
| Actual native child, wrong expected executable hash | Refused |
| Independent same-user caller, same live socket and correct hash | Refused; independent kernel peer still identifies the original Claude process |
| Same address after normal native exit | Unavailable/refused |

Neither the token nor arbitrary environment/command arguments are captured.
Only sanitized facts and diagnostic debug/settings evidence are retained; the
temporary socket-address handoff file stays outside the evidence archive.
Settings are unchanged; native diagnostic validation passes with attribution and
directory-submission warnings. This qualifies the tested local physical relation,
not production binding/admission, effective hook enforcement or Windows.

Immutable native image:
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`.
Native executable SHA-256:
`356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58`.

Linux Rust 1.88 locked/offline: **190 integration tests pass**, including the two
new parser/input tests; the thin example builds. Existing unused-must-use test
warnings remain; no full-workspace strict lint or consumer/package gate is
claimed. The native evidence verifier and **eight altered-evidence rejection
checks** pass in Linux Node 22 and retained host Node 20. These are verifier
sensitivity checks, not eight additional native trials.

The first offline build lacked `memoffset 0.9.1`, enabled by the existing Unix
socket dependency. The public crate is fetched into a task-local writable
registry overlay and checked by Cargo against the lock checksum; the shared
cache remains read-only. The failed build and qualification lock are retained.
No owner credentials, policy, profile or permissions change.

Private parent-workspace evidence: `.local/native-child-20261024`, with
`python-final/evidence` and `rust/evidence`. Rust-native archive SHA-256:
`bf0fac10cdefb7bbef67989a54ae8717d54992fdf6faebefd8137d4945a329d6`.

```sh
cargo +1.88.0 test -p aikit-sdk --features integration --lib integration:: --locked --offline -- --test-threads=1
cargo +1.88.0 build -p aikit-sdk --features integration --example native_caller_inspect --locked --offline
node verify-native-child.mjs EVIDENCE --rust
node verify-native-child.test.mjs EVIDENCE
```

Next: bind these current process facts to the existing original-session journal
and verify revocation/replacement boundaries before enabling any physical caller
policy. Native module generation/effective policy and the separately documented
[request-bound input limitation](claude-request-snapshot-boundary.md) remain.
