# Owned SDK append client, 2026-10-21

This step wires the previously implemented checkpoint APIs into the opt-in
Claude module and existing fixed-command receiver. It also runs that combined
path in a real ordinary Linux terminal session, using the SDK's existing
installer and journal. Starting revision: AIKit `7c041af`; Codelaya `b044f1e`.
No consumer dependency, production capability, owner profile or Windows policy
was changed. Windows acceptance remains deferred.

## Implementation

`RecordedMessageAction::PrepareAppend` / `RecordAppendStored` retain typed
candidate metadata against the exact original reservation. The receiver reuses
the checkpoint transaction implementation without acquiring nested write locks.
`RecordedMessageReply.append_checkpoint` is additive and optional; protocol
schema remains 1, storage remains 12. Old receivers reject unknown actions.

The generated module checks the original in-flight ticket, full native plugin
origin, main-session scope, row UUID, pinned client version and exact known
envelope before preparing the row. It delegates storage once, then retains the
actual returned digest and native row reference. It validates the checkpoint
response against the original binding/generation/payload. Rewrites and capture
loss stop further pumping. Successful submit without an append observation also
stops it. Raw submit return stays separate; no acknowledgement, consumption or
automatic resend is introduced. An observer failure cannot be described as
preventing native storage.

`integration_hooks` only adds thin library delegation for immutable synthetic
message staging and checkpoint inspection. It implements no new transport or
review policy. `setup-sdk-append.mjs` is a disposable qualification controller,
not an application launcher; the terminal starts Claude normally.

## Failed predictions and native framing

The first setup incorrectly requested native Turn events as settings hooks.
The installer refused it. The fixture now requests only classic events; the
owned module supplies native Turn observations. The rejected source is retained.

The first actual combined run delivered the synthetic text and retained Unknown,
but captured no append checkpoint: the strict full-name envelope check failed.
Claude shortens long plugin names inside message text while retaining the full
name in `origin.name`. This is retained as `framing-negative`, not a passing run.

Six native ASCII-name cases sampled lengths 21, 22, 23, 24, 25 and 35 through
ordinary project discovery. At 24 and below the name was intact; at 25 and 35
the label was the first 23 characters plus U+2026. Append result, saved transcript
and actual endpoint input agreed; full provenance was unchanged. The separate
`verify-append-labels.mjs` passed all six captures. The corrected SDK client uses
that rule only for Claude 2.1.296 and generated ASCII names up to 35 characters.
Other versions/framing remain unsupported by the candidate pump. Unicode
manifest-name handling is not qualified or needed by this SDK name generator.

An attempted fixture-directory rename hit an overlay `EXDEV`; the final case
uses fresh fixed workspace/state/evidence paths instead. Copying a whole debug
directory hit the provider's `latest` symlink on Windows; named regular files
were retained instead. Neither failure changed host policy or required elevated
Windows symlink privileges.

## Actual combined result

Fresh public-image onboarding used no mounts, owner profiles or real credentials.
External networking was removed before source copy/workspace trust. The ordinary
session discovered the owned module in the disposable project's skills directory,
without `--plugin-dir`, managed launch or permission bypass. A loopback endpoint
supplied scripted replies; this is not a real-model semantic evaluation.

After one actual initial Turn, the controller staged one exact Unicode/newline
review through the library. The existing module pump reserved it once and called
the native prompt API. Real fixed-command RPCs persisted both append phases.
Independent checks after normal exit verified:

- Original session/generation/operation/reservation agree across SDK records.
- One native row UUID and exact expected/stored envelope digest agree with the
  provider's saved user row and one actual outgoing input block.
- Two main API requests: initial input, then the review. The review payload
  appears once in its request; one native enqueue is retained.
- Both actual native Turns have correlated successful outcome observations.
- Raw return and checkpoints survived normal exit/reopen unchanged. Repeating
  immutable staging returns the same Unknown receipt, not another attempt.
- A separate Python `sqlite3` reader opened a copied closed journal read-only.
  Its actual message/checkpoint/handoff rows exactly match SDK inspection, schema
  is 12 and the qualified observation table contains zero records.
- Receipt stays Unknown with no acknowledgement, consumption or failure claim.

