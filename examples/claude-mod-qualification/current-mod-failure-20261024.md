# Current native mod budget and worker recovery probe

Predeclared 2026-10-24, after the scoped managed-command comparison. Test the
existing mod failure mechanism before assuming that command-hook failures are
universal provider limitations. Linux x86-64 native Claude 2.1.308 in immutable
image `sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`;
ordinary interactive CLI, no external network or mounts, disposable profile and
public dummy key against the existing 32-request bounded scripted gateway.
No owner policy/settings/credentials, Windows checks or Codelaya installation.

Two fresh sessions load a diagnostic guard and an independent completion
observer. The guard's first native Stop records its actual event and budget,
then either awaits a never-settling promise (`pending`) or executes an infinite
synchronous loop (`busy`). Subsequent Stops read the retained call file and
delegate unchanged. The failure handler is extracted verbatim from the current
SDK completion source's constant `.catch`; no logging/I/O is added to it.
These are failure primitives with the exact production fallback, not execution
of the complete generated SDK bridge or consumer policy.

Require native timeout/failure evidence, exact fallback feedback in the next
main HTTP request and native transcript, a second Stop and only then settled
engine/core completion. Observe whether busy triggers actual worker loss or
merely deadline handling; do not equate a loop with a proven worker kill. Record
Docker process inventory during the pending dispatch. If the native host fails
to settle within 35 seconds after recorded Stop, preserve the run as failed or
inconclusive and stop only its owned container. Never relabel/retry a submitted
trial. Preserve all files, native logs, container metadata and source hashes.

The exact installed declarations say hook own-time budget is 10000 ms, catch
grace 1000 ms, linger 5000 ms; time awaiting `$`/`next` does not count. A worker
ending under a hook is declared a `throw` failure. Establish actual behavior
separately, without granting N1/N2/N3, production or Windows readiness.

## Observed result

| Case | Native session | Failure handling | Result |
| --- | --- | --- | --- |
| pending | `e01dce0f-cfb7-4550-a752-82e456d12361` | Native hook budget expires at about 10020 ms; exact constant catch answers | Block feedback in second main request; second Stop delegates; settled engine/core answer |
| busy | `37177e32-e4be-49c9-9640-b8d2a655e89d` | Native heartbeat detects wedged worker after 5000 ms; replaces worker; explicitly asks catch in replacement | Exact Block feedback in second main request; second Stop delegates; settled engine/core answer |

This **changes the next action**: do not request a generic worker-loss fallback
from the provider. The current host already recovers this actual wedged-worker
case and invokes the constant handler in its replacement. Reuse and qualify it
with the full SDK module and Codelaya consumer. Arbitrary external worker kills,
unload, competing middleware, effective configuration and protected identity are
not established by this result. The earlier managed command fail-open result is
specific to that command path; these successful mod cases are a different path.

There are four main and two title requests across the two runs. Every first
Stop is retained before injection with the same session/prompt as its second
Stop. The actual native core receives the non-aborted answer only afterward.
Both transcripts and outgoing second requests contain the exact production
fallback reason. Native elapsed times were below the declared 35-second bound;
both sessions exited normally. The pending promise remains unresolved, and the
host reports it lingering 5000 ms past its budget even though its catch already
blocked. This is retained; it is not claimed as successful hook cancellation.

### Witness limitation

The installed host uses **one shared hooks worker** for all plugin environments,
not one process per plugin. During the busy trial it replaced both the guard and
observer environments. The observer's in-memory event list resets and its file
contains the replacement's `session.start` followed by genuine settled completion.
It is a separate completion handler, not an independently surviving worker or
a complete append-only pre-loss journal. Original guard Stop calls survive on
disk. The verifier requires the replacement timeline, same retained session,
actual debug recovery, exact outgoing feedback and genuine core trace; it does
not invent an initial observer event. Reproduction retains this behavior; the
observer comment was clarified after execution without changing runtime code.

The initial ad hoc process-summary command had a JavaScript syntax error; it
made no mutation. A corrected read captured the results and separate Docker
process inventories were preserved. No submitted native trial was rerun or
repaired. Startup attempts to fetch the default marketplace fail because the
container has no external network; the two local plugins still load and run.
These failures remain in the complete native log.

## Reproduce

Create two fresh containers using the immutable image, ordinary disposable
onboarding and loopback gateway from the
[current Stop protocol](current-stop-failure-20261024.md#reproduce-and-retain).
Do not create managed settings for this probe. Before copying, create node-owned
`guard/{.claude-plugin,hooks}`, `observer/{.claude-plugin,hooks}`, `evidence` and
`workspace` under `/home/node`. Copy:

- SDK `src/integration/agent_tools_completion_claude.js` to
  `/home/node/completion-source.js` (qualifying SDK source `f2d0526`, runtime `9cdc7ab`).
- `current-mod-failure-prepare.mjs` to `/home/node/prepare.mjs`.
- `current-mod-failure-observer.js` to `/home/node/observer/hooks/register.js`.
- Existing `gateway.mjs` to `/home/node/gateway.mjs`.

Run `node /home/node/prepare.mjs pending` or `busy`, retain executable sha256,
start the gateway, then ordinary interactive Claude in the disposable workspace:

```sh
claude --plugin-dir /home/node/guard --plugin-dir /home/node/observer \
  --debug-file /home/node/evidence/native-debug.log
```

Accept only that disposable workspace. Submit `AIKIT_MOD_FAILURE_PENDING` or
`AIKIT_MOD_FAILURE_BUSY` once. Inspect Docker processes while Stop is pending.
Require native settlement within the declared bound; copy the native transcript
matching the retained session, then exit. Archive profile, both plugins, source,
preparer, gateway and evidence with `tar --exclude=latest`; save Docker inspect
metadata as `<case>/container.json` alongside extracted files.

```sh
node verify-current-mod-failure.mjs /retained/mod-failure-308-20261024
node tests/verify-current-mod-failure.test.mjs /retained/mod-failure-308-20261024
```

The private retained root is `.local/mod-failure-308-20261024` in the parent
workspace. Complete archive SHA-256:

- pending: `6cb66635e95125e7eba0d334fdf54d36fdde3d78cd2b4c33253231972bde556d`
- busy: `fc092bd863bb9341724e98f11708e4a17b6747ff0a690295a087d0adac63f2db`

No production code, dependency pin, schema or capability assessment is changed
by these primitive probes. The full generated callback's lost in-memory Turn
context after replacement still needs qualification; it must block rather than
invent recovered authority. These results do not finish N2 or the Windows goal.

The capture verifier and eight altered-evidence checks pass on Linux Node 22
and retained host files. Mutations reject missing worker recovery, false native
core provenance, aborted completion, lost outgoing Block reason, foreign Stop
session, changed fallback, changed native budget and external networking.
Host verification is not a Windows provider run. No unchanged Rust suite is
recounted for harness/document changes. The native probe containers are stopped
after retaining evidence; the owner's application preview remains running.
