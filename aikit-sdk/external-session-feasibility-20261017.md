# External-session feasibility refresh, 2026-10-17

Later evidence: [normal native completion](native-turn-observations.md) and
[adversarial native probes](../examples/claude-mod-qualification/native-adversarial-probes.md).
The latter verifies Linux abort-after-Allow and did not reproduce the offline
wrapper-erasure prediction against real command hooks (22 Blocks were retained).
The historical engine observations below do not override that native evidence.

Baseline: SDK `4b5bb1d937352708bed5032521faa1e9e6620e76`. No capability is promoted by this report. User-started interactive Windows sessions remain the target topology.

## Claude candidate APIs

Current official documentation describes plugin mods (v2.1.287+) with native `turn.start`, `turn.step` and `turn.complete` events. Completion includes a Turn identifier, final answer and an abort indicator. This is a candidate for the authoritative outcome missing from command Stop hooks; verify failure, abort, competing handlers and correlation before treating it as accepted completion.

The mods API also documents `session.send` queue results and `session.receive` before model consumption. Receive handlers can suppress delivery, so observation at that boundary alone is not proof of the next model input. Investigate model-step context evidence and replay/lookup before implementing durable SDK receipts. Native mods run in the user's existing session and need no separate Node runtime, but loading/trust/version constraints must be qualified.

Channels are another documented push mechanism for running sessions, with explicit per-session opt-in and preview/organization restrictions. Reuse an existing qualified path where possible; do not replace user-started sessions with a managed gateway or infer acknowledgement from an uncorrelated return.

The command-hook documentation now describes an eight-consecutive-continuation Stop cap, reset by tool calls. This can conflict with unconditional repeated-stop enforcement. Qualify the effective configured behavior on the target version and retain Unknown until there is sufficient evidence.

The current environment-variable reference explicitly documents zero as disabling
that cap. `plan_install_with_completion_policy` now provides owned configuration
and restoration through the existing installer, as described in
[completion-policy.md](completion-policy.md). This closes a configuration API gap;
it supplies no native enforcement or effective-settings qualification.

## Bounded static probe

Claude Code 2.1.296 on Linux in a disposable Docker container statically validates a plugin registering `turn.start`, `turn.complete`, `classic.Stop` and `session.receive`. Native validation passes with warnings about author attribution and absent catch handlers. The probe makes no model request, installs no user-profile settings and supplies no model credentials.

The reproducible pass-through probe is retained in [examples/claude-mod-qualification](../examples/claude-mod-qualification/README.md); the retained version supplies author attribution and may still report the catch-handler warnings.

This establishes event-name recognition by that Linux executable only. It does not prove native Windows loading, executed callback shapes, actual completion/abort behavior, message acknowledgement, consumption, durable lookup or enforcement. Existing SDK Unsupported/Unknown assessments remain unchanged.

## Executed offline plugin-engine probes

The same Linux 2.1.296 executable also ran four credential-free, network-isolated
test-kit cases. These retain a valid denial over twelve separate Stop calls,
preserve abort correlation/text, show receive suppression, and demonstrate that
a wrapper can erase a valid downstream Stop denial by returning an empty result.
The monotonic-denial hypothesis was falsified; the retained test asserts the
counterexample and keeps readiness unqualified. It does not exercise the native
model loop, model input, actual sessions or Windows behavior.

Mod Stop decisions require the engine's object wrapper with behavior allow/deny;
plain command-hook JSON is rejected and skipped. At that point native callable
signatures and generated interactive declarations still needed inspection.

Native authoring also needs a qualified admission boundary shared with generation
changes. Comparing a recorded SessionRef before an application SQLite commit
cannot prevent the native journal changing between verification and commit.
The SDK must coordinate that boundary and retain durable native evidence for
message acknowledgement and actual next-model-input consumption. User-started
sessions must remain user-started throughout.

The SDK now adds [recorded admission](recorded-session-admission.md) using its
existing installation lock and journal transaction. This closes recorded
check/commit races when held through an application commit. It does not supply
native process authentication, notification buffering or deadline qualification.

## Credential-free interactive startup check

The same Linux executable/hash was started under a fresh disposable container
UID/home with `--plugin-dir` and no credentials or prompt. With networking
disconnected, startup exits on its service connectivity check, including with
the documented nonessential-traffic option. Allowing connectivity reaches theme
selection and then the login-method screen. The check stopped there without
signing in or submitting a model request; no `.claude-plugin/types` files had
been generated. The offline `plugin init --with hooks` scaffold creates classic
command hooks, not mod declarations. The owned container was removed afterward.

