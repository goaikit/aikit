# Existing native APIs and reusable stored-history lookup

2026-10-24. Two fresh ordinary Linux Claude 2.1.308 API probes and an actual
AIKit library read identify reusable functionality and one concrete dependency
bug. No production capability, acknowledgement or consumption is promoted.
Windows checks remain deferred.

## Existing functionality, not a replacement protocol

| Surface | Actual finding | Limit |
| --- | --- | --- |
| Native inbox socket/token | Linux kernel SO_PEERCRED names the checksum-pinned Claude process, UID/GID and process-start ticks; a separate process read matches; peer is gone after normal exit | Valid/invalid/no-auth connections all remain open until the 750 ms observer timeout. No explicit authentication acknowledgement is observed. Server process facts do not authenticate the callback or attest its generation/handler chain. |
| `$.session.append` | One text-only meta note returns a native UUID; that exact row/text persists in the same-session transcript after exit | Storage is not main-loop consumption or a qualified receipt. |
| `$.session.messages()` / API form | Ordinary view omits the meta note; API view contains its exact text as role/content without UUID | These view shapes are not a stored-row-by-ID lookup. This observation is scoped to this note. |
| Existing `Backend::Claude.history_reader()` | The actual `HistoryReader::info/messages` API retains UUID/session/content and reads the original frozen review from stored native history after fixing its dependency | It is file-backed stored-history evidence, not independent native admission or consumption. The reader intentionally filters meta notes and its canonical messages omit ingress provenance. |

