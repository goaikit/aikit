# Durable Linux native caller association

2026-10-24. Follow-up to [physical caller inspection](linux-native-caller.md).
Reusable library implementation, disposable ordinary-Claude qualification and
schema downgrade protection. Windows checks remain deferred. No readiness,
delivery, acknowledgement or consumption capability is promoted.

## Public surface and semantics

### Provider-neutral consumer entry points

The follow-up introduces `NativeCallerPolicy::LinuxExecutable { sha256 }`, with
bounded `read`/validation. The executable identity is independently owner-selected,
never learned from request data or copied from the inspected process. AIKit owns
provider/environment resolution; unsupported providers/platforms refuse explicitly.

`IntegrationService::handle_hook_with_native_caller(..., policy)` discovers the
real caller at startup before journaling, associates the actual SessionStart
before application projection, and revalidates the original recorded process for
later callbacks without requiring fresh inbox discovery.
It preserves the existing deferred classic-start and compaction behavior.
`SessionBinding::check_native_caller(policy)` checks callback correlation without
granting mutation/idle admission. `admit_recorded_for_native_caller(cursor, wait,
policy)` combines provider resolution with the existing recorded guard. A selected
policy requires an existing exact association; lookup never silently creates one.
No policy cannot bypass an association. These APIs do not grant readiness.

The checked hook entry also refuses a missing policy before journaling any new
startup while an associated handle for that installation revision is attached.
This prevents a fresh invocation from silently dropping the physical boundary.
Explicit owner detachment permits later recorded-only startup; retained historical
associations remain untouched. The legacy raw hook API retains its observation
semantics, so consumers selecting physical checks must use the checked entry.
The final locked/offline Linux gate passes all 196 integration tests, including
these additional downgrade/detachment assertions, at source `9ffb3f8`.

The latest locked/offline Linux gate is **196 integration tests passing**, including
unsupported-policy refusal before hook journaling, no-policy refusal for an
associated binding, and bounded/strict policy decoding. The earlier 194-test and
native captures below retain their original source scope; they are not automatically
transferred to this new hook wrapper or its Codelaya adoption. Consumer native
qualification is the next gate.

Linux, `integration` feature, on the existing `SessionBinding`:

- `bind_linux_native_caller(address, expected_executable_sha256)` inspects the
  actual calling process and records `RecordedLinuxNativeCaller` once. Exact
  repeats are idempotent; changed process, boot, namespace or reference conflict.
- `recorded_linux_native_caller()` reads retained historical facts, including
  after replacement/detachment. It neither proves liveness nor reactivates work.
- `admit_recorded_with_linux_native_caller(cursor, address, hash)` checks current
  kernel caller facts, then compares the immutable association under the existing
  recorded journal guard. The `_wait` variant accepts at most the existing
  250-ms journal-lock budget; it does not retry native requests or mutations.

The record contains the original binding/reference, Linux peer PID/UID/birth,
independently expected executable digest, boot ID and PID namespace. No token is
read, persisted or forwarded. Kernel/process/executable I/O occurs before the
short journal guard, which must be held only through an application commit.
The guard pins recorded identity, not native process lifetime, idle status,
effective mandatory handlers or transport admission. A native process can end
after inspection. Module invocation fields remain observations, not independently
attested native generation. Callers must not promote those guarantees.

Once a binding has an association, ordinary `admit_recorded[_with_wait]` refuses
it, including on non-Linux builds. There is no fallback on missing/corrupt/mismatched
facts. Existing replacement, observation cursor and owner revocation checks still
apply. No implicit binding, detach, replay or recovery occurs on lookup failure.

## Schema 16 and consumer adoption

The additive `linux_native_callers` table is fenced by schema 16. Older readers
must refuse instead of ignoring its admission rule. `open_existing` requires an
explicit upgrade. Initializing a schema-15 store also refuses attached bindings
or unresolved plans; the existing scoped owner upgrade can detach selected handles
after all attached handles/plans are resolved. Advancing admission floors uses
the maximum existing/current cursor, so old observations cannot rebound. Retained
messages, Unknown outcomes and history are preserved; no receipt is promoted.

