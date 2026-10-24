# Claude clear/resume lifecycle recovery

2026-10-25. Runtime source `5bfe1346f5d5cab279bd8b4161dde4494d564a87`.
Windows checks remain deferred. This closes an AIKit lifecycle implementation
gap using existing Claude APIs; no provider enhancement is required for it.

## Implementation

The generated module previously initialized only on `session.start` and stopped
its feedback loop on every `session.end`. Clear/resume therefore left that loop
stopped and could retain old Turn state. The [official session reference](https://code.claude.com/docs/en/plugins/mods/reference)
distinguishes mod loading from conversation replacement.

The existing integration module now:

- Invalidates the departing epoch and creates a fresh invocation on clear/resume,
  without registering duplicate tools or resetting the immutable delivery journal.
- Requires the requested session/workspace to match the native API before
  publishing new context. On native 2.1.308, resume's classic event runs while
  the API still names the departing session, even after its downstream return.
- Reuses the opted-in feedback timer to finish that transition. Authoring-only
  modules initialize on the next Turn and start no background polling loop.
- Keeps message reservations, input candidates and asynchronous callbacks in
  their original epoch. Late results cannot stop or mutate the replacement pump.
- Serializes environment publication and fences Turn observation, authoring and
  completion validation across await boundaries. Old completion cannot Allow a
  replacement Turn. Uncertain original operations are never retargeted/resubmitted.

This extends the existing generated bridge. There is no new public Rust service,
provider daemon, transcript parser, journal schema or Codelaya-specific mechanism.

## Evidence and limits

- **197 Rust SDK integration tests**, locked/offline Rust 1.88, serial.
- **57 JavaScript module tests**, including delayed old poll/completion/env writes,
  stale Allow, abandoned startup, native identity switching and one opted-in timer.
- Native Claude **2.1.308** validates the generated module with warnings.
- Fresh ordinary interactive Linux CLI: initial prompt, clear, second prompt,
  resume initial session, third prompt, normal exit. Three distinct invocations;
  native session IDs initial/cleared/initial; polling and one correlated
  start/Stop/native completion in each. Five scripted gateway requests, three
  main requests. No module failure notices in retained native history.

The native executable SHA-256 is
`356733f185fa988e6bf3aa9a555269fdd167982a2e2a80d3c8a78037bb92ad58`.
Generated fixture module SHA-256 is
`be270edb5dd352306b138a66f9a25b478913735bfe071dea7a1c920d02cb1330`.
Private final evidence archive SHA-256 is
`8b005413a4118ada3f0a85b238acfc5e670bde662b892ad720803cc53a320274`.
It retains generated source, synthetic requests, diagnostic frames and transcripts.

Isolation: disposable home, node UID, no host mounts, network none, loopback
gateway, public dummy key, independently pinned native binary. The owner starts
ordinary Claude; AIKit does not spawn a managed session. The
[reproducible protocol and verifier](examples/claude-session-lifecycle/README.md)
use a diagnostic journal and scripted model. **This trial does not exercise the
actual Codelaya journal or qualify native feedback consumption.** The previous
consumer run retains its original `4efaefd` source identity.

Retained failed/intermediate trials are not final evidence: initial resume
identity mismatch and request ceiling; native success with an additional timer;
one Rust failure detecting polling in authoring-only modules; premature terminal
startup causing the fresh-home guard to refuse preparation. Final qualification
uses a separate fresh container and the single opted-in timer implementation.

## Remaining contracts: investigate existing APIs first

| Contract | Current classification | Next evidence required |
| --- | --- | --- |
| Native clear/resume | AIKit implementation fixed; scoped native lifecycle pass | Actual registered consumer workflow through replacement sessions |
| Effective mandatory handler and module generation | Implementation/qualification gap | Exercise native registration/order/guard APIs and the full failure matrix |
| Original-session review admission and raw handoff | Existing SDK path, scoped earlier consumer evidence | Qualify the updated pin with actual journals and competing handlers |
| Independently durable acknowledgement and actual consumption | Unproven; keep Unknown | Original checkpoint plus native request-bound evidence and retained reconciliation |

The [hook failure contract](https://code.claude.com/docs/en/plugins/mods/events#handle-a-hook-that-fails)
documents skipping failed handlers and a catch path. A local catch does not by
itself qualify the effective mandatory chain under replacement or competing mods.
The [mods API](https://code.claude.com/docs/en/plugins/mods/api) provides existing
session and prompt operations; their existence alone proves neither durable
acknowledgement nor exact model-input consumption. The retained API snapshot
counterexample still applies. No blanket missing-Claude-API claim or upstream
enhancement request is made by this change.
