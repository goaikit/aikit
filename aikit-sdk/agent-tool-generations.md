# Recorded native tool generations

Schema 9 extends this mechanism with [native Turn observations](native-turn-observations.md).
The startup evidence below retains its original schema 8/source scope.

Owned Claude modules bridge the module-issued invocation UUID into the existing
hook journal, SessionRef and binding validation. This is recorded generation
correlation, not native-process authentication or liveness.

## Startup and compatibility

The module publishes bounded JSON into a literal process environment variable
whose name derives from application/installation identity. Its schema version,
installation ID, native session ID, module UUID and cwd are claims, not credentials.
Subsequent commands inherit the frame. No project setting or profile is changed.
Other programs running as the same owner are outside this authority boundary;
competing plugins can interfere with native operations.

`plan_agent_tool` adds fixed argv from the receipt's validated hook handler.
After setting context, the module invokes that handler with a SessionStart
notification, then registers the tool. No shell string or agent launch is used,
and unrelated startup hooks are not rerun. Standalone exports do not bind a hook
handler or establish a recorded start.

The first controlled run disproved an assumed ordering: classic SessionStart
executes before module session.start. Newly owned integrations therefore defer
that initial notification without recording a session or granting authority.
The later module callback records the scoped start. Input/decisions without
required context still fail before journaling. Bootstrap failure prevents tool
registration and cannot authorize completion.

The SDK reads only the environment key selected by its owned receipt. Native
hook JSON cannot select the key or supply the invocation ID. The strict bounded
frame must match installation, session and canonical workspace and contain a UUID.
The ID flows through existing journal/binding checks, including observed old
invocation and stale binding/revision refusal. No second generation table exists.

Schema 8 prevents older readers ignoring this requirement. Schema 7 tool receipts
default to no inherited context until explicitly updated through plan_agent_tool;
migration invents no identity evidence. Coordinate application/hook binary upgrades.
Ordinary hook updates retain tool ownership. Replacing the hook executable/argv
beneath a bound module is refused because it would leave its startup callback
stale; remove/reinstall while idle is currently required. A combined handler/tool
replacement plan remains future work.

## Qualification, 2026-10-17

Rust 1.88 selected integration tests passed 90 on Windows and 92 in isolated Linux.
Two new cases cover inherited context bounds/scope and refusal to journal/admit
input when context is missing. Installer tests also cover handler-replacement
refusal. SDK/example Clippy passes with the existing explicit
`uninlined_format_args` allowance; full strict SDK Clippy is not claimed green.

A real ordinary Linux Claude 2.1.296 terminal loaded the owned module without
plugin flags and bootstrapped exactly one recorded SessionStart. Its invocation
UUID matched the Rust inspector's tool result in actual API input and the SDK's
observed SessionRef/binding start cursor. `verify-generation.mjs` checks this
alongside the original request verifier. Replies were scripted by a loopback
gateway; no account credentials or owner profile were mounted. The fresh final
session ran on Docker's none network. Owned removal/preservation checks passed;
the container was removed.

The negative ordering probe sent no model prompt. Its fixture was updated using
the SDK's plan/apply path before a fresh ordinary session. The successful gateway
received two main requests. Raw logs remain local under the implementation chat's
`.local/generation-tool-generated/evidence`; the negative log is retained in
`startup-order-negative`. Source tests/verifiers are published, not raw contexts.

API request evidence SHA-256:
`9b850439782820e193c7f511988f62f4d76449ff11dc15b172eab5defde6d38d`.
Rust inspector SHA-256:
`7d012ed73646faa1d7d26373bf72aae250c9efffdc232ade501b4c1132d981d1`.
Hook example SHA-256:
`dcad7ee392e12836839431dac089c8f41c7a526f468f19d987c1f93ee6d4bf00`.
Client/image identity, Linux example build and ordinary onboarding/network steps
are in the [fixture guide](examples/agent-tool-qualification/README.md).

Reproduce the owned fixture with both compiled examples. After the ordinary
session's `AIKIT_NATIVE_BRIDGE_PROBE`, run `verify.mjs` and
`verify-generation.mjs` with `/home/node/evidence` before removal. Setup checks
installed bytes against SDK plan fingerprints; an unowned export differs because
it has no owned startup callback. The final installer additionally revalidates
the existing hook executable when planning tools; that preflight and the capability
report update were added after the native run and are separately Rust-tested.

Claude NativeSessionIdentity is implemented but Unknown, so required readiness
still fails. Unseen delayed starts, native-process authentication, notification
delivery/deadlines, competing wrappers, installed invocation authority, Codelaya
operations and the full Windows matrix remain unqualified. UUID/environment
values must never be presented as credentials or independent native caller proof.
