# Generated SDK Stop callback native comparison

Predeclared 2026-10-22. Ordinary interactive Linux Claude 2.1.296, pinned native
image, no owner mounts/credentials and scripted bounded loopback replies. Build
the real `integration_hooks` and `agent_tool_plugin` SDK examples from `a922431`.
Copy them to `/home/node/bin` in a fresh native fixture container. Use normal
onboarding and disconnect external networking before trusting any workspace.

For each mode below, run `node setup-generated-stop.mjs MODE`. It creates an
empty Git checkout, SDK installation and generated opt-in module via existing
plan/apply calls. The retained project command Stop hook remains installed.
A disposable directory marketplace and user prepend list load that same owned
module first. Start Claude in the reported checkout, with no `--plugin-dir`,
submit `AIKIT_GENERATED_STOP_MODE`, await completion and exit normally. Retain
debug log, native transcript, SDK events, wrapper calls, settings and setup.

| Mode | First Stop callback | Hypothesis |
| --- | --- | --- |
| baseline | Actual SDK example policy blocks its first proposal | Native second request contains policy reason; later mod and command validations both Allow |
| invalid | Wrapper returns invalid JSON before SDK invocation | Generated constant catch blocks, then subsequent module/command callbacks Allow |
| timeout | Wrapper waits 1500 ms against module timeout 1000 ms | Generated callback blocks; retain actual cancellation/timeout evidence and check for late output |

The wrapper delegates all ordinary events to the real SDK hook service and logs
synthetic frames. It injects a failure only on its first native Stop. It is not
Codelaya's policy handler. Count native API requests independently of callback
counts; the retained command hook can cause an extra validation after mod Allow.
Do not infer successful native execution from module source or setup plans.
No production readiness, caller authentication, effective-ordering attestation,
arbitrary competitor matrix, real-model semantics or Windows proof follows.

## Observed result

Three submitted native Turns completed successfully after two main requests
each. Nine total requests include three auxiliary calls. The generated module
loaded at `tier prepend` in all three submitted runs. The baseline retained SDK
decisions Block, Allow, Allow: its first mod callback blocked, its second
delegated, and the project command hook validated again. Each failure case had
no SDK policy invocation for the injected first Stop; the generated catch
supplied Block, then the later mod and command invocations recorded Allow.

All three native transcripts and independently captured second requests contain
the exact policy/fallback reason. SDK start, proposal and final outcome records
share session, invocation and native Turn identities; outcomes are non-aborted
`answer` with `Fixture answer`. This is correlation, not protected caller proof.
The timeout mod settled in about 1037 ms; its catch answered after the configured
1000 ms process deadline. The wrapper recorded `delayed`, no `after_delay` and
no first SDK response. No successful late response is claimed, and this is not
an all-descendant cleanup or power-loss test.

An initial invalid-case startup reused the baseline's marketplace name with a
different path. Native cached lookup skipped the requested prepend id and
loaded its skills-directory copy at user tier. No prompt was submitted in that
session; it exited normally. The fixture was amended between sessions to use
one marketplace name per case, and a fresh invalid session had the required
tier before submission. Retain the initial setup source and premise debug log.
The raw invalid debug file contains both startups; `final-debug.log` is exactly
the suffix after the retained premise bytes, and the verifier checks that split.
The baseline used the original setup helper; the other cases used the amended
helper. Generated SDK bridge source was unchanged throughout.

The initial verifier assumed the command string included arguments. It failed
against the SDK's actual separate `command`/`args` representation. The amended
check requires the exact executable, arguments and timeout; its initial failure
is retained. No native result was repaired or relabeled. The final capture and
four negative evidence checks passed on Linux Node 22.23.3 and retained host
Node 20.19.0. They reject erased fallback propagation, changed retained command
arguments, a late callback record and a foreign native completion. These are
verifier checks, not four more sessions.

```sh
node verify-generated-stop.mjs /home/node/evidence
node tests/verify-generated-stop.test.mjs /home/node/evidence
```

This closes the generated-module/example-handler execution gap for the tested
baseline and two failures. It does not qualify Codelaya's actual policy handler,
protected identity/configuration, cancellation/new-Turn races, process loss,
repeated blocking beyond one continuation or feedback acknowledgement/consumption.
Those remain required before production activation. The retained command hook
and duplicate validation must remain part of subsequent Codelaya qualification.
No SDK capability or Windows package is promoted.

## Identities and evidence

SDK source: `a92243182b3b3e3f6162bc32a2ed0cffcb4b156e`.
Build image: `sha256:c917b61f52e31ea6a0534cbfcf91f4e2426de4b745ae41ca994cc3a86a46f3bc`.
Native image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Claude executable: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Build used the same retained ignored lockfile as
[the implementation gate](../../aikit-sdk/native-completion-bridge.md#scoped-implementation-qualification-2026-10-22).
Raw binaries, build output, generated files, native logs/events/transcripts,
requests, initial setup source, initial verifier failure, final checks and hashes
are retained at `.local/generated-stop-20261022` in the parent Codelaya workspace.
Only the synthetic fixture sources/report are published.

| Artifact | SHA-256 |
| --- | --- |
| SDK `integration_hooks` | `9aa835861259717b1b0d1f604c4a2b600ff5104181c586c6b9f078a6f98ffefe` |
| SDK `agent_tool_plugin` | `136b48b4e2e86cf74cd2c81254a860dd28b7f5bbe5e0ec45d6c276f9a72d82aa` |
| Requests | `37f7097e7358c93b6836e36a108b0385c4e063523ebfb632c27eabe9d5a68707` |
| Baseline generated module | `07b6f134e0fa5c71b07bcac90ddff8d9a1fc429318c221f39125910d19b809f2` |
| Invalid-response generated module | `d7592bef36daecc74ca7796fe83050d9d820855d4b3c971c04dd4f94de13937d` |
| Timeout generated module | `663b005ffa94b5266f2c65e41771856b9a2bdd842a7753c505e6b1d0dc9488d8` |
