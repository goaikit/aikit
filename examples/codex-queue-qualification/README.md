# Native Windows Codex queue feasibility

Qualification tools only. No production transport, installer, capability promotion
or application authority is provided here. Do not point them at an owner profile.
Use fresh disposable workspaces/profiles and synthetic text only.

## Observed result, 2026-10-18

Official standalone `@openai/codex@0.163.0`, native Windows x86-64 executable
SHA-256 `2692c8c7bb2cf8b0aefe638741fc90236f4b824c59578ce84994f9de58200993`.
Ordinary terminal CLI startup created its native background server. The probe
did not start an AIKit-managed agent or use `--no-daemon`, `--remote`, hook-trust
bypass, sandbox bypass, owner credentials or owner sessions.

One terminal baseline completed, then exactly one fixture `codex queue --thread
<verified-fixture-UUID> --message <synthetic-UTF8-text>` command returned a native
queue ID for that original thread. The UI displayed a second answer. Independent
loopback capture recorded the exact feedback once in its next main request.
Native read-only history showed two completed Turns, matching text and a native
client message ID. Queue lookup was empty. Those records survived normal terminal
exit with the native background server still alive. The original target was never
resumed, replaced or resent by the inspection helper.

There were four recorded loopback requests: two main requests and two automatic
title requests. Replies were scripted SSE; no real model or paid endpoint was used.
Native plugin-catalog requests to external services were observed, so this Windows
run is **not** an externally network-isolated test. The fresh profile had no owner
credentials. The loopback gateway refuses Authorization headers without storing them.

### Identity limitation

The first terminal run inherited `CODEX_INTERNAL_ORIGINATOR_OVERRIDE` from the
calling Codex app and recorded originator `Codex Desktop`, source `vscode`.
A fresh independent run cleared that variable plus `CODEX_THREAD_ID` and
`CODEX_SESSION_ID`. Its rollout originator was empty, but native thread source
still read `vscode`. Thus the environment-only explanation did not hold. The
actual terminal command/frontend was observed; the native source label is not
qualified evidence of terminal versus IDE topology. No third retry relabelled
this as full terminal-provider acceptance. The verifier reports the observed label
and explicitly leaves source classification unqualified.

### Other retained failures

- App-bundled `0.163.0-alpha.10.2` (SHA-256
  `9ecb9df2ecafcc00542ef0966c6f1737a81397dc9531b0a4b178238e2bf4a46d`)
  refused daemon startup: no complete local package. No queue/model request.
