# Current native guarded Stop failure qualification

Declared before execution, 2026-10-24. Recheck N2 on installed Claude 2.1.308;
do not transfer the earlier 2.1.296 counterexample to a new version.

Use three fresh ordinary interactive Linux sessions in the immutable image
`sha256:9312badacd0cdf5496308a7292a5dc9965d25c616dbda94764ee10bbe8c30924`,
network none, no mounts, disposable node profiles and public dummy key against
the bounded loopback gateway. No owner settings/policy/login or Windows checks.
The only managed file is root-owned `/etc/claude-code/managed-settings.json`
inside each disposable container. This tests native primitives, not a Codelaya
installation, application completion or production activation.

All cases load the same deliberate user-tier short-circuit Stop competitor via
`--plugin-dir /home/node/probe`. Managed Stop command hooks have timeout one
second and `onFailure:"block"`. Require actual built-in guard loading and bypass
of the competitor, not just declared settings. Only the command differs:

| Case | Command | Expected observation to test |
| --- | --- | --- |
| baseline | Existing first-Block/second-Allow command | Two native calls; first reason reaches a second main request; native core completion only after Allow |
| missing | Node with intentionally missing script | Does a failed managed command prevent completion despite Stop's documented onFailure exclusion? |
| timeout | Existing command records startup, waits 1500 ms, then intends Block | Native cancellation at 1000 ms; determine whether completion proceeds without any delivered Block |

Each session receives one `AIKIT_STOP_FAILURE_<CASE>` prompt. Preserve native
HTTP/debug/transcript, version/executable hash, actual managed settings ownership,
command inputs/results and settled engine/core completion traces. No retry or
in-place repair of submitted trials. Fail/inconclusive if guard, identity,
request classification, command evidence or actual completion is missing.
The main-loop request count must come from debug source and captured requests;
title/hello requests are separate. Gateway cap stays 32 requests / 4 MiB each.

This can qualify one protected ordering mechanism or establish a precise failure
counterexample. It cannot prove all native APIs lack enforcement. Keep production
capabilities unqualified and investigate other existing mechanisms before any
broader provider conclusion. No new auth/policy framework is proposed.

## Results

The first three trials failed the predeclared ownership gate: `cp` preserved the
node user's ownership (uid 1000). The native guard loaded, but those trials are
not counted as the root-owned managed configuration. Their complete evidence is
retained unchanged. Three fresh corrected trials explicitly set root ownership
and mode 0644 **before** native startup; capture and verification require it.
The reusable root setup helper now refuses an existing managed file and verifies
ownership, preventing this harness error on reproduction.

| Corrected case | Session | Main requests | Observed command and native outcome |
| --- | --- | --- | --- |
| baseline | `7f4b7919-a306-42c2-bc1a-f1c51216d82f` | 2 | First Block reason reaches second request; second Allow; settled engine/core answer |
| missing | `1ecac71e-2b43-44f5-a0fa-6a3221699b09` | 1 | Missing module, exit 1; native log says onFailure block ignored on Stop; settled engine/core answer |
| timeout | `82a1deb7-b728-4fe0-b414-16c17f1b5062` | 1 | Command starts, native cancels at 1000 ms before intended Block; settled engine/core answer |

All corrected cases actually load `cc-plugin-sec-default@builtin` outermost and
bypass the competitor. Its short-circuit Stop handler never executes. Every
completion observer awaits `next(event)` and retains a matching returned native
engine/core trace, positive token accounting and non-aborted answer. Captured
HTTP requests agree with debug request sources: four main and three title calls
across corrected runs, below each gateway's 32-request ceiling. All sessions exit
normally. Scripted fixture replies are not real-model semantic qualification.

