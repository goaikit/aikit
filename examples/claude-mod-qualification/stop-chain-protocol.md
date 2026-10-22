# Native Stop chain protocol

Predeclared diagnostic, 2026-10-22. Use fresh ordinary interactive Claude Linux
2.1.296 in the pinned credential-free native image, no external networking after
onboarding. This does not qualify Codelaya, Windows or another provider version.

The command hook records native Stop input and returns Block on its first call
per session, Allow on its second. The existing loopback gateway returns only
`Fixture answer`. First run a baseline without mod middleware, then a new session
with only `hooks/short-circuit-stop.js` selected in the disposable plugin copy.
Keep the exact same command hook settings and endpoint for both sessions.

Prediction to test: a competing middleware Stop handler returning `{}` without
calling `next` may bypass the command hook. A bypass requires the competitor's
record, zero command Stop calls for that session and accepted native completion.
If the first command Block is recorded and a second main model request follows,
the predicted command-hook bypass was not observed. Missing events, request
errors or missing completion make the case inconclusive, not passing.

The baseline must first show two command calls (Block then Allow), two main
model requests, the unique block reason in the second request and an accepted
completion/transcript. Retain both sessions, outgoing requests, native events,
settings, source/image identities and failed checks. Bound each run at 32 total
HTTP requests using the existing gateway. No paid model, permissions bypass,
owner profile, credential, plugin policy or production activation is used.

This tests actual command-hook independence from one competing mod strategy.
It does not establish all-handler protection, effective deadlines, caller
authentication, semantic note enforcement or delivery/consumption guarantees.

## Observed result

The predicted bypass **was observed**. The baseline recorded first Block and
then Allow, two main requests and the blocking reason in the second. The
competitor recorded one short-circuit Stop callback, zero command calls and one
main request. Native `turn.complete` reported `reason: answer`, `isAborted: false`,
`Fixture answer` and nonzero usage. Its transcript recorded Turn duration and no
Stop command attachment. Both sessions exited normally. Two auxiliary title
requests (without tools) are excluded from main counts; five HTTP requests total.

The baseline has command results and transcript completion, but no mod outcome
event because it intentionally loads no mod. The competing case has the native
outcome event. Codelaya itself was not installed: this is a counterexample to
protection of its command-hook primitive, not a falsely Completed Codelaya Turn.

The verifier passed against original and retained bytes in Linux. Six verifier
checks passed: capture plus rejection of missing competitor callback, failed
outcome, foreign session, extra command invocation and missing block propagation.
These mutations are not additional native runs.

The initial competitor launch did not select the intended module: the copied
manifest was root-owned and selection failed with EACCES. No prompt was sent.
That session was closed, ownership of only the copied container directory was
corrected, and the intended session started afresh. Its failed startup log is
retained. Bulk evidence copy encountered the debug `latest` symlink; named
regular files were then copied and verified. No host policy/profile was changed.

## Required provider/AIKit enhancement

Claude needs a non-bypassable policy command-hook path, or an attested effective
configuration boundary excluding middleware capable of skipping it throughout
the decision interval. AIKit must qualify that guarantee before promoting
required repeated completion blocking. Adding another ordinary handler, trusting
Stop Allow, observing eventual success or relaxing Codelaya rules is insufficient.
The SDK's repeated-blocking assessment remains Unknown and fails readiness.

## Reproduce

Use [ordinary isolated onboarding](native-gateway-probe.md#reproduce) in a fresh
empty container without host mounts/profile. Disconnect bridge, attach only
`none` before workspace trust, and copy this fixture to `/home/node/probe`, owned
by container user `node`. Start the existing loopback gateway and create
`/home/node/workspace/.claude/settings.json`:

```json
{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"node /home/node/probe/stop-chain-command.mjs","timeout":10}]}]}}
```

Start ordinary interactive Claude with the public fixture key/base URL from the
runbook, without a plugin argument. Submit `AIKIT_STOP_CHAIN_BASELINE`, wait for
two answers and exit normally. Save its transcript as `baseline-transcript.jsonl`.
Set the copied `hooks/hooks.json` to `{"modules":["./short-circuit-stop.js"]}`.
Start a fresh session with `--plugin-dir /home/node/probe`, submit
`AIKIT_STOP_CHAIN_COMPETITOR`, wait for its answer and exit. Save its transcript
as `competitor-transcript.jsonl` and the shared workspace settings as `settings.json`:

```sh
node /home/node/probe/verify-stop-chain.mjs /home/node/evidence
node /home/node/probe/tests/verify-stop-chain.test.mjs /home/node/evidence
```

## Identities and limits

Native image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Claude binary: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Raw synthetic contexts and verifier results remain in the parent workspace's
`.local/stop-chain-20261022`. Only diagnostic source/report is published. Windows,
real-model semantics, arbitrary plugin combinations and production readiness
are not qualified. The disposable container was removed after retention.

| Evidence | SHA-256 |
| --- | --- |
| Outgoing requests | `d2335d517d7f6ee83e948a9943781086d8b7835bc2ba822b87c1a0638c56f200` |
| Command hook calls | `59e26663ba46295bea8f2ce0ebde754eea14926fdb033238100f6ff5085b2a48` |
| Native competitor events | `ab991d7c5fd06ae9738c146f2fe882382453d2a293464ddc690148522d124232` |
| Baseline transcript | `8d293e1781a3d3ab6713a5bd1dbae7d62f891ab322179b2dec895500c6311e79` |
| Competitor transcript | `c6968579f8d9ffcfe898e98674d4370eb4e054cc968ed785f0bf86b6d2c02c5b` |
| Shared settings | `dcfc67824e872980b14d3aea2d883645c84759fb477eef4b308daef8fbd2f29f` |