- The standalone package's first long `CODEX_HOME` failed with `path must be
  shorter than SUN_LEN`. A separate short fixture profile allowed startup.
- The first inspection sent JSON lines directly to the proxy. Native stderr
  reported an invalid WebSocket handshake and the helper timed out at 15 seconds.
  Correct inspection uses WebSocket framing over proxy stdin/stdout. No review
  submission was retried.
- The second case's sandbox-started gateway was unreachable from a normal Windows
  process (three-second GET timeout); the normal-context first gateway returned
  404 as expected. The live native baseline remained pending with zero recorded
  requests. Moving only the owned gateway to normal context on its original port
  allowed that same baseline to complete. A precondition check had refused to
  queue feedback while baseline completion was missing. No feedback resend.
- An npm prefix invocation failed to locate the example package. Installation
  from this example's directory with `--workspaces=false` succeeded.

Both terminal processes exited normally. Only their isolated native daemons and
owned fixture gateways were stopped. Profiles/evidence remain for inspection.
Owner configuration, trust records and policy were not changed.

## Native protocol candidate

### Busy queue, duplicate IDs and restart follow-up

A fresh Windows 0.163.0 fixture held its original baseline model request open.
While that Turn remained in progress, `queue-contract.mjs` intentionally called
native `thread/queue/add` twice with identical client ID/input, then once with
the same client ID and changed text. Each call returned success and a **different
queue ID**. Lookup retained all three entries, with unchanged caller ID and their
respective text. Native client IDs are correlation fields, not unique idempotency
keys; changed content is not rejected by this API.

Those deliberate probes followed received replies, not uncertain handoffs. The
helper deleted all three exact probe entries while the baseline stayed held,
verified an empty queue, then added one distinct client-ID delivery with the
frozen UTF-8 feedback. Lookup confirmed that sole entry while the original Turn
remained busy. After one explicit gateway release, the terminal/native daemon
automatically processed that entry after the baseline. No `turn/start`, queue
start or owner follow-up was sent by the helper. None of the probe text reached
a recorded model request.

History retained the exact chosen delivery client ID and feedback text in the
second completed Turn. Read-only captures matched before and after terminal
exit. The fixture daemon then stopped (old PID 3148); ordinary `codex resume`
for the same fixture UUID started a new daemon. History/client ID and empty queue
survived; request count remained four, so no additional recorded main/title
request was caused by that resume. The new daemon (PID 44628) and owned gateway
were stopped after evidence capture. No owner profile or process was used.

`verify-contract.mjs` passed factual assertions for the native counterexample,
probe deletion before release, sole busy delivery, exact retained client ID,
two unchanged completed Turns across restart and four recorded requests. Its
output explicitly says native same-ID deduplication and changed-content conflict
are **false**. This is not a pass of the application's idempotency requirement.
The same native `vscode` source-label limitation remains.

The SDK must keep `stage_message` immutable and durably `reserve_message` before
native I/O. A lost add reply must use lookup/retained history, never another add.
Multiple matching client IDs, changed bytes, incomplete pages or a changed target
must remain conflicting/unknown evidence. Existing reservations also require an
observed invocation generation; current Codex hooks do not provide that required
generation. The queue probe does not waive that guard, authenticate a native
caller, qualify a protected admission lease or implement the SDK adapter.
Crash during queue addition, lost native response, historical lookup pagination,
second-session/daemon replacement races and independent production consumption
proof remain open.

Local evidence: `.local/codex-queue-contract-20261018`.

| Contract-case evidence | SHA-256 |
| --- | --- |
| Native queue RPC trace | `a2c9aefa4ac94892cb79b62fb23184a7fe7dc510ce102a30bb7a95552d63d1eb` |
| Recorded requests | `44c2ac493a27428ebb60c9c00da920b4edb0f4650627db1a2707e360498df6a6` |
| Inspection after daemon restart | `a51607fa5471df57528ea1d7890117d18a23e93b808e73e56749e3c837e09504` |
| Rollout after restart | `7f20253742e4f091a8faba12daadcda3145de32dc258a0c7e8c5d50e8cc25f3e` |

To reproduce this separate case, start a fresh normal-context gateway with
`node gateway.mjs <new-evidence-directory> 0 busy`, then the ordinary CLI using a
new short profile and that endpoint. Submit the same baseline; confirm
`GET /fixture/state` reports `held:true`. Capture only its verified fixture UUID
in `owner-review.json` as above, then run
`node queue-contract.mjs <native-exe> <fixture-home> <evidence-directory>` once.
The helper preserves replies in `queue-contract.json` even on failure. Do not
rerun it after uncertainty. Its three controlled duplicate/content attempts
must be kept inside the held disposable fixture, never used as owner recovery.

On successful preparation, POST once to `/fixture/release` and retain the 200
status as `release.json` with `http_status:200`. Capture native inspection after
the second Turn, exit normally and capture `after-exit`. Stop only the fixture
daemon with its explicit home, then use ordinary CLI `resume` for its original
UUID without a prompt. Capture inspection with final stage `after-restart`, copy
the sole fixture rollout to `rollout-after-restart.jsonl`, and run
`node verify-contract.mjs <evidence-directory>`. Stop the completed fixture
daemon/gateway. Preserve the pending and post-restart captures separately.

Matching declarations were generated by the actual standalone executable using
`app-server generate-ts --experimental`. Queue methods are experimental:

| Native method/type | Relevant contract candidate |
| --- | --- |
| `thread/queue/add` | Takes `threadId`, input and caller-selected `clientUserMessageId`; returns a queued submission with its own ID, input and client ID. |
| `thread/queue/list` | Returns queued submissions with pagination. Empty queue alone does not establish consumption. |
| `thread/queue/start` | Selects a queued submission; returns a Turn. Busy/competing admission and race semantics remain untested. |
| `thread/read` | Retained user messages expose `clientId`, and Turns expose ID/status. Missing/paginated history must not become invented delivery evidence. |
| `turn/completed` | Correlated status includes completed/interrupted/failed; native event authenticity and gate acceptance still need qualification. |

`app-server proxy` connects to the already running native daemon and forwards
WebSocket bytes. It is not a JSON-line app-server launch. The helper initializes
an experimental client, reads only the selected fixture thread and queue, checks
its canonical workspace, and exits. It never resumes, creates or submits a Turn.

Next SDK work must preserve the native ID counterexample above and qualify
pending versus retained lookup, pagination, uncertain handoff and daemon replacement,
busy admission and concurrent owner input before implementing a supported
existing-session adapter. CLI-generated queue IDs alone cannot implement the
application's preallocated immutable operation/reconciliation contract. Native
caller/generation authentication, actual consumption provenance, repeated Stop
enforcement and installed Codelaya operations remain open. Capabilities stay
Unsupported/Unknown; no production readiness result changes.

## Reproduce safely

1. In this directory run `npm ci --ignore-scripts --workspaces=false`. The only
   helper dependency is pinned `ws@8.22.0`; `gateway.mjs`/`verify.mjs` use Node builtins.
2. Install the official pinned standalone CLI in a separate new fixture directory,
   not globally. Record its lockfile, version, executable path and SHA-256. Generate
   the matching experimental declarations; do not use the alpha build's protocol
   to qualify the standalone package.
3. Choose a new absolute evidence directory with a new `workspace` child and a
   short, separate `CODEX_HOME`. Start `node gateway.mjs <evidence-directory>` in
   normal Windows process context. It writes `gateway.json`; optional third
   argument selects a specific loopback port. Never proxy to a live endpoint.
4. In only that fresh profile configure the generated loopback URL as a custom
   Responses provider: name `Codelaya loopback fixture`, `requires_openai_auth=false`,
   `supports_websockets=false`, request/stream retries zero, analytics off. Select
   fixture model `codelaya-native-fixture`, context window 64000, read-only sandbox
   and on-request approval. Clear API-key variables and app identity variables for
   the fixture child only; preserve policy settings and restore parent variables.
5. Launch the ordinary terminal CLI in the fixture workspace. Type
   `CODELAYA_CODEX_BASELINE: Reply with the fixture answer. Do not call tools.`
   Require completion before queueing. Capture its UUID from only that fixture's
   native records and validate the exact workspace; never select the most recent
   owner session. Prepare `owner-review.json` with `thread_id`, `cli_version` and
   `message: "CODELAYA_CODEX_FEEDBACK: preserve UTF-8 雪 and reply once."`.
6. From another terminal with the same fixture home, invoke `codex queue` once
   using that verified UUID/message. Save stdout as `queue-output.log`. Never
   retry an uncertain result. Wait for the second answer without typing a follow-up.
7. Run `node inspect.mjs <absolute-native-exe> <absolute-fixture-home>
   <absolute-evidence-directory>`. Exit the terminal normally, then run the same
   command with final argument `after-exit`. Copy its sole native rollout to
   `rollout-after-exit.jsonl`, then run `node verify.mjs <absolute-evidence-directory>`.
   Captures use exclusive creation, preserving earlier observations.
8. Stop only the fixture daemon via `codex app-server daemon stop` with that
   explicit fixture home. Stop the owned gateway. Retain failures and successful
   records separately; do not delete or merge owner profiles.

Raw local records: `.local/codex-native-queue-20261018` (inherited metadata) and
`.local/codex-native-queue-clean-20261018` (fresh case, source label still `vscode`).

| Fresh-case evidence | SHA-256 |
| --- | --- |
| Recorded requests | `bec4e2e0ca752a4bdc83775309ffbbb73b5aafe388f573b652f8e1b529002c52` |
| Native inspection after exit | `0a9712008c368592b7f6322083087492a1a374aa0d8194e65d30125c1a7b8a9d` |
| Native rollout after exit | `fe554736b0df66768a079327556f29514873e80a86e3aa3190b64e4ae24baaad` |

Official references inspected on 2026-10-18:
[hooks](https://learn.chatgpt.com/docs/hooks) and
[app-server protocol initialization](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server).
Hook docs still describe a competing Stop `continue:false` overriding continuation;
schema/probe results do not close that enforcement gap.