This confirms the current documented exclusion of Stop from
[`onFailure: "block"`](https://code.claude.com/docs/en/hooks#block-the-action-when-a-hook-fails).
It closes the tested competitor-ordering question using an existing guard, but
**does not close N2** for managed command failure. It does not show a false
Codelaya Completed record: Codelaya is not installed in these primitive trials.

The precise enhancement candidate for this path is a host-enforced mandatory
completion gate whose missing command, timeout or worker loss produces a
correlated error/incomplete native outcome instead of successful acceptance.
Do not implement this as unbounded model retries to repair a missing executable.
The [mod failure handler](https://code.claude.com/docs/en/plugins/mods/events#handle-a-hook-that-fails)
is a different existing mechanism: AIKit already emits its constant denial
fallback. Its protected ordering, whole-worker loss and final settlement still
need qualification. These command results do not prove all Claude APIs lack a
solution. N1 and N3 likewise remain specific open contracts, not blanket provider
blockers.

## Reproduce and retain

Create three fresh containers from the pinned image above with `--network none`,
no mounts, `--user node`, disposable `/home/node/workspace`, public fixture key
and `ANTHROPIC_BASE_URL=http://127.0.0.1:8787`. Create the probe directories as
node before copying scripts. For each case:

1. Copy this directory's `gateway.mjs`, `stop-chain-command.mjs`,
   `stop-timeout-command.mjs`, `current-stop-prepare.mjs`,
   `current-stop-managed-setup.mjs` and `current-stop-capture.mjs` to
   `/home/node/probe`. Copy `current-stop-observer.js` to
   `/home/node/probe/hooks/register.js`; create `.claude-plugin` first.
2. As node, run `node /home/node/probe/current-stop-prepare.mjs <case>`.
   As container root, run `node /home/node/probe/current-stop-managed-setup.mjs`.
   Record `sha256sum /usr/local/bin/claude` in `evidence/native-sha256.txt`.
3. Start the gateway as node: `node /home/node/probe/gateway.mjs /home/node/evidence`.
   Start ordinary interactive Claude in the workspace with
   `--plugin-dir /home/node/probe --debug-file /home/node/evidence/native-debug.log`.
   Accept this disposable workspace, submit the declared case prompt once,
   wait for completion and run the capture helper as node. Exit normally.
4. Retain Docker inspect output as `<case>-root/container.json`; copy the
   regular evidence files into `<case>-root/evidence/`. Archive profile, probe
   and evidence using tar `--exclude=latest` to exclude the debug symlink.

Run against the retained parent directory:

```sh
node verify-current-stop-failure.mjs /retained/stop-failure-308-20261024
node tests/verify-current-stop-failure.test.mjs /retained/stop-failure-308-20261024
```

Both Linux Node 22 and host Node verification pass, including eight altered
evidence rejection checks (ownership, guard, genuine core trace, completion,
Block propagation, late timeout result, configuration and external network).
Host execution only verifies retained files; Windows provider checks remain
deferred. No unchanged Rust regression suite is recounted for these harness edits.

Private evidence is retained under `.local/stop-failure-308-20261024` in the parent
workspace. SHA-256 of each complete archived run:

| Archive | SHA-256 |
| --- | --- |
| baseline, ownership gate failed | `bee9c0fe5b0553eb3773b9f7d30f84e5cfae6408eeee2f01b6c8e83a2bc6e756` |
| missing, ownership gate failed | `d4eb5ffc15e2dbb8cbaf1f670f7f2efc76b603cbee7b81bccd5ca64b9a625428` |
| timeout, ownership gate failed | `d76f478eae5db3de9493bca8062ccd54c52887558d8543a0a1c0b96ff2986f8f` |
| baseline-root | `be32cb4918cf3a62d26935a2f7509866b99f4466acca1cc9a6b65170fb6c8321` |
| missing-root | `686661ad125e2260d274436eb8c0470121127b9da2c79eb78b718657dad15e17` |
| timeout-root | `3728003ea4bed9a8793783eff51e5b8d3ab5b931b96139ba788742b073e67b1e` |
