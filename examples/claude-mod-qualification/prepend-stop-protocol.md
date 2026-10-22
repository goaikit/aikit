# Native Stop handler ordering candidate

Predeclared 2026-10-22. Ordinary interactive Linux Claude 2.1.296; no production
readiness claim. Use the pinned image and normal onboarding from
[the gateway protocol](native-gateway-probe.md#reproduce), with no owner mounts
or credentials. External networking is disconnected before trust. The existing
bounded gateway supplies scripted replies, not a real model.

`setup-prepend-probe.mjs user` configures a disposable local directory marketplace
and enables the existing Stop catch fixture from user settings, listing it in
`prependPlugins`. Start Claude with only the separate short-circuit competitor
as `--plugin-dir /home/node/competitor`. Submit `AIKIT_PREPEND_STOP_USER` and
exit after native completion. Retain both plugins' events, native transcript,
debug log and settings before changing the case.

Then run `setup-prepend-probe.mjs managed` in the same disposable container. It
clears the disposable user settings and puts the marketplace, enabled plugin
and prepend list in `/etc/claude-code/managed-settings.json`, explicitly listing
the built-in guard second. Root owns `/opt/aikit-prepend-probe` and managed
settings. Start a fresh ordinary session with the same competitor, submit
`AIKIT_PREPEND_STOP_MANAGED`, then retain evidence and exit normally.

Hypothesis: a correctly seated first mod can return its catch Block before the
competitor can skip it, forcing a second model request. Inspect actual loaded
tiers, catch calls, native feedback, outgoing reason and accepted outcome. Do
not infer ordering from the configuration file alone. An ignored list or
unloaded mod falsifies that case's premise; retain it without relabeling.

The [official managed mod guide](https://code.claude.com/docs/en/plugins/mods/admin)
describes user ordering only without managed settings or Team/Enterprise login,
and managed directory plugins as a separate protected configuration. This
experiment changes policy only inside its disposable container; it does not
authorize changing owner managed settings or qualify existing owner policy.
Worker loss, cancellation, identity, configuration attestation, feedback delivery
and native Windows remain independent requirements.

## Observed result

Both cases passed their ordering hypothesis on this executable. Native debug
logs seat `stop-guard@aikit-prepend-probe` at `tier prepend` and the separate
`stop-competitor@inline` at `tier user`. Both sessions recorded a first Stop
throw, catch Block, exact reason in the second outgoing request and durable
native feedback, second Stop, then successful non-aborted completion. Each
session made two main requests; six total requests include two auxiliary calls.

In the unmanaged case the debug log identifies user `prependPlugins`, and the
competitor ran only on the second Stop (`stop_hook_active: true`). In the
managed case the log identifies managed `prependPlugins` and the built-in
guard at position 2. The first mod's catch Block still ran; the guard skipped
the competitor's Stop handler on the later delegated call. The competitor's
session-start and matching completion events prove it was loaded in both cases.

The capture and three negative evidence checks passed on Linux Node 22.23.3 and
retained host Node 20.19.0. The negative checks reject a wrong effective tier,
wrong built-in guard position and erased outgoing Block reason. They are not
three additional native sessions.

```sh
node verify-prepend-stop.mjs /home/node/evidence
node tests/verify-prepend-stop.test.mjs /home/node/evidence
```

This supplies a working conditional ordering candidate, including an unmanaged
local path without introducing administrator policy. It is not permission to
overwrite user ordering or managed settings. Existing managed policy or a
Team/Enterprise login can make the user ordering path inapplicable. No such
login was tested. The SDK must qualify effective session protection, detect
configuration changes and preserve conflicts rather than infer readiness from
an installed file or version. The managed case establishes a separate candidate
only where the owner/admin explicitly configures it.

The next integration step is to reuse the existing AIKit native module and
owned install/update planner for a bounded Stop callback and constant fallback,
keeping Codelaya policy in its current operation service. Before enabling it,
qualify the actual generated callback with failure, cancellation, identity,
repeated decisions and effective-ordering changes. Do not silently switch an
existing command-hook installation, add an activation exception or replace the
owner-started terminal. Native worker loss and feedback guarantees stay open.

Starting AIKit revision: `ddb065628d4f54d8851d4f607bb461337edafb3e`.
Image: `sha256:fe24c3f19fe12410b4d57bb1101def27a05a92fa7083611623157a0a2b433e65`.
Executable: `24972e3bc859fab2b46ed4c1e51f7d6130f06d3bd550811a114640de3370d0de`.
Raw native logs/events/transcripts/settings/requests, predeclared protocol,
source hashes and verification outputs are retained under
`.local/prepend-stop-20261022` in the parent Codelaya chat workspace.
Production adapter, SDK schema/pin, activation and Windows package are unchanged.

| Evidence | SHA-256 |
| --- | --- |
| Requests | `4cd737eb329527e5db791e4facf9594cf2f8f0e585c6ca3a7613593572a6c404` |
| User-order events | `ec5b52f8814c5bcdebe25e4ae30a75dd0b7e57b2823aa3b57d5782020b4ec295` |
| Managed-order events | `d2acc4f4336eb873a6b18161b50b3fc734cfb55b96ffa79c0abadb66c0b2a68c` |
