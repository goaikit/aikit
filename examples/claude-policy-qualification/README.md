# Native Claude policy diagnostics

These are experiments for **fresh disposable Linux containers**, not installers
for an owner's profile. They reject an existing node Claude profile, require the
recorded native binary checksum, use only synthetic workspace data and a public
dummy key, and grant no readiness. Do not run the preparation against a real home
or install its managed-settings file on the Windows host.

The [admission protocol](protocol.md) tests the existing `plugin.register` API.
The [managed protocol](managed-protocol.md) tests native managed settings, the
built-in guard, and command-hook failure behavior. Retain failed experiments;
never modify live settings or a live responder to make a case pass.

## Reproduction shape

Use pinned Linux native Claude 2.1.308, Node 22, `/home/node/workspace`, UID 1000,
an empty disposable home and `--network none`, without host mounts. The binary
SHA-256 is in both preparers. Copy this directory to `/qualification`.

For admission: run `node /qualification/prepare.mjs healthy` (or `catch`,
`catch-fails`). Launch ordinary interactive Claude in a PTY, accept trust for
the synthetic workspace, wait for startup, and `/exit`. No model prompt is used.
Run `node /qualification/verify-admission.mjs /home/node/evidence` as UID 1000.

For a managed case: run `node /qualification/prepare-managed.mjs CASE`. As root
**inside that disposable container only**, copy the emitted
`/home/node/evidence/managed-settings.json` to
`/etc/claude-code/managed-settings.json`, root-owned, directory 0755/file 0644.
As node, syntax-check and start `gateway-managed.mjs`; require a successful
`GET http://127.0.0.1:8787/__aikit_probe_ready` before launching Claude.

Launch the ordinary terminal CLI with these container-only variables:

```sh
export ANTHROPIC_API_KEY=sk-ant-aikit-local-fixture-not-a-credential
export ANTHROPIC_BASE_URL=http://127.0.0.1:8787
export DISABLE_AUTOUPDATER=1
export CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1
script -q -f -e -c 'claude --debug-file /home/node/evidence/native-debug.log' \
  /home/node/evidence/native-terminal.log
```

Accept trust for the synthetic workspace. Submit one short prompt; tool cases
should ask to read `/home/node/workspace/.aikit-native-policy-probe`. In `block`
and `fallback`, verify the held second main request, press Escape before its
120-second deadline, then `/exit`. Other cases answer normally except
`input-block`, which blocks before any model request; clear the retained input
with Ctrl-C and `/exit`. Run `node /qualification/verify-managed.mjs` as node.

Preserve evidence, source, actual user and root settings, transcript (when one
exists), native debug and terminal exit status before stopping the container.
Exclude the debug `latest` symlink from Windows-bound archives. Native debug's
module `loaded` line occurs before admission: use real refusal/startup evidence,
not that line alone. Model replies here are scripted; no model quality or
real-provider account qualification is claimed.
