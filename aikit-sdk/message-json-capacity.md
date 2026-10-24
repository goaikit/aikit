# Recorded-message JSON capacity, 2026-10-24

Runtime parent `1fac9a4`, branch parent `c8bd7b0`. This reuses the existing
generated sender and recorded-message bridge, without a new transport or daemon.

## Concrete SDK gap

A fresh ordinary interactive Linux Claude 2.1.308 consumer run submits an exact
65536-byte frozen UTF-8/JSON review containing Unicode and many escaped quotes.
The actual native `prompt.submit` result is **130934 JSON bytes**, with one
matching settled engine/core trace. Its complete recorded-return envelope would
be **131414 bytes**. Existing SDK caps of 98304 and 131072 respectively could not
retain it; the SDK correctly kept Unknown and `native_receipt_unavailable`, but
unnecessarily lost the complete return. This is an SDK sizing gap, not an absent
Claude API. Independent pre-input/native core/HTTP/cold checkpoint evidence
retains the full review without truncation or resend.

The consumer first proves 65537 bytes is rejected before queue insertion, then
submits 65536 bytes under that still-unused operation ID. The completed native
trial preserves original history, one attempt and absent receipt/consumption
proof. One local completion-handler failure caused a normal fourth feedback step;
that repair and the original fixed-count verifier failure are retained. An
earlier diagnostic plugin named two entry modules and failed to load; that trial
remains failed qualification, with its old Unknown review preserved.

## Public surface and bounded implementation

- `MESSAGE_BRIDGE_PAYLOAD_LIMIT` stays **64 KiB**, measured in exact UTF-8 bytes.
- Add `MESSAGE_BRIDGE_RESULT_LIMIT` **512 KiB**, measured after JSON serialization.
- Add `MESSAGE_BRIDGE_ENVELOPE_LIMIT` **1 MiB**, measured on bounded JSON stdin.
- `RecordedMessageRequest::read` applies the envelope bound; existing native
  context/identifier/scope checks still use the original `AgentToolCall` decoder
  on the canonical context-only envelope. Model tool-call limits remain unchanged.
- The renderer puts the SDK constants into the generated module configuration.
  The sender validates those qualified values and uses them for its message,
  result and RPC-envelope checks. The callback validates the same result limit.

JSON can expand one logical byte to six bytes. The result bound accommodates a
fully escaped 64 KiB payload plus bounded metadata, and the separate envelope
bound accommodates context/action fields. All reads remain finite. The existing
1 MiB callback-output bound, 4 MiB API-view bound, native framing, scope/core
correlation and no-resend behavior remain unchanged.

No delivery, acknowledgement, consumption, readiness or authentication is
inferred from a retained result. Unknown remains Unknown. No schema migration,
record version or new dependency is introduced. Older readers explicitly reject
large records under their old limits; update handler and generated module
together, rather than silently dropping evidence.

## Qualification

Linux Rust 1.88 locked/offline: **182 integration tests pass**, serially without
concurrent builds; **41 base/message-module tests pass**. New tests cover maximum
escaped UTF-8/control payloads through JSON decode, exact result/envelope limits
and next-byte rejection, persistent reopen without promotion, one attempt and
genuine oversized-result uncertainty. Existing oversized-result tests now use
the new explicit bound, preserving their negative assertion. No timeout or
native policy requirement was relaxed. Existing Rust guard/rustfmt warnings remain.

Fresh matching consumer/native qualification of the fix is still required;
the baseline above uses the old limits. Native Windows checks remain deferred.
Local consumer evidence/logs: `.local/payload-native-20261024/`.
