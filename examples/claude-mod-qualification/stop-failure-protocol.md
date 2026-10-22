# Native guarded Stop command failure

Executed 2026-10-22, ordinary interactive Linux x86-64 Claude 2.1.296. The
[guard comparison](managed-stop-protocol.md) resisted the tested short-circuit
competitor; this follow-up tests native command failure with that guard active.
It does not qualify Codelaya or Windows and changes no owner policy/profile.

## Predeclared hypothesis and observed result

The [provider's hooks reference](https://code.claude.com/docs/en/hooks#block-the-action-when-a-hook-fails)
excludes Stop from `onFailure: "block"`. Test the pinned executable before treating
guard/configuration evidence as sufficient. Use three fresh sessions, the same
short-circuit competitor, managed Stop settings, timeout one second and
`onFailure: "block"` in every case. Only the command changes:

| Case | Command behavior | Actual command evidence | Native result |
| --- | --- | --- | --- |
| baseline | First Block, second Allow | Two native Stop inputs, correct outputs | Two main requests, reason propagated to second, successful completion |
| missing | Node invoked with an intentionally nonexistent script | Native module-not-found failure, status 1 | One main request, successful completion without Block |
| timeout | Record input, wait 1500 ms, then intend to print Block | One started record; native cancellation after 1000 ms; no late Block record | One main request, successful completion without Block |

All three debug logs show the built-in guard seated outermost, the competitor
loaded and its Stop handler bypassed by the guard. All three native completion
events report `reason: answer`, `isAborted: false`, nonzero usage and the fixture
answer; transcripts record completion. The baseline transcript contains a
blocking attachment and successful command attachment. The timeout transcript
contains `hook_cancelled`. Failure logs explicitly say the command failed/timed
out without blocking because `onFailure: "block"` is ignored on Stop.

The timeout command was cancelled before its intended delayed response. This
does not claim a completed late callback was delivered or that Codelaya falsely
marked an application Turn Completed. Codelaya was not installed. It demonstrates
that the command-hook primitive does not enforce native completion on these two
failure paths even with the tested native guard active.

Seven total loopback requests comprised four main requests and three auxiliary
requests; the gateway ceiling remained 32. Replies were scripted, no paid model
or provider credentials. External networking was disconnected before workspace
trust. All sessions exited normally. No failure was repaired in place during a
session, no output was relabeled, and the original observations remain retained.

## Reproduce

Follow [ordinary isolated onboarding](native-gateway-probe.md#reproduce) in a
fresh pinned container without owner mounts. Select `short-circuit-stop.js` in
the copied fixture's `hooks/hooks.json`. Run the existing loopback gateway,
disconnect external networking before trust, and use this root-owned container
file at `/etc/claude-code/managed-settings.json`:

```json
{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"node /home/node/probe/stop-chain-command.mjs","timeout":1,"onFailure":"block"}]}]}}
```

Start each ordinary interactive Claude session with the public fixture key,
loopback base URL, `--plugin-dir /home/node/probe` and a distinct debug file.

1. Submit `AIKIT_STOP_FAILURE_BASELINE`, wait for two answers and exit normally.
2. In the container's managed file, change only command to
   `node /home/node/probe/intentionally-missing-policy.mjs`. Do not create that
   script. Start a fresh session, submit `AIKIT_STOP_FAILURE_MISSING`, wait and exit.
3. Change only command to `node /home/node/probe/stop-timeout-command.mjs`.
   Start a fresh session, submit `AIKIT_STOP_FAILURE_TIMEOUT`, wait and exit.

After each run, save `short-circuit-events.json`, its actual native transcript,
managed settings and debug log as `<case>-events.json`, `<case>-transcript.jsonl`,
`<case>-settings.json` and `<case>-debug.log`. Retain shared `requests.jsonl`,
`command-stops.jsonl`, `timeout-command.jsonl`, and Docker network inventory as
`networks.json`. Copy named regular debug files to Windows to avoid the `latest`
symlink; no symlink privilege or policy change is required.

```sh
node verify-stop-failure.mjs /home/node/evidence
node tests/verify-stop-failure.test.mjs /home/node/evidence
```

The capture and six negative evidence checks passed in Linux Node 22 and against
retained bytes in host Node 20. Mutations reject missing guard, removed failure
policy, foreign timed-out invocation, failed native outcome, missing effective
timeout and missing baseline reason propagation. These are verifier checks, not
six additional native runs. No production readiness follows from a verifier pass.

## Reusable SDK consequence

The existing exact-context `HardHookDeadline` assessment now exposes this failure
diagnostic and remains Unknown. The repeated-completion assessment includes the
guarded controls and does not overstate the unguarded bypass as universal. No
new public API, storage schema or activation exception is introduced. Other
provider versions, platforms and modes retain independent unqualified reports.

The existing scoped capability test checks both diagnoses against matched and
unmatched contexts, with required readiness still failing. A version-only probe
reports known evidence; it is not a live guard or failure qualification.

Final isolated offline Rust 1.88 validation passed eight scoped capability tests,
strict SDK library Clippy (warnings denied, established uninlined-format-args
allowance), and formatting of the changed Rust file. A workspace-wide formatting
check reported unrelated baseline differences/configuration warnings; it is not
claimed green. Final source commands:

```sh
rustfmt +1.88.0 --edition 2021 --check aikit-sdk/src/integration/capabilities.rs
cargo +1.88.0 test -p aikit-sdk --no-default-features --features integration --lib integration::capabilities:: --locked --offline
cargo +1.88.0 clippy -p aikit-sdk --no-default-features --features integration --lib --locked --offline -- -D warnings -A clippy::uninlined_format_args
```

Meeting mandatory enforcement requires a native failure path that refuses
completion, or another qualified provider mechanism with that guarantee. Merely
attesting settings, increasing timeout, adding `onFailure`, trusting successful
completion, or marking the application Turn Incomplete cannot establish that
native guarantee. Investigation of other supported mechanisms remains open.
Protected idle admission and authoritative feedback acknowledgement/consumption
also remain open. Windows checks remain deferred.

## Identities

Starting AIKit source: `6d99567e8b84da4f7657872dd7cc482d7900056f`.
Image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Claude executable: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Raw synthetic captures, predeclared protocol, generated version-matched types,
source hashes and verifier logs remain at `.local/stop-failure-20261022` in the
parent Codelaya chat workspace. Only diagnostic source/report is published.

| Evidence | SHA-256 |
| --- | --- |
| Requests | `d719c39bfccdc6aaa5cfe0fa466a716bd5ae1d194f16e60126fc67a4f414127f` |
| Baseline command calls | `b958beb0d2b5511d567d529d65ab9d427446ee6298ee1294704376f17c0ca27e` |
| Timed-out command input | `7da82845a24d92931d6209d7473c5c2e0ec0ac8ea5d09bd217091e33d9df7fef` |
| Missing-command native events | `9d6c4990958f3bb280692c3500d702fe82ad9a3177916e1470e7871eebaa6227` |
| Timeout native events | `3d75947ea772a06481bc89e82d5df1456b6da6a1a18f707bcc10b498a34ad309` |