The [official socket contract](https://code.claude.com/docs/en/cross-session-messaging#the-sessions-inbox-socket)
documents an optional auth line on Linux and a required one on native Windows.
This probe does not test Windows or infer a successful authentication from an
open connection. The inherited token is never printed, persisted or hashed;
only presence and sanitized observations are retained. No inbox message is sent.

Installed declarations are pinned to SHA-256
`3c29d70e7f113f2409db12c3db20dd8385296c46aaf26984d00e5ecacd37c2c0`.
Their `SessionAppendResult` has a stable UUID; `SessionSendResult` reports queued
delivery rather than a durable consumption receipt. `TurnStepInput` pins a Turn,
step and message count but does not expose the final request body. These are
specific surface facts, not proof no other existing path can close a contract.

## History dependency bug fixed in its existing implementation

AIKit already wraps the spawn-free Claude Agent SDK history reader behind its
`claude-sdk` feature (enabled by default). Do not build another transcript parser.
The consumer-pinned dependency `aa01a48` found metadata for 73 real transcript
entries but returned zero messages: the last assistant had attachment/system
children, so the selector found no visible user/assistant graph leaf.

[Dependency draft PR #5](https://github.com/aroff/claude-agent-sdk-rust/pull/5)
fixes main-chain selection before visibility filtering, retaining control/meta
ancestry while excluding sidechain/team edges and unrelated orphan tails. It
adds no public API and changes no transcript. Three regressions fail unchanged
and pass fixed. Mechanical lint/format housekeeping required by that repository
is a separate commit. The fix is based on the actual pinned revision, without
pulling unrelated later main commits into this dependency update.

AIKit now pins `57ad0ec71c9e1de19cc750ba6ab7591adfd13e9f` and provides the thin,
marker-protected `integration_history_read` example. It only invokes existing
library methods and serializes results. Against the same unmodified native file:

- Before: metadata exists, zero messages.
- After: **32 messages**, including UUID `2d574e75-14c3-4c7c-af0c-4c2df2231f2e`
  from session `c0ea32f2-ab7a-4f2a-8088-aea36f4ee235` and its exact complete
  frozen review frame. Its SHA-256 matches the existing append checkpoint.
- Meta-note-only history still exposes zero visible messages by design.
- Before/after native transcript hashes are identical. No agent is spawned by
  the reader and no message is resent. The original receipt remains Unknown.

The actual Codelaya integration still pins `9cdc7ab` with default features off;
this new history path is **not yet wired into its receipt reconciliation**.
Validate canonical workspace, exact original session/binding/generation, row ID,
complete framing/digests and provenance before using lookup evidence. Neither a
matching string nor stored history is authoritative main-loop consumption.
Preserve N1/N3 Unknown until the required facts are established. No new generic
identity API, daemon, message state machine or provider transport is introduced.

## Reusable history correlation follow-up

`integration::inspect_message_history_page` now correlates an existing
`MessageAppendCheckpoint` with typed `HistoryReader` metadata and one message
page. It requires the original session, exact canonical workspace path and
logical payload digest, and compares every matching preallocated row UUID with
the full provider-frame SHA-256. Changed text, subagent lineage and a non-user
role are reported as nonmatching candidates. Duplicate UUIDs remain separate;
the caller must not choose the first successful-looking match. The bounded
inspector accepts at most 1,000 rows / 16 MiB serialized per page.

This reuses existing canonical history types and append checkpoints. It adds no
native parser, transport, receipt state, storage schema or automatic resend.
Enable `integration,history` for copied-page inspection; `claude-sdk` supplies
the existing real Claude reader. The thin `inspect_message_history` example
accepts the retained checkpoint and `integration_history_read` output. The
application's pinned integration-only dependency is still not wired to this
lookup, and this helper is not an authenticated native receipt resolver.

Linux qualification: the three focused inspector regressions and all **188
integration library tests pass**, serial, locked/offline on Rust 1.88. The thin
example builds and finds exactly one candidate at page index 26 with the full
frame hash in the retained native review transcript. It reads the actual
version-two checkpoint from `worker-live-20261024/databases-cold.json`, with no
new native prompt, send or database write. Output SHA-256:
`04d2c2d10fa1409d2f004164e846d49c4d1f723f12b7bcef573d572934477dfe`.
Logs and copied checkpoint remain in `.local/history-reuse-20261024`.
Existing unused-must-use test warnings remain; no strict full-monorepo lint or
new consumer/package qualification is claimed. An initial format invocation
used a nonexistent config path and failed; the corrected build completed before
the native-data inspection ran.

### Existing Claude surfaces reviewed again

Official documentation checked on 2026-10-24:

- [Hook failures and chain ordering](https://code.claude.com/docs/en/plugins/mods/events#handle-a-hook-that-fails):
  `.catch` supplies fallback behavior, but failure of that fallback skips the
  hook. Ordered middleware and earlier short-circuits must still be qualified.
  This corroborates the existing constant fallback implementation; it does not
  prove a mandatory chain remains effective under every failure.
- [Session events and API methods](https://code.claude.com/docs/en/plugins/mods/reference):
  `session.append`, `session.messages`, `turn.step` and `session.authorize` are
  existing investigation surfaces. `session.messages` retains at most the
  newest 4,096 entries; absence there is not proof of nondelivery.
- [Session messaging](https://code.claude.com/docs/en/plugins/mods/api#send-and-receive-messages-between-sessions):
  `session.send` success describes queuing; `session.receive` precedes Claude
  reading and may consume a message. Sender names are not authenticated. These
  are useful primitives, not independent durable model-consumption receipts.

The remaining items are **qualification gaps**, not a demonstrated absence of
Claude APIs: authenticate the native invocation/configuration and correlate
retained append/input evidence with an independent durable acceptance and
consumption source. Inspect the exact build's types and exercise candidate APIs
before requesting a provider enhancement. Do not relax readiness in the interim.
Windows checks remain deferred; no owner configuration or credentials change.

## Qualification and retained preparation failures

- Claude SDK Linux Rust 1.88 full tests: **187 passed / 5 live ignored**;
  all-target build, strict Clippy (`-D warnings`) and fmt check pass.
- AIKit with the exact published Git revision: history tests **24 passed /
  1 ignored**, locked/offline. Example build passes; there is no path patch in
  the final qualification. This is not a full AIKit/consumer/Windows gate.
- Native-data verifier and **nine altered-evidence checks** pass in Linux Node
  and on retained host files. They reject foreign peer binary/start time,
  invented auth acknowledgement, wrong stored UUID, changed frame/session,
  wrong dependency revision, changed transcript and receipt promotion.

Initial standalone resolution selected uuid 1.27, requiring newer Rust; the
qualification lock reuses 1.26.1 from the existing SDK lock. The first strict
lint/format attempts expose existing drift; required mechanical corrections
remain separate in the dependency's Git history. An initial fixed-reader call
ran before its build handle completed and still used the old binary; that
comparison is retained and excluded. The completed fixed build finds the row.

The builder cannot resolve GitHub and its shared Git cache is read-only. A
verified bundle of the published commit is imported into a task-local writable
Cargo cache, reusing the read-only registry. Final locked source metadata names
the exact Git revision above. Earlier failed fetch/readonly/package-selection
attempts, local-path comparison and all lock/build/test logs remain retained.
Existing repository rustfmt-option warnings remain; no strict monorepo lint
result is claimed. Credential values and raw owner profiles are not retained.

## Reproduce and evidence

Native probes: [inbox protocol](../examples/claude-mod-qualification/inbox-auth-protocol-20261024.md)
and [row-read protocol](../examples/claude-mod-qualification/row-read-protocol-20261024.md).
They use immutable image
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`,
network none, no mounts and disposable node homes. Both exit normally with zero
main-model requests. No owner policy/login or Windows configuration changes.

```sh
cargo +1.88.0 build -p aikit-sdk --example integration_history_read --locked --offline
CLAUDE_CONFIG_DIR=/isolated/config integration_history_read SESSION_UUID MARKED_WORKSPACE
node verify-native-api-reuse.mjs /retained/parent
node tests/verify-native-api-reuse.test.mjs /retained/parent
```

Private retained directories under the parent workspace `.local/`:
`inbox-auth-20261024`, `row-read-20261024`, `history-reuse-20261024`.
Only sanitized inbox facts and a whitelisted native loading line are copied;
no token, full auth transcript or owner profile is exported. Key captures:

| File | SHA-256 |
| --- | --- |
| inbox-auth.json | `032acdd0a5bf7bb485eef9415b252fcd186b1116252b5b9e3e35a58ab825cb9e` |
| row-read.json | `d96d3f428233608b7bafd2db67b34433d3cd73f9ba8b65b755ba83dd52cfc7a4` |
| stored-row.json | `ff6a0301fac75dd67dd3fe784184aa8b36dac377e5102e34067db3c95592735f` |
