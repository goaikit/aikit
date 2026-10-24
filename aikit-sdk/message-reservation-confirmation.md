# Confirm the reserved message before native handoff

2026-10-24, based on SDK parent `a7b22d50f0ec02df2b628424756aa4f9cc549673`.

## Implemented change

The recorded bridge's ready `Recheck` now returns its existing
`MessageReservation` through `RecordedMessageReply.reservation`, just as Poll
returns a newly reserved operation. The response identifies the operation the
SDK actually checked; a same-session readiness boolean alone does not do that.
Busy or retired operations do not receive a dispatch reservation.

Before calling Claude, the owned message module requires that returned
reservation to match its retained binding, native target, reservation ID,
operation ID, exact content and payload digest. A missing or changed confirmation
stops the pump before native I/O. It does not reset or resend the operation.
Retired Unknown/Failed/Acknowledged attempts still allow a later distinct review.

No new method, daemon, dependency, database table or schema version was added.
This reuses the existing public reply field and immutable reservation. Consumers
of the reply must allow it to be present on a ready Recheck, not only Poll.
An older native module ignores the additional response value; a new module with
an older handler stops safely because confirmation is absent. Update the owned
bundle and its handler together through the existing installation journal;
the generated module fingerprint changes.

This is a recorded-state consistency check, **not a native admission lease**,
acknowledgement, consumption evidence or production readiness promotion.

## Linux validation

- 33 generated outcome/completion/message module tests pass, including seven
  independent changed/missing confirmation cases with zero native calls.
- 175 SDK integration tests pass with Rust/Cargo 1.88.0, locked/offline resolution.
  Two existing `unused_must_use` warnings remain in installation tests.
- The new regression fails against the previous helper: its missing-confirmation
  case makes one native call instead of zero. That expected failure is retained.
- The first Rust test selector matched no tests; it is not counted. The explicit
  test module subsequently passed 28 message tests, followed by the 175-test gate.
- The initial toolchain selection attempted an uncached repository toolchain and
  stopped. Qualification explicitly selected the already installed Rust 1.88.0;
  no repository toolchain file or owner configuration changed. Stable rustfmt
  reports existing unsupported configuration options; no strict formatting or
  lint gate is claimed here.
- A fixture for a second distinct review initially returned the first review's
  content on recheck. The new guard rejected it. The fixture now returns the
  second actual reservation; the production guard was not weakened.

Exact local logs: `.local/aikit-recheck-20261024/` in the qualification workspace.
Windows checks remain deferred. Native/API qualification is separate from these
stub and journal tests; see the [input-boundary comparison](../examples/claude-input-qualification/README.md).