`verify-sdk-append.mjs` passed in Linux and against retained host evidence. Five
verifier checks passed in Linux: the original capture passes; foreign reservation,
changed native row ID, invented acknowledgement and missing wire payload fail.
These mutations are sensitivity checks, not four additional native fault runs.

## Checks and identities

Final Rust 1.88 Linux: 135 integration-module tests passed, zero failed. Node:
22 generated-module tests passed (18 message-pump and four outcome cases).
Scoped library/embedding-example Clippy passed with
`-D warnings -A clippy::uninlined_format_args`, the existing format allowance.
Build/test commands use `--no-default-features --features integration --locked
--offline`; Rust tests use `--lib integration:: -- --test-threads=1`.
The same pinned native image/executable as the baseline report was used.

| Executed component | SHA-256 |
| --- | --- |
| Message helper source | `210504248a6b31d72fb04d5446353500181f0624d2cdc39399ae7cde8dc4a971` |
| Bridge source | `4de4bacb4a0f6efd3413ee5a80334301f7c5e08a4bcff0518f3a052f5cf9d691` |
| Checkpoint source | `fa706dd2c54f0fc627ef878600993cc5555976bfb2cdad8cd67694146adc597b` |
| Renderer source | `fad33fac3d820d079c285d627276ac80b42d4a89f5035929273ae3a7f48b7a20` |
| Thin hooks embedding source | `fb5e1a94565891a5617b6352cd6695c07fc98f6d02fd0a8c953fd0be64d68ea0` |
| Executed hooks binary | `b40566181d8b01a49ca42673c1c26488ec0add19c16263a4324e57ee7ceb7a5b` |
| Executed message receiver | `ccb77c46fed9b21b3b92c01d161ef1c3fa2038a5202db3c05e12710494cccd70` |
| Generated owned module | `58bac6d87d4f422b86bebb13782ae793432efa19db402eb4b6691aa9d3a9fb8e` |
| Persisted SDK inspection | `1db0eb7bc6c348aaaf625ef20c49fe0eff6c3919b76f5c45686d76a4d34b5944` |
| Actual outgoing requests | `629edf91d80072f7ab86907e2513893ceab219a5c4058bc5a22a5b5c3393ca0e` |
| Native transcript | `afbcbbf2461fe6db89717d53eb3a89a6ed10014100572a5dcd7b87ea199d65cc` |

Raw parent-chat evidence is `.local/sdk-append-native-20261021/{final,framing-negative}`,
with `label-evidence`, `label-transcripts`, `label-verification.json` and the
rejected setup source under `.local/append-client-generated`. Final verification
JSON records the remaining artifact hashes. Native sessions and owned containers
are removed after retention. Relevant logs are
`docker-sdk-append-client-{regression-final,node-complete,clippy}-20261021.log`.

## Reproduce and remaining work

Build the three embedding examples `integration_hooks`,
`integration_message_bridge` and `agent_tool_plugin` from the exact source, with
Rust 1.88/integration feature. Use the baseline's empty-container onboarding and
network-disconnection sequence, then copy the examples to `/home/node/bin` and
this fixture to `/home/node/probe`. Run `setup-sdk-append.mjs setup final`, start
the loopback endpoint at `/home/node/evidence-final`, and start ordinary Claude
in `/home/node/append-client-final`. Submit `SDK_APPEND_CLIENT_INITIAL` once.
After its actual completion, run `setup-sdk-append.mjs stage final` externally,
wait for the stored checkpoint, inspect, then exit normally and inspect again.
Retain named files; independently inspect the closed SQLite copy with
`inspect-sdk-append-db.py`. Follow the verifier's named evidence inputs and run
`verify-sdk-append.mjs` plus `tests/verify-sdk-append.test.mjs` on the capture.

Next qualify SIGKILL/lost-RPC recovery in this combined SDK path, not just the
earlier diagnostic helper. Native caller authentication, atomic generation/idle
admission, competing native wrappers on the integrated path, protected actual
consumption and authoritative reconciliation remain open. A native row reference
alone is not any of those guarantees. Do not promote capabilities, activate the
consumer or treat this as four-provider/Windows acceptance.
