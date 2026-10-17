# External-session feasibility refresh, 2026-10-17

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
plain command-hook JSON is rejected and skipped. Native callable signatures and
generated interactive declarations still need inspection before production use.

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

## Required next work

1. Obtain the target executable's generated mod type declarations and qualify native loading/trust in the user-started interactive topology.
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
