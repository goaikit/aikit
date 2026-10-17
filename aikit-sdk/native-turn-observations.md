# Owned native Turn observations

Claude's owned tool module now forwards `turn.start` and `turn.complete` through
the installation's existing fixed hook handler. It reuses the same SQLite journal,
installation revision and inherited invocation generation. It creates no parallel
store or application policy endpoint. Settings-only installations cannot submit
these observations; they require an owned module and matching inherited Turn ID.
These identities are claims, not native-process credentials.

Public normalized evidence:

- `HookEvent::NativeTurnStarted` and `NativeTurnFinished` are observations, not
  decisions or configurable settings hook names. `InstallSpec` refuses them.
- `HookRequest::native_turn: Option<NativeTurn>` carries the native Turn ID and
  optional `NativeTurnOutcome { reason, is_aborted }`. Reasons are `answer`,
  `aborted`, `refusal`, `error`; unknown values fail decoding.
- Classic hook records from an owned module inherit the current native Turn ID.
  A `NativeTurnFinished` frame must match that inherited ID. Raw JSON cannot
  override the installation-selected invocation generation.
- The completion observer awaits the downstream native event chain, retains the
  original result, and attempts exactly one fixed-argv observation. Failure never
  fabricates an outcome or triggers a blind retry. Missing observations leave
  application completion unproven and can be replayed only if already journaled.
- Subagent lifecycle events do not overwrite the main Turn context or emit main
  Turn outcomes. Session replacement still uses the existing generation guards.
- Owned tool calls await their main Turn-start observation before invoking the
  application. Native lifecycle observers are asynchronous and do not themselves
  serialize the model/tool loop. Missing/failed startup refuses the tool; it does
  not retry an application operation or guess its completion.

The native event is not an application completion authorization. A consumer must
match the recorded generation, current native Turn, latest allowed proposal,
authorized application revision, exact answer and unchanged workspace before
creating an accepted end snapshot. An abort, refusal, error, stale outcome,
missing Allow or changed evidence cannot establish successful completion.

Metadata is schema 9; older readers refuse it. Existing rows remain readable
with `native_turn: None` and cannot gain invented correlation. Update owned
modules explicitly through plan/apply before expecting these events. No owner
profile or policy configuration is changed by this source update.

`SuccessfulCompletionObservation` is implemented but **Unknown**, never Supported.
The earlier controlled Linux event probe is candidate API evidence; it does not
qualify this implementation, native Windows, caller protection, notification
delivery/deadlines or competing handlers. The known mod wrapper counterexample
can erase a downstream Stop denial and still prevents unconditional readiness.

References: [native events](https://code.claude.com/docs/en/plugins/mods/events),
[generated declarations](https://code.claude.com/docs/en/plugins/mods/create),
[recorded generation contract](agent-tool-generations.md).

## Validation, 2026-10-17

Rust 1.88 selected SDK integration gate: 92 Windows / 94 isolated Linux passed,
including existing ownership/recovery/generation regressions and new bounded
Turn decoding/environment-matching and unowned-observation refusal cases.
Three Node module behavior tests passed on both platforms, covering downstream
ordering, all outcome reasons, subagent/mismatched IDs and one-attempt failure.
Windows Clippy passed with the existing explicit `uninlined_format_args`
allowance; this is not a claim of strict whole-SDK lint success. Local logs:
`windows-native-turn-sdk-tests-final.log`, `windows-native-turn-sdk-clippy-final.log`,
`docker-native-turn-sdk-tests.log` under the implementation chat's `.local`.
The Linux harness was corrected to include current source files and the exact
ignored lockfile before the passing run. No real native client run of this new
forwarder or Codelaya accepted-completion transition is claimed by this gate.

The subsequent native Codelaya canary exposed startup/tool overlap: get-context
returned unavailable while the Turn-start handler was projecting, and the native
error outcome left work Incomplete. Raw evidence is retained locally in the
implementation chat's `.local/native-completion-generated/start-race-negative`.
The owned tool now awaits that start observation. Four Node behavior tests cover
the barrier, including a deliberately deferred startup and exactly one eventual
tool dispatch.

A subsequent fresh ordinary Claude Linux 2.1.296 session at SDK `3b22c60` /
Codelaya `b83c633` passed actual context/name calls, native UTF-8 Write, fresh Work
Note and correlated accepted completion. The ending snapshot and Completed state
survived normal exit. The verifier passed both inside the isolated container and
on retained host evidence. Replies were scripted; no owner profile/credentials,
permission bypass or managed session. External networking was removed before
trust/plugin loading; the disposable container was removed after evidence capture.
Request evidence SHA-256:
`ebfa815d7f5c19b8b9732b42e457aec251906f97919abe77ca014f9213ae54b0`.
This normal-outcome Linux evidence does not qualify native abort-after-Allow,
competing handlers, protected caller identity/deadlines, review delivery,
real-model semantics, Windows or other providers. Capability remains Unknown.
