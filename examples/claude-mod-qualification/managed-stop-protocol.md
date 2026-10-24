# Native built-in guard / Stop comparison

Executed 2026-10-22, ordinary interactive Linux x86-64 Claude 2.1.296. This
narrows the [unprotected command-hook bypass](stop-chain-protocol.md); it does
not qualify production readiness or Windows. No owner policy/profile was mounted
or changed. No provider credential, paid model or managed agent session was used.

## Hypothesis and controls

The [provider's administration reference](https://code.claude.com/docs/en/plugins/mods/admin)
describes a built-in guard that protects settings hooks under managed settings.
Test the exact existing short-circuit competitor and diagnostic command in fresh
sessions, moving only the settings source. The command's first Stop blocks and
second allows. The same bounded loopback gateway returns `Fixture answer`.

| Run | Hook source | Managed settings | Guard actually loaded | Native result |
| --- | --- | --- | --- | --- |
| user | Workspace | Absent | No | Competitor called; no command calls; one main request; accepted answer |
| managed | Managed | Identical Stop configuration | Yes | Command Block then Allow; competitor Stop skipped; two main requests; block reason propagated; accepted answer |
| empty-managed | Workspace | Empty object | No | Same bypass as user |
| guard-present | Workspace | Only `env.AIKIT_MANAGED_CONTROL=1` | Yes | Command Block then Allow; competitor Stop skipped; two main requests; block reason propagated; accepted answer |

The first two conditions were declared before execution. The third control was
declared afterward to distinguish hook placement from guard loading. Its premise
failed: an empty managed object did not seat the guard, and the initial verifier
rejected the assumed guard condition. The fourth control was declared before
execution using a nonempty unrelated managed setting. The original third run,
protocol amendments and failed verification are retained, not counted as a
guard-present run. Ten total HTTP requests include six main model requests and
four auxiliary requests; the gateway ceiling remained 32. All sessions exited
normally. Replies were scripted, so semantic model behavior is not qualified.

Actual guarded debug logs report `cc-plugin-sec-default@builtin seated outermost`
because of managed settings, the competitor admitted at user tier, and its
`classic.Stop` bypassed by the guard with the underlying command path run.
Both guarded sessions have native Stop inputs, Block/Allow outputs, the blocking
attachment in transcript, the reason in the second outgoing model request, a
successful command attachment and native non-aborted `reason: answer` completion.
The unguarded sessions have successful native completion without command calls.

## Reproduce

Use [ordinary isolated onboarding](native-gateway-probe.md#reproduce) and the
existing pinned image. Copy this fixture into a fresh container home, select
`hooks/short-circuit-stop.js` in its copied `hooks/hooks.json`, and start the
existing gateway. Disconnect external networking before workspace trust. No
host agent profile, credentials or policy mounts are needed.

Use the identical hook configuration for each placement:

```json
{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"node /home/node/probe/stop-chain-command.mjs","timeout":10}]}]}}
```

1. Put it in `/home/node/workspace/.claude/settings.json`, with no managed file.
   Start ordinary interactive Claude with `--plugin-dir /home/node/probe` and
   `--debug-file /home/node/evidence/user-debug.log`. Submit
   `AIKIT_MANAGED_STOP_USER`, wait for completion and exit normally.
2. Move the identical configuration to `/etc/claude-code/managed-settings.json`
   inside the disposable container, root-owned/mode 644; remove the workspace
   copy. Start a fresh session with `managed-debug.log`; submit
   `AIKIT_MANAGED_STOP_MANAGED`, complete and exit.
3. Restore the workspace hook and use `{}` in managed settings. Start a fresh
   session with `empty-managed-debug.log`; submit `AIKIT_MANAGED_STOP_GUARD_ONLY`.
4. Keep the workspace hook; set the container's managed file to
   `{"env":{"AIKIT_MANAGED_CONTROL":"1"}}`. Start another fresh session with
   `guard-present-debug.log`; submit `AIKIT_MANAGED_STOP_GUARD_PRESENT`.

After each run, retain `short-circuit-events.json` as `<mode>-events.json` and
its native transcript as `<mode>-transcript.jsonl`. Save settings at each step
as `user-settings.json`, `managed-settings.json`, `empty-managed-user-settings.json`,
`empty-managed-managed-settings.json`, `guard-present-user-settings.json` and
`guard-present-managed-settings.json`. Retain shared `requests.jsonl`,
`command-stops.jsonl` and Docker's network inventory as `networks.json`.

```sh
node verify-managed-stop.mjs /home/node/evidence
node tests/verify-managed-stop.test.mjs /home/node/evidence
```

The capture verifier and seven negative evidence checks passed both inside Linux
Node 22 and against retained host bytes with Node 20. Mutations reject missing
guard loading, foreign command identity, missing block propagation, false native
completion, missing competitor, changed managed command and external network.
These are verifier checks, not additional native runs. Bulk evidence retention
hit the debug `latest` symlink; named regular files were then copied successfully.

## Consequence for the reusable integration

The later [native Stop failure comparison](stop-failure-protocol.md) observes
missing and timed-out managed commands permit successful completion even with
this guard active and `onFailure: "block"`. Guard attestation alone therefore
cannot close mandatory enforcement for the tested command-hook path.

There is now a demonstrated protected candidate for this competitor. Another
ordinary hook is unnecessary for this case. However, neither a version probe,
a managed file's existence nor configuration copied to disk proves that a live
session loaded the effective guard. The empty-object control demonstrates why.

AIKit needs provider-authenticated effective-configuration evidence tied to the
original session/generation, including guard identity/order and applicable hook
source. Readiness must remain Unknown until that evidence and required native
deadlines/failure behavior, competing handlers and configuration-change windows
are qualified. This report changes no SDK capability status, public contract,
dependency pin, schema or production installation behavior.

The local Windows product cannot silently create/overwrite machine managed
policy. Existing enterprise policy also cannot be replaced by user settings.
Whether a supported non-administrative session can attest equivalent protection
is still open. No owner action is requested by this diagnostic report.

Feedback admission, authoritative acknowledgement/consumption, other versions,
arbitrary competitors, Windows and real-model semantics remain unqualified.

## Identities and retained evidence

Starting AIKit source: `836fa928dad2447c27d57fdce76162897272cb37`.
Image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Claude executable: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Raw synthetic captures, amended protocol, initial failure, final verifier output
and full SHA-256 manifest remain in the parent Codelaya chat workspace at
`.local/managed-stop-20261022`. Only diagnostic source and report are published.

| Evidence | SHA-256 |
| --- | --- |
| Outgoing requests | `5d47e10f101cbb546ce118f17e937a76a8e7206cbbb345081b77e82608a6e523` |
| Command calls | `22527d03f207d6b47a46d6fecb5dea193628d0ab22915c17889dd187dfee5f9d` |
| User events | `be37bc38ea22bed5dcc95b4943b7d4aec4c7ed97c8ece8c76c29d6303e7227e2` |
| Managed events | `c2877c616eb7b62db30a16835e1ab69519723c7cb95ea849a5f76f3e967e468a` |
| Guard-present events | `7c07a3607f4ce951503e4bc474f1a0611e69897a74e62e52c503aab1205f9120` |
