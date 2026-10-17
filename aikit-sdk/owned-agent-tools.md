# Owned native tool files

The current schema is 9: owned tools bootstrap inherited recorded generation
context as described in [tool generations](agent-tool-generations.md), and forward
[native Turn observations](native-turn-observations.md). The
ownership gate below originally introduced schema 7 and retains its own evidence.

`IntegrationService::plan_agent_tool(installation_id, spec)` adds or updates the
generated tool and instructions on an existing configured hook installation.
The spec's application and installation IDs must match that receipt. Other
providers retain the renderer's explicit Unsupported result. The example
`integration_hooks plan-tool STATE INSTALLATION_ID` reads a bounded `AgentToolSpec`
from stdin and delegates to this method.

```rust,ignore
let plan = service.plan_agent_tool(&installation.id, tool_spec)?;
// Inspect plan.files: exact paths and before/after fingerprints, no contents.
let status = service.apply_install(&plan.id)?;
// Existing removal covers owned hook settings AND owned tool files:
let removal = service.plan_remove(&installation.id)?;
```

## Ownership and recovery

This extends the existing installation receipt and `install_plans` journal;
there is no second installer, registry, database or credential framework. Metadata
schema 7 makes older readers refuse these receipts rather than orphaning files
or ignoring prepared recovery. Upgrade all participating application/hook binaries
together before using this state. Existing version 6 receipts/plans keep their
history and default to no tool ownership.

Files live under `<workspace>/.claude/skills/<generated-plugin-name>/`:
`.claude-plugin/plugin.json`, `hooks/hooks.json` and `hooks/register.js`.
Existing unowned directories are refused, including empty ones. Every owned file
must match its receipt before update/removal. Legacy hook updates retain owned
tool files and check their fingerprints. A disabled/missing native plugin or a
managed policy can still prevent loading: Configured describes owned bytes only.

Apply rechecks the configuration, all changed and unchanged owned tool files,
and the absence of a new unowned root before mutation. The durable prepared
phase precedes file writes. Module/configuration files precede manifest
publication; removal withdraws the manifest first. Native decisions and recorded
admission refuse a prepared installation. The receipt and installation revision
commit only after the writes; old bindings become stale. Coordinate updates with
idle/detached sessions rather than upgrading beneath active work.

Recovery resumes the same opaque plan ID and accepts only exact before/after
bytes for each planned file. External edits refuse recovery without adopting or
overwriting them. Completed plan bodies discard generated contents and old
receipt bodies. Removal deletes only owned files and prunes their empty owned
directories. Unrelated files/directories and settings survive; no recursive
directory replacement/deletion is used. Cooperating writers share the existing
locks; arbitrary external editors must still be coordinated around filesystem
replacement, as with existing hook configuration.

AIKit's existing `deploy_skill_entry` helper replaces directory entries without
these receipts or crash recovery. It is unsuitable for this ownership contract.
The installer instead reuses its own path checks, fingerprinting, atomic file
replacement, locks, persisted plans, revision invalidation and removal logic.

## Native discovery and evidence, 2026-10-17

Claude documents project skills-directory plugin discovery, workspace trust and
primary-directory restrictions in its
[loading reference](https://code.claude.com/docs/en/plugins/loading#plugins-shared-through-a-repository).
Start from the registered checkout root; plugins are not searched in parent
directories like plain skills. No marketplace or profile-wide installation is
needed. A documented discovery mechanism is not qualification for every version,
platform, policy or checkout form.

The selected Rust 1.88 integration gate passed 88 Windows / 90 isolated Linux
tests. Four added cases cover preservation/update/removal, all three partial-file
recovery boundaries and reinstall, stale/unowned scope and edits during recovery.
The stale-plan case also checks a newly appeared directory and a changed owned
file whose desired bytes were unchanged. Tests inject faults at actual journal/
file boundaries; they are not OS process-kill evidence. SDK/example Clippy passes
with the existing explicit `uninlined_format_args` allowance.

A separate real Linux Claude 2.1.296 terminal discovered the SDK-installed project
plugin without `--plugin-dir`, after ordinary trust. The Rust inspector's exact
Unicode result appeared once in the next actual API input; application schema,
instructions, unrelated CLAUDE.md context and native session IDs matched. Native
`plugin list --json` reported the project plugin before SDK removal and an empty
list afterward. The removal verifier checked unrelated settings/files. Replies
were scripted, Docker networking was disabled before trust/loading, and no owner
profile or account credentials were mounted. The owned container was removed.

Request evidence SHA-256:
`afe39ab7ba7eca9e7349d7dfae958333688a562d992cb0be89c7f929399b5dd1`.
Rust inspector SHA-256:
`df2ae1698e3001eed3c31602959609da1610ac94c680362cb0e24341d8e73b03`.
Installer example SHA-256:
`f97d2ba8274d3f7df90a61d6a7478055f99514d1afcb60ab6f9a461eec15a0f0`.
Native client/image identity remains the same as the
[generated bridge fixture](examples/agent-tool-qualification/README.md).

That native run used installer-source SHA-256
`70a6b73934868c69dfc8d3c3f6ed10dbdbb6608be1df1bcc347971fead2a1b50`.
Two subsequent apply preflight guards (new unowned root / unchanged-file drift)
are covered by the final Rust gates; the generated plugin and ownership helper
were unchanged. This is controlled discovery/transport/removal evidence, not a
full native rerun of the final application or its provider matrix.

Reproduce with the existing pinned native image and Linux example build described
in the fixture guide, building both `agent_tool_plugin` and `integration_hooks`.
Mount that fixture directory at `/qualification`, native gateway directory at
`/fixture` and both compiled examples at `/generated`. In a new disposable
container run `node /qualification/setup-owned.mjs /home/node/evidence`, then
start `node /fixture/gateway.mjs /home/node/evidence /home/node/evidence/plugin-info.json`.
Start ordinary `claude --debug-file /home/node/evidence/native-startup.log` with
the same fixture-key/gateway environment. Follow the guide's normal onboarding
and disconnect external networking before accepting trust. Submit
`AIKIT_NATIVE_BRIDGE_PROBE`, run `verify.mjs`, exit Claude and run
`node /qualification/remove-owned.mjs /home/node/evidence`. Retain named regular
evidence files and network/binary/source identities before removing the container.

The required native caller authentication/generation bridge, Codelaya operation
routing, semantic model behavior, protected completion/deadlines and original-
session feedback remain unfinished. This API promotes no capability. The full
native Windows target, including Cursor/Codex/Pi, remains unqualified.