Codelaya runtime `b0a4550` now adopts `4efaefd` at its agent-mutation and
native-handoff boundaries, with optional owner policy and controlled setup
upgrade from schema 15. Its [qualified consumer report](https://github.com/aroff/codelaya/blob/9de63e5/docs/native-caller-consumer.md)
retains exact source scope. Background history lookup does not use these guards. A daemon
cannot impersonate a native child merely by copying its socket address: perform
the physical check at the actual native callback boundary. Do not replace these
requirements with a managed agent or an unrelated inspector service.

## Qualification

Linux Rust 1.88, locked/offline: **194 integration tests pass**, including four
new association/migration tests. Synthetic cases cover immutable tuples,
reopen, changed process/boot/namespace/digest, stale replacement, detach,
corrupt scope, ordinary-admission refusal and schema-15 Unknown/floor preservation.
These synthetic tests are not native generation evidence. The optimized example
builds. Existing unused-result test warnings and stable rustfmt config warnings
remain; no full-workspace strict-lint or consumer/package gate is claimed.

Fresh ordinary interactive Claude **2.1.308**, network-disabled disposable Linux
container, no mounts, dummy key, no model prompt and no managed AIKit session:

1. The actual installed SDK session module invokes `handle_hook`, records the
   original reference and binds the real caller. Ordinary admission refuses;
   inspected admission succeeds. Native startup settles in 692 ms with the
   existing 10-second callback budget unchanged.
2. A real native custom-command child passes. An independent same-user process
   with the same address/reference fails the kernel ancestry check.
3. Documented `/reload-plugins --force` retains this unchanged module. No fresh
   SDK invocation is observed: **native replacement is not qualified**.
4. Another real child passes, then explicit owner detach runs while Claude stays
   alive. The next real child fails with `Detached`. The exact original association
   remains readable. Claude exits normally.
5. The previous Codelaya/SDK-15 binary refuses this store with `newer integration
   schema`; its logical SQLite dump is unchanged.

The retained-evidence verifier passes. Protocol, diagnostic commands, thin
marker-protected example and verifier are checked in under
`examples/claude-mod-qualification` and `aikit-sdk/examples`. Private evidence:
parent workspace `.local/native-binding-20261024/accepted-native.tgz`, SHA-256
`6a9913ef7d82b3b6a96f3100c4b18c7ea6d8e3f9c6c8c8783502af6b809d4bd1`.
Image/executable identities are pinned in the protocol. Private boot/namespace
and full native captures are not published in Git.

Failed premises remain separate: the first native fixture requested 1,000 SDK
events, exceeding its documented 500-row limit, and failed before checked
admission. A fresh profile with the corrected limit supplies the accepted trial.
The first full Rust run had six stale expected-schema assertions; correcting
those yielded 194/194. Neither is called a provider blocker. No callback budget
or readiness requirement was weakened to obtain a pass.

## Existing Claude APIs and remaining gaps

### Qualified shutdown correction

The first Codelaya consumer trial at `9ffb3f8` passes completion repair, automatic
same-session feedback and independent-caller refusal, but rejects normal
SessionEnd and leaves no End observation. That trial is unaccepted. The checked
boundary incorrectly requires fresh inbox discovery on every callback, including
shutdown. Do not weaken the End requirement or synthesize an observation.

The correction uses the inbox only to discover a genuinely new startup. Later
callbacks revalidate the immutable original process directly: actual ancestry,
PID/birth, current UID, executable digest, boot and PID namespace. A copied
record or socket address cannot manufacture that ancestry. Missing/changed facts,
old generation and detached bindings still refuse; there is no recorded-only
fallback. The explicit socket-inspection APIs retain their original behavior.
No lifetime/idle or module-generation authority is granted. At `4efaefd`, all
**197 SDK integration tests** and **25 serial consumer cases** pass. Existing
test warnings remain; this is not a full-workspace strict-lint or Windows gate.

The fresh ordinary Linux consumer run at Codelaya `b0a4550` passes all twelve
main scripted requests: repair/accepted completion, one owner review delivered
to the original session, accepted follow-up, independent caller refusal without
app changes, one actual End and unchanged cold app/SDK/process records. The
source-hash-checked diagnostic wrapper delegates every decision to the real CLI.
At End it observes **no socket-address environment variable**, while owned context
remains present and the recorded native process is its parent. CLI exit is zero.
This confirms unavailable inherited address metadata, not that the socket server
itself had necessarily closed. The first failed End trial remains unaccepted.

Eight altered-evidence checks reject missing End/context, changed original
process, foreign input scope/frame, missing dispatch candidate, implicit version
selection and changed cold app data. These are verifier checks, not eight native
trials. Legacy verifier assumptions are retained: explicit wrapper-source-hash
and append-version-two modes preserve strict default checks; runtime data and
gateway setup were unchanged. Both Turns remain Completed, the frozen review
and SDK receipt remain Unknown, and proof observation rows remain absent.

The report links the protocol, source/binary/image/package hashes and private
archive identity. Production status still refuses readiness/activation; full
native session-generation/effective mandatory policy and independently durable
acknowledgement/actual-input reconciliation remain unqualified. Windows is deferred.

Official docs rechecked on 2026-10-24:

- [Hook failure handling](https://code.claude.com/docs/en/plugins/mods/events#handle-a-hook-that-fails)
  provides `.catch`; failure of that handler can still skip the hook. Existing
  ordinary native timeout/worker-recovery reports exercise this path. A normal
  catch is reusable, but does not alone prove mandatory enforcement under every
  failure or competing handler.
- [Session API](https://code.claude.com/docs/en/plugins/mods/api#send-and-receive-messages-between-sessions)
  provides queued delivery and receive hooks. Queue/append observations are
  useful candidates; they are not independently durable actual-model-input
  evidence. Do not invent a second transport before exhausting these APIs.
- [Session events](https://code.claude.com/docs/en/plugins/mods/reference)
  distinguish session start/reload and append/attach/detach. The reload request
  above did not exercise a changed module. Native generation/effective-policy
  attestation remains an integration qualification task, not demonstrated API
  absence.
- [Request-snapshot counterexample](claude-request-snapshot-boundary.md) already
  demonstrates that a pre-read API snapshot can contain a row missing from the
  actual captured main request. Investigate a request-bound view/retained lookup
  through the existing API before treating exact-consumption evidence as a
  provider enhancement request. Unknown must remain Unknown meanwhile.

Next: adopt the checked boundary and explicit upgrade in Codelaya, qualify real
module replacement/effective mandatory enforcement, and establish independently
durable feedback acknowledgement plus actual input reconciliation. This change
closes the missing durable physical association primitive, not all N1-N3 contracts.
