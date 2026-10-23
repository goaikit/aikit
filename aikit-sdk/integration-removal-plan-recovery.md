# Unstarted removal plan recovery

Runtime source `e193d56` strengthens the existing public
`IntegrationService::plan_remove(installation_id)` contract. No new provider
transport, consumer-side SDK database access, capability or schema is introduced.

An owner removal could lose its reply after the SDK persisted a `planned` row.
The prepared-plan lookup deliberately does not return previews. Previously a
repeat `plan_remove` created another UUID: removal could finish while leaving the
first row unfinished, which blocks a later explicit SDK schema upgrade.

The SDK now reuses one unstarted removal only when its complete recipe matches:
installation/configuration identity, preview metadata, exact prior receipt,
before/after bytes, deletion mode, workspace, owned tool-file changes, detachment
requirement and request identity. Only the SDK-generated preview ID is excluded
from the comparison. Existing configuration/tool fingerprint checks still run
before reuse. The persisted row/ID is returned without inserting another plan.

Changed configuration fails with the retained plan ID and no writes. Multiple
existing unstarted removal previews fail for inspection; they are not silently
deleted or consolidated. This prevents new orphan plans in the qualified retry
path; it does not repair historical duplicate previews. It does not authorize
automatic restoration of owner settings or cancel prepared/applied work.

Serialized fields and schema 15 remain unchanged. `InstallPlan` and
`AgentToolFileChange` gain additive `PartialEq`/`Eq` implementations for typed
recipe comparison; the owned-tool-file comparison is private. Prepared removal
recovery and installation/update preview behavior remain separate.

## Linux qualification

Isolated offline Rust 1.88, no real provider session or owner profile:

- Both new regressions failed on `c2fa381`: repeated planning returned different
  IDs, and an owner configuration edit silently produced another preview.
- **155 SDK integration tests passed** after the fix. The new tests reopen the
  SDK between plans, verify one unfinished row, apply the original ID, verify zero
  unfinished rows, and preserve owner configuration. The changed-owner case
  refuses replacement, reports the original ID and retains the exact bytes/row.
- Scoped library Clippy with `--no-deps` completed with **101 existing warnings**;
  this is not a warning-denied pass. The separately known strict lint gate remains
  open. No warning was reported in the changed installer file.
- The first command encountered the repository's unavailable pinned toolchain;
  execution explicitly selected the already installed Rust 1.88. No download or
  owner toolchain/configuration change was used.

```sh
cargo +1.88.0 test --locked --offline -p aikit-sdk \
  --no-default-features --features integration --lib integration::
cargo +1.88.0 clippy --locked --offline -p aikit-sdk \
  --no-default-features --features integration --lib --no-deps
```

Codelaya's consumer qualification additionally kills an actual unit-test child
at removal boundaries; its results and dependency pin are recorded in that
repository. SDK unit results alone do not establish that application process
recovery, native identity, hook policy or Windows acceptance passed.