This records the attempted supported paths and the current declaration gap; it
does not prove no credential-free generation path exists. Windows machine policy
and native authentication remain separate requirements, neither bypassed here.

## Local gateway follow-up

The subsequent [native terminal fixture](../examples/claude-mod-qualification/native-gateway-probe.md)
uses a public dummy key and a deterministic server on container loopback. Normal
key approval/workspace trust generated the exact 2.1.296 declarations. A fresh
ordinary terminal session then passed the command/tool stdin and real outgoing
input verifier with Docker networking disabled, without demo mode or a real
account. This resolves the Linux declaration/process-stdin signature gap.

Generated completion has a reason (`answer`, `aborted`, `refusal`, `error`), so
the non-aborted flag alone is insufficient. The native fixture captured a
correlated `answer` and exact tool result in the next API request. It registers an
echo rather than Codelaya operations and supplies scripted model responses. It
does not qualify Windows, original-session review delivery, caller authentication,
protected completion ordering or final acceptance under competing handlers.

## Required next work

### Local SDK URL candidate rejected, 2026-10-22

The pinned native Linux x86-64 Claude 2.1.296 executable rejects
`claude --sdk-url ws://127.0.0.1:9` with exit status 1 before starting a
conversation. Its diagnostic says the host is not an approved Anthropic
endpoint and the flag is reserved for Remote Control workers connecting to
Anthropic's backend. This rules out the proposed loopback SDK URL attachment
on this exact executable. It does not establish behavior on other versions or
qualify any interactive Turn, hook, failure handling or delivery capability.

The executable SHA-256 is
`24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Two fresh invocations reproduced the same rejection in disposable containers
from `aikit-claude-native-probe:2.1.296`, with `--network none`, no mounted
owner profile, no credentials and no model request. The second invocation's
output, exit code and a separate identity probe are retained in the consumer's
parent workspace under `.local/sdk-url-20261022`.

Reproduce without overriding endpoint restrictions:

```sh
docker run --rm --network none --entrypoint sh \
  aikit-claude-native-probe:2.1.296 \
  -c 'claude --sdk-url ws://127.0.0.1:9'
# Expected: reserved-endpoint diagnostic, container exit 1.
```

The existing `runner::claude_session::open_claude_session` creates a managed
SDK session and sends its first prompt. Its callback/control support is not
evidence of attachment to an owner's already-running interactive terminal.
Do not replace that required topology with this managed API or attempt to
bypass Claude's endpoint restriction. The official
[CLI reference](https://code.claude.com/docs/en/cli-reference) distinguishes
interactive startup from SDK print mode; the
[SDK hook reference](https://code.claude.com/docs/en/agent-sdk/hooks) describes
SDK-registered callbacks, not an existing-terminal attachment contract.

The subsequent [native mod catch comparison](../examples/claude-mod-qualification/stop-catch-protocol.md)
recovers an injected throw and timeout, but injected fallback failure still
allows completion. Qualify handler ordering, cancellation and process loss
before production use. The earlier command-hook failure result remains
authoritative; a catch handler's presence alone is not proof of protected
enforcement. No capability, schema or readiness changes.

1. Use the retained Linux declarations/signatures to implement the bridge, and qualify the corresponding native Windows loading/trust and caller contracts.
2. Extend the existing Claude adapter/installer with owned plugin configuration if this API satisfies the required contracts; preserve unrelated user hooks and the command-hook path.
3. Normalize native Turn/generation/outcome evidence into the existing integration journal. Preserve the distinction between the application's Allow and the final observed result.
4. Qualify repeated denial/deadlines and transport receipt/consumption/reconciliation. Report a concrete native gap wherever a primitive is absent; never promote support from this static probe.

Sources inspected on 2026-10-17:

- [Mod events](https://code.claude.com/docs/en/plugins/mods/events)
- [Mods API](https://code.claude.com/docs/en/plugins/mods/api)
- [Mod creation, versions and generated types](https://code.claude.com/docs/en/plugins/mods/create)
- [Channels](https://code.claude.com/docs/en/channels)
- [Stop hook behavior](https://code.claude.com/docs/en/hooks#stop)
- [Environment variables](https://code.claude.com/docs/en/env-vars)
- [Offline plugin test kit](https://code.claude.com/docs/en/plugins/mods/test)
