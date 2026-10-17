# External-session feasibility refresh, 2026-10-17

Baseline: SDK `4b5bb1d937352708bed5032521faa1e9e6620e76`. No capability is promoted by this report. User-started interactive Windows sessions remain the target topology.

## Claude candidate APIs

Current official documentation describes plugin mods (v2.1.287+) with native `turn.start`, `turn.step` and `turn.complete` events. Completion includes a Turn identifier, final answer and an abort indicator. This is a candidate for the authoritative outcome missing from command Stop hooks; verify failure, abort, competing handlers and correlation before treating it as accepted completion.

The mods API also documents `session.send` queue results and `session.receive` before model consumption. Receive handlers can suppress delivery, so observation at that boundary alone is not proof of the next model input. Investigate model-step context evidence and replay/lookup before implementing durable SDK receipts. Native mods run in the user's existing session and need no separate Node runtime, but loading/trust/version constraints must be qualified.

Channels are another documented push mechanism for running sessions, with explicit per-session opt-in and preview/organization restrictions. Reuse an existing qualified path where possible; do not replace user-started sessions with a managed gateway or infer acknowledgement from an uncorrelated return.

The command-hook documentation now describes an eight-consecutive-continuation Stop cap, reset by tool calls. This can conflict with unconditional repeated-stop enforcement. Qualify the effective configured behavior on the target version and retain Unknown until there is sufficient evidence.

## Bounded static probe

Claude Code 2.1.296 on Linux in a disposable Docker container statically validates a plugin registering `turn.start`, `turn.complete`, `classic.Stop` and `session.receive`. Native validation passes with warnings about author attribution and absent catch handlers. The probe makes no model request, installs no user-profile settings and supplies no model credentials.

The reproducible pass-through probe is retained in [examples/claude-mod-qualification](../examples/claude-mod-qualification/README.md); the retained version supplies author attribution and may still report the catch-handler warnings.

This establishes event-name recognition by that Linux executable only. It does not prove native Windows loading, executed callback shapes, actual completion/abort behavior, message acknowledgement, consumption, durable lookup or enforcement. Existing SDK Unsupported/Unknown assessments remain unchanged.

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
