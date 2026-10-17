# Owned completion-loop configuration

`IntegrationService::plan_install_with_completion_policy` reuses the existing
owned prepare/write/commit installation journal. `InstallSpec` is unchanged.
No agent is launched and no policy-support capability is promoted.

```rust,ignore
let plan = service.plan_install_with_completion_policy(
    spec,
    CompletionBlockingPolicy::RequireUnbounded,
)?;
// Inspect plan.config_path, plan.settings and fingerprints before applying.
service.apply_install(&plan.id)?;
```

For Claude, RequireUnbounded owns the local settings entry
`/env/CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` with string value `"0"`. Claude's
[environment reference](https://code.claude.com/docs/en/env-vars) documents zero
as disabling the default eight-continuation cap. Other adapters currently return
Unsupported for this policy. A CompletionProposed hook is required. NativeDefault
makes no new claim about continuation limits.

`InstallPlan.settings` contains exact related key/value changes, preserving
absent versus explicit null values. `Installation.completion_policy` records the
request. Configured means owned bytes match; effective native settings,
permission precedence and unconditional enforcement need separate evidence.

Ordinary `plan_install` updates retain an already selected policy. Explicitly
requesting NativeDefault releases ownership and previews restoration of the old
value. Update/removal restore the prior cap value, environment/container absence
and owned hook entries while preserving unrelated keys. External edits to the
owned setting conflict. A later Stop hook may depend on zero: restoring a limit
while that hook remains is refused. Remove the later borrower first or reconcile
ownership. Updating the original owner while keeping zero remains possible.

The receipt retains a fingerprint of preexisting Stop hooks rather than their
contents. Shared-setting protection also works across separate SDK state
directories. Native profile files are unchanged until an explicit plan apply.
Prepared writes and removals reuse the existing crash recovery mechanism.
Malformed non-string values of the owned environment setting (including null)
are refused without changing configuration. The receipt must never collapse an
explicit null into absence during removal.

## Metadata compatibility

This change originated on `feat/codelaya-integration` at `4b5bb1d`.
PRs #187/#188 merged into that feature branch. Main at `f91c37f`, including its
per-user session-start hook APIs and newer SDK ZIP dependency, was merged into
the parent at `a487c0c` and this child at `6196294` without textual conflicts.
Its project-scoped durable lifecycle service covers more than the startup-only
registration API; reuse shared native format/configuration primitives where their
semantics match, preserving ownership, replay and conflict guarantees.

Integration state advances to schema 6 to protect completion-policy ownership.
Older SDK readers reject that version instead of silently ignoring metadata and
orphaning an owned setting. Coordinate SDK/application upgrades; do not run an
older hook executable against upgraded state. Existing rows/plans are preserved;
missing policy fields default to NativeDefault. Metadata migration changes no
native configuration.

## Recorded validation scope

On 2026-10-17, Rust 1.88 integration unit tests passed on Windows x86_64 (78)
and isolated Linux x86_64 Docker (80, including Unix-only cases). Seven new
policy cases cover preview/update/restoration, absence/empty/preexisting zero,
external drift/stale plans, shared ownership ordering, crash recovery/legacy
records, unsupported/malformed inputs and explicit policy release.

The library-only `integration_hooks plan-unbounded` example wires directly to
the service. A Windows process smoke test used a temporary workspace with spaces
and Unicode, checked preview before apply, applied the zero setting and removed
it while retaining an unrelated setting. Configuration tests use temporary
workspaces; native actions in hook/mod tests are fixtures. No owner profile was
installed or changed.

SDK strict Clippy is not green: the repository has 101 existing SDK
`uninlined_format_args` findings, with additional findings in its workspace agent
dependency. The SDK-only check passed with that one category explicitly allowed:

```text
cargo +1.88.0 clippy --no-deps -p aikit-sdk --lib --no-default-features --features integration --locked --offline -- -D warnings -A clippy::uninlined_format_args
```

### Reproduce the bounded SDK gate

This feature-branch baseline ignores Cargo.lock. Fresh workspace resolution also
encounters its root CLI dependency on yanked zip 0.6.6. Main upgraded the SDK ZIP
dependency to version 9; the root CLI still uses 0.6.6 after reconciliation.
The resolved lock used here
is retained as `qualification/20261017/validation-cargo.lock` with SHA-256
`ef111e91aec4cc31cc1810e077dfc357d1d5010539b7d239434744792c9fbf06`.
In a fresh isolated checkout, copy it to the workspace root as Cargo.lock, then
run the command below. Preserve any existing local lock rather than replacing it
silently. Rust 1.88 is explicit because the repository's toolchain file selects a
newer toolchain. The fixture pins validation inputs; it changes no package API.

```text
cargo +1.88.0 test -p aikit-sdk --lib --no-default-features --features integration --locked integration::
```

The Linux run used the pinned validation image
`sha256:c917b61f52e31ea6a0534cbfcf91f4e2426de4b745ae41ca994cc3a86a46f3bc`,
read-only tracked source plus the new module/lockfile, dependency cache trees and
no network or provider profile/credentials. The fixture lock makes the selected
versions reproducible.

### Reconciled main gate

The combined source at `9a9a726` passed the entire SDK library test suite on
native Windows with Rust 1.88: **594 passed, 4 ignored**. The isolated Linux
container passed **633 tests, 4 ignored**, with networking disabled and no
provider profile or credentials mounted. This includes the newer
session-hook, skill, ZIP and user-folder code alongside the integration service.
The ignored tests require live agents/pipeline behavior and are not acceptance
evidence. Run the same command above without the `integration::` filter to
reproduce this broader gate. The lock snapshot above now records the reconciled
dependencies; the original pre-reconciliation lock remains in Git history.

## Required native qualification

The [mod probes](../examples/claude-mod-qualification/README.md) demonstrate that
a wrapper can erase a downstream Stop denial in Claude 2.1.296's offline plugin
engine. A zero continuation cap does not establish protected middleware order.
The probes also preserve abort correlation and demonstrate receive suppression.
They exercise neither the normal native turn loop nor a model/user session.
Callback, native acceptance, caller generation/admission, instruction installation,
message acknowledgement/consumption/reconciliation and effective settings remain
separate qualification gates.
