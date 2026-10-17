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
